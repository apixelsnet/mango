//! "Export my files": every chapter the account downloaded, as one zip laid out as the
//! local source of Mihon, Suwayomi and Mango reads it: `<title>/<chapter>.cbz`, with
//! the title's `ComicInfo.xml` and cover beside them. Unzipped into their local folder,
//! the titles are there to read.
//!
//! The zip is written as it goes out, so a library of many gigabytes takes no room on
//! the disk and little in memory. The `zip` crate wants to seek in what it writes, so
//! the format is written here by hand: stored entries only, since a CBZ is packed
//! already, and ZIP64 records once the archive passes 4 GB.

use crate::{
    db::Db,
    downloads::{clean, escape, extension},
    error::Result,
    export::date,
    images::{sniff, Images},
};
use axum::body::Body;
use serde::Serialize;
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
};
use tokio::sync::mpsc;

/// Bytes sent at a time, and read from a file at a time.
const CHUNK: usize = 256 << 10;
/// Names are UTF-8: bit 11 of the flags.
const UTF8: u16 = 1 << 11;
const MAX32: u64 = u32::MAX as u64;
/// Mihon's words for a title's status, by Mango's number for it.
const STATUS: [&str; 7] = ["Unknown", "Ongoing", "Completed", "Licensed", "Publishing finished", "Cancelled", "On hiatus"];

/// One title's folder in the zip.
pub struct Title {
    folder: String,
    info: String,
    cover: Option<PathBuf>,
    chapters: Vec<(PathBuf, u64)>,
}

#[derive(Serialize)]
pub struct Size {
    titles: usize,
    chapters: usize,
    bytes: u64,
}

pub fn size(titles: &[Title]) -> Size {
    Size {
        titles: titles.len(),
        chapters: titles.iter().map(|t| t.chapters.len()).sum(),
        bytes: titles.iter().flat_map(|t| &t.chapters).map(|(_, bytes)| bytes).sum(),
    }
}

/// What an export holds: each title with a downloaded chapter, and its files.
pub async fn plan(db: &Db, images: &Images, downloads: &Path) -> Result<Vec<Title>> {
    type Row = (i64, String, [Option<String>; 4], i64, Option<String>, Option<String>, String);
    let rows: Vec<Row> = db
        .call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT m.id, m.title, m.author, m.artist, m.description, m.genre, m.status, m.thumbnail_url, s.name, c.download_path
                 FROM chapters c JOIN manga m ON m.id = c.manga_id LEFT JOIN sources s ON s.id = m.source_id
                 WHERE c.download_path IS NOT NULL ORDER BY m.title COLLATE NOCASE, m.id, c.source_order DESC",
            )?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, [r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?], r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?)))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(rows)
        })
        .await?;

    let mut titles: Vec<Title> = Vec::new();
    let mut last = None;
    // Two titles of one name, from two sources, get a folder each.
    let mut taken = HashSet::new();
    for (id, name, [author, artist, summary, genre], status, cover, source, path) in rows {
        // The path is Mango's own, but it is still kept inside the downloads folder.
        let relative = Path::new(&path);
        if !relative.components().all(|part| matches!(part, Component::Normal(_))) {
            continue;
        }
        let file = downloads.join(relative);
        let Ok(meta) = fs::metadata(&file) else { continue };
        if last != Some(id) {
            last = Some(id);
            // The source's name tells them apart; a number does where it is not known.
            let mut folder = clean(&name);
            if let Some(source) = source.filter(|_| taken.contains(&folder.to_lowercase())) {
                folder = clean(&format!("{name} ({source})"));
            }
            let mut n = 1;
            while taken.contains(&folder.to_lowercase()) {
                n += 1;
                folder = format!("{} ({n})", clean(&name));
            }
            taken.insert(folder.to_lowercase());
            let info = comic_info(&name, [author.as_deref(), artist.as_deref(), summary.as_deref(), genre.as_deref()], status);
            let cover = cover.map(|url| images.kept_cover(id, &url)).filter(|kept| kept.is_file());
            titles.push(Title { folder, info, cover, chapters: Vec::new() });
        }
        if let Some(title) = titles.last_mut() {
            title.chapters.push((file, meta.len()));
        }
    }
    Ok(titles)
}

