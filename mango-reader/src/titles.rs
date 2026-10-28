//! Titles and chapters: what browsing finds, the title page, reading progress.
//!
//! Every title a source shows is stored, in the library or not, so it has an id to
//! open, a cover to cache and a place for its chapters.

use crate::{
    db::{now, Db},
    error::{AppError, Result},
    host::{Call, Host},
    local::{self, Local},
    migrate::{key, Key},
    settings,
};
use reqwest::Method;
use rusqlite::{params, types::Value as Sql, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};

pub const READER_MODES: [&str; 5] = ["ltr", "rtl", "vertical", "webtoon", "sideways"];
/// The detail fields that can be edited by hand. An edited one is locked: a refresh from
/// the source leaves it as it is.
pub const LOCKABLE: [&str; 7] = ["title", "author", "artist", "description", "genre", "status", "cover"];
/// A cover set by hand is named in `thumbnail_url` by this, then a mark of its file.
pub const CUSTOM_COVER: &str = "custom:";
/// Reading a chapter to its end marks its other uploads read too.
pub const MARK_DUPLICATES: &str = "chapters.markDuplicates";
/// Pages a reading app streams over OPDS move the chapter's place on. On unless set false.
pub const OPDS_PROGRESS: &str = "opds.progress";
/// The most scanlation groups a title keeps an order for.
const MAX_GROUPS: usize = 100;
pub const READER_FITS: [&str; 4] = ["width", "height", "screen", "original"];
/// The most a title's own reader settings may weigh, as JSON text.
const MAX_READER_PREFS: usize = 4 * 1024;

const CHAPTER_COLUMNS: &str =
    "id, name, chapter_number, scanlator, date_upload, fetched_at, read, last_page_read, page_count, download_path IS NOT NULL, bookmark, manga_id";

const DAY_MS: i64 = 86_400_000;
/// The longest a title is taken to wait between two chapters, in days.
pub const MAX_RELEASE_DAYS: i64 = 28;

// ---------- protocol v1 shapes ----------

fn always_update() -> String {
    "ALWAYS_UPDATE".into()
}

fn empty_object() -> Value {
    json!({})
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WireManga {
    pub url: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub status: i64,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default = "always_update")]
    pub update_strategy: String,
    #[serde(default)]
    pub initialized: bool,
    #[serde(default = "empty_object")]
    pub memo: Value,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WireChapter {
    pub url: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub date_upload: i64,
    #[serde(default = "unknown_number")]
    pub chapter_number: f64,
    #[serde(default)]
    pub scanlator: Option<String>,
    #[serde(default = "empty_object")]
    pub memo: Value,
}

fn unknown_number() -> f64 {
    -1.0
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WirePage {
    pub index: i64,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub image_url: Option<String>,
}

// ---------- what the API answers ----------

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub id: i64,
    pub source_id: String,
    pub title: String,
    pub cover: Option<String>,
    pub in_library: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub items: Vec<Card>,
    pub has_next_page: bool,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChapterView {
    pub id: i64,
    pub name: String,
    /// -1 when neither the source nor the name gives one.
    pub number: f64,
    pub scanlator: Option<String>,
    /// Epoch milliseconds, 0 when unknown.
    pub date_upload: i64,
    pub fetched_at: i64,
    pub read: bool,
    pub last_page_read: i64,
    pub page_count: Option<i64>,
    pub downloaded: bool,
    pub bookmark: bool,
    /// The title it belongs to: the one shown, or one merged into it.
    pub manga_id: i64,
    /// The volume its name gives, as "Vol.2 Ch. 10" gives 2.
    pub volume: Option<f64>,
    /// The title shows one upload per chapter, and prefers another upload of this one.
    pub duplicate: bool,
}

/// One of the titles a title is read from: itself, or one merged into it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub id: i64,
    pub title: String,
    pub source_name: Option<String>,
    pub source_lang: Option<String>,
}

/// A page kept with a note.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PageMark {
    pub chapter_id: i64,
    pub chapter_name: String,
    /// From 0, as the reader counts.
    pub page: i64,
    pub note: String,
    pub created_at: i64,
}

/// A library title that goes by the same name as another, from another source.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Namesake {
    pub id: i64,
    pub title: String,
    pub cover: Option<String>,
    pub source_name: Option<String>,
}

/// Detail fields changed by hand. Each one given is locked; an empty text clears it.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Edit {
    pub title: Option<String>,
    pub author: Option<String>,
    pub artist: Option<String>,
    pub description: Option<String>,
    pub genres: Option<Vec<String>>,
    pub status: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TitleView {
    pub id: i64,
    pub source_id: String,
    /// None when the extension that provides the source is not installed.
    pub source_name: Option<String>,
    pub source_lang: Option<String>,
    pub web_url: Option<String>,
    pub title: String,
    pub artist: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub genres: Vec<String>,
    pub status: i64,
    pub cover: Option<String>,
    pub in_library: bool,
    pub added_at: Option<i64>,
    pub initialized: bool,
    pub reader_mode: Option<String>,
    pub reader_fit: Option<String>,
    pub auto_download: bool,
    pub chapters_at: Option<i64>,
    /// How many days usually pass between two chapters, when the source's dates say.
    pub release_days: Option<i64>,
    /// The library categories it is in.
    pub categories: Vec<i64>,
    /// The detail fields edited by hand, which a refresh leaves alone.
    pub locked: Vec<String>,
    /// Scanlation groups in the order wanted. Groups not named come after them.
    pub scanlators: Vec<String>,
    /// Only the preferred upload of each chapter shows; the others are marked `duplicate`.
    pub dedupe: bool,
    /// The title this one was merged into, when it was.
    pub merged_into: Option<i64>,
    /// This title and the titles merged into it, in their order.
    pub members: Vec<Member>,
    /// In the source's order: the first is usually the newest. With titles merged in, by number.
    pub chapters: Vec<ChapterView>,
}

#[derive(Serialize)]
pub struct ChapterName {
    pub id: i64,
    pub name: String,
}

/// What the reader needs to open a chapter, all from the database.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderView {
    pub chapter: ChapterView,
    pub manga_id: i64,
    pub title: String,
    /// What sort of series it looks like: manga, manhwa, manhua or comic.
    pub kind: &'static str,
    pub reader_mode: Option<String>,
    pub reader_fit: Option<String>,
    /// The other reader settings the title keeps for itself, shaped by the UI.
    pub reader_prefs: Option<Value>,
    pub in_library: bool,
    /// The chapter read before this one, and the one read after it.
    pub prev: Option<i64>,
    pub next: Option<i64>,
    /// The chapters reading goes through, newest first: the uploads the title prefers.
    pub chapters: Vec<ChapterName>,
    /// The pages of this chapter kept with a note.
    pub marks: Vec<PageMark>,
}

pub enum Browse {
    Popular,
    Latest,
    Search,
}

impl Browse {
    pub fn parse(kind: &str) -> Option<Self> {
        match kind {
            "popular" => Some(Self::Popular),
            "latest" => Some(Self::Latest),
            "search" => Some(Self::Search),
            _ => None,
        }
    }

    fn path(&self) -> &'static str {
        match self {
            Self::Popular => "popular",
            Self::Latest => "latest",
            Self::Search => "search",
        }
    }
}

#[derive(Clone)]
pub struct Titles {
    db: Db,
    host: Arc<Host>,
    local: Local,
}

impl Titles {
    pub fn new(db: Db, host: Arc<Host>, local: Local) -> Self {
        Self { db, host, local }
    }

