//! The library: the titles the owner follows, their categories, and the reading history.
//!
//! Everything here reads and writes the database only, so the home screen never
//! waits for the extension host.

use crate::{
    db::now,
    error::{AppError, Result},
    titles::cover_url,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItem {
    pub id: i64,
    pub title: String,
    pub cover: Option<String>,
    pub source_name: Option<String>,
    pub status: i64,
    pub added_at: i64,
    pub total: i64,
    pub unread: i64,
    pub downloaded: i64,
    /// When a chapter of it was last opened in the reader; 0 for never.
    pub last_read_at: i64,
    /// When its newest chapter came out, or was first seen when the source gives no date.
    pub latest_at: i64,
    pub categories: Vec<i64>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Category {
    pub id: i64,
    pub name: String,
}

/// Where to pick up again: the chapter last opened, or the next unread one of that title.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Resume {
    pub manga_id: i64,
    pub title: String,
    pub cover: Option<String>,
    pub chapter_id: i64,
    pub chapter_name: String,
    pub page: i64,
    pub page_count: Option<i64>,
}

#[derive(Serialize)]
pub struct LibraryView {
    pub items: Vec<LibraryItem>,
    pub categories: Vec<Category>,
    pub resume: Option<Resume>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub chapter_id: i64,
    pub chapter_name: String,
    pub read_at: i64,
    pub page: i64,
    pub page_count: Option<i64>,
    pub read: bool,
    pub manga_id: i64,
    pub title: String,
    pub cover: Option<String>,
}

pub enum Bulk {
    Read,
    Unread,
    /// Takes the titles out of the library. Their chapters and progress stay.
    Remove,
    /// Adds the titles to these categories; with `replace`, takes them out of all others.
    Categories { ids: Vec<i64>, replace: bool },
}

/// The whole library in one answer. Sorting and filtering happen on screen.
pub fn read(conn: &Connection) -> Result<LibraryView> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.title, m.thumbnail_url, s.name, m.status, COALESCE(m.added_at, 0),
                COUNT(c.id), COALESCE(SUM(c.read = 0), 0), COALESCE(SUM(c.download_path IS NOT NULL), 0),
                COALESCE((SELECT MAX(read_at) FROM history WHERE manga_id = m.id), 0),
                COALESCE(MAX(CASE WHEN c.date_upload > 0 THEN c.date_upload / 1000 ELSE c.fetched_at END), 0),
                (SELECT group_concat(category_id) FROM manga_categories WHERE manga_id = m.id)
         FROM manga m
         LEFT JOIN chapters c ON c.manga_id = m.id
         LEFT JOIN sources s ON s.id = m.source_id
         WHERE m.in_library = 1
         GROUP BY m.id",
    )?;
    let items = stmt
        .query_map([], |r| {
            let id: i64 = r.get(0)?;
            let thumbnail: Option<String> = r.get(2)?;
            let categories: Option<String> = r.get(11)?;
            Ok(LibraryItem {
                id,
                title: r.get(1)?,
                cover: cover_url(id, thumbnail.as_deref()),
                source_name: r.get(3)?,
                status: r.get(4)?,
                added_at: r.get(5)?,
                total: r.get(6)?,
                unread: r.get(7)?,
                downloaded: r.get(8)?,
                last_read_at: r.get(9)?,
                latest_at: r.get(10)?,
                categories: categories.iter().flat_map(|list| list.split(',')).filter_map(|id| id.parse().ok()).collect(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(LibraryView { items, categories: categories(conn)?, resume: resume(conn)? })
}

fn resume(conn: &Connection) -> Result<Option<Resume>> {
    let last: Option<(i64, i64)> = conn
        .query_row("SELECT manga_id, chapter_id FROM history ORDER BY read_at DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let Some((manga, chapter)) = last else { return Ok(None) };
    // The chapter last opened if it is unfinished; else the oldest unread one of that title.
    let resume = conn
        .query_row(
            "SELECT m.id, m.title, m.thumbnail_url, c.id, c.name, c.last_page_read, c.page_count
             FROM chapters c JOIN manga m ON m.id = c.manga_id
             WHERE c.manga_id = ?1 AND c.read = 0
             ORDER BY c.id = ?2 DESC, c.source_order DESC LIMIT 1",
            params![manga, chapter],
            |r| {
                let manga_id: i64 = r.get(0)?;
                let thumbnail: Option<String> = r.get(2)?;
                Ok(Resume {
                    manga_id,
                    title: r.get(1)?,
                    cover: cover_url(manga_id, thumbnail.as_deref()),
                    chapter_id: r.get(3)?,
                    chapter_name: r.get(4)?,
                    page: r.get(5)?,
                    page_count: r.get(6)?,
                })
            },
        )
        .optional()?;
    Ok(resume)
}

// ---------- categories ----------

pub fn categories(conn: &Connection) -> Result<Vec<Category>> {
    let mut stmt = conn.prepare("SELECT id, name FROM categories ORDER BY position, id")?;
    let categories = stmt.query_map([], |r| Ok(Category { id: r.get(0)?, name: r.get(1)? }))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(categories)
}

/// A category name, trimmed and checked against the ones in use.
fn free_name(conn: &Connection, name: &str, except: Option<i64>) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err(AppError::bad_request("A category name has 1 to 60 characters"));
    }
    let taken = conn.query_row("SELECT 1 FROM categories WHERE name = ?1 AND id IS NOT ?2", params![name, except], |_| Ok(())).optional()?;
    match taken {
        Some(()) => Err(AppError::new(409, "category_exists", "There is already a category with this name")),
        None => Ok(name.to_owned()),
    }
}

fn no_category() -> AppError {
    AppError::new(404, "unknown_category", "No such category")
}

pub fn add_category(conn: &Connection, name: &str) -> Result<Category> {
    let name = free_name(conn, name, None)?;
    conn.execute("INSERT INTO categories (name, position) VALUES (?1, (SELECT COALESCE(MAX(position), -1) + 1 FROM categories))", [&name])?;
    Ok(Category { id: conn.last_insert_rowid(), name })
}

pub fn rename_category(conn: &Connection, id: i64, name: &str) -> Result<()> {
    let name = free_name(conn, name, Some(id))?;
    match conn.execute("UPDATE categories SET name = ?2 WHERE id = ?1", params![id, name])? {
        0 => Err(no_category()),
        _ => Ok(()),
    }
}

/// Removes a category. Its titles stay in the library.
pub fn remove_category(conn: &Connection, id: i64) -> Result<()> {
    match conn.execute("DELETE FROM categories WHERE id = ?1", [id])? {
        0 => Err(no_category()),
        _ => Ok(()),
    }
}

/// Puts the categories in the order given. Any left out keep their order, after the others.
pub fn order_categories(conn: &mut Connection, ids: &[i64]) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute("UPDATE categories SET position = position + ?1", [ids.len() as i64 + 1_000_000])?;
    for (position, id) in ids.iter().enumerate() {
        tx.execute("UPDATE categories SET position = ?2 WHERE id = ?1", params![id, position as i64])?;
    }
    tx.commit()?;
    Ok(())
}

/// Sets the categories one title is in. Ids that do not exist are skipped.
pub fn set_categories(conn: &mut Connection, manga: i64, ids: &[i64]) -> Result<()> {
    bulk(conn, &[manga], &Bulk::Categories { ids: ids.to_vec(), replace: true })
}

// ---------- changes to many titles at once ----------

pub fn bulk(conn: &mut Connection, titles: &[i64], action: &Bulk) -> Result<()> {
    if titles.is_empty() {
        return Ok(());
    }
    // Numbers only, so the list can go into the statement as it is.
    let list = titles.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    let tx = conn.transaction()?;
    match action {
        Bulk::Read | Bulk::Unread => {
            let read = matches!(action, Bulk::Read);
            tx.execute(&format!("UPDATE chapters SET read = ?1, last_page_read = 0 WHERE manga_id IN ({list})"), [read])?;
        }
        Bulk::Remove => {
            tx.execute(&format!("UPDATE manga SET in_library = 0, added_at = NULL WHERE id IN ({list})"), [])?;
            tx.execute(&format!("DELETE FROM manga_categories WHERE manga_id IN ({list})"), [])?;
        }
        Bulk::Categories { ids, replace } => {
            if *replace {
                tx.execute(&format!("DELETE FROM manga_categories WHERE manga_id IN ({list})"), [])?;
            }
            for id in ids {
                tx.execute(
                    &format!(
                        "INSERT OR IGNORE INTO manga_categories (manga_id, category_id)
                         SELECT m.id, c.id FROM manga m, categories c WHERE c.id = ?1 AND m.id IN ({list})"
                    ),
                    [id],
                )?;
            }
        }
    }
    tx.commit()?;
    Ok(())
}

// ---------- history ----------

/// Notes that a chapter was read just now. Called whenever the reader saves its place.
pub fn remember(conn: &Connection, chapter: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO history (chapter_id, manga_id, read_at) SELECT id, manga_id, ?2 FROM chapters WHERE id = ?1
         ON CONFLICT(chapter_id) DO UPDATE SET read_at = excluded.read_at",
        params![chapter, now()],
    )?;
    Ok(())
}

/// Chapters read, the most recent first.
pub fn history(conn: &Connection, limit: i64, offset: i64) -> Result<Vec<HistoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT h.chapter_id, c.name, h.read_at, c.last_page_read, c.page_count, c.read, m.id, m.title, m.thumbnail_url
         FROM history h JOIN chapters c ON c.id = h.chapter_id JOIN manga m ON m.id = h.manga_id
         ORDER BY h.read_at DESC, h.chapter_id DESC LIMIT ?1 OFFSET ?2",
    )?;
    let entries = stmt
        .query_map([limit.clamp(1, 500), offset.max(0)], |r| {
            let manga_id: i64 = r.get(6)?;
            let thumbnail: Option<String> = r.get(8)?;
            Ok(HistoryEntry {
                chapter_id: r.get(0)?,
                chapter_name: r.get(1)?,
                read_at: r.get(2)?,
                page: r.get(3)?,
                page_count: r.get(4)?,
                read: r.get(5)?,
                manga_id,
                title: r.get(7)?,
                cover: cover_url(manga_id, thumbnail.as_deref()),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(entries)
}

/// Drops one entry from the history, or all of it. Reading progress is not touched.
pub fn forget(conn: &Connection, chapter: Option<i64>) -> Result<()> {
    match chapter {
        Some(id) => conn.execute("DELETE FROM history WHERE chapter_id = ?1", [id])?,
        None => conn.execute("DELETE FROM history", [])?,
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three library titles and one outside it, each with chapters newest first.
    fn database() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();
        crate::db::migrate(&mut conn).unwrap();
        for (id, title, in_library, chapters) in [(1, "Alpha", 1, 3), (2, "Beta", 1, 2), (3, "Gamma", 1, 0), (4, "Outside", 0, 1)] {
            conn.execute(
                "INSERT INTO manga (id, source_id, url, title, in_library, added_at, thumbnail_url) VALUES (?1, 7, ?2, ?2, ?3, 100, 'c')",
                params![id, title, in_library],
            )
            .unwrap();
            for n in 0..chapters {
                conn.execute(
                    "INSERT INTO chapters (id, manga_id, url, name, source_order, fetched_at, date_upload) VALUES (?1, ?2, ?3, ?3, ?4, 50, ?5)",
                    params![id * 10 + n, id, format!("ch{}", chapters - n), n, (chapters - n) * 1_000_000],
                )
                .unwrap();
            }
        }
        conn
    }

    #[test]
    fn library_counts() {
        let conn = database();
        conn.execute("UPDATE chapters SET read = 1 WHERE id = 12", []).unwrap();
        conn.execute("UPDATE chapters SET download_path = 'a.cbz' WHERE id IN (10, 11)", []).unwrap();
        let view = read(&conn).unwrap();
        let alpha = view.items.iter().find(|i| i.title == "Alpha").unwrap();
        assert_eq!((alpha.total, alpha.unread, alpha.downloaded, alpha.latest_at), (3, 2, 2, 3000));
        let gamma = view.items.iter().find(|i| i.title == "Gamma").unwrap();
        assert_eq!((gamma.total, gamma.unread, gamma.last_read_at), (0, 0, 0), "a title without chapters still shows");
        assert_eq!(view.items.len(), 3, "titles outside the library are left out");
        assert!(view.resume.is_none());
    }

    #[test]
    fn categories_and_bulk() {
        let mut conn = database();
        let reading = add_category(&conn, "  Reading ").unwrap();
        let later = add_category(&conn, "Later").unwrap();
        assert_eq!(reading.name, "Reading");
        assert_eq!(add_category(&conn, "reading").unwrap_err().code, "category_exists");
        assert!(add_category(&conn, "   ").is_err());
        rename_category(&conn, later.id, "Plan to read").unwrap();
        rename_category(&conn, later.id, "plan to read").unwrap();
        assert_eq!(rename_category(&conn, 99, "X").unwrap_err().code, "unknown_category");

        order_categories(&mut conn, &[later.id, reading.id]).unwrap();
        assert_eq!(categories(&conn).unwrap().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["plan to read", "Reading"]);

        let cats = |conn: &Connection, title: &str| read(conn).unwrap().items.into_iter().find(|i| i.title == title).unwrap().categories;
        bulk(&mut conn, &[1, 2], &Bulk::Categories { ids: vec![reading.id, 99], replace: false }).unwrap();
        set_categories(&mut conn, 1, &[later.id]).unwrap();
        assert_eq!(cats(&conn, "Alpha"), [later.id]);
        assert_eq!(cats(&conn, "Beta"), [reading.id]);

        bulk(&mut conn, &[1], &Bulk::Read).unwrap();
        assert_eq!(read(&conn).unwrap().items.iter().find(|i| i.id == 1).unwrap().unread, 0);
        bulk(&mut conn, &[1], &Bulk::Unread).unwrap();
        bulk(&mut conn, &[2], &Bulk::Remove).unwrap();
        let view = read(&conn).unwrap();
        assert_eq!(view.items.iter().map(|i| (i.id, i.unread)).collect::<Vec<_>>().len(), 2);
        assert!(view.items.iter().all(|i| i.id != 2));

        remove_category(&conn, later.id).unwrap();
        assert!(cats(&conn, "Alpha").is_empty(), "a removed category lets go of its titles");
    }

    #[test]
    fn history_and_resume() {
        let conn = database();
        // Alpha's chapters are 10 (newest), 11, 12 (oldest).
        remember(&conn, 12).unwrap();
        conn.execute("UPDATE history SET read_at = 10 WHERE chapter_id = 12", []).unwrap();
        remember(&conn, 20).unwrap();
        conn.execute("UPDATE history SET read_at = 20 WHERE chapter_id = 20", []).unwrap();
        remember(&conn, 12).unwrap();
        remember(&conn, 999).unwrap();
        let entries = history(&conn, 10, 0).unwrap();
        assert_eq!(entries.iter().map(|e| e.chapter_id).collect::<Vec<_>>(), [12, 20], "one entry per chapter, latest first");

        conn.execute("UPDATE chapters SET last_page_read = 4 WHERE id = 12", []).unwrap();
        let resume = read(&conn).unwrap().resume.unwrap();
        assert_eq!((resume.chapter_id, resume.page), (12, 4), "an unfinished chapter is where to go on");
        conn.execute("UPDATE chapters SET read = 1 WHERE id = 12", []).unwrap();
        assert_eq!(read(&conn).unwrap().resume.unwrap().chapter_id, 11, "after it, the next one up the list");
        assert_eq!(read(&conn).unwrap().items.iter().find(|i| i.id == 1).unwrap().last_read_at > 0, true);

        forget(&conn, Some(12)).unwrap();
        assert_eq!(history(&conn, 10, 0).unwrap().len(), 1);
        forget(&conn, None).unwrap();
        assert!(history(&conn, 10, 0).unwrap().is_empty());
        conn.execute("DELETE FROM chapters WHERE id = 20", []).unwrap();
    }
}
