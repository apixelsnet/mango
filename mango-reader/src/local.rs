//! The local source: titles the owner already has on disk, laid out as
//! `<root>/<title>/<chapter>`. A chapter is a `.cbz`, a `.zip`, or a folder of images.
//! A title with no chapter level is one chapter: a folder of images, or an archive
//! straight in the root. Such a chapter goes by `WHOLE` in place of a name on disk.
//!
//! Pure Rust: the extension host is never involved. The folder is only ever read.
//! Names that reach this module come out of the database, where they were put from a
//! directory listing; they are still checked before they touch the file system.

use crate::{
    db::now,
    error::{AppError, Result},
    titles::{WireChapter, WireManga},
};
use rusqlite::{params, Connection};
use serde_json::json;
use std::{
    cmp::Ordering,
    fs,
    io::Read,
    iter::Peekable,
    path::{Component, Path, PathBuf},
    str::Chars,
    time::UNIX_EPOCH,
};

/// The local source's id, the same one Mihon uses.
pub const SOURCE: i64 = 0;
/// The stand-in for an extension that the source hangs under.
pub const PKG: &str = "mango.local";
/// Titles per page of the source's list.
const PAGE: usize = 100;
/// The name of the one chapter of a title that has no chapter level: the title itself.
const WHOLE: &str = ".";
/// Cap on one image, against a broken or hostile file.
const MAX_IMAGE: u64 = 64 << 20;
const IMAGES: [&str; 8] = ["jpg", "jpeg", "png", "webp", "gif", "avif", "jxl", "bmp"];

#[derive(Clone)]
pub struct Local {
    root: PathBuf,
}

/// A chapter's pages: where they are and their names, in reading order.
enum Pages {
    Folder(PathBuf, Vec<String>),
    Archive(PathBuf, Vec<String>),
}

/// Runs file work off the async threads.
pub async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(work).await.map_err(|e| AppError::internal(e.to_string()))?
}

impl Local {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Lists the source beside the ones extensions provide, so the rest of Mango treats it like any other.
    pub fn register(conn: &Connection) -> Result<()> {
        conn.execute(
            "INSERT OR IGNORE INTO extensions (pkg, name, version_name, version_code, lib, content_warning, installed_at, updated_at)
             VALUES (?1, 'Local source', '1', 1, '', 0, ?2, ?2)",
            params![PKG, now()],
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO sources (id, pkg, name, lang, base_url, supports_latest) VALUES (?1, ?2, 'Local', 'other', NULL, 1)",
            params![SOURCE, PKG],
        )?;
        Ok(())
    }

