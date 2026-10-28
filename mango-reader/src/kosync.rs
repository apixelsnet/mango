//! KOReader's progress sync, under `/opds/kosync`. KOReader takes that address as its
//! own sync server and signs in with the account's name and an app password.
//!
//! KOReader names a document by a mark made from its file's bytes, or, when its user
//! chose so, from the file's name. Mango notes both marks for every chapter file it
//! hands out, so a place reported for one of them moves that chapter on, and a place
//! read in Mango is offered back. A document Mango did not hand out is kept as KOReader
//! reports it, so that KOReader's own devices still keep each other's place.

use crate::{
    api::Me,
    db::{self, Db},
    error::{AppError, Result},
    spaces::Spaces,
};
use axum::{
    extract::{ConnectInfo, Path, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use md5::{Digest, Md5};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;

/// What a place is reported from, when it is Mango's own.
const DEVICE: &str = "Mango";
const DEVICE_ID: &str = "mango";
const MAX_DOCUMENT: usize = 128;
const MAX_PROGRESS: usize = 1024;

pub fn routes(state: Spaces) -> Router {
    let signed = Router::new()
        .route("/users/auth", get(signed_in))
        .route("/syncs/progress", put(save))
        .route("/syncs/progress/{document}", get(load))
        .layer(middleware::from_fn_with_state(state.clone(), sign_in));
    Router::new().route("/healthcheck", get(healthy)).route("/users/create", post(no_signup)).merge(signed).with_state(state)
}

/// An error as KOReader reads one: a number and a message.
fn refuse(status: StatusCode, code: u32, message: &str) -> Response {
    (status, Json(json!({ "code": code, "message": message }))).into_response()
}

/// Lets a call through when its name and key are an account's name and the MD5 of one of its app passwords.
async fn sign_in(State(server): State<Spaces>, ConnectInfo(peer): ConnectInfo<SocketAddr>, mut request: Request, next: Next) -> Response {
    // In a block of its own: a reference to the request may not be held across the wait below.
    let (name, key) = {
        let header = |name: &str| request.headers().get(name).and_then(|value| value.to_str().ok()).unwrap_or("").to_owned();
        (header("x-auth-user"), header("x-auth-key"))
    };
    let from = server.auth.caller(peer.ip(), request.headers());
    let app = match server.auth.kosync(from, &name, &key).await {
        Ok(who) => server.of(who),
        Err(why) => Err(why),
    };
    match app {
        Ok(app) => {
            request.extensions_mut().insert(app);
            next.run(request).await
        }
        Err(why) if why.status == StatusCode::UNAUTHORIZED => refuse(why.status, 2001, "Unauthorized"),
        Err(why) => refuse(why.status, 2000, &why.message),
    }
}

async fn healthy() -> Json<Value> {
    Json(json!({ "state": "OK" }))
}

/// Accounts are made in Mango, not from KOReader.
async fn no_signup() -> Response {
    refuse(StatusCode::PAYMENT_REQUIRED, 2005, "Make an app password in Mango, under Settings › Account, and sign in with your name and it.")
}

async fn signed_in() -> Json<Value> {
    Json(json!({ "authorized": "OK" }))
}

#[derive(Deserialize)]
struct Report {
    document: String,
    progress: Value,
    percentage: f64,
    #[serde(default)]
    device: String,
    #[serde(default)]
    device_id: String,
}

fn check_document(document: &str) -> Result<()> {
    if document.is_empty() || document.len() > MAX_DOCUMENT || !document.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(AppError::bad_request("Not a document's mark"));
    }
    Ok(())
}

/// Keeps a place KOReader reports, and moves the chapter on when the document is one of Mango's.
async fn save(Me(app): Me, Json(report): Json<Report>) -> Result<Response> {
    check_document(&report.document)?;
    let progress = match report.progress {
        Value::String(text) => text,
        other => other.to_string(),
    };
    if progress.len() > MAX_PROGRESS || !report.percentage.is_finite() {
        return Err(AppError::bad_request("Not a place in a document"));
    }
    let document = report.document.clone();
    let found = app
        .db
        .call(move |conn| {
            let find = "SELECT c.id, c.page_count FROM sync_documents d JOIN chapters c ON c.id = d.chapter_id WHERE d.document = ?1";
            Ok(conn.query_row(find, [document], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?))).optional()?)
        })
        .await?;
    // A paged document's place is its page number, counted from 1.
    if let (Some((chapter, count)), Ok(page)) = (found, progress.trim().parse::<f64>()) {
        let page = page.max(1.0) as i64;
        let finished = report.percentage >= 0.999 || count.is_some_and(|count| count > 0 && page >= count);
        app.titles.progress(chapter, page - 1, finished.then_some(true)).await?;
        if finished {
            app.downloads.finished_reading(chapter).await;
            app.trackers.read_chapter(chapter);
        }
    }
    // Kept after the chapter moved on, so it is the newer of the two.
    let now = db::now();
    let (document, percentage) = (report.document.clone(), report.percentage.clamp(0.0, 1.0));
    let (device, device_id): (String, String) = (report.device.chars().take(64).collect(), report.device_id.chars().take(64).collect());
    app.db
        .call(move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO sync_progress (document, progress, percentage, device, device_id, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![document, progress, percentage, device, device_id, now],
            )?;
            Ok(())
        })
        .await?;
    Ok(Json(json!({ "document": report.document, "timestamp": now })).into_response())
}

