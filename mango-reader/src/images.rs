//! Covers and pages. They are fetched through the extension host, so each source's
//! own headers and cookies apply, and then kept on disk.
//!
//! `<data>/cache` holds pages and the covers of titles that were only browsed; it is
//! swept to stay under a size cap, oldest use first. `<data>/covers` holds the covers
//! of library titles and is never swept, so the library paints with the host stopped.

use crate::{
    db::Db,
    error::{AppError, Result},
    host::{Call, Host},
    local::{self, Local},
    titles::{fnv, wire_chapter, WirePage},
};
use reqwest::Method;
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex as StdMutex,
    },
    time::{Duration, Instant, SystemTime},
};
use tokio::sync::{Mutex, Notify, OwnedMutexGuard};

/// How long a chapter's page list is trusted. Sources sign page addresses that expire.
const LIST_TTL: Duration = Duration::from_secs(600);
/// A failed page asks for a new list only when the one at hand is at least this old.
const LIST_RETRY_AGE: Duration = Duration::from_secs(30);
const SWEEP_EVERY: Duration = Duration::from_secs(600);
/// A JPEG cover bigger than this is made smaller before it is kept or sent. Some sources
/// hand out the scan itself, tens of megabytes, for a picture shown a few hundred pixels wide.
const SHRINK_FROM: usize = 256 << 10;
/// The smallest a shrunk cover may come out, in pixels. Decoding only scales by halves,
/// so it lands between this and twice it.
const COVER_BOX: (u16, u16) = (600, 900);
/// A progressive JPEG is held whole while it is decoded, about 7 bytes a pixel, so a
/// bigger one is left as it came.
const PROGRESSIVE_MAX_PIXELS: u32 = 16_000_000;
/// Covers are shrunk one at a time in the whole process, so a page of them cannot add up.
static SHRINKING: StdMutex<()> = StdMutex::new(());

pub struct Image {
    pub bytes: Vec<u8>,
    pub content_type: &'static str,
}

impl Image {
    /// Accepts only what is recognisably an image, so a source cannot hand the browser a page of its own.
    fn checked(bytes: Vec<u8>) -> Result<Self> {
        match sniff(&bytes) {
            Some(content_type) => Ok(Self { bytes, content_type }),
            None => Err(AppError::new(502, "source_error", "The source did not send an image")),
        }
    }
}

/// Where a chapter's pages come from.
enum Kept {
    /// A CBZ under the downloads folder.
    File(PathBuf),
    /// The local source's folder: the title's and the chapter's names in it.
    Local { title: String, chapter: String },
    /// The source, through the extension host.
    Remote,
}

struct PageList {
    fetched: Instant,
    source: i64,
    pages: Arc<Vec<WirePage>>,
}

/// One lock per key, so two requests for the same file fetch it once.
#[derive(Clone, Default)]
struct KeyedLock(Arc<StdMutex<HashMap<String, Arc<Mutex<()>>>>>);

impl KeyedLock {
    async fn lock(&self, key: String) -> OwnedMutexGuard<()> {
        let lock = {
            let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
            map.retain(|_, lock| Arc::strong_count(lock) > 1);
            map.entry(key).or_default().clone()
        };
        lock.lock_owned().await
    }
}

#[derive(Clone)]
pub struct Images {
    db: Db,
    host: Arc<Host>,
    local: Local,
    cache: PathBuf,
    covers: PathBuf,
    downloads: PathBuf,
    max_bytes: u64,
    /// Bytes put in the cache since the last sweep, and the call for an early one.
    written: Arc<AtomicU64>,
    early: Arc<Notify>,
    lists: Arc<StdMutex<HashMap<i64, PageList>>>,
    locks: KeyedLock,
}

impl Images {
    pub fn new(db: Db, host: Arc<Host>, local: Local, data_dir: &Path, cache_mb: u64) -> Self {
        Self {
            db,
            host,
            local,
            cache: data_dir.join("cache"),
            covers: data_dir.join("covers"),
            downloads: data_dir.join("downloads"),
            max_bytes: cache_mb.saturating_mul(1 << 20),
            written: Arc::default(),
            early: Arc::default(),
            lists: Arc::default(),
            locks: KeyedLock::default(),
        }
    }