    /// One page of a source's popular, latest or search list. The titles are stored on the way.
    /// `filters` are the changes a search makes to the source's own list; only a search reads them.
    pub async fn browse(&self, source: i64, kind: Browse, page: i64, query: &str, filters: &[Value]) -> Result<Listing> {
        if source == local::SOURCE {
            let (local, latest, query) = (self.local.clone(), matches!(kind, Browse::Latest), query.to_owned());
            let (mangas, has_next_page) = local::blocking(move || local.browse(latest, &query, page)).await?;
            let items = self.db.call(move |conn| save_listing(conn, source, &mangas)).await?;
            return Ok(Listing { items, has_next_page });
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Found {
            mangas: Vec<WireManga>,
            #[serde(default)]
            has_next_page: bool,
        }
        let body = json!({ "page": page.max(1), "query": query, "filters": filters });
        let opts = Call { source: Some(source), ..Call::default() };
        let found: Found = self.host.call(Method::POST, &format!("/sources/{source}/{}", kind.path()), Some(&body), opts).await?;
        let has_next_page = found.has_next_page;
        let items = self.db.call(move |conn| save_listing(conn, source, &found.mangas)).await?;
        Ok(Listing { items, has_next_page })
    }

    /// What a source's search can be narrowed by, as the host lists it. The source makes
    /// the list anew each time, so nothing here is any account's own.
    pub async fn filters(&self, source: i64) -> Result<Value> {
        if source == local::SOURCE {
            return Ok(json!({ "filters": [] }));
        }
        let opts = Call { source: Some(source), ..Call::default() };
        self.host.call(Method::GET, &format!("/sources/{source}/filters"), None, opts).await
    }

    /// A title and its chapters, from the database.
    pub async fn get(&self, id: i64) -> Result<TitleView> {
        self.db.call(move |conn| read_title(conn, id)).await
    }

    /// Fetches details and chapters from the source, then answers like `get`.
    pub async fn refresh(&self, id: i64) -> Result<TitleView> {
        self.fetch(id, true, true).await?;
        self.get(id).await
    }

    /// Asks the source for a title's details, its chapters, or both, and stores the answer.
    /// Returns the chapters that are new since the last look.
    pub async fn fetch(&self, id: i64, details: bool, chapters: bool) -> Result<Vec<i64>> {
        #[derive(Deserialize)]
        struct Update {
            manga: WireManga,
            #[serde(default)]
            chapters: Vec<WireChapter>,
            /// The title's page on the source's site, as the source makes it.
            #[serde(default, rename = "webUrl")]
            web_url: Option<String>,
        }
        let (source, manga, known) = self
            .db
            .call(move |conn| {
                let (source, manga) = wire_manga(conn, id)?;
                Ok((source, manga, wire_chapters(conn, id)?))
            })
            .await?;
        if source == local::SOURCE {
            let local = self.local.clone();
            let (found, listed) = local::blocking(move || local.details(&manga.url)).await?;
            return self.db.call(move |conn| apply_update(conn, id, &found, &listed, details, chapters)).await;
        }
        let body = json!({ "manga": manga, "chapters": known, "fetchDetails": details, "fetchChapters": chapters });
        let opts = Call { source: Some(source), ..Call::default() };
        let update: Update = self.host.call(Method::POST, &format!("/sources/{source}/manga"), Some(&body), opts).await?;
        self.db
            .call(move |conn| {
                if let Some(own) = update.web_url.filter(|own| own.starts_with("https://") || own.starts_with("http://")) {
                    conn.execute("UPDATE manga SET web_url = ?1 WHERE id = ?2", params![own, id])?;
                }
                apply_update(conn, id, &update.manga, &update.chapters, details, chapters)
            })
            .await
    }

    pub async fn set_library(&self, id: i64, in_library: bool) -> Result<()> {
        self.change(move |conn| {
            if !in_library {
                conn.execute("DELETE FROM manga_categories WHERE manga_id = ?1", [id])?;
            }
            Ok(conn.execute(
                "UPDATE manga SET added_at = CASE WHEN NOT ?2 THEN NULL WHEN in_library THEN added_at ELSE ?3 END, in_library = ?2 WHERE id = ?1",
                params![id, in_library, now()],
            )?)
        })
        .await
    }

    /// Remembers how this title is read. An empty or missing value goes back to the default.
    pub async fn set_reader(&self, id: i64, mode: Option<String>, fit: Option<String>, prefs: Option<Value>) -> Result<()> {
        let mode = mode.filter(|m| !m.is_empty());
        let fit = fit.filter(|f| !f.is_empty());
        if mode.as_deref().is_some_and(|m| !READER_MODES.contains(&m)) || fit.as_deref().is_some_and(|f| !READER_FITS.contains(&f)) {
            return Err(AppError::bad_request("Unknown reader mode or fit"));
        }
        let prefs = stored_prefs(prefs)?;
        self.change(move |conn| {
            Ok(conn.execute("UPDATE manga SET reader_mode = ?2, reader_fit = ?3, reader_prefs = ?4 WHERE id = ?1", params![id, mode, fit, prefs])?)
        })
        .await
    }

    /// Whether new chapters of this title are downloaded as updates find them.
    pub async fn set_auto_download(&self, id: i64, on: bool) -> Result<()> {
        self.change(move |conn| Ok(conn.execute("UPDATE manga SET auto_download = ?2 WHERE id = ?1", params![id, on])?)).await
    }

    /// Bookmarks chapters of a title, or takes their bookmarks away. Without ids, all of them.
    /// The chapters of titles merged into it count as its own.
    pub async fn bookmark(&self, manga: i64, chapters: Option<Vec<i64>>, on: bool) -> Result<()> {
        self.db
            .call(move |conn| {
                let Some(only) = among(&chapters) else { return Ok(()) };
                let list = id_list(&members(conn, manga)?);
                conn.execute(&format!("UPDATE chapters SET bookmark = ?1 WHERE manga_id IN ({list}){only}"), params![on])?;
                Ok(())
            })
            .await
    }

    /// Marks chapters of a title read or unread. Without ids, all of them.
    pub async fn mark(&self, manga: i64, chapters: Option<Vec<i64>>, read: bool) -> Result<()> {
        self.db
            .call(move |conn| {
                let Some(only) = among(&chapters) else { return Ok(()) };
                let list = id_list(&members(conn, manga)?);
                conn.execute(
                    &format!(
                        "UPDATE chapters SET read = ?1, last_page_read = 0, read_at = CASE WHEN ?1 THEN ?2 ELSE read_at END
                         WHERE manga_id IN ({list}){only}"
                    ),
                    params![read, now()],
                )?;
                Ok(())
            })
            .await
    }

    /// Changes detail fields by hand, and locks them against refreshes from the source.
    pub async fn edit(&self, id: i64, edit: Edit) -> Result<()> {
        self.change(move |conn| edit_details(conn, id, &edit)).await
    }

    /// Lets every detail field follow the source again. The next refresh fills them in.
    pub async fn unlock(&self, id: i64) -> Result<()> {
        self.change(move |conn| Ok(conn.execute("UPDATE manga SET locked = NULL WHERE id = ?1", [id])?)).await
    }

    /// Shows a cover Mango keeps, named by `mark`, in place of the source's, and locks it.
    pub async fn set_cover(&self, id: i64, mark: String) -> Result<()> {
        self.change(move |conn| {
            let locked = with_locks(conn, id, &["cover"])?;
            Ok(conn.execute("UPDATE manga SET thumbnail_url = ?2, locked = ?3 WHERE id = ?1", params![id, format!("{CUSTOM_COVER}{mark}"), locked])?)
        })
        .await
    }

    /// The order a title's scanlation groups are wanted in, and whether only the preferred
    /// upload of each chapter shows.
    pub async fn set_scanlators(&self, id: i64, order: Vec<String>, dedupe: bool) -> Result<()> {
        let mut groups: Vec<String> = Vec::new();
        for group in order.iter().map(|group| group.trim()).filter(|group| !group.is_empty()) {
            if group.chars().count() > 200 {
                return Err(AppError::bad_request("A group's name is too long"));
            }
            if !groups.iter().any(|known| known == group) {
                groups.push(group.to_owned());
            }
        }
        if groups.len() > MAX_GROUPS {
            return Err(AppError::bad_request(format!("A title keeps an order for {MAX_GROUPS} groups at most")));
        }
        let kept = (!groups.is_empty()).then(|| json!(groups).to_string());
        self.change(move |conn| Ok(conn.execute("UPDATE manga SET scanlators = ?2, dedupe = ?3 WHERE id = ?1", params![id, kept, dedupe])?))
            .await
    }

    /// Merges `other` into `head`: its chapters show, update and are read under `head`.
    pub async fn merge(&self, head: i64, other: i64) -> Result<()> {
        self.db.call(move |conn| merge(conn, head, other)).await
    }

    /// Takes a merged title out of `head` again. It is not in the library on its own after.
    pub async fn unmerge(&self, head: i64, other: i64) -> Result<()> {
        self.change(move |conn| {
            Ok(conn.execute(
                "UPDATE manga SET merged_into = NULL, merged_at = NULL, added_at = NULL WHERE id = ?2 AND merged_into = ?1",
                [head, other],
            )?)
        })
        .await
    }

    /// Library titles that go by the same name as this one: most likely the same series.
    pub async fn namesakes(&self, id: i64) -> Result<Vec<Namesake>> {
        self.db.call(move |conn| namesakes(conn, id)).await
    }

    /// Keeps a page with a note, or with None lets go of it.
    pub async fn mark_page(&self, chapter: i64, page: i64, note: Option<String>) -> Result<()> {
        if page < 0 {
            return Err(AppError::not_found("No such page"));
        }
        let note = note.map(|note| note.trim().to_owned());
        if note.as_ref().is_some_and(|note| note.chars().count() > 2000) {
            return Err(AppError::bad_request("A note has 2000 characters at most"));
        }
        self.db
            .call(move |conn| {
                let known = conn.query_row("SELECT 1 FROM chapters WHERE id = ?1", [chapter], |_| Ok(())).optional()?;
                known.ok_or_else(|| AppError::new(404, "unknown_chapter", "No such chapter"))?;
                match note {
                    Some(note) => conn.execute(
                        "INSERT INTO page_marks (chapter_id, page, note, created_at) VALUES (?1, ?2, ?3, ?4)
                         ON CONFLICT(chapter_id, page) DO UPDATE SET note = excluded.note",
                        params![chapter, page, note, now()],
                    )?,
                    None => conn.execute("DELETE FROM page_marks WHERE chapter_id = ?1 AND page = ?2", [chapter, page])?,
                };
                Ok(())
            })
            .await
    }

    /// The pages kept in a title's chapters, the latest kept first.
    pub async fn page_marks(&self, manga: i64) -> Result<Vec<PageMark>> {
        self.db
            .call(move |conn| {
                let list = id_list(&members(conn, manga)?);
                page_marks(conn, &format!("c.manga_id IN ({list})"))
            })
            .await
    }

    pub async fn reader(&self, chapter: i64) -> Result<ReaderView> {
        self.db.call(move |conn| read_reader(conn, chapter)).await
    }

    /// Saves where the reader is. `read` set marks the chapter finished or not; left out, it keeps its state.
    pub async fn progress(&self, chapter: i64, page: i64, read: Option<bool>) -> Result<()> {
        self.change(move |conn| save_place(conn, chapter, page, read)).await
    }

    /// A page a reading app fetched over OPDS-PSE, which has no other way to tell what was
    /// read. The chapter's place moves on when the page is further on, and its last page
    /// marks it read. A chapter read already is left as it is, as is everything when the
    /// option is off. Answers whether this finished the chapter.
    pub async fn streamed(&self, chapter: i64, page: i64) -> Result<bool> {
        self.db.call(move |conn| streamed_page(conn, chapter, page)).await
    }

    /// Runs an update that must hit exactly one row.
    async fn change<F>(&self, update: F) -> Result<()>
    where
        F: FnOnce(&mut Connection) -> Result<usize> + Send + 'static,
    {
        match self.db.call(update).await? {
            0 => Err(AppError::not_found("No such title or chapter")),
            _ => Ok(()),
        }
    }
}

// ---------- helpers ----------

/// Address of a title's cover. It changes with the source's image, so it can be cached for good.
pub fn cover_url(id: i64, thumbnail: Option<&str>) -> Option<String> {
    thumbnail.filter(|t| !t.is_empty()).map(|t| format!("api/manga/{id}/cover?v={:08x}", fnv(t)))
}

pub fn fnv(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5, |hash, byte| (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193))
}