/// The latest place of a document: the one KOReader reported, or Mango's own when it was read in Mango since.
async fn load(Me(app): Me, Path(document): Path<String>) -> Result<Json<Value>> {
    check_document(&document)?;
    Ok(Json(place(&app.db, document).await?))
}

async fn place(db: &Db, document: String) -> Result<Value> {
    db
        .call(move |conn| {
            let kept = conn
                .query_row("SELECT progress, percentage, device, device_id, updated_at FROM sync_progress WHERE document = ?1", [&document], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, i64>(4)?))
                })
                .optional()?;
            let ours = conn
                .query_row(
                    "SELECT c.last_page_read, c.page_count, c.read, c.read_at FROM sync_documents d JOIN chapters c ON c.id = d.chapter_id
                     WHERE d.document = ?1 AND c.read_at IS NOT NULL",
                    [&document],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, bool>(2)?, r.get::<_, i64>(3)?)),
                )
                .optional()?;
            Ok(match (kept, ours) {
                (kept, Some((last, count, read, at))) if kept.as_ref().is_none_or(|kept| at > kept.4) => {
                    let count = count.filter(|count| *count > 0);
                    let page = match count {
                        Some(count) if read => count,
                        _ => last + 1,
                    };
                    let percentage = count.map_or(0.0, |count| (page as f64 / count as f64).min(1.0));
                    json!({ "document": document, "progress": page.to_string(), "percentage": percentage, "device": DEVICE, "device_id": DEVICE_ID, "timestamp": at })
                }
                (Some((progress, percentage, device, device_id, at)), _) => {
                    json!({ "document": document, "progress": progress, "percentage": percentage, "device": device, "device_id": device_id, "timestamp": at })
                }
                (None, _) => json!({}),
            })
        })
        .await
}

/// KOReader's mark for a file's bytes: the MD5 of 1 KB read at 0 and at 1 KB × 4^i for
/// i from 0 to 10, as far as the file goes.
pub fn partial_md5(bytes: &[u8]) -> String {
    let mut md5 = Md5::new();
    for i in -1..=10 {
        let at = if i < 0 { 0 } else { 1024usize << (2 * i) };
        if at >= bytes.len() {
            break;
        }
        md5.update(&bytes[at..(at + 1024).min(bytes.len())]);
    }
    hex::encode(md5.finalize())
}

/// The name KOReader saves a chapter's file under when it fetches it from the catalog:
/// the entry's author, which the catalog gives as the series, and its title, with what a
/// file name cannot hold made `_`.
pub fn koreader_name(series: &str, chapter: &str) -> String {
    let joined = format!("{series} - {chapter}").replace(['\r', '\n', '\t'], " ");
    let mut name: String = joined.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect();
    name.truncate(name.trim_end_matches(|c: char| c == '.' || c.is_whitespace()).len());
    // KOReader keeps the first 240 bytes, and no broken character at the end.
    let mut end = name.len().min(240);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name.truncate(end);
    name + ".cbz"
}

