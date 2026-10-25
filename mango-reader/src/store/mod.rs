//! Extension stores: add, refresh, browse the catalog, install, update, remove.
//!
//! Only install, remove and the comparison with the host start the JVM. Everything
//! else reads the index a store publishes and the database.

pub mod blocklist;
pub mod index;
pub mod verify;

use crate::{
    auth::Who,
    db::{now, Db},
    error::{AppError, Result},
    host::{Call, Host},
    local::PKG as LOCAL_PKG,
    net,
};
use reqwest::{header, Method, Url};
use rusqlite::{params, params_from_iter, types::Value as Sql, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};
use blocklist::Blocklist;
use tokio::sync::{watch, Mutex};
use verify::VerifyError;

/// Cap on any one download: an index, a hash list or an extension.
const MAX_DOWNLOAD: usize = 64 << 20;
const INSTALL_TIMEOUT: Duration = Duration::from_secs(180);
/// How often an account looks whether the blocklist changed.
const BLOCKLIST_EVERY: Duration = Duration::from_secs(60);
const KEY_CHANGED: &str = "The store now publishes a different signing key. Accept it only if the store announced the change.";

const STORE_COLUMNS: &str = "SELECT s.id, s.url, s.index_url, s.name, s.badge, s.website, s.signing_key, s.offered_key, \
     s.checked_at, s.updated_at, s.error, (SELECT COUNT(*) FROM catalog c WHERE c.store_id = s.id) FROM stores s";
const CATALOG_COLUMNS: &str = "SELECT c.store_id, c.pkg, c.name, c.version_name, c.version_code, c.lib, c.content_warning, \
     c.icon_url, c.langs, c.source_count, c.jar_url IS NOT NULL, e.version_name, e.version_code, e.store_id";