    pub async fn cover(&self, manga: i64) -> Result<Image> {
        let row = self
            .db
            .call(move |conn| {
                Ok(conn
                    .query_row("SELECT source_id, thumbnail_url, in_library, url FROM manga WHERE id = ?1", [manga], |r| {
                        Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, bool>(2)?, r.get::<_, String>(3)?))
                    })
                    .optional()?)
            })
            .await?;
        let Some((source, Some(url), in_library, key)) = row else {
            return Err(AppError::not_found("This title has no cover"));
        };
        // A local title's cover is read from its folder each time; there is nothing to fetch or keep.
        if source == local::SOURCE {
            let local = self.local.clone();
            return Image::checked(local::blocking(move || local.cover(&key)).await?);
        }
        let name = format!("{manga}-{:08x}", fnv(&url));
        let kept = self.kept_cover(manga, &url);
        let cached = self.cache.join("covers").join(&name);
        if let Some(bytes) = read(kept.clone()).await {
            return Image::checked(bytes);
        }

        let _once = self.locks.lock(format!("cover:{name}")).await;
        let (bytes, fetched) = match read(cached.clone()).await {
            Some(bytes) => (bytes, false),
            None => {
                let opts = Call { source: Some(source), ..Call::default() };
                (self.host.bytes(&format!("/sources/{source}/image"), &json!({ "cover": url }), opts).await?, true)
            }
        };
        let mut image = Image::checked(bytes)?;
        if fetched || in_library {
            let bytes = std::mem::take(&mut image.bytes);
            image = Image::checked(tokio::task::spawn_blocking(move || shrink(bytes)).await.map_err(|_| AppError::internal("Could not read the cover"))?)?;
        }
        if in_library {
            write(kept, image.bytes.clone()).await;
        } else if fetched {
            self.stash(cached, image.bytes.clone()).await;
        }
        Ok(image)
    }

    /// Puts a file in the cache. One bigger than an eighth of the cap is only passed on,
    /// as it would push out most of the rest. A tenth of the cap put in since the last
    /// sweep calls the next one early, so a fast browse cannot run far past the cap.
    async fn stash(&self, path: PathBuf, bytes: Vec<u8>) {
        let size = bytes.len() as u64;
        if size > self.max_bytes / 8 {
            return;
        }
        write(path, bytes).await;
        if self.written.fetch_add(size, Ordering::Relaxed) + size > self.max_bytes / 10 {
            self.early.notify_one();
        }
    }

    /// Where a library title's cover is kept once it was shown.
    pub fn kept_cover(&self, manga: i64, url: &str) -> PathBuf {
        self.covers.join(format!("{manga}-{:08x}", fnv(url)))
    }

    /// How many pages a chapter has. Known chapters answer from the database; others ask the source.
    pub async fn page_count(&self, chapter: i64) -> Result<i64> {
        let known = self
            .db
            .call(move |conn| Ok(conn.query_row("SELECT page_count FROM chapters WHERE id = ?1", [chapter], |r| r.get::<_, Option<i64>>(0)).optional()?))
            .await?
            .ok_or_else(|| AppError::new(404, "unknown_chapter", "No such chapter"))?;
        if let Some(count) = known {
            return Ok(count);
        }
        let Kept::Local { title, chapter: name } = self.kept(chapter).await? else {
            return Ok(self.page_list(chapter, false).await?.1.len() as i64);
        };
        let local = self.local.clone();
        let count = local::blocking(move || local.page_count(&title, &name)).await?;
        self.db
            .call(move |conn| {
                conn.execute("UPDATE chapters SET page_count = ?2 WHERE id = ?1", params![chapter, count])?;
                Ok(())
            })
            .await?;
        Ok(count)
    }

    pub async fn page(&self, chapter: i64, index: i64) -> Result<Image> {
        if index < 0 {
            return Err(AppError::not_found("No such page"));
        }
        let file = self.cache.join("pages").join(chapter.to_string()).join(index.to_string());
        if let Some(bytes) = read(file.clone()).await {
            return Image::checked(bytes);
        }
        match self.kept(chapter).await? {
            // A downloaded chapter is read from its file. The source is never asked.
            Kept::File(saved) => {
                let bytes = tokio::task::spawn_blocking(move || read_cbz(&saved, index as usize)).await.ok().flatten();
                return Image::checked(bytes.ok_or_else(|| AppError::not_found("No such page in the downloaded chapter"))?);
            }
            Kept::Local { title, chapter: name } => {
                let local = self.local.clone();
                return Image::checked(local::blocking(move || local.page(&title, &name, index as usize)).await?);
            }
            Kept::Remote => {}
        }
        let _once = self.locks.lock(format!("page:{chapter}:{index}")).await;
        if let Some(bytes) = read(file.clone()).await {
            return Image::checked(bytes);
        }

        let mut retry = false;
        loop {
            let (source, pages, fresh) = self.page_list(chapter, retry).await?;
            let page = pages.get(index as usize).ok_or_else(|| AppError::not_found("No such page"))?;
            let opts = Call { source: Some(source), ..Call::default() };
            let fetched = self.host.bytes(&format!("/sources/{source}/image"), &json!({ "page": page }), opts).await.and_then(Image::checked);
            match fetched {
                Ok(image) => {
                    self.stash(file, image.bytes.clone()).await;
                    return Ok(image);
                }
                // The addresses in an old list may have run out; ask for new ones once.
                Err(_) if !fresh && !retry => retry = true,
                Err(e) => return Err(e),
            }
        }
    }

    /// Held while a chapter is packed for a reading app. A packed chapter sits in memory
    /// whole, so an account packs one at a time.
    pub async fn packing(&self) -> OwnedMutexGuard<()> {
        self.locks.lock("pack".into()).await
    }

    /// The downloaded chapter's file, when it has one.
    pub async fn saved(&self, chapter: i64) -> Result<Option<PathBuf>> {
        Ok(match self.kept(chapter).await? {
            Kept::File(file) => Some(file),
            _ => None,
        })
    }

    /// Where a chapter's pages are when they are not in the cache.
    async fn kept(&self, chapter: i64) -> Result<Kept> {
        let row: Option<(Option<String>, i64, String, String)> = self
            .db
            .call(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT c.download_path, m.source_id, m.url, c.url FROM chapters c JOIN manga m ON m.id = c.manga_id WHERE c.id = ?1",
                        [chapter],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                    )
                    .optional()?)
            })
            .await?;
        Ok(match row {
            Some((Some(path), ..)) => Kept::File(self.downloads.join(path)),
            Some((None, local::SOURCE, title, chapter)) => Kept::Local { title, chapter },
            _ => Kept::Remote,
        })
    }

    /// The pages of a chapter, with the source to ask for them. The flag tells whether the list was fetched just now.
    async fn page_list(&self, chapter: i64, renew: bool) -> Result<(i64, Arc<Vec<WirePage>>, bool)> {
        let usable = |list: &PageList| list.fetched.elapsed() < if renew { LIST_RETRY_AGE } else { LIST_TTL };
        let held = |this: &Self| {
            let lists = this.lists.lock().unwrap_or_else(|e| e.into_inner());
            lists.get(&chapter).filter(|list| usable(list)).map(|list| (list.source, list.pages.clone(), false))
        };
        if let Some(hit) = held(self) {
            return Ok(hit);
        }
        let _once = self.locks.lock(format!("list:{chapter}")).await;
        if let Some(hit) = held(self) {
            return Ok(hit);
        }

        #[derive(Deserialize)]
        struct Pages {
            pages: Vec<WirePage>,
        }
        let (source, wire) = self.db.call(move |conn| wire_chapter(conn, chapter)).await?;
        let opts = Call { source: Some(source), ..Call::default() };
        let got: Pages = self.host.call(Method::POST, &format!("/sources/{source}/pages"), Some(&json!({ "chapter": wire })), opts).await?;
        if got.pages.is_empty() {
            return Err(AppError::new(502, "source_error", "The source lists no pages for this chapter"));
        }
        let count = got.pages.len() as i64;
        self.db
            .call(move |conn| {
                conn.execute("UPDATE chapters SET page_count = ?2 WHERE id = ?1", params![chapter, count])?;
                Ok(())
            })
            .await?;
        let pages = Arc::new(got.pages);
        let mut lists = self.lists.lock().unwrap_or_else(|e| e.into_inner());
        lists.retain(|_, list| list.fetched.elapsed() < LIST_TTL);
        lists.insert(chapter, PageList { fetched: Instant::now(), source, pages: pages.clone() });
        Ok((source, pages, true))
    }

    /// Keeps the cache under its cap: every ten minutes, and sooner when much was put in. Never returns.
    pub async fn sweep_forever(self) {
        loop {
            self.written.store(0, Ordering::Relaxed);
            let (dir, max) = (self.cache.clone(), self.max_bytes);
            let _ = tokio::task::spawn_blocking(move || sweep(&dir, max)).await;
            tokio::select! {
                _ = tokio::time::sleep(SWEEP_EVERY) => {}
                _ = self.early.notified() => {}
            }
        }
    }
}

