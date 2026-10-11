//! MyAnimeList: its v2 API. Signing in goes through its own page and comes back with a
//! code, under an application the owner registers there: its address has to be Mango's.

use super::{call, label, plain, renewal, sign_in, text, tracker_error, whole, year, Account, Entry, Found, Kind, Track};
use crate::{
    db::now,
    error::{AppError, Result},
};
use reqwest::{Client, Url};
use serde_json::Value;

const API: &str = "https://api.myanimelist.net/v2";
const OAUTH: &str = "https://myanimelist.net/v1/oauth2";
const REREADING: i32 = 7;
const STATUSES: [(i32, &str); 5] = [(1, "reading"), (2, "completed"), (3, "on_hold"), (4, "dropped"), (6, "plan_to_read")];

/// The page to send the owner to. MyAnimeList takes the verifier itself as the challenge.
pub(super) fn authorize_url(client_id: &str, verifier: &str, state: &str, redirect: &str) -> Result<String> {
    let params = [
        ("response_type", "code"),
        ("client_id", client_id),
        ("code_challenge", verifier),
        ("code_challenge_method", "plain"),
        ("state", state),
        ("redirect_uri", redirect),
    ];
    Url::parse_with_params(&format!("{OAUTH}/authorize"), params).map(String::from).map_err(|e| AppError::internal(e.to_string()))
}

fn account(client_id: &str, tokens: &Value) -> Result<Account> {
    let token = tokens["access_token"].as_str().ok_or_else(|| tracker_error("MyAnimeList sent no token".into()))?;
    Ok(Account {
        token: token.to_owned(),
        refresh: text(&tokens["refresh_token"]),
        expires_at: now() + tokens["expires_in"].as_i64().unwrap_or(3600),
        client_id: client_id.to_owned(),
        ..Account::default()
    })
}

pub(super) async fn login(http: &Client, client_id: &str, code: &str, verifier: &str, redirect: &str) -> Result<Account> {
    let form = [("client_id", client_id), ("code", code), ("code_verifier", verifier), ("grant_type", "authorization_code"), ("redirect_uri", redirect)];
    let tokens = sign_in(Kind::MyAnimeList, http.post(format!("{OAUTH}/token")).form(&form)).await?;
    let mut account = account(client_id, &tokens)?;
    let me = call(Kind::MyAnimeList, http.get(format!("{API}/users/@me")).bearer_auth(&account.token)).await?;
    account.user_id = whole(&me["id"]).to_string();
    account.user_name = text(&me["name"]);
    Ok(account)
}

pub(super) async fn renew(http: &Client, old: &Account) -> Result<Account> {
    let form = [("client_id", old.client_id.as_str()), ("refresh_token", old.refresh.as_str()), ("grant_type", "refresh_token")];
    let tokens = renewal(Kind::MyAnimeList, http.post(format!("{OAUTH}/token")).form(&form)).await?;
    let mut renewed = account(&old.client_id, &tokens)?;
    if renewed.refresh.is_empty() {
        renewed.refresh = old.refresh.clone();
    }
    renewed.user_id = old.user_id.clone();
    renewed.user_name = old.user_name.clone();
    Ok(renewed)
}

pub(super) async fn search(http: &Client, account: &Account, query: &str) -> Result<Vec<Found>> {
    // MyAnimeList takes 3 to 64 letters.
    let query: String = query.chars().take(64).collect();
    if query.chars().count() < 3 {
        return Err(AppError::bad_request("MyAnimeList wants three letters or more"));
    }
    let params = [("q", query.as_str()), ("limit", "20"), ("nsfw", "true"), ("fields", "id,title,synopsis,num_chapters,media_type,start_date")];
    let body = call(Kind::MyAnimeList, http.get(format!("{API}/manga")).bearer_auth(&account.token).query(&params)).await?;
    let found = body["data"].as_array().into_iter().flatten().filter_map(|item| {
        let node = &item["node"];
        let media_id = node["id"].as_i64()?;
        Some(Found {
            media_id,
            title: text(&node["title"]),
            summary: plain(&node["synopsis"]),
            kind: label(&node["media_type"]),
            year: year(&node["start_date"]),
            total_chapters: node["num_chapters"].as_i64().unwrap_or(0),
            url: format!("https://myanimelist.net/manga/{media_id}"),
        })
    });
    Ok(found.collect())
}

pub(super) async fn find(http: &Client, account: &Account, media: i64) -> Result<Option<Entry>> {
    let request = http.get(format!("{API}/manga/{media}")).bearer_auth(&account.token).query(&[("fields", "num_chapters,my_list_status")]);
    let body = call(Kind::MyAnimeList, request).await?;
    let mine = &body["my_list_status"];
    if mine.is_null() {
        return Ok(None);
    }
    let status = if mine["is_rereading"].as_bool() == Some(true) {
        REREADING
    } else {
        STATUSES.iter().find(|(_, name)| mine["status"].as_str() == Some(*name)).map_or(1, |(number, _)| *number)
    };
    Ok(Some(Entry {
        // MyAnimeList has no id for an entry apart from the title's.
        library_id: media,
        status,
        last_chapter_read: mine["num_chapters_read"].as_f64().unwrap_or(0.0),
        score: mine["score"].as_f64().unwrap_or(0.0),
        total_chapters: body["num_chapters"].as_i64().unwrap_or(0),
    }))
}

pub(super) async fn save(http: &Client, account: &Account, track: &Track) -> Result<i64> {
    let rereading = track.status == REREADING;
    let status = STATUSES.iter().find(|(number, _)| *number == track.status).map_or("reading", |(_, name)| *name);
    let form = [
        ("status", status.to_owned()),
        ("is_rereading", rereading.to_string()),
        ("num_chapters_read", (track.last_chapter_read as i64).to_string()),
        ("score", (track.score as i64).to_string()),
    ];
    let request = http.put(format!("{API}/manga/{}/my_list_status", track.media_id)).bearer_auth(&account.token).form(&form);
    call(Kind::MyAnimeList, request).await?;
    Ok(track.media_id)
}