    /// One page of titles: every folder in the root, by name, or the most recently changed first.
    pub fn browse(&self, latest: bool, query: &str, page: i64) -> Result<(Vec<WireManga>, bool)> {
        let needle = query.trim().to_lowercase();
        let mut titles: Vec<(String, i64)> = entries(&self.root)
            .into_iter()
            .filter(|(name, meta)| meta.is_dir() || (meta.is_file() && archive_stem(name).is_some()))
            .filter(|(name, _)| needle.is_empty() || name.to_lowercase().contains(&needle))
            .map(|(name, meta)| (name, modified(&meta)))
            .collect();
        if latest {
            titles.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| natural_cmp(&a.0, &b.0)));
        } else {
            titles.sort_by(|a, b| natural_cmp(&a.0, &b.0));
        }
        let from = (page.max(1) as usize - 1) * PAGE;
        let more = titles.len() > from + PAGE;
        let mangas = titles.into_iter().skip(from).take(PAGE).map(|(name, changed)| manga(&name, changed)).collect();
        Ok((mangas, more))
    }

    /// A title's details and chapters. Chapters are listed newest first, like a source does.
    pub fn details(&self, title: &str) -> Result<(WireManga, Vec<WireChapter>)> {
        let path = child(&self.root, title)?;
        let meta = fs::metadata(&path).map_err(|_| AppError::not_found("This title is no longer in the local folder"))?;
        let mut chapters = listed(&path, &meta, title);
        chapters.sort_by(|a, b| natural_cmp(&b.1, &a.1));

        let mut manga = manga(title, modified(&meta));
        manga.initialized = true;
        // Details come from a ComicInfo.xml in the title's folder, or else in its first chapter.
        let info = fs::read_to_string(path.join("ComicInfo.xml")).ok().or_else(|| {
            let (first, _, _) = chapters.last()?;
            comic_info(&within(&path, first))
        });
        if let Some(xml) = info {
            manga.description = tag(&xml, "Summary");
            manga.author = tag(&xml, "Writer");
            manga.artist = tag(&xml, "Penciller");
            manga.genre = tag(&xml, "Genre");
        }
        // A chapter's own ComicInfo.xml names and numbers it, where it says so.
        let chapters = chapters
            .into_iter()
            .map(|(url, shown, changed)| {
                let own = if url == WHOLE { None } else { comic_info(&path.join(&url)) };
                let info = |name: &str| own.as_deref().and_then(|xml| tag(xml, name));
                let number = info("Number").and_then(|number| number.parse::<f64>().ok()).filter(|number| number.is_finite() && *number >= 0.0);
                WireChapter {
                    name: info("Title").unwrap_or(shown),
                    chapter_number: number.unwrap_or(-1.0),
                    scanlator: info("Translator"),
                    date_upload: changed * 1000,
                    memo: json!({}),
                    url,
                }
            })
            .collect();
        Ok((manga, chapters))
    }

    /// `cover.jpg` or its kin in the title's folder, else the first page of the first chapter.
    pub fn cover(&self, title: &str) -> Result<Vec<u8>> {
        let dir = child(&self.root, title)?;
        let named = entries(&dir).into_iter().find(|(name, meta)| meta.is_file() && is_cover(name));
        if let Some((name, _)) = named {
            return read_image(&dir.join(name));
        }
        let meta = fs::metadata(&dir).map_err(|_| AppError::not_found("This title is no longer in the local folder"))?;
        let mut chapters = listed(&dir, &meta, title);
        chapters.sort_by(|a, b| natural_cmp(&a.1, &b.1));
        let (first, _, _) = chapters.first().ok_or_else(|| AppError::not_found("This title has no cover"))?;
        self.page(title, first, 0)
    }

    pub fn page_count(&self, title: &str, chapter: &str) -> Result<i64> {
        Ok(match self.pages(title, chapter)? {
            Pages::Folder(_, names) | Pages::Archive(_, names) => names.len() as i64,
        })
    }

    pub fn page(&self, title: &str, chapter: &str, index: usize) -> Result<Vec<u8>> {
        let missing = || AppError::not_found("No such page");
        match self.pages(title, chapter)? {
            Pages::Folder(dir, names) => read_image(&dir.join(names.get(index).ok_or_else(missing)?)),
            Pages::Archive(file, names) => {
                let mut archive = zip::ZipArchive::new(fs::File::open(file)?)?;
                let mut bytes = Vec::new();
                archive.by_name(names.get(index).ok_or_else(missing)?)?.take(MAX_IMAGE + 1).read_to_end(&mut bytes)?;
                if bytes.len() as u64 > MAX_IMAGE {
                    return Err(AppError::new(422, "too_large", "This page is larger than Mango reads"));
                }
                Ok(bytes)
            }
        }
    }

    fn pages(&self, title: &str, chapter: &str) -> Result<Pages> {
        let base = child(&self.root, title)?;
        // The title itself, when it is its own one chapter.
        let path = if chapter == WHOLE { base } else { child(&base, chapter)? };
        let gone = |_| AppError::not_found("This chapter is no longer in the local folder");
        if fs::metadata(&path).map_err(gone)?.is_dir() {
            let mut names: Vec<String> = entries(&path)
                .into_iter()
                .filter(|(name, meta)| meta.is_file() && is_image(name) && !(chapter == WHOLE && is_cover(name)))
                .map(|(name, _)| name)
                .collect();
            names.sort_by(|a, b| natural_cmp(a, b));
            return Ok(Pages::Folder(path, names));
        }
        // Pages are read out of the archive by name. Nothing is ever unpacked to disk,
        // so an entry's path cannot reach outside it.
        let archive = zip::ZipArchive::new(fs::File::open(&path)?)?;
        let mut names: Vec<String> = archive
            .file_names()
            .filter(|name| is_image(name) && !name.starts_with("__MACOSX/") && !name.rsplit('/').next().is_some_and(|file| file.starts_with('.')))
            .map(str::to_owned)
            .collect();
        names.sort_by(|a, b| natural_cmp(a, b));
        Ok(Pages::Archive(path, names))
    }
}

fn manga(name: &str, changed: i64) -> WireManga {
    WireManga {
        url: name.to_owned(),
        // An archive straight in the root is a title of its own, named without its extension.
        title: archive_stem(name).unwrap_or(name).to_owned(),
        artist: None,
        author: None,
        description: None,
        genre: None,
        status: 0,
        // Not an address: covers of local titles are read from the folder. It changes
        // when the folder does, which is what makes the browser ask again.
        thumbnail_url: Some(format!("local:{changed}")),
        update_strategy: "ALWAYS_UPDATE".into(),
        initialized: false,
        memo: json!({}),
    }
}

