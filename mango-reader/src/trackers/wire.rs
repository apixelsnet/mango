//! The four tracker clients against a stand-in on this machine: what each sends, and how it
//! reads the answers, in the shapes each tracker's documentation gives. No real tracker is
//! reached; signing in to a real one is still checked by hand.

use super::{anilist, kitsu, mal, mangaupdates, Account, Track, STAND_IN};
use axum::{body::Bytes, extract::Request, http::StatusCode, response::IntoResponse, Router};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

/// A call the stand-in took: method, path with its query, the Authorization header, the body.
#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    auth: String,
    body: String,
}

type Answer = fn(&str, &str, &str) -> (StatusCode, Value);

/// Starts the stand-in, sends every tracker's calls to it, and answers each with `answer`
/// (method, path, body). Answers what it saw.
async fn stand_in(answer: Answer) -> Arc<Mutex<Vec<Seen>>> {
    let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
    let log = seen.clone();
    let app = Router::new().fallback(move |request: Request| {
        let log = log.clone();
        async move {
            let (parts, body) = request.into_parts();
            let body = String::from_utf8_lossy(&axum::body::to_bytes(body, 1 << 20).await.unwrap_or_else(|_| Bytes::new())).into_owned();
            let path = parts.uri.path_and_query().map(|p| p.as_str().to_owned()).unwrap_or_default();
            let auth = parts.headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
            let method = parts.method.to_string();
            let (status, value) = answer(&method, &path, &body);
            log.lock().unwrap().push(Seen { method, path, auth, body });
            (status, axum::Json(value)).into_response()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    *STAND_IN.lock().unwrap() = Some(base);
    seen
}

fn track(media_id: i64, library_id: i64, status: i32, chapter: f64, score: f64) -> Track {
    Track { tracker: 0, library_id, media_id, url: String::new(), title: String::new(), last_chapter_read: chapter, total_chapters: 0, score, status, error: None }
}

fn last(seen: &Arc<Mutex<Vec<Seen>>>) -> Seen {
    seen.lock().unwrap().last().cloned().unwrap()
}

// One test for all four: the stand-in's address is one for the whole process.
#[tokio::test]
async fn every_tracker_client_speaks_its_trackers_tongue() {
    let http = reqwest::Client::new();

    // ---------- AniList: GraphQL at one address ----------
    let seen = stand_in(|_, _, body| {
        let query: Value = serde_json::from_str(body).unwrap_or_default();
        let text = query["query"].as_str().unwrap_or_default();
        let data = if text.contains("Viewer") {
            json!({ "Viewer": { "id": 7, "name": "Ann" } })
        } else if text.contains("Page(perPage") {
            json!({ "Page": { "media": [{ "id": 31, "title": { "userPreferred": "Alpha" }, "format": "ONE_SHOT", "chapters": 12, "description": "<b>Bold</b> start", "startDate": { "year": 2019 } }] } })
        } else if text.contains("mediaList") {
            json!({ "Page": { "mediaList": [{ "id": 3, "status": "PAUSED", "progress": 12, "score": 80, "media": { "chapters": 50 } }] } })
        } else if query["variables"]["progress"] == json!(-1) {
            return (StatusCode::BAD_REQUEST, json!({ "errors": [{ "message": "Invalid token" }] }));
        } else {
            json!({ "SaveMediaListEntry": { "id": 99 } })
        };
        (StatusCode::OK, json!({ "data": data }))
    })
    .await;
    let account = anilist::login(&http, "https://anilist.co/api/v2/oauth/pin#access_token=tok123&token_type=Bearer&expires_in=1").await.unwrap();
    assert_eq!((account.token.as_str(), account.user_id.as_str(), account.user_name.as_str()), ("tok123", "7", "Ann"), "the token is taken out of the pasted address");
    assert_eq!(last(&seen).auth, "Bearer tok123");
    let found = anilist::search(&http, "alpha").await.unwrap();
    assert_eq!((found[0].media_id, found[0].kind.as_str(), found[0].year, found[0].total_chapters), (31, "one shot", Some(2019), 12));
    assert_eq!(found[0].summary, "Bold start", "tags out of the summary");
    assert_eq!(last(&seen).auth, "", "searching takes no token");
    let entry = anilist::find(&http, &account, 31).await.unwrap().unwrap();
    assert_eq!((entry.library_id, entry.status, entry.last_chapter_read, entry.score, entry.total_chapters), (3, 3, 12.0, 80.0, 50));
    assert_eq!(anilist::save(&http, &account, &track(31, 3, 2, 20.0, 85.0)).await.unwrap(), 99);
    let sent: Value = serde_json::from_str(&last(&seen).body).unwrap();
    assert_eq!(sent["variables"], json!({ "media": 31, "status": "COMPLETED", "progress": 20, "score": 85 }));
    let gone = anilist::save(&http, &account, &track(31, 3, 1, -1.0, 0.0)).await.unwrap_err();
    assert_eq!(gone.code, "tracker_signed_out", "AniList's \"Invalid token\" is a sign-in that ran out");

    // ---------- Kitsu: a JSON:API, with tokens for a password ----------
    let seen = stand_in(|method, path, _| match (method, path.split('?').next().unwrap_or_default()) {
        ("POST", "/api/oauth/token") => (StatusCode::OK, json!({ "access_token": "a1", "refresh_token": "r1", "expires_in": 100 })),
        ("GET", "/api/edge/users") => (StatusCode::OK, json!({ "data": [{ "id": "5", "attributes": { "name": "Kit" } }] })),
        ("GET", "/api/edge/manga") => (
            StatusCode::OK,
            json!({ "data": [{ "id": "12", "attributes": { "canonicalTitle": "Beta", "chapterCount": 30, "synopsis": "S", "subtype": "manga", "startDate": "2001-02-03", "slug": "beta" } }] }),
        ),
        ("GET", "/api/edge/library-entries") => (
            StatusCode::OK,
            json!({ "data": [{ "id": "44", "attributes": { "status": "on_hold", "progress": 3, "ratingTwenty": 14 } }], "included": [{ "attributes": { "chapterCount": 30 } }] }),
        ),
        ("POST", "/api/edge/library-entries") => (StatusCode::CREATED, json!({ "data": { "id": "45" } })),
        ("PATCH", _) => (StatusCode::OK, json!({ "data": { "id": "44" } })),
        _ => (StatusCode::NOT_FOUND, json!({ "errors": [{ "detail": "no such thing" }] })),
    })
    .await;
    let account = kitsu::login(&http, "kit@example.org", "pw").await.unwrap();
    assert_eq!((account.token.as_str(), account.refresh.as_str(), account.user_id.as_str(), account.user_name.as_str()), ("a1", "r1", "5", "Kit"));
    let token_call = seen.lock().unwrap()[0].clone();
    assert!(token_call.body.contains("grant_type=password") && token_call.body.contains("username=kit%40example.org"), "{}", token_call.body);
    let found = kitsu::search(&http, "beta").await.unwrap();
    assert_eq!((found[0].media_id, found[0].year, found[0].url.as_str()), (12, Some(2001), "https://kitsu.app/manga/beta"));
    let entry = kitsu::find(&http, &account, 12).await.unwrap().unwrap();
    assert_eq!((entry.library_id, entry.status, entry.last_chapter_read, entry.score, entry.total_chapters), (44, 3, 3.0, 7.0, 30), "twentieths come back out of ten");
    assert_eq!(kitsu::save(&http, &account, &track(12, 0, 5, 4.0, 8.0)).await.unwrap(), 45);
    let made = last(&seen);
    let body: Value = serde_json::from_str(&made.body).unwrap();
    assert_eq!((made.method.as_str(), made.auth.as_str()), ("POST", "Bearer a1"));
    assert_eq!(body["data"]["attributes"], json!({ "status": "planned", "progress": 4, "ratingTwenty": 16 }));
    assert_eq!(body["data"]["relationships"]["media"]["data"]["id"], "12");
    assert_eq!(kitsu::save(&http, &account, &track(12, 44, 1, 5.0, 0.0)).await.unwrap(), 44);
    let patched: Value = serde_json::from_str(&last(&seen).body).unwrap();
    assert_eq!(patched["data"]["attributes"]["ratingTwenty"], Value::Null, "no score is a null");

    // ---------- MyAnimeList: its v2 API, signed in through its own page ----------
    let seen = stand_in(|method, path, _| match (method, path.split('?').next().unwrap_or_default()) {
        ("POST", "/v1/oauth2/token") => (StatusCode::OK, json!({ "access_token": "m1", "refresh_token": "mr", "expires_in": 100 })),
        ("GET", "/v2/users/@me") => (StatusCode::OK, json!({ "id": 9, "name": "Mal" })),
        ("GET", "/v2/manga") => (StatusCode::OK, json!({ "data": [{ "node": { "id": 77, "title": "Gamma", "num_chapters": 40, "media_type": "manhwa", "start_date": "2015" } }] })),
        ("GET", "/v2/manga/77") => (StatusCode::OK, json!({ "num_chapters": 40, "my_list_status": { "status": "completed", "is_rereading": true, "num_chapters_read": 40, "score": 9 } })),
        ("GET", "/v2/manga/78") => (StatusCode::OK, json!({ "num_chapters": 10 })),
        ("PUT", _) => (StatusCode::OK, json!({})),
        _ => (StatusCode::UNAUTHORIZED, json!({ "error": "invalid_token" })),
    })
    .await;
    let account = mal::login(&http, "client", "code1", "verifier", "https://mango.example.org/").await.unwrap();
    assert_eq!((account.token.as_str(), account.user_id.as_str(), account.client_id.as_str()), ("m1", "9", "client"));
    assert!(mal::search(&http, &account, "ab").await.is_err(), "three letters or more");
    let found = mal::search(&http, &account, "gamma").await.unwrap();
    assert_eq!((found[0].media_id, found[0].kind.as_str(), found[0].year), (77, "manhwa", Some(2015)));
    assert!(last(&seen).path.contains("q=gamma"));
    let entry = mal::find(&http, &account, 77).await.unwrap().unwrap();
    assert_eq!((entry.status, entry.last_chapter_read, entry.score), (7, 40.0, 9.0), "reading again is a status of its own");
    assert!(mal::find(&http, &account, 78).await.unwrap().is_none(), "not on the list");
    mal::save(&http, &account, &track(77, 77, 7, 3.0, 6.0)).await.unwrap();
    let put = last(&seen);
    assert_eq!((put.method.as_str(), put.path.as_str()), ("PUT", "/v2/manga/77/my_list_status"));
    assert!(put.body.contains("is_rereading=true") && put.body.contains("num_chapters_read=3") && put.body.contains("score=6"), "{}", put.body);
    let gone = mal::find(&http, &Account { token: "old".into(), ..account.clone() }, 79).await.unwrap_err();
    assert_eq!(gone.code, "tracker_signed_out");

    // ---------- MangaUpdates: a session token, and five lists ----------
    let seen = stand_in(|method, path, _| match (method, path) {
        ("PUT", "/v1/account/login") => (StatusCode::OK, json!({ "context": { "session_token": "s1", "uid": 3 } })),
        ("POST", "/v1/series/search") => (
            StatusCode::OK,
            json!({ "results": [{ "record": { "series_id": 555, "title": "Delta", "url": "https://www.mangaupdates.com/series/x", "type": "Manhua", "year": "2010", "description": "D" } }] }),
        ),
        ("PUT", _) | ("POST", _) | ("DELETE", _) => (StatusCode::OK, json!({})),
        ("GET", "/v1/lists/series/555") => (StatusCode::OK, json!({ "list_id": 1, "status": { "chapter": 5 } })),
        ("GET", "/v1/series/555/rating") => (StatusCode::OK, json!({ "rating": 8.5 })),
        _ => (StatusCode::NOT_FOUND, json!({ "reason": "not on a list" })),
    })
    .await;
    let account = mangaupdates::login(&http, "reader", "pw").await.unwrap();
    assert_eq!((account.token.as_str(), account.user_id.as_str(), account.user_name.as_str()), ("s1", "3", "reader"));
    let found = mangaupdates::search(&http, "delta").await.unwrap();
    assert_eq!((found[0].media_id, found[0].kind.as_str(), found[0].year), (555, "manhua", Some(2010)));
    let entry = mangaupdates::find(&http, &account, 555).await.unwrap().unwrap();
    assert_eq!((entry.library_id, entry.status, entry.last_chapter_read, entry.score), (555, 1, 5.0, 8.5));
    assert!(mangaupdates::find(&http, &account, 556).await.unwrap().is_none(), "not on any list");
    let before = seen.lock().unwrap().len();
    assert_eq!(mangaupdates::save(&http, &account, &track(555, 0, 2, 7.0, 0.0)).await.unwrap(), 555);
    let calls: Vec<(String, String)> = seen.lock().unwrap()[before..].iter().map(|s| (s.method.clone(), s.path.clone())).collect();
    let expected = [("POST", "/v1/lists/series"), ("POST", "/v1/lists/series/update"), ("DELETE", "/v1/series/555/rating")];
    assert_eq!(calls, expected.map(|(m, p)| (m.to_owned(), p.to_owned())), "added to its list first, then moved on; no score takes the rating away");

    *STAND_IN.lock().unwrap() = None;
}
