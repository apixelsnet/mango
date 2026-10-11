//! Kitsu: a JSON:API. Signing in trades the account's e-mail and password for tokens.

use super::{call, label, plain, renewal, sign_in, text, tracker_error, whole, year, Account, Entry, Found, Kind, Track};
use crate::{db::now, error::Result};
use reqwest::{
    header::{ACCEPT, CONTENT_TYPE},
    Client, RequestBuilder,
};
use serde_json::{json, Value};

const API: &str = "https://kitsu.app/api/edge";
const OAUTH: &str = "https://kitsu.app/api/oauth/token";
const SITE: &str = "https://kitsu.app/manga";
// Not secrets: the one client Kitsu's documentation gives to every application.
const CLIENT_ID: &str = "dd031b32d2f56c990b1425efe6c42ad847e7fe3ab46bf1299f05ecd856bdb7dd";
const CLIENT_SECRET: &str = "54d7307928f63414defd96399fc31ba847961ceaecef3a5fd93144e960c0e151";
const JSON_API: &str = "application/vnd.api+json";
const STATUSES: [(i32, &str); 5] = [(1, "current"), (2, "completed"), (3, "on_hold"), (4, "dropped"), (5, "planned")];

fn account(tokens: &Value) -> Result<Account> {
    let token = tokens["access_token"].as_str().ok_or_else(|| tracker_error("Kitsu sent no token".into()))?;
    Ok(Account {
        token: token.to_owned(),
        refresh: text(&tokens["refresh_token"]),
        expires_at: now() + tokens["expires_in"].as_i64().unwrap_or(3600),
        ..Account::default()
    })
}

pub(super) async fn login(http: &Client, username: &str, password: &str) -> Result<Account> {
    let form = [("grant_type", "password"), ("username", username), ("password", password), ("client_id", CLIENT_ID), ("client_secret", CLIENT_SECRET)];
    let tokens = sign_in(Kind::Kitsu, http.post(OAUTH).form(&form)).await?;
    let mut account = account(&tokens)?;
    let request = http.get(format!("{API}/users")).bearer_auth(&account.token).header(ACCEPT, JSON_API).query(&[("filter[self]", "true")]);
    let me = call(Kind::Kitsu, request).await?;
    let user = &me["data"][0];
    account.user_id = text(&user["id"]);
    if account.user_id.is_empty() {
        return Err(tracker_error("Kitsu did not say whose account this is".into()));
    }
    account.user_name = text(&user["attributes"]["name"]);
    Ok(account)
}

pub(super) async fn renew(http: &Client, old: &Account) -> Result<Account> {
    let form = [("grant_type", "refresh_token"), ("refresh_token", old.refresh.as_str()), ("client_id", CLIENT_ID), ("client_secret", CLIENT_SECRET)];
    let tokens = renewal(Kind::Kitsu, http.post(OAUTH).form(&form)).await?;
    let mut renewed = account(&tokens)?;
    if renewed.refresh.is_empty() {
        renewed.refresh = old.refresh.clone();
    }
    renewed.user_id = old.user_id.clone();
    renewed.user_name = old.user_name.clone();
    Ok(renewed)
}

/// Open to everyone.
pub(super) async fn search(http: &Client, query: &str) -> Result<Vec<Found>> {
    let params = [("filter[text]", query), ("page[limit]", "20"), ("fields[manga]", "canonicalTitle,chapterCount,synopsis,subtype,startDate,slug")];
    let body = call(Kind::Kitsu, http.get(format!("{API}/manga")).header(ACCEPT, JSON_API).query(&params)).await?;
    let found = body["data"].as_array().into_iter().flatten().filter_map(|item| {
        let (media_id, about) = (whole(&item["id"]), &item["attributes"]);
        let slug = about["slug"].as_str().map_or_else(|| media_id.to_string(), str::to_owned);
        (media_id > 0).then(|| Found {
            media_id,
            title: text(&about["canonicalTitle"]),
            summary: plain(&about["synopsis"]),
            kind: label(&about["subtype"]),
            year: year(&about["startDate"]),
            total_chapters: about["chapterCount"].as_i64().unwrap_or(0),
            url: format!("{SITE}/{slug}"),
        })
    });
    Ok(found.collect())
}

pub(super) async fn find(http: &Client, account: &Account, media: i64) -> Result<Option<Entry>> {
    let params = [("filter[manga_id]", media.to_string()), ("filter[user_id]", account.user_id.clone()), ("include", "manga".into()), ("page[limit]", "1".into())];
    let request = http.get(format!("{API}/library-entries")).bearer_auth(&account.token).header(ACCEPT, JSON_API).query(&params);
    let body = call(Kind::Kitsu, request).await?;
    let entry = &body["data"][0];
    if entry.is_null() {
        return Ok(None);
    }
    let about = &entry["attributes"];
    Ok(Some(Entry {
        library_id: whole(&entry["id"]),
        status: STATUSES.iter().find(|(_, name)| about["status"].as_str() == Some(*name)).map_or(1, |(number, _)| *number),
        last_chapter_read: about["progress"].as_f64().unwrap_or(0.0),
        // Kitsu counts a score in twentieths; a link keeps it out of ten.
        score: about["ratingTwenty"].as_f64().map_or(0.0, |rating| rating / 2.0),
        total_chapters: body["included"][0]["attributes"]["chapterCount"].as_i64().unwrap_or(0),
    }))
}

fn write(request: RequestBuilder, account: &Account, body: Value) -> RequestBuilder {
    request.bearer_auth(&account.token).header(ACCEPT, JSON_API).header(CONTENT_TYPE, JSON_API).body(body.to_string())
}

pub(super) async fn save(http: &Client, account: &Account, track: &Track) -> Result<i64> {
    let status = STATUSES.iter().find(|(number, _)| *number == track.status).map_or("current", |(_, name)| *name);
    // No score is a null; the lowest one Kitsu takes is 2.
    let rating = (track.score > 0.0).then(|| ((track.score * 2.0).round() as i64).clamp(2, 20));
    let attributes = json!({ "status": status, "progress": track.last_chapter_read as i64, "ratingTwenty": rating });
    if track.library_id > 0 {
        let body = json!({ "data": { "type": "libraryEntries", "id": track.library_id.to_string(), "attributes": attributes } });
        call(Kind::Kitsu, write(http.patch(format!("{API}/library-entries/{}", track.library_id)), account, body)).await?;
        return Ok(track.library_id);
    }
    let body = json!({ "data": {
        "type": "libraryEntries",
        "attributes": attributes,
        "relationships": {
            "user": { "data": { "type": "users", "id": account.user_id } },
            "media": { "data": { "type": "manga", "id": track.media_id.to_string() } },
        },
    } });
    let made = call(Kind::Kitsu, write(http.post(format!("{API}/library-entries")), account, body)).await?;
    match whole(&made["data"]["id"]) {
        0 => Err(tracker_error("Kitsu did not confirm the new entry".into())),
        id => Ok(id),
    }
}
