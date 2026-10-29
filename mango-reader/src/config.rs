//! Settings: defaults, then a TOML file, then environment variables.
//!
//! The file is `$MANGO_CONFIG`, or `mango.toml` in the working directory when it exists.

use serde::Deserialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Address the web server binds to. `MANGO_LISTEN`
    pub listen: String,
    /// The one directory Mango writes to. `MANGO_DATA`
    pub data_dir: PathBuf,
    /// error, warn, info, debug or trace. `MANGO_LOG`
    pub log: String,
    /// Cap on the page and cover cache, in megabytes. `MANGO_CACHE_MB`
    pub cache_mb: u64,
    /// Downloads stop while the disk has less than this free, in megabytes. 0 never stops them. `MANGO_KEEP_FREE_MB`
    pub keep_free_mb: u64,
    /// The download queue waits while this file has a line that reads `full`. For whoever runs
    /// Mango for others and keeps each reader's storage in bounds. Unset: never. `MANGO_HOLD_FILE`
    pub hold_file: Option<PathBuf>,
    /// The folder the local source reads: `<title>/<chapter>` inside it. Default: `local` in the data directory. `MANGO_LOCAL`
    pub local_dir: Option<PathBuf>,
    /// Ask for a password. Off is for a network where everyone may use Mango. `MANGO_LOGIN`
    pub login: bool,
    /// Sets the password on a start that finds none. Environment only: `MANGO_PASSWORD`
    #[serde(skip)]
    pub first_password: Option<String>,
    /// Names Mango goes by besides addresses and names only a local network knows. They
    /// matter while a request can act without signing in. `MANGO_HOSTS`, comma separated
    pub hosts: Vec<String>,
    /// A proxy stands in front of Mango and writes each caller's address into
    /// `X-Forwarded-For`. Wrong passwords are then counted by that address. `MANGO_PROXIED`
    pub proxied: bool,
    /// Let whoever holds an admin's invitation make an account, and show the main page
    /// to a browser that is not signed in. `MANGO_SIGNUP`
    pub signup: bool,
    /// Where whoever runs this Mango for others keeps each reader's account: their plan,
    /// billing and the like. Shown as a link in Settings and the tour. Unset, nothing is
    /// shown. `MANGO_ACCOUNT_URL`
    pub account_url: Option<String>,
    pub host: HostConfig,
    pub stores: StoresConfig,
    pub releases: ReleasesConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReleasesConfig {
    /// `owner/name` of the repository whose releases are Mango's. Default: the one the build came from. `MANGO_RELEASES_REPO`
    pub repo: Option<String>,
    /// For a repository that is not public: a token that may read it. `MANGO_RELEASES_TOKEN`
    pub token: Option<String>,
    /// Where the release list is asked. Another address is for a self-hosted forge. `MANGO_RELEASES_API`
    pub api: String,
    /// How often to look for a newer Mango. 0 leaves it to the owner. `MANGO_RELEASES_CHECK_HOURS`
    pub check_hours: u64,
}

/// Who sets the proxy the sources reach sites through.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxySetting {
    #[default]
    Admin,
    Anyone,
    Off,
}

