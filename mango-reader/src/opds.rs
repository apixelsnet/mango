//! OPDS 1.2: the library as a catalog that reading apps browse and fetch from.
//!
//! Everything sits under `/opds`. An app signs in with an account's name and
//! password on every call, and sees that account's library. Addresses in the feeds
//! are relative, so a proxy may put Mango under a sub-path.

use crate::{
    api::{AppState, Me},
    downloads::extension,
    error::{AppError, Result},
    export::date,
    library::{self, LibraryItem},
    spaces::Spaces,
};
use axum::{
    extract::{ConnectInfo, OriginalUri, Path, Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use serde::Deserialize;
use std::{
    fmt::Write as _,
    io::{Cursor, Write as _},
    net::SocketAddr,
};

const NAVIGATION: &str = "application/atom+xml;profile=opds-catalog;kind=navigation";
const ACQUISITION: &str = "application/atom+xml;profile=opds-catalog;kind=acquisition";
const TITLES_PER_PAGE: usize = 100;
const CHAPTERS_PER_PAGE: usize = 200;
/// Cap on a chapter that is put together for one request and never saved.
const MAX_ARCHIVE: usize = 512 << 20;

pub fn routes(state: Spaces) -> Router {
    Router::new()
        .route("/", get(start))
        .route("/v1.2/catalog", get(catalog))
        .route("/v1.2/library", get(titles))
        .route("/v1.2/search.xml", get(search))
        .route("/v1.2/manga/{id}", get(title))
        .route("/v1.2/manga/{id}/cover", get(cover))
        .route("/v1.2/chapters/{id}/file", get(file))
        .route("/v1.2/chapters/{id}/pages/{index}", get(page))
        .layer(middleware::from_fn_with_state(state.clone(), sign_in))
        .with_state(state)
}

/// Lets a call through when it carries an account's name and password, or a browser's session.
async fn sign_in(State(server): State<Spaces>, ConnectInfo(peer): ConnectInfo<SocketAddr>, mut request: Request, next: Next) -> Response {
    let app = match server.auth.basic(server.auth.caller(peer.ip(), request.headers()), request.headers()).await {
        Ok(who) => server.of(who),
        Err(why) => Err(why),
    };
    match app {
        Ok(app) => {
            request.extensions_mut().insert(app);
            next.run(request).await
        }
        // The header is what makes an app ask its user for the name and password.
        Err(why) if why.status == StatusCode::UNAUTHORIZED => {
            (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Basic realm=\"Mango\", charset=\"UTF-8\"")], why.message).into_response()
        }
        Err(why) => why.into_response(),
    }
}

// ---------- feeds ----------

fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // XML has no way to carry the other control characters.
            c if c.is_control() && !matches!(c, '\n' | '\t') => {}
            c => out.push(c),
        }
    }
    out
}

/// A time in epoch seconds, as Atom writes it.
fn stamp(seconds: i64) -> String {
    let (year, month, day) = date(seconds);
    let of_day = seconds.rem_euclid(86_400);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", of_day / 3600, of_day / 60 % 60, of_day % 60)
}

struct Feed {
    xml: String,
    kind: &'static str,
}

impl Feed {
    /// `root` leads from the feed's own address back to the catalog's folder.
    fn new(kind: &'static str, id: &str, title: &str, root: &str, own: &str) -> Self {
        let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        let _ = write!(
            xml,
            "<feed xmlns=\"http://www.w3.org/2005/Atom\" xmlns:opds=\"http://opds-spec.org/2010/catalog\" xmlns:pse=\"http://vaemendis.net/opds-pse/ns\">\n\
             <id>urn:mango:{id}</id>\n<title>{}</title>\n<updated>{}</updated>\n<author><name>Mango</name></author>\n\
             <link rel=\"self\" type=\"{kind}\" href=\"{}\"/>\n\
             <link rel=\"start\" type=\"{NAVIGATION}\" href=\"{root}catalog\"/>\n\
             <link rel=\"search\" type=\"application/opensearchdescription+xml\" href=\"{root}search.xml\"/>\n",
            esc(title),
            stamp(crate::db::now()),
            esc(own),
        );
        Self { xml, kind }
    }

    fn link(&mut self, rel: &str, kind: &str, href: &str) {
        let _ = writeln!(self.xml, "<link rel=\"{rel}\" type=\"{kind}\" href=\"{}\"/>", esc(href));
    }

