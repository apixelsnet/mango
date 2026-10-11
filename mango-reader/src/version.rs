//! Which Mango this is, and whether a newer one is out.
//!
//! The version is fixed when the binary is built (`build.rs`); a release build takes
//! it from its tag. The list of releases comes from the repository's release API, is
//! kept in the database, and is asked for again once a day. Mango never replaces
//! itself: it says what is out and where to get it.

use crate::{
    config::ReleasesConfig,
    db::{self, Db},
    error::{AppError, Result},
};
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

pub const VERSION: &str = env!("MANGO_VERSION");
/// The commit the binary was built from. Empty when the build did not say.
pub const COMMIT: &str = env!("MANGO_COMMIT");
/// `owner/name` of the repository the build came from. Empty in a build made by hand.
const BUILT_FROM: &str = env!("MANGO_REPO");

const STATE_KEY: &str = "releases";
const KEPT: usize = 10;
const MAX_NOTES: usize = 8000;

/// "0.1.0 (1a2b3c4)", for the log and `mango version`.
pub fn describe() -> String {
    if COMMIT.is_empty() {
        VERSION.to_owned()
    } else {
        format!("{VERSION} ({COMMIT})")
    }
}

/// `linux-x86_64`, `macos-aarch64`, `windows-x86_64`: how release files are named.
fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

// ---------- comparing versions ----------

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Part {
    Number(u64),
    Text(String),
}

/// A version as semver orders it: by its numbers, then a pre-release before the release it leads to.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    numbers: [u64; 3],
    released: bool,
    pre: Vec<Part>,
}

/// Reads `1.2.3`, `v1.2`, `1.2.3-rc.1` and `1.2.3+build`. None for anything else.
fn parse(text: &str) -> Option<Version> {
    let text = text.trim().trim_start_matches(['v', 'V']).split('+').next()?;
    let (core, pre) = match text.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (text, None),
    };
    let mut parts = core.split('.');
    let mut numbers = [0u64; 3];
    for (index, slot) in numbers.iter_mut().enumerate() {
        match parts.next() {
            Some(part) => *slot = part.parse().ok()?,
            None if index > 0 => {}
            None => return None,
        }
    }
    if parts.next().is_some() {
        return None;
    }
    let pre = pre.map(|pre| pre.split('.').map(|part| part.parse().map_or_else(|_| Part::Text(part.to_owned()), Part::Number)).collect());
    Some(Version { numbers, released: pre.is_none(), pre: pre.unwrap_or_default() })
}

// ---------- the release list ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    name: String,
    notes: String,
    /// The release's page.
    url: String,
    /// As the list gives it: `2026-01-31T12:00:00Z`.
    published_at: Option<String>,
    prerelease: bool,
    /// The file for the platform this Mango runs on, when the release has one.
    download: Option<Download>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Download {
    name: String,
    url: String,
    size: u64,
}

/// What the last look at the list found. Kept in the `state` table.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Check {
    checked_at: Option<i64>,
    error: Option<String>,
    releases: Vec<Release>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    current: &'static str,
    commit: &'static str,
    platform: String,
    /// Running from the Docker image: an update is a new image, not a download.
    docker: bool,
    /// Whether this Mango knows where its releases are listed.
    can_check: bool,
    /// How often the list is asked. 0: only when the owner asks.
    check_hours: u64,
    checked_at: Option<i64>,
    error: Option<String>,
    update_available: bool,
    latest: Option<Release>,
    /// Newest first.
    releases: Vec<Release>,
}