/// A big JPEG cover decoded at a fraction of its size and saved again. Anything else,
/// or anything that goes wrong, comes back as it was.
fn shrink(bytes: Vec<u8>) -> Vec<u8> {
    if bytes.len() < SHRINK_FROM || sniff(&bytes) != Some("image/jpeg") {
        return bytes;
    }
    let smaller = (|| {
        let _one = SHRINKING.lock().unwrap_or_else(|e| e.into_inner());
        let mut decoder = jpeg_decoder::Decoder::new(bytes.as_slice());
        decoder.read_info().ok()?;
        let info = decoder.info()?;
        if info.coding_process == jpeg_decoder::CodingProcess::DctProgressive
            && u32::from(info.width) * u32::from(info.height) > PROGRESSIVE_MAX_PIXELS
        {
            return None;
        }
        // The browser turns the picture by its EXIF orientation; a new file would lose it.
        if decoder.exif_data().is_some_and(|exif| turned(exif)) {
            return None;
        }
        let (width, height) = decoder.scale(COVER_BOX.0, COVER_BOX.1).ok()?;
        if (width, height) == (info.width, info.height) {
            return None;
        }
        let pixels = decoder.decode().ok()?;
        let color = match decoder.info()?.pixel_format {
            jpeg_decoder::PixelFormat::RGB24 => jpeg_encoder::ColorType::Rgb,
            jpeg_decoder::PixelFormat::L8 => jpeg_encoder::ColorType::Luma,
            _ => return None,
        };
        let mut out = Vec::new();
        jpeg_encoder::Encoder::new(&mut out, 85).encode(&pixels, width, height, color).ok()?;
        Some(out)
    })();
    match smaller {
        Some(out) if out.len() < bytes.len() => out,
        _ => bytes,
    }
}