/// A path one name below `base`. Anything that is not a plain name is refused.
fn child(base: &Path, name: &str) -> Result<PathBuf> {
    let mut parts = Path::new(name).components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(_)), None) if !name.contains(['/', '\\']) => Ok(base.join(name)),
        _ => Err(AppError::bad_request("Not a name in the local folder")),
    }
}

/// What is in a folder, leaving out hidden names and whatever cannot be read.
fn entries(dir: &Path) -> Vec<(String, fs::Metadata)> {
    let Ok(listing) = fs::read_dir(dir) else { return Vec::new() };
    listing
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok().filter(|name| !name.starts_with('.'))?;
            // `metadata` follows links, so a linked folder counts as a folder.
            Some((name, fs::metadata(entry.path()).ok()?))
        })
        .collect()
}

/// A title's chapters as `(name on disk, name to show, changed)`. Without a chapter level,
/// the title is its own one chapter: an archive, or a folder with pages of its own.
fn listed(path: &Path, meta: &fs::Metadata, title: &str) -> Vec<(String, String, i64)> {
    let found = if meta.is_dir() { chapters(path) } else { Vec::new() };
    let pages = || entries(path).iter().any(|(name, meta)| meta.is_file() && is_image(name) && !is_cover(name));
    if found.is_empty() && (meta.is_file() || pages()) {
        let shown = archive_stem(title).filter(|_| meta.is_file()).unwrap_or(title);
        return vec![(WHOLE.to_owned(), shown.to_owned(), modified(meta))];
    }
    found
}

/// A chapter's path from its title's.
fn within(title: &Path, chapter: &str) -> PathBuf {
    if chapter == WHOLE {
        title.to_path_buf()
    } else {
        title.join(chapter)
    }
}

/// A title's chapters as `(name on disk, name to show, changed)`: archives and folders.
fn chapters(dir: &Path) -> Vec<(String, String, i64)> {
    entries(dir)
        .into_iter()
        .filter_map(|(name, meta)| {
            let shown = match archive_stem(&name) {
                Some(stem) if meta.is_file() => stem.to_owned(),
                None if meta.is_dir() => name.clone(),
                _ => return None,
            };
            Some((name, shown, modified(&meta)))
        })
        .collect()
}

fn archive_stem(name: &str) -> Option<&str> {
    let (stem, extension) = name.rsplit_once('.')?;
    (extension.eq_ignore_ascii_case("cbz") || extension.eq_ignore_ascii_case("zip")).then_some(stem)
}

/// `cover.jpg` and its kin: a title's cover, not a page.
fn is_cover(name: &str) -> bool {
    is_image(name) && name.rsplit_once('.').is_some_and(|(stem, _)| stem.eq_ignore_ascii_case("cover"))
}

fn is_image(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, extension)| IMAGES.iter().any(|known| extension.eq_ignore_ascii_case(known)))
}

fn modified(meta: &fs::Metadata) -> i64 {
    meta.modified().ok().and_then(|time| time.duration_since(UNIX_EPOCH).ok()).map_or(0, |since| since.as_secs() as i64)
}

fn read_image(path: &Path) -> Result<Vec<u8>> {
    if fs::metadata(path)?.len() > MAX_IMAGE {
        return Err(AppError::new(422, "too_large", "This image is larger than Mango reads"));
    }
    Ok(fs::read(path)?)
}

/// The ComicInfo.xml of a chapter, whether it is an archive or a folder.
fn comic_info(chapter: &Path) -> Option<String> {
    if chapter.is_dir() {
        return fs::read_to_string(chapter.join("ComicInfo.xml")).ok();
    }
    let mut archive = zip::ZipArchive::new(fs::File::open(chapter).ok()?).ok()?;
    let mut xml = String::new();
    archive.by_name("ComicInfo.xml").ok()?.take(1 << 20).read_to_string(&mut xml).ok()?;
    Some(xml)
}

/// The text of one element of a ComicInfo.xml. Enough for a flat file of plain elements.
fn tag(xml: &str, name: &str) -> Option<String> {
    let (open, close) = (format!("<{name}>"), format!("</{name}>"));
    let start = xml.find(&open)? + open.len();
    let end = start + xml[start..].find(&close)?;
    let text = xml[start..end].replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&");
    Some(text.trim().to_owned()).filter(|text| !text.is_empty())
}

