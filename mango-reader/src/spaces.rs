//! One library per account.
//!
//! The owner's is the data directory itself, as it was before there were accounts.
//! Every other account has a folder under `users/` with a database, a cache and
//! downloads of its own. An account also has its own stores and its own list of
//! installed extensions. Only the extension host is shared: it holds every
//! extension any account has installed.

use crate::{
    api::AppState,
    auth::{Auth, Who, OWNER},
    backups::Backups,
    db::Db,
    downloads::Downloads,
    error::{AppError, Result},
    host::Host,
    images::Images,
    import::Importer,
    local::Local,
    notify::Notifier,
    store::{blocklist::Blocklist, Fleet, Rules, Stores},
    titles::Titles,
    trackers::Trackers,
    updates::Updates,
    version::Releases,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::task::JoinHandle;

/// What every library is built from.
pub struct Shared {
    /// The owner's database. Accounts, sessions and the solver's address are in it.
    pub db: Db,
    pub releases: Arc<Releases>,
    pub host: Arc<Host>,
    pub local: Local,
    pub data_dir: PathBuf,
    pub cache_mb: u64,
    pub stores_refresh_hours: u64,
    pub store_rules: Rules,
    pub blocklist: Arc<Blocklist>,
    /// Downloads stop while the disk has less than this free, in megabytes. 0: never.
    pub keep_free_mb: u64,
    /// The download queue waits while this file has a line `full`.
    pub hold_file: Option<PathBuf>,
}

struct Open {
    app: AppState,
    /// The library's background work: the cache sweep, the download queue, the update and
    /// backup schedules, and keeping its stores and extensions current and clear of the blocklist.
    tasks: Vec<JoinHandle<()>>,
}

#[derive(Clone)]
pub struct Spaces {
    pub auth: Arc<Auth>,
    shared: Arc<Shared>,
    fleet: Fleet,
    open: Arc<Mutex<HashMap<i64, Open>>>,
}

impl Spaces {
    /// Opens the library of every account, and clears away what removed accounts left.
    pub fn new(auth: Arc<Auth>, shared: Shared) -> Result<Self> {
        let spaces = Self { auth, shared: Arc::new(shared), fleet: Fleet::default(), open: Arc::default() };
        let ids = spaces.auth.ids();
        for id in &ids {
            spaces.open(*id)?;
        }
        let users = spaces.shared.data_dir.join("users");
        for entry in std::fs::read_dir(&users).into_iter().flatten().flatten() {
            let Some(id) = entry.file_name().to_str().and_then(|name| name.parse::<i64>().ok()) else { continue };
            if !ids.contains(&id) {
                leftover(&users, id, &entry.path());
            }
        }
        Ok(spaces)
    }

    fn dir(&self, id: i64) -> PathBuf {
        match id {
            OWNER => self.shared.data_dir.clone(),
            _ => self.shared.data_dir.join("users").join(id.to_string()),
        }
    }

    /// Opens the library of an account that was just added. A folder that is there
    /// already was someone else's, and is not taken over.
    pub fn create(&self, id: i64) -> Result<()> {
        if id != OWNER && self.dir(id).exists() {
            return Err(AppError::internal("there is a library under this account's number already"));
        }
        self.open(id)
    }

    /// Opens an account's library and starts its background work. Call it once per account.
    pub fn open(&self, id: i64) -> Result<()> {
        let shared = &self.shared;
        let dir = self.dir(id);
        let db = match id {
            OWNER => shared.db.clone(),
            _ => {
                std::fs::create_dir_all(&dir)?;
                // Other users of the machine have no business in an account's folder.
                crate::db::owner_only(&shared.data_dir.join("users"));
                Db::open(&dir.join("mango.db"))?
            }
        };
        // The local source needs no extension and no host: every library has it from the start.
        db.now(|conn| Local::register(conn))?;
        let stores = Stores::new(db.clone(), shared.host.clone(), self.fleet.clone(), id, shared.store_rules, shared.blocklist.clone())?;
        let titles = Titles::new(db.clone(), shared.host.clone(), shared.local.clone());
        let images = Images::new(db.clone(), shared.host.clone(), shared.local.clone(), &dir, shared.cache_mb);
        let downloads = Downloads::new(db.clone(), images.clone(), &dir, shared.keep_free_mb, shared.hold_file.clone());
        let trackers = Trackers::new(db.clone())?;
        let notifier = Notifier::new(db.clone(), shared.store_rules.local)?;
        let updates = Updates::new(db.clone(), titles.clone(), downloads.clone(), notifier.clone(), trackers.clone());
        let backups = Backups::new(db.clone(), &dir);
        let tasks = vec![
            tokio::spawn(images.clone().sweep_forever()),
            tokio::spawn(downloads.clone().run()),
            tokio::spawn(updates.clone().schedule()),
            tokio::spawn(backups.clone().schedule()),
            tokio::spawn(stores.clone().keep_in_sync(shared.host.subscribe())),
            tokio::spawn(stores.clone().refresh_on_schedule(shared.stores_refresh_hours)),
            tokio::spawn(stores.clone().keep_blocked_out()),
        ];
        let app = AppState {
            me: Who::default(),
            importer: Importer::new(db.clone()),
            trackers,
            notifier,
            backups,
            auth: self.auth.clone(),
            releases: shared.releases.clone(),
            host: shared.host.clone(),
            stores,
            titles,
            images,
            downloads,
            updates,
            local_dir: shared.local.root().display().to_string(),
            db: db.clone(),
        };
        self.fleet.join(id, db);
        self.open.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(id, Open { app, tasks });
        Ok(())
    }

    /// The owner's database: the one place for what all accounts share.
    pub fn shared(&self) -> &Db {
        &self.shared.db
    }

    /// The library of the account a request comes from.
    pub fn of(&self, who: Who) -> Result<AppState> {
        let open = self.open.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let space = open.get(&who.id).ok_or_else(|| AppError::internal("this account has no library"))?;
        Ok(AppState { me: who, ..space.app.clone() })
    }

    /// Stops an account's background work and deletes its library. Not for the owner's.
    pub async fn close(&self, id: i64) {
        if id == OWNER {
            return;
        }
        let Some(space) = self.open.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&id) else { return };
        space.tasks.iter().for_each(JoinHandle::abort);
        // Its extensions leave the host, unless another account has them too.
        space.app.stores.remove_all().await;
        self.fleet.leave(id);
        let dir = self.dir(id);
        // Marks the folder as one to delete, so that a start before it is gone finishes the job.
        // A folder without the mark is never deleted at start: see `leftover`.
        if let Err(e) = std::fs::write(dir.join(REMOVED), b"") {
            tracing::warn!("could not mark {} as removed: {e}", dir.display());
        }
        drop(space);
        // The database stays open until the stopped work lets go of it. What is still
        // there after a few tries goes at the next start.
        tokio::spawn(async move {
            for _ in 0..5 {
                tokio::time::sleep(Duration::from_secs(1)).await;
                let gone = tokio::task::spawn_blocking({
                    let dir = dir.clone();
                    move || std::fs::remove_dir_all(&dir).is_ok() || !dir.exists()
                });
                if gone.await.unwrap_or(false) {
                    return;
                }
            }
            tracing::warn!("could not delete {} yet; it goes at the next start", dir.display());
        });
    }
}

