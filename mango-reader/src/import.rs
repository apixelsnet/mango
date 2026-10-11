//! Import of a Mihon or Suwayomi backup: a `.tachibk`, which is a gzipped protobuf.
//! Both apps write the same format. Pure Rust; the extension host is not needed.
//!
//! The file is untrusted. Its unpacked size and its lists are capped, and nothing in
//! it is used as a path. Titles are decoded one at a time, so memory holds the
//! unpacked file and one title, never the whole library as objects.
//!
//! Importing merges and never overwrites: a chapter read on either side stays read,
//! the further page wins, and what Mango already knows about a title is kept. So
//! importing the same file twice changes nothing the second time.

use crate::{
    db::Db,
    error::{AppError, Result},
};
use prost::{
    bytes::Buf,
    encoding::{decode_key, decode_varint, skip_field, DecodeContext, WireType},
    Message,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap},
    io::Read,
    sync::{Arc, Mutex},
};

/// Cap on the upload, and on what it may unpack to.
pub const MAX_UPLOAD: usize = 128 << 20;
const MAX_UNPACKED: u64 = 768 << 20;
const MAX_TITLES: usize = 200_000;
const MAX_CHAPTERS: usize = 100_000;
const MAX_CATEGORIES: usize = 10_000;
/// Reasons listed in a report; beyond this only the count grows.
const MAX_SKIPPED: usize = 200;

// ---------- the backup's shape (Mihon's `Backup*` classes, by field number) ----------

#[derive(Clone, PartialEq, Message)]
pub struct BackupManga {
    #[prost(int64, tag = "1")]
    pub source: i64,
    #[prost(string, tag = "2")]
    pub url: String,
    #[prost(string, tag = "3")]
    pub title: String,
    #[prost(string, optional, tag = "4")]
    pub artist: Option<String>,
    #[prost(string, optional, tag = "5")]
    pub author: Option<String>,
    #[prost(string, optional, tag = "6")]
    pub description: Option<String>,
    #[prost(string, repeated, tag = "7")]
    pub genre: Vec<String>,
    #[prost(int32, tag = "8")]
    pub status: i32,
    #[prost(string, optional, tag = "9")]
    pub thumbnail_url: Option<String>,
    /// Epoch milliseconds, like every time in a backup.
    #[prost(int64, tag = "13")]
    pub date_added: i64,
    #[prost(message, repeated, tag = "16")]
    pub chapters: Vec<BackupChapter>,
    /// The `order` of each category the title is in.
    #[prost(int64, repeated, tag = "17")]
    pub categories: Vec<i64>,
    #[prost(message, repeated, tag = "18")]
    pub tracking: Vec<BackupTracking>,
    /// Left out of the file when true.
    #[prost(bool, optional, tag = "100")]
    pub favorite: Option<bool>,
    /// The low three bits are the reading mode.
    #[prost(int32, optional, tag = "103")]
    pub viewer_flags: Option<i32>,
    #[prost(message, repeated, tag = "104")]
    pub history: Vec<BackupHistory>,
    /// 0 always update, 1 only fetch once.
    #[prost(int32, tag = "105")]
    pub update_strategy: i32,
}

#[derive(Clone, PartialEq, Message)]
pub struct BackupChapter {
    #[prost(string, tag = "1")]
    pub url: String,
    #[prost(string, tag = "2")]
    pub name: String,
    #[prost(string, optional, tag = "3")]
    pub scanlator: Option<String>,
    #[prost(bool, tag = "4")]
    pub read: bool,
    #[prost(bool, tag = "5")]
    pub bookmark: bool,
    #[prost(int64, tag = "6")]
    pub last_page_read: i64,
    #[prost(int64, tag = "7")]
    pub date_fetch: i64,
    #[prost(int64, tag = "8")]
    pub date_upload: i64,
    #[prost(float, tag = "9")]
    pub chapter_number: f32,
    #[prost(int64, tag = "10")]
    pub source_order: i64,
}

#[derive(Clone, PartialEq, Message)]
pub struct BackupCategory {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(int64, tag = "2")]
    pub order: i64,
}