impl ProxySetting {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Admin => "admin",
            Self::Anyone => "anyone",
            Self::Off => "off",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HostConfig {
    /// Folder holding `mango-host.jar` and `runtime.jar`. Default: `host` beside the binary. `MANGO_HOST_DIR`
    pub dir: Option<PathBuf>,
    /// The Java launcher. `MANGO_JAVA`
    pub java: String,
    pub jvm_args: Vec<String>,
    /// Stop the host after this long without a call. 0 keeps it running. `MANGO_HOST_IDLE_SECS`
    pub idle_secs: u64,
    pub start_timeout_secs: u64,
    /// Limit for one call from the core to the host.
    pub call_timeout_secs: u64,
    /// Limit the host gives a source to answer. Shorter than the call limit so the host reports it.
    pub source_timeout_secs: u64,
    /// Calls allowed at once to a single source.
    pub per_source: usize,
    /// Who sets the proxy the sources reach sites through: `admin`; `anyone`, every account,
    /// for whoever runs a Mango for each reader; or `off`, no one, and one set before is not
    /// used. `MANGO_PROXY_SETTING`
    pub proxy_setting: ProxySetting,
    /// Let sources open a web view: a Chromium the host downloads once and runs without a
    /// window. On Linux it needs a display and the libraries Chromium links. `MANGO_WEBVIEW`
    pub webview: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StoresConfig {
    /// How often stores are checked for new versions. 0 turns the check off. `MANGO_STORES_REFRESH_HOURS`
    pub refresh_hours: u64,
    /// Let stores be on this machine or its network, and be fetched over plain HTTP. `MANGO_STORES_LOCAL`
    pub local: bool,
    /// Let every account bring extensions into the host, not only an admin. `MANGO_ANYONE_INSTALLS`
    pub anyone_installs: bool,
    /// A file of extensions and stores no account may have, for whoever runs Mango for
    /// others. See `store/blocklist.rs`. `MANGO_BLOCKLIST`
    pub blocklist: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8787".into(),
            data_dir: "data".into(),
            log: "info".into(),
            cache_mb: 1024,
            keep_free_mb: 0,
            hold_file: None,
            local_dir: None,
            login: true,
            first_password: None,
            hosts: Vec::new(),
            proxied: false,
            signup: false,
            account_url: None,
            host: HostConfig::default(),
            stores: StoresConfig::default(),
            releases: ReleasesConfig::default(),
        }
    }
}

impl Default for ReleasesConfig {
    fn default() -> Self {
        Self { repo: None, token: None, api: "https://api.github.com".into(), check_hours: 24 }
    }
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            dir: None,
            java: "java".into(),
            jvm_args: ["-XX:+UseSerialGC", "-Xmx256m", "-Xss512k", "-XX:TieredStopAtLevel=1"].map(String::from).to_vec(),
            idle_secs: 300,
            start_timeout_secs: 30,
            call_timeout_secs: 90,
            source_timeout_secs: 60,
            per_source: 4,
            proxy_setting: ProxySetting::Admin,
            webview: true,
        }
    }
}

impl Default for StoresConfig {
    fn default() -> Self {
        Self { refresh_hours: 12, local: false, anyone_installs: false, blocklist: None }
    }
}

impl Config {
    pub fn load() -> Result<Self, String> {
        let file = match env::var_os("MANGO_CONFIG") {
            Some(path) => Some(PathBuf::from(path)),
            None => Some(PathBuf::from("mango.toml")).filter(|p| p.exists()),
        };
        let mut cfg = match file {
            Some(path) => {
                let text = fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
                toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?
            }
            None => Config::default(),
        };
        cfg.apply_env(|name| env::var(name).ok())?;
        cfg.data_dir = std::path::absolute(&cfg.data_dir).map_err(|e| format!("bad data directory: {e}"))?;
        Ok(cfg)
    }