    /// One entry. `links` are written as given, inside it.
    fn entry(&mut self, id: &str, title: &str, updated: i64, about: &str, links: &str) {
        let _ = write!(
            self.xml,
            "<entry>\n<id>urn:mango:{id}</id>\n<title>{}</title>\n<updated>{}</updated>\n<content type=\"text\">{}</content>\n{links}</entry>\n",
            esc(title),
            stamp(updated),
            esc(about),
        );
    }

    fn done(mut self) -> Response {
        self.xml.push_str("</feed>\n");
        ([(header::CONTENT_TYPE, self.kind), (header::CACHE_CONTROL, "no-cache")], self.xml).into_response()
    }
}

/// `/opds` leads to the catalog, whether or not the app wrote the last slash.
async fn start(OriginalUri(uri): OriginalUri) -> Redirect {
    Redirect::temporary(if uri.path().ends_with('/') { "v1.2/catalog" } else { "opds/v1.2/catalog" })
}

/// Where to begin: the whole library, what is unread, and each category.
async fn catalog(Me(app): Me) -> Result<Response> {
    let categories = app.db.call(|conn| library::categories(conn)).await?;
    let mut feed = Feed::new(NAVIGATION, "catalog", &format!("Mango · {}", app.me.name), "", "catalog");
    let subsection = |href: &str| format!("<link rel=\"subsection\" type=\"{NAVIGATION}\" href=\"{}\"/>\n", esc(href));
    let now = crate::db::now();
    feed.entry("library", "Library", now, "Every title in the library", &subsection("library"));
    feed.entry("unread", "Unread", now, "Titles with chapters left to read", &subsection("library?unread=1"));
    for category in categories {
        feed.entry(&format!("category:{}", category.id), &category.name, now, "A category of the library", &subsection(&format!("library?category={}", category.id)));
    }
    Ok(feed.done())
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Wanted {
    category: Option<i64>,
    unread: Option<u8>,
    /// Text to find in titles.
    q: String,
    /// Counted from 1.
    page: Option<usize>,
}

impl Wanted {
    fn query(&self, page: usize) -> String {
        let mut parts = Vec::new();
        if let Some(category) = self.category {
            parts.push(format!("category={category}"));
        }
        if self.unread.is_some_and(|unread| unread > 0) {
            parts.push("unread=1".into());
        }
        if !self.q.is_empty() {
            parts.push(format!("q={}", percent(&self.q)));
        }
        if page > 1 {
            parts.push(format!("page={page}"));
        }
        if parts.is_empty() { String::new() } else { format!("?{}", parts.join("&")) }
    }
}

/// The titles of the library, or of a part of it, by name.
async fn titles(Me(app): Me, Query(wanted): Query<Wanted>) -> Result<Response> {
    let view = app.db.call(|conn| library::read(conn)).await?;
    let needle = wanted.q.trim().to_lowercase();
    let mut items: Vec<LibraryItem> = view
        .items
        .into_iter()
        .filter(|item| wanted.category.is_none_or(|id| item.categories.contains(&id)))
        .filter(|item| !wanted.unread.is_some_and(|unread| unread > 0) || item.unread > 0)
        .filter(|item| needle.is_empty() || item.title.to_lowercase().contains(&needle))
        .collect();
    items.sort_by_cached_key(|item| item.title.to_lowercase());

    let page = wanted.page.unwrap_or(1).max(1);
    let name = match wanted.category.and_then(|id| view.categories.iter().find(|category| category.id == id)) {
        Some(category) => category.name.clone(),
        None if !needle.is_empty() => format!("Search: {}", wanted.q.trim()),
        None if wanted.unread.is_some_and(|unread| unread > 0) => "Unread".into(),
        None => "Library".into(),
    };
    let mut feed = Feed::new(NAVIGATION, "library", &name, "", &format!("library{}", wanted.query(page)));
    if items.len() > page * TITLES_PER_PAGE {
        feed.link("next", NAVIGATION, &format!("library{}", wanted.query(page + 1)));
    }
    for item in items.iter().skip((page - 1) * TITLES_PER_PAGE).take(TITLES_PER_PAGE) {
        let mut links = format!("<link rel=\"subsection\" type=\"{ACQUISITION}\" href=\"manga/{}\"/>\n", item.id);
        if item.cover.is_some() {
            for (rel, size) in [("http://opds-spec.org/image", ""), ("http://opds-spec.org/image/thumbnail", "?size=small")] {
                let _ = writeln!(links, "<link rel=\"{rel}\" type=\"image/jpeg\" href=\"manga/{}/cover{size}\"/>", item.id);
            }
        }
        let about = format!("{} chapters, {} unread{}", item.total, item.unread, item.source_name.as_ref().map(|source| format!(" · {source}")).unwrap_or_default());
        feed.entry(&format!("manga:{}", item.id), &item.title, item.latest_at.max(item.added_at), &about, &links);
    }
    Ok(feed.done())
}

#[derive(Deserialize)]
struct Paged {
    page: Option<usize>,
}

/// A title's chapters, the first one first. Each can be fetched whole as a CBZ, and
/// page by page once Mango knows how many pages it has.
async fn title(Me(app): Me, Path(id): Path<i64>, Query(paged): Query<Paged>) -> Result<Response> {
    let title = app.titles.get(id).await?;
    let page = paged.page.unwrap_or(1).max(1);
    let own = |page: usize| if page > 1 { format!("{id}?page={page}") } else { id.to_string() };
    let mut feed = Feed::new(ACQUISITION, &format!("manga:{id}"), &title.title, "../", &own(page));
    feed.link("up", NAVIGATION, "../library");
    if title.chapters.len() > page * CHAPTERS_PER_PAGE {
        feed.link("next", ACQUISITION, &own(page + 1));
    }
    // The source lists the newest first; a reader wants the first chapter first.
    for chapter in title.chapters.iter().rev().skip((page - 1) * CHAPTERS_PER_PAGE).take(CHAPTERS_PER_PAGE) {
        let mut links = format!("<link rel=\"http://opds-spec.org/acquisition\" type=\"application/vnd.comicbook+zip\" href=\"../chapters/{}/file\"/>\n", chapter.id);
        if let Some(count) = chapter.page_count.filter(|count| *count > 0) {
            let last = if chapter.last_page_read > 0 { format!(" pse:lastRead=\"{}\"", (chapter.last_page_read + 1).min(count)) } else { String::new() };
            let _ = writeln!(
                links,
                "<link rel=\"http://vaemendis.net/opds-pse/stream\" type=\"image/jpeg\" href=\"../chapters/{}/pages/{{pageNumber}}\" pse:count=\"{count}\"{last}/>",
                chapter.id
            );
        }
        // A first page as the picture only where showing it asks no source for anything.
        // Shown in a list, it is not read.
        if chapter.downloaded {
            let _ = writeln!(links, "<link rel=\"http://opds-spec.org/image/thumbnail\" type=\"image/jpeg\" href=\"../chapters/{}/pages/0?look=1\"/>", chapter.id);
        }
        let mut about = String::from(if chapter.read { "Read" } else { "Unread" });
        if let Some(count) = chapter.page_count {
            let _ = write!(about, " · {count} pages");
        }
        if chapter.downloaded {
            about.push_str(" · downloaded");
        }
        if let Some(scanlator) = chapter.scanlator.as_deref().filter(|name| !name.is_empty()) {
            let _ = write!(about, " · {scanlator}");
        }
        let updated = if chapter.date_upload > 0 { chapter.date_upload / 1000 } else { chapter.fetched_at };
        feed.entry(&format!("chapter:{}", chapter.id), &chapter.name, updated, &about, &links);
    }
    Ok(feed.done())
}

/// Tells an app how to search: by sending the words to the library feed.
async fn search(headers: HeaderMap) -> Response {
    let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok()).unwrap_or("");
    // This one address has to be whole, so it is put together from what the proxy says.
    let scheme = if header("x-forwarded-proto").eq_ignore_ascii_case("https") { "https" } else { "http" };
    let prefix = header("x-forwarded-prefix").trim_end_matches('/');
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <OpenSearchDescription xmlns=\"http://a9.com/-/spec/opensearch/1.1/\">\n\
         <ShortName>Mango</ShortName>\n<Description>Search the library</Description>\n<InputEncoding>UTF-8</InputEncoding>\n\
         <Url type=\"{NAVIGATION}\" template=\"{scheme}://{}{}/opds/v1.2/library?q={{searchTerms}}\"/>\n\
         </OpenSearchDescription>\n",
        esc(header("host")),
        esc(prefix),
    );
    ([(header::CONTENT_TYPE, "application/opensearchdescription+xml")], xml).into_response()
}