#[derive(Clone, PartialEq, Message)]
pub struct BackupHistory {
    #[prost(string, tag = "1")]
    pub url: String,
    #[prost(int64, tag = "2")]
    pub last_read: i64,
}

#[derive(Clone, PartialEq, Message)]
pub struct BackupSource {
    #[prost(string, tag = "1")]
    pub name: String,
    #[prost(int64, tag = "2")]
    pub source_id: i64,
}

#[derive(Clone, PartialEq, Message)]
pub struct BackupTracking {
    #[prost(int32, tag = "1")]
    pub sync_id: i32,
    #[prost(int64, tag = "2")]
    pub library_id: i64,
    #[prost(int32, tag = "3")]
    pub media_id_int: i32,
    #[prost(string, tag = "4")]
    pub tracking_url: String,
    #[prost(string, tag = "5")]
    pub title: String,
    #[prost(float, tag = "6")]
    pub last_chapter_read: f32,
    #[prost(int32, tag = "7")]
    pub total_chapters: i32,
    #[prost(float, tag = "8")]
    pub score: f32,
    #[prost(int32, tag = "9")]
    pub status: i32,
    #[prost(int64, tag = "10")]
    pub started_reading_date: i64,
    #[prost(int64, tag = "11")]
    pub finished_reading_date: i64,
    #[prost(int64, tag = "100")]
    pub media_id: i64,
}

// ---------- what the API answers ----------

/// A source the backup refers to, and whether Mango can read from it.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SourceNeed {
    pub id: String,
    pub name: String,
    pub titles: usize,
    pub installed: bool,
    /// The extension that provides it, when an added store lists one.
    pub pkg: Option<String>,
    pub store_id: Option<i64>,
    pub extension: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub titles: usize,
    pub in_library: usize,
    pub chapters: usize,
    pub read_chapters: usize,
    pub tracked: usize,
    pub categories: Vec<String>,
    pub sources: Vec<SourceNeed>,
    /// Entries that could not be read at all.
    pub unreadable: usize,
}

#[derive(Serialize, Clone, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub titles_added: usize,
    pub titles_merged: usize,
    pub chapters_added: usize,
    /// Chapters Mango already had whose read state, page or bookmark moved forward.
    pub chapters_updated: usize,
    pub categories_added: usize,
    pub history_entries: usize,
    pub tracker_links: usize,
    pub skipped: usize,
    /// Why, for the first of them: `title: reason`.
    pub reasons: Vec<String>,
    pub sources: Vec<SourceNeed>,
}

