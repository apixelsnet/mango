//! Trackers: sites that keep a reading list. Mango links a title to its entry on one
//! and sends the chapter reached, the status and the score.
//!
//! Tracker ids, statuses and scores are stored as Mihon stores them, so links come in
//! with a backup and go out with one unchanged. Tokens live in the `state` table, which
//! the settings API never shows. A password is used once to sign in and is not kept.

mod anilist;
mod kitsu;
mod mal;
mod mangaupdates;

use crate::{
    db::{self, now, Db},
    error::{AppError, Result},
    titles::{id_list, members},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::{Client, RequestBuilder, StatusCode};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;

/// Where a sign-in to MyAnimeList waits between leaving for its page and coming back.
const PENDING: &str = "tracker.pending";
/// After each library update, take in what the trackers say was read elsewhere.
pub const PULL: &str = "trackers.pull";
/// The pause between two titles when the trackers are asked in a row, to keep under their limits.
const BETWEEN_TITLES: Duration = Duration::from_secs(2);
const MAX_CHAPTER: f64 = 100_000.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    AniList,
    MyAnimeList,
    Kitsu,
    MangaUpdates,
}

impl Kind {
    const ALL: [Kind; 4] = [Kind::AniList, Kind::MyAnimeList, Kind::Kitsu, Kind::MangaUpdates];

    /// The tracker's id in Mihon, which backups carry.
    pub fn id(self) -> i64 {
        match self {
            Kind::MyAnimeList => 1,
            Kind::AniList => 2,
            Kind::Kitsu => 3,
            Kind::MangaUpdates => 7,
        }
    }

    pub fn from_id(id: i64) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.id() == id)
    }

    fn name(self) -> &'static str {
        match self {
            Kind::AniList => "AniList",
            Kind::MyAnimeList => "MyAnimeList",
            Kind::Kitsu => "Kitsu",
            Kind::MangaUpdates => "MangaUpdates",
        }
    }

    /// How one signs in: a token pasted in, a trip to the site and back, or a name and password.
    fn login(self) -> &'static str {
        match self {
            Kind::AniList => "token",
            Kind::MyAnimeList => "oauth",
            Kind::Kitsu | Kind::MangaUpdates => "password",
        }
    }

    /// The statuses in the order shown, by Mihon's numbers. On MangaUpdates they are its lists.
    fn statuses(self) -> &'static [(i32, &'static str)] {
        match self {
            Kind::AniList => &[(1, "Reading"), (2, "Completed"), (3, "Paused"), (4, "Dropped"), (5, "Planning"), (6, "Rereading")],
            Kind::MyAnimeList => &[(1, "Reading"), (2, "Completed"), (3, "On hold"), (4, "Dropped"), (6, "Plan to read"), (7, "Rereading")],
            Kind::Kitsu => &[(1, "Reading"), (2, "Completed"), (3, "On hold"), (4, "Dropped"), (5, "Want to read")],
            Kind::MangaUpdates => &[(0, "Reading"), (1, "Wish list"), (2, "Complete"), (3, "Unfinished"), (4, "On hold")],
        }
    }

    fn reading(self) -> i32 {
        if self == Kind::MangaUpdates { 0 } else { 1 }
    }

    fn completed(self) -> i32 {
        2
    }

    /// The status of a title that was added and not started.
    fn planned(self) -> i32 {
        match self {
            Kind::MyAnimeList => 6,
            Kind::AniList | Kind::Kitsu => 5,
            Kind::MangaUpdates => 1,
        }
    }

    /// Scores are shown out of ten everywhere: `(what is stored per point shown, the smallest step shown)`.
    fn score(self) -> (f64, f64) {
        match self {
            Kind::AniList => (10.0, 0.5),
            Kind::MyAnimeList => (1.0, 1.0),
            Kind::Kitsu => (1.0, 0.5),
            Kind::MangaUpdates => (1.0, 0.1),
        }
    }
}

/// A signed-in account, as kept in the `state` table.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Account {
    token: String,
    #[serde(default)]
    refresh: String,
    /// Epoch seconds; 0 when the tracker gives its tokens no end.
    #[serde(default)]
    expires_at: i64,
    user_id: String,
    user_name: String,
    /// The owner's own application at the tracker, where one is needed.
    #[serde(default)]
    client_id: String,
}

/// A sign-in to MyAnimeList that left for its page and has not come back yet.
#[derive(Serialize, Deserialize)]
struct Pending {
    client_id: String,
    verifier: String,
    state: String,
    redirect: String,
}