/// The file that marks an account's folder as removed: `close` writes it before deleting.
const REMOVED: &str = "removed";

/// Deals with a folder under `users/` whose account is not in the database. One marked
/// as removed is deleted. Any other is moved aside and kept: the database may be an
/// older copy put back, or a new one beside an old `users/`, and that folder holds
/// someone's library. Moving it away keeps its number free for `create`.
fn leftover(users: &std::path::Path, id: i64, path: &std::path::Path) {
    if path.join(REMOVED).exists() {
        if let Err(e) = std::fs::remove_dir_all(path) {
            tracing::warn!("could not delete {}: {e}", path.display());
        }
        return;
    }
    let aside = users.join(format!("{id}.unknown-{}", crate::db::now()));
    match std::fs::rename(path, &aside) {
        Ok(()) => tracing::warn!("{} belongs to no account; moved to {} and kept", path.display(), aside.display()),
        Err(e) => tracing::warn!("{} belongs to no account and could not be moved aside: {e}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_removed_folders_are_deleted() {
        let users = std::env::temp_dir().join(format!("mango-users-{}", crate::db::now() ^ i64::from(std::process::id())));
        for (id, marked) in [(2, true), (3, false)] {
            let dir = users.join(id.to_string());
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("mango.db"), b"library").unwrap();
            if marked {
                std::fs::write(dir.join(REMOVED), b"").unwrap();
            }
            leftover(&users, id, &dir);
        }
        let names: Vec<String> = std::fs::read_dir(&users).unwrap().flatten().map(|e| e.file_name().into_string().unwrap()).collect();
        assert_eq!(names.len(), 1, "{names:?}");
        assert!(names[0].starts_with("3.unknown-"), "{names:?}");
        assert!(users.join(&names[0]).join("mango.db").is_file(), "the library is kept");
        assert!(names[0].parse::<i64>().is_err(), "a folder moved aside is not taken for an account");
        let _ = std::fs::remove_dir_all(&users);
    }
}