#[derive(Serialize, Clone, Default)]
pub struct Status {
    pub running: bool,
    pub done: usize,
    pub total: usize,
    pub report: Option<Report>,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct Importer {
    db: Db,
    status: Arc<Mutex<Status>>,
    /// Held while a backup is unpacked to be looked at: one at a time, since it sits in memory whole.
    reading: Arc<tokio::sync::Mutex<()>>,
}

impl Importer {
    pub fn new(db: Db) -> Self {
        Self { db, status: Arc::default(), reading: Arc::default() }
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// What the file holds, without changing anything.
    pub async fn preview(&self, file: Vec<u8>) -> Result<Preview> {
        let Ok(_one_at_a_time) = self.reading.try_lock() else {
            return Err(AppError::new(409, "import_running", "Another backup is being read. Try again when it is done."));
        };
        let tally = crate::local::blocking(move || tally(&unpack(&file)?)).await?;
        let sources = self.db.call(move |conn| needs(conn, &tally.per_source, &tally.source_names).map(|sources| (sources, tally))).await?;
        let (sources, tally) = sources;
        Ok(Preview {
            titles: tally.titles,
            in_library: tally.in_library,
            chapters: tally.chapters,
            read_chapters: tally.read_chapters,
            tracked: tally.tracked,
            categories: tally.categories,
            sources,
            unreadable: tally.unreadable,
        })
    }

    /// Starts the import in the background. It works on a connection of its own, in one
    /// transaction: readers carry on meanwhile, and a failure leaves the library as it was.
    pub fn start(&self, file: Vec<u8>) -> Result<Status> {
        {
            let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
            if status.running {
                return Err(AppError::new(409, "import_running", "An import is already running"));
            }
            *status = Status { running: true, ..Status::default() };
        }
        let (db, shared) = (self.db.clone(), self.status.clone());
        tokio::task::spawn_blocking(move || {
            let progress = |done: usize, total: usize| {
                let mut status = shared.lock().unwrap_or_else(|e| e.into_inner());
                (status.done, status.total) = (done, total);
            };
            let outcome = db.second().and_then(|mut conn| run(&mut conn, &file, &progress));
            let mut status = shared.lock().unwrap_or_else(|e| e.into_inner());
            status.running = false;
            match outcome {
                Ok(report) => {
                    tracing::info!("import: {} titles added, {} merged, {} chapters added", report.titles_added, report.titles_merged, report.chapters_added);
                    status.report = Some(report);
                }
                Err(e) => status.error = Some(e.message),
            }
        });
        Ok(self.status())
    }
}

// ---------- reading the file ----------

fn damaged(_: prost::DecodeError) -> AppError {
    AppError::new(422, "bad_backup", "This file is not a readable Mihon or Suwayomi backup")
}

/// Undoes the gzip, up to the cap.
pub(crate) fn unpack(file: &[u8]) -> Result<Vec<u8>> {
    if !file.starts_with(&[0x1f, 0x8b]) {
        return Err(AppError::new(422, "bad_backup", "This is not a .tachibk backup: it is not gzip-compressed"));
    }
    let mut data = Vec::new();
    flate2::read::GzDecoder::new(file)
        .take(MAX_UNPACKED + 1)
        .read_to_end(&mut data)
        .map_err(|_| AppError::new(422, "bad_backup", "This backup is damaged: it does not unpack"))?;
    if data.len() as u64 > MAX_UNPACKED {
        return Err(AppError::new(422, "bad_backup", "This backup unpacks to more than Mango accepts"));
    }
    Ok(data)
}

/// Calls `each` with every top-level field of the backup that has the wanted number.
/// Titles are field 1, categories 2, source names 101.
pub(crate) fn fields<'a>(mut data: &'a [u8], wanted: u32, mut each: impl FnMut(&'a [u8]) -> Result<()>) -> Result<()> {
    while data.has_remaining() {
        let (number, wire) = decode_key(&mut data).map_err(damaged)?;
        if wire != WireType::LengthDelimited {
            skip_field(wire, number, &mut data, DecodeContext::default()).map_err(damaged)?;
            continue;
        }
        let length = decode_varint(&mut data).map_err(damaged)? as usize;
        if length > data.len() {
            return Err(AppError::new(422, "bad_backup", "This backup is damaged: it ends too early"));
        }
        let (field, rest) = data.split_at(length);
        data = rest;
        if number == wanted {
            each(field)?;
        }
    }
    Ok(())
}

fn categories_of(data: &[u8]) -> Result<Vec<BackupCategory>> {
    let mut categories = Vec::new();
    fields(data, 2, |field| {
        if categories.len() < MAX_CATEGORIES {
            categories.extend(BackupCategory::decode(field).ok().filter(|c| !c.name.trim().is_empty()));
        }
        Ok(())
    })?;
    Ok(categories)
}

struct Tally {
    titles: usize,
    in_library: usize,
    chapters: usize,
    read_chapters: usize,
    tracked: usize,
    unreadable: usize,
    categories: Vec<String>,
    per_source: BTreeMap<i64, usize>,
    source_names: HashMap<i64, String>,
}

fn source_names(data: &[u8]) -> Result<HashMap<i64, String>> {
    let mut names = HashMap::new();
    fields(data, 101, |field| {
        if let Ok(source) = BackupSource::decode(field) {
            names.insert(source.source_id, source.name);
        }
        Ok(())
    })?;
    Ok(names)
}

fn tally(data: &[u8]) -> Result<Tally> {
    let mut tally = Tally {
        titles: 0,
        in_library: 0,
        chapters: 0,
        read_chapters: 0,
        tracked: 0,
        unreadable: 0,
        categories: categories_of(data)?.into_iter().map(|c| c.name).collect(),
        per_source: BTreeMap::new(),
        source_names: source_names(data)?,
    };
    fields(data, 1, |field| {
        match BackupManga::decode(field) {
            Ok(manga) if !manga.url.is_empty() => {
                tally.titles += 1;
                tally.in_library += usize::from(manga.favorite.unwrap_or(true));
                tally.chapters += manga.chapters.len();
                tally.read_chapters += manga.chapters.iter().filter(|c| c.read).count();
                tally.tracked += usize::from(!manga.tracking.is_empty());
                *tally.per_source.entry(manga.source).or_default() += 1;
            }
            _ => tally.unreadable += 1,
        }
        Ok(())
    })?;
    Ok(tally)
}

/// For each source the backup uses: is it installed, and if not, which extension would provide it.
fn needs(conn: &Connection, per_source: &BTreeMap<i64, usize>, names: &HashMap<i64, String>) -> Result<Vec<SourceNeed>> {
    let mut sources = Vec::new();
    for (&id, &titles) in per_source {
        let installed: Option<String> = conn.query_row("SELECT name FROM sources WHERE id = ?1", [id], |r| r.get(0)).optional()?;
        let offer: Option<(String, i64, String)> = match installed {
            Some(_) => None,
            None => conn
                .query_row(
                    "SELECT c.pkg, c.store_id, c.name FROM catalog_sources s JOIN catalog c ON c.store_id = s.store_id AND c.pkg = s.pkg
                     WHERE s.source_id = ?1 AND c.jar_url IS NOT NULL ORDER BY c.version_code DESC LIMIT 1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?,
        };
        let name = installed.clone().or_else(|| names.get(&id).cloned()).filter(|name| !name.is_empty());
        sources.push(SourceNeed {
            id: id.to_string(),
            name: name.unwrap_or_else(|| format!("Source {id}")),
            titles,
            installed: installed.is_some(),
            pkg: offer.as_ref().map(|(pkg, ..)| pkg.clone()),
            store_id: offer.as_ref().map(|(_, store, _)| *store),
            extension: offer.map(|(.., name)| name),
        });
    }
    // The ones that need doing first.
    sources.sort_by_key(|source| (source.installed, std::cmp::Reverse(source.titles)));
    Ok(sources)
}

// ---------- importing ----------

fn reader_mode(viewer_flags: Option<i32>) -> Option<&'static str> {
    match viewer_flags? & 7 {
        1 => Some("ltr"),
        2 => Some("rtl"),
        3 => Some("vertical"),
        4 | 5 => Some("webtoon"),
        _ => None,
    }
}

/// Imports the whole file in one transaction. `progress` is told `(titles done, titles in all)`.
pub fn run(conn: &mut Connection, file: &[u8], progress: &dyn Fn(usize, usize)) -> Result<Report> {
    let data = unpack(file)?;
    let mut total = 0;
    fields(&data, 1, |_| {
        total += 1;
        Ok(())
    })?;
    if total > MAX_TITLES {
        return Err(AppError::new(422, "bad_backup", "This backup holds more titles than Mango accepts"));
    }
    let mut report = Report::default();
    let mut per_source = BTreeMap::new();
    let tx = conn.transaction()?;

    // Categories are matched by name; the backup refers to them by their `order`.
    let mut category_ids: HashMap<i64, i64> = HashMap::new();
    for category in categories_of(&data)? {
        let name = category.name.trim();
        let known: Option<i64> = tx.query_row("SELECT id FROM categories WHERE name = ?1", [name], |r| r.get(0)).optional()?;
        let id = match known {
            Some(id) => id,
            None => {
                tx.execute("INSERT INTO categories (name, position) VALUES (?1, (SELECT COALESCE(MAX(position), -1) + 1 FROM categories))", [name])?;
                report.categories_added += 1;
                tx.last_insert_rowid()
            }
        };
        category_ids.insert(category.order, id);
    }

    let mut done = 0;
    fields(&data, 1, |field| {
        done += 1;
        if done % 25 == 0 {
            progress(done, total);
        }
        let skip = |report: &mut Report, title: &str, reason: &str| {
            report.skipped += 1;
            if report.reasons.len() < MAX_SKIPPED {
                report.reasons.push(format!("{}: {reason}", if title.is_empty() { "(no title)" } else { title }));
            }
        };
        let manga = match BackupManga::decode(field) {
            Ok(manga) => manga,
            Err(_) => {
                skip(&mut report, "", "the entry could not be read");
                return Ok(());
            }
        };
        if manga.url.is_empty() {
            skip(&mut report, &manga.title, "it has no address at its source");
            return Ok(());
        }
        if manga.chapters.len() > MAX_CHAPTERS {
            skip(&mut report, &manga.title, "it has more chapters than Mango accepts");
            return Ok(());
        }
        *per_source.entry(manga.source).or_default() += 1;
        title(&tx, &manga, &category_ids, &mut report)
    })?;

    report.sources = needs(&tx, &per_source, &source_names(&data)?)?;
    tx.commit()?;
    progress(total, total);
    Ok(report)
}

/// One title: the title itself, its chapters, categories, history and tracker links.
fn title(tx: &Connection, manga: &BackupManga, category_ids: &HashMap<i64, i64>, report: &mut Report) -> Result<()> {
    let favorite = manga.favorite.unwrap_or(true);
    let added = (manga.date_added > 0).then_some(manga.date_added / 1000).filter(|_| favorite);
    let genre = (!manga.genre.is_empty()).then(|| manga.genre.join(", "));
    let strategy = if manga.update_strategy == 1 { "ONLY_FETCH_ONCE" } else { "ALWAYS_UPDATE" };
    let known: Option<i64> =
        tx.query_row("SELECT id FROM manga WHERE source_id = ?1 AND url = ?2", params![manga.source, manga.url], |r| r.get(0)).optional()?;
    let id = match known {
        Some(id) => {
            // Mango's own details stand. The backup only fills what is still empty, and can add the title to the library.
            tx.execute(
                "UPDATE manga SET in_library = in_library OR ?2, added_at = COALESCE(added_at, ?3), reader_mode = COALESCE(reader_mode, ?4),
                     title = CASE WHEN initialized OR ?5 = '' THEN title ELSE ?5 END,
                     artist = COALESCE(artist, ?6), author = COALESCE(author, ?7), description = COALESCE(description, ?8),
                     genre = COALESCE(genre, ?9), thumbnail_url = COALESCE(thumbnail_url, ?10),
                     status = CASE WHEN status = 0 THEN ?11 ELSE status END
                 WHERE id = ?1",
                params![id, favorite, added, reader_mode(manga.viewer_flags), manga.title, manga.artist, manga.author, manga.description, genre, manga.thumbnail_url, manga.status],
            )?;
            report.titles_merged += 1;
            id
        }
        None => {
            tx.execute(
                "INSERT INTO manga (source_id, url, title, artist, author, description, genre, status, thumbnail_url, update_strategy,
                     initialized, in_library, added_at, reader_mode)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    manga.source,
                    manga.url,
                    manga.title,
                    manga.artist,
                    manga.author,
                    manga.description,
                    genre,
                    manga.status,
                    manga.thumbnail_url,
                    strategy,
                    manga.description.is_some() || !manga.chapters.is_empty(),
                    favorite,
                    added,
                    reader_mode(manga.viewer_flags)
                ],
            )?;
            report.titles_added += 1;
            tx.last_insert_rowid()
        }
    };