/// Whether EXIF data says the picture is to be shown turned or mirrored.
fn turned(exif: &[u8]) -> bool {
    let tiff = exif.strip_prefix(b"Exif\0\0").unwrap_or(exif);
    let big = match tiff.get(..2) {
        Some(b"MM") => true,
        Some(b"II") => false,
        _ => return false,
    };
    let u16_at = |at: usize| tiff.get(at..at + 2).map(|b| if big { u16::from_be_bytes([b[0], b[1]]) } else { u16::from_le_bytes([b[0], b[1]]) });
    let u32_at = |at: usize| tiff.get(at..at + 4).map(|b| if big { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) } else { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) });
    let Some(ifd) = u32_at(4).map(|at| at as usize) else { return false };
    let count = u16_at(ifd).unwrap_or(0) as usize;
    (0..count).map(|i| ifd + 2 + i * 12).any(|entry| u16_at(entry) == Some(0x0112) && u16_at(entry + 8).is_some_and(|value| value > 1))
}

/// One page out of a CBZ. Pages are its entries in name order; ComicInfo.xml is not one.
fn read_cbz(path: &Path, index: usize) -> Option<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).ok()?).ok()?;
    let mut names: Vec<String> =
        archive.file_names().filter(|name| !name.ends_with('/') && !name.eq_ignore_ascii_case("ComicInfo.xml")).map(str::to_owned).collect();
    names.sort();
    let mut bytes = Vec::new();
    archive.by_name(names.get(index)?).ok()?.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// Reads a cached file and marks it as used now, which is what the sweep goes by.
async fn read(path: PathBuf) -> Option<Vec<u8>> {
    tokio::task::spawn_blocking(move || {
        let bytes = fs::read(&path).ok().filter(|bytes| !bytes.is_empty())?;
        if let Ok(file) = fs::OpenOptions::new().write(true).open(&path) {
            let _ = file.set_modified(SystemTime::now());
        }
        Some(bytes)
    })
    .await
    .ok()
    .flatten()
}

/// Writes a file whole or not at all. A failure only costs a refetch, so it is logged and dropped.
async fn write(path: PathBuf, bytes: Vec<u8>) {
    let outcome = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let part = path.with_extension(format!("part{:08x}", fnv(&format!("{:?}", Instant::now()))));
        fs::write(&part, bytes)?;
        fs::rename(&part, &path).or_else(|e| {
            let _ = fs::remove_file(&part);
            Err(e)
        })
    })
    .await;
    if let Ok(Err(e)) = outcome {
        tracing::warn!("could not cache an image: {e}");
    }
}

/// Deletes the least recently used files until the folder is a tenth under `max`.
fn sweep(dir: &Path, max: u64) -> u64 {
    fn walk(dir: &Path, files: &mut Vec<(SystemTime, u64, PathBuf)>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                walk(&entry.path(), files);
                let _ = fs::remove_dir(entry.path()); // only succeeds when it is empty
            } else {
                files.push((meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), meta.len(), entry.path()));
            }
        }
    }
    let mut files = Vec::new();
    walk(dir, &mut files);
    let mut total: u64 = files.iter().map(|(_, size, _)| size).sum();
    if total <= max {
        return total;
    }
    files.sort();
    let goal = max / 10 * 9;
    for (_, size, path) in files {
        if total <= goal {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total -= size;
        }
    }
    total
}