/// The title's own `ComicInfo.xml`, as Mihon's local source writes and reads it.
fn comic_info(title: &str, [author, artist, summary, genre]: [Option<&str>; 4], status: i64) -> String {
    let mut xml = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<ComicInfo xmlns:ty=\"http://www.w3.org/2001/XMLSchema\">\n");
    let mut field = |tag: &str, value: Option<&str>| {
        if let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) {
            xml.push_str(&format!("  <{tag}>{}</{tag}>\n", escape(value)));
        }
    };
    field("Series", Some(title));
    field("Summary", summary);
    field("Writer", author);
    field("Penciller", artist);
    field("Genre", genre);
    field("ty:PublishingStatusTachiyomi", usize::try_from(status).ok().and_then(|i| STATUS.get(i)).copied());
    xml.push_str("</ComicInfo>\n");
    xml
}

pub fn file_name() -> String {
    let (year, month, day) = date(crate::db::now());
    format!("mango_files_{year:04}-{month:02}-{day:02}.zip")
}

/// The zip as a body that is written while it is sent. A caller that goes away stops it.
pub fn send(titles: Vec<Title>) -> Body {
    let (to, from) = mpsc::channel(4);
    tokio::task::spawn_blocking(move || {
        let failed = to.clone();
        if let Err(e) = write(&titles, Pipe { to, held: Vec::with_capacity(CHUNK) }) {
            if e.kind() != io::ErrorKind::BrokenPipe {
                tracing::warn!("an export of files stopped: {e}");
                let _ = failed.blocking_send(Err(e));
            }
        }
    });
    Body::from_stream(Chunks(from))
}

pub fn write(titles: &[Title], out: impl Write) -> io::Result<()> {
    let mut zip = Zip::new(out, crate::db::now());
    for title in titles {
        zip.add_bytes(&format!("{}/ComicInfo.xml", title.folder), title.info.as_bytes())?;
        if let Some(cover) = title.cover.as_ref().and_then(|kept| fs::read(kept).ok()) {
            if let Some(kind) = sniff(&cover) {
                zip.add_bytes(&format!("{}/cover.{}", title.folder, extension(kind)), &cover)?;
            }
        }
        for (path, _) in &title.chapters {
            // A chapter deleted since the plan was made is left out.
            let Ok(mut file) = File::open(path) else { continue };
            // Cleaned again: the zip may be opened on a system stricter than this one.
            let name = clean(&path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default());
            zip.add_file(&format!("{}/{name}", title.folder), &mut file)?;
        }
    }
    zip.finish()?.flush()
}

struct Entry {
    name: String,
    crc: u32,
    size: u64,
    offset: u64,
}

/// A zip of stored entries, written front to back.
struct Zip<W: Write> {
    out: W,
    at: u64,
    time: u16,
    day: u16,
    entries: Vec<Entry>,
}

impl<W: Write> Zip<W> {
    fn new(out: W, now: i64) -> Self {
        let (year, month, day) = date(now);
        let seconds = now.rem_euclid(86_400);
        Self {
            out,
            at: 0,
            time: ((seconds / 3600) << 11 | (seconds / 60 % 60) << 5 | (seconds % 60) / 2) as u16,
            day: ((year - 1980).max(0) << 9 | month << 5 | day) as u16,
            entries: Vec::new(),
        }
    }

    fn put(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.out.write_all(bytes)?;
        self.at += bytes.len() as u64;
        Ok(())
    }

    fn add_bytes(&mut self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let mut crc = flate2::Crc::new();
        crc.update(bytes);
        self.head(name, crc.sum(), bytes.len() as u64)?;
        self.put(bytes)
    }

