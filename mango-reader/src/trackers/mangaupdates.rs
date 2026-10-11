//! MangaUpdates: its v1 API. Signing in trades a name and password for a session token.
//! What other trackers call a status is here one of the account's five lists.

use super::{ask, call, judge, label, plain, sign_in, text, tracker_error, year, Account, Entry, Found, Kind, Track};
use crate::error::Result;
use reqwest::{Client, StatusCode};
use serde_json::json;

const API: &str = "https://api.mangaupdates.com/v1";

pub(super) async fn login(http: &Client, username: &str, password: &str) -> Result<Account> {
    let body = sign_in(Kind::MangaUpdates, http.put(format!("{API}/account/login")).json(&json!({ "username": username, "password": password }))).await?;
    let context = &body["context"];
    let token = context["session_token"].as_str().ok_or_else(|| tracker_error("MangaUpdates sent no token".into()))?;
    Ok(Account {
        token: token.to_owned(),
        user_id: context["uid"].as_i64().unwrap_or(0).to_string(),
        user_name: username.to_owned(),
        ..Account::default()
    })
}

/// Open to everyone.
pub(super) async fn search(http: &Client, query: &str) -> Result<Vec<Found>> {
    let body = call(Kind::MangaUpdates, http.post(format!("{API}/series/search")).json(&json!({ "search": query, "perpage": 20 }))).await?;
    let found = body["results"].as_array().into_iter().flatten().filter_map(|item| {
        let record = &item["record"];
        Some(Found {
            media_id: record["series_id"].as_i64()?,
            title: text(&record["title"]),
            summary: plain(&record["description"]),
            kind: label(&record["type"]),
            year: year(&record["year"]),
            // It knows the latest chapter out, not how many there will be.
            total_chapters: 0,
            url: text(&record["url"]),
        })
    });
    Ok(found.collect())
}

pub(super) async fn find(http: &Client, account: &Account, media: i64) -> Result<Option<Entry>> {
    let (status, body) = ask(Kind::MangaUpdates, http.get(format!("{API}/lists/series/{media}")).bearer_auth(&account.token)).await?;
    if status == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let body = judge(Kind::MangaUpdates, status, body)?;
    // The rating is kept apart from the lists, and missing until one is given.
    let (rated, rating) = ask(Kind::MangaUpdates, http.get(format!("{API}/series/{media}/rating")).bearer_auth(&account.token)).await?;
    Ok(Some(Entry {
        // There is no id for an entry apart from the series'.
        library_id: media,
        status: body["list_id"].as_i64().unwrap_or(0) as i32,
        last_chapter_read: body["status"]["chapter"].as_f64().unwrap_or(0.0),
        score: if rated.is_success() { rating["rating"].as_f64().unwrap_or(0.0) } else { 0.0 },
        total_chapters: 0,
    }))
}

pub(super) async fn save(http: &Client, account: &Account, track: &Track) -> Result<i64> {
    let series = json!({ "id": track.media_id });
    if track.library_id == 0 {
        let item = json!([{ "series": series, "list_id": track.status }]);
        call(Kind::MangaUpdates, http.post(format!("{API}/lists/series")).bearer_auth(&account.token).json(&item)).await?;
    }
    let item = json!([{ "series": series, "list_id": track.status, "status": { "chapter": track.last_chapter_read as i64 } }]);
    call(Kind::MangaUpdates, http.post(format!("{API}/lists/series/update")).bearer_auth(&account.token).json(&item)).await?;
    let rating = format!("{API}/series/{}/rating", track.media_id);
    if track.score > 0.0 {
        call(Kind::MangaUpdates, http.put(rating).bearer_auth(&account.token).json(&json!({ "rating": track.score }))).await?;
    } else {
        // Taking away a rating that was never given is refused; that is fine.
        let _ = ask(Kind::MangaUpdates, http.delete(rating).bearer_auth(&account.token)).await;
    }
    Ok(track.media_id)
}
