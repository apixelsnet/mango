//! The download queue. A chapter is fetched page by page and saved as one CBZ at
//! `<data>/downloads/<source>/<title>/<chapter>.cbz`, with a `ComicInfo.xml` inside.
//! The queue lives in the database, so it survives a restart.

use crate::{
    db::{now, Db},
    error::{AppError, Result},
    images::Images,
    settings,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};
use tokio::sync::Notify;

pub const PAUSED: &str = "downloads.paused";
pub const DELETE_READ: &str = "downloads.deleteAfterReading";
/// Tries a chapter gets before it is parked as failed.
const TRIES: i64 = 5;
/// Seconds to wait before the next try, by how many have failed so far.
const WAITS: [i64; 4] = [30, 120, 600, 1800];

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Queued {
    pub chapter_id: i64,
    pub chapter_name: String,
    pub manga_id: i64,
    pub title: String,
    /// queued, downloading or failed.
    pub state: String,
    pub done: i64,
    pub total: Option<i64>,
    pub attempts: i64,
    pub retry_at: Option<i64>,
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct QueueView {
    pub paused: bool,
    pub queue: Vec<Queued>,
}

/// A chapter picked from the queue, with what its file name and ComicInfo need.
struct Job {
    chapter: i64,
    attempts: i64,
    source: String,
    title: String,
    name: String,
    number: f64,
    scanlator: Option<String>,
    author: Option<String>,
    artist: Option<String>,
    genre: Option<String>,
    summary: Option<String>,
    lang: Option<String>,
}

#[derive(Clone)]
pub struct Downloads {
    db: Db,
    images: Images,
    dir: PathBuf,
    data_dir: PathBuf,
    /// A chapter is not fetched while the disk has less than this free, in bytes. 0: always.
    keep_free: u64,
    wake: Arc<Notify>,
    /// The chapter being fetched: its id, pages done, pages in all.
    progress: Arc<StdMutex<Option<(i64, i64, i64)>>>,
}

impl Downloads {
    pub fn new(db: Db, images: Images, data_dir: &Path, keep_free_mb: u64) -> Self {
        Self {
            db,
            images,
            dir: data_dir.join("downloads"),
            data_dir: data_dir.to_owned(),
            keep_free: keep_free_mb << 20,
            wake: Arc::default(),
            progress: Arc::default(),
        }
    }

    /// Adds chapters to the end of the queue. Ones already saved or queued are skipped.
    pub async fn enqueue(&self, chapters: Vec<i64>) -> Result<usize> {
        let added = self.db.call(move |conn| add(conn, &chapters)).await?;
        self.wake.notify_one();
        Ok(added)
    }

    /// Queues the unread chapters of these titles, oldest first.
    pub async fn enqueue_unread(&self, titles: Vec<i64>) -> Result<usize> {
        let added = self
            .db
            .call(move |conn| {
                let chapters = unread_of(conn, &titles)?;
                add(conn, &chapters)
            })
            .await?;
        self.wake.notify_one();
        Ok(added)
    }

    pub async fn list(&self) -> Result<QueueView> {
        let mut view = self.db.call(|conn| read_queue(conn)).await?;
        let live = *self.progress.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((chapter, done, total)) = live {
            if let Some(entry) = view.queue.iter_mut().find(|q| q.chapter_id == chapter) {
                entry.done = done;
                entry.total = Some(total);
            }
        }
        Ok(view)
    }

    /// Takes one chapter out of the queue, or with None all of them. A fetch in progress stops at its next page.
    pub async fn cancel(&self, chapter: Option<i64>) -> Result<()> {
        self.db
            .call(move |conn| {
                match chapter {
                    Some(id) => conn.execute("DELETE FROM downloads WHERE chapter_id = ?1", [id])?,
                    None => conn.execute("DELETE FROM downloads", [])?,
                };
                Ok(())
            })
            .await
    }

    /// Gives a parked chapter a fresh set of tries.
    pub async fn retry(&self, chapter: i64) -> Result<()> {
        self.db
            .call(move |conn| {
                conn.execute(
                    "UPDATE downloads SET state = 'queued', attempts = 0, retry_at = NULL, error = NULL WHERE chapter_id = ?1 AND state != 'downloading'",
                    [chapter],
                )?;
                Ok(())
            })
            .await?;
        self.wake.notify_one();
        Ok(())
    }

    pub async fn set_paused(&self, paused: bool) -> Result<()> {
        self.db.call(move |conn| settings::set(conn, PAUSED, &paused.into())).await?;
        self.wake.notify_one();
        Ok(())
    }

    /// Puts the queue in the order given. Chapters left out keep their order, after the others.
    pub async fn reorder(&self, chapters: Vec<i64>) -> Result<()> {
        self.db
            .call(move |conn| {
                let tx = conn.transaction()?;
                tx.execute("UPDATE downloads SET position = position + ?1", [chapters.len() as i64 + 1_000_000_000])?;
                for (position, id) in chapters.iter().enumerate() {
                    tx.execute("UPDATE downloads SET position = ?2 WHERE chapter_id = ?1", params![id, position as i64])?;
                }
                tx.commit()?;
                Ok(())
            })
            .await
    }

    /// Deletes a chapter's file. The chapter and its progress stay.
    pub async fn delete(&self, chapter: i64) -> Result<()> {
        let path = self
            .db
            .call(move |conn| {
                let path: Option<String> =
                    conn.query_row("SELECT download_path FROM chapters WHERE id = ?1", [chapter], |r| r.get(0)).optional()?.flatten();
                conn.execute("UPDATE chapters SET download_path = NULL WHERE id = ?1", [chapter])?;
                Ok(path)
            })
            .await?;
        if let Some(relative) = path {
            let file = self.dir.join(relative);
            let _ = tokio::task::spawn_blocking(move || {
                let _ = fs::remove_file(&file);
                // The title's folder and the source's go too, when this was the last file in them.
                for folder in file.ancestors().skip(1).take(2) {
                    let _ = fs::remove_dir(folder);
                }
            })
            .await;
        }
        Ok(())
    }

    /// Called when a chapter is finished in the reader: deletes its file if the owner asked for that.
    pub async fn finished_reading(&self, chapter: i64) {
        let wanted = self.db.call(|conn| Ok(settings::get::<bool>(conn, DELETE_READ).unwrap_or(false))).await.unwrap_or(false);
        if wanted {
            if let Err(e) = self.delete(chapter).await {
                tracing::warn!("could not delete the file of a chapter that was read: {e}");
            }
        }
    }

    /// Works through the queue, one chapter at a time. Never returns.
    pub async fn run(self) {
        // Whatever was being fetched when Mango stopped starts over.
        let _ = self
            .db
            .call(|conn| {
                conn.execute("UPDATE downloads SET state = 'queued' WHERE state = 'downloading'", [])?;
                Ok(())
            })
            .await;
        loop {
            let Ok(Some(job)) = self.db.call(|conn| next_job(conn)).await else {
                // Nothing due. Wait for a nudge, and look again before long in case a retry has come due.
                let _ = tokio::time::timeout(Duration::from_secs(30), self.wake.notified()).await;
                continue;
            };
            let outcome = self.save(&job).await;
            *self.progress.lock().unwrap_or_else(|e| e.into_inner()) = None;
            if let Err(e) = &outcome {
                tracing::warn!("download of {} / {} failed: {e}", job.title, job.name);
            }
            let (chapter, attempts) = (job.chapter, job.attempts);
            if let Err(e) = self.db.call(move |conn| record(conn, chapter, attempts, outcome)).await {
                tracing::warn!("could not record a download: {e}");
            }
        }
    }

    /// Fetches a chapter and writes its CBZ. None when it was cancelled or paused part way.
    async fn save(&self, job: &Job) -> Result<Option<(String, i64)>> {
        // A disk that fills up takes the database's next write with it.
        if self.keep_free > 0 && free_space(&self.data_dir).is_some_and(|free| free < self.keep_free) {
            let why = format!("Less than {} MB is free on the disk. Delete some downloads to make room.", self.keep_free >> 20);
            return Err(AppError::new(507, "disk_full", why));
        }
        let total = self.images.page_count(job.chapter).await?;
        let relative = self.place(job);
        let file = self.dir.join(&relative);
        let part = file.with_extension("cbz.part");
        let outcome = self.write(job, total, &part).await;
        if !matches!(outcome, Ok(true)) {
            let _ = fs::remove_file(&part);
            return outcome.map(|_| None);
        }
        fs::rename(&part, &file)?;
        Ok(Some((relative.to_string_lossy().replace('\\', "/"), total)))
    }

    /// Writes the pages and the ComicInfo into `part`. False when the chapter stopped being wanted.
    async fn write(&self, job: &Job, total: i64, part: &Path) -> Result<bool> {
        let blocking = |e: tokio::task::JoinError| AppError::internal(e.to_string());
        let stored = || zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        if let Some(folder) = part.parent() {
            fs::create_dir_all(folder)?;
        }
        let archive = Arc::new(StdMutex::new(zip::ZipWriter::new(fs::File::create(part)?)));

        // Two pages are fetched at a time and written in order, so memory holds two pages at most.
        let mut next = 0;
        while next < total {
            if !self.wanted(job.chapter).await {
                return Ok(false);
            }
            let second = (next + 1 < total).then_some(next + 1);
            let (first, other) = tokio::join!(self.images.page(job.chapter, next), async {
                match second {
                    Some(index) => Some(self.images.page(job.chapter, index).await),
                    None => None,
                }
            });
            let mut pages = vec![first?];
            if let Some(page) = other {
                pages.push(page?);
            }
            let (writer, from) = (archive.clone(), next);
            next += pages.len() as i64;
            tokio::task::spawn_blocking(move || -> Result<()> {
                let mut zip = writer.lock().unwrap_or_else(|e| e.into_inner());
                for (offset, page) in pages.iter().enumerate() {
                    zip.start_file(format!("{:03}.{}", from + offset as i64 + 1, extension(page.content_type)), stored())?;
                    zip.write_all(&page.bytes)?;
                }
                Ok(())
            })
            .await
            .map_err(blocking)??;
            *self.progress.lock().unwrap_or_else(|e| e.into_inner()) = Some((job.chapter, next, total));
        }

        let info = comic_info(job, total);
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut zip = Arc::try_unwrap(archive)
                .ok()
                .and_then(|lock| lock.into_inner().ok())
                .ok_or_else(|| AppError::internal("the archive is still in use"))?;
            zip.start_file("ComicInfo.xml", stored())?;
            zip.write_all(info.as_bytes())?;
            zip.finish()?.sync_all()?;
            Ok(())
        })
        .await
        .map_err(blocking)??;
        Ok(true)
    }

    /// Is this chapter still to be fetched? Not when it was cancelled, or the queue was paused.
    async fn wanted(&self, chapter: i64) -> bool {
        self.db
            .call(move |conn| {
                let queued = conn.query_row("SELECT 1 FROM downloads WHERE chapter_id = ?1", [chapter], |_| Ok(())).optional()?.is_some();
                Ok(queued && !settings::get::<bool>(conn, PAUSED).unwrap_or(false))
            })
            .await
            .unwrap_or(false)
    }

    /// Where a chapter's file goes, relative to the downloads folder.
    fn place(&self, job: &Job) -> PathBuf {
        let folder = Path::new(&clean(&job.source)).join(clean(&job.title));
        let name = match job.scanlator.as_deref().filter(|s| !s.is_empty()) {
            Some(scanlator) => clean(&format!("{} [{scanlator}]", job.name)),
            None => clean(&job.name),
        };
        let path = folder.join(format!("{name}.cbz"));
        // Two chapters of one title can share a name; the second gets its id added.
        if self.dir.join(&path).exists() {
            return folder.join(format!("{name} ({}).cbz", job.chapter));
        }
        path
    }
}