    /// The file is read twice: once for its checksum, which goes ahead of it, then to send.
    fn add_file(&mut self, name: &str, file: &mut File) -> io::Result<()> {
        let size = file.metadata()?.len();
        let mut buf = vec![0; CHUNK];
        let mut crc = flate2::Crc::new();
        let mut read = 0;
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            crc.update(&buf[..n]);
            read += n as u64;
        }
        if read != size {
            return Err(io::Error::other(format!("{name} changed while it was read")));
        }
        file.seek(SeekFrom::Start(0))?;
        self.head(name, crc.sum(), size)?;
        let mut left = size;
        while left > 0 {
            let n = file.read(&mut buf[..CHUNK.min(left as usize)])?;
            if n == 0 {
                return Err(io::Error::other(format!("{name} got shorter while it was sent")));
            }
            self.put(&buf[..n])?;
            left -= n as u64;
        }
        Ok(())
    }

    fn head(&mut self, name: &str, crc: u32, size: u64) -> io::Result<()> {
        // A chapter is never near this; a file that is would need ZIP64 sizes as well.
        if size >= MAX32 {
            return Err(io::Error::other(format!("{name} is 4 GB or more")));
        }
        let mut head = Vec::with_capacity(30 + name.len());
        head.extend(0x0403_4b50u32.to_le_bytes());
        head.extend(20u16.to_le_bytes());
        head.extend(UTF8.to_le_bytes());
        head.extend(0u16.to_le_bytes());
        head.extend(self.time.to_le_bytes());
        head.extend(self.day.to_le_bytes());
        head.extend(crc.to_le_bytes());
        head.extend((size as u32).to_le_bytes());
        head.extend((size as u32).to_le_bytes());
        head.extend((name.len() as u16).to_le_bytes());
        head.extend(0u16.to_le_bytes());
        head.extend(name.as_bytes());
        self.entries.push(Entry { name: name.to_owned(), crc, size, offset: self.at });
        self.put(&head)
    }

    /// The central directory, with ZIP64 records where an offset or a count outgrows 32 bits.
    fn finish(mut self) -> io::Result<W> {
        let start = self.at;
        let entries = std::mem::take(&mut self.entries);
        for entry in &entries {
            let far = entry.offset >= MAX32;
            let mut head = Vec::with_capacity(58 + entry.name.len());
            head.extend(0x0201_4b50u32.to_le_bytes());
            head.extend(45u16.to_le_bytes());
            head.extend((if far { 45u16 } else { 20 }).to_le_bytes());
            head.extend(UTF8.to_le_bytes());
            head.extend(0u16.to_le_bytes());
            head.extend(self.time.to_le_bytes());
            head.extend(self.day.to_le_bytes());
            head.extend(entry.crc.to_le_bytes());
            head.extend((entry.size as u32).to_le_bytes());
            head.extend((entry.size as u32).to_le_bytes());
            head.extend((entry.name.len() as u16).to_le_bytes());
            head.extend((if far { 12u16 } else { 0 }).to_le_bytes());
            // Comment length, disk, internal and external attributes.
            head.extend([0u8; 10]);
            head.extend((entry.offset.min(MAX32) as u32).to_le_bytes());
            head.extend(entry.name.as_bytes());
            if far {
                head.extend(1u16.to_le_bytes());
                head.extend(8u16.to_le_bytes());
                head.extend(entry.offset.to_le_bytes());
            }
            self.put(&head)?;
        }
        let count = entries.len() as u64;
        let size = self.at - start;
        if count >= 0xFFFF || start >= MAX32 || size >= MAX32 {
            let record = self.at;
            let mut z = Vec::with_capacity(76);
            z.extend(0x0606_4b50u32.to_le_bytes());
            z.extend(44u64.to_le_bytes());
            z.extend(45u16.to_le_bytes());
            z.extend(45u16.to_le_bytes());
            z.extend([0u8; 8]);
            z.extend(count.to_le_bytes());
            z.extend(count.to_le_bytes());
            z.extend(size.to_le_bytes());
            z.extend(start.to_le_bytes());
            z.extend(0x0706_4b50u32.to_le_bytes());
            z.extend(0u32.to_le_bytes());
            z.extend(record.to_le_bytes());
            z.extend(1u32.to_le_bytes());
            self.put(&z)?;
        }
        let mut end = Vec::with_capacity(22);
        end.extend(0x0605_4b50u32.to_le_bytes());
        end.extend([0u8; 4]);
        end.extend((count.min(0xFFFF) as u16).to_le_bytes());
        end.extend((count.min(0xFFFF) as u16).to_le_bytes());
        end.extend((size.min(MAX32) as u32).to_le_bytes());
        end.extend((start.min(MAX32) as u32).to_le_bytes());
        end.extend(0u16.to_le_bytes());
        self.put(&end)?;
        Ok(self.out)
    }
}