/// Order as a person reads names: runs of digits by their value, so `2` comes before `10`.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    fn number(chars: &mut Peekable<Chars>) -> String {
        let mut digits = String::new();
        while let Some(digit) = chars.peek().copied().filter(char::is_ascii_digit) {
            digits.push(digit);
            chars.next();
        }
        digits
    }
    let (mut x, mut y) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (x.peek().copied(), y.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(p), Some(q)) if p.is_ascii_digit() && q.is_ascii_digit() => {
                let (m, n) = (number(&mut x), number(&mut y));
                let (m, n) = (m.trim_start_matches('0'), n.trim_start_matches('0'));
                let order = m.len().cmp(&n.len()).then_with(|| m.cmp(n));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(p), Some(q)) => {
                let order = p.to_lowercase().cmp(q.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                x.next();
                y.next();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

    fn archive(path: &Path, files: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for (name, data) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }

    /// A folder with two titles: one of archives with a cover and details, one with a chapter that is a folder.
    fn library() -> (PathBuf, Local) {
        let root = std::env::temp_dir().join(format!("mango-local-{:08x}", crate::titles::fnv(&format!("{:?}", std::time::Instant::now()))));
        fs::create_dir_all(root.join("Salt Wind/Chapter 2")).unwrap();
        fs::create_dir_all(root.join("alpha")).unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::write(root.join("stray.txt"), b"not a title").unwrap();

        let info = "<ComicInfo><Series>Ignored</Series><Summary>Tom &amp; the tide</Summary><Writer> A. Writer </Writer><Genre></Genre></ComicInfo>";
        archive(
            &root.join("Salt Wind/Chapter 1.cbz"),
            &[("10.png", b"\x89PNG ten"), ("2.png", b"\x89PNG two"), ("ComicInfo.xml", info.as_bytes()), ("__MACOSX/1.png", PNG), ("notes.txt", b"x")],
        );
        archive(&root.join("Salt Wind/Chapter 10.zip"), &[("a/001.jpg", b"\xFF\xD8\xFF first")]);
        fs::write(root.join("Salt Wind/Chapter 2/b.png"), b"\x89PNG b").unwrap();
        fs::write(root.join("Salt Wind/Chapter 2/a.PNG"), b"\x89PNG a").unwrap();
        fs::write(root.join("Salt Wind/Cover.png"), b"\x89PNG cover").unwrap();
        fs::write(root.join("Salt Wind/readme.txt"), b"x").unwrap();
        archive(&root.join("alpha/One.cbz"), &[("p.png", b"\x89PNG alpha")]);
        (root.clone(), Local::new(root))
    }

    #[test]
    fn reads_a_folder_of_titles() {
        let (root, local) = library();

        let (titles, more) = local.browse(false, "", 1).unwrap();
        assert_eq!(titles.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(), ["alpha", "Salt Wind"], "folders only, by name");
        assert!(!more);
        assert_eq!(local.browse(false, "WIND", 1).unwrap().0.len(), 1);
        assert!(local.browse(false, "", 2).unwrap().0.is_empty());

        let (manga, chapters) = local.details("Salt Wind").unwrap();
        assert_eq!(chapters.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Chapter 10", "Chapter 2", "Chapter 1"], "newest first, 2 before 10");
        assert_eq!(chapters[0].url, "Chapter 10.zip");
        assert_eq!((manga.description.as_deref(), manga.author.as_deref(), manga.genre), (Some("Tom & the tide"), Some("A. Writer"), None));
        assert!(manga.initialized && manga.thumbnail_url.unwrap().starts_with("local:"));

        // Pages in reading order, whatever the archive's own order; junk is left out.
        assert_eq!(local.page_count("Salt Wind", "Chapter 1.cbz").unwrap(), 2);
        assert_eq!(local.page("Salt Wind", "Chapter 1.cbz", 0).unwrap(), b"\x89PNG two");
        assert_eq!(local.page("Salt Wind", "Chapter 1.cbz", 1).unwrap(), b"\x89PNG ten");
        assert_eq!(local.page("Salt Wind", "Chapter 2", 0).unwrap(), b"\x89PNG a");
        assert_eq!(local.page("Salt Wind", "Chapter 10.zip", 0).unwrap(), b"\xFF\xD8\xFF first");
        assert_eq!(local.page("Salt Wind", "Chapter 2", 9).unwrap_err().status, 404);

        assert_eq!(local.cover("Salt Wind").unwrap(), b"\x89PNG cover", "a cover file wins");
        assert_eq!(local.cover("alpha").unwrap(), b"\x89PNG alpha", "else the first page");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stays_inside_its_folder() {
        let (root, local) = library();
        fs::write(root.parent().unwrap().join("mango-outside.png"), PNG).unwrap();
        for name in ["..", "../alpha", "Salt Wind/Chapter 2", "/etc", "", "a\\b"] {
            assert_eq!(local.details(name).unwrap_err().status, 400, "{name:?}");
            assert_eq!(local.page("alpha", name, 0).unwrap_err().status, 400, "{name:?}");
        }
        assert_eq!(local.details("No such title").unwrap_err().status, 404);
        assert_eq!(local.page("alpha", "Gone.cbz", 0).unwrap_err().status, 404);
        fs::remove_file(root.parent().unwrap().join("mango-outside.png")).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn chapters_name_themselves_and_titles_need_no_chapters() {
        let (root, local) = library();
        let info = "<ComicInfo><Title>The Return</Title><Number>12.5</Number><Translator>Tide Scans</Translator></ComicInfo>";
        archive(&root.join("Salt Wind/c012.cbz"), &[("1.png", b"\x89PNG one"), ("ComicInfo.xml", info.as_bytes())]);
        archive(&root.join("Salt Wind/c013.cbz"), &[("1.png", b"\x89PNG one"), ("ComicInfo.xml", b"<ComicInfo><Number>soon</Number></ComicInfo>")]);
        let (_, chapters) = local.details("Salt Wind").unwrap();
        let named = chapters.iter().find(|c| c.url == "c012.cbz").unwrap();
        assert_eq!((named.name.as_str(), named.chapter_number, named.scanlator.as_deref()), ("The Return", 12.5, Some("Tide Scans")));
        let bare = chapters.iter().find(|c| c.url == "c013.cbz").unwrap();
        assert_eq!((bare.name.as_str(), bare.chapter_number), ("c013", -1.0), "what it does not say comes from the file");

        // Pages straight in a title's folder, and an archive straight in the root.
        fs::create_dir_all(root.join("One Shot")).unwrap();
        fs::write(root.join("One Shot/2.png"), b"\x89PNG second").unwrap();
        fs::write(root.join("One Shot/1.png"), b"\x89PNG first").unwrap();
        fs::write(root.join("One Shot/cover.png"), b"\x89PNG cover").unwrap();
        archive(&root.join("Loose.cbz"), &[("a.png", b"\x89PNG loose"), ("ComicInfo.xml", b"<ComicInfo><Writer>W</Writer></ComicInfo>")]);
        let titles = local.browse(false, "", 1).unwrap().0;
        assert_eq!(titles.iter().map(|t| (t.url.as_str(), t.title.as_str())).collect::<Vec<_>>(), [("alpha", "alpha"), ("Loose.cbz", "Loose"), ("One Shot", "One Shot"), ("Salt Wind", "Salt Wind")]);

        let (_, chapters) = local.details("One Shot").unwrap();
        assert_eq!(chapters.iter().map(|c| (c.url.as_str(), c.name.as_str())).collect::<Vec<_>>(), [(WHOLE, "One Shot")]);
        assert_eq!(local.page_count("One Shot", WHOLE).unwrap(), 2, "the cover is not a page");
        assert_eq!(local.page("One Shot", WHOLE, 0).unwrap(), b"\x89PNG first");

        let (manga, chapters) = local.details("Loose.cbz").unwrap();
        assert_eq!((manga.title.as_str(), manga.author.as_deref()), ("Loose", Some("W")));
        assert_eq!(chapters.iter().map(|c| (c.url.as_str(), c.name.as_str())).collect::<Vec<_>>(), [(WHOLE, "Loose")]);
        assert_eq!(local.page("Loose.cbz", WHOLE, 0).unwrap(), b"\x89PNG loose");
        assert_eq!(local.cover("Loose.cbz").unwrap(), b"\x89PNG loose");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn natural_order() {
        let mut names = vec!["Chapter 10", "chapter 2", "Chapter 1", "Chapter 1.5", "Chapter 02", "Vol 3 Ch 1", "Vol 12 Ch 1"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["Chapter 1", "Chapter 1.5", "Chapter 02", "chapter 2", "Chapter 10", "Vol 3 Ch 1", "Vol 12 Ch 1"]);
        assert_eq!(tag("<a><Title>  x &lt; y </Title></a>", "Title").as_deref(), Some("x < y"));
        assert_eq!(tag("<a><Title/></a>", "Title"), None);
    }
}