fn add(conn: &mut Connection, chapters: &[i64]) -> Result<usize> {
    let tx = conn.transaction()?;
    let mut added = 0;
    for chapter in chapters {
        added += tx.execute(
            "INSERT OR IGNORE INTO downloads (chapter_id, position, added_at)
             SELECT id, (SELECT COALESCE(MAX(position), 0) + 1 FROM downloads), ?2 FROM chapters WHERE id = ?1 AND download_path IS NULL",
            params![chapter, now()],
        )?;
    }
    tx.commit()?;
    Ok(added)
}

fn unread_of(conn: &Connection, titles: &[i64]) -> Result<Vec<i64>> {
    let list = titles.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    let mut stmt = conn.prepare(&format!(
        "SELECT id FROM chapters WHERE manga_id IN ({list}) AND read = 0 AND download_path IS NULL ORDER BY manga_id, source_order DESC"
    ))?;
    let chapters = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<i64>>>()?;
    Ok(chapters)
}

fn read_queue(conn: &Connection) -> Result<QueueView> {
    let mut stmt = conn.prepare(
        "SELECT d.chapter_id, c.name, m.id, m.title, d.state, c.page_count, d.attempts, d.retry_at, d.error
         FROM downloads d JOIN chapters c ON c.id = d.chapter_id JOIN manga m ON m.id = c.manga_id
         ORDER BY d.state != 'downloading', d.position, d.chapter_id",
    )?;
    let queue = stmt
        .query_map([], |r| {
            Ok(Queued {
                chapter_id: r.get(0)?,
                chapter_name: r.get(1)?,
                manga_id: r.get(2)?,
                title: r.get(3)?,
                state: r.get(4)?,
                done: 0,
                total: r.get(5)?,
                attempts: r.get(6)?,
                retry_at: r.get(7)?,
                error: r.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(QueueView { paused: settings::get::<bool>(conn, PAUSED).unwrap_or(false), queue })
}

/// The next chapter that is due, marked as being fetched. None while paused or when nothing is due.
fn next_job(conn: &Connection) -> Result<Option<Job>> {
    if settings::get::<bool>(conn, PAUSED).unwrap_or(false) {
        return Ok(None);
    }
    let job = conn
        .query_row(
            "SELECT d.chapter_id, d.attempts, COALESCE(s.name, 'Unknown source'), m.title, c.name, c.chapter_number, c.scanlator,
                    m.author, m.artist, m.genre, m.description, s.lang
             FROM downloads d JOIN chapters c ON c.id = d.chapter_id JOIN manga m ON m.id = c.manga_id
             LEFT JOIN sources s ON s.id = m.source_id
             WHERE d.state = 'queued' AND (d.retry_at IS NULL OR d.retry_at <= ?1)
             ORDER BY d.position, d.chapter_id LIMIT 1",
            [now()],
            |r| {
                Ok(Job {
                    chapter: r.get(0)?,
                    attempts: r.get(1)?,
                    source: r.get(2)?,
                    title: r.get(3)?,
                    name: r.get(4)?,
                    number: r.get(5)?,
                    scanlator: r.get(6)?,
                    author: r.get(7)?,
                    artist: r.get(8)?,
                    genre: r.get(9)?,
                    summary: r.get(10)?,
                    lang: r.get(11)?,
                })
            },
        )
        .optional()?;
    if let Some(job) = &job {
        conn.execute("UPDATE downloads SET state = 'downloading' WHERE chapter_id = ?1", [job.chapter])?;
    }
    Ok(job)
}

/// Writes down how a fetch ended: saved, put back, or failed with a later try or none.
fn record(conn: &Connection, chapter: i64, attempts: i64, outcome: Result<Option<(String, i64)>>) -> Result<()> {
    match outcome {
        Ok(Some((path, pages))) => {
            conn.execute("UPDATE chapters SET download_path = ?2, page_count = ?3 WHERE id = ?1", params![chapter, path, pages])?;
            conn.execute("DELETE FROM downloads WHERE chapter_id = ?1", [chapter])?;
        }
        Ok(None) => {
            conn.execute("UPDATE downloads SET state = 'queued' WHERE chapter_id = ?1", [chapter])?;
        }
        Err(e) => {
            let tried = attempts + 1;
            let (state, retry_at) = match WAITS.get(tried as usize - 1) {
                Some(wait) if tried < TRIES => ("queued", Some(now() + wait)),
                _ => ("failed", None),
            };
            conn.execute(
                "UPDATE downloads SET state = ?2, attempts = ?3, retry_at = ?4, error = ?5 WHERE chapter_id = ?1",
                params![chapter, state, tried, retry_at, e.message],
            )?;
        }
    }
    Ok(())
}

/// Bytes the disk that holds `dir` has free for Mango. None where that is not known.
#[cfg(unix)]
fn free_space(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    // SAFETY: statvfs fills the struct it is given, which is plain numbers, and reads a
    // path that lives until it returns.
    let mut seen: libc::statvfs = unsafe { std::mem::zeroed() };
    let done = unsafe { libc::statvfs(path.as_ptr(), &mut seen) };
    #[allow(clippy::unnecessary_cast)]
    (done == 0).then(|| seen.f_bavail as u64 * seen.f_frsize as u64)
}

#[cfg(not(unix))]
fn free_space(_: &Path) -> Option<u64> {
    None
}

/// A name that is safe as a file or folder on Linux, macOS and Windows.
fn clean(name: &str) -> String {
    let safe: String = name.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { '_' } else { c }).collect();
    let safe: String = safe.trim().trim_end_matches('.').trim().chars().take(120).collect();
    let reserved = ["CON", "PRN", "AUX", "NUL"].iter().any(|word| safe.eq_ignore_ascii_case(word));
    match safe.as_str() {
        "" | "." | ".." => "_".into(),
        _ if reserved => format!("_{safe}"),
        _ => safe.trim_end().to_owned(),
    }
}

pub(crate) fn extension(content_type: &str) -> &'static str {
    match content_type {
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/avif" => "avif",
        "image/jxl" => "jxl",
        "image/bmp" => "bmp",
        _ => "jpg",
    }
}

/// The ComicInfo.xml other readers look for inside a CBZ.
fn comic_info(job: &Job, pages: i64) -> String {
    fn escape(text: &str) -> String {
        text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
    }
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<ComicInfo>\n");
    let mut field = |tag: &str, value: Option<&str>| {
        if let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) {
            xml.push_str(&format!("  <{tag}>{}</{tag}>\n", escape(value)));
        }
    };
    let number = (job.number >= 0.0).then(|| job.number.to_string());
    field("Title", Some(&job.name));
    field("Series", Some(&job.title));
    field("Number", number.as_deref());
    field("Summary", job.summary.as_deref());
    field("Writer", job.author.as_deref());
    field("Penciller", job.artist.as_deref());
    field("Genre", job.genre.as_deref());
    field("Translator", job.scanlator.as_deref());
    field("LanguageISO", job.lang.as_deref().filter(|lang| !matches!(*lang, "all" | "other")));
    field("PageCount", Some(&pages.to_string()));
    field("Notes", Some("Saved by Mango"));
    xml.push_str("</ComicInfo>\n");
    xml
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&mut conn).unwrap();
        conn.execute("INSERT INTO manga (id, source_id, url, title, in_library) VALUES (1, 7, '/a', 'Alpha: A/B?', 1)", []).unwrap();
        // Listed newest first: 12 is the oldest chapter.
        for (id, order, read) in [(10, 0, 0), (11, 1, 1), (12, 2, 0)] {
            conn.execute(
                "INSERT INTO chapters (id, manga_id, url, name, source_order, fetched_at, read) VALUES (?1, 1, ?1, 'Ch ' || ?1, ?2, 0, ?3)",
                params![id, order, read],
            )
            .unwrap();
        }
        conn
    }

    fn states(conn: &Connection) -> Vec<(i64, String, i64)> {
        read_queue(conn).unwrap().queue.into_iter().map(|q| (q.chapter_id, q.state, q.attempts)).collect()
    }

    #[test]
    fn queue_from_add_to_saved() {
        let mut conn = database();
        let unread = unread_of(&conn, &[1]).unwrap();
        assert_eq!(unread, [12, 10], "unread chapters, oldest first");
        assert_eq!(add(&mut conn, &unread).unwrap(), 2);
        assert_eq!(add(&mut conn, &[12, 999]).unwrap(), 0, "queued and unknown chapters are skipped");

        let job = next_job(&conn).unwrap().unwrap();
        assert_eq!((job.chapter, job.source.as_str()), (12, "Unknown source"));
        assert_eq!(states(&conn)[0], (12, "downloading".into(), 0));
        assert_eq!(next_job(&conn).unwrap().unwrap().chapter, 10, "the one being fetched is not handed out twice");

        // A failure waits before its next try; five in a row park the chapter.
        record(&conn, 12, 0, Err(AppError::new(502, "source_error", "boom"))).unwrap();
        let queue = read_queue(&conn).unwrap().queue;
        let failed = queue.iter().find(|q| q.chapter_id == 12).unwrap();
        assert_eq!((failed.state.as_str(), failed.attempts, failed.error.as_deref()), ("queued", 1, Some("boom")));
        assert!(failed.retry_at.unwrap() > now());
        record(&conn, 12, 4, Err(AppError::internal("still boom"))).unwrap();
        assert!(states(&conn).contains(&(12, "failed".into(), 5)));

        record(&conn, 10, 0, Ok(None)).unwrap();
        assert!(states(&conn).contains(&(10, "queued".into(), 0)), "a cancelled fetch goes back in line");
        record(&conn, 10, 0, Ok(Some(("Src/Alpha/Ch 10.cbz".into(), 30)))).unwrap();
        let saved: (Option<String>, Option<i64>) =
            conn.query_row("SELECT download_path, page_count FROM chapters WHERE id = 10", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(saved, (Some("Src/Alpha/Ch 10.cbz".into()), Some(30)));
        assert_eq!(states(&conn).len(), 1, "a saved chapter leaves the queue");
        assert_eq!(add(&mut conn, &[10]).unwrap(), 0, "and is not queued again");
    }

    #[test]
    fn pause_holds_the_queue() {
        let mut conn = database();
        add(&mut conn, &[10]).unwrap();
        settings::set(&conn, PAUSED, &true.into()).unwrap();
        assert!(next_job(&conn).unwrap().is_none());
        assert!(read_queue(&conn).unwrap().paused);
        settings::set(&conn, PAUSED, &false.into()).unwrap();
        assert!(next_job(&conn).unwrap().is_some());
    }

    #[test]
    fn names_and_comic_info() {
        assert_eq!(clean("Alpha: A/B?"), "Alpha_ A_B_");
        assert_eq!(clean("  dots... "), "dots");
        assert_eq!(clean(".."), "_");
        assert_eq!(clean("nul"), "_nul");
        assert_eq!(clean(&"x".repeat(300)).len(), 120);

        let job = Job {
            chapter: 1,
            attempts: 0,
            source: "Src".into(),
            title: "Tom & Jerry <3".into(),
            name: "Ch \"1\"".into(),
            number: 1.5,
            scanlator: Some(String::new()),
            author: Some("A".into()),
            artist: None,
            genre: None,
            summary: None,
            lang: Some("all".into()),
        };
        let xml = comic_info(&job, 12);
        assert!(xml.contains("<Series>Tom &amp; Jerry &lt;3</Series>"));
        assert!(xml.contains("<Title>Ch &quot;1&quot;</Title>"));
        assert!(xml.contains("<Number>1.5</Number>") && xml.contains("<PageCount>12</PageCount>"));
        assert!(!xml.contains("Translator") && !xml.contains("LanguageISO"), "empty fields are left out");
    }
}
