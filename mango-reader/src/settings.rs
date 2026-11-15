//! Preferences: a small key-value table. Values are JSON, so the UI can keep
//! whatever shape it needs (the theme, reader defaults, update options).

use crate::error::{AppError, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

const MAX_VALUE: usize = 64 * 1024;

/// Keys kept in this table that are not the UI's: the solver and the proxy, with the
/// proxy's password, and the header that signs notifications in. Only their own routes read
/// and write them, with their checks; `all` leaves them out and `put` refuses them.
const RESERVED: [&str; 3] = [crate::host::Solver::KEY, crate::host::Proxy::KEY, crate::notify::HEADER];

pub fn all(conn: &Connection) -> Result<Map<String, Value>> {
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows.into_iter().filter(|(key, _)| !RESERVED.contains(&key.as_str())).filter_map(|(key, value)| Some((key, serde_json::from_str(&value).ok()?))).collect())
}

/// Stores the given keys. A null value removes the key.
pub fn put(conn: &mut Connection, values: &Map<String, Value>) -> Result<()> {
    let tx = conn.transaction()?;
    for (key, value) in values {
        if key.is_empty() || key.len() > 64 || !key.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) {
            return Err(AppError::bad_request(format!("Not a setting name: {key}")));
        }
        if RESERVED.contains(&key.as_str()) {
            return Err(AppError::bad_request(format!("{key} is set on its own page")));
        }
        if value.is_null() {
            tx.execute("DELETE FROM settings WHERE key = ?1", [key])?;
            continue;
        }
        let text = value.to_string();
        if text.len() > MAX_VALUE {
            return Err(AppError::bad_request(format!("The value of {key} is too large")));
        }
        tx.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, text])?;
    }
    tx.commit()?;
    Ok(())
}

/// Keys a backup leaves out and an import never takes. This Mango's own state rather than a
/// choice (when updates and backups last ran, tracker changes waiting, a paused queue); keys
/// that name rows by their number here (the update category lists); and keys that are not
/// the reader's alone to carry across. The update interval is one: a hosted plan may hold
/// it to a minimum, which is checked where settings are changed, not in a backup.
const NOT_BACKED_UP: [&str; 7] =
    ["backup.lastRun", "updates.lastRun", "tracker.pending", "updates.categories", "updates.excluded", "downloads.paused", "updates.hours"];

/// Where notifications go is left out of a backup and never taken from one: a file someone
/// else made would otherwise send this account's notices to its maker.
const NOT_BACKED_UP_PREFIX: &str = "notify.";

fn carried(key: &str) -> bool {
    !NOT_BACKED_UP.contains(&key) && !key.starts_with(NOT_BACKED_UP_PREFIX) && !RESERVED.contains(&key)
}

/// The settings a backup carries: the reader's choices, without the reserved keys and
/// without this Mango's own state.
pub fn backed_up(conn: &Connection) -> Result<Map<String, Value>> {
    let mut values = all(conn)?;
    values.retain(|key, _| carried(key));
    Ok(values)
}

/// Takes settings from a backup. Like the rest of an import it never overwrites: only keys
/// this Mango has no value for are set. The checks `put` makes hold, and what does not
/// pass is left out rather than failing the import. Answers how many were set.
pub fn restore(conn: &Connection, values: &Map<String, Value>) -> Result<usize> {
    let mut set_now = 0;
    for (key, value) in values {
        let fit = !key.is_empty() && key.len() <= 64 && key.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
        let text = value.to_string();
        if !fit || value.is_null() || !carried(key) || text.len() > MAX_VALUE {
            continue;
        }
        set_now += conn.execute("INSERT OR IGNORE INTO settings (key, value) VALUES (?1, ?2)", params![key, text])?;
    }
    Ok(set_now)
}

/// Removes one key, without the checks `put` makes on what comes from outside.
pub fn forget(conn: &Connection, key: &str) -> Result<()> {
    conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
    Ok(())
}

/// Stores one value, without the checks `put` makes on what comes from outside.
pub fn set(conn: &Connection, key: &str, value: &Value) -> Result<()> {
    conn.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value.to_string()])?;
    Ok(())
}

/// One setting, or None when it is unset or has another shape than asked for.
pub fn get<T: DeserializeOwned>(conn: &Connection, key: &str) -> Option<T> {
    let text: String = conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional().ok()??;
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trip() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&mut conn).unwrap();
        let values = json!({ "theme": { "spot": "#FFC400" }, "updates.hours": 6 });
        put(&mut conn, values.as_object().unwrap()).unwrap();
        assert_eq!(get::<i64>(&conn, "updates.hours"), Some(6));
        assert_eq!(get::<String>(&conn, "updates.hours"), None, "a value of another shape reads as unset");
        assert_eq!(all(&conn).unwrap().len(), 2);

        put(&mut conn, json!({ "updates.hours": null }).as_object().unwrap()).unwrap();
        assert_eq!(get::<i64>(&conn, "updates.hours"), None);
        assert!(put(&mut conn, json!({ "bad key": 1 }).as_object().unwrap()).is_err());
    }

    #[test]
    fn proxy_and_solver_stay_out() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&mut conn).unwrap();
        set(&conn, "proxy", &json!({ "url": "socks5://p:1080", "username": "u", "password": "secret" })).unwrap();
        set(&conn, "solver", &json!({ "url": "http://solver:8191" })).unwrap();
        set(&conn, "theme", &json!("dark")).unwrap();
        let shown = all(&conn).unwrap();
        assert_eq!(shown.keys().collect::<Vec<_>>(), ["theme"], "the proxy's password never leaves through the settings");
        for key in ["proxy", "solver"] {
            assert!(put(&mut conn, json!({ key: { "url": "http://10.0.0.1" } }).as_object().unwrap()).is_err(), "{key}");
        }
        assert_eq!(get::<Value>(&conn, "proxy").unwrap()["password"], "secret", "its own route still reads it");
    }

    #[test]
    fn a_backup_never_brings_where_notices_go() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&mut conn).unwrap();
        let from_a_file = json!({
            "notify.url": "https://ntfy.example.org/someone-else", "notify.kind": "webhook", "updates.hours": 1,
            "solver": { "url": "http://10.0.0.1" }, "reader.mode": "rtl", "bad key!": 1
        });
        assert_eq!(restore(&conn, from_a_file.as_object().unwrap()).unwrap(), 1, "only the reading mode is taken");
        assert!(get::<String>(&conn, "notify.url").is_none() && get::<i64>(&conn, "updates.hours").is_none());
        assert_eq!(get::<String>(&conn, "reader.mode").as_deref(), Some("rtl"));
    }
}
