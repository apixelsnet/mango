//! Word of new chapters, sent where an account asks: an ntfy topic, a Discord channel's
//! webhook, or any address that takes a JSON post.
//!
//! The address is the account's to name, so it is fetched as a store is: over HTTPS and
//! from public addresses only, unless the owner allowed local ones (`MANGO_STORES_LOCAL`).
//! See `net.rs`.

use crate::{
    db::Db,
    error::{AppError, Result},
    net,
    push::{Note, Push},
    settings,
};
use reqwest::{Client, Url};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::json;
use std::time::Duration;

/// Where word goes. Empty or unset: nowhere.
pub const URL: &str = "notify.url";
/// How it is put: ntfy, discord or webhook.
pub const KIND: &str = "notify.kind";
/// Chapters named one by one in a message; the rest are counted.
const NAMED: usize = 20;
/// The same on a phone's notification, which shows a few lines.
const PUSHED: usize = 5;

/// One new chapter, as a webhook gets it.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Fresh {
    pub manga_id: i64,
    pub title: String,
    pub chapter_id: i64,
    pub chapter: String,
}

#[derive(Clone)]
pub struct Notifier {
    db: Db,
    http: Client,
    local: bool,
    /// The account's devices that asked for Web Push.
    pub push: Push,
}

impl Notifier {
    pub fn new(db: Db, local: bool) -> Result<Self> {
        let builder = Client::builder()
            .user_agent(concat!("Mango/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20));
        let http = net::guard(builder, local).build().map_err(|e| AppError::internal(e.to_string()))?;
        Ok(Self { push: Push::new(db.clone())?, db, http, local })
    }

    /// Tells of the chapters a library update found: at the address set, and on every
    /// device that turned on notifications. Without either, or without chapters, nothing
    /// is sent. A failure is logged: an update does not fail for it.
    pub async fn chapters(&self, ids: Vec<i64>) {
        if ids.is_empty() {
            return;
        }
        let found = match self.db.call(move |conn| Ok((target(conn), fresh(conn, &ids)?))).await {
            Ok(found) => found,
            Err(e) => return tracing::warn!("notifications: {e}"),
        };
        let (target, list) = found;
        if list.is_empty() {
            return;
        }
        let title = format!("{} new {}", list.len(), if list.len() == 1 { "chapter" } else { "chapters" });
        if let Some((url, kind)) = target {
            if let Err(e) = self.send(&url, &kind, &title, &list).await {
                tracing::warn!("notifications: {}", e.message);
            }
        }
        let body = message(&title, &list[..list.len().min(PUSHED)]);
        let body = if list.len() > PUSHED { format!("{body}\nand {} more", list.len() - PUSHED) } else { body };
        if let Err(e) = self.push.send(&Note { title: &title, body: &body, url: "#/updates" }).await {
            tracing::warn!("push: {}", e.message);
        }
    }

    /// Sends a message that says only that it arrived, so the owner can try an address.
    pub async fn test(&self) -> Result<()> {
        let (url, kind) = self
            .db
            .call(|conn| Ok(target(conn)))
            .await?
            .ok_or_else(|| AppError::bad_request("Set an address for notifications first"))?;
        self.send(&url, &kind, "Mango can reach you here", &[]).await
    }

    async fn send(&self, url: &str, kind: &str, title: &str, list: &[Fresh]) -> Result<()> {
        let to = Url::parse(url).map_err(|_| AppError::bad_request("The notification address is not an address"))?;
        if !matches!(to.scheme(), "https" | "http") || net::refusal(&to, self.local).is_some() {
            return Err(AppError::bad_request(
                "Notifications go to HTTPS addresses on the internet. MANGO_STORES_LOCAL=on allows this machine's network and plain HTTP.",
            ));
        }
        let text = message(title, list);
        let request = match kind {
            "discord" => self.http.post(to).json(&json!({ "username": "Mango", "content": text.chars().take(1900).collect::<String>() })),
            "webhook" => self.http.post(to).json(&json!({ "event": "chapters", "count": list.len(), "chapters": list })),
            // ntfy takes the text as the body and the rest as headers, which must be plain ASCII.
            _ => self.http.post(to).header("Title", ascii(title)).header("Tags", "books").body(text),
        };
        let answer = request.send().await.map_err(|e| AppError::new(502, "notify_failed", format!("The address did not answer: {}", net::told(&e))))?;
        if !answer.status().is_success() {
            return Err(AppError::new(502, "notify_failed", format!("The address answered {}", answer.status().as_u16())));
        }
        Ok(())
    }
}

/// The address and the kind of message, when an address is set.
fn target(conn: &Connection) -> Option<(String, String)> {
    let url = settings::get::<String>(conn, URL).map(|url| url.trim().to_owned()).filter(|url| !url.is_empty())?;
    Some((url, settings::get::<String>(conn, KIND).unwrap_or_else(|| "ntfy".into())))
}

/// The chapters named, with their titles; a chapter of a merged title under the title it was merged into.
fn fresh(conn: &Connection, ids: &[i64]) -> Result<Vec<Fresh>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT h.id, h.title, c.id, c.name FROM chapters c JOIN manga m ON m.id = c.manga_id JOIN manga h ON h.id = COALESCE(m.merged_into, m.id)
         WHERE c.id IN ({}) ORDER BY h.title COLLATE NOCASE, c.source_order DESC",
        crate::titles::id_list(ids)
    ))?;
    let list = stmt
        .query_map([], |r| Ok(Fresh { manga_id: r.get(0)?, title: r.get(1)?, chapter_id: r.get(2)?, chapter: r.get(3)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(list)
}

/// The message as text: a line a chapter, the first few, then how many more.
fn message(title: &str, list: &[Fresh]) -> String {
    let mut lines: Vec<String> = list.iter().take(NAMED).map(|fresh| format!("{} — {}", fresh.title, fresh.chapter)).collect();
    if list.len() > NAMED {
        lines.push(format!("and {} more", list.len() - NAMED));
    }
    if lines.is_empty() {
        return title.to_owned();
    }
    lines.join("\n")
}

/// Text for a header: what is not ASCII goes.
fn ascii(text: &str) -> String {
    text.chars().filter(|c| c.is_ascii() && !c.is_ascii_control()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_name_a_few_and_count_the_rest() {
        let list: Vec<Fresh> = (0..23).map(|i| Fresh { manga_id: 1, title: "Alpha".into(), chapter_id: i, chapter: format!("Chapter {i}") }).collect();
        let text = message("23 new chapters", &list);
        assert_eq!(text.lines().count(), NAMED + 1);
        assert!(text.starts_with("Alpha — Chapter 0") && text.ends_with("and 3 more"));
        assert_eq!(message("Mango can reach you here", &[]), "Mango can reach you here");
        assert_eq!(ascii("Ä 3 new\nchapters"), " 3 newchapters");
    }

    #[test]
    fn the_address_is_read_from_the_settings() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&mut conn).unwrap();
        assert_eq!(target(&conn), None);
        settings::set(&conn, URL, &json!(" https://ntfy.example.org/mango ")).unwrap();
        assert_eq!(target(&conn), Some(("https://ntfy.example.org/mango".into(), "ntfy".into())));
        conn.execute_batch(
            "INSERT INTO manga (id, source_id, url, title) VALUES (1, 7, '/a', 'Alpha'), (2, 8, '/b', 'Alpha elsewhere');
             UPDATE manga SET merged_into = 1 WHERE id = 2;
             INSERT INTO chapters (id, manga_id, url, name, source_order, fetched_at) VALUES (10, 2, '/b/1', 'One', 0, 0);",
        )
        .unwrap();
        assert_eq!(fresh(&conn, &[10]).unwrap(), [Fresh { manga_id: 1, title: "Alpha".into(), chapter_id: 10, chapter: "One".into() }]);
    }

    #[tokio::test]
    async fn this_machine_is_not_reached() {
        let notifier = Notifier::new(Db::open(std::path::Path::new(":memory:")).unwrap(), false).unwrap();
        for url in ["http://ntfy.example.org/t", "https://127.0.0.1/t", "https://10.0.0.2/t", "file:///etc/passwd"] {
            assert_eq!(notifier.send(url, "ntfy", "t", &[]).await.unwrap_err().status, 400, "{url}");
        }
    }
}
