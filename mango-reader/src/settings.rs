//! Preferences: a small key-value table. Values are JSON, so the UI can keep
//! whatever shape it needs (the theme, reader defaults, update options).

use crate::error::{AppError, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

const MAX_VALUE: usize = 64 * 1024;

pub fn all(conn: &Connection) -> Result<Map<String, Value>> {
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows.into_iter().filter_map(|(key, value)| Some((key, serde_json::from_str(&value).ok()?))).collect())
}

/// Stores the given keys. A null value removes the key.
pub fn put(conn: &mut Connection, values: &Map<String, Value>) -> Result<()> {
    let tx = conn.transaction()?;
    for (key, value) in values {
        if key.is_empty() || key.len() > 64 || !key.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) {
            return Err(AppError::bad_request(format!("Not a setting name: {key}")));
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

/// Stores one value, without the checks `put` makes on what comes from outside.
pub fn set(conn: &Connection, key: &str, value: &Value) -> Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value.to_string()],
    )?;
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
}