/// The image type of a file, by its first bytes. SVG is left out on purpose: it can carry script.
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => Some("image/jpeg"),
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [b'G', b'I', b'F', b'8', ..] => Some("image/gif"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("image/webp"),
        [_, _, _, _, b'f', b't', b'y', b'p', b'a', b'v', b'i', ..] => Some("image/avif"),
        [0xFF, 0x0A, ..] | [0, 0, 0, 0x0C, b'J', b'X', b'L', b' ', ..] => Some("image/jxl"),
        [b'B', b'M', ..] => Some("image/bmp"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_images_from_other_things() {
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0, 0]), Some("image/jpeg"));
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n"), Some("image/png"));
        assert_eq!(sniff(b"RIFF\x24\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff(b"\0\0\0\x1cftypavif"), Some("image/avif"));
        assert_eq!(sniff(b"<!doctype html><script>"), None);
        assert_eq!(sniff(b"<svg xmlns='http://www.w3.org/2000/svg'>"), None);
        assert!(Image::checked(b"<html>blocked</html>".to_vec()).is_err());
    }

    #[test]
    fn big_covers_come_back_smaller() {
        let (width, height) = (2400u16, 3600u16);
        let mut seed = 1u32;
        let pixels: Vec<u8> = (0..width as usize * height as usize * 3)
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                (seed >> 24) as u8
            })
            .collect();
        let mut big = Vec::new();
        jpeg_encoder::Encoder::new(&mut big, 95).encode(&pixels, width, height, jpeg_encoder::ColorType::Rgb).unwrap();
        assert!(big.len() > SHRINK_FROM);
        let small = shrink(big.clone());
        assert!(small.len() < big.len() / 4, "{} from {}", small.len(), big.len());
        let mut decoder = jpeg_decoder::Decoder::new(small.as_slice());
        decoder.read_info().unwrap();
        let info = decoder.info().unwrap();
        assert!((COVER_BOX.0..COVER_BOX.0 * 2).contains(&info.width) && (COVER_BOX.1..COVER_BOX.1 * 2).contains(&info.height));

        let png = [b"\x89PNG\r\n\x1a\n".as_slice(), &vec![0u8; SHRINK_FROM]].concat();
        assert_eq!(shrink(png.clone()), png, "only JPEGs are shrunk");
        assert_eq!(shrink(b"\xFF\xD8\xFFnot really".to_vec()), b"\xFF\xD8\xFFnot really", "a broken one is left alone");
    }

    #[test]
    fn reads_the_exif_orientation() {
        let exif = |big: bool, value: u16| {
            let (order, two, four): (&[u8], fn(u16) -> [u8; 2], fn(u32) -> [u8; 4]) =
                if big { (b"MM", u16::to_be_bytes, u32::to_be_bytes) } else { (b"II", u16::to_le_bytes, u32::to_le_bytes) };
            [b"Exif\0\0".as_slice(), order, &two(42), &four(8), &two(1), &two(0x0112), &two(3), &four(1), &two(value), &[0, 0]].concat()
        };
        assert!(turned(&exif(true, 6)) && turned(&exif(false, 3)));
        assert!(!turned(&exif(true, 1)) && !turned(&exif(false, 1)));
        assert!(!turned(b"Exif\0\0junk"));
    }

    #[test]
    fn sweep_drops_the_oldest_first() {
        let dir = std::env::temp_dir().join(format!("mango-sweep-{:08x}", fnv(&format!("{:?}", Instant::now()))));
        fs::create_dir_all(dir.join("pages/1")).unwrap();
        let old = SystemTime::now() - Duration::from_secs(3600);
        for (name, age) in [("pages/1/0", Some(old)), ("pages/1/1", None), ("cover", None)] {
            let path = dir.join(name);
            fs::write(&path, vec![0u8; 400]).unwrap();
            if let Some(time) = age {
                fs::OpenOptions::new().write(true).open(&path).unwrap().set_modified(time).unwrap();
            }
        }
        assert_eq!(sweep(&dir, 2000), 1200, "under the cap nothing goes");
        assert_eq!(sweep(&dir, 1000), 800);
        assert!(!dir.join("pages/1/0").exists() && dir.join("pages/1/1").exists() && dir.join("cover").exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
