//! Supervisor for the extension host, a JVM child process.
//!
//! The host is started on the first call, pinged while it runs, restarted with a
//! backoff when it fails, and stopped once it has been idle. Every call carries a
//! time limit, and calls to one source are capped.

use crate::{
    config::HostConfig,
    error::{AppError, Result},
};
use reqwest::Method;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex as StdMutex,
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::{Child, ChildStdin, Command},
    sync::{oneshot, watch, Mutex, OwnedSemaphorePermit, Semaphore},
};

const PROTOCOL: u32 = 6;
/// Cap on one image from a source.
const MAX_IMAGE: usize = 64 << 20;
const PING_EVERY: Duration = Duration::from_secs(15);
const PING_TIMEOUT: Duration = Duration::from_secs(5);
const FIRST_PING: Duration = Duration::from_secs(3);
/// The host exits by itself when nothing has called it for this long. Pings keep
/// it alive while the core runs, so this only fires when the core is gone.
const ORPHAN_SECS: u64 = 60;
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// A host that dies sooner than this after starting counts as a failed start.
const SETTLED: Duration = Duration::from_secs(10);
/// A call waits this long for a backoff to pass before it gives up.
const PATIENCE: Duration = Duration::from_secs(2);
/// How long a host gets to exit after being asked, before it is killed.
const EXIT_GRACE: Duration = Duration::from_secs(5);

struct Running {
    child: Child,
    /// The host exits when this closes. See `shut`.
    stdin: Option<ChildStdin>,
    port: u16,
    token: String,
    since: Instant,
}

struct Slot {
    running: Option<Running>,
    failures: u32,
    retry_at: Option<Instant>,
    last_ping: Instant,
    missed_pings: u32,
    health: Option<Value>,
}

/// Options for one call.
#[derive(Default)]
pub struct Call {
    /// Overrides the configured call limit.
    pub timeout: Option<Duration>,
    /// The source the call goes to, so the per-source cap applies.
    pub source: Option<i64>,
}

pub struct Host {
    cfg: HostConfig,
    dir: PathBuf,
    data: PathBuf,
    http: reqwest::Client,
    slot: Mutex<Slot>,
    in_flight: AtomicUsize,
    last_used: StdMutex<Instant>,
    caps: StdMutex<HashMap<i64, Arc<Semaphore>>>,
    starts: watch::Sender<u64>,
    /// Where Cloudflare's checks are sent to be passed; see `set_solver`.
    solver: StdMutex<Solver>,
    /// Whether sources get a web view, and the program that lends Java a display for it.
    web_view: (bool, Option<PathBuf>),
}

/// Whether sources can be given a web view here. Its browser hangs without a display,
/// so on a Linux machine that has none it needs `xvfb-run` to lend one, and stays off
/// without. The runtime has no web view for macOS.
fn web_view(wanted: bool) -> (bool, Option<PathBuf>) {
    if !wanted || cfg!(target_os = "macos") {
        return (false, None);
    }
    if !cfg!(target_os = "linux") || std::env::var_os("DISPLAY").is_some_and(|display| !display.is_empty()) {
        return (true, None);
    }
    let lender = std::env::var_os("PATH").and_then(|path| std::env::split_paths(&path).map(|dir| dir.join("xvfb-run")).find(|file| file.is_file()));
    if lender.is_none() {
        tracing::info!("no display and no xvfb-run here: sources that open a web view will not work");
    }
    (lender.is_some(), lender)
}

/// A FlareSolverr, or anything that answers like one: a browser the sources hand a
/// Cloudflare check to. Mango does not run one; it is told where one is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Solver {
    /// Its address, such as `http://localhost:8191`. Empty: there is none.
    pub url: String,
    /// How long it gets for one check.
    pub timeout_secs: u64,
}

impl Default for Solver {
    fn default() -> Self {
        Self { url: String::new(), timeout_secs: 60 }
    }
}