    // Chapters, by their address. `(id, read, last page, bookmark)` of the ones Mango has.
    let mut chapters: HashMap<String, (i64, bool, i64, bool)> = {
        let mut stmt = tx.prepare_cached("SELECT url, id, read, last_page_read, bookmark FROM chapters WHERE manga_id = ?1")?;
        let rows = stmt.query_map([id], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))))?.collect::<rusqlite::Result<_>>()?;
        rows
    };
    for chapter in manga.chapters.iter().filter(|c| !c.url.is_empty()) {
        match chapters.get(&chapter.url).copied() {
            Some((chapter_id, read, page, bookmark)) => {
                // Read on either side is read; an unfinished chapter keeps the further page.
                let now_read = read || chapter.read;
                let now_page = if now_read { page } else { page.max(chapter.last_page_read) };
                let now_bookmark = bookmark || chapter.bookmark;
                if (now_read, now_page, now_bookmark) != (read, page, bookmark) {
                    tx.prepare_cached("UPDATE chapters SET read = ?2, last_page_read = ?3, bookmark = ?4 WHERE id = ?1")?
                        .execute(params![chapter_id, now_read, now_page, now_bookmark])?;
                    report.chapters_updated += 1;
                }
            }
            None => {
                let seen = if chapter.date_fetch > 0 { chapter.date_fetch } else { chapter.date_upload } / 1000;
                tx.prepare_cached(
                    "INSERT INTO chapters (manga_id, url, name, date_upload, chapter_number, scanlator, source_order, fetched_at, read, last_page_read, bookmark)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                )?
                .execute(params![
                    id,
                    chapter.url,
                    chapter.name,
                    chapter.date_upload.max(0),
                    f64::from(chapter.chapter_number),
                    chapter.scanlator,
                    chapter.source_order,
                    seen.max(0),
                    chapter.read,
                    chapter.last_page_read.max(0),
                    chapter.bookmark
                ])?;
                chapters.insert(chapter.url.clone(), (tx.last_insert_rowid(), chapter.read, chapter.last_page_read, chapter.bookmark));
                report.chapters_added += 1;
            }
        }
    }

    for order in &manga.categories {
        if let Some(category) = category_ids.get(order) {
            tx.prepare_cached("INSERT OR IGNORE INTO manga_categories (manga_id, category_id) VALUES (?1, ?2)")?.execute(params![id, category])?;
        }
    }

    for entry in manga.history.iter().filter(|h| h.last_read > 0) {
        let Some((chapter_id, ..)) = chapters.get(&entry.url) else { continue };
        let changed = tx
            .prepare_cached(
                "INSERT INTO history (chapter_id, manga_id, read_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(chapter_id) DO UPDATE SET read_at = excluded.read_at WHERE excluded.read_at > history.read_at",
            )?
            .execute(params![chapter_id, id, entry.last_read / 1000])?;
        report.history_entries += changed;
    }

    for track in &manga.tracking {
        let media = if track.media_id != 0 { track.media_id } else { i64::from(track.media_id_int) };
        let changed = tx
            .prepare_cached(
                "INSERT INTO tracks (manga_id, tracker, library_id, media_id, url, title, last_chapter_read, total_chapters, score, status, started_at, finished_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                 ON CONFLICT(manga_id, tracker) DO NOTHING",
            )?
            .execute(params![
                id,
                track.sync_id,
                track.library_id,
                media,
                // Shown as a link: nothing but a web address will do.
                Some(track.tracking_url.as_str()).filter(|url| url.starts_with("https://")).unwrap_or_default(),
                track.title,
                f64::from(track.last_chapter_read),
                track.total_chapters,
                f64::from(track.score),
                track.status,
                track.started_reading_date,
                track.finished_reading_date
            ])?;
        report.tracker_links += changed;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Mihon's whole backup. The importer never decodes it in one piece; the tests build it this way.
    #[derive(Clone, PartialEq, Message)]
    struct Backup {
        #[prost(message, repeated, tag = "1")]
        manga: Vec<BackupManga>,
        #[prost(message, repeated, tag = "2")]
        categories: Vec<BackupCategory>,
        #[prost(message, repeated, tag = "101")]
        sources: Vec<BackupSource>,
        #[prost(string, repeated, tag = "104")]
        other: Vec<String>,
    }

    fn chapter(url: &str, read: bool, page: i64) -> BackupChapter {
        BackupChapter { url: url.into(), name: format!("Chapter {url}"), read, last_page_read: page, date_fetch: 2_000_000, source_order: 0, ..Default::default() }
    }

    fn file() -> Vec<u8> {
        let backup = Backup {
            manga: vec![
                BackupManga {
                    source: 7,
                    url: "/a".into(),
                    title: "Alpha".into(),
                    description: Some("About Alpha".into()),
                    genre: vec!["Action".into(), "Drama".into()],
                    date_added: 5_000_000,
                    chapters: vec![chapter("/a/2", false, 6), chapter("/a/1", true, 0), BackupChapter { bookmark: true, ..chapter("/a/0", false, 0) }],
                    categories: vec![40],
                    tracking: vec![BackupTracking { sync_id: 2, media_id: 99, title: "Alpha".into(), score: 8.5, ..Default::default() }],
                    viewer_flags: Some(4),
                    history: vec![BackupHistory { url: "/a/1".into(), last_read: 9_000_000 }, BackupHistory { url: "/gone".into(), last_read: 1 }],
                    ..Default::default()
                },
                BackupManga { source: 8, url: "/b".into(), title: "Beta".into(), favorite: Some(false), update_strategy: 1, ..Default::default() },
                BackupManga { source: 8, title: "No address".into(), ..Default::default() },
            ],
            categories: vec![BackupCategory { name: "Reading".into(), order: 40 }, BackupCategory { name: "  ".into(), order: 41 }],
            sources: vec![BackupSource { name: "Seven".into(), source_id: 7 }],
            other: vec!["a setting".into()],
        };
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&backup.encode_to_vec()).unwrap();
        gz.finish().unwrap()
    }

    fn database() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();
        crate::db::migrate(&mut conn).unwrap();
        conn
    }

    /// The rows an import may touch, as text, to see whether a second import changed any.
    fn snapshot(conn: &Connection) -> String {
        let mut out = String::new();
        for sql in [
            "SELECT id, source_id, url, title, description, genre, in_library, added_at, reader_mode, update_strategy, initialized FROM manga ORDER BY id",
            "SELECT id, manga_id, url, read, last_page_read, bookmark, fetched_at FROM chapters ORDER BY id",
            "SELECT id, name, position FROM categories ORDER BY id",
            "SELECT manga_id, category_id FROM manga_categories ORDER BY 1, 2",
            "SELECT chapter_id, read_at FROM history ORDER BY 1",
            "SELECT manga_id, tracker, media_id, score FROM tracks ORDER BY 1, 2",
        ] {
            let mut stmt = conn.prepare(sql).unwrap();
            let columns = stmt.column_count();
            let mut rows = stmt.query([]).unwrap();
            while let Some(row) = rows.next().unwrap() {
                for column in 0..columns {
                    out += &format!("{:?}|", row.get_ref(column).unwrap());
                }
                out.push('\n');
            }
        }
        out
    }

    #[test]
    fn previews_without_touching_anything() {
        let conn = database();
        conn.execute_batch(
            "INSERT INTO stores (id, url, index_url, name, signing_key, added_at) VALUES (1, 'u', 'i', 'S', 'k', 0);
             INSERT INTO catalog (store_id, pkg, name, version_name, version_code, lib, content_warning, jar_url, langs, source_count, search)
                 VALUES (1, 'ext.eight', 'Eight Ext', '1', 1, '1.6', 0, 'https://example.org/e.jar', ',en,', 1, 'eight');
             INSERT INTO catalog_sources (store_id, pkg, source_id, name, lang) VALUES (1, 'ext.eight', 8, 'Eight', 'en');",
        )
        .unwrap();
        let tally = tally(&unpack(&file()).unwrap()).unwrap();
        assert_eq!((tally.titles, tally.in_library, tally.chapters, tally.read_chapters, tally.tracked, tally.unreadable), (2, 1, 3, 1, 1, 1));
        assert_eq!(tally.categories, ["Reading"]);
        let sources = needs(&conn, &tally.per_source, &tally.source_names).unwrap();
        assert_eq!(sources.iter().map(|s| (s.name.as_str(), s.titles, s.pkg.as_deref())).collect::<Vec<_>>(), [("Seven", 1, None), ("Source 8", 1, Some("ext.eight"))]);
        assert_eq!(snapshot(&conn), "", "a preview writes nothing");
    }

    #[test]
    fn imports_then_changes_nothing_the_second_time() {
        let mut conn = database();
        let report = run(&mut conn, &file(), &|_, _| {}).unwrap();
        assert_eq!((report.titles_added, report.titles_merged, report.chapters_added, report.categories_added), (2, 0, 3, 1));
        assert_eq!((report.history_entries, report.tracker_links, report.skipped), (1, 1, 1));
        assert!(report.reasons[0].starts_with("No address:"));

        let alpha: (String, bool, i64, String, String) = conn
            .query_row("SELECT genre, in_library, added_at, reader_mode, update_strategy FROM manga WHERE url = '/a'", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })
            .unwrap();
        assert_eq!(alpha, ("Action, Drama".into(), true, 5000, "webtoon".into(), "ALWAYS_UPDATE".into()));
        let beta: (bool, Option<i64>, String) =
            conn.query_row("SELECT in_library, added_at, update_strategy FROM manga WHERE url = '/b'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!(beta, (false, None, "ONLY_FETCH_ONCE".into()));
        let view = crate::library::read(&conn).unwrap();
        assert_eq!((view.items.len(), view.items[0].unread, view.items[0].last_read_at, view.categories.len()), (1, 2, 9000, 1));
        assert_eq!(view.items[0].categories.len(), 1);

        let before = snapshot(&conn);
        let again = run(&mut conn, &file(), &|_, _| {}).unwrap();
        assert_eq!((again.titles_added, again.titles_merged, again.chapters_added, again.chapters_updated), (0, 2, 0, 0));
        assert_eq!((again.categories_added, again.history_entries, again.tracker_links), (0, 0, 0));
        assert_eq!(snapshot(&conn), before, "the same file a second time changes nothing");
    }

    #[test]
    fn merges_and_never_takes_progress_back() {
        let mut conn = database();
        // Mango already knows Alpha: one chapter further along than the backup, one behind it.
        conn.execute_batch(
            "INSERT INTO manga (id, source_id, url, title, description, initialized, in_library) VALUES (1, 7, '/a', 'Alpha, as Mango calls it', 'Ours', 1, 0);
             INSERT INTO chapters (id, manga_id, url, name, source_order, fetched_at, read, last_page_read) VALUES
                 (1, 1, '/a/2', 'Two', 0, 1, 0, 30), (2, 1, '/a/1', 'One', 1, 1, 0, 3), (3, 1, '/a/9', 'Only here', 2, 1, 1, 0);",
        )
        .unwrap();
        let report = run(&mut conn, &file(), &|_, _| {}).unwrap();
        assert_eq!((report.titles_added, report.titles_merged, report.chapters_added, report.chapters_updated), (1, 1, 1, 1));

        let state = |url: &str| -> (bool, i64) {
            conn.query_row("SELECT read, last_page_read FROM chapters WHERE manga_id = 1 AND url = ?1", [url], |r| Ok((r.get(0)?, r.get(1)?))).unwrap()
        };
        assert_eq!(state("/a/2"), (false, 30), "Mango's further page stands");
        assert_eq!(state("/a/1"), (true, 3), "read in the backup means read");
        assert_eq!(state("/a/9"), (true, 0), "what only Mango has is left alone");
        let ours: (String, String, bool) = conn.query_row("SELECT title, description, in_library FROM manga WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!(ours, ("Alpha, as Mango calls it".into(), "Ours".into(), true), "details stay, the library gains the title");
    }

    #[test]
    fn refuses_what_is_not_a_backup() {
        let mut conn = database();
        assert_eq!(run(&mut conn, b"plain text", &|_, _| {}).unwrap_err().code, "bad_backup");
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&[0x0a, 0xff, 0xff, 0xff, 0x0f, 1, 2, 3]).unwrap();
        assert_eq!(run(&mut conn, &gz.finish().unwrap(), &|_, _| {}).unwrap_err().code, "bad_backup");
        assert_eq!(snapshot(&conn), "", "a refused file leaves nothing behind");
        assert_eq!(reader_mode(Some(0b1010)), Some("rtl"));
    }
}