fn no_title() -> AppError {
    AppError::new(404, "unknown_title", "No such title")
}

fn json_column(r: &Row, index: usize) -> rusqlite::Result<Value> {
    let text: String = r.get(index)?;
    Ok(serde_json::from_str(&text).unwrap_or_else(|_| json!({})))
}

/// A title's page on the source's site: as the source made it, or else put together from
/// the title's key, when that is an address or a path.
fn web_url(own: Option<String>, base: Option<String>, url: &str) -> Option<String> {
    let web = |address: &str| address.starts_with("http://") || address.starts_with("https://");
    if let Some(own) = own.filter(|own| web(own)) {
        return Some(own);
    }
    if web(url) {
        return Some(url.to_owned());
    }
    let base = base.filter(|b| web(b))?;
    url.starts_with('/').then(|| format!("{}{url}", base.trim_end_matches('/')))
}

/// A chapter number read from its name, for sources that give none: "Vol.2 Ch. 10.5 - Rain" is 10.5.
fn number_from_name(name: &str) -> f64 {
    fn leading(text: &str) -> Option<f64> {
        let end = text.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(text.len());
        text[..end].trim_end_matches('.').parse().ok()
    }
    let lower = name.to_lowercase();
    for mark in ["chapter", "ch.", "ch ", "episode", "ep.", "ep ", "#"] {
        if let Some(number) = lower.find(mark).and_then(|at| leading(lower[at + mark.len()..].trim_start())) {
            return number;
        }
    }
    // Otherwise the first number that is not a volume or a season.
    let mut rest = lower.as_str();
    while let Some(at) = rest.find(|c: char| c.is_ascii_digit()) {
        let before = rest[..at].trim_end();
        let volume = ["vol.", "vol", "volume", "season"].iter().any(|word| before.ends_with(word));
        let end = rest[at..].find(|c: char| !(c.is_ascii_digit() || c == '.')).map_or(rest.len(), |len| at + len);
        match leading(&rest[at..]) {
            Some(number) if !volume => return number,
            _ => rest = &rest[end..],
        }
    }
    -1.0
}

/// The volume a chapter's name gives: "Vol.2 Ch. 10" is in volume 2.
fn volume_from_name(name: &str) -> Option<f64> {
    let lower = name.to_lowercase();
    ["volume", "vol.", "vol "].iter().find_map(|mark| {
        let rest = lower[lower.find(mark)? + mark.len()..].trim_start();
        let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len());
        rest[..end].trim_end_matches('.').parse().ok()
    })
}

/// What keeps a statement to the chapters named: nothing for all of them, None when none is named.
fn among(chapters: &Option<Vec<i64>>) -> Option<String> {
    match chapters {
        Some(ids) if ids.is_empty() => None,
        Some(ids) => Some(format!(" AND id IN ({})", id_list(ids))),
        None => Some(String::new()),
    }
}

/// Numbers only, so the list can go into a statement as it is.
pub(crate) fn id_list(ids: &[i64]) -> String {
    ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
}

/// The title a title shows under: the one it was merged into, or itself.
pub(crate) fn head_of(conn: &Connection, id: i64) -> Result<i64> {
    conn.query_row("SELECT COALESCE(merged_into, id) FROM manga WHERE id = ?1", [id], |r| r.get(0)).optional()?.ok_or_else(no_title)
}

/// A title, then the titles merged into it in the order they came.
pub(crate) fn members(conn: &Connection, head: i64) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare_cached("SELECT id FROM manga WHERE merged_into = ?1 ORDER BY merged_at, id")?;
    let mut ids = vec![head];
    for id in stmt.query_map([head], |r| r.get(0))? {
        ids.push(id?);
    }
    Ok(ids)
}

