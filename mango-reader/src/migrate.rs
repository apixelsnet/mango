//! Migration: a title's place in the library goes over to the same title on another source.
//!
//! Nothing to do with the database's own migrations in `db.rs`. Everything here reads and
//! writes the database only; the caller has the other title's chapters fetched first.

use crate::{
    db::now,
    error::{AppError, Result},
    titles::cover_url,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// What goes along. Left out, everything does and the old title leaves the library.
#[derive(Deserialize)]
#[serde(default)]
pub struct Options {
    /// Read chapters, bookmarks, the page reached and the history.
    pub chapters: bool,
    pub categories: bool,
    /// Tracker links.
    pub tracks: bool,
    /// Leave the old title in the library with all it has: a copy, not a move.
    pub keep: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { chapters: true, categories: true, tracks: true, keep: false }
    }
}

/// One of the two titles, as the screen that compares them shows it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Side {
    pub id: i64,
    pub title: String,
    pub cover: Option<String>,
    /// None when the extension that provides the source is not installed.
    pub source_name: Option<String>,
    pub source_lang: Option<String>,
    pub in_library: bool,
    pub chapters: i64,
    pub read: i64,
    /// The highest chapter number it has, and the highest that is read. -1 when there is none.
    pub latest: f64,
    pub last_read: f64,
    pub downloaded: i64,
}

#[derive(Serialize)]
pub struct Comparison {
    pub from: Side,
    pub to: Side,
    /// Chapters of the new title that would be marked read.
    pub read: i64,
    /// Categories and tracker links that would come along.
    pub categories: i64,
    pub tracks: i64,
}

#[derive(Serialize, Default, Debug, PartialEq)]
pub struct Outcome {
    pub read: i64,
    pub categories: i64,
    pub tracks: i64,
}

fn same_title() -> AppError {
    AppError::bad_request("A title cannot be moved onto itself")
}

fn no_title() -> AppError {
    AppError::new(404, "unknown_title", "No such title")
}

/// Two chapters are the same one when their numbers agree to the hundredth. Without a
/// number, as with a one-shot or a prologue, their names must.
#[derive(PartialEq, Eq, Hash)]
enum Key {
    Number(i64),
    Name(String),
}

fn key(number: f64, name: &str) -> Key {
    if number >= 0.0 {
        Key::Number((number * 100.0).round() as i64)
    } else {
        Key::Name(name.trim().to_lowercase())
    }
}

/// What the old title knows about one chapter. Several uploads of it count as one.
#[derive(Default)]
struct Old {
    read: bool,
    page: i64,
    bookmark: bool,
    read_at: Option<i64>,
    /// When it was last open in the reader, from the history.
    opened: Option<i64>,
}

/// What one chapter of the new title takes over.
struct Change {
    id: i64,
    /// It becomes read.
    read: bool,
    /// The page reached, 0 for none to set.
    page: i64,
    bookmark: bool,
    read_at: Option<i64>,
    opened: Option<i64>,
}