/// Hands what the zip writer makes to the body, a chunk at a time.
struct Pipe {
    to: mpsc::Sender<io::Result<Vec<u8>>>,
    held: Vec<u8>,
}

impl Write for Pipe {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.held.extend_from_slice(bytes);
        if self.held.len() >= CHUNK {
            self.flush()?;
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.held.is_empty() {
            return Ok(());
        }
        let chunk = std::mem::replace(&mut self.held, Vec::with_capacity(CHUNK));
        self.to.blocking_send(Ok(chunk)).map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "the caller went away"))
    }
}

struct Chunks(mpsc::Receiver<io::Result<Vec<u8>>>);

impl futures_core::Stream for Chunks {
    type Item = io::Result<Vec<u8>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.poll_recv(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn title(dir: &Path, folder: &str, chapters: &[(&str, &[u8])]) -> Title {
        let chapters = chapters
            .iter()
            .map(|(name, bytes)| {
                let path = dir.join(name);
                fs::write(&path, bytes).unwrap();
                (path, bytes.len() as u64)
            })
            .collect();
        Title { folder: folder.into(), info: comic_info(folder, [Some("A & B"), None, None, Some("Action")], 2), cover: None, chapters }
    }

    #[test]
    fn the_zip_reads_back() {
        let dir = std::env::temp_dir().join(format!("mango-files-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let cover = dir.join("cover");
        fs::write(&cover, [0x89, b'P', b'N', b'G', 1, 2, 3]).unwrap();
        let mut first = title(&dir, "Salt Wind", &[("Chapter 1.cbz", b"one"), ("Chapter 2.cbz", &[7; 300_000])]);
        first.cover = Some(cover);
        let second = title(&dir, "Ünïcode ✓", &[("Ch 1.cbz", b"")]);
        let mut out = Vec::new();
        write(&[first, second], &mut out).unwrap();

        let mut zip = zip::ZipArchive::new(Cursor::new(out)).unwrap();
        let names: Vec<String> = (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().to_owned()).collect();
        assert_eq!(
            names,
            ["Salt Wind/ComicInfo.xml", "Salt Wind/cover.png", "Salt Wind/Chapter 1.cbz", "Salt Wind/Chapter 2.cbz", "Ünïcode ✓/ComicInfo.xml", "Ünïcode ✓/Ch 1.cbz"]
        );
        let mut read = |name: &str| {
            let mut bytes = Vec::new();
            zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
            bytes
        };
        assert_eq!(read("Salt Wind/Chapter 2.cbz"), vec![7; 300_000]);
        assert_eq!(read("Salt Wind/Chapter 1.cbz"), b"one");
        let info = String::from_utf8(read("Salt Wind/ComicInfo.xml")).unwrap();
        assert!(info.contains("<Series>Salt Wind</Series>") && info.contains("<Writer>A &amp; B</Writer>"), "{info}");
        assert!(info.contains("<ty:PublishingStatusTachiyomi>Completed</ty:PublishingStatusTachiyomi>"), "{info}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn far_offsets_take_zip64() {
        // The writer is told it is far along already; what matters is what it writes after.
        let mut zip = Zip::new(Vec::new(), 0);
        zip.at = MAX32 + 10;
        zip.add_bytes("a", b"x").unwrap();
        let out = zip.finish().unwrap();
        let central = out.windows(4).position(|w| w == 0x0201_4b50u32.to_le_bytes()).unwrap();
        assert_eq!(u32::from_le_bytes(out[central + 42..central + 46].try_into().unwrap()), u32::MAX);
        assert_eq!(u64::from_le_bytes(out[central + 51..central + 59].try_into().unwrap()), MAX32 + 10);
        assert!(out.windows(4).any(|w| w == 0x0606_4b50u32.to_le_bytes()) && out.windows(4).any(|w| w == 0x0706_4b50u32.to_le_bytes()));
    }

    #[test]
    fn dos_time() {
        // 2024-03-05 14:30:20 UTC
        let zip = Zip::new(Vec::new(), 1_709_649_020);
        assert_eq!(zip.day, (44 << 9 | 3 << 5 | 5) as u16);
        assert_eq!(zip.time, (14 << 11 | 30 << 5 | 10) as u16);
    }
}