/// A JSON list of names, as `locked` and `scanlators` keep them.
fn names(text: Option<String>) -> Vec<String> {
    text.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

/// A title's locked fields with `more` added, in the order `LOCKABLE` gives, as stored.
fn with_locks(conn: &Connection, id: i64, more: &[&str]) -> Result<String> {
    let held = names(conn.query_row("SELECT locked FROM manga WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten());
    let locked: Vec<&str> = LOCKABLE.into_iter().filter(|field| more.contains(field) || held.iter().any(|kept| kept == field)).collect();
    Ok(json!(locked).to_string())
}

/// Writes the fields an edit gives. Answers how many titles changed: 0 for none by that id.
fn edit_details(conn: &mut Connection, id: i64, edit: &Edit) -> Result<usize> {
    fn text(value: &str, most: usize, what: &str) -> Result<Option<String>> {
        let value = value.trim();
        if value.chars().count() > most {
            return Err(AppError::bad_request(format!("The {what} is too long")));
        }
        Ok((!value.is_empty()).then(|| value.to_owned()))
    }
    // (the field as it is locked, its column, the value)
    let mut changes: Vec<(&str, &str, Sql)> = Vec::new();
    if let Some(title) = &edit.title {
        let title = text(title, 300, "name")?.ok_or_else(|| AppError::bad_request("A title needs a name"))?;
        changes.push(("title", "title", title.into()));
    }
    for (field, value) in [("author", &edit.author), ("artist", &edit.artist)] {
        if let Some(value) = value {
            changes.push((field, field, text(value, 300, field)?.into()));
        }
    }
    if let Some(description) = &edit.description {
        changes.push(("description", "description", text(description, 20_000, "description")?.into()));
    }
    if let Some(genres) = &edit.genres {
        let genres: Vec<&str> = genres.iter().map(|genre| genre.trim()).filter(|genre| !genre.is_empty()).collect();
        if genres.len() > 60 || genres.iter().any(|genre| genre.chars().count() > 60 || genre.contains(',')) {
            return Err(AppError::bad_request("Up to 60 genres, each of up to 60 characters and without commas"));
        }
        changes.push(("genre", "genre", (!genres.is_empty()).then(|| genres.join(", ")).into()));
    }
    if let Some(status) = edit.status {
        if !(0..=6).contains(&status) {
            return Err(AppError::bad_request("Not a status"));
        }
        changes.push(("status", "status", status.into()));
    }
    if changes.is_empty() {
        return Err(AppError::bad_request("Say what to change"));
    }
    let tx = conn.transaction()?;
    if tx.query_row("SELECT 1 FROM manga WHERE id = ?1", [id], |_| Ok(())).optional()?.is_none() {
        return Ok(0);
    }
    for (_, column, value) in &changes {
        tx.execute(&format!("UPDATE manga SET {column} = ?2 WHERE id = ?1"), params![id, value])?;
    }
    let locked = with_locks(&tx, id, &changes.iter().map(|(field, ..)| *field).collect::<Vec<_>>())?;
    tx.execute("UPDATE manga SET locked = ?2 WHERE id = ?1", params![id, locked])?;
    tx.commit()?;
    Ok(1)
}

/// Every upload of a chapter but one is marked a duplicate. The one kept is from the
/// earliest of `members`, then from the group earliest in `groups`, then listed first.
fn prefer(chapters: &mut [ChapterView], members: &[i64], groups: &[String]) {
    let rank = |chapter: &ChapterView| {
        let source = members.iter().position(|id| *id == chapter.manga_id).unwrap_or(usize::MAX);
        let group = chapter.scanlator.as_deref().and_then(|name| groups.iter().position(|wanted| wanted == name)).unwrap_or(groups.len());
        (source, group)
    };
    let mut best: HashMap<Key, usize> = HashMap::new();
    for (at, chapter) in chapters.iter().enumerate() {
        let one = key(chapter.number, &chapter.name);
        if best.get(&one).is_none_or(|&kept| rank(chapter) < rank(&chapters[kept])) {
            best.insert(one, at);
        }
    }
    for (at, chapter) in chapters.iter_mut().enumerate() {
        chapter.duplicate = best.get(&key(chapter.number, &chapter.name)) != Some(&at);
    }
}

/// Merges `other` into `head`, with the titles merged into `other` before.
fn merge(conn: &mut Connection, head: i64, other: i64) -> Result<()> {
    let head = head_of(conn, head)?;
    head_of(conn, other)?;
    if head == other {
        return Err(AppError::bad_request("A title cannot be merged into itself"));
    }
    let tx = conn.transaction()?;
    let now = now();
    tx.execute("UPDATE manga SET merged_into = ?1 WHERE merged_into = ?2", [head, other])?;
    // It leaves the library as a title of its own. What it had found before is not news under `head`.
    tx.execute("UPDATE manga SET merged_into = ?1, merged_at = ?2, in_library = 0, added_at = ?2 WHERE id = ?3", params![head, now, other])?;
    tx.execute("DELETE FROM manga_categories WHERE manga_id = ?1", [other])?;
    tx.commit()?;
    Ok(())
}

/// A name with only its letters and digits, in lower case: "Solo Leveling!" and "solo leveling" are one.
fn plain_name(name: &str) -> String {
    name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn namesakes(conn: &Connection, id: i64) -> Result<Vec<Namesake>> {
    let name: String = conn.query_row("SELECT title FROM manga WHERE id = ?1", [id], |r| r.get(0)).optional()?.ok_or_else(no_title)?;
    let wanted = plain_name(&name);
    let mut stmt = conn.prepare(
        "SELECT m.id, m.title, m.thumbnail_url, s.name FROM manga m LEFT JOIN sources s ON s.id = m.source_id
         WHERE m.in_library = 1 AND m.id != ?1 ORDER BY m.title COLLATE NOCASE",
    )?;
    let found = stmt
        .query_map([id], |r| {
            let (id, thumbnail): (i64, Option<String>) = (r.get(0)?, r.get(2)?);
            Ok(Namesake { id, title: r.get(1)?, cover: cover_url(id, thumbnail.as_deref()), source_name: r.get(3)? })
        })?
        .filter(|found| found.as_ref().map_or(true, |found| plain_name(&found.title) == wanted))
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(found)
}

/// Kept pages, the latest kept first, of the chapters `which` picks out by `c`.
fn page_marks(conn: &Connection, which: &str) -> Result<Vec<PageMark>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT p.chapter_id, c.name, p.page, p.note, p.created_at FROM page_marks p JOIN chapters c ON c.id = p.chapter_id
         WHERE {which} ORDER BY p.created_at DESC, p.page"
    ))?;
    let marks = stmt
        .query_map([], |r| Ok(PageMark { chapter_id: r.get(0)?, chapter_name: r.get(1)?, page: r.get(2)?, note: r.get(3)?, created_at: r.get(4)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(marks)
}

/// Marks the other uploads of a chapter read along with it: the same number, from any of
/// the title's groups or the titles merged into it.
/// Keeps where a chapter is read to, and with `read` whether it is finished. Answers how
/// many chapters that was: none when there is no such chapter.
fn save_place(conn: &Connection, chapter: i64, page: i64, read: Option<bool>) -> Result<usize> {
    let changed = conn.execute(
        "UPDATE chapters SET last_page_read = ?2, read = COALESCE(?3, read), read_at = ?4 WHERE id = ?1",
        params![chapter, page.max(0), read, now()],
    )?;
    crate::library::remember(conn, chapter)?;
    if read == Some(true) && settings::get::<bool>(conn, MARK_DUPLICATES).unwrap_or(false) {
        read_duplicates(conn, chapter)?;
    }
    Ok(changed)
}

fn streamed_page(conn: &Connection, chapter: i64, page: i64) -> Result<bool> {
    if !settings::get::<bool>(conn, OPDS_PROGRESS).unwrap_or(true) {
        return Ok(false);
    }
    let row = conn
        .query_row("SELECT read, last_page_read, page_count FROM chapters WHERE id = ?1", [chapter], |r| {
            Ok((r.get::<_, bool>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<i64>>(2)?))
        })
        .optional()?;
    let Some((false, kept, count)) = row else { return Ok(false) };
    let finished = count.is_some_and(|count| count > 0 && page + 1 >= count);
    if !finished && page <= kept {
        return Ok(false);
    }
    save_place(conn, chapter, page, finished.then_some(true))?;
    Ok(finished)
}

fn read_duplicates(conn: &Connection, chapter: i64) -> Result<()> {
    let found = conn.query_row("SELECT manga_id, chapter_number FROM chapters WHERE id = ?1", [chapter], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?)));
    let Some((owner, number)) = found.optional()? else { return Ok(()) };
    if number < 0.0 {
        return Ok(());
    }
    let list = id_list(&members(conn, head_of(conn, owner)?)?);
    conn.execute(
        &format!(
            "UPDATE chapters SET read = 1, last_page_read = 0, read_at = ?2
             WHERE manga_id IN ({list}) AND id != ?1 AND read = 0 AND ROUND(chapter_number * 100) = ROUND(?3 * 100)"
        ),
        params![chapter, now(), number],
    )?;
    Ok(())
}

/// How a title gets its chapters, read from the days they came out on, counted from the
/// epoch: the usual number of days between two, and the day of the latest. The middle one
/// of the last few gaps, so one long break or one double release does not sway it.
/// None when fewer than three days are known: too little to tell.
pub(crate) fn rhythm(mut days: Vec<i64>) -> Option<(i64, i64)> {
    days.sort_unstable_by(|a, b| b.cmp(a));
    days.dedup();
    days.truncate(10);
    if days.len() < 3 {
        return None;
    }
    let mut gaps: Vec<i64> = days.windows(2).map(|pair| pair[0] - pair[1]).collect();
    gaps.sort_unstable();
    Some((gaps[(gaps.len() - 1) / 2].clamp(1, MAX_RELEASE_DAYS), days[0]))
}

fn chapter_row(r: &Row) -> rusqlite::Result<ChapterView> {
    let name: String = r.get(1)?;
    Ok(ChapterView {
        id: r.get(0)?,
        volume: volume_from_name(&name),
        name,
        number: r.get(2)?,
        scanlator: r.get(3)?,
        date_upload: r.get(4)?,
        fetched_at: r.get(5)?,
        read: r.get(6)?,
        last_page_read: r.get(7)?,
        page_count: r.get(8)?,
        downloaded: r.get(9)?,
        bookmark: r.get(10)?,
        manga_id: r.get(11)?,
        duplicate: false,
    })
}

/// Stores one page of browse results and answers with their cards.
/// A title already opened keeps its own name and state; a new one is created.
fn save_listing(conn: &mut Connection, source: i64, mangas: &[WireManga]) -> Result<Vec<Card>> {
    let tx = conn.transaction()?;
    let mut cards: Vec<Card> = Vec::with_capacity(mangas.len());
    {
        let mut upsert = tx.prepare(
            "INSERT INTO manga (source_id, url, title, artist, author, description, genre, status, thumbnail_url, update_strategy, memo)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(source_id, url) DO UPDATE SET
                 title = CASE WHEN manga.initialized OR excluded.title = '' OR manga.locked LIKE '%\"title\"%' THEN manga.title ELSE excluded.title END,
                 thumbnail_url = CASE WHEN manga.locked LIKE '%\"cover\"%' THEN manga.thumbnail_url
                                      ELSE COALESCE(excluded.thumbnail_url, manga.thumbnail_url) END,
                 memo = CASE WHEN manga.initialized THEN manga.memo ELSE excluded.memo END
             RETURNING id, title, thumbnail_url, in_library",
        )?;
        for manga in mangas.iter().filter(|m| !m.url.is_empty()) {
            let card = upsert.query_row(
                params![
                    source,
                    manga.url,
                    manga.title,
                    manga.artist,
                    manga.author,
                    manga.description,
                    manga.genre,
                    manga.status,
                    manga.thumbnail_url.as_deref().filter(|t| !t.is_empty()),
                    manga.update_strategy,
                    manga.memo.to_string()
                ],
                |r| {
                    let id: i64 = r.get(0)?;
                    let thumbnail: Option<String> = r.get(2)?;
                    Ok(Card { id, source_id: source.to_string(), title: r.get(1)?, cover: cover_url(id, thumbnail.as_deref()), in_library: r.get(3)? })
                },
            )?;
            if !cards.iter().any(|seen| seen.id == card.id) {
                cards.push(card);
            }
        }
    }
    tx.commit()?;
    Ok(cards)
}

fn wire_manga(conn: &Connection, id: i64) -> Result<(i64, WireManga)> {
    conn.query_row(
        "SELECT source_id, url, title, artist, author, description, genre, status, thumbnail_url, update_strategy, initialized, memo
         FROM manga WHERE id = ?1",
        [id],
        |r| {
            Ok((
                r.get(0)?,
                WireManga {
                    url: r.get(1)?,
                    title: r.get(2)?,
                    artist: r.get(3)?,
                    author: r.get(4)?,
                    description: r.get(5)?,
                    genre: r.get(6)?,
                    status: r.get(7)?,
                    thumbnail_url: r.get(8)?,
                    update_strategy: r.get(9)?,
                    initialized: r.get(10)?,
                    memo: json_column(r, 11)?,
                },
            ))
        },
    )
    .optional()?
    .ok_or_else(no_title)
}

fn wire_chapters(conn: &Connection, manga: i64) -> Result<Vec<WireChapter>> {
    let mut stmt = conn
        .prepare("SELECT url, name, date_upload, chapter_number, scanlator, memo FROM chapters WHERE manga_id = ?1 ORDER BY source_order")?;
    let chapters = stmt
        .query_map([manga], |r| {
            Ok(WireChapter {
                url: r.get(0)?,
                name: r.get(1)?,
                date_upload: r.get(2)?,
                chapter_number: r.get(3)?,
                scanlator: r.get(4)?,
                memo: json_column(r, 5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(chapters)
}

/// A chapter as the source knows it, with the source to ask. Used to fetch its pages.
pub(crate) fn wire_chapter(conn: &Connection, chapter: i64) -> Result<(i64, WireChapter)> {
    conn.query_row(
        "SELECT m.source_id, c.url, c.name, c.date_upload, c.chapter_number, c.scanlator, c.memo
         FROM chapters c JOIN manga m ON m.id = c.manga_id WHERE c.id = ?1",
        [chapter],
        |r| {
            Ok((
                r.get(0)?,
                WireChapter {
                    url: r.get(1)?,
                    name: r.get(2)?,
                    date_upload: r.get(3)?,
                    chapter_number: r.get(4)?,
                    scanlator: r.get(5)?,
                    memo: json_column(r, 6)?,
                },
            ))
        },
    )
    .optional()?
    .ok_or_else(|| AppError::new(404, "unknown_chapter", "No such chapter"))
}

/// The chapters of these titles. One title's go in its source's order; several titles'
/// go by number, newest first, and a chapter of an earlier title before the same of a later one.
fn read_chapters(conn: &Connection, titles: &[i64]) -> Result<Vec<ChapterView>> {
    let mut stmt = conn.prepare(&format!("SELECT {CHAPTER_COLUMNS} FROM chapters WHERE manga_id IN ({}) ORDER BY source_order", id_list(titles)))?;
    let mut chapters = stmt.query_map([], chapter_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    if titles.len() > 1 {
        let rank = |chapter: &ChapterView| titles.iter().position(|id| *id == chapter.manga_id);
        chapters.sort_by(|a, b| b.number.total_cmp(&a.number).then_with(|| rank(a).cmp(&rank(b))));
    }
    Ok(chapters)
}

/// Every chapter a title is read from, its own and those of the titles merged into it, with
/// the uploads it does not prefer marked when it shows one upload per chapter.
pub(crate) fn listed_chapters(conn: &Connection, head: i64) -> Result<Vec<ChapterView>> {
    let titles = members(conn, head)?;
    let (order, dedupe): (Option<String>, bool) = conn.query_row("SELECT scanlators, dedupe FROM manga WHERE id = ?1", [head], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut chapters = read_chapters(conn, &titles)?;
    if dedupe {
        prefer(&mut chapters, &titles, &names(order));
    }
    Ok(chapters)
}

pub(crate) fn read_title(conn: &Connection, id: i64) -> Result<TitleView> {
    let mut title = conn
        .query_row(
            "SELECT m.id, m.source_id, s.name, s.lang, s.base_url, m.url, m.title, m.artist, m.author, m.description, m.genre,
                    m.status, m.thumbnail_url, m.in_library, m.added_at, m.initialized, m.reader_mode, m.reader_fit,
                    m.auto_download, m.chapters_at, m.locked, m.scanlators, m.dedupe, m.merged_into, m.web_url
             FROM manga m LEFT JOIN sources s ON s.id = m.source_id WHERE m.id = ?1",
            [id],
            |r| {
                let url: String = r.get(5)?;
                let genre: Option<String> = r.get(10)?;
                let thumbnail: Option<String> = r.get(12)?;
                Ok(TitleView {
                    id,
                    source_id: r.get::<_, i64>(1)?.to_string(),
                    source_name: r.get(2)?,
                    source_lang: r.get(3)?,
                    web_url: web_url(r.get(24)?, r.get(4)?, &url),
                    title: r.get(6)?,
                    artist: r.get(7)?,
                    author: r.get(8)?,
                    description: r.get(9)?,
                    genres: genre.iter().flat_map(|g| g.split(',')).map(str::trim).filter(|g| !g.is_empty()).map(str::to_owned).collect(),
                    status: r.get(11)?,
                    cover: cover_url(id, thumbnail.as_deref()),
                    in_library: r.get(13)?,
                    added_at: r.get(14)?,
                    initialized: r.get(15)?,
                    reader_mode: r.get(16)?,
                    reader_fit: r.get(17)?,
                    auto_download: r.get(18)?,
                    chapters_at: r.get(19)?,
                    release_days: None,
                    categories: Vec::new(),
                    locked: names(r.get(20)?),
                    scanlators: names(r.get(21)?),
                    dedupe: r.get(22)?,
                    merged_into: r.get(23)?,
                    members: Vec::new(),
                    chapters: Vec::new(),
                })
            },
        )
        .optional()?
        .ok_or_else(no_title)?;
    title.categories = {
        let mut stmt = conn.prepare("SELECT category_id FROM manga_categories WHERE manga_id = ?1")?;
        let ids = stmt.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<Vec<i64>>>()?;
        ids
    };
    title.members = {
        let mut stmt = conn.prepare_cached("SELECT m.title, s.name, s.lang FROM manga m LEFT JOIN sources s ON s.id = m.source_id WHERE m.id = ?1")?;
        let mut found = Vec::new();
        for member in members(conn, id)? {
            found.push(stmt.query_row([member], |r| Ok(Member { id: member, title: r.get(0)?, source_name: r.get(1)?, source_lang: r.get(2)? }))?);
        }
        found
    };
    title.chapters = listed_chapters(conn, id)?;
    let days = title.chapters.iter().filter(|c| c.date_upload > 0).map(|c| c.date_upload / DAY_MS).collect();
    title.release_days = rhythm(days).map(|(every, _)| every);
    Ok(title)
}

/// What sort of series a title is. The reader lays each sort out its own way until the
/// title is told otherwise. Tags speak first, then the source's name, then its language.
fn kind_of(genre: Option<&str>, source: Option<&str>, lang: Option<&str>) -> &'static str {
    let tags: Vec<String> = genre.iter().flat_map(|g| g.split(',')).map(|tag| tag.trim().to_lowercase()).collect();
    let tagged = |words: &[&str]| tags.iter().any(|tag| words.contains(&tag.as_str()));
    let source = source.unwrap_or_default().to_lowercase();
    if tagged(&["manhwa", "webtoon", "webtoons", "long strip", "korean"]) {
        "manhwa"
    } else if tagged(&["manhua", "chinese"]) {
        "manhua"
    } else if tagged(&["comic", "comics", "western"]) {
        "comic"
    } else if tagged(&["manga", "japanese"]) {
        "manga"
    } else if source.contains("manhwa") || source.contains("toon") {
        "manhwa"
    } else if source.contains("manhua") {
        "manhua"
    } else {
        match lang.unwrap_or_default() {
            "ko" => "manhwa",
            lang if lang.starts_with("zh") => "manhua",
            _ => "manga",
        }
    }
}

/// A title's own reader settings as they are stored: an object, or nothing when it is empty.
fn stored_prefs(prefs: Option<Value>) -> Result<Option<String>> {
    match prefs {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(map)) if map.is_empty() => Ok(None),
        Some(value @ Value::Object(_)) => {
            let text = value.to_string();
            if text.len() > MAX_READER_PREFS {
                return Err(AppError::bad_request("Too many reader settings"));
            }
            Ok(Some(text))
        }
        Some(_) => Err(AppError::bad_request("Reader settings must be an object")),
    }
}

/// A chapter as the reader opens it. Everything about how it is read comes from the title
/// it shows under, which for a chapter of a merged title is the one it was merged into.
fn read_reader(conn: &Connection, chapter: i64) -> Result<ReaderView> {
    let unknown = || AppError::new(404, "unknown_chapter", "No such chapter");
    let owner: i64 = conn.query_row("SELECT manga_id FROM chapters WHERE id = ?1", [chapter], |r| r.get(0)).optional()?.ok_or_else(unknown)?;
    let manga_id = head_of(conn, owner)?;
    let (title, reader_mode, reader_fit, prefs, kind, in_library) = conn.query_row(
        "SELECT m.title, m.reader_mode, m.reader_fit, m.reader_prefs, m.genre, s.name, s.lang, m.in_library
         FROM manga m LEFT JOIN sources s ON s.id = m.source_id WHERE m.id = ?1",
        [manga_id],
        |r| {
            let (genre, source, lang): (Option<String>, Option<String>, Option<String>) = (r.get(4)?, r.get(5)?, r.get(6)?);
            let kind = kind_of(genre.as_deref(), source.as_deref(), lang.as_deref());
            Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get::<_, Option<String>>(3)?, kind, r.get::<_, bool>(7)?))
        },
    )?;
    let reader_prefs = prefs.and_then(|text| serde_json::from_str(&text).ok());

    let listed = listed_chapters(conn, manga_id)?;
    let view = listed.iter().find(|c| c.id == chapter).cloned().ok_or_else(unknown)?;
    // Reading passes over the uploads the title does not prefer, unless one of them is open.
    let chapters: Vec<ChapterName> =
        listed.into_iter().filter(|c| !c.duplicate || c.id == chapter).map(|c| ChapterName { id: c.id, name: c.name }).collect();
    // Sources list the newest first, so the next chapter to read is the one listed just above.
    let at = chapters.iter().position(|c| c.id == chapter).unwrap_or(0);
    let next = at.checked_sub(1).map(|i| chapters[i].id);
    let prev = chapters.get(at + 1).map(|c| c.id);
    let marks = page_marks(conn, &format!("p.chapter_id = {chapter}"))?;
    Ok(ReaderView { chapter: view, manga_id, title, kind, reader_mode, reader_fit, reader_prefs, in_library, prev, next, chapters, marks })
}

/// Stores what a source answered about a title. Returns the ids of chapters that are new,
/// which is none on the first fetch: a title's whole back catalogue is not news.
pub(crate) fn apply_update(
    conn: &mut Connection,
    id: i64,
    manga: &WireManga,
    chapters: &[WireChapter],
    with_details: bool,
    with_chapters: bool,
) -> Result<Vec<i64>> {
    let tx = conn.transaction()?;
    let now = now();
    let mut added = Vec::new();
    if with_details {
        // What was edited by hand stays as it was edited.
        let locked = names(tx.query_row("SELECT locked FROM manga WHERE id = ?1", [id], |r| r.get(0)).optional()?.flatten());
        let kept = |field: &str| locked.iter().any(|held| held == field);
        tx.execute(
            "UPDATE manga SET title = CASE WHEN ?2 = '' OR ?12 THEN title ELSE ?2 END, artist = CASE WHEN ?13 THEN artist ELSE ?3 END,
                 author = CASE WHEN ?14 THEN author ELSE ?4 END, description = CASE WHEN ?15 THEN description ELSE ?5 END,
                 genre = CASE WHEN ?16 THEN genre ELSE ?6 END, status = CASE WHEN ?17 THEN status ELSE ?7 END,
                 thumbnail_url = CASE WHEN ?18 THEN thumbnail_url ELSE COALESCE(?8, thumbnail_url) END, update_strategy = ?9,
                 initialized = 1, memo = ?10, details_at = ?11
             WHERE id = ?1",
            params![
                id,
                manga.title,
                manga.artist,
                manga.author,
                manga.description,
                manga.genre,
                manga.status,
                manga.thumbnail_url.as_deref().filter(|t| !t.is_empty()),
                manga.update_strategy,
                manga.memo.to_string(),
                now,
                kept("title"),
                kept("artist"),
                kept("author"),
                kept("description"),
                kept("genre"),
                kept("status"),
                kept("cover")
            ],
        )?;
    }
    if with_chapters {
        let mut known: HashMap<String, i64> = {
            let mut stmt = tx.prepare("SELECT url, id FROM chapters WHERE manga_id = ?1")?;
            let known = stmt.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
            known
        };
        let first_fetch = known.is_empty();
        // An empty answer for a title that had chapters is a source hiccup, not a deletion.
        if !chapters.is_empty() {
            for (order, chapter) in chapters.iter().enumerate().filter(|(_, c)| !c.url.is_empty()) {
                let number = if chapter.chapter_number >= 0.0 { chapter.chapter_number } else { number_from_name(&chapter.name) };
                let memo = chapter.memo.to_string();
                match known.remove(&chapter.url) {
                    Some(existing) => {
                        tx.execute(
                            "UPDATE chapters SET name = ?2, date_upload = CASE WHEN ?3 > 0 THEN ?3 ELSE date_upload END,
                                 chapter_number = ?4, scanlator = ?5, memo = ?6, source_order = ?7
                             WHERE id = ?1",
                            params![existing, chapter.name, chapter.date_upload, number, chapter.scanlator, memo, order as i64],
                        )?;
                    }
                    None => {
                        // On the first fetch a chapter counts as seen when it was published, not today.
                        let seen = match (first_fetch, chapter.date_upload > 0) {
                            (false, _) => now,
                            (true, true) => (chapter.date_upload / 1000).min(now),
                            (true, false) => 0,
                        };
                        let inserted = tx.execute(
                            "INSERT OR IGNORE INTO chapters (manga_id, url, name, date_upload, chapter_number, scanlator, memo, source_order, fetched_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                            params![id, chapter.url, chapter.name, chapter.date_upload, number, chapter.scanlator, memo, order as i64, seen],
                        )?;
                        if inserted == 1 && !first_fetch {
                            added.push(tx.last_insert_rowid());
                        }
                    }
                }
            }
            // What the source no longer lists goes, unless it is on disk.
            for gone in known.values() {
                tx.execute("DELETE FROM chapters WHERE id = ?1 AND download_path IS NULL", [gone])?;
            }
        }
        tx.execute("UPDATE manga SET chapters_at = ?2 WHERE id = ?1", params![id, now])?;
    }
    tx.commit()?;
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&mut conn).unwrap();
        conn
    }

    fn manga(url: &str, title: &str) -> WireManga {
        serde_json::from_value(json!({ "url": url, "title": title, "thumbnailUrl": "https://example.org/c.jpg" })).unwrap()
    }

    fn chapter(url: &str, name: &str, date_upload: i64) -> WireChapter {
        serde_json::from_value(json!({ "url": url, "name": name, "dateUpload": date_upload })).unwrap()
    }

    #[test]
    fn listing_makes_and_keeps_titles() {
        let mut conn = database();
        let cards = save_listing(&mut conn, 7, &[manga("/a", "Alpha"), manga("/b", "Beta"), manga("/a", "Alpha"), manga("", "No key")]).unwrap();
        assert_eq!(cards.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), ["Alpha", "Beta"]);
        assert!(cards[0].cover.as_deref().unwrap().starts_with(&format!("api/manga/{}/cover?v=", cards[0].id)));

        // Once opened, a title keeps its name whatever a list calls it, and stays one row.
        apply_update(&mut conn, cards[0].id, &manga("/a", "Alpha: The Full Name"), &[], true, false).unwrap();
        conn.execute("UPDATE manga SET in_library = 1 WHERE id = ?1", [cards[0].id]).unwrap();
        let again = save_listing(&mut conn, 7, &[manga("/a", "ALPHA")]).unwrap();
        assert_eq!((again[0].id, again[0].title.as_str(), again[0].in_library), (cards[0].id, "Alpha: The Full Name", true));
        assert_eq!(save_listing(&mut conn, 8, &[manga("/a", "Other source")]).unwrap()[0].title, "Other source");
    }

    #[test]
    fn chapters_follow_the_source() {
        let mut conn = database();
        let id = save_listing(&mut conn, 7, &[manga("/a", "Alpha")]).unwrap()[0].id;
        let day = 86_400_000;

        let first = [chapter("/a/2", "Chapter 2", 2 * day), chapter("/a/1", "Vol.1 Ch. 1.5 - Start", day)];
        assert!(apply_update(&mut conn, id, &manga("/a", "Alpha"), &first, true, true).unwrap().is_empty(), "a first fetch is not news");
        let title = read_title(&conn, id).unwrap();
        assert!(title.initialized);
        assert_eq!(title.chapters.iter().map(|c| (c.name.as_str(), c.number, c.fetched_at)).collect::<Vec<_>>(), [
            ("Chapter 2", 2.0, 2 * 86_400),
            ("Vol.1 Ch. 1.5 - Start", 1.5, 86_400)
        ]);

        // Reading state survives a refresh; a new chapter is reported; a dropped one goes.
        conn.execute("UPDATE chapters SET read = 1 WHERE manga_id = ?1 AND url = '/a/2'", [id]).unwrap();
        let second = [chapter("/a/3", "Chapter 3", 3 * day), chapter("/a/2", "Chapter 2 (v2)", 0)];
        let added = apply_update(&mut conn, id, &manga("/a", "Alpha"), &second, false, true).unwrap();
        let title = read_title(&conn, id).unwrap();
        assert_eq!(title.chapters.iter().map(|c| (c.name.as_str(), c.read)).collect::<Vec<_>>(), [("Chapter 3", false), ("Chapter 2 (v2)", true)]);
        assert_eq!(added, [title.chapters[0].id]);
        assert_eq!(title.chapters[1].date_upload, 2 * day, "an unknown date does not erase a known one");

        // An empty list is taken as a hiccup. A downloaded chapter is never dropped.
        apply_update(&mut conn, id, &manga("/a", "Alpha"), &[], false, true).unwrap();
        assert_eq!(read_title(&conn, id).unwrap().chapters.len(), 2);
        conn.execute("UPDATE chapters SET download_path = 'x.cbz' WHERE manga_id = ?1 AND url = '/a/2'", [id]).unwrap();
        apply_update(&mut conn, id, &manga("/a", "Alpha"), &[chapter("/a/4", "Chapter 4", 0)], false, true).unwrap();
        let names: Vec<String> = read_title(&conn, id).unwrap().chapters.into_iter().map(|c| c.name).collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"Chapter 2 (v2)".to_owned()) && names.contains(&"Chapter 4".to_owned()));
    }

    #[test]
    fn reader_neighbours() {
        let mut conn = database();
        let id = save_listing(&mut conn, 7, &[manga("/a", "Alpha")]).unwrap()[0].id;
        let list = [chapter("/3", "Three", 0), chapter("/2", "Two", 0), chapter("/1", "One", 0)];
        apply_update(&mut conn, id, &manga("/a", "Alpha"), &list, true, true).unwrap();
        let ids: Vec<i64> = read_title(&conn, id).unwrap().chapters.iter().map(|c| c.id).collect();

        let middle = read_reader(&conn, ids[1]).unwrap();
        assert_eq!((middle.prev, middle.next), (Some(ids[2]), Some(ids[0])), "reading goes from the bottom of the list up");
        assert_eq!(read_reader(&conn, ids[0]).unwrap().next, None);
        assert_eq!(read_reader(&conn, ids[2]).unwrap().prev, None);
        assert!(read_reader(&conn, 999).is_err());
    }

    #[test]
    fn kinds() {
        for (genre, source, lang, kind) in [
            (Some("Action, Manhwa"), Some("Example"), Some("en"), "manhwa"),
            (Some("Drama, Long Strip"), None, None, "manhwa"),
            (Some("Manhua, Comedy"), Some("Some Toons"), Some("en"), "manhua"),
            (Some("Comic"), None, Some("en"), "comic"),
            (Some("Manga, Romance"), Some("Some Toons"), Some("ko"), "manga"),
            (Some("Action"), Some("Some Toons"), Some("en"), "manhwa"),
            (Some("Action, Comical"), Some("Example"), Some("en"), "manga"),
            (None, Some("Example"), Some("ko"), "manhwa"),
            (None, Some("Example"), Some("zh-Hans"), "manhua"),
            (None, None, None, "manga"),
        ] {
            assert_eq!(kind_of(genre, source, lang), kind, "{genre:?} {source:?} {lang:?}");
        }
    }

    #[test]
    fn a_title_keeps_its_reader_settings() {
        let mut conn = database();
        let id = save_listing(&mut conn, 7, &[manga("/a", "Alpha")]).unwrap()[0].id;
        apply_update(&mut conn, id, &manga("/a", "Alpha"), &[chapter("/1", "One", 0)], true, true).unwrap();
        let chapter = read_title(&conn, id).unwrap().chapters[0].id;
        let plain = read_reader(&conn, chapter).unwrap();
        assert_eq!((plain.kind, plain.reader_prefs), ("manga", None));

        let kept = stored_prefs(Some(json!({ "contrast": 120 }))).unwrap();
        conn.execute("UPDATE manga SET genre = 'Action, Webtoon', reader_prefs = ?2 WHERE id = ?1", params![id, kept]).unwrap();
        let view = read_reader(&conn, chapter).unwrap();
        assert_eq!((view.kind, view.reader_prefs), ("manhwa", Some(json!({ "contrast": 120 }))));

        assert_eq!(stored_prefs(Some(json!({}))).unwrap(), None, "nothing kept is stored as nothing");
        assert_eq!(stored_prefs(None).unwrap(), None);
        assert!(stored_prefs(Some(json!([1]))).is_err());
        assert!(stored_prefs(Some(json!({ "tint": "x".repeat(MAX_READER_PREFS) }))).is_err());
    }

    #[test]
    fn a_title_tells_its_rhythm_and_its_bookmarks() {
        let mut conn = database();
        let id = save_listing(&mut conn, 7, &[manga("/a", "Alpha")]).unwrap()[0].id;
        let list = [chapter("/3", "Three", 21 * DAY_MS), chapter("/2", "Two", 14 * DAY_MS), chapter("/1", "One", 7 * DAY_MS)];
        apply_update(&mut conn, id, &manga("/a", "Alpha"), &list, true, true).unwrap();
        conn.execute("UPDATE chapters SET bookmark = 1 WHERE manga_id = ?1 AND url = '/2'", [id]).unwrap();
        let title = read_title(&conn, id).unwrap();
        assert_eq!(title.release_days, Some(7));
        assert_eq!(title.chapters.iter().map(|c| c.bookmark).collect::<Vec<_>>(), [false, true, false]);
        assert!(read_reader(&conn, title.chapters[1].id).unwrap().chapter.bookmark);
        assert_eq!(among(&Some(Vec::new())), None, "no chapter named is no chapter changed");
        assert_eq!(among(&None).as_deref(), Some(""));
    }

    #[test]
    fn rhythms() {
        assert_eq!(rhythm(vec![10, 3, 17, 17, 24]), Some((7, 24)));
        assert_eq!(rhythm(vec![1, 2]), None, "two days say too little");
        assert_eq!(rhythm(vec![5, 5, 5, 5]), None, "what came out together is one day");
        assert_eq!(rhythm(vec![0, 7, 14, 60, 67, 74]), Some((7, 74)), "one long break does not sway it");
        assert_eq!(rhythm(vec![0, 400, 800]), Some((MAX_RELEASE_DAYS, 800)));
        assert_eq!(rhythm((0..40).collect()), Some((1, 39)));
    }

    #[test]
    fn numbers_from_names() {
        for (name, number) in [
            ("Chapter 12", 12.0),
            ("Vol.2 Ch. 10.5 - Rain", 10.5),
            ("Episode 7", 7.0),
            ("#33 The End", 33.0),
            ("Volume 3 - 41 A Title", 41.0),
            ("Season 2 14", 14.0),
            ("104.", 104.0),
            ("Oneshot", -1.0),
        ] {
            assert_eq!(number_from_name(name), number, "{name}");
        }
    }

    #[test]
    fn links() {
        assert_eq!(web_url(None, Some("https://example.org/".into()), "/t/1").as_deref(), Some("https://example.org/t/1"));
        assert_eq!(web_url(None, None, "https://example.org/t/1").as_deref(), Some("https://example.org/t/1"));
        assert_eq!(web_url(None, Some("javascript:alert(1)".into()), "/t/1"), None);
        assert_eq!(web_url(None, Some("https://example.org".into()), "opaque-key"), None);
        assert_eq!(web_url(Some("https://example.org/title/9".into()), None, "opaque-key").as_deref(), Some("https://example.org/title/9"), "the source's own wins");
        assert_eq!(web_url(Some("javascript:alert(1)".into()), None, "opaque-key"), None);
        assert_eq!(cover_url(3, Some("")), None);
    }

    #[test]
    fn volumes_from_names() {
        for (name, volume) in [("Vol.2 Ch. 10", Some(2.0)), ("Volume 3 - 41 A Title", Some(3.0)), ("vol 4 chapter 1", Some(4.0)), ("Chapter 9", None), ("Volatile 2", None)] {
            assert_eq!(volume_from_name(name), volume, "{name}");
        }
    }

    #[test]
    fn edits_hold_against_the_source() {
        let mut conn = database();
        let id = save_listing(&mut conn, 7, &[manga("/a", "Alpha")]).unwrap()[0].id;
        apply_update(&mut conn, id, &manga("/a", "Alpha"), &[], true, false).unwrap();
        let edit = Edit { title: Some(" Alpha, Mine ".into()), author: Some(String::new()), genres: Some(vec!["Action".into(), " ".into(), "Drama".into()]), ..Edit::default() };
        assert_eq!(edit_details(&mut conn, id, &edit).unwrap(), 1);
        assert_eq!(edit_details(&mut conn, 999, &edit).unwrap(), 0);
        assert!(edit_details(&mut conn, id, &Edit { title: Some("  ".into()), ..Edit::default() }).is_err(), "a name cannot be cleared");
        assert!(edit_details(&mut conn, id, &Edit::default()).is_err());

        let mut source = manga("/a", "Alpha (source)");
        source.author = Some("Someone".into());
        source.description = Some("From the source".into());
        apply_update(&mut conn, id, &source, &[], true, false).unwrap();
        save_listing(&mut conn, 7, &[manga("/a", "ALPHA")]).unwrap();
        let title = read_title(&conn, id).unwrap();
        assert_eq!((title.title.as_str(), title.author.as_deref(), title.description.as_deref()), ("Alpha, Mine", None, Some("From the source")));
        assert_eq!(title.genres, ["Action", "Drama"]);
        assert_eq!(title.locked, ["title", "author", "genre"]);

        conn.execute("UPDATE manga SET thumbnail_url = 'custom:abc', locked = ?2 WHERE id = ?1", params![id, with_locks(&conn, id, &["cover"]).unwrap()]).unwrap();
        apply_update(&mut conn, id, &source, &[], true, false).unwrap();
        save_listing(&mut conn, 7, &[manga("/a", "Alpha")]).unwrap();
        let thumbnail: String = conn.query_row("SELECT thumbnail_url FROM manga WHERE id = ?1", [id], |r| r.get(0)).unwrap();
        assert_eq!(thumbnail, "custom:abc", "a cover set by hand stays");
    }

    #[test]
    fn merged_titles_read_as_one() {
        let mut conn = database();
        let cards = save_listing(&mut conn, 7, &[manga("/a", "Alpha")]).unwrap();
        let (head, other) = (cards[0].id, save_listing(&mut conn, 8, &[manga("/b", "Alpha")]).unwrap()[0].id);
        apply_update(&mut conn, head, &manga("/a", "Alpha"), &[chapter("/a/2", "Chapter 2", 0), chapter("/a/1", "Chapter 1", 0)], true, true).unwrap();
        apply_update(&mut conn, other, &manga("/b", "Alpha"), &[chapter("/b/3", "Chapter 3", 0), chapter("/b/2", "Chapter 2", 0)], true, true).unwrap();
        conn.execute("UPDATE manga SET in_library = 1 WHERE id IN (?1, ?2)", [head, other]).unwrap();
        assert_eq!(namesakes(&conn, head).unwrap().iter().map(|n| n.id).collect::<Vec<_>>(), [other]);

        merge(&mut conn, head, other).unwrap();
        assert!(merge(&mut conn, other, head).is_err(), "a title is not merged into itself, by any way round");
        let title = read_title(&conn, head).unwrap();
        assert_eq!(title.members.iter().map(|m| m.id).collect::<Vec<_>>(), [head, other]);
        let listed = |conn: &Connection| read_title(conn, head).unwrap().chapters.iter().map(|c| (c.number, c.manga_id, c.duplicate)).collect::<Vec<_>>();
        assert_eq!(listed(&conn), [(3.0, other, false), (2.0, head, false), (2.0, other, false), (1.0, head, false)]);
        assert_eq!(read_title(&conn, other).unwrap().merged_into, Some(head));
        let in_library: bool = conn.query_row("SELECT in_library FROM manga WHERE id = ?1", [other], |r| r.get(0)).unwrap();
        assert!(!in_library, "a merged title is no longer in the library on its own");

        // One upload per chapter: the first source's.
        conn.execute("UPDATE manga SET dedupe = 1 WHERE id = ?1", [head]).unwrap();
        assert_eq!(listed(&conn), [(3.0, other, false), (2.0, head, false), (2.0, other, true), (1.0, head, false)]);
        let duplicate = read_title(&conn, head).unwrap().chapters[2].id;
        let reader = read_reader(&conn, duplicate).unwrap();
        assert_eq!((reader.manga_id, reader.chapters.len()), (head, 4), "an open duplicate stays in reach");
        let first = read_title(&conn, head).unwrap().chapters[3].id;
        let reader = read_reader(&conn, first).unwrap();
        assert_eq!(reader.chapters.len(), 3, "otherwise reading passes over it");
        assert_eq!(reader.next, Some(read_title(&conn, head).unwrap().chapters[1].id));

        // Reading one upload to its end marks the other read when asked to.
        settings::set(&conn, MARK_DUPLICATES, &json!(true)).unwrap();
        let twin = read_title(&conn, head).unwrap().chapters[1].id;
        conn.execute("UPDATE chapters SET read = 1 WHERE id = ?1", [twin]).unwrap();
        read_duplicates(&conn, twin).unwrap();
        assert!(read_title(&conn, head).unwrap().chapters[2].read);

        conn.execute("UPDATE manga SET merged_into = NULL WHERE id = ?1", [other]).unwrap();
        assert_eq!(read_title(&conn, head).unwrap().chapters.len(), 2);
    }

    #[test]
    fn groups_in_order() {
        let mut list: Vec<ChapterView> = [(1.0, "A"), (1.0, "B"), (2.0, "B"), (-1.0, "C")]
            .iter()
            .enumerate()
            .map(|(id, (number, group))| ChapterView {
                id: id as i64,
                name: format!("Ch {number}"),
                number: *number,
                scanlator: Some((*group).into()),
                date_upload: 0,
                fetched_at: 0,
                read: false,
                last_page_read: 0,
                page_count: None,
                downloaded: false,
                bookmark: false,
                manga_id: 1,
                volume: None,
                duplicate: false,
            })
            .collect();
        prefer(&mut list, &[1], &["B".into()]);
        assert_eq!(list.iter().map(|c| c.duplicate).collect::<Vec<_>>(), [true, false, false, false]);
        prefer(&mut list, &[1], &[]);
        assert_eq!(list.iter().map(|c| c.duplicate).collect::<Vec<_>>(), [false, true, false, false], "no order: the first listed");
    }

    #[test]
    fn pages_keep_notes() {
        let mut conn = database();
        let id = save_listing(&mut conn, 7, &[manga("/a", "Alpha")]).unwrap()[0].id;
        apply_update(&mut conn, id, &manga("/a", "Alpha"), &[chapter("/1", "One", 0)], true, true).unwrap();
        let chapter = read_title(&conn, id).unwrap().chapters[0].id;
        conn.execute("INSERT INTO page_marks (chapter_id, page, note, created_at) VALUES (?1, 4, 'the map', 10)", [chapter]).unwrap();
        let marks = read_reader(&conn, chapter).unwrap().marks;
        assert_eq!(marks, [PageMark { chapter_id: chapter, chapter_name: "One".into(), page: 4, note: "the map".into(), created_at: 10 }]);
        assert_eq!(plain_name("Solo Leveling!"), plain_name("solo  leveling"));
    }

    #[test]
    fn streamed_pages_move_the_place_on() {
        let mut conn = database();
        let id = save_listing(&mut conn, 7, &[manga("/a", "Alpha")]).unwrap()[0].id;
        apply_update(&mut conn, id, &manga("/a", "Alpha"), &[chapter("/1", "One", 0)], true, true).unwrap();
        let chapter = read_title(&conn, id).unwrap().chapters[0].id;
        let place = |conn: &Connection| read_title(conn, id).unwrap().chapters.iter().map(|c| (c.last_page_read, c.read)).next().unwrap();
        conn.execute("UPDATE chapters SET page_count = 5 WHERE id = ?1", [chapter]).unwrap();

        assert!(!streamed_page(&conn, chapter, 2).unwrap());
        assert_eq!(place(&conn), (2, false));
        assert!(!streamed_page(&conn, chapter, 1).unwrap(), "going back leaves the place");
        assert_eq!(place(&conn), (2, false));
        assert!(streamed_page(&conn, chapter, 4).unwrap(), "the last page finishes it");
        assert_eq!(place(&conn), (4, true));
        conn.execute("UPDATE chapters SET read = 1, last_page_read = 0 WHERE id = ?1", [chapter]).unwrap();
        assert!(!streamed_page(&conn, chapter, 3).unwrap());
        assert_eq!(place(&conn), (0, true), "a chapter read already is left alone");

        conn.execute("UPDATE chapters SET read = 0 WHERE id = ?1", [chapter]).unwrap();
        settings::set(&conn, OPDS_PROGRESS, &json!(false)).unwrap();
        assert!(!streamed_page(&conn, chapter, 4).unwrap());
        assert_eq!(place(&conn), (0, false), "nothing moves with the option off");
    }
}
