//! AniList: one GraphQL address. Signing in is a token the owner gets there and pastes in,
//! since AniList only sends tokens to addresses registered with it.

use super::{ask, judge, label, plain, said, sign_in, signed_out, text, tracker_error, year, Account, Entry, Found, Kind, Track};
use crate::error::{AppError, Result};
use reqwest::Client;
use serde_json::{json, Value};

const API: &str = "https://graphql.anilist.co";
const STATUSES: [(i32, &str); 6] = [(1, "CURRENT"), (2, "COMPLETED"), (3, "PAUSED"), (4, "DROPPED"), (5, "PLANNING"), (6, "REPEATING")];

const SEARCH: &str = "query ($q: String) { Page(perPage: 20) { media(search: $q, type: MANGA, format_not_in: [NOVEL]) {
    id title { userPreferred } format chapters description startDate { year } siteUrl } } }";
// Scores are asked for out of 100 whatever scale the account shows them in.
const FIND: &str = "query ($user: Int, $media: Int) { Page { mediaList(userId: $user, mediaId: $media, type: MANGA) {
    id status progress score(format: POINT_100) media { chapters } } } }";
const SAVE: &str = "mutation ($media: Int, $status: MediaListStatus, $progress: Int, $score: Int) {
    SaveMediaListEntry(mediaId: $media, status: $status, progress: $progress, scoreRaw: $score) { id } }";

fn request(http: &Client, token: Option<&str>, query: &str, variables: Value) -> reqwest::RequestBuilder {
    let request = http.post(API).json(&json!({ "query": query, "variables": variables }));
    match token {
        Some(token) => request.bearer_auth(token),
        None => request,
    }
}

async fn graphql(http: &Client, token: Option<&str>, query: &str, variables: Value) -> Result<Value> {
    let (status, body) = ask(Kind::AniList, request(http, token, query, variables)).await?;
    // AniList calls a token that ran out a bad request, not an unauthorized one.
    if token.is_some() && said(&body) == "Invalid token" {
        return Err(signed_out(Kind::AniList));
    }
    let mut body = judge(Kind::AniList, status, body)?;
    Ok(body.get_mut("data").map(Value::take).unwrap_or_default())
}

pub(super) async fn login(http: &Client, pasted: &str) -> Result<Account> {
    // The whole address of the page AniList shows will do too: the token is in it.
    let pasted = pasted.trim();
    let token = pasted.split_once("access_token=").map_or(pasted, |(_, rest)| rest.split('&').next().unwrap_or(rest));
    if token.is_empty() {
        return Err(AppError::bad_request("Paste the token AniList shows"));
    }
    let body = sign_in(Kind::AniList, request(http, Some(token), "query { Viewer { id name } }", json!({}))).await?;
    let viewer = &body["data"]["Viewer"];
    let id = viewer["id"].as_i64().ok_or_else(|| tracker_error("AniList did not say whose token this is".into()))?;
    Ok(Account { token: token.to_owned(), user_id: id.to_string(), user_name: text(&viewer["name"]), ..Account::default() })
}

/// Open to everyone, so it works before signing in and with a token that ran out.
pub(super) async fn search(http: &Client, query: &str) -> Result<Vec<Found>> {
    let data = graphql(http, None, SEARCH, json!({ "q": query })).await?;
    let found = data["Page"]["media"].as_array().into_iter().flatten().filter_map(|media| {
        let media_id = media["id"].as_i64()?;
        Some(Found {
            media_id,
            title: text(&media["title"]["userPreferred"]),
            summary: plain(&media["description"]),
            kind: label(&media["format"]),
            year: year(&media["startDate"]["year"]),
            total_chapters: media["chapters"].as_i64().unwrap_or(0),
            url: format!("https://anilist.co/manga/{media_id}"),
        })
    });
    Ok(found.collect())
}

pub(super) async fn find(http: &Client, account: &Account, media: i64) -> Result<Option<Entry>> {
    let user: i64 = account.user_id.parse().map_err(|_| signed_out(Kind::AniList))?;
    let data = graphql(http, Some(&account.token), FIND, json!({ "user": user, "media": media })).await?;
    let Some(entry) = data["Page"]["mediaList"].get(0) else { return Ok(None) };
    Ok(Some(Entry {
        library_id: entry["id"].as_i64().unwrap_or(0),
        status: STATUSES.iter().find(|(_, name)| entry["status"].as_str() == Some(*name)).map_or(1, |(number, _)| *number),
        last_chapter_read: entry["progress"].as_f64().unwrap_or(0.0),
        score: entry["score"].as_f64().unwrap_or(0.0),
        total_chapters: entry["media"]["chapters"].as_i64().unwrap_or(0),
    }))
}

pub(super) async fn save(http: &Client, account: &Account, track: &Track) -> Result<i64> {
    let status = STATUSES.iter().find(|(number, _)| *number == track.status).map_or("CURRENT", |(_, name)| *name);
    let variables = json!({ "media": track.media_id, "status": status, "progress": track.last_chapter_read as i64, "score": track.score as i64 });
    let data = graphql(http, Some(&account.token), SAVE, variables).await?;
    data["SaveMediaListEntry"]["id"].as_i64().ok_or_else(|| tracker_error("AniList did not confirm the change".into()))
}