impl Solver {
    /// The setting it is kept under.
    pub const KEY: &'static str = "solver";

    /// The same, tidied, or an error when it could not work.
    pub fn checked(mut self) -> Result<Self> {
        self.url = self.url.trim().trim_end_matches('/').to_owned();
        if !self.url.is_empty() && !(self.url.starts_with("http://") || self.url.starts_with("https://")) {
            return Err(AppError::bad_request("The solver's address starts with http:// or https://"));
        }
        self.timeout_secs = self.timeout_secs.clamp(5, 300);
        Ok(self)
    }
}

/// Marks a call in progress, so the idle check leaves the host alone.
struct Busy<'a>(&'a Host);

impl<'a> Busy<'a> {
    fn new(host: &'a Host) -> Self {
        host.in_flight.fetch_add(1, Ordering::SeqCst);
        host.touch();
        Busy(host)
    }
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.touch();
        self.0.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Host {
    pub fn new(cfg: HostConfig, dir: PathBuf, data_dir: &Path) -> Result<Arc<Self>> {
        // Loopback only: a proxy from the environment must never see these calls.
        let http = reqwest::Client::builder().no_proxy().build().map_err(|e| AppError::internal(e.to_string()))?;
        Ok(Arc::new(Self {
            dir,
            data: data_dir.join("host"),
            http,
            slot: Mutex::new(Slot {
                running: None,
                failures: 0,
                retry_at: None,
                last_ping: Instant::now(),
                missed_pings: 0,
                health: None,
            }),
            in_flight: AtomicUsize::new(0),
            last_used: StdMutex::new(Instant::now()),
            caps: StdMutex::new(HashMap::new()),
            starts: watch::channel(0).0,
            solver: StdMutex::default(),
            web_view: web_view(cfg.webview),
            cfg,
        }))
    }

    pub fn solver(&self) -> Solver {
        self.solver.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The host reads the solver when it starts, so a running one is stopped: the next call brings up one that knows.
    pub async fn set_solver(&self, solver: Solver) {
        let changed = std::mem::replace(&mut *self.solver.lock().unwrap_or_else(|e| e.into_inner()), solver.clone()) != solver;
        if changed {
            self.stop().await;
        }
    }

    /// Counts host starts. Changes each time a fresh host comes up.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.starts.subscribe()
    }

    /// Sends one request to the host, starting it first when it is asleep, and reads its JSON answer.
    pub async fn call<T: DeserializeOwned>(&self, method: Method, path: &str, body: Option<&Value>, opts: Call) -> Result<T> {
        let _held = self.enter(opts.source).await?;
        let response = self.exchange(method, path, body, opts.timeout).await?;
        response.json().await.map_err(|e| AppError::new(502, "host_error", format!("Unreadable answer from the extension host: {e}")))
    }

    /// Like `call`, for the endpoint that answers with an image.
    pub async fn bytes(&self, path: &str, body: &Value, opts: Call) -> Result<Vec<u8>> {
        let _held = self.enter(opts.source).await?;
        let mut response = self.exchange(Method::POST, path, Some(body), opts.timeout).await?;
        let mut bytes = Vec::new();
        while let Some(chunk) =
            response.chunk().await.map_err(|e| AppError::new(502, "source_error", format!("The image broke off: {e}")))?
        {
            if bytes.len() + chunk.len() > MAX_IMAGE {
                return Err(AppError::new(502, "source_error", "The image is too large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    /// Takes a place under the source's cap and marks the host as in use, for as long as the result lives.
    async fn enter(&self, source: Option<i64>) -> Result<(Option<OwnedSemaphorePermit>, Busy<'_>)> {
        let permit = match source {
            Some(id) => Some(self.cap(id).acquire_owned().await.map_err(|_| AppError::internal("source limiter closed"))?),
            None => None,
        };
        Ok((permit, Busy::new(self)))
    }

    /// One request and its answer's head. An answer that is not a success comes back as the error it describes.
    async fn exchange(&self, method: Method, path: &str, body: Option<&Value>, timeout: Option<Duration>) -> Result<reqwest::Response> {
        let timeout = timeout.unwrap_or(Duration::from_secs(self.cfg.call_timeout_secs));
        let mut retried = false;
        loop {
            let (port, token) = self.ensure().await?;
            let mut request =
                self.http.request(method.clone(), format!("http://127.0.0.1:{port}{path}")).bearer_auth(&token).timeout(timeout);
            if let Some(body) = body {
                request = request.json(body);
            }
            match request.send().await {
                Ok(response) if response.status().is_success() => return Ok(response),
                Ok(response) => return Err(host_error(response).await),
                // Nothing was listening, so the request never ran: safe to start over once.
                Err(e) if e.is_connect() && !retried => {
                    retried = true;
                    self.discard(port).await;
                }
                Err(e) if e.is_timeout() => {
                    return Err(AppError::new(504, "host_timeout", "The extension host did not answer in time"));
                }
                Err(e) => return Err(AppError::new(502, "host_error", format!("Could not reach the extension host: {e}"))),
            }
        }
    }

    /// For the health endpoint. Never waits for a start in progress.
    pub fn status(&self) -> Value {
        let starts = *self.starts.borrow();
        let Ok(slot) = self.slot.try_lock() else {
            return json!({ "state": "starting", "starts": starts });
        };
        let state = match &slot.running {
            Some(_) => "running",
            None if self.jars().is_none() => "missing",
            None if slot.retry_at.is_some_and(|at| at > Instant::now()) => "backoff",
            None => "asleep",
        };
        json!({
            "state": state,
            "starts": starts,
            "upSeconds": slot.running.as_ref().map(|r| r.since.elapsed().as_secs()),
            "health": slot.health,
        })
    }

    pub async fn stop(&self) {
        let mut slot = self.slot.lock().await;
        if let Some(running) = slot.running.take() {
            shut(running).await;
        }
        slot.health = None;
    }

    /// Runs for the life of the process: stops an idle host and replaces one that hangs.
    pub async fn monitor(self: Arc<Self>) {
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        loop {
            tick.tick().await;
            let (port, token) = {
                let mut slot = self.slot.lock().await;
                let Some(running) = slot.running.as_mut() else { continue };
                let alive = matches!(running.child.try_wait(), Ok(None));
                let settled = running.since.elapsed() >= SETTLED;
                if !alive {
                    tracing::warn!("the extension host exited on its own");
                    slot.running = None;
                    slot.health = None;
                    continue;
                }
                if settled {
                    slot.failures = 0;
                }
                let idle = self.last_used.lock().unwrap_or_else(|e| e.into_inner()).elapsed();
                let limit = Duration::from_secs(self.cfg.idle_secs);
                if self.cfg.idle_secs > 0 && idle >= limit && self.in_flight.load(Ordering::SeqCst) == 0 {
                    if let Some(running) = slot.running.take() {
                        shut(running).await;
                    }
                    slot.health = None;
                    tracing::info!("extension host stopped after {} s idle", idle.as_secs());
                    continue;
                }
                if slot.last_ping.elapsed() < PING_EVERY {
                    continue;
                }
                slot.last_ping = Instant::now();
                let Some(running) = slot.running.as_ref() else { continue };
                (running.port, running.token.clone())
            };

            let answer = self.http.get(format!("http://127.0.0.1:{port}/health")).bearer_auth(&token).timeout(PING_TIMEOUT).send().await;
            let health = match answer {
                Ok(response) if response.status().is_success() => response.json::<Value>().await.ok(),
                _ => None,
            };

            let mut slot = self.slot.lock().await;
            if !slot.running.as_ref().is_some_and(|r| r.port == port) {
                continue;
            }
            match health {
                Some(health) => {
                    slot.missed_pings = 0;
                    slot.health = Some(health);
                }
                None => {
                    slot.missed_pings += 1;
                    if slot.missed_pings >= 2 {
                        tracing::warn!("the extension host stopped answering; it will be restarted on the next call");
                        if let Some(running) = slot.running.take() {
                            shut(running).await;
                        }
                        slot.health = None;
                    }
                }
            }
        }
    }

    fn touch(&self) {
        *self.last_used.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
    }

    fn cap(&self, source: i64) -> Arc<Semaphore> {
        let mut caps = self.caps.lock().unwrap_or_else(|e| e.into_inner());
        caps.entry(source).or_insert_with(|| Arc::new(Semaphore::new(self.cfg.per_source.max(1)))).clone()
    }

    fn jars(&self) -> Option<(PathBuf, PathBuf)> {
        let (host, runtime) = (self.dir.join("mango-host.jar"), self.dir.join("runtime.jar"));
        (host.is_file() && runtime.is_file()).then_some((host, runtime))
    }

    /// Port and token of a live host, starting one when needed.
    async fn ensure(&self) -> Result<(u16, String)> {
        let mut slot = self.slot.lock().await;
        if let Some(running) = slot.running.as_mut() {
            if matches!(running.child.try_wait(), Ok(None)) {
                return Ok((running.port, running.token.clone()));
            }
            let died_young = running.since.elapsed() < SETTLED;
            slot.running = None;
            slot.health = None;
            tracing::warn!("the extension host exited; starting it again");
            if died_young {
                slot.failures += 1;
                slot.retry_at = Some(Instant::now() + backoff(slot.failures));
            }
        }
        // One crash is waited out here, so the caller only sees a slower answer. A host
        // that keeps failing is not hammered: callers are told when to come back.
        if let Some(wait) = slot.retry_at.and_then(|at| at.checked_duration_since(Instant::now())) {
            if wait > PATIENCE {
                return Err(AppError::new(
                    503,
                    "host_unavailable",
                    format!("The extension host keeps failing. Trying again in {} s", wait.as_secs() + 1),
                ));
            }
            tokio::time::sleep(wait).await;
        }
        match self.start().await {
            Ok(running) => {
                let live = (running.port, running.token.clone());
                slot.running = Some(running);
                // `failures` is cleared by the monitor once this host has stayed up: one that
                // starts fine and dies seconds later, over and over, must still back off.
                slot.retry_at = None;
                // The first ping comes early, so the health endpoint soon has the host's numbers.
                slot.last_ping = Instant::now().checked_sub(PING_EVERY - FIRST_PING).unwrap_or_else(Instant::now);
                slot.missed_pings = 0;
                self.starts.send_modify(|count| *count += 1);
                Ok(live)
            }
            Err(e) => {
                slot.failures += 1;
                slot.retry_at = Some(Instant::now() + backoff(slot.failures));
                Err(e)
            }
        }
    }

    async fn start(&self) -> Result<Running> {
        let failed = |why: String| AppError::new(503, "host_unavailable", why);
        let (host_jar, runtime_jar) = self.jars().ok_or_else(|| {
            AppError::new(503, "host_missing", format!("The extension host is not installed (looked in {})", self.dir.display()))
        })?;
        std::fs::create_dir_all(&self.data)?;
        let token = new_token()?;
        let class_path = std::env::join_paths([&host_jar, &runtime_jar]).map_err(|e| failed(e.to_string()))?;

        let started = Instant::now();
        let solver = self.solver();
        let (web_view, lender) = &self.web_view;
        let mut command = match lender {
            Some(lender) => {
                let mut command = Command::new(lender);
                command.arg("-a").arg(&self.cfg.java);
                command
            }
            None => Command::new(&self.cfg.java),
        };
        // The host, the browser it may start and a lent display end together; see `end`.
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .args(&self.cfg.jvm_args)
            // The classes a run loaded are kept in an archive beside the host's data when it
            // stops, and mapped in at the next start: a host woken after idling is ready sooner.
            // Java checks the archive against the jars and makes it anew when they changed.
            .arg("-XX:+IgnoreUnrecognizedVMOptions")
            .arg("-XX:+AutoCreateSharedArchive")
            .arg(format!("-XX:SharedArchiveFile={}", self.data.join("host.jsa").display()))
            // It says at length which classes it leaves out, such as an extension's, which are signed.
            .arg("-Xlog:cds=off")
            .arg("-Xlog:cds+dynamic=off")
            // Without the web view Java needs no display at all.
            .args((!*web_view).then_some("-Djava.awt.headless=true"))
            .arg(format!("-Duser.home={}", self.data.display()))
            .arg("-cp")
            .arg(class_path)
            .arg("mango.host.MainKt")
            .env("MANGO_HOST_TOKEN", &token)
            .env("MANGO_HOST_PORT", "0")
            .env("MANGO_HOST_DATA", &self.data)
            .env("MANGO_HOST_IDLE_SECONDS", ORPHAN_SECS.to_string())
            .env("MANGO_HOST_CALL_TIMEOUT_SECONDS", self.cfg.source_timeout_secs.to_string())
            .env("MANGO_HOST_WATCH_STDIN", "1")
            .env("MANGO_HOST_WEBVIEW", if *web_view { "1" } else { "0" })
            // The browser looks for a desktop's message bus, and says so at length when there is none.
            .env("DBUS_SESSION_BUS_ADDRESS", std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap_or_else(|_| "disabled:".into()))
            .env("MANGO_HOST_SOLVER_URL", &solver.url)
            .env("MANGO_HOST_SOLVER_TIMEOUT_SECONDS", solver.timeout_secs.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| failed(format!("Could not run Java ({}): {e}", self.cfg.java)))?;

        // Both pipes are read for as long as the host lives, or it would block on a full one.
        let (ready, port) = oneshot::channel();
        if let Some(out) = child.stdout.take() {
            let mut ready = Some(ready);
            tokio::spawn(lines(out, move |line| match parse_ready(line) {
                Some(answer) => {
                    if let Some(ready) = ready.take() {
                        let _ = ready.send(answer);
                    }
                }
                None if noise(line) => tracing::debug!(target: "host", "{line}"),
                None => tracing::info!(target: "host", "{line}"),
            }));
        }
        if let Some(err) = child.stderr.take() {
            tokio::spawn(lines(err, |line| match noise(line) {
                true => tracing::debug!(target: "host", "{line}"),
                false => tracing::warn!(target: "host", "{line}"),
            }));
        }

        let limit = Duration::from_secs(self.cfg.start_timeout_secs);
        let port = match tokio::time::timeout(limit, port).await {
            Ok(Ok(Ok(port))) => port,
            Ok(Ok(Err(why))) => {
                let _ = child.kill().await;
                return Err(failed(why));
            }
            Ok(Err(_)) => {
                let _ = child.kill().await;
                return Err(failed("The extension host exited while starting; see the log".into()));
            }
            Err(_) => {
                let _ = child.kill().await;
                return Err(failed(format!("The extension host was not ready after {} s", limit.as_secs())));
            }
        };
        tracing::info!("extension host ready in {} ms", started.elapsed().as_millis());
        Ok(Running { stdin: child.stdin.take(), child, port, token, since: Instant::now() })
    }

    /// Drops a host that refused a connection, so the next `ensure` starts a new one.
    async fn discard(&self, port: u16) {
        let mut slot = self.slot.lock().await;
        if slot.running.as_ref().is_some_and(|r| r.port == port) {
            if let Some(running) = slot.running.take() {
                shut(running).await;
            }
            slot.health = None;
        }
    }
}

/// Stops a host: closes its stdin, which it takes as the order to exit, and kills it
/// if it has not gone after a grace period. The pipe matters where a kill falls
/// short: when `java` is a launcher, the kill ends the launcher and the JVM lives on,
/// holding the extension files open.
async fn shut(mut running: Running) {
    drop(running.stdin.take());
    if tokio::time::timeout(EXIT_GRACE, running.child.wait()).await.is_err() {
        tracing::warn!("the extension host did not exit when asked; killing it");
        end(&running.child);
        let _ = running.child.kill().await;
    }
}

/// Kills everything the host started along with it: a web view's browser, a lent display.
/// They share the process group the host was given.
#[cfg(unix)]
fn end(child: &Child) {
    extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    const SIGKILL: i32 = 9;
    if let Some(pid) = child.id() {
        // SAFETY: a libc call that takes two numbers and touches no memory of ours.
        unsafe {
            kill(-(pid as i32), SIGKILL);
        }
    }
}

#[cfg(not(unix))]
fn end(_: &Child) {}

/// What the web view's browser writes about the desktop it does not find. Not worth a warning.
fn noise(line: &str) -> bool {
    line.starts_with('[') && line.contains(":ERROR:dbus/")
}

/// The error a failed host answer describes.
async fn host_error(response: reqwest::Response) -> AppError {
    let status = response.status().as_u16();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    let code = body["error"]["code"].as_str().unwrap_or("host_error");
    let message = body["error"]["message"].as_str().unwrap_or("The extension host reported an error");
    // A 401 or 500 from the host is the core's fault or the host's bug, never the caller's.
    let status = if matches!(status, 401 | 500) { 502 } else { status };
    AppError::new(status, code, message)
}

fn backoff(failures: u32) -> Duration {
    (Duration::from_millis(500) * 2u32.saturating_pow(failures.saturating_sub(1).min(8))).min(MAX_BACKOFF)
}

fn new_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|e| AppError::internal(format!("no random numbers: {e}")))?;
    Ok(hex::encode(bytes))
}

/// The line the host prints once it listens: `MANGO_HOST_READY port=<n> protocol=<n>`.
fn parse_ready(line: &str) -> Option<std::result::Result<u16, String>> {
    let rest = line.strip_prefix("MANGO_HOST_READY ")?;
    let field = |name: &str| rest.split_whitespace().find_map(|part| part.strip_prefix(name)?.parse::<u32>().ok());
    Some(match (field("port="), field("protocol=")) {
        (Some(port), Some(PROTOCOL)) if (1..=65535).contains(&port) => Ok(port as u16),
        (_, Some(other)) if other != PROTOCOL => {
            Err(format!("The extension host speaks protocol {other}; this Mango needs protocol {PROTOCOL}"))
        }
        _ => Err("The extension host sent an unreadable ready line".into()),
    })
}

/// Feeds a pipe to `each` line by line until it closes. Bytes that are not UTF-8 are replaced, not fatal.
async fn lines<R: AsyncRead + Unpin>(stream: R, mut each: impl FnMut(&str)) {
    let mut reader = BufReader::new(stream);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => each(String::from_utf8_lossy(&line).trim_end()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ready_line() {
        assert_eq!(parse_ready("MANGO_HOST_READY port=41233 protocol=6"), Some(Ok(41233)));
        assert!(matches!(parse_ready("MANGO_HOST_READY port=41233 protocol=1"), Some(Err(why)) if why.contains("protocol 1")));
        assert!(matches!(parse_ready("MANGO_HOST_READY port=0 protocol=6"), Some(Err(_))));
        assert_eq!(parse_ready("some log line"), None);
    }

    #[test]
    fn backoff_grows_and_stops() {
        assert_eq!(backoff(1), Duration::from_millis(500));
        assert_eq!(backoff(3), Duration::from_secs(2));
        assert_eq!(backoff(40), MAX_BACKOFF);
    }
}
