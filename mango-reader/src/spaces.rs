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
    db::Db,
    downloads::Downloads,
    error::{AppError, Result},
    host::Host,
    images::Images,
    import::Importer,
    local::Local,
    store::{Fleet, Rules, Stores},
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
}

struct Open {
    app: AppState,
    /// The library's background work: the cache sweep, the download queue, the update
    /// schedule, and keeping its stores and extensions current.
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
        for entry in std::fs::read_dir(spaces.shared.data_dir.join("users")).into_iter().flatten().flatten() {
            let left = entry.file_name().to_str().and_then(|name| name.parse::<i64>().ok()).is_some_and(|id| !ids.contains(&id));
            if left {
                let _ = std::fs::remove_dir_all(entry.path());
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
        let stores = Stores::new(db.clone(), shared.host.clone(), self.fleet.clone(), id, shared.store_rules)?;
        let titles = Titles::new(db.clone(), shared.host.clone(), shared.local.clone());
        let images = Images::new(db.clone(), shared.host.clone(), shared.local.clone(), &dir, shared.cache_mb);
        let downloads = Downloads::new(db.clone(), images.clone(), &dir);
        let updates = Updates::new(db.clone(), titles.clone(), downloads.clone());
        let tasks = vec![
            tokio::spawn(images.clone().sweep_forever()),
            tokio::spawn(downloads.clone().run()),
            tokio::spawn(updates.clone().schedule()),
            tokio::spawn(stores.clone().keep_in_sync(shared.host.subscribe())),
            tokio::spawn(stores.clone().refresh_on_schedule(shared.stores_refresh_hours)),
        ];
        let app = AppState {
            me: Who::default(),
            importer: Importer::new(db.clone()),
            trackers: Trackers::new(db.clone())?,
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