/// Works out what the new title's chapters take over, changing nothing.
/// As Mihon does it: every numbered chapter up to the last one read counts as read, so a
/// source that splits or numbers its chapters differently still lands in the right place.
fn plan(conn: &Connection, from: i64, to: i64) -> Result<Vec<Change>> {
    let mut olds: HashMap<Key, Old> = HashMap::new();
    let mut furthest: Option<i64> = None;
    {
        let mut stmt = conn.prepare(
            "SELECT c.chapter_number, c.name, c.read, c.last_page_read, c.bookmark, c.read_at, h.read_at
             FROM chapters c LEFT JOIN history h ON h.chapter_id = c.id WHERE c.manga_id = ?1",
        )?;
        let mut rows = stmt.query([from])?;
        while let Some(r) = rows.next()? {
            let key = key(r.get(0)?, &r.get::<_, String>(1)?);
            let (read, page): (bool, i64) = (r.get(2)?, r.get(3)?);
            if let (Key::Number(number), true) = (&key, read) {
                furthest = furthest.max(Some(*number));
            }
            let old = olds.entry(key).or_default();
            old.read |= read;
            old.page = old.page.max(page);
            old.bookmark |= r.get::<_, bool>(4)?;
            old.read_at = old.read_at.max(r.get(5)?);
            old.opened = old.opened.max(r.get(6)?);
        }
    }

    let mut changes = Vec::new();
    let mut stmt = conn.prepare("SELECT id, chapter_number, name, read, last_page_read, bookmark FROM chapters WHERE manga_id = ?1")?;
    let mut rows = stmt.query([to])?;
    while let Some(r) = rows.next()? {
        let key = key(r.get(1)?, &r.get::<_, String>(2)?);
        let (was_read, had_page, had_bookmark): (bool, i64, bool) = (r.get(3)?, r.get(4)?, r.get(5)?);
        let behind = matches!((&key, furthest), (Key::Number(number), Some(furthest)) if *number <= furthest);
        let old = olds.get(&key);
        let read = !was_read && (behind || old.is_some_and(|old| old.read));
        let page = if was_read || read || had_page > 0 { 0 } else { old.map_or(0, |old| old.page) };
        let bookmark = !had_bookmark && old.is_some_and(|old| old.bookmark);
        let opened = old.and_then(|old| old.opened);
        if read || page > 0 || bookmark || opened.is_some() {
            changes.push(Change { id: r.get(0)?, read, page, bookmark, read_at: old.and_then(|old| old.read_at), opened });
        }
    }
    Ok(changes)
}

fn side(conn: &Connection, id: i64) -> Result<Side> {
    conn.query_row(
        "SELECT m.title, m.thumbnail_url, s.name, s.lang, m.in_library, COUNT(c.id), COALESCE(SUM(c.read), 0),
                COALESCE(MAX(c.chapter_number), -1), COALESCE(MAX(CASE WHEN c.read THEN c.chapter_number END), -1),
                COALESCE(SUM(c.download_path IS NOT NULL), 0)
         FROM manga m LEFT JOIN chapters c ON c.manga_id = m.id LEFT JOIN sources s ON s.id = m.source_id
         WHERE m.id = ?1 GROUP BY m.id",
        [id],
        |r| {
            let thumbnail: Option<String> = r.get(1)?;
            Ok(Side {
                id,
                title: r.get(0)?,
                cover: cover_url(id, thumbnail.as_deref()),
                source_name: r.get(2)?,
                source_lang: r.get(3)?,
                in_library: r.get(4)?,
                chapters: r.get(5)?,
                read: r.get(6)?,
                latest: r.get(7)?,
                last_read: r.get(8)?,
                downloaded: r.get(9)?,
            })
        },
    )
    .optional()?
    .ok_or_else(no_title)
}

/// The two titles side by side, and what moving one onto the other would carry over.
pub fn compare(conn: &Connection, from: i64, to: i64) -> Result<Comparison> {
    if from == to {
        return Err(same_title());
    }
    let (from_side, to_side) = (side(conn, from)?, side(conn, to)?);
    let read = plan(conn, from, to)?.iter().filter(|change| change.read).count() as i64;
    let categories = conn.query_row(
        "SELECT COUNT(*) FROM manga_categories WHERE manga_id = ?1
         AND category_id NOT IN (SELECT category_id FROM manga_categories WHERE manga_id = ?2)",
        [from, to],
        |r| r.get(0),
    )?;
    let tracks = conn.query_row("SELECT COUNT(*) FROM tracks WHERE manga_id = ?1", [from], |r| r.get(0))?;
    Ok(Comparison { from: from_side, to: to_side, read, categories, tracks })
}