const CATALOG_FROM: &str = " FROM catalog c LEFT JOIN extensions e ON e.pkg = c.pkg";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreInfo {
    pub id: i64,
    pub url: String,
    pub index_url: String,
    pub name: String,
    pub badge: String,
    pub website: Option<String>,
    /// The key pinned when the store was added.
    pub signing_key: String,
    /// True while the store publishes another key. Nothing installs from it until that is settled.
    pub key_changed: bool,
    pub offered_key: Option<String>,
    pub extensions: i64,
    pub checked_at: Option<i64>,
    pub updated_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogItem {
    pub store_id: i64,
    pub pkg: String,
    pub name: String,
    pub version_name: String,
    pub version_code: i64,
    pub lib: String,
    /// 0 or 1 safe, 2 mixed, 3 NSFW.
    pub content_warning: i64,
    pub icon_url: Option<String>,
    pub languages: Vec<String>,
    pub source_count: i64,
    /// False for extensions published as an APK only.
    pub installable: bool,
    pub installed: bool,
    pub installed_version: Option<String>,
    pub update_available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<CatalogSource>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSource {
    pub id: String,
    pub name: String,
    pub lang: String,
    pub home_url: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Installed {
    pub pkg: String,
    pub name: String,
    pub version_name: String,
    pub version_code: i64,
    pub lib: String,
    pub content_warning: i64,
    pub icon_url: Option<String>,
    pub store_id: Option<i64>,
    pub update_available: bool,
    pub available_version: Option<String>,
    pub sources: Vec<SourceInfo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    /// A 64-bit number, sent as text.
    pub id: String,
    pub pkg: String,
    pub name: String,
    pub lang: String,
    pub base_url: Option<String>,
    pub supports_latest: bool,
    /// The owner's choices for the source list.
    pub pinned: bool,
    pub hidden: bool,
    /// The folder the owner put it in on Browse.
    pub folder: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateResult {
    pub pkg: String,
    pub version_name: Option<String>,
    pub error: Option<String>,
}

/// What to list from the catalog. Empty fields do not filter.
#[derive(Default)]
pub struct CatalogQuery {
    pub text: String,
    /// Any of these languages.
    pub langs: Vec<String>,
    /// Content-warning values to include. Empty means everything except NSFW.
    pub levels: Vec<i64>,
    /// "installed", "available" or "updates".
    pub state: Option<String>,
    pub store: Option<i64>,
    pub limit: i64,
    pub offset: i64,
}

/// The host's description of an installed extension (protocol v1, `Extension`).
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostExtension {
    pkg: String,
    name: String,
    version_name: String,
    version_code: i64,
    lib: String,
    content_warning: i64,
    sources: Vec<HostSource>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostSource {
    id: String,
    name: String,
    lang: String,
    base_url: Option<String>,
    #[serde(default)]
    supports_latest: bool,
}

#[derive(Deserialize)]
struct HostExtensions {
    extensions: Vec<HostExtension>,
}

enum Fetched {
    NotModified,
    Missing,
    Body { bytes: Vec<u8>, etag: Option<String> },
}

/// A store index, downloaded and decoded.
struct Pulled {
    store: index::StoreMsg,
    key: String,
    etag: Option<String>,
    /// sha256 of each extension's jar, by package, when the store publishes them.
    hashes: HashMap<String, String>,
}

/// The accounts that share one extension host. Each keeps its own stores and its own
/// list of what it installed; the host holds every extension any of them has.
#[derive(Clone, Default)]
pub struct Fleet {
    /// Installs, removals and the comparison with the host run one at a time, whoever asks.
    changes: Arc<Mutex<()>>,
    libraries: Arc<StdMutex<HashMap<i64, Db>>>,
}

impl Fleet {
    pub fn join(&self, account: i64, db: Db) {
        self.libraries.lock().unwrap_or_else(|e| e.into_inner()).insert(account, db);
    }

    pub fn leave(&self, account: i64) {
        self.libraries.lock().unwrap_or_else(|e| e.into_inner()).remove(&account);
    }

    /// The databases of every account but this one.
    fn others(&self, account: i64) -> Vec<Db> {
        let libraries = self.libraries.lock().unwrap_or_else(|e| e.into_inner());
        libraries.iter().filter(|(id, _)| **id != account).map(|(_, db)| db.clone()).collect()
    }
}

/// What the owner allows around stores. Both are off unless switched on.
#[derive(Clone, Copy, Default)]
pub struct Rules {
    /// Stores may be on this machine or its network, and fetched over plain HTTP.
    pub local: bool,
    /// Any account may bring an extension into the host, not only an admin.
    pub anyone_installs: bool,
}

#[derive(Clone)]
pub struct Stores {
    db: Db,
    http: reqwest::Client,
    host: Arc<Host>,
    fleet: Fleet,
    /// The account these stores belong to.
    account: i64,
    rules: Rules,
    blocked: Arc<Blocklist>,
}

impl Stores {
    pub fn new(db: Db, host: Arc<Host>, fleet: Fleet, account: i64, rules: Rules, blocked: Arc<Blocklist>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("Mango/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120));
        let http = net::guard(http, rules.local).build().map_err(|e| AppError::internal(e.to_string()))?;
        Ok(Self { db, http, host, fleet, account, rules, blocked })
    }

    /// Whether `who` may bring code into the extension host, which every account shares:
    /// install an extension no account has yet, or change what a source is set to.
    pub fn open_to(&self, who: &Who) -> bool {
        self.rules.anyone_installs || who.admin
    }

    pub fn anyone_installs(&self) -> bool {
        self.rules.anyone_installs
    }

    /// A store's address, parsed. Refused where stores are not fetched from.
    fn address(&self, url: &str) -> Result<Url> {
        let parsed = http_url(url)?;
        match net::refusal(&parsed, self.rules.local) {
            Some(why) => Err(AppError::new(400, "address_refused", why)),
            None => Ok(parsed),
        }
    }

    // ---------- stores ----------

    pub async fn list(&self) -> Result<Vec<StoreInfo>> {
        self.db
            .call(|conn| {
                let mut stmt = conn.prepare(&format!("{STORE_COLUMNS} ORDER BY s.id"))?;
                let stores = stmt.query_map([], store_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(stores)
            })
            .await
    }

    pub async fn get(&self, id: i64) -> Result<StoreInfo> {
        self.db
            .call(move |conn| {
                conn.query_row(&format!("{STORE_COLUMNS} WHERE s.id = ?1"), [id], store_row).optional()?.ok_or_else(no_store)
            })
            .await
    }

    /// Adds a store from the URL of its `repo.json` or its `index.pb`, and pins its signing key.
    pub async fn add(&self, url: &str) -> Result<StoreInfo> {
        let url = self.address(url)?.to_string();
        if self.blocked.store(&url) {
            return Err(blocked_store());
        }
        let (mut bytes, mut etag) = self.download(&url).await?;
        let mut index_url = url.clone();
        let mut repo_key = None;
        match serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Object(_)) => {
                let repo: index::RepoJson = serde_json::from_slice(&bytes).map_err(|_| not_a_store())?;
                index_url = self.address(&repo.index_v2.ok_or_else(not_a_store)?)?.to_string();
                if self.blocked.store(&index_url) {
                    return Err(blocked_store());
                }
                repo_key = repo.meta.signing_key;
                (bytes, etag) = self.download(&index_url).await?;
            }
            Ok(_) => {
                return Err(AppError::new(
                    400,
                    "legacy_store",
                    "This is an old index.min.json store. Mango reads stores that publish repo.json and index.pb",
                ));
            }
            Err(_) => {}
        }
        let pulled = self.decode(&index_url, &bytes, etag).await?;
        if repo_key.is_some_and(|key| index::normalize_key(&key).as_deref() != Some(pulled.key.as_str())) {
            return Err(AppError::new(422, "signing_key_mismatch", "The store's repo.json and its index name different signing keys"));
        }

        let id = self
            .db
            .call(move |conn| {
                let tx = conn.transaction()?;
                // One store can be reached by several addresses. Its signing key tells that it is the same one.
                let known = "SELECT 1 FROM stores WHERE index_url = ?1 OR signing_key = ?2 OR offered_key = ?2";
                if tx.query_row(known, [&index_url, &pulled.key], |_| Ok(())).optional()?.is_some() {
                    return Err(AppError::new(409, "store_exists", "This store was already added"));
                }
                tx.execute(
                    "INSERT INTO stores (url, index_url, name, signing_key, added_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![url, index_url, pulled.store.name, pulled.key, now()],
                )?;
                let id = tx.last_insert_rowid();
                write_catalog(&tx, id, &pulled)?;
                tx.commit()?;
                Ok(id)
            })
            .await?;
        self.get(id).await
    }

    /// Fetches a store's index again and replaces its catalog when it changed.
    /// A changed signing key is recorded and the catalog left alone.
    pub async fn refresh(&self, id: i64) -> Result<StoreInfo> {
        let (index_url, etag, pinned) = self
            .db
            .call(move |conn| {
                conn.query_row("SELECT index_url, etag, signing_key FROM stores WHERE id = ?1", [id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?))
                })
                .optional()?
                .ok_or_else(no_store)
            })
            .await?;

        let pulled = match self.pull(&index_url, etag.as_deref()).await {
            Ok(pulled) => pulled,
            Err(e) => {
                let why = e.message.clone();
                self.db
                    .call(move |conn| {
                        conn.execute("UPDATE stores SET error = ?2, checked_at = ?3 WHERE id = ?1", params![id, why, now()])?;
                        Ok(())
                    })
                    .await?;
                return Err(e);
            }
        };

        self.db
            .call(move |conn| {
                match pulled {
                    None => {
                        conn.execute(
                            "UPDATE stores SET checked_at = ?2, error = CASE WHEN offered_key IS NULL THEN NULL ELSE error END WHERE id = ?1",
                            params![id, now()],
                        )?;
                    }
                    Some(pulled) if pulled.key != pinned => {
                        tracing::warn!("store {id} changed its signing key from {pinned} to {}", pulled.key);
                        conn.execute(
                            "UPDATE stores SET offered_key = ?2, error = ?3, checked_at = ?4 WHERE id = ?1",
                            params![id, pulled.key, KEY_CHANGED, now()],
                        )?;
                    }
                    Some(pulled) => {
                        let tx = conn.transaction()?;
                        write_catalog(&tx, id, &pulled)?;
                        tx.commit()?;
                    }
                }
                Ok(())
            })
            .await?;
        self.get(id).await
    }

    /// Refreshes every store. One that fails keeps its old catalog and reports the error in its entry.
    pub async fn refresh_all(&self) -> Result<Vec<StoreInfo>> {
        for store in self.list().await? {
            if let Err(e) = self.refresh(store.id).await {
                tracing::warn!("store {}: {e}", store.id);
            }
        }
        self.list().await
    }

    /// Pins the key a store changed to. The owner's decision; nothing else calls this.
    pub async fn accept_key(&self, id: i64) -> Result<StoreInfo> {
        if !self.get(id).await?.key_changed {
            return Err(AppError::new(409, "no_key_change", "This store has no new signing key waiting"));
        }
        self.db
            .call(move |conn| {
                conn.execute(
                    "UPDATE stores SET signing_key = offered_key, offered_key = NULL, etag = NULL, error = NULL WHERE id = ?1 AND offered_key IS NOT NULL",
                    [id],
                )?;
                Ok(())
            })
            .await?;
        self.refresh(id).await
    }

    /// Removes a store and its catalog. Extensions installed from it stay, without updates.
    pub async fn remove_store(&self, id: i64) -> Result<()> {
        let removed = self.db.call(move |conn| Ok(conn.execute("DELETE FROM stores WHERE id = ?1", [id])?)).await?;
        if removed == 0 {
            return Err(no_store());
        }
        Ok(())
    }

    /// Checks stores for new versions every `hours`. Never returns.
    pub async fn refresh_on_schedule(self, hours: u64) {
        if hours == 0 {
            return;
        }
        let every = Duration::from_secs(hours * 3600);
        loop {
            let before = now() - every.as_secs() as i64;
            let due = self
                .db
                .call(move |conn| {
                    let mut stmt = conn.prepare("SELECT id FROM stores WHERE checked_at IS NULL OR checked_at <= ?1")?;
                    let ids = stmt.query_map([before], |r| r.get::<_, i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
                    Ok(ids)
                })
                .await
                .unwrap_or_default();
            for id in due {
                if let Err(e) = self.refresh(id).await {
                    tracing::warn!("store {id}: {e}");
                }
            }
            tokio::time::sleep(every.min(Duration::from_secs(900))).await;
        }
    }

    // ---------- catalog ----------

    pub async fn catalog(&self, query: CatalogQuery) -> Result<(Vec<CatalogItem>, i64)> {
        self.db.call(move |conn| query_catalog(conn, &query)).await
    }

    /// One catalog entry with its sources.
    pub async fn catalog_item(&self, store: i64, pkg: String) -> Result<CatalogItem> {
        self.db
            .call(move |conn| {
                let mut item = conn
                    .query_row(
                        &format!("{CATALOG_COLUMNS}{CATALOG_FROM} WHERE c.store_id = ?1 AND c.pkg = ?2"),
                        params![store, pkg],
                        catalog_row,
                    )
                    .optional()?
                    .ok_or_else(|| AppError::new(404, "unknown_extension", "The store does not list this extension"))?;
                let mut stmt = conn.prepare(
                    "SELECT source_id, name, lang, home_url FROM catalog_sources WHERE store_id = ?1 AND pkg = ?2 ORDER BY name COLLATE NOCASE, lang",
                )?;
                let sources = stmt
                    .query_map(params![store, pkg], |r| {
                        Ok(CatalogSource { id: r.get::<_, i64>(0)?.to_string(), name: r.get(1)?, lang: r.get(2)?, home_url: r.get(3)? })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                item.sources = Some(sources);
                Ok(item)
            })
            .await
    }

    // ---------- installed extensions ----------

    pub async fn installed(&self) -> Result<Vec<Installed>> {
        self.db.call(|conn| read_installed(conn, None)).await
    }

    /// Sources of the installed extensions. This is what Browse lists.
    pub async fn sources(&self) -> Result<Vec<SourceInfo>> {
        self.db.call(|conn| read_sources(conn)).await
    }

    /// Pins or hides a source in the source list, or puts it in a folder. A value left out
    /// keeps its state; an empty folder takes it out of its folder.
    pub async fn set_source(&self, id: i64, pinned: Option<bool>, hidden: Option<bool>, folder: Option<String>) -> Result<()> {
        let folder = folder.map(|name| name.trim().to_owned());
        if folder.as_ref().is_some_and(|name| name.chars().count() > 60) {
            return Err(AppError::bad_request("A folder's name has 60 characters at most"));
        }
        self.db
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO source_prefs (source_id, pinned, hidden, folder) VALUES (?1, COALESCE(?2, 0), COALESCE(?3, 0), NULLIF(?4, ''))
                     ON CONFLICT(source_id) DO UPDATE SET pinned = COALESCE(?2, pinned), hidden = COALESCE(?3, hidden),
                         folder = CASE WHEN ?4 IS NULL THEN folder ELSE NULLIF(?4, '') END",
                    params![id, pinned, hidden, folder],
                )?;
                Ok(())
            })
            .await
    }

    /// What a source lets its user set, as the host describes it. The host keeps one set
    /// per source, so accounts that have the same extension share its preferences.
    pub async fn preferences(&self, id: i64) -> Result<Value> {
        self.preference_call(id, Method::GET, None).await
    }

    /// Changes one preference. Answers with the list as it is afterwards.
    pub async fn set_preference(&self, id: i64, change: &Value) -> Result<Value> {
        self.preference_call(id, Method::PUT, Some(change)).await
    }

    async fn preference_call(&self, id: i64, method: Method, body: Option<&Value>) -> Result<Value> {
        let pkg: Option<String> =
            self.db.call(move |conn| Ok(conn.query_row("SELECT pkg FROM sources WHERE id = ?1", [id], |r| r.get(0)).optional()?)).await?;
        match pkg.as_deref() {
            None => Err(AppError::new(404, "unknown_source", "No installed extension provides this source")),
            // The local source is not the host's, and has nothing to set.
            Some(LOCAL_PKG) => Ok(json!({ "preferences": [] })),
            Some(_) => {
                let opts = Call { source: Some(id), ..Call::default() };
                self.host.call(method, &format!("/sources/{id}/preferences"), body, opts).await
            }
        }
    }

    /// Installs an extension from a store, or replaces the installed version with the store's.
    ///
    /// The jar is downloaded and checked here: its hash against the store's list when
    /// there is one, its signature against the store's pinned key. The host then
    /// fetches the same file and is given the hash of the bytes that passed.
    ///
    /// `may_bring` says whether the caller may put code into the host; see `open_to`.
    /// Without it, only what the host holds already is installed.
    pub async fn install(&self, pkg: &str, store: Option<i64>, may_bring: bool) -> Result<Installed> {
        if !valid_pkg(pkg) {
            return Err(AppError::bad_request("Not a package name"));
        }
        if self.blocked.package(pkg) {
            return Err(AppError::new(403, "blocked", "The operator of this server has blocked this extension."));
        }
        let _one_at_a_time = self.fleet.changes.lock().await;

        struct Pick {
            store_id: i64,
            jar_url: Option<String>,
            sha256: Option<String>,
            icon_url: Option<String>,
            version_code: i64,
            /// The highest version this store ever listed.
            highest: i64,
            key: String,
            key_changed: bool,
        }
        let wanted = pkg.to_owned();
        let pick = self
            .db
            .call(move |conn| {
                // The store it is installed from wins; otherwise the newest version on offer.
                let pick = conn
                    .query_row(
                        "SELECT c.store_id, c.jar_url, c.jar_sha256, c.icon_url, s.signing_key, s.offered_key IS NOT NULL, c.version_code,
                                COALESCE((SELECT v.version_code FROM seen_versions v WHERE v.store_id = c.store_id AND v.pkg = c.pkg), 0)
                         FROM catalog c JOIN stores s ON s.id = c.store_id
                         WHERE c.pkg = ?1 AND (?2 IS NULL OR c.store_id = ?2)
                         ORDER BY c.store_id = (SELECT store_id FROM extensions WHERE pkg = ?1) DESC, c.version_code DESC
                         LIMIT 1",
                        params![wanted, store],
                        |r| {
                            Ok(Pick {
                                store_id: r.get(0)?,
                                jar_url: r.get(1)?,
                                sha256: r.get(2)?,
                                icon_url: r.get(3)?,
                                key: r.get(4)?,
                                key_changed: r.get(5)?,
                                version_code: r.get(6)?,
                                highest: r.get(7)?,
                            })
                        },
                    )
                    .optional()?;
                Ok(pick)
            })
            .await?
            .ok_or_else(|| AppError::new(404, "unknown_extension", "No store lists this extension"))?;

        if pick.key_changed {
            return Err(AppError::new(409, "signing_key_changed", KEY_CHANGED));
        }
        // A store blocked a moment ago may not be swept away yet.
        let from = self.get(pick.store_id).await?;
        if self.blocked.store(&from.url) || self.blocked.store(&from.index_url) {
            return Err(blocked_store());
        }
        // The index is not signed. An older version would still pass the signature check.
        if pick.version_code < pick.highest {
            let why = "The store lists an older version of this extension than it did before. Mango does not go back to one. Remove the store and add it again if the store meant it.";
            return Err(AppError::new(409, "older_version", why));
        }
        let jar_url = pick.jar_url.ok_or_else(|| {
            AppError::new(501, "apk_not_supported", "This extension is published as an APK only, which Mango cannot install yet")
        })?;

        let (bytes, _) = self.download(&jar_url).await?;
        let (key, expected) = (pick.key, pick.sha256);
        let sha256 = tokio::task::spawn_blocking(move || check_jar(&bytes, &key, expected.as_deref()))
            .await
            .map_err(|e| AppError::internal(e.to_string()))??;

        // Another account may have brought this very version into the host already. Then
        // there is nothing to fetch again: the jar passed this account's check just now.
        let there: HostExtensions = self.host.call(Method::GET, "/extensions", None, Call::default()).await?;
        let ext = match there.extensions.into_iter().find(|ext| ext.pkg == pkg && ext.version_code == pick.version_code) {
            Some(ext) => ext,
            None if !may_bring => {
                let why = "Only an admin can bring in an extension, or a version of one, that no account has yet. Ask one to install it first.";
                return Err(AppError::new(403, "admin_only", why));
            }
            None => {
                // The host is told which extension and version were asked for, and refuses a file that is another.
                let body = json!({ "url": jar_url, "sha256": sha256, "pkg": pkg, "versionCode": pick.version_code });
                let opts = Call { timeout: Some(INSTALL_TIMEOUT), ..Call::default() };
                self.host.call(Method::POST, "/extensions/install", Some(&body), opts).await?
            }
        };

        // The host has one version of an extension. Accounts that had another now have this one.
        for other in self.fleet.others(self.account) {
            let ext = ext.clone();
            if let Err(e) = other.call(move |conn| follow(conn, &ext)).await {
                tracing::warn!("another account's list of extensions could not follow: {e}");
            }
        }

        let (store_id, icon_url) = (pick.store_id, pick.icon_url);
        self.db
            .call(move |conn| {
                save_installed(conn, &ext, Some(store_id), icon_url.as_deref())?;
                read_installed(conn, Some(&ext.pkg))?.pop().ok_or_else(|| AppError::internal("the extension vanished after install"))
            })
            .await
    }

    pub async fn remove(&self, pkg: &str) -> Result<()> {
        if !valid_pkg(pkg) || pkg == LOCAL_PKG {
            return Err(AppError::bad_request("Not a package name"));
        }
        let _one_at_a_time = self.fleet.changes.lock().await;
        let name = pkg.to_owned();
        let known = self
            .db
            .call(move |conn| Ok(conn.query_row("SELECT 1 FROM extensions WHERE pkg = ?1", [&name], |_| Ok(())).optional()?.is_some()))
            .await?;
        if !known {
            return Err(AppError::new(404, "not_installed", "This extension is not installed"));
        }
        // The host lets go of it only when no other account uses it.
        let mut shared = false;
        for other in self.fleet.others(self.account) {
            let name = pkg.to_owned();
            shared |= other.call(move |conn| Ok(conn.query_row("SELECT 1 FROM extensions WHERE pkg = ?1", [&name], |_| Ok(())).optional()?.is_some())).await?;
        }
        if !shared {
            let _: Value = self.host.call(Method::DELETE, &format!("/extensions/{pkg}"), None, Call::default()).await?;
        }
        let name = pkg.to_owned();
        self.db
            .call(move |conn| {
                conn.execute("DELETE FROM extensions WHERE pkg = ?1", [&name])?;
                Ok(())
            })
            .await
    }

    /// Installs every newer version the stores offer. One failure does not stop the rest.
    pub async fn update_all(&self, may_bring: bool) -> Result<Vec<UpdateResult>> {
        let due: Vec<(String, i64)> = self
            .db
            .call(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT e.pkg, e.store_id FROM extensions e JOIN catalog c ON c.store_id = e.store_id AND c.pkg = e.pkg
                     WHERE c.version_code > e.version_code ORDER BY e.pkg",
                )?;
                let due = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(due)
            })
            .await?;
        let mut results = Vec::new();
        for (pkg, store) in due {
            results.push(match self.install(&pkg, Some(store), may_bring).await {
                Ok(ext) => UpdateResult { pkg, version_name: Some(ext.version_name), error: None },
                Err(e) => UpdateResult { pkg, version_name: None, error: Some(e.message) },
            });
        }
        Ok(results)
    }

    /// Runs after each host start. Never returns.
    pub async fn keep_in_sync(self, mut starts: watch::Receiver<u64>) {
        while starts.changed().await.is_ok() {
            if let Err(e) = self.reconcile().await {
                tracing::warn!("could not compare the installed extensions with the host: {e}");
            }
        }
    }

    /// Makes this account's list match what the host really has. The host owns the files,
    /// so it wins: what a wiped host folder lost is dropped, and a version another account
    /// installed is taken over. What the host has beyond that belongs to other accounts.
    async fn reconcile(&self) -> Result<()> {
        let _one_at_a_time = self.fleet.changes.lock().await;
        let host: HostExtensions = self.host.call(Method::GET, "/extensions", None, Call::default()).await?;
        self.db
            .call(move |conn| {
                let known: HashMap<String, i64> = {
                    // The local source is listed like an extension but is not one the host knows.
                    let mut stmt = conn.prepare("SELECT pkg, version_code FROM extensions WHERE pkg != ?1")?;
                    let known = stmt.query_map([LOCAL_PKG], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
                    known
                };
                for pkg in known.keys().filter(|pkg| !host.extensions.iter().any(|ext| &ext.pkg == *pkg)) {
                    tracing::warn!("{pkg} is gone from the extension host; dropping it");
                    conn.execute("DELETE FROM extensions WHERE pkg = ?1", [pkg])?;
                }
                for ext in host.extensions.iter().filter(|ext| known.get(&ext.pkg).is_some_and(|code| *code != ext.version_code)) {
                    follow(conn, ext)?;
                }
                Ok(())
            })
            .await
    }

    /// Takes out of this account what the operator's blocklist names, each time the list
    /// changes: blocked stores, what was installed from them, and blocked extensions.
    /// Never returns.
    pub async fn keep_blocked_out(self) {
        let mut swept = 0;
        loop {
            let generation = self.blocked.generation();
            if generation != swept {
                match self.sweep_blocked().await {
                    Ok(()) => swept = generation,
                    Err(e) => tracing::warn!("could not take out what the blocklist names: {e}"),
                }
            }
            tokio::time::sleep(BLOCKLIST_EVERY).await;
        }
    }

    async fn sweep_blocked(&self) -> Result<()> {
        let stores: Vec<i64> = self.list().await?.into_iter().filter(|s| self.blocked.store(&s.url) || self.blocked.store(&s.index_url)).map(|s| s.id).collect();
        let installed = self
            .db
            .call(|conn| {
                let mut stmt = conn.prepare("SELECT pkg, store_id FROM extensions WHERE pkg != ?1")?;
                let installed = stmt.query_map([LOCAL_PKG], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<i64>>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(installed)
            })
            .await?;
        let mut failed = None;
        for (pkg, store) in installed {
            if self.blocked.package(&pkg) || store.is_some_and(|id| stores.contains(&id)) {
                match self.remove(&pkg).await {
                    Ok(()) => tracing::info!("{pkg} is blocked; account {} no longer has it", self.account),
                    Err(e) => failed = Some(e),
                }
            }
        }
        for id in stores {
            self.remove_store(id).await?;
            tracing::info!("store {id} is blocked; account {} no longer has it", self.account);
        }
        failed.map_or(Ok(()), Err)
    }

    /// Takes this account's extensions out of the host, where no other account has them.
    /// For an account that is being removed.
    pub async fn remove_all(&self) {
        let mine = self.db.call(|conn| {
            let mut stmt = conn.prepare("SELECT pkg FROM extensions WHERE pkg != ?1")?;
            let mine = stmt.query_map([LOCAL_PKG], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
            Ok(mine)
        });
        for pkg in mine.await.unwrap_or_default() {
            if let Err(e) = self.remove(&pkg).await {
                tracing::warn!("{pkg} stays in the extension host: {e}");
            }
        }
    }

    // ---------- downloads ----------

    async fn fetch(&self, url: &str, etag: Option<&str>) -> Result<Fetched> {
        let failed = |why: String| AppError::new(502, "fetch_failed", why);
        let mut request = self.http.get(self.address(url)?);
        if let Some(etag) = etag {
            request = request.header(header::IF_NONE_MATCH, etag);
        }
        let mut response = request.send().await.map_err(|e| failed(format!("Could not download {url}: {}", net::told(&e))))?;
        match response.status().as_u16() {
            304 => return Ok(Fetched::NotModified),
            404 => return Ok(Fetched::Missing),
            200..=299 => {}
            code => return Err(failed(format!("{url} answered HTTP {code}"))),
        }
        let etag = response.headers().get(header::ETAG).and_then(|v| v.to_str().ok()).map(str::to_owned);
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| failed(format!("The download of {url} broke off: {e}")))? {
            if bytes.len() + chunk.len() > MAX_DOWNLOAD {
                return Err(failed(format!("{url} is larger than {} MB", MAX_DOWNLOAD >> 20)));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Fetched::Body { bytes, etag })
    }

    async fn download(&self, url: &str) -> Result<(Vec<u8>, Option<String>)> {
        match self.fetch(url, None).await? {
            Fetched::Body { bytes, etag } => Ok((bytes, etag)),
            _ => Err(AppError::new(502, "fetch_failed", format!("{url} was not found"))),
        }
    }

    /// Downloads and decodes a store index. None when the server says it has not changed.
    async fn pull(&self, index_url: &str, etag: Option<&str>) -> Result<Option<Pulled>> {
        match self.fetch(index_url, etag).await? {
            Fetched::NotModified => Ok(None),
            Fetched::Missing => Err(AppError::new(502, "fetch_failed", format!("{index_url} was not found"))),
            Fetched::Body { bytes, etag } => self.decode(index_url, &bytes, etag).await.map(Some),
        }
    }

    async fn decode(&self, index_url: &str, bytes: &[u8], etag: Option<String>) -> Result<Pulled> {
        let mut store = index::parse(bytes)?;
        if store.extensions().is_empty() && !store.extension_list_url.is_empty() {
            let list_url = Url::parse(index_url).and_then(|base| base.join(&store.extension_list_url)).map_err(|_| not_a_store())?;
            let (list, _) = self.download(list_url.as_str()).await?;
            store.extension_list = Some(index::parse_list(&list)?);
        }
        let key = index::normalize_key(&store.signing_key).ok_or_else(|| {
            AppError::new(422, "no_signing_key", "The store publishes no signing key, so its extensions could not be checked")
        })?;
        let hashes = self.hashes(index_url).await;
        Ok(Pulled { store, key, etag, hashes })
    }

    /// The jar hashes a store publishes in `release-assets.json` beside its index.
    /// Optional: without the file, extensions are checked by signature alone.
    async fn hashes(&self, index_url: &str) -> HashMap<String, String> {
        #[derive(Deserialize)]
        struct Assets {
            jar: Option<Asset>,
        }
        #[derive(Deserialize)]
        struct Asset {
            sha256: String,
        }
        let Ok(url) = Url::parse(index_url).and_then(|base| base.join("release-assets.json")) else {
            return HashMap::new();
        };
        match self.fetch(url.as_str(), None).await {
            Ok(Fetched::Body { bytes, .. }) => match serde_json::from_slice::<HashMap<String, Assets>>(&bytes) {
                Ok(assets) => assets.into_iter().filter_map(|(pkg, a)| Some((pkg, a.jar?.sha256.to_lowercase()))).collect(),
                Err(e) => {
                    tracing::warn!("ignoring an unreadable {url}: {e}");
                    HashMap::new()
                }
            },
            Ok(_) => HashMap::new(),
            Err(e) => {
                tracing::warn!("no jar hashes for this store: {e}");
                HashMap::new()
            }
        }
    }
}

// ---------- helpers ----------

fn no_store() -> AppError {
    AppError::new(404, "unknown_store", "No such store")
}

fn blocked_store() -> AppError {
    AppError::new(403, "blocked", "The operator of this server has blocked this store.")
}

fn not_a_store() -> AppError {
    AppError::new(400, "not_a_store", "This is not a store: give the URL of its repo.json or index.pb")
}

fn http_url(url: &str) -> Result<Url> {
    let parsed = Url::parse(url.trim()).map_err(|_| AppError::bad_request("Not a valid URL"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::bad_request("Only http and https URLs are supported"));
    }
    Ok(parsed)
}

/// Package names end up in file names and URL paths, so nothing but these characters.
fn valid_pkg(pkg: &str) -> bool {
    !pkg.is_empty() && pkg.len() <= 200 && pkg.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_')
}

fn non_empty(text: &str) -> Option<&str> {
    (!text.is_empty()).then_some(text)
}

fn like_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// Hash and signature check of a downloaded jar. Returns its sha256.
fn check_jar(bytes: &[u8], key: &str, expected: Option<&str>) -> Result<String> {
    let sha256 = hex::encode(Sha256::digest(bytes));
    if expected.is_some_and(|want| !want.eq_ignore_ascii_case(&sha256)) {
        return Err(AppError::new(422, "sha256_mismatch", "The downloaded extension does not match the hash the store publishes"));
    }
    verify::verify_jar(bytes, key).map_err(|e| match e {
        VerifyError::Unsigned => {
            AppError::new(422, "unsigned_extension", "The extension is not signed, so it cannot be checked against the store's key")
        }
        VerifyError::WrongKey(found) => AppError::new(
            422,
            "signing_key_mismatch",
            format!("The extension is signed by another key than the store's: {}", found.join(", ")),
        ),
        VerifyError::Invalid(why) => AppError::new(422, "signature_invalid", format!("The extension's signature does not hold: {why}")),
    })?;
    Ok(sha256)
}

fn store_row(r: &Row) -> rusqlite::Result<StoreInfo> {
    let offered_key: Option<String> = r.get(7)?;
    Ok(StoreInfo {
        id: r.get(0)?,
        url: r.get(1)?,
        index_url: r.get(2)?,
        name: r.get(3)?,
        badge: r.get(4)?,
        website: r.get(5)?,
        signing_key: r.get(6)?,
        key_changed: offered_key.is_some(),
        offered_key,
        checked_at: r.get(8)?,
        updated_at: r.get(9)?,
        error: r.get(10)?,
        extensions: r.get(11)?,
    })
}

fn catalog_row(r: &Row) -> rusqlite::Result<CatalogItem> {
    let store_id: i64 = r.get(0)?;
    let version_code: i64 = r.get(4)?;
    let langs: String = r.get(8)?;
    let installed_code: Option<i64> = r.get(12)?;
    let installed_from: Option<i64> = r.get(13)?;
    Ok(CatalogItem {
        store_id,
        pkg: r.get(1)?,
        name: r.get(2)?,
        version_name: r.get(3)?,
        version_code,
        lib: r.get(5)?,
        content_warning: r.get(6)?,
        icon_url: r.get(7)?,
        languages: langs.split(',').filter(|lang| !lang.is_empty()).map(str::to_owned).collect(),
        source_count: r.get(9)?,
        installable: r.get(10)?,
        installed: installed_code.is_some(),
        installed_version: r.get(11)?,
        update_available: installed_from == Some(store_id) && installed_code.is_some_and(|code| version_code > code),
        sources: None,
    })
}

/// Replaces a store's catalog with a freshly pulled index. Run inside a transaction.
fn write_catalog(conn: &Connection, store_id: i64, pulled: &Pulled) -> Result<()> {
    conn.execute("DELETE FROM catalog_sources WHERE store_id = ?1", [store_id])?;
    conn.execute("DELETE FROM catalog WHERE store_id = ?1", [store_id])?;
    let mut add = conn.prepare(
        "INSERT OR REPLACE INTO catalog (store_id, pkg, name, version_name, version_code, lib, content_warning,
             jar_url, apk_url, icon_url, jar_sha256, langs, source_count, search)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )?;
    let mut add_source = conn.prepare(
        "INSERT OR REPLACE INTO catalog_sources (store_id, pkg, source_id, name, lang, home_url) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    for ext in pulled.store.extensions().iter().filter(|ext| valid_pkg(&ext.package_name)) {
        let resources = ext.resources.clone().unwrap_or_default();
        let langs: BTreeSet<&str> = ext.sources.iter().filter_map(|s| non_empty(&s.language)).collect();
        let names: BTreeSet<&str> = ext.sources.iter().map(|s| s.name.as_str()).collect();
        let mut search = format!("{} {}", ext.name, ext.package_name);
        for name in names {
            search.push(' ');
            search.push_str(name);
        }
        add.execute(params![
            store_id,
            ext.package_name,
            ext.name,
            ext.version_name,
            ext.version_code,
            ext.extension_lib,
            ext.content_warning,
            non_empty(&resources.jar_url),
            non_empty(&resources.apk_url),
            non_empty(&resources.icon_url),
            pulled.hashes.get(&ext.package_name),
            format!(",{},", langs.into_iter().collect::<Vec<_>>().join(",")),
            ext.sources.len() as i64,
            search.to_lowercase(),
        ])?;
        for source in &ext.sources {
            add_source.execute(params![
                store_id,
                ext.package_name,
                source.id,
                source.name,
                source.language,
                non_empty(&source.home_url)
            ])?;
        }
    }
    // The highest version the store ever listed stays known; see `install`.
    conn.execute(
        "INSERT INTO seen_versions (store_id, pkg, version_code) SELECT store_id, pkg, version_code FROM catalog WHERE store_id = ?1
         ON CONFLICT(store_id, pkg) DO UPDATE SET version_code = max(version_code, excluded.version_code)",
        [store_id],
    )?;
    let website = pulled.store.contact.as_ref().and_then(|contact| non_empty(&contact.website));
    conn.execute(
        "UPDATE stores SET name = ?2, badge = ?3, website = ?4, etag = ?5, offered_key = NULL, error = NULL,
             updated_at = ?6, checked_at = ?6 WHERE id = ?1",
        params![store_id, pulled.store.name, pulled.store.badge_label, website, pulled.etag, now()],
    )?;
    Ok(())
}

fn query_catalog(conn: &Connection, query: &CatalogQuery) -> Result<(Vec<CatalogItem>, i64)> {
    let mut filter = format!("{CATALOG_FROM} WHERE 1 = 1");
    let mut args: Vec<Sql> = Vec::new();
    if let Some(store) = query.store {
        filter += " AND c.store_id = ?";
        args.push(store.into());
    }
    let text = query.text.trim().to_lowercase();
    if !text.is_empty() {
        filter += " AND c.search LIKE ? ESCAPE '\\'";
        args.push(format!("%{}%", like_escape(&text)).into());
    }
    if !query.langs.is_empty() {
        filter += &format!(" AND ({})", vec!["c.langs LIKE ? ESCAPE '\\'"; query.langs.len()].join(" OR "));
        args.extend(query.langs.iter().map(|lang| Sql::from(format!("%,{},%", like_escape(lang)))));
    }
    let levels: &[i64] = if query.levels.is_empty() { &[0, 1, 2] } else { &query.levels };
    filter += &format!(" AND c.content_warning IN ({})", levels.iter().map(i64::to_string).collect::<Vec<_>>().join(","));
    match query.state.as_deref() {
        Some("installed") => filter += " AND e.pkg IS NOT NULL",
        Some("available") => filter += " AND e.pkg IS NULL",
        Some("updates") => filter += " AND e.store_id = c.store_id AND c.version_code > e.version_code",
        _ => {}
    }

    let total = conn.query_row(&format!("SELECT COUNT(*){filter}"), params_from_iter(args.iter()), |r| r.get(0))?;
    args.push(query.limit.clamp(1, 500).into());
    args.push(query.offset.max(0).into());
    let mut stmt = conn.prepare(&format!("{CATALOG_COLUMNS}{filter} ORDER BY c.name COLLATE NOCASE, c.pkg, c.store_id LIMIT ? OFFSET ?"))?;
    let items = stmt.query_map(params_from_iter(args.iter()), catalog_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok((items, total))
}

fn read_sources(conn: &Connection) -> Result<Vec<SourceInfo>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.pkg, s.name, s.lang, s.base_url, s.supports_latest, COALESCE(p.pinned, 0), COALESCE(p.hidden, 0), p.folder
         FROM sources s LEFT JOIN source_prefs p ON p.source_id = s.id
         ORDER BY s.name COLLATE NOCASE, s.lang, s.id",
    )?;
    let sources = stmt
        .query_map([], |r| {
            Ok(SourceInfo {
                id: r.get::<_, i64>(0)?.to_string(),
                pkg: r.get(1)?,
                name: r.get(2)?,
                lang: r.get(3)?,
                base_url: r.get(4)?,
                supports_latest: r.get(5)?,
                pinned: r.get(6)?,
                hidden: r.get(7)?,
                folder: r.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(sources)
}

/// Installed extensions with their sources and whether their store has a newer version.
fn read_installed(conn: &Connection, only: Option<&str>) -> Result<Vec<Installed>> {
    let mut sources: HashMap<String, Vec<SourceInfo>> = HashMap::new();
    for source in read_sources(conn)? {
        sources.entry(source.pkg.clone()).or_default().push(source);
    }
    let mut stmt = conn.prepare(
        "SELECT e.pkg, e.name, e.version_name, e.version_code, e.lib, e.content_warning, e.icon_url, e.store_id,
                c.version_name, c.version_code
         FROM extensions e LEFT JOIN catalog c ON c.store_id = e.store_id AND c.pkg = e.pkg
         WHERE (?1 IS NULL OR e.pkg = ?1) AND e.pkg != ?2
         ORDER BY e.name COLLATE NOCASE, e.pkg",
    )?;
    let installed = stmt
        .query_map([only, Some(LOCAL_PKG)], |r| {
            let pkg: String = r.get(0)?;
            let version_code: i64 = r.get(3)?;
            let offered: Option<i64> = r.get(9)?;
            let update_available = offered.is_some_and(|code| code > version_code);
            Ok(Installed {
                sources: sources.remove(&pkg).unwrap_or_default(),
                pkg,
                name: r.get(1)?,
                version_name: r.get(2)?,
                version_code,
                lib: r.get(4)?,
                content_warning: r.get(5)?,
                icon_url: r.get(6)?,
                store_id: r.get(7)?,
                update_available,
                available_version: r.get::<_, Option<String>>(8)?.filter(|_| update_available),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(installed)
}

/// Records what the host reports about an extension it has installed.
/// Takes over the version of an extension the host has now, where this database lists the extension.
fn follow(conn: &mut Connection, ext: &HostExtension) -> Result<()> {
    let mine: Option<(i64, Option<i64>, Option<String>)> = conn
        .query_row("SELECT version_code, store_id, icon_url FROM extensions WHERE pkg = ?1", [&ext.pkg], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    match mine {
        Some((code, store_id, icon_url)) if code != ext.version_code => save_installed(conn, ext, store_id, icon_url.as_deref()),
        _ => Ok(()),
    }
}

fn save_installed(conn: &mut Connection, ext: &HostExtension, store_id: Option<i64>, icon_url: Option<&str>) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO extensions (pkg, store_id, name, version_name, version_code, lib, content_warning, icon_url, installed_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
         ON CONFLICT(pkg) DO UPDATE SET store_id = excluded.store_id, name = excluded.name, version_name = excluded.version_name,
             version_code = excluded.version_code, lib = excluded.lib, content_warning = excluded.content_warning,
             icon_url = excluded.icon_url, updated_at = excluded.updated_at",
        params![ext.pkg, store_id, ext.name, ext.version_name, ext.version_code, ext.lib, ext.content_warning, icon_url, now()],
    )?;
    tx.execute("DELETE FROM sources WHERE pkg = ?1", [&ext.pkg])?;
    for source in &ext.sources {
        let Ok(id) = source.id.parse::<i64>() else { continue };
        tx.execute(
            "INSERT OR REPLACE INTO sources (id, pkg, name, lang, base_url, supports_latest) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, ext.pkg, source.name, source.lang, source.base_url, source.supports_latest],
        )?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use index::{ExtensionListMsg, ExtensionMsg, ResourcesMsg, SourceMsg, StoreMsg};

    fn extension(name: &str, warning: i32, langs: &[&str]) -> ExtensionMsg {
        ExtensionMsg {
            name: name.into(),
            package_name: format!("example.{}", name.to_lowercase()),
            resources: Some(ResourcesMsg { jar_url: "https://example.org/x.jar".into(), ..Default::default() }),
            version_code: 5,
            version_name: "1.6.5".into(),
            content_warning: warning,
            sources: langs
                .iter()
                .enumerate()
                .map(|(n, lang)| SourceMsg { id: n as i64 + 1, name: format!("{name} Site"), language: (*lang).into(), ..Default::default() })
                .collect(),
            ..Default::default()
        }
    }

    fn database() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&mut conn).unwrap();
        conn.execute("INSERT INTO stores (id, url, index_url, name, signing_key, added_at) VALUES (1, 'u', 'i', 'old name', 'k', 0)", [])
            .unwrap();
        let pulled = Pulled {
            store: StoreMsg {
                name: "Example".into(),
                extension_list: Some(ExtensionListMsg {
                    extensions: vec![
                        extension("Alpha", 1, &["en", "es"]),
                        extension("Beta", 3, &["en"]),
                        extension("Gamma_x", 2, &["ja"]),
                        ExtensionMsg { package_name: "../escape".into(), ..Default::default() },
                    ],
                }),
                ..Default::default()
            },
            key: "k".into(),
            etag: Some("\"v1\"".into()),
            hashes: HashMap::from([("example.alpha".to_owned(), "aa".to_owned())]),
        };
        write_catalog(&conn, 1, &pulled).unwrap();
        conn
    }

    fn names(conn: &Connection, query: CatalogQuery) -> Vec<String> {
        let (items, total) = query_catalog(conn, &CatalogQuery { limit: 50, ..query }).unwrap();
        assert_eq!(items.len() as i64, total);
        items.into_iter().map(|item| item.name).collect()
    }

    #[test]
    fn catalog_filters() {
        let conn = database();
        assert_eq!(names(&conn, CatalogQuery::default()), ["Alpha", "Gamma_x"], "NSFW is hidden unless asked for");
        assert_eq!(names(&conn, CatalogQuery { levels: vec![3], ..Default::default() }), ["Beta"]);
        assert_eq!(names(&conn, CatalogQuery { text: "ALPHA site".into(), ..Default::default() }), ["Alpha"]);
        assert_eq!(names(&conn, CatalogQuery { text: "a_x".into(), ..Default::default() }), ["Gamma_x"], "_ is not a wildcard");
        assert_eq!(names(&conn, CatalogQuery { text: "%".into(), ..Default::default() }), [] as [&str; 0]);
        assert_eq!(names(&conn, CatalogQuery { langs: vec!["es".into(), "ja".into()], ..Default::default() }), ["Alpha", "Gamma_x"]);
        assert_eq!(names(&conn, CatalogQuery { langs: vec!["e".into()], ..Default::default() }), [] as [&str; 0]);

        let (items, _) = query_catalog(&conn, &CatalogQuery { limit: 1, offset: 1, ..Default::default() }).unwrap();
        assert_eq!(items[0].name, "Gamma_x");
        assert_eq!(items[0].languages, ["ja"]);
        let name: String = conn.query_row("SELECT name FROM stores", [], |r| r.get(0)).unwrap();
        assert_eq!(name, "Example", "the store takes its name from the index");
    }

    #[test]
    fn the_highest_version_a_store_listed_stays_known() {
        let conn = database();
        let seen = |conn: &Connection| conn.query_row("SELECT version_code FROM seen_versions WHERE pkg = 'example.alpha'", [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(seen(&conn), 5);
        let older = Pulled {
            store: StoreMsg {
                extension_list: Some(ExtensionListMsg { extensions: vec![ExtensionMsg { version_code: 3, ..extension("Alpha", 1, &["en"]) }] }),
                ..Default::default()
            },
            key: "k".into(),
            etag: None,
            hashes: HashMap::new(),
        };
        write_catalog(&conn, 1, &older).unwrap();
        let listed: i64 = conn.query_row("SELECT version_code FROM catalog WHERE pkg = 'example.alpha'", [], |r| r.get(0)).unwrap();
        assert_eq!((listed, seen(&conn)), (3, 5), "an index that goes back does not take the mark with it");
    }

    #[test]
    fn installed_and_updates() {
        let mut conn = database();
        let ext = HostExtension {
            pkg: "example.alpha".into(),
            name: "Alpha".into(),
            version_name: "1.6.4".into(),
            version_code: 4,
            lib: "1.6".into(),
            content_warning: 1,
            sources: vec![HostSource { id: "-7".into(), name: "Alpha Site".into(), lang: "en".into(), base_url: None, supports_latest: true }],
        };
        save_installed(&mut conn, &ext, Some(1), None).unwrap();

        let installed = read_installed(&conn, None).unwrap();
        assert_eq!(installed.len(), 1);
        assert!(installed[0].update_available);
        assert_eq!(installed[0].available_version.as_deref(), Some("1.6.5"));
        assert_eq!(installed[0].sources[0].id, "-7");
        assert_eq!(names(&conn, CatalogQuery { state: Some("updates".into()), ..Default::default() }), ["Alpha"]);
        assert_eq!(names(&conn, CatalogQuery { state: Some("available".into()), ..Default::default() }), ["Gamma_x"]);

        save_installed(&mut conn, &HostExtension { version_code: 5, sources: vec![], ..ext }, Some(1), None).unwrap();
        assert!(!read_installed(&conn, Some("example.alpha")).unwrap()[0].update_available);
        assert!(read_sources(&conn).unwrap().is_empty(), "sources follow the installed version");
        assert!(names(&conn, CatalogQuery { state: Some("updates".into()), ..Default::default() }).is_empty());
    }

    #[test]
    fn input_checks() {
        assert!(valid_pkg("eu.kanade.tachiyomi.extension.all.example"));
        assert!(!valid_pkg("../../etc"));
        assert!(!valid_pkg(""));
        assert!(http_url("https://example.org/repo.json").is_ok());
        assert!(http_url("file:///etc/passwd").is_err());
        assert!(http_url("not a url").is_err());
    }
}