// ---------- files ----------

fn picture(image: crate::images::Image) -> Response {
    let headers = [(header::CONTENT_TYPE, image.content_type), (header::CACHE_CONTROL, "private, max-age=86400"), (header::X_CONTENT_TYPE_OPTIONS, "nosniff")];
    (headers, image.bytes).into_response()
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Look {
    /// `small` for a cover's small copy.
    size: String,
    /// Set when a page is only shown as a chapter's picture.
    look: u8,
}

async fn cover(Me(app): Me, Path(id): Path<i64>, Query(look): Query<Look>) -> Result<Response> {
    Ok(picture(app.images.cover(id, look.size == "small").await?))
}

/// One page of a chapter, counted from 0. A page streamed is a page read: the chapter's
/// place moves on, and its last page marks it read, as the reader in the browser does.
async fn page(Me(app): Me, Path((id, index)): Path<(i64, i64)>, Query(look): Query<Look>) -> Result<Response> {
    let image = app.images.page(id, index).await?;
    if look.look == 0 && app.titles.streamed(id, index).await? {
        app.downloads.finished_reading(id).await;
        app.trackers.read_chapter(id);
    }
    Ok(picture(image))
}

/// For an address or a header: everything but letters, digits and a few marks as %XX.
fn percent(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

/// A chapter as a CBZ: the downloaded file when there is one, else its pages
/// fetched now and packed for this one request.
async fn file(Me(app): Me, Path(id): Path<i64>) -> Result<Response> {
    let view = app.titles.reader(id).await?;
    let bytes = match app.images.saved(id).await? {
        Some(saved) => tokio::task::spawn_blocking(move || std::fs::read(saved)).await.map_err(|e| AppError::internal(e.to_string()))??,
        None => {
            let _one_at_a_time = app.images.packing().await;
            pack(&app, id).await?
        }
    };
    let name = format!("{} - {}.cbz", view.title, view.chapter.name);
    let plain: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '.' | '_') { c } else { '_' }).collect();
    let headers = [
        (header::CONTENT_TYPE, "application/vnd.comicbook+zip".to_owned()),
        (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{plain}\"; filename*=UTF-8''{}", percent(&name))),
        (header::CACHE_CONTROL, "no-store".to_owned()),
    ];
    Ok((headers, bytes).into_response())
}

