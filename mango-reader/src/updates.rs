//! Library updates: ask each library title's source for new chapters, on a schedule
//! or on request. Sources run side by side, a few at a time; within one source the
//! titles go one after another with a pause, so no source is hammered. One failing
//! title or source never stops the rest.
//!
//! Asked to, a run leaves out the titles no chapter is expected of yet: a weekly series
//! is asked around its day, not twice every day. See `expected`.

use crate::{
    db::{now, Db},
    downloads::Downloads,
    error::Result,
    library::FOLLOWED,
    notify::Notifier,
    settings,
    titles::{cover_url, rhythm, Titles, MAX_RELEASE_DAYS},
    trackers::{self, Trackers},
};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::{
    collections::HashMap,
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
/// Leave out the titles no chapter is expected of yet.
pub const SMART: &str = "updates.smart";
const LAST_RUN: &str = "updates.lastRun";

const DEFAULT_HOURS: i64 = 12;
const SOURCES_AT_ONCE: usize = 4;
const BETWEEN_TITLES: Duration = Duration::from_secs(1);
/// How far back the "new chapters" count for the menu looks.
const FRESH_SECS: i64 = 7 * 86_400;
const DAY: i64 = 86_400;
/// How many days before and after the one a chapter is expected on a title is asked.
const GRACE: i64 = 1;
/// After this many expected days with nothing new, a title is asked half as often.
const PATIENCE: i64 = 10;

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub running: bool,
    pub done: usize,
    pub total: usize,
    /// Titles this run left out because no chapter is expected of them yet.
    pub waiting: usize,
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
    notifier: Notifier,
    trackers: Trackers,
    status: Arc<StdMutex<Status>>,
    /// The chapters the run going on has found so far.
    found: Arc<StdMutex<Vec<i64>>>,
}

impl Updates {
    pub fn new(db: Db, titles: Titles, downloads: Downloads, notifier: Notifier, trackers: Trackers) -> Self {
        Self { db, titles, downloads, notifier, trackers, status: Arc::default(), found: Arc::default() }
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
        let plan = self.db.call(|conn| plan(conn, now() / DAY)).await.unwrap_or_default();
        self.note(|status| {
            status.total = plan.sources.iter().map(|(_, titles)| titles.len()).sum();
            status.waiting = plan.waiting;
        });

        let gate = Arc::new(Semaphore::new(SOURCES_AT_ONCE));
        let mut sources = JoinSet::new();
        for (_, titles) in plan.sources {
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

        let found = std::mem::take(&mut *self.found.lock().unwrap_or_else(|e| e.into_inner()));
        self.notifier.chapters(found).await;
        let pull = self.db.call(|conn| Ok(settings::get::<bool>(conn, trackers::PULL).unwrap_or(false))).await.unwrap_or(false);
        if pull {
            self.trackers.pull_all().await;
        }
        let _ = self.db.call(|conn| settings::set(conn, LAST_RUN, &now().into())).await;
        self.note(|status| {
            status.running = false;
            status.finished_at = Some(now());
        });
        let status = self.status();
        tracing::info!(
            "library update: {} titles, {} not due, {} new chapters, {} failed",
            status.total,
            status.waiting,
            status.found,
            status.failed
        );
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
        self.found.lock().unwrap_or_else(|e| e.into_inner()).extend(&new);
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

#[derive(Default)]
struct Plan {
    /// The titles to update, grouped by source: `(source, [(title, auto-download)])`.
    sources: Vec<(i64, Vec<(i64, bool)>)>,
    /// Titles left out because no chapter is expected of them yet.
    waiting: usize,
}

/// What a run on the day `today`, counted from the epoch, asks about.
/// Titles whose extension is not installed are left out; there is no one to ask.
fn plan(conn: &Connection, today: i64) -> Result<Plan> {
    let ids = |key: &str| settings::get::<Vec<i64>>(conn, key).unwrap_or_default().iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    // A title merged into another updates with it, and goes by that one's categories and downloads.
    let mut sql = format!(
        "SELECT m.source_id, m.id, COALESCE((SELECT auto_download FROM manga WHERE id = m.merged_into), m.auto_download), m.chapters_at
         FROM manga m JOIN sources s ON s.id = m.source_id
         WHERE {FOLLOWED}
           AND NOT (m.update_strategy = 'ONLY_FETCH_ONCE' AND EXISTS (SELECT 1 FROM chapters WHERE manga_id = m.id))"
    );
    if settings::get::<bool>(conn, SKIP_COMPLETED).unwrap_or(false) {
        sql += " AND m.status NOT IN (2, 4)";
    }
    if settings::get::<bool>(conn, SKIP_UNREAD).unwrap_or(false) {
        sql += " AND NOT EXISTS (SELECT 1 FROM chapters WHERE manga_id = m.id AND read = 0)";
    }
    let (only, never) = (ids(ONLY), ids(NEVER));
    if !only.is_empty() {
        sql += &format!(" AND EXISTS (SELECT 1 FROM manga_categories WHERE manga_id = COALESCE(m.merged_into, m.id) AND category_id IN ({only}))");
    }
    if !never.is_empty() {
        sql += &format!(" AND NOT EXISTS (SELECT 1 FROM manga_categories WHERE manga_id = COALESCE(m.merged_into, m.id) AND category_id IN ({never}))");
    }
    sql += " ORDER BY m.source_id, m.id";

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, bool>(2)?, r.get::<_, Option<i64>>(3)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let known = if settings::get::<bool>(conn, SMART).unwrap_or(false) { rhythms(conn, today)? } else { HashMap::new() };
    let mut plan = Plan::default();
    for (source, manga, auto_download, asked) in rows {
        // A title whose dates say too little is asked every time.
        if known.get(&manga).is_some_and(|&(every, latest)| !expected(every, latest, asked.map(|at| at / DAY), today)) {
            plan.waiting += 1;
            continue;
        }
        match plan.sources.last_mut() {
            Some((last, titles)) if *last == source => titles.push((manga, auto_download)),
            _ => plan.sources.push((source, vec![(manga, auto_download)])),
        }
    }
    Ok(plan)
}

/// The rhythm of each library title whose chapters carry enough dates: `(days between two, day of the latest)`.
/// A date still to come is a source's mistake and is not counted.
fn rhythms(conn: &Connection, today: i64) -> Result<HashMap<i64, (i64, i64)>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT DISTINCT c.manga_id, c.date_upload / 86400000 FROM chapters c JOIN manga m ON m.id = c.manga_id
         WHERE {FOLLOWED} AND c.date_upload > 0 AND c.date_upload / 86400000 <= ?1"
    ))?;
    let mut days: HashMap<i64, Vec<i64>> = HashMap::new();
    for row in stmt.query_map([today], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
        let (manga, day) = row?;
        days.entry(manga).or_default().push(day);
    }
    Ok(days.into_iter().filter_map(|(manga, days)| Some((manga, rhythm(days)?))).collect())
}

