//! SQLite. One connection behind a lock; calls run off the async threads.

use crate::error::{AppError, Result};
use rusqlite::{Connection, OptionalExtension};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Applied in order. `PRAGMA user_version` counts how many a database has seen.
/// Never edit one that has shipped; add the next file.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/001_init.sql"),
    include_str!("../migrations/002_titles.sql"),
    include_str!("../migrations/003_library.sql"),
    include_str!("../migrations/004_updates.sql"),
    include_str!("../migrations/005_import.sql"),
    include_str!("../migrations/006_auth.sql"),
    include_str!("../migrations/007_trackers.sql"),
    include_str!("../migrations/008_users.sql"),
    include_str!("../migrations/009_reader.sql"),
    include_str!("../migrations/010_versions.sql"),
    include_str!("../migrations/011_invites.sql"),
    include_str!("../migrations/012_requests.sql"),
    include_str!("../migrations/013_push.sql"),
    include_str!("../migrations/014_read_at.sql"),
    include_str!("../migrations/015_sign_in_sync.sql"),
];

/// Now, in epoch seconds: the unit of every time the database stores.
pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64)
}

#[derive(Clone)]
pub struct Db(Arc<Mutex<Connection>>, Arc<PathBuf>);

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        // The file is made before SQLite opens it, so that it is closed to other users
        // from the first moment; the write-ahead log takes its permissions from it.
        if path.to_str() != Some(":memory:") {
            let _ = std::fs::OpenOptions::new().write(true).create_new(true).open(path);
            for beside in ["", "-wal", "-shm"] {
                owner_only(Path::new(&format!("{}{beside}", path.display())));
            }
        }
        let mut conn = Connection::open(path)?;
        // Long enough to sit out an import, which writes from a connection of its own.
        conn.busy_timeout(Duration::from_secs(30))?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;")?;
        migrate(&mut conn)?;
        Ok(Self(Arc::new(Mutex::new(conn)), Arc::new(path.to_path_buf())))
    }

    /// Another connection to the same database, for long work that must not hold up the
    /// shared one. With the write-ahead log, readers carry on while it writes.
    pub fn second(&self) -> Result<Connection> {
        let conn = Connection::open(self.1.as_path())?;
        conn.busy_timeout(Duration::from_secs(60))?;
        conn.execute_batch("PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;")?;
        Ok(conn)
    }

    /// Runs `work` on the connection, on this thread. For start-up, before anything is served.
    pub fn now<T>(&self, work: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        work(&mut self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
    }

    pub async fn call<T, F>(&self, work: F) -> Result<T>
    where
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = db.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            work(&mut conn)
        })
        .await
        .map_err(|e| AppError::internal(format!("database task: {e}")))?
    }
}

/// Closes a file or folder to every user but the one Mango runs as: the databases hold
/// password hashes and tracker sign-ins. Where the system knows no such permissions,
/// nothing is done.
pub fn owner_only(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Ok(found) = std::fs::metadata(path) else { return };
        let mode = if found.is_dir() { 0o700 } else { 0o600 };
        if found.permissions().mode() & 0o777 != mode {
            if let Err(why) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)) {
                tracing::warn!("could not close {} to other users: {why}", path.display());
            }
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Writes a file that only the user Mango runs as can read.
pub fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    std::fs::write(path, "")?;
    owner_only(path);
    std::fs::write(path, text)
}

/// One value the server keeps to itself; see the `state` table.
pub fn state(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn.query_row("SELECT value FROM state WHERE key = ?1", [key], |r| r.get(0)).optional()?)
}

pub fn set_state(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute("INSERT INTO state (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [key, value])?;
    Ok(())
}

pub fn forget_state(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM state WHERE key = ?1", [key])?;
    Ok(())
}

pub(crate) fn migrate(conn: &mut Connection) -> Result<()> {
    let seen = conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as usize;
    if seen > MIGRATIONS.len() {
        return Err(AppError::internal("the database was written by a newer Mango"));
    }
    for (done, sql) in MIGRATIONS.iter().enumerate().skip(seen) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute_batch(&format!("PRAGMA user_version = {}", done + 1))?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_once() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        migrate(&mut conn).unwrap();
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version as usize, MIGRATIONS.len());
        conn.execute("INSERT INTO stores (url, index_url, name, signing_key, added_at) VALUES ('u', 'i', 'n', 'k', 0)", [])
            .unwrap();
    }

    #[test]
    fn an_old_password_becomes_the_first_account() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&MIGRATIONS[..7].concat()).unwrap();
        conn.execute_batch("PRAGMA user_version = 7; INSERT INTO state VALUES ('auth.password', 'hash')").unwrap();
        migrate(&mut conn).unwrap();
        let owner: (i64, String, i64) = conn.query_row("SELECT id, password, admin FROM users", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!(owner, (1, "hash".into(), 1));
        assert_eq!(state(&conn, "auth.password").unwrap(), None);
    }
}