async fn pack(app: &AppState, chapter: i64) -> Result<Vec<u8>> {
    let total = app.images.page_count(chapter).await?;
    let stored = || zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let (mut next, mut size) = (0, 0);
    // Two pages at a time, as a download does.
    while next < total {
        let second = (next + 1 < total).then_some(next + 1);
        let (first, other) = tokio::join!(app.images.page(chapter, next), async {
            match second {
                Some(index) => Some(app.images.page(chapter, index).await),
                None => None,
            }
        });
        for page in [Some(first), other].into_iter().flatten() {
            let page = page?;
            size += page.bytes.len();
            if size > MAX_ARCHIVE {
                return Err(AppError::new(413, "too_large", "This chapter is too large to pack. Download it in Mango first."));
            }
            next += 1;
            archive.start_file(format!("{next:03}.{}", extension(page.content_type)), stored())?;
            archive.write_all(&page.bytes)?;
        }
    }
    Ok(archive.finish()?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_made_safe() {
        assert_eq!(esc("Tom & <Jerry> \"2\"\u{1}"), "Tom &amp; &lt;Jerry&gt; &quot;2&quot;");
        assert_eq!(percent("a b/ü.cbz"), "a%20b%2F%C3%BC.cbz");
        assert_eq!(stamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(stamp(1_760_104_245), "2025-10-10T13:50:45Z");
    }

    #[test]
    fn a_feed_says_where_it_is() {
        let mut feed = Feed::new(ACQUISITION, "manga:5", "A & B", "../", "5?page=2");
        feed.entry("chapter:9", "Ch. <1>", 0, "Unread", "<link rel=\"x\" href=\"y\"/>\n");
        let xml = feed.xml;
        assert!(xml.contains("<title>A &amp; B</title>") && xml.contains("href=\"../catalog\"") && xml.contains("href=\"5?page=2\""));
        assert!(xml.contains("<id>urn:mango:chapter:9</id>\n<title>Ch. &lt;1&gt;</title>\n<updated>1970-01-01T00:00:00Z</updated>"));
    }

    #[test]
    fn a_part_of_the_library_keeps_its_address() {
        let wanted = Wanted { category: Some(3), unread: Some(1), q: "one piece".into(), page: None };
        assert_eq!(wanted.query(1), "?category=3&unread=1&q=one%20piece");
        assert_eq!(Wanted::default().query(2), "?page=2");
        assert_eq!(Wanted::default().query(1), "");
    }
}