/// What signs one in. Which fields count depends on the tracker.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Credentials {
    pub token: String,
    pub username: String,
    pub password: String,
    pub code: String,
    pub state: String,
}

#[derive(Serialize)]
pub struct Choice {
    value: i32,
    label: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    id: i64,
    name: &'static str,
    login: &'static str,
    signed_in: bool,
    user_name: Option<String>,
    statuses: Vec<Choice>,
    score_scale: f64,
    score_step: f64,
}

/// A title as a tracker's search lists it.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    media_id: i64,
    title: String,
    summary: String,
    kind: String,
    year: Option<i64>,
    total_chapters: i64,
    url: String,
}

/// The search result the owner picked for a title.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pick {
    pub media_id: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub total_chapters: i64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub status: Option<i32>,
    pub score: Option<f64>,
    pub last_chapter_read: Option<f64>,
}

/// The owner's entry for a title on a tracker, in the units a link is stored in.
#[derive(Debug)]
struct Entry {
    library_id: i64,
    status: i32,
    last_chapter_read: f64,
    score: f64,
    total_chapters: i64,
}

/// A title's link to a tracker.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub tracker: i64,
    /// The entry's id on the tracker; 0 until it was sent once.
    pub library_id: i64,
    pub media_id: i64,
    pub url: String,
    pub title: String,
    pub last_chapter_read: f64,
    /// 0 when the tracker does not know.
    pub total_chapters: i64,
    pub score: f64,
    pub status: i32,
    /// Why the last sending failed. Set while Mango is ahead of the tracker.
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct View {
    trackers: Vec<Info>,
    tracks: Vec<Track>,
}

// ---------- talking to a tracker ----------

fn tracker_error(message: String) -> AppError {
    AppError::new(502, "tracker_error", message)
}

fn signed_out(kind: Kind) -> AppError {
    AppError::new(409, "tracker_signed_out", format!("Sign in to {} again, in Settings", kind.name()))
}

/// Sends a request and reads the answer as JSON, whatever its status.
async fn ask(kind: Kind, request: RequestBuilder) -> Result<(StatusCode, Value)> {
    let response = request.send().await.map_err(|e| tracker_error(format!("{} did not answer: {}", kind.name(), e.without_url())))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
}

/// What a tracker said went wrong, from the places they put it.
fn said(body: &Value) -> String {
    let places = [&body["errors"][0]["message"], &body["errors"][0]["detail"], &body["error_description"], &body["reason"], &body["message"], &body["error"]];
    places.into_iter().find_map(Value::as_str).unwrap_or("no reason given").chars().take(200).collect()
}

/// Turns a refusal into an error that names the tracker. A 401 means the token is no good any more.
fn judge(kind: Kind, status: StatusCode, body: Value) -> Result<Value> {
    if status.is_success() {
        Ok(body)
    } else if status == StatusCode::UNAUTHORIZED {
        Err(signed_out(kind))
    } else {
        Err(tracker_error(format!("{} answered {}: {}", kind.name(), status.as_u16(), said(&body))))
    }
}

/// A call made with a token.
async fn call(kind: Kind, request: RequestBuilder) -> Result<Value> {
    let (status, body) = ask(kind, request).await?;
    judge(kind, status, body)
}

/// A sign-in: a refusal is the owner's to put right.
async fn sign_in(kind: Kind, request: RequestBuilder) -> Result<Value> {
    let (status, body) = ask(kind, request).await?;
    if status.is_client_error() {
        return Err(AppError::new(422, "tracker_login", format!("{} did not accept that: {}", kind.name(), said(&body))));
    }
    judge(kind, status, body)
}

/// A token traded for a newer one: a refusal means signing in again.
async fn renewal(kind: Kind, request: RequestBuilder) -> Result<Value> {
    let (status, body) = ask(kind, request).await?;
    if status.is_client_error() {
        return Err(signed_out(kind));
    }
    judge(kind, status, body)
}

fn text(value: &Value) -> String {
    value.as_str().unwrap_or_default().to_owned()
}

/// A number that some trackers send as text.
fn whole(value: &Value) -> i64 {
    value.as_i64().or_else(|| value.as_str()?.parse().ok()).unwrap_or(0)
}

/// "ONE_SHOT" as "one shot".
fn label(value: &Value) -> String {
    text(value).replace('_', " ").to_lowercase()
}

