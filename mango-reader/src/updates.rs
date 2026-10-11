//! Library updates: ask each library title's source for new chapters, on a schedule
//! or on request. Sources run side by side, a few at a time; within one source the
//! titles go one after another with a pause, so no source is hammered. One failing
//! title or source never stops the rest.

use crate::{
    db::{now, Db},
    downloads::Downloads,
    error::Result,
    settings,
    titles::{cover_url, Titles},
};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::{
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};
use tokio::{sync::Semaphore, task::JoinSet};

pub const HOURS: &str = "updates.hours";
pub const SKIP_COMPLETED: &str = "updates.skipCompleted";
pub const SKIP_UNREAD: &str = "updates.skipUnread";
/// Category ids. Set and not empty: only titles in one of these are updated.
pub const ONLY: &str = "updates.categories";
/// Category ids whose titles are never updated.
pub const NEVER: &str = "updates.excluded";
const LAST_RUN: &str = "updates.lastRun";

const DEFAULT_HOURS: i64 = 12;
const SOURCES_AT_ONCE: usize = 4;
const BETWEEN_TITLES: Duration = Duration::from_secs(1);
/// How far back the "new chapters" count for the menu looks.
const FRESH_SECS: i64 = 7 * 86_400;

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub running: bool,
    pub done: usize,
    pub total: usize,
    /// New chapters found by this run.
    pub found: usize,
    pub failed: usize,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateItem {
    pub chapter_id: i64,
    pub chapter_name: String,
    pub fetched_at: i64,
    pub read: bool,
    pub downloaded: bool,
    pub manga_id: i64,
    pub title: String,
    pub cover: Option<String>,
    pub source_name: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateError {
    pub manga_id: i64,
    pub title: String,
    pub error: String,
    pub at: i64,
}

#[derive(Serialize)]
pub struct UpdatesView {
    pub items: Vec<UpdateItem>,
    pub errors: Vec<UpdateError>,
    /// Unread chapters found in the last week, for the menu.
    pub fresh: i64,
    pub status: Status,
}

#[derive(Clone)]
pub struct Updates {
    db: Db,
    titles: Titles,
    downloads: Downloads,
    status: Arc<StdMutex<Status>>,
}

impl Updates {
    pub fn new(db: Db, titles: Titles, downloads: Downloads) -> Self {
        Self { db, titles, downloads, status: Arc::default() }
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub async fn view(&self, limit: i64) -> Result<UpdatesView> {
        let status = self.status();
        self.db
            .call(move |conn| {
                let (items, errors) = list(conn, limit)?;
                Ok(UpdatesView { items, errors, fresh: fresh(conn)?, status })
            })
            .await
    }

    /// Starts a run unless one is going. Answers at once; the run carries on by itself.
    pub fn start(&self) -> Status {
        {
            let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
            if status.running {
                return status.clone();
            }
            *status = Status { running: true, started_at: Some(now()), ..Status::default() };
        }
        let this = self.clone();
        tokio::spawn(async move { this.run().await });
        self.status()
    }

    /// Starts a run whenever the owner's interval has passed. Never returns.
    pub async fn schedule(self) {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let due = self
                .db
                .call(|conn| {
                    let hours = settings::get::<i64>(conn, HOURS).unwrap_or(DEFAULT_HOURS);
                    match settings::get::<i64>(conn, LAST_RUN) {
                        Some(last) => Ok(hours > 0 && now() - last >= hours * 3600),
                        // A fresh install waits one interval rather than updating at once.
                        None => settings::set(conn, LAST_RUN, &now().into()).map(|()| false),
                    }
                })
                .await
                .unwrap_or(false);
            if due {
                self.start();
            }
        }
    }

    fn note(&self, change: impl FnOnce(&mut Status)) {
        change(&mut self.status.lock().unwrap_or_else(|e| e.into_inner()));
    }

    async fn run(&self) {
        let plan = self.db.call(|conn| plan(conn)).await.unwrap_or_default();
        self.note(|status| status.total = plan.iter().map(|(_, titles)| titles.len()).sum());

        let gate = Arc::new(Semaphore::new(SOURCES_AT_ONCE));
        let mut sources = JoinSet::new();
        for (_, titles) in plan {
            let (this, gate) = (self.clone(), gate.clone());
            sources.spawn(async move {
                let _slot = gate.acquire_owned().await;
                for (place, (manga, auto_download)) in titles.into_iter().enumerate() {
                    if place > 0 {
                        tokio::time::sleep(BETWEEN_TITLES).await;
                    }
                    this.one(manga, auto_download).await;
                }
            });
        }
        while sources.join_next().await.is_some() {}

        let _ = self.db.call(|conn| settings::set(conn, LAST_RUN, &now().into())).await;
        self.note(|status| {
            status.running = false;
            status.finished_at = Some(now());
        });
        let status = self.status();
        tracing::info!("library update: {} titles, {} new chapters, {} failed", status.total, status.found, status.failed);
    }

    async fn one(&self, manga: i64, auto_download: bool) {
        let outcome = self.titles.fetch(manga, false, true).await;
        let error = outcome.as_ref().err().map(|e| e.message.clone());
        let failed = error.is_some();
        let _ = self
            .db
            .call(move |conn| {
                match error {
                    Some(error) => conn.execute(
                        "INSERT INTO update_errors (manga_id, error, at) VALUES (?1, ?2, ?3)
                         ON CONFLICT(manga_id) DO UPDATE SET error = excluded.error, at = excluded.at",
                        params![manga, error, now()],
                    )?,
                    None => conn.execute("DELETE FROM update_errors WHERE manga_id = ?1", [manga])?,
                };
                Ok(())
            })
            .await;
        let new = outcome.unwrap_or_default();
        self.note(|status| {
            status.done += 1;
            status.found += new.len();
            status.failed += usize::from(failed);
        });
        if auto_download && !new.is_empty() {
            if let Err(e) = self.downloads.enqueue(new).await {
                tracing::warn!("could not queue new chapters: {e}");
            }
        }
    }
}

/// The titles to update, grouped by source: `(source, [(title, auto-download)])`.
/// Titles whose extension is not installed are left out; there is no one to ask.
fn plan(conn: &Connection) -> Result<Vec<(i64, Vec<(i64, bool)>)>> {
    let ids = |key: &str| settings::get::<Vec<i64>>(conn, key).unwrap_or_default().iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    let mut sql = String::from(
        "SELECT m.source_id, m.id, m.auto_download FROM manga m JOIN sources s ON s.id = m.source_id
         WHERE m.in_library = 1
           AND NOT (m.update_strategy = 'ONLY_FETCH_ONCE' AND EXISTS (SELECT 1 FROM chapters WHERE manga_id = m.id))",
    );
    if settings::get::<bool>(conn, SKIP_COMPLETED).unwrap_or(false) {
        sql += " AND m.status NOT IN (2, 4)";
    }
    if settings::get::<bool>(conn, SKIP_UNREAD).unwrap_or(false) {
        sql += " AND NOT EXISTS (SELECT 1 FROM chapters WHERE manga_id = m.id AND read = 0)";
    }
    let (only, never) = (ids(ONLY), ids(NEVER));
    if !only.is_empty() {
        sql += &format!(" AND EXISTS (SELECT 1 FROM manga_categories WHERE manga_id = m.id AND category_id IN ({only}))");
    }
    if !never.is_empty() {
        sql += &format!(" AND NOT EXISTS (SELECT 1 FROM manga_categories WHERE manga_id = m.id AND category_id IN ({never}))");
    }
    sql += " ORDER BY m.source_id, m.id";

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, bool>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut plan: Vec<(i64, Vec<(i64, bool)>)> = Vec::new();
    for (source, manga, auto_download) in rows {
        match plan.last_mut() {
            Some((last, titles)) if *last == source => titles.push((manga, auto_download)),
            _ => plan.push((source, vec![(manga, auto_download)])),
        }
    }
    Ok(plan)
}

/// A chapter counts as an update when Mango first saw it after its title joined the library.
const IS_UPDATE: &str = "m.in_library = 1 AND c.fetched_at > COALESCE(m.added_at, 0)";

fn list(conn: &Connection, limit: i64) -> Result<(Vec<UpdateItem>, Vec<UpdateError>)> {
    let mut stmt = conn.prepare(&format!(
        "SELECT c.id, c.name, c.fetched_at, c.read, c.download_path IS NOT NULL, m.id, m.title, m.thumbnail_url, s.name
         FROM chapters c JOIN manga m ON m.id = c.manga_id LEFT JOIN sources s ON s.id = m.source_id
         WHERE {IS_UPDATE}
         ORDER BY c.fetched_at DESC, m.title, c.source_order LIMIT ?1"
    ))?;
    let items = stmt
        .query_map([limit.clamp(1, 1000)], |r| {
            let manga_id: i64 = r.get(5)?;
            let thumbnail: Option<String> = r.get(7)?;
            Ok(UpdateItem {
                chapter_id: r.get(0)?,
                chapter_name: r.get(1)?,
                fetched_at: r.get(2)?,
                read: r.get(3)?,
                downloaded: r.get(4)?,
                manga_id,
                title: r.get(6)?,
                cover: cover_url(manga_id, thumbnail.as_deref()),
                source_name: r.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut stmt = conn.prepare(
        "SELECT e.manga_id, m.title, e.error, e.at FROM update_errors e JOIN manga m ON m.id = e.manga_id
         WHERE m.in_library = 1 ORDER BY m.title COLLATE NOCASE",
    )?;
    let errors = stmt
        .query_map([], |r| Ok(UpdateError { manga_id: r.get(0)?, title: r.get(1)?, error: r.get(2)?, at: r.get(3)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok((items, errors))
}

fn fresh(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row(
        &format!("SELECT COUNT(*) FROM chapters c JOIN manga m ON m.id = c.manga_id WHERE {IS_UPDATE} AND c.read = 0 AND c.fetched_at > ?1"),
        [now() - FRESH_SECS],
        |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn database() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&mut conn).unwrap();
        conn.execute_batch(
            "INSERT INTO extensions (pkg, name, version_name, version_code, lib, content_warning, installed_at, updated_at) VALUES ('p', 'P', '1', 1, '1.6', 0, 0, 0);
             INSERT INTO sources (id, pkg, name, lang, supports_latest) VALUES (7, 'p', 'Seven', 'en', 1), (8, 'p', 'Eight', 'en', 1);
             INSERT INTO categories (id, name, position) VALUES (1, 'Reading', 0), (2, 'Paused', 1);
             INSERT INTO manga (id, source_id, url, title, in_library, added_at, status, auto_download, update_strategy) VALUES
                 (1, 7, '/1', 'Ongoing', 1, 100, 1, 1, 'ALWAYS_UPDATE'),
                 (2, 7, '/2', 'Completed', 1, 100, 2, 0, 'ALWAYS_UPDATE'),
                 (3, 8, '/3', 'Once', 1, 100, 1, 0, 'ONLY_FETCH_ONCE'),
                 (4, 8, '/4', 'Not in library', 0, NULL, 1, 0, 'ALWAYS_UPDATE'),
                 (5, 9, '/5', 'Extension gone', 1, 100, 1, 0, 'ALWAYS_UPDATE'),
                 (6, 8, '/6', 'Caught up', 1, 100, 1, 0, 'ALWAYS_UPDATE');
             INSERT INTO chapters (id, manga_id, url, name, source_order, fetched_at, read) VALUES
                 (10, 1, 'a', 'Old', 1, 50, 1), (11, 1, 'b', 'New', 0, 9999999999, 0),
                 (30, 3, 'a', 'Only', 0, 50, 1), (60, 6, 'a', 'Read', 0, 9999999999, 1);
             INSERT INTO manga_categories VALUES (1, 1), (6, 2);",
        )
        .unwrap();
        conn
    }

    fn planned(conn: &Connection) -> Vec<(i64, Vec<i64>)> {
        plan(conn).unwrap().into_iter().map(|(source, titles)| (source, titles.into_iter().map(|(id, _)| id).collect())).collect()
    }

    fn set(conn: &Connection, key: &str, value: serde_json::Value) {
        settings::set(conn, key, &value).unwrap();
    }

    #[test]
    fn plan_follows_the_options() {
        let conn = database();
        assert_eq!(planned(&conn), [(7, vec![1, 2]), (8, vec![6])], "library titles with a source, grouped by it");
        assert!(plan(&conn).unwrap()[0].1[0].1, "auto-download travels with the title");

        set(&conn, SKIP_COMPLETED, json!(true));
        assert_eq!(planned(&conn), [(7, vec![1]), (8, vec![6])]);
        set(&conn, SKIP_UNREAD, json!(true));
        assert_eq!(planned(&conn), [(8, vec![6])]);
        set(&conn, SKIP_UNREAD, json!(false));
        set(&conn, ONLY, json!([1]));
        assert_eq!(planned(&conn), [(7, vec![1])]);
        set(&conn, ONLY, json!([]));
        set(&conn, NEVER, json!([2]));
        assert_eq!(planned(&conn), [(7, vec![1])]);
    }

    #[test]
    fn updates_are_what_came_after_joining() {
        let conn = database();
        conn.execute("INSERT INTO update_errors VALUES (2, 'boom', 5), (4, 'unseen', 5)", []).unwrap();
        let (items, errors) = list(&conn, 50).unwrap();
        assert_eq!(items.iter().map(|i| i.chapter_name.as_str()).collect::<Vec<_>>(), ["Read", "New"]);
        assert_eq!(errors.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["Completed"], "only library titles report errors");
        assert_eq!(fresh(&conn).unwrap(), 1, "the menu counts what is new and unread");
    }
}