/// Whether a title is worth asking today. It gets a chapter every `every` days and the last
/// came out on the day `latest`; `asked` is the day its source was last asked. All days are
/// counted from the epoch.
///
/// A title is asked from the day before a chapter is expected to the day after. It is also
/// asked once when such days went by unasked, so a Mango that was switched off catches up.
/// After a long silence the days it is expected on move apart, up to four weeks.
fn expected(every: i64, latest: i64, asked: Option<i64>, today: i64) -> bool {
    let Some(asked) = asked else { return true };
    let since = (today - latest).max(0);
    let mut every = every.clamp(1, MAX_RELEASE_DAYS);
    while every < MAX_RELEASE_DAYS && since / every >= PATIENCE {
        every = (every * 2).min(MAX_RELEASE_DAYS);
    }
    // How many expected days have come into reach since the latest chapter.
    let reached = (since + GRACE) / every;
    if reached == 0 {
        return false;
    }
    let opened = latest + reached * every - GRACE;
    today <= opened + 2 * GRACE || asked < opened
}

/// A chapter counts as an update when Mango first saw it after its title joined the library,
/// or after it was merged into a library title.
fn is_update() -> String {
    format!("{FOLLOWED} AND c.fetched_at > COALESCE(m.added_at, 0)")
}

/// Chapters of a merged title show under the title it was merged into.
fn list(conn: &Connection, limit: i64) -> Result<(Vec<UpdateItem>, Vec<UpdateError>)> {
    let mut stmt = conn.prepare(&format!(
        "SELECT c.id, c.name, c.fetched_at, c.read, c.download_path IS NOT NULL, h.id, h.title, h.thumbnail_url, s.name
         FROM chapters c JOIN manga m ON m.id = c.manga_id JOIN manga h ON h.id = COALESCE(m.merged_into, m.id)
         LEFT JOIN sources s ON s.id = m.source_id
         WHERE {}
         ORDER BY c.fetched_at DESC, h.title, c.source_order LIMIT ?1",
        is_update()
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

    let mut stmt = conn.prepare(&format!(
        "SELECT e.manga_id, m.title, e.error, e.at FROM update_errors e JOIN manga m ON m.id = e.manga_id
         WHERE {FOLLOWED} ORDER BY m.title COLLATE NOCASE"
    ))?;
    let errors = stmt
        .query_map([], |r| Ok(UpdateError { manga_id: r.get(0)?, title: r.get(1)?, error: r.get(2)?, at: r.get(3)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok((items, errors))
}

/// Unread chapters found in the last week: the number beside Updates in the menu.
pub(crate) fn fresh(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row(
        &format!("SELECT COUNT(*) FROM chapters c JOIN manga m ON m.id = c.manga_id WHERE {} AND c.read = 0 AND c.fetched_at > ?1", is_update()),
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

    /// The day the tests take place on.
    const TODAY: i64 = 20_000;

    fn planned(conn: &Connection) -> Vec<(i64, Vec<i64>)> {
        planned_on(conn, TODAY).0
    }

    fn planned_on(conn: &Connection, today: i64) -> (Vec<(i64, Vec<i64>)>, usize) {
        let plan = plan(conn, today).unwrap();
        (plan.sources.into_iter().map(|(source, titles)| (source, titles.into_iter().map(|(id, _)| id).collect())).collect(), plan.waiting)
    }

    fn set(conn: &Connection, key: &str, value: serde_json::Value) {
        settings::set(conn, key, &value).unwrap();
    }

    #[test]
    fn plan_follows_the_options() {
        let conn = database();
        assert_eq!(planned(&conn), [(7, vec![1, 2]), (8, vec![6])], "library titles with a source, grouped by it");
        assert!(plan(&conn, TODAY).unwrap().sources[0].1[0].1, "auto-download travels with the title");

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
    fn a_title_is_asked_around_its_day() {
        // A weekly title whose latest chapter came out on day 100: `(last asked, today, asked now)`.
        for (asked, today, due) in [
            (100, 101, false),
            (104, 105, false),
            (105, 106, true),
            (106, 107, true),
            (107, 108, true),
            (108, 109, false),
            (104, 111, true),
            (111, 112, false),
            (112, 113, true),
            (160, 162, true),
            // Ten weeks of nothing: every second week from now on.
            (175, 176, false),
            (180, 183, true),
        ] {
            assert_eq!(expected(7, 100, Some(asked), today), due, "asked on {asked}, today is {today}");
        }
        assert!(expected(7, 100, None, 101), "a title never asked is asked");
        assert!((100..120).all(|today| expected(1, 100, Some(today), today)), "a daily one every day, for a while");
        assert!(!expected(1, 100, Some(121), 122), "then less often");
    }

    #[test]
    fn smart_updates_leave_out_what_is_not_due() {
        let conn = database();
        // Title 1 gets a chapter a week, the last one today; the others carry no dates.
        for (id, day) in [(12, TODAY - 14), (13, TODAY - 7), (14, TODAY)] {
            conn.execute(
                "INSERT INTO chapters (id, manga_id, url, name, source_order, fetched_at, date_upload) VALUES (?1, 1, ?1, 'Dated', ?1, 50, ?2)",
                params![id, day * 86_400_000],
            )
            .unwrap();
        }
        conn.execute("UPDATE manga SET chapters_at = ?1", [TODAY * DAY]).unwrap();
        assert_eq!(planned_on(&conn, TODAY + 3), (vec![(7, vec![1, 2]), (8, vec![6])], 0), "left alone, every title is asked");

        set(&conn, SMART, json!(true));
        assert_eq!(planned_on(&conn, TODAY + 3), (vec![(7, vec![2]), (8, vec![6])], 1));
        assert_eq!(planned_on(&conn, TODAY + 6), (vec![(7, vec![1, 2]), (8, vec![6])], 0), "the day before it is expected");
        // A chapter dated next year is not believed.
        conn.execute("UPDATE chapters SET date_upload = ?1 WHERE id = 14", [(TODAY + 365) * 86_400_000]).unwrap();
        conn.execute("INSERT INTO chapters (id, manga_id, url, name, source_order, fetched_at, date_upload) VALUES (15, 1, 'e', 'Dated', 15, 50, ?1)", [(TODAY - 21) * 86_400_000])
            .unwrap();
        assert_eq!(planned_on(&conn, TODAY + 6).1, 0);
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

    #[test]
    fn a_merged_title_updates_with_its_head() {
        let conn = database();
        // "Not in library" is merged into "Ongoing": it is asked, takes its auto-download, and its chapters show under it.
        conn.execute("UPDATE manga SET merged_into = 1, added_at = 100 WHERE id = 4", []).unwrap();
        conn.execute("INSERT INTO chapters (id, manga_id, url, name, source_order, fetched_at) VALUES (40, 4, 'x', 'Merged', 0, 9999999999)", []).unwrap();
        let plan = plan(&conn, TODAY).unwrap();
        assert!(plan.sources.iter().any(|(source, titles)| *source == 8 && titles.contains(&(4, true))));
        let (items, _) = list(&conn, 50).unwrap();
        let merged = items.iter().find(|i| i.chapter_id == 40).unwrap();
        assert_eq!((merged.manga_id, merged.title.as_str()), (1, "Ongoing"));
        set(&conn, ONLY, json!([1]));
        assert!(planned(&conn).iter().any(|(_, titles)| titles.contains(&4)), "in its head's category");
    }
}