/// A release as GitHub's API lists it. Gitea and Forgejo answer in the same shape.
#[derive(Deserialize)]
struct Listed {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    published_at: Option<String>,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// Drafts and tags that are not versions are left out. Newest first.
fn tidy(listed: Vec<Listed>, platform: &str) -> Vec<Release> {
    let wanted = format!("-{platform}.");
    let mut releases: Vec<(Version, Release)> = listed
        .into_iter()
        .filter(|release| !release.draft)
        .filter_map(|release| {
            let version = parse(&release.tag_name)?;
            let download = release.assets.into_iter().find(|asset| asset.name.contains(&wanted));
            let tag = release.tag_name.trim().trim_start_matches(['v', 'V']).to_owned();
            Some((
                version,
                Release {
                    name: release.name.filter(|name| !name.trim().is_empty()).unwrap_or_else(|| release.tag_name.clone()),
                    version: tag,
                    notes: release.body.unwrap_or_default().chars().take(MAX_NOTES).collect(),
                    url: release.html_url,
                    published_at: release.published_at,
                    prerelease: release.prerelease,
                    download: download.map(|asset| Download { name: asset.name, url: asset.browser_download_url, size: asset.size }),
                },
            ))
        })
        .collect();
    releases.sort_by(|a, b| b.0.cmp(&a.0));
    releases.into_iter().map(|(_, release)| release).take(KEPT).collect()
}

/// The release to move to: the newest one, pre-releases counted only for someone already on one.
fn newest<'a>(releases: &'a [Release], current: &str) -> Option<&'a Release> {
    let on_pre = parse(current).is_some_and(|version| !version.released);
    releases.iter().filter(|release| on_pre || !release.prerelease).max_by_key(|release| parse(&release.version))
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub struct Releases {
    db: Db,
    http: reqwest::Client,
    api: String,
    repo: Option<String>,
    token: Option<String>,
    hours: u64,
    docker: bool,
    last: Mutex<Check>,
    /// One look at a time.
    checking: tokio::sync::Mutex<()>,
}