/// The year of a date like "2019-03-01", or of a bare number.
fn year(value: &Value) -> Option<i64> {
    value.as_i64().or_else(|| value.as_str()?.get(..4)?.parse().ok()).filter(|year| *year > 0)
}

/// A summary for a search result: tags out, a few lines at most.
fn plain(value: &Value) -> String {
    let (mut out, mut tag) = (String::new(), false);
    for c in value.as_str().unwrap_or_default().chars() {
        match c {
            '<' => tag = true,
            '>' if tag => tag = false,
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    match out.char_indices().nth(280) {
        Some((cut, _)) => format!("{}…", &out[..cut]),
        None => out,
    }
}

/// Random letters for a sign-in's secrets.
fn secret(bytes: usize) -> Result<String> {
    let mut raw = vec![0u8; bytes];
    getrandom::getrandom(&mut raw).map_err(|e| AppError::internal(format!("no random bytes: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(raw))
}

// ---------- links in the database ----------

const COLUMNS: &str = "tracker, library_id, media_id, url, title, last_chapter_read, total_chapters, score, status, error";

fn key(kind: Kind) -> String {
    format!("tracker.{}", kind.id())
}

fn track_row(r: &Row) -> rusqlite::Result<Track> {
    Ok(Track {
        tracker: r.get(0)?,
        library_id: r.get(1)?,
        media_id: r.get(2)?,
        url: r.get(3)?,
        title: r.get(4)?,
        last_chapter_read: r.get(5)?,
        total_chapters: r.get(6)?,
        score: r.get(7)?,
        status: r.get(8)?,
        error: r.get(9)?,
    })
}

fn tracks(conn: &Connection, manga: i64) -> Result<Vec<Track>> {
    let mut stmt = conn.prepare_cached(&format!("SELECT {COLUMNS} FROM tracks WHERE manga_id = ?1 ORDER BY tracker"))?;
    let rows = stmt.query_map([manga], track_row)?.collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn track(conn: &Connection, manga: i64, kind: Kind) -> Result<Track> {
    conn.query_row(&format!("SELECT {COLUMNS} FROM tracks WHERE manga_id = ?1 AND tracker = ?2"), [manga, kind.id()], track_row)
        .optional()?
        .ok_or_else(|| AppError::not_found(format!("This title is not linked to {}", kind.name())))
}

/// Writes a link. What a backup brought along and Mango does not change, the dates, stays.
fn store(conn: &Connection, manga: i64, track: &Track) -> Result<()> {
    conn.execute(
        "INSERT INTO tracks (manga_id, tracker, library_id, media_id, url, title, last_chapter_read, total_chapters, score, status, error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(manga_id, tracker) DO UPDATE SET library_id = excluded.library_id, media_id = excluded.media_id, url = excluded.url,
             title = excluded.title, last_chapter_read = excluded.last_chapter_read, total_chapters = excluded.total_chapters,
             score = excluded.score, status = excluded.status, error = excluded.error",
        params![
            manga,
            track.tracker,
            track.library_id,
            track.media_id,
            track.url,
            track.title,
            track.last_chapter_read,
            track.total_chapters,
            track.score,
            track.status,
            track.error
        ],
    )?;
    Ok(())
}

/// The highest number among a title's read chapters, in whole chapters; 0 when none is read or numbered.
/// The chapters of titles merged into it count.
fn reached(conn: &Connection, manga: i64) -> Result<f64> {
    let list = id_list(&members(conn, manga)?);
    let highest: f64 =
        conn.query_row(&format!("SELECT COALESCE(MAX(chapter_number), 0) FROM chapters WHERE manga_id IN ({list}) AND read"), [], |r| r.get(0))?;
    Ok(highest.clamp(0.0, MAX_CHAPTER).floor())
}

/// Marks read every numbered chapter up to `upto`, which a tracker says was read elsewhere.
/// Answers how many changed.
fn catch_up(conn: &Connection, manga: i64, upto: f64) -> Result<usize> {
    if upto <= 0.0 {
        return Ok(0);
    }
    let list = id_list(&members(conn, manga)?);
    Ok(conn.execute(
        &format!(
            "UPDATE chapters SET read = 1, last_page_read = 0, read_at = ?1
             WHERE manga_id IN ({list}) AND read = 0 AND chapter_number >= 0 AND chapter_number <= ?2"
        ),
        params![now(), upto],
    )?)
}

fn infos(conn: &Connection) -> Result<Vec<Info>> {
    Kind::ALL
        .into_iter()
        .map(|kind| {
            let account: Option<Account> = db::state(conn, &key(kind))?.and_then(|text| serde_json::from_str(&text).ok());
            let (score_scale, score_step) = kind.score();
            Ok(Info {
                id: kind.id(),
                name: kind.name(),
                login: kind.login(),
                signed_in: account.is_some(),
                user_name: account.map(|account| account.user_name),
                statuses: kind.statuses().iter().map(|&(value, label)| Choice { value, label }).collect(),
                score_scale,
                score_step,
            })
        })
        .collect()
}

/// Moves a link forward to the chapter reached, never back, and its status along with it.
/// Says whether anything changed.
fn advance(kind: Kind, track: &mut Track, reached: f64) -> bool {
    if reached <= track.last_chapter_read {
        return false;
    }
    track.last_chapter_read = reached;
    if track.total_chapters > 0 && reached >= track.total_chapters as f64 {
        track.status = kind.completed();
    } else if track.status == kind.planned() {
        track.status = kind.reading();
    }
    true
}

/// Takes over what the tracker holds.
fn adopt(track: &mut Track, entry: Entry) {
    track.library_id = entry.library_id;
    track.status = entry.status;
    track.last_chapter_read = entry.last_chapter_read;
    track.score = entry.score;
    if entry.total_chapters > 0 {
        track.total_chapters = entry.total_chapters;
    }
}

// ---------- the service ----------

#[derive(Clone)]
pub struct Trackers {
    db: Db,
    http: Client,
    /// Progress goes out for one title at a time.
    sending: Arc<Mutex<()>>,
    /// One trade of an old token for a new one at a time: the old one works once.
    renewing: Arc<Mutex<()>>,
}

impl Trackers {
    pub fn new(db: Db) -> Result<Self> {
        let http = Client::builder()
            .user_agent(concat!("Mango/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| AppError::internal(e.to_string()))?;
        Ok(Self { db, http, sending: Arc::default(), renewing: Arc::default() })
    }

    /// Every tracker, and who is signed in to it.
    pub async fn list(&self) -> Result<Vec<Info>> {
        self.db.call(|conn| infos(conn)).await
    }

    /// The trackers and a title's links to them. From the database alone.
    pub async fn view(&self, manga: i64) -> Result<View> {
        self.db.call(move |conn| Ok(View { trackers: infos(conn)?, tracks: tracks(conn, manga)? })).await
    }

    async fn stored(&self, kind: Kind) -> Result<Option<Account>> {
        let text = self.db.call(move |conn| db::state(conn, &key(kind))).await?;
        Ok(text.and_then(|text| serde_json::from_str(&text).ok()))
    }

    async fn keep(&self, kind: Kind, account: &Account) -> Result<()> {
        let text = serde_json::to_string(account).map_err(|e| AppError::internal(e.to_string()))?;
        self.db.call(move |conn| db::set_state(conn, &key(kind), &text)).await
    }

    /// The signed-in account, with a token that is good for a while yet.
    async fn account(&self, kind: Kind) -> Result<Account> {
        let fresh = |account: &Account| account.expires_at == 0 || account.expires_at > now() + 300;
        let account = self.stored(kind).await?.ok_or_else(|| signed_out(kind))?;
        if fresh(&account) {
            return Ok(account);
        }
        let _one = self.renewing.lock().await;
        // Someone else may have renewed it while this call waited.
        let account = self.stored(kind).await?.ok_or_else(|| signed_out(kind))?;
        if fresh(&account) {
            return Ok(account);
        }
        let renewed = match kind {
            Kind::MyAnimeList => mal::renew(&self.http, &account).await?,
            Kind::Kitsu => kitsu::renew(&self.http, &account).await?,
            Kind::AniList | Kind::MangaUpdates => return Ok(account),
        };
        self.keep(kind, &renewed).await?;
        Ok(renewed)
    }

    /// Starts a sign-in that goes through the tracker's own page. Answers with that page's address.
    pub async fn authorize(&self, kind: Kind, client_id: String, redirect: String) -> Result<String> {
        if kind != Kind::MyAnimeList {
            return Err(AppError::bad_request(format!("{} has no page to sign in through", kind.name())));
        }
        let client_id = client_id.trim().to_owned();
        if client_id.is_empty() || !(redirect.starts_with("http://") || redirect.starts_with("https://")) {
            return Err(AppError::bad_request("A client id and the address to come back to are needed"));
        }
        let pending = Pending { client_id, verifier: secret(48)?, state: secret(16)?, redirect };
        let url = mal::authorize_url(&pending.client_id, &pending.verifier, &pending.state, &pending.redirect)?;
        let text = serde_json::to_string(&pending).map_err(|e| AppError::internal(e.to_string()))?;
        self.db.call(move |conn| db::set_state(conn, PENDING, &text)).await?;
        Ok(url)
    }

    pub async fn login(&self, kind: Kind, given: Credentials) -> Result<()> {
        let account = match kind {
            Kind::AniList => anilist::login(&self.http, &given.token).await?,
            Kind::Kitsu => kitsu::login(&self.http, given.username.trim(), &given.password).await?,
            Kind::MangaUpdates => mangaupdates::login(&self.http, given.username.trim(), &given.password).await?,
            Kind::MyAnimeList => {
                let started = self.db.call(|conn| db::state(conn, PENDING)).await?;
                let pending: Pending = started
                    .and_then(|text| serde_json::from_str(&text).ok())
                    .ok_or_else(|| AppError::new(409, "tracker_login", "Start the sign-in to MyAnimeList again"))?;
                if given.code.is_empty() || given.state != pending.state {
                    return Err(AppError::new(422, "tracker_login", "This is not the answer to the sign-in Mango started. Start it again."));
                }
                let account = mal::login(&self.http, &pending.client_id, &given.code, &pending.verifier, &pending.redirect).await?;
                self.db.call(|conn| db::forget_state(conn, PENDING)).await?;
                account
            }
        };
        self.keep(kind, &account).await
    }

    /// Forgets the account. The links stay, and go on when someone signs in again.
    pub async fn logout(&self, kind: Kind) -> Result<()> {
        self.db.call(move |conn| db::forget_state(conn, &key(kind))).await
    }

    pub async fn search(&self, kind: Kind, query: &str) -> Result<Vec<Found>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        match kind {
            Kind::AniList => anilist::search(&self.http, query).await,
            Kind::Kitsu => kitsu::search(&self.http, query).await,
            Kind::MangaUpdates => mangaupdates::search(&self.http, query).await,
            Kind::MyAnimeList => mal::search(&self.http, &self.account(kind).await?, query).await,
        }
    }

    async fn find(&self, kind: Kind, account: &Account, media: i64) -> Result<Option<Entry>> {
        match kind {
            Kind::AniList => anilist::find(&self.http, account, media).await,
            Kind::MyAnimeList => mal::find(&self.http, account, media).await,
            Kind::Kitsu => kitsu::find(&self.http, account, media).await,
            Kind::MangaUpdates => mangaupdates::find(&self.http, account, media).await,
        }
    }

    /// Sends a link's state, making the entry when there is none. Answers with the entry's id.
    async fn save(&self, kind: Kind, account: &Account, track: &Track) -> Result<i64> {
        match kind {
            Kind::AniList => anilist::save(&self.http, account, track).await,
            Kind::MyAnimeList => mal::save(&self.http, account, track).await,
            Kind::Kitsu => kitsu::save(&self.http, account, track).await,
            Kind::MangaUpdates => mangaupdates::save(&self.http, account, track).await,
        }
    }

    /// Brings a link and the tracker's entry together: the tracker's status and score, the
    /// further chapter of the two. Sends Mango's side when it is ahead or the entry is new.
    async fn settle(&self, kind: Kind, account: &Account, track: &mut Track, reached: f64) -> Result<()> {
        let theirs = self.find(kind, account, track.media_id).await?;
        let target = track.last_chapter_read.max(reached);
        let known = theirs.is_some();
        if let Some(entry) = theirs {
            adopt(track, entry);
        }
        if advance(kind, track, target) || !known {
            track.library_id = self.save(kind, account, track).await?;
        }
        track.error = None;
        Ok(())
    }

    /// Links a title to the search result picked for it.
    pub async fn bind(&self, manga: i64, kind: Kind, pick: Pick) -> Result<Track> {
        if pick.media_id <= 0 {
            return Err(AppError::bad_request("Which title on the tracker?"));
        }
        let account = self.account(kind).await?;
        let reached = self
            .db
            .call(move |conn| {
                let known: Option<i64> = conn.query_row("SELECT id FROM manga WHERE id = ?1", [manga], |r| r.get(0)).optional()?;
                known.ok_or_else(|| AppError::not_found("No such title"))?;
                reached(conn, manga)
            })
            .await?;
        let mut track = Track {
            tracker: kind.id(),
            library_id: 0,
            media_id: pick.media_id,
            // It is shown as a link, so it has to be one.
            url: Some(pick.url).filter(|url| url.starts_with("https://")).unwrap_or_default(),
            title: pick.title.chars().take(300).collect(),
            last_chapter_read: 0.0,
            total_chapters: pick.total_chapters.max(0),
            score: 0.0,
            status: kind.planned(),
            error: None,
        };
        self.settle(kind, &account, &mut track, reached).await?;
        self.db
            .call(move |conn| {
                store(conn, manga, &track)?;
                Ok(track)
            })
            .await
    }

    /// Changes a link's status, score or chapter, on the tracker first.
    pub async fn edit(&self, manga: i64, kind: Kind, change: Change) -> Result<Track> {
        let account = self.account(kind).await?;
        let mut track = self.db.call(move |conn| track(conn, manga, kind)).await?;
        if let Some(status) = change.status {
            if !kind.statuses().iter().any(|(value, _)| *value == status) {
                return Err(AppError::bad_request(format!("{} has no such status", kind.name())));
            }
            track.status = status;
        }
        if let Some(score) = change.score {
            if !(0.0..=10.0 * kind.score().0).contains(&score) {
                return Err(AppError::bad_request("A score is between 0 and 10"));
            }
            track.score = score;
        }
        if let Some(chapter) = change.last_chapter_read {
            if !(0.0..=MAX_CHAPTER).contains(&chapter) {
                return Err(AppError::bad_request("Not a chapter number"));
            }
            track.last_chapter_read = chapter.floor();
        }
        track.library_id = self.save(kind, &account, &track).await?;
        track.error = None;
        self.db
            .call(move |conn| {
                store(conn, manga, &track)?;
                Ok(track)
            })
            .await
    }

    /// Looks at the tracker's entry again and settles the two sides. Chapters the tracker
    /// says were read elsewhere are marked read here too.
    pub async fn refresh(&self, manga: i64, kind: Kind) -> Result<Track> {
        let account = self.account(kind).await?;
        let (mut track, reached) = self.db.call(move |conn| Ok((track(conn, manga, kind)?, reached(conn, manga)?))).await?;
        self.settle(kind, &account, &mut track, reached).await?;
        self.db
            .call(move |conn| {
                store(conn, manga, &track)?;
                catch_up(conn, manga, track.last_chapter_read)?;
                Ok(track)
            })
            .await
    }

    /// Refreshes every link of every library title, one title at a time. For after a
    /// library update, so what was read on another app shows here. A failing tracker is logged.
    pub async fn pull_all(&self) {
        let linked = self
            .db
            .call(|conn| {
                let mut stmt = conn.prepare("SELECT t.manga_id, t.tracker FROM tracks t JOIN manga m ON m.id = t.manga_id WHERE m.in_library = 1 ORDER BY 1, 2")?;
                let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await
            .unwrap_or_default();
        for (place, (manga, tracker)) in linked.into_iter().enumerate() {
            let Some(kind) = Kind::from_id(tracker) else { continue };
            if place > 0 {
                tokio::time::sleep(BETWEEN_TITLES).await;
            }
            match self.refresh(manga, kind).await {
                Ok(_) => {}
                Err(e) if e.code == "tracker_signed_out" => {}
                Err(e) => tracing::warn!("trackers: {} for title {manga}: {e}", kind.name()),
            }
        }
    }

    /// Takes the link away. The entry on the tracker stays as it is.
    pub async fn unbind(&self, manga: i64, kind: Kind) -> Result<()> {
        let gone = self.db.call(move |conn| Ok(conn.execute("DELETE FROM tracks WHERE manga_id = ?1 AND tracker = ?2", [manga, kind.id()])?)).await?;
        match gone {
            0 => Err(AppError::not_found(format!("This title is not linked to {}", kind.name()))),
            _ => Ok(()),
        }
    }

    /// Call after chapters of a title were marked read. Answers at once; the trackers
    /// hear of it in the background.
    pub fn read_title(&self, manga: i64) {
        let this = self.clone();
        tokio::spawn(async move {
            if let Err(e) = this.follow(manga).await {
                tracing::warn!("trackers: {e}");
            }
        });
    }

    /// The same, for one chapter that was finished.
    pub fn read_chapter(&self, chapter: i64) {
        let this = self.clone();
        tokio::spawn(async move {
            // A chapter of a merged title moves the links of the title it was merged into.
            let manga = this
                .db
                .call(move |conn| {
                    Ok(conn
                        .query_row("SELECT COALESCE(m.merged_into, m.id) FROM chapters c JOIN manga m ON m.id = c.manga_id WHERE c.id = ?1", [chapter], |r| r.get(0))
                        .optional()?)
                })
                .await;
            match manga {
                Ok(Some(manga)) => {
                    if let Err(e) = this.follow(manga).await {
                        tracing::warn!("trackers: {e}");
                    }
                }
                Ok(None) => {}
                Err(e) => tracing::warn!("trackers: {e}"),
            }
        });
    }

    /// Sends a title's links on to where reading has got to. A link whose last sending
    /// failed goes again. Trackers nobody is signed in to are left for later.
    async fn follow(&self, manga: i64) -> Result<()> {
        let (links, reached) = self
            .db
            .call(move |conn| {
                let links = tracks(conn, manga)?;
                let reached = if links.is_empty() { 0.0 } else { reached(conn, manga)? };
                Ok((links, reached))
            })
            .await?;
        if links.is_empty() {
            return Ok(());
        }
        let _one = self.sending.lock().await;
        for mut track in links {
            let Some(kind) = Kind::from_id(track.tracker) else { continue };
            if !advance(kind, &mut track, reached) && track.error.is_none() {
                continue;
            }
            let sent = match self.account(kind).await {
                Ok(account) => self.save(kind, &account, &track).await,
                Err(e) if e.code == "tracker_signed_out" => continue,
                Err(e) => Err(e),
            };
            match sent {
                Ok(library_id) => (track.library_id, track.error) = (library_id, None),
                Err(e) => {
                    tracing::warn!("trackers: {} for title {manga}: {e}", kind.name());
                    track.error = Some(e.message);
                }
            }
            self.db.call(move |conn| store(conn, manga, &track)).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn database() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();
        crate::db::migrate(&mut conn).unwrap();
        conn.execute_batch(
            "INSERT INTO manga (id, source_id, url, title) VALUES (1, 7, '/a', 'Alpha');
             INSERT INTO chapters (manga_id, url, name, chapter_number, source_order, fetched_at, read) VALUES
                 (1, '/a/3', 'Three', 3, 0, 1, 0), (1, '/a/2', 'Two and a half', 2.5, 1, 1, 1), (1, '/a/1', 'One', 1, 2, 1, 1), (1, '/a/x', 'Extra', -1, 3, 1, 1);",
        )
        .unwrap();
        conn
    }

    fn link(kind: Kind, status: i32, last: f64, total: i64) -> Track {
        Track {
            tracker: kind.id(),
            library_id: 5,
            media_id: 9,
            url: "https://example.org/9".into(),
            title: "Alpha".into(),
            last_chapter_read: last,
            total_chapters: total,
            score: 0.0,
            status,
            error: None,
        }
    }

    #[test]
    fn trackers_keep_mihons_numbers() {
        for kind in Kind::ALL {
            assert_eq!(Kind::from_id(kind.id()), Some(kind));
            for status in [kind.reading(), kind.completed(), kind.planned()] {
                assert!(kind.statuses().iter().any(|(value, _)| *value == status), "{} has status {status}", kind.name());
            }
        }
        assert_eq!(Kind::ALL.map(Kind::id), [2, 1, 3, 7]);
        assert_eq!(Kind::from_id(4), None, "a tracker Mango does not speak to");
    }

    #[test]
    fn progress_only_moves_forward() {
        let mut track = link(Kind::AniList, 5, 0.0, 12);
        assert!(advance(Kind::AniList, &mut track, 3.0));
        assert_eq!((track.last_chapter_read, track.status), (3.0, 1), "a planned title that is read is being read");
        assert!(!advance(Kind::AniList, &mut track, 2.0));
        assert!(!advance(Kind::AniList, &mut track, 3.0));
        assert_eq!(track.last_chapter_read, 3.0, "never back");

        track.status = 3;
        assert!(advance(Kind::AniList, &mut track, 4.0));
        assert_eq!(track.status, 3, "a paused title stays paused");
        assert!(advance(Kind::AniList, &mut track, 12.0));
        assert_eq!(track.status, 2, "the last chapter completes it");

        let mut list = link(Kind::MangaUpdates, 1, 0.0, 0);
        assert!(advance(Kind::MangaUpdates, &mut list, 40.0));
        assert_eq!(list.status, 0, "from the wish list to the reading list; no total, so never complete");
    }

    #[test]
    fn links_are_stored_and_found() {
        let conn = database();
        // As a backup brought it: with a date Mango does not handle.
        conn.execute("INSERT INTO tracks (manga_id, tracker, media_id, started_at) VALUES (1, 2, 9, 1700000000000)", []).unwrap();
        let mut track = link(Kind::AniList, 1, 2.0, 12);
        track.error = Some("AniList did not answer".into());
        store(&conn, 1, &track).unwrap();
        store(&conn, 1, &link(Kind::Kitsu, 5, 0.0, 0)).unwrap();
        assert_eq!(super::track(&conn, 1, Kind::AniList).unwrap(), track);
        assert_eq!(tracks(&conn, 1).unwrap().iter().map(|t| t.tracker).collect::<Vec<_>>(), [2, 3]);
        assert_eq!(super::track(&conn, 1, Kind::MyAnimeList).unwrap_err().code, "not_found");
        let started: i64 = conn.query_row("SELECT started_at FROM tracks WHERE tracker = 2", [], |r| r.get(0)).unwrap();
        assert_eq!(started, 1_700_000_000_000, "what Mango does not change is kept");
    }

    #[test]
    fn reached_is_the_highest_read_chapter() {
        let conn = database();
        assert_eq!(reached(&conn, 1).unwrap(), 2.0, "whole chapters; the unread third does not count");
        conn.execute("UPDATE chapters SET read = 0", []).unwrap();
        assert_eq!(reached(&conn, 1).unwrap(), 0.0);
        assert_eq!(reached(&conn, 99).unwrap(), 0.0);
    }

    #[test]
    fn what_was_read_elsewhere_comes_in() {
        let conn = database();
        conn.execute("UPDATE chapters SET read = 0", []).unwrap();
        assert_eq!(catch_up(&conn, 1, 2.0).unwrap(), 1, "chapter one; not two and a half, not the extra");
        assert_eq!(reached(&conn, 1).unwrap(), 1.0);
        assert_eq!(catch_up(&conn, 1, 2.0).unwrap(), 0, "nothing twice");
        assert_eq!(catch_up(&conn, 1, 0.0).unwrap(), 0);
        conn.execute_batch(
            "INSERT INTO manga (id, source_id, url, title, merged_into) VALUES (2, 8, '/b', 'Alpha', 1);
             INSERT INTO chapters (manga_id, url, name, chapter_number, source_order, fetched_at, read) VALUES (2, '/b/9', 'Nine', 9, 0, 1, 1);",
        )
        .unwrap();
        assert_eq!(reached(&conn, 1).unwrap(), 9.0, "a merged title's chapters count");
    }

    #[test]
    fn accounts_show_without_their_tokens() {
        let conn = database();
        assert!(infos(&conn).unwrap().iter().all(|info| !info.signed_in));
        let account = Account { token: "secret-token".into(), user_name: "reader".into(), ..Account::default() };
        db::set_state(&conn, &key(Kind::Kitsu), &serde_json::to_string(&account).unwrap()).unwrap();
        let shown = serde_json::to_string(&infos(&conn).unwrap()).unwrap();
        assert!(shown.contains(r#""signedIn":true,"userName":"reader""#));
        assert!(!shown.contains("secret-token"));
        assert!(crate::settings::all(&conn).unwrap().is_empty(), "and the settings API has no way to them");
    }

    #[test]
    fn answers_are_read_loosely() {
        assert_eq!(plain(&json!("A <i>quiet</i> town.<br><br>\nThen it rains.")), "A quiet town. Then it rains.");
        assert_eq!(plain(&json!("ab".repeat(200))).chars().count(), 281);
        assert_eq!(plain(&Value::Null), "");
        assert_eq!((whole(&json!("42")), whole(&json!(42)), whole(&json!(null))), (42, 42, 0));
        assert_eq!((year(&json!("2019-03-01")), year(&json!(2019)), year(&json!("")), year(&json!(0))), (Some(2019), Some(2019), None, None));
        assert_eq!(label(&json!("ONE_SHOT")), "one shot");
        assert_eq!(said(&json!({ "errors": [{ "message": "Invalid token" }] })), "Invalid token");
        assert_eq!(said(&json!({ "status": "exception", "reason": "No such user" })), "No such user");
        assert_eq!(said(&Value::Null), "no reason given");
        assert_eq!(secret(48).unwrap().len(), 64);
    }
}