    fn apply_env(&mut self, var: impl Fn(&str) -> Option<String>) -> Result<(), String> {
        let number = |name: &str| match var(name) {
            Some(value) => value.trim().parse::<u64>().map(Some).map_err(|_| format!("{name} must be a whole number")),
            None => Ok(None),
        };
        if let Some(v) = var("MANGO_LISTEN") {
            self.listen = v;
        }
        if let Some(v) = var("MANGO_DATA") {
            self.data_dir = v.into();
        }
        if let Some(v) = var("MANGO_LOG") {
            self.log = v;
        }
        if let Some(v) = var("MANGO_LOCAL") {
            self.local_dir = Some(v.into());
        }
        if let Some(v) = var("MANGO_JAVA") {
            self.host.java = v;
        }
        if let Some(v) = var("MANGO_HOST_DIR") {
            self.host.dir = Some(v.into());
        }
        if let Some(v) = number("MANGO_CACHE_MB")? {
            self.cache_mb = v;
        }
        if let Some(v) = number("MANGO_KEEP_FREE_MB")? {
            self.keep_free_mb = v;
        }
        if let Some(v) = var("MANGO_HOLD_FILE").filter(|v| !v.trim().is_empty()) {
            self.hold_file = Some(v.trim().into());
        }
        if let Some(v) = var("MANGO_BLOCKLIST").filter(|v| !v.trim().is_empty()) {
            self.stores.blocklist = Some(v.into());
        }
        if let Some(v) = number("MANGO_HOST_IDLE_SECS")? {
            self.host.idle_secs = v;
        }
        if let Some(v) = number("MANGO_STORES_REFRESH_HOURS")? {
            self.stores.refresh_hours = v;
        }
        let switch = |name: &str| match var(name).map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("on" | "true" | "yes" | "1") => Ok(Some(true)),
            Some("off" | "false" | "no" | "0") => Ok(Some(false)),
            Some(_) => Err(format!("{name} must be on or off")),
            None => Ok(None),
        };
        if let Some(v) = switch("MANGO_LOGIN")? {
            self.login = v;
        }
        if let Some(v) = switch("MANGO_WEBVIEW")? {
            self.host.webview = v;
        }
        if let Some(v) = switch("MANGO_PROXIED")? {
            self.proxied = v;
        }
        if let Some(v) = switch("MANGO_SIGNUP")? {
            self.signup = v;
        }
        if let Some(v) = switch("MANGO_STORES_LOCAL")? {
            self.stores.local = v;
        }
        if let Some(v) = switch("MANGO_ANYONE_INSTALLS")? {
            self.stores.anyone_installs = v;
        }
        if let Some(v) = var("MANGO_PROXY_SETTING") {
            self.host.proxy_setting = match v.trim().to_ascii_lowercase().as_str() {
                "admin" => ProxySetting::Admin,
                "anyone" => ProxySetting::Anyone,
                "off" => ProxySetting::Off,
                _ => return Err("MANGO_PROXY_SETTING must be admin, anyone or off".into()),
            };
        }
        if let Some(v) = var("MANGO_ACCOUNT_URL").filter(|v| !v.trim().is_empty()) {
            self.account_url = Some(v.trim().to_owned());
        }
        self.first_password = var("MANGO_PASSWORD").filter(|v| !v.is_empty());
        if let Some(v) = var("MANGO_HOSTS") {
            self.hosts = v.split(',').map(str::trim).filter(|name| !name.is_empty()).map(str::to_owned).collect();
        }
        if let Some(v) = var("MANGO_RELEASES_REPO") {
            self.releases.repo = Some(v);
        }
        if let Some(v) = var("MANGO_RELEASES_TOKEN") {
            self.releases.token = Some(v);
        }
        if let Some(v) = var("MANGO_RELEASES_API") {
            self.releases.api = v;
        }
        if let Some(v) = number("MANGO_RELEASES_CHECK_HOURS")? {
            self.releases.check_hours = v;
        }
        // The name goes into an address, so nothing but a plain `owner/name` will do.
        let plain = |part: &str| !part.is_empty() && !part.starts_with('.') && part.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
        if let Some(repo) = self.releases.repo.as_deref().filter(|repo| !repo.is_empty()) {
            if !repo.split_once('/').is_some_and(|(owner, name)| plain(owner) && plain(name)) {
                return Err("releases.repo must read owner/name".into());
            }
        }
        // It becomes a link: an https address, or a path on this same site.
        if let Some(url) = self.account_url.as_deref() {
            let shaped = url.starts_with("https://") || (url.starts_with('/') && !url.starts_with("//"));
            if !shaped || url.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '"' | '<' | '>' | '\\')) {
                return Err("account_url must be an https:// address or a path starting with /".into());
            }
        }
        Ok(())
    }

    /// The port in `listen`.
    pub fn port(&self) -> Result<u16, String> {
        let port = self.listen.rsplit(':').next().and_then(|port| port.parse().ok());
        port.ok_or_else(|| format!("listen must end in a port: {:?}", self.listen))
    }

    /// The local source's folder.
    pub fn local_dir(&self) -> PathBuf {
        match &self.local_dir {
            Some(dir) => std::path::absolute(dir).unwrap_or_else(|_| dir.clone()),
            None => self.data_dir.join("local"),
        }
    }

    /// Where the extension host's jars are expected.
    pub fn host_dir(&self) -> PathBuf {
        self.host.dir.clone().unwrap_or_else(|| {
            let exe = env::current_exe().ok();
            exe.as_deref().and_then(Path::parent).unwrap_or(Path::new(".")).join("host")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_then_environment() {
        let mut cfg: Config = toml::from_str("listen = \"0.0.0.0:9000\"\n[host]\nidle_secs = 30\n").unwrap();
        assert_eq!(cfg.listen, "0.0.0.0:9000");
        assert_eq!(cfg.host.per_source, 4, "unset keys keep their defaults");

        cfg.apply_env(|name| match name {
            "MANGO_HOST_IDLE_SECS" => Some("5".into()),
            "MANGO_DATA" => Some("/somewhere".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(cfg.host.idle_secs, 5);
        assert_eq!(cfg.data_dir, PathBuf::from("/somewhere"));
        assert_eq!(cfg.listen, "0.0.0.0:9000");

        assert!(cfg.login, "login is on unless switched off");
        assert_eq!(cfg.port(), Ok(9000));
        cfg.apply_env(|name| match name {
            "MANGO_LOGIN" => Some("off".into()),
            "MANGO_RELEASES_REPO" => Some("someone/mango".into()),
            _ => None,
        })
        .unwrap();
        assert!(!cfg.login);
        assert!(cfg.host.webview, "the web view is on unless switched off");
        cfg.apply_env(|name| (name == "MANGO_WEBVIEW").then(|| "off".into())).unwrap();
        assert!(!cfg.host.webview);
        assert!(cfg.apply_env(|name| (name == "MANGO_LOGIN").then(|| "maybe".into())).is_err());
        assert!(!cfg.proxied && !cfg.signup && !cfg.stores.local && !cfg.stores.anyone_installs, "what widens Mango is off unless switched on");
        cfg.apply_env(|name| matches!(name, "MANGO_PROXIED" | "MANGO_SIGNUP" | "MANGO_STORES_LOCAL" | "MANGO_ANYONE_INSTALLS").then(|| "on".into())).unwrap();
        assert!(cfg.proxied && cfg.signup && cfg.stores.local && cfg.stores.anyone_installs);
        assert_eq!(cfg.host.proxy_setting, ProxySetting::Admin, "only an admin sets the proxy unless opened");
        cfg.apply_env(|name| (name == "MANGO_PROXY_SETTING").then(|| " Anyone ".into())).unwrap();
        assert_eq!(cfg.host.proxy_setting, ProxySetting::Anyone);
        cfg.apply_env(|name| (name == "MANGO_PROXY_SETTING").then(|| "off".into())).unwrap();
        assert_eq!(cfg.host.proxy_setting, ProxySetting::Off);
        assert!(cfg.apply_env(|name| (name == "MANGO_PROXY_SETTING").then(|| "everyone".into())).is_err());
        assert!(cfg.hosts.is_empty());
        cfg.apply_env(|name| (name == "MANGO_HOSTS").then(|| "mango.example.org, read.example.org,".into())).unwrap();
        assert_eq!(cfg.hosts, ["mango.example.org", "read.example.org"]);
        assert!(cfg.keep_free_mb == 0 && cfg.stores.blocklist.is_none() && cfg.hold_file.is_none(), "nothing is held back unless asked");
        cfg.apply_env(|name| match name {
            "MANGO_KEEP_FREE_MB" => Some("200".into()),
            "MANGO_BLOCKLIST" => Some("/etc/mango/blocklist".into()),
            "MANGO_HOLD_FILE" => Some(" /etc/mango/limits ".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!((cfg.keep_free_mb, cfg.stores.blocklist.as_deref()), (200, Some(Path::new("/etc/mango/blocklist"))));
        assert_eq!(cfg.hold_file.as_deref(), Some(Path::new("/etc/mango/limits")));
        assert!(cfg.account_url.is_none(), "no account page is shown unless one is named");
        for good in ["/account/", "https://example.org/account"] {
            cfg.apply_env(|name| (name == "MANGO_ACCOUNT_URL").then(|| good.into())).unwrap();
            assert_eq!(cfg.account_url.as_deref(), Some(good));
        }
        for bad in ["http://example.org", "//example.org", "javascript:alert(1)", "/a b", "/a\"b"] {
            assert!(cfg.apply_env(|name| (name == "MANGO_ACCOUNT_URL").then(|| bad.into())).is_err(), "{bad}");
        }
        cfg.account_url = None;
        assert!(cfg.apply_env(|name| (name == "MANGO_RELEASES_REPO").then(|| "someone/mango/../x".into())).is_err());

        assert!(cfg.apply_env(|name| (name == "MANGO_HOST_IDLE_SECS").then(|| "soon".into())).is_err());
        assert!(toml::from_str::<Config>("lisen = \"x\"").is_err(), "typos are reported");
    }
}