impl Releases {
    pub async fn open(db: Db, cfg: &ReleasesConfig) -> Result<Arc<Self>> {
        let repo = cfg.repo.clone().or_else(|| Some(BUILT_FROM.to_owned())).filter(|repo| !repo.is_empty());
        let http = reqwest::Client::builder()
            .user_agent(format!("mango/{VERSION}"))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| AppError::internal(e.to_string()))?;
        let stored = db.call(|conn| db::state(conn, STATE_KEY)).await?;
        Ok(Arc::new(Self {
            db,
            http,
            api: cfg.api.trim_end_matches('/').to_owned(),
            repo,
            token: cfg.token.clone().filter(|token| !token.is_empty()),
            hours: cfg.check_hours,
            docker: std::env::var_os("MANGO_DOCKER").is_some(),
            last: Mutex::new(stored.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()),
            checking: tokio::sync::Mutex::new(()),
        }))
    }

    pub fn view(&self) -> View {
        let last = locked(&self.last).clone();
        let latest = newest(&last.releases, VERSION).cloned();
        let update_available = latest.as_ref().is_some_and(|latest| parse(&latest.version) > parse(VERSION));
        View {
            current: VERSION,
            commit: COMMIT,
            platform: platform(),
            docker: self.docker,
            can_check: self.repo.is_some(),
            check_hours: self.hours,
            checked_at: last.checked_at,
            error: last.error,
            update_available,
            latest,
            releases: last.releases,
        }
    }

    /// Asks the release list now. A list that does not answer is reported in the view, not as an error.
    pub async fn check(&self) -> Result<View> {
        let Some(repo) = self.repo.as_deref() else {
            return Err(AppError::bad_request("This build does not know where its releases are listed. Set MANGO_RELEASES_REPO to owner/name."));
        };
        let _one = self.checking.lock().await;
        let mut next = Check { checked_at: Some(db::now()), ..Check::default() };
        match self.fetch(repo).await {
            Ok(releases) => next.releases = releases,
            Err(why) => {
                tracing::warn!("release check: {why}");
                // What was known before is still the best there is.
                next.releases = locked(&self.last).releases.clone();
                next.error = Some(why);
            }
        }
        let text = serde_json::to_string(&next).map_err(|e| AppError::internal(e.to_string()))?;
        self.db.call(move |conn| db::set_state(conn, STATE_KEY, &text)).await?;
        *locked(&self.last) = next;
        Ok(self.view())
    }

    async fn fetch(&self, repo: &str) -> std::result::Result<Vec<Release>, String> {
        let url = format!("{}/repos/{repo}/releases?per_page=30", self.api);
        let mut request = self.http.get(url).header("accept", "application/vnd.github+json").header("x-github-api-version", "2022-11-28");
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await.map_err(|e| format!("The release list did not answer: {}", e.without_url()))?;
        match response.status().as_u16() {
            200 => {}
            401 => return Err("The release list refused the token in MANGO_RELEASES_TOKEN.".into()),
            403 | 429 => return Err("The release list is taking no more questions for now. Try again in an hour.".into()),
            404 if self.token.is_none() => {
                return Err("There is no public release list for this build. A private repository needs a token in MANGO_RELEASES_TOKEN.".into())
            }
            status => return Err(format!("The release list answered {status}.")),
        }
        let listed: Vec<Listed> = response.json().await.map_err(|_| "The release list did not read as one.".to_owned())?;
        Ok(tidy(listed, &platform()))
    }

    /// Looks at the list every `check_hours`. Returns at once when there is nothing to ask or the check is off.
    pub async fn check_on_schedule(self: Arc<Self>) {
        if self.repo.is_none() || self.hours == 0 {
            return;
        }
        let every = self.hours as i64 * 3600;
        loop {
            let last = locked(&self.last).checked_at.unwrap_or(0);
            // Never straight at the start: a Mango that restarts in a loop must not hammer the list.
            let wait = (last + every - db::now()).max(60);
            tokio::time::sleep(Duration::from_secs(wait as u64)).await;
            let _ = self.check().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_order_like_semver() {
        let order = ["0.9.9", "1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-beta.2", "1.0.0-beta.11", "1.0.0-rc.1", "1.0.0", "1.0.1", "1.2", "1.10.0"];
        for pair in order.windows(2) {
            assert!(parse(pair[0]) < parse(pair[1]), "{} comes before {}", pair[0], pair[1]);
        }
        assert_eq!(parse("v1.2.3+build.7"), parse("1.2.3"));
        assert_eq!(parse("1.2"), parse("1.2.0"));
        assert!(parse("nightly").is_none() && parse("1.2.3.4").is_none() && parse("").is_none());
    }

    #[test]
    fn list_is_tidied_and_the_newest_found() {
        let listed: Vec<Listed> = serde_json::from_value(serde_json::json!([
            { "tag_name": "v0.2.0", "name": "", "body": "Second", "html_url": "https://example.local/r/2", "published_at": "2026-02-01T00:00:00Z",
              "assets": [
                { "name": "mango-0.2.0-linux-x86_64.tar.gz", "browser_download_url": "https://example.local/d/linux", "size": 5 },
                { "name": "mango-0.2.0-windows-x86_64.zip", "browser_download_url": "https://example.local/d/windows", "size": 6 } ] },
            { "tag_name": "v0.10.0", "name": "Ten", "body": null, "html_url": "https://example.local/r/10", "published_at": null },
            { "tag_name": "v0.11.0-rc.1", "prerelease": true, "html_url": "https://example.local/r/11" },
            { "tag_name": "v0.12.0", "draft": true, "html_url": "https://example.local/r/12" },
            { "tag_name": "nightly", "html_url": "https://example.local/r/n" }
        ]))
        .unwrap();
        let releases = tidy(listed, "windows-x86_64");
        let versions: Vec<&str> = releases.iter().map(|release| release.version.as_str()).collect();
        assert_eq!(versions, ["0.11.0-rc.1", "0.10.0", "0.2.0"], "newest first, no draft, no tag that is not a version");
        assert_eq!(releases[2].name, "v0.2.0", "a release without a name goes by its tag");
        assert_eq!(releases[2].download.as_ref().unwrap().url, "https://example.local/d/windows");
        assert!(releases[1].download.is_none());

        assert_eq!(newest(&releases, "0.2.0").unwrap().version, "0.10.0", "a pre-release is not offered to a release");
        assert_eq!(newest(&releases, "0.11.0-beta.1").unwrap().version, "0.11.0-rc.1");
        assert!(newest(&[], "0.2.0").is_none());
    }
}