/// Moves `from` onto `to`, or copies it with `keep`. Nothing the new title already has
/// is lost: a chapter read on either side stays read. All of it happens, or none.
pub fn apply(conn: &mut Connection, from: i64, to: i64, options: &Options) -> Result<Outcome> {
    if from == to {
        return Err(same_title());
    }
    let tx = conn.transaction()?;
    let library = |id: i64| -> Result<(bool, Option<i64>)> {
        tx.query_row("SELECT in_library, added_at FROM manga WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?.ok_or_else(no_title)
    };
    let ((followed, added_at), (already, _)) = (library(from)?, library(to)?);
    let now = now();
    let mut outcome = Outcome::default();

    if options.chapters {
        for change in plan(&tx, from, to)? {
            tx.execute(
                "UPDATE chapters SET read = read OR ?2, bookmark = bookmark OR ?4, read_at = COALESCE(?5, read_at),
                     last_page_read = CASE WHEN ?2 THEN 0 WHEN ?3 > 0 THEN ?3 ELSE last_page_read END
                 WHERE id = ?1",
                params![change.id, change.read, change.page, change.bookmark, change.read_at.or(change.read.then_some(now))],
            )?;
            if let Some(opened) = change.opened {
                tx.execute(
                    "INSERT INTO history (chapter_id, manga_id, read_at) VALUES (?1, ?2, ?3)
                     ON CONFLICT(chapter_id) DO UPDATE SET read_at = MAX(read_at, excluded.read_at)",
                    params![change.id, to, opened],
                )?;
            }
            outcome.read += i64::from(change.read);
        }
    }
    if options.categories {
        outcome.categories = tx.execute(
            "INSERT OR IGNORE INTO manga_categories (manga_id, category_id) SELECT ?2, category_id FROM manga_categories WHERE manga_id = ?1",
            [from, to],
        )? as i64;
    }
    if options.tracks {
        // The old title's link wins over one the new title has to the same tracker.
        outcome.tracks = tx.execute(
            "INSERT OR REPLACE INTO tracks (manga_id, tracker, library_id, media_id, url, title, last_chapter_read, total_chapters,
                 score, status, started_at, finished_at, error)
             SELECT ?2, tracker, library_id, media_id, url, title, last_chapter_read, total_chapters, score, status, started_at,
                 finished_at, error
             FROM tracks WHERE manga_id = ?1",
            [from, to],
        )? as i64;
        if !options.keep {
            tx.execute("DELETE FROM tracks WHERE manga_id = ?1", [from])?;
        }
    }
    // How it is read goes along, where the new title has no say of its own yet.
    tx.execute(
        "UPDATE manga SET reader_mode = COALESCE(reader_mode, (SELECT reader_mode FROM manga WHERE id = ?1)),
             reader_fit = COALESCE(reader_fit, (SELECT reader_fit FROM manga WHERE id = ?1)),
             reader_prefs = COALESCE(reader_prefs, (SELECT reader_prefs FROM manga WHERE id = ?1)),
             auto_download = auto_download OR (SELECT auto_download FROM manga WHERE id = ?1)
         WHERE id = ?2",
        [from, to],
    )?;
    if followed && !already {
        tx.execute("UPDATE manga SET in_library = 1, added_at = ?2 WHERE id = ?1", params![to, added_at.unwrap_or(now)])?;
    }
    if !options.keep {
        // Its chapters and saved files stay, as when a title is taken out of the library.
        // Its history goes, or the library would offer to go on reading it.
        tx.execute("UPDATE manga SET in_library = 0, added_at = NULL WHERE id = ?1", [from])?;
        for table in ["manga_categories", "update_errors", "history"] {
            tx.execute(&format!("DELETE FROM {table} WHERE manga_id = ?1"), [from])?;
        }
    }
    tx.commit()?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&mut conn).unwrap();
        conn
    }

    fn title(conn: &Connection, source: i64, in_library: bool) -> i64 {
        conn.execute(
            "INSERT INTO manga (source_id, url, title, in_library, added_at) VALUES (?1, '/t', 'Alpha', ?2, ?3)",
            params![source, in_library, in_library.then_some(1000)],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    /// Adds chapters as `(number, name, read)`, newest first.
    fn chapters(conn: &Connection, manga: i64, list: &[(f64, &str, bool)]) -> Vec<i64> {
        list.iter()
            .enumerate()
            .map(|(order, (number, name, read))| {
                conn.execute(
                    "INSERT INTO chapters (manga_id, url, name, chapter_number, source_order, fetched_at, read, read_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7)",
                    params![manga, format!("/c/{order}"), name, number, order as i64, read, read.then_some(500)],
                )
                .unwrap();
                conn.last_insert_rowid()
            })
            .collect()
    }

    fn state(conn: &Connection, manga: i64) -> Vec<(bool, i64, bool)> {
        let mut stmt = conn.prepare("SELECT read, last_page_read, bookmark FROM chapters WHERE manga_id = ?1 ORDER BY source_order").unwrap();
        let rows = stmt.query_map([manga], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        rows
    }

    fn count(conn: &Connection, sql: &str, id: i64) -> i64 {
        conn.query_row(sql, [id], |r| r.get(0)).unwrap()
    }

    #[test]
    fn progress_follows_the_numbers() {
        let mut conn = database();
        let (old, new) = (title(&conn, 1, true), title(&conn, 2, false));
        let had = chapters(&conn, old, &[(4.0, "Ch 4", false), (3.0, "Ch 3", true), (2.0, "Ch 2", false), (1.0, "Ch 1", true), (-1.0, "Prologue", true)]);
        conn.execute("UPDATE chapters SET last_page_read = 7, bookmark = 1 WHERE id = ?1", [had[0]]).unwrap();
        conn.execute("INSERT INTO history (chapter_id, manga_id, read_at) VALUES (?1, ?2, 900)", [had[0], old]).unwrap();
        // The other source splits chapter 2 in two, has one more, and names its extras its own way.
        let got = chapters(&conn, new, &[
            (5.0, "Five", false),
            (4.0, "Four", false),
            (3.0, "Three", false),
            (2.5, "Two, part two", false),
            (2.0, "Two", false),
            (1.0, "One", false),
            (-1.0, " prologue", false),
            (-1.0, "Extra", false),
        ]);

        let seen = compare(&conn, old, new).unwrap();
        assert_eq!((seen.read, seen.from.read, seen.from.last_read, seen.to.latest, seen.to.chapters), (5, 3, 3.0, 5.0, 8));
        assert_eq!(state(&conn, new).iter().filter(|c| c.0).count(), 0, "comparing changes nothing");

        let outcome = apply(&mut conn, old, new, &Options::default()).unwrap();
        assert_eq!(outcome.read, 5);
        assert_eq!(state(&conn, new), [
            (false, 0, false),
            (false, 7, true),
            (true, 0, false),
            (true, 0, false),
            (true, 0, false),
            (true, 0, false),
            (true, 0, false),
            (false, 0, false)
        ]);
        assert_eq!(count(&conn, "SELECT read_at FROM history WHERE chapter_id = ?1", got[1]), 900, "the history comes along");
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM history WHERE manga_id = ?1", old), 0);
        assert_eq!(count(&conn, "SELECT in_library FROM manga WHERE id = ?1", old), 0);
        assert_eq!(count(&conn, "SELECT added_at FROM manga WHERE id = ?1 AND in_library", new), 1000, "it keeps its place in the library");
        assert_eq!(state(&conn, old).iter().filter(|c| c.0).count(), 3, "the old title keeps what it had");

        // Again changes nothing more, and what was read on the new side is never taken back.
        conn.execute("UPDATE chapters SET read = 1 WHERE id = ?1", [got[0]]).unwrap();
        assert_eq!(apply(&mut conn, old, new, &Options::default()).unwrap(), Outcome::default());
        assert!(state(&conn, new)[0].0);
    }

    #[test]
    fn the_rest_comes_along() {
        let mut conn = database();
        let (old, new) = (title(&conn, 1, true), title(&conn, 2, false));
        conn.execute_batch("INSERT INTO categories (id, name, position) VALUES (1, 'Action', 0), (2, 'Slow', 1)").unwrap();
        conn.execute("INSERT INTO manga_categories (manga_id, category_id) VALUES (?1, 1), (?1, 2), (?2, 2)", [old, new]).unwrap();
        conn.execute("INSERT INTO tracks (manga_id, tracker, media_id, title, last_chapter_read) VALUES (?1, 2, 77, 'Alpha', 3)", [old]).unwrap();
        conn.execute("INSERT INTO tracks (manga_id, tracker, media_id, title) VALUES (?1, 2, 5, 'Wrong one')", [new]).unwrap();
        conn.execute("UPDATE manga SET reader_mode = 'webtoon', auto_download = 1 WHERE id = ?1", [old]).unwrap();
        conn.execute("UPDATE manga SET reader_fit = 'height' WHERE id = ?1", [new]).unwrap();
        conn.execute("INSERT INTO update_errors (manga_id, error, at) VALUES (?1, 'gone', 1)", [old]).unwrap();

        let seen = compare(&conn, old, new).unwrap();
        assert_eq!((seen.categories, seen.tracks), (1, 1));

        // A copy leaves the old title as it was.
        let copy = Options { keep: true, ..Options::default() };
        assert_eq!(apply(&mut conn, old, new, &copy).unwrap(), Outcome { read: 0, categories: 1, tracks: 1 });
        assert_eq!(count(&conn, "SELECT media_id FROM tracks WHERE manga_id = ?1", new), 77);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM tracks WHERE manga_id = ?1", old), 1);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM manga_categories WHERE manga_id = ?1", old), 2);
        assert_eq!(count(&conn, "SELECT in_library FROM manga WHERE id = ?1", old), 1);
        let reader: (String, String, bool) =
            conn.query_row("SELECT reader_mode, reader_fit, auto_download FROM manga WHERE id = ?1", [new], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!(reader, ("webtoon".to_owned(), "height".to_owned(), true));

        // A move empties it.
        apply(&mut conn, old, new, &Options::default()).unwrap();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM manga_categories WHERE manga_id = ?1", new), 2);
        for table in ["tracks", "manga_categories", "update_errors"] {
            assert_eq!(count(&conn, &format!("SELECT COUNT(*) FROM {table} WHERE manga_id = ?1"), old), 0, "{table}");
        }
        assert_eq!(count(&conn, "SELECT in_library FROM manga WHERE id = ?1", old), 0);
    }

    #[test]
    fn what_is_left_out_stays() {
        let mut conn = database();
        let (old, new) = (title(&conn, 1, true), title(&conn, 2, false));
        chapters(&conn, old, &[(1.0, "Ch 1", true)]);
        chapters(&conn, new, &[(1.0, "Ch 1", false)]);
        conn.execute("INSERT INTO categories (id, name, position) VALUES (1, 'Action', 0)", []).unwrap();
        conn.execute("INSERT INTO manga_categories (manga_id, category_id) VALUES (?1, 1)", [old]).unwrap();
        conn.execute("INSERT INTO tracks (manga_id, tracker, media_id) VALUES (?1, 2, 77)", [old]).unwrap();

        let bare = Options { chapters: false, categories: false, tracks: false, keep: false };
        assert_eq!(apply(&mut conn, old, new, &bare).unwrap(), Outcome::default());
        assert!(!state(&conn, new)[0].0);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM manga_categories WHERE manga_id = ?1", new), 0);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM tracks WHERE manga_id = ?1", old), 1, "a link that is not moved is not dropped");
        assert_eq!(count(&conn, "SELECT in_library FROM manga WHERE id = ?1", new), 1);

        assert_eq!(apply(&mut conn, old, old, &bare).unwrap_err().status, 400);
        assert_eq!(apply(&mut conn, old, 999, &bare).unwrap_err().status, 404);
        assert!(compare(&conn, 999, new).is_err_and(|e| e.status == 404));
    }
}