/// Notes the marks KOReader will know a chapter's file by: its bytes', and the names it may be saved under.
pub async fn remember(db: &Db, chapter: i64, bytes: &[u8], names: &[String]) -> Result<()> {
    let marks: Vec<String> = std::iter::once(partial_md5(bytes)).chain(names.iter().map(|name| hex::encode(Md5::digest(name.as_bytes())))).collect();
    db.call(move |conn| {
        for mark in marks {
            conn.execute("INSERT OR REPLACE INTO sync_documents (document, chapter_id) VALUES (?1, ?2)", params![mark, chapter])?;
        }
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_a_file_as_koreader_does() {
        let bytes: Vec<u8> = (0..20_000u32).map(|n| (n % 251) as u8).collect();
        let mut md5 = Md5::new();
        for range in [0..1024, 1024..2048, 4096..5120, 16384..17408] {
            md5.update(&bytes[range]);
        }
        assert_eq!(partial_md5(&bytes), hex::encode(md5.finalize()));
        assert_eq!(partial_md5(b"short"), hex::encode(Md5::digest(b"short")), "a small file is read whole");
        assert_eq!(partial_md5(&bytes[..1024]), hex::encode(Md5::digest(&bytes[..1024])), "a sample that starts at the end is not taken");
        assert_eq!(koreader_name("Sample Series", "Arrival"), "Sample Series - Arrival.cbz");
        assert_eq!(koreader_name("Who/What", "Ch. 2: Why?.."), "Who_What - Ch. 2_ Why_.cbz", "what a file name cannot hold, and dots at the end");
    }

    #[tokio::test]
    async fn a_place_goes_both_ways() {
        let db = Db::open(std::path::Path::new(":memory:")).unwrap();
        db.call(|conn| {
            conn.execute_batch(
                "INSERT INTO extensions (pkg, name, version_name, version_code, lib, content_warning, installed_at, updated_at) VALUES ('p', 'P', '1', 1, '1.6', 0, 0, 0);
                 INSERT INTO sources (id, pkg, name, lang, supports_latest) VALUES (7, 'p', 'Seven', 'en', 1);
                 INSERT INTO manga (id, source_id, url, title, in_library, added_at, status, auto_download, update_strategy) VALUES (1, 7, '/1', 'T', 1, 100, 1, 0, 'ALWAYS_UPDATE');
                 INSERT INTO chapters (id, manga_id, url, name, source_order, fetched_at, page_count) VALUES (10, 1, 'a', 'One', 0, 50, 20);",
            )?;
            Ok(())
        })
        .await
        .unwrap();
        remember(&db, 10, b"chapter file", &["T - One.cbz".to_owned()]).await.unwrap();
        let mark = partial_md5(b"chapter file");
        let rows: i64 = db.call(|conn| Ok(conn.query_row("SELECT count(*) FROM sync_documents WHERE chapter_id = 10", [], |r| r.get(0))?)).await.unwrap();
        assert_eq!(rows, 2, "by its bytes and by its name");

        // Nothing read anywhere yet.
        assert_eq!(place(&db, mark.clone()).await.unwrap(), json!({}));
        // Read in Mango: offered back as Mango's place, page 5 of 20.
        db.call(|conn| Ok(conn.execute("UPDATE chapters SET last_page_read = 4, read_at = 1000 WHERE id = 10", [])?)).await.unwrap();
        let ours = place(&db, mark.clone()).await.unwrap();
        assert_eq!((ours["progress"].as_str(), ours["device_id"].as_str(), ours["percentage"].as_f64()), (Some("5"), Some(DEVICE_ID), Some(0.25)));
        // A newer report from KOReader wins, as it was sent.
        db.call(|conn| {
            conn.execute("INSERT INTO sync_progress VALUES ('m', '7', 0.35, 'Kobo', 'k1', 2000)", [])?;
            Ok(conn.execute("UPDATE sync_progress SET document = (SELECT document FROM sync_documents WHERE chapter_id = 10 LIMIT 1)", [])?)
        })
        .await
        .unwrap();
        let first: String = db.call(|conn| Ok(conn.query_row("SELECT document FROM sync_progress", [], |r| r.get(0))?)).await.unwrap();
        let theirs = place(&db, first).await.unwrap();
        assert_eq!((theirs["progress"].as_str(), theirs["device"].as_str()), (Some("7"), Some("Kobo")));
        // Read to the end in Mango afterwards: the last page, whole.
        db.call(|conn| Ok(conn.execute("UPDATE chapters SET read = 1, read_at = 3000 WHERE id = 10", [])?)).await.unwrap();
        let done = place(&db, mark).await.unwrap();
        assert_eq!((done["progress"].as_str(), done["percentage"].as_f64()), (Some("20"), Some(1.0)));
    }
}
