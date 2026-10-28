//! Mango: self-hosted manga and manhwa reader.
//!
//! This binary is the core. The extension host is a JVM process it starts on
//! demand; see `host.rs`.

mod api;
mod auth;
mod backups;
mod config;
mod db;
mod downloads;
mod error;
mod export;
mod files;
mod host;
mod images;
mod import;
mod kosync;
mod library;
mod local;
mod migrate;
mod net;
mod notify;
mod opds;
mod push;
mod settings;
mod spaces;
mod store;
mod titles;
mod trackers;
mod ui;
mod updates;
mod version;

use std::{
    io::{IsTerminal, Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    process::ExitCode,
    time::Duration,
};

const USAGE: &str = "\
Usage: mango [command]

  (none)           Run the server.
  health           Ask the running server whether it answers. For container health checks.
  address          Print the address a browser on this machine opens.
  reset-password   Remove the first account's password and sign it out. Restart Mango afterwards.
  version          Print the version.

Settings come from mango.toml, or the file MANGO_CONFIG names, and from MANGO_* variables.";

/// In the data folder while Mango runs: what `mango health` tells this Mango by.
const INSTANCE: &str = "instance";
/// In the data folder until the first password is set: the code that setting it takes.
const SETUP_CODE: &str = "setup-code";

/// Has glibc hand big blocks back to the system as soon as they are freed. Left alone,
/// it starts keeping them once it has seen one, a set per thread: a password hash takes
/// 19 MB and a page a few, and an idle Mango would sit on all of them.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn return_big_blocks() {
    extern "C" {
        fn mallopt(param: i32, value: i32) -> i32;
    }
    const M_MMAP_THRESHOLD: i32 = -3;
    // SAFETY: a libc call that changes one allocator setting and touches no memory of ours.
    unsafe {
        mallopt(M_MMAP_THRESHOLD, 128 * 1024);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn return_big_blocks() {}

#[tokio::main]
async fn main() -> ExitCode {
    return_big_blocks();
    let outcome = match std::env::args().nth(1).as_deref() {
        None | Some("serve") => run().await,
        Some("health") => health(),
        Some("address") => here().map(|(addr, _)| println!("http://{addr}")),
        Some("reset-password") => reset_password(),
        Some("version" | "--version" | "-V") => Ok(println!("mango {}", version::describe())),
        Some("help" | "--help" | "-h") => Ok(println!("{USAGE}")),
        Some(other) => Err(format!("unknown command {other:?}\n\n{USAGE}")),
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("mango: {why}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let cfg = config::Config::load()?;
    let level = cfg.log.parse::<tracing::Level>().map_err(|_| format!("unknown log level {:?}", cfg.log))?;
    tracing_subscriber::fmt().with_max_level(level).with_ansi(std::io::stdout().is_terminal()).init();

    std::fs::create_dir_all(&cfg.data_dir).map_err(|e| format!("cannot create {}: {e}", cfg.data_dir.display()))?;
    let db = db::Db::open(&cfg.data_dir.join("mango.db")).map_err(|e| e.to_string())?;
    let front = auth::Front { hosts: cfg.hosts.clone(), proxied: cfg.proxied, signup: cfg.signup, account: cfg.account_url.clone() };
    let auth = auth::Auth::open(db.clone(), cfg.login, cfg.port()?, cfg.first_password.clone(), front).await.map_err(|e| e.to_string())?;
    let private = |name: &str, text: &str| db::write_private(&cfg.data_dir.join(name), text).map_err(|e| format!("cannot write {name} in {}: {e}", cfg.data_dir.display()));
    private(INSTANCE, auth.instance())?;
    match auth.setup_code() {
        Some(code) => private(SETUP_CODE, code)?,
        None => drop(std::fs::remove_file(cfg.data_dir.join(SETUP_CODE))),
    }
    // What sources keep in the host's folder is as private as the databases: their cookies and sign-ins.
    db::owner_only(&cfg.data_dir.join("host"));
    let releases = version::Releases::open(db.clone(), &cfg.releases).await.map_err(|e| e.to_string())?;
    let host = host::Host::new(cfg.host.clone(), cfg.host_dir(), &cfg.data_dir).map_err(|e| e.to_string())?;
    if let Some(solver) = db.call(|conn| Ok(settings::get::<host::Solver>(conn, host::Solver::KEY))).await.map_err(|e| e.to_string())? {
        host.set_solver(solver).await;
    }

    // The local source needs no extension and no host: it is there from the first start.
    let local = local::Local::new(cfg.local_dir());
    std::fs::create_dir_all(local.root()).map_err(|e| format!("cannot create {}: {e}", local.root().display()))?;

    tokio::spawn(host.clone().monitor());
    tokio::spawn(releases.clone().check_on_schedule());

    let listener =
        tokio::net::TcpListener::bind(&cfg.listen).await.map_err(|e| format!("cannot listen on {}: {e}", cfg.listen))?;
    tracing::info!("Mango {} on http://{}, data in {}", version::describe(), cfg.listen, cfg.data_dir.display());
    if !cfg.login {
        tracing::warn!("login is switched off: anyone who can reach this address can use Mango");
    } else if let Some(code) = auth.setup_code() {
        let kept = cfg.data_dir.join(SETUP_CODE);
        tracing::info!("no password yet: open the page and set one. From another machine that takes the setup code {code}, also kept in {}", kept.display());
    }

    // Every account gets its library; the owner's is the data directory itself.
    let shared = spaces::Shared {
        db,
        releases,
        host: host.clone(),
        local,
        data_dir: cfg.data_dir.clone(),
        cache_mb: cfg.cache_mb,
        stores_refresh_hours: cfg.stores.refresh_hours,
        store_rules: store::Rules { local: cfg.stores.local, anyone_installs: cfg.stores.anyone_installs },
        blocklist: std::sync::Arc::new(store::blocklist::Blocklist::new(cfg.stores.blocklist.clone())),
        keep_free_mb: cfg.keep_free_mb,
        hold_file: cfg.hold_file.clone(),
    };
    let state = spaces::Spaces::new(auth, shared).map_err(|e| e.to_string())?;
    // The login counts wrong passwords per address, so the server hands each call its caller's.
    let app = api::router(state).into_make_service_with_connect_info::<SocketAddr>();
    let served = axum::serve(listener, app).with_graceful_shutdown(shutdown()).await;
    host.stop().await;
    let _ = std::fs::remove_file(cfg.data_dir.join(INSTANCE));
    served.map_err(|e| e.to_string())
}

/// Where the server set up by these settings is reached from this machine, and the settings.
fn here() -> Result<(SocketAddr, config::Config), String> {
    let cfg = config::Config::load()?;
    let mut addr = cfg.listen.to_socket_addrs().ok().and_then(|mut found| found.next()).ok_or_else(|| format!("cannot read {:?} as an address", cfg.listen))?;
    // A server on every address is asked on this machine's own.
    if addr.ip().is_unspecified() {
        addr.set_ip(if addr.is_ipv4() { [127, 0, 0, 1].into() } else { std::net::Ipv6Addr::LOCALHOST.into() });
    }
    Ok((addr, cfg))
}

/// Succeeds when the server set up by the same settings answers on its port, and shows
/// that it is the Mango that runs on this data folder: something else may have taken
/// the port, and the desktop shell opens whatever this command vouches for.
fn health() -> Result<(), String> {
    let (addr, cfg) = here()?;
    let mark = cfg.data_dir.join(INSTANCE);
    let instance = std::fs::read_to_string(&mark).map_err(|e| format!("no running Mango left its mark at {}: {e}", mark.display()))?;
    let mut nonce = [0u8; 16];
    getrandom::getrandom(&mut nonce).map_err(|e| format!("no random bytes: {e}"))?;
    let challenge = hex::encode(nonce);

    let wait = Some(Duration::from_secs(3));
    let asked = (|| {
        let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(3))?;
        stream.set_read_timeout(wait)?;
        stream.set_write_timeout(wait)?;
        stream.write_all(format!("GET /api/auth?challenge={challenge} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").as_bytes())?;
        let mut answer = Vec::new();
        // The answer is short. An error after it has come is the connection closing.
        match stream.take(64 << 10).read_to_end(&mut answer) {
            Err(why) if answer.is_empty() => Err(why),
            _ => Ok(String::from_utf8_lossy(&answer).into_owned()),
        }
    })();
    match asked {
        Ok(answer) if !answer.starts_with("HTTP/1.1 200") => Err(format!("{addr} answered {:?}", answer.lines().next().unwrap_or_default())),
        Ok(answer) if !answer.contains(&auth::answer_to(instance.trim(), &challenge)) => {
            Err(format!("{addr} answers, but it is not the Mango that runs on {}", cfg.data_dir.display()))
        }
        Ok(_) => Ok(()),
        Err(why) => Err(format!("{addr} did not answer: {why}")),
    }
}

fn reset_password() -> Result<(), String> {
    let cfg = config::Config::load()?;
    let path = cfg.data_dir.join("mango.db");
    if !path.exists() {
        return Err(format!("there is no database at {}", path.display()));
    }
    let mut conn = rusqlite::Connection::open(&path).map_err(|e| e.to_string())?;
    conn.busy_timeout(Duration::from_secs(10)).map_err(|e| e.to_string())?;
    // A database from before there were accounts learns of them first.
    db::migrate(&mut conn).map_err(|e| e.to_string())?;
    auth::reset(&conn).map_err(|e| e.to_string())?;
    println!("The first account's password is removed and it is signed out.\nRestart Mango, then open it to set a new one.");
    Ok(())
}

/// Resolves on Ctrl-C, and on SIGTERM where there is one.
async fn shutdown() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    // A desktop shell holds a pipe to Mango's input. When it lets go, or dies, Mango stops.
    let let_go = async {
        if std::env::var_os("MANGO_WATCH_STDIN").is_none() {
            return std::future::pending().await;
        }
        let (closed, wait) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let mut sink = [0u8; 64];
            while matches!(std::io::stdin().read(&mut sink), Ok(read) if read > 0) {}
            let _ = closed.send(());
        });
        let _ = wait.await;
    };

    tokio::select! {
        _ = interrupt => {}
        _ = terminate => {}
        _ = let_go => {}
    }
}
