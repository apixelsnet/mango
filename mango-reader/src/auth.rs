//! Login: accounts, passwords, session cookies.
//!
//! A password is kept as an Argon2 hash. A browser that signs in gets a random
//! token in a cookie; the database holds only the token's hash. The first account
//! is the owner's: until it has a password, the API answers nothing but the call
//! that sets one. Reading apps that cannot keep a cookie send name and password
//! with every call instead; see `basic`. Other accounts are an admin's to add,
//! or, where the owner switched that on, their holders' to make with an admin's
//! invitation; see `join`.

use crate::{
    db::{self, Db},
    error::{AppError, Result},
    spaces::Spaces,
};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{ConnectInfo, Path, Query, Request, State},
    http::{header, HeaderMap, HeaderValue},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use base64::Engine;
use hmac::{Hmac, Mac};
use md5::Md5;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    hash::Hash,
    net::{IpAddr, Ipv6Addr, SocketAddr},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard,
    },
};

/// The first account. It runs the server and cannot be removed.
pub const OWNER: i64 = 1;
const OWNER_NAME: &str = "owner";
/// A session ends this long after it was last used.
const SESSION_SECS: i64 = 30 * 86_400;
/// A session's end is pushed out at most this often, to spare the database.
const TOUCH_SECS: i64 = 86_400;
/// The cookie outlives the session; the server decides when a session is over.
const COOKIE_SECS: i64 = 400 * 86_400;
const MIN_PASSWORD: usize = 8;
const MAX_PASSWORD: usize = 256;
const MAX_NAME: usize = 32;
/// Wrong passwords an address may send before it has to wait.
const FREE_TRIES: u32 = 5;
const LONGEST_WAIT: i64 = 300;
/// A name and password sent with a call are checked again after this long.
const BASIC_SECS: i64 = 600;
/// An invitation holds this many days unless its maker says otherwise, and never more than the second.
const INVITE_DAYS: i64 = 7;
const MAX_INVITE_DAYS: i64 = 90;
const MAX_NOTE: usize = 80;
/// Where the highest number an account ever had is kept. A number is not given out
/// twice: it names the account's folder, which may outlive the account for a moment.
const LAST_ID: &str = "users.last_id";
/// Endings of names that only a local network resolves.
const LOCAL_ENDINGS: [&str; 5] = [".localhost", ".local", ".lan", ".internal", ".home.arpa"];
/// Seconds one authenticator code holds (RFC 6238).
const STEP_SECS: i64 = 30;
/// A secret shown for setting an authenticator up waits this long for its first code.
const PENDING_SECS: i64 = 900;
const RECOVERY_CODES: usize = 10;
/// App passwords one account may have.
const MAX_APPS: i64 = 20;
const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
/// What an app password is made of: no `l`, `o`, `0` or `1`, which read alike.
const APP_LETTERS: &[u8; 32] = b"abcdefghijkmnpqrstuvwxyz23456789";
const APP_LENGTH: usize = 16;

/// Who a request comes from.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Who {
    pub id: i64,
    pub name: String,
    pub admin: bool,
}

impl Who {
    /// An error unless this account may change what every account shares.
    pub fn admin(&self) -> Result<()> {
        if self.admin {
            return Ok(());
        }
        Err(AppError::new(403, "admin_only", "Only an admin can do that"))
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub admin: bool,
    created_at: i64,
    /// False for the owner after `mango reset-password`, until a new one is set.
    has_password: bool,
    /// Signing in takes a code from an authenticator app as well.
    two_step: bool,
}

/// What an admin may change about an account. Whatever is left out stays.
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccountChange {
    name: Option<String>,
    password: Option<String>,
    admin: Option<bool>,
    /// Only `false`: for a holder who lost both the authenticator and the recovery codes.
    two_step: Option<bool>,
}

/// A password Mango made for one reading app.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPassword {
    pub id: i64,
    name: String,
    created_at: i64,
    used_at: Option<i64>,
    /// The password. Only the answer that makes it carries it: Mango keeps its hashes.
    #[serde(skip_serializing_if = "Option::is_none")]
    password: Option<String>,
}

/// An authenticator being set up: the secret to add to it, as text and as a link.
#[derive(Serialize)]
pub struct Enrolment {
    secret: String,
    uri: String,
}

/// Whether an account has two-factor authentication, and how many recovery codes it has left.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TwoStep {
    on: bool,
    codes_left: i64,
}

/// What lets one person make one account.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Invite {
    pub id: i64,
    /// Who it is for, in the words of the admin who made it.
    note: String,
    created_at: i64,
    expires_at: i64,
    used_at: Option<i64>,
    /// The account made with it, while there is one.
    used_by: Option<String>,
    /// The code. Only the answer that makes the invitation carries it: Mango keeps its hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
}

/// How Mango is reached and who may come in, as the owner described it.
#[derive(Clone, Default)]
pub struct Front {
    /// Names Mango goes by; see `Auth::answers`.
    pub hosts: Vec<String>,
    /// A proxy stands in front and writes each caller's address into `X-Forwarded-For`.
    pub proxied: bool,
    /// Whoever holds an admin's invitation may make an account; see `Auth::join`.
    pub signup: bool,
    /// Where a reader's account is kept, for whoever runs this Mango for others.
    pub account: Option<String>,
}

pub struct Auth {
    db: Db,
    /// False when the owner switched login off: every request is the owner's.
    on: bool,
    /// Named after the port, so two Mangos on one machine keep separate sessions.
    cookie: String,
    front: Front,
    /// What setting the first password from another machine takes, while there is none.
    code: Option<String>,
    /// Made anew at every start and kept in the data folder; see `proof`.
    instance: String,
    has_password: AtomicBool,
    /// The accounts, as the database has them.
    accounts: Mutex<Vec<Account>>,
    /// Token hash to the account and the time the session ends. The database holds the same.
    sessions: Mutex<HashMap<String, (i64, i64)>>,
    /// Names and passwords that were checked a moment ago: hash of the header to the account and how long it holds.
    basics: Mutex<HashMap<String, (i64, i64)>>,
    /// Wrong passwords per address: how many, and when the last one came.
    strikes: Mutex<HashMap<IpAddr, (u32, i64)>>,
    /// Wrong codes per account. Kept apart from the addresses': a right password clears
    /// those, and would otherwise buy whoever has it more tries at the code.
    misses: Mutex<HashMap<i64, (u32, i64)>>,
    /// Authenticators being set up: account to secret and when the offer ends.
    pending: Mutex<HashMap<i64, (Vec<u8>, i64)>>,
    /// One hash at a time: each takes 19 MB while it runs.
    hashing: tokio::sync::Mutex<()>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// Whether this Mango asks for a password at all.
    login: bool,
    /// No password has been set yet: the next step is to set one.
    setup: bool,
    /// Setting it takes the setup code as well: the caller is not on Mango's own machine.
    setup_code: bool,
    /// Whoever holds an invitation can make an account here: the page offers it.
    signup: bool,
    signed_in: bool,
    /// More than one account exists, so signing in takes a name.
    named: bool,
    name: Option<String>,
    admin: bool,
    /// Where the signed-in reader's account is kept, when whoever runs this Mango keeps one.
    #[serde(skip_serializing_if = "Option::is_none")]
    account: Option<String>,
    /// The answer to a challenge; see `Auth::proof`.
    #[serde(skip_serializing_if = "Option::is_none")]
    proof: Option<String>,
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn random<const N: usize>() -> Result<[u8; N]> {
    let mut bytes = [0u8; N];
    getrandom::getrandom(&mut bytes).map_err(|e| AppError::internal(format!("no random bytes: {e}")))?;
    Ok(bytes)
}

/// Sixteen characters in groups of four: read once from the log, typed once into the page.
fn new_code() -> Result<String> {
    let text = hex::encode(random::<8>()?);
    Ok(format!("{}-{}-{}-{}", &text[..4], &text[4..8], &text[8..12], &text[12..]))
}

/// A code as it is compared: its letters and digits, however it was typed.
fn plain_code(text: &str) -> String {
    text.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

/// Whether two setup codes are the same, however they were typed. They are compared by
/// hash, so the time it takes says nothing about where they differ.
fn same_code(given: &str, code: &str) -> bool {
    token_hash(&plain_code(given)) == token_hash(&plain_code(code))
}

/// What the Mango that keeps `instance` answers to `challenge`.
pub fn answer_to(instance: &str, challenge: &str) -> String {
    token_hash(&format!("{instance}:{challenge}"))
}

/// What wrong passwords are counted by: an address, or for IPv6 the /64 its holder was given whole.
fn bucket(from: IpAddr) -> IpAddr {
    match from {
        IpAddr::V6(long) => match long.to_ipv4_mapped() {
            Some(short) => short.into(),
            None => {
                let parts = long.segments();
                Ipv6Addr::new(parts[0], parts[1], parts[2], parts[3], 0, 0, 0, 0).into()
            }
        },
        short => short,
    }
}

fn check_length(password: &str) -> Result<()> {
    match password.chars().count() {
        n if n < MIN_PASSWORD => Err(AppError::bad_request(format!("A password needs at least {MIN_PASSWORD} characters"))),
        _ if password.len() > MAX_PASSWORD => Err(AppError::bad_request("That password is too long")),
        _ => Ok(()),
    }
}

/// A name as it is stored. A colon would end it early where name and password travel as one text.
fn clean_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME || name.chars().any(|c| c == ':' || c.is_control()) {
        return Err(AppError::bad_request(format!("A name is 1 to {MAX_NAME} characters, without a colon")));
    }
    Ok(name.to_owned())
}

fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::encode_b64(&random::<16>()?).map_err(|e| AppError::internal(format!("salt: {e}")))?;
    let hash = Argon2::default().hash_password(password.as_bytes(), &salt);
    hash.map(|hash| hash.to_string()).map_err(|e| AppError::internal(format!("hashing: {e}")))
}

/// Text as RFC 4648 base32 without padding, as authenticator apps take a secret.
fn base32(bytes: &[u8]) -> String {
    let mut out = String::new();
    let (mut buffer, mut bits) = (0u32, 0);
    for &byte in bytes {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(BASE32[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(BASE32[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// The six digits an authenticator shows for `secret` in the step `step` (RFC 6238, SHA-1).
fn totp(secret: &[u8], step: i64) -> u32 {
    let mut mac = <Hmac<Sha1> as Mac>::new_from_slice(secret).expect("HMAC takes a key of any length");
    mac.update(&(step as u64).to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let at = usize::from(digest[19] & 0x0f);
    (u32::from_be_bytes([digest[at], digest[at + 1], digest[at + 2], digest[at + 3]]) & 0x7fff_ffff) % 1_000_000
}

/// The step a typed code belongs to, allowing one either side for a clock that is a little off.
fn code_step(secret: &[u8], typed: &str, now: i64) -> Option<i64> {
    let digits: String = typed.chars().filter(|c| !c.is_whitespace()).collect();
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let typed: u32 = digits.parse().ok()?;
    let step = now / STEP_SECS;
    (step - 1..=step + 1).find(|&at| totp(secret, at) == typed)
}

/// What KOReader sends in place of a password: the MD5 of it, in hex.
fn md5_hex(text: &str) -> String {
    hex::encode(Md5::digest(text.as_bytes()))
}

/// Seconds a key with this many misses must still wait before it may try again.
fn wait_in<K: Hash + Eq>(misses: &Mutex<HashMap<K, (u32, i64)>>, key: &K) -> i64 {
    let Some((count, last)) = locked(misses).get(key).copied() else { return 0 };
    if count < FREE_TRIES {
        return 0;
    }
    let pause = (1i64 << (count - FREE_TRIES).min(9)).min(LONGEST_WAIT);
    (last + pause - db::now()).max(0)
}

fn strike_in<K: Hash + Eq>(misses: &Mutex<HashMap<K, (u32, i64)>>, key: K) {
    let now = db::now();
    let mut misses = locked(misses);
    if misses.len() > 4096 {
        misses.retain(|_, (_, last)| now - *last < 3600);
    }
    let entry = misses.entry(key).or_insert((0, now));
    // An hour of quiet wipes the slate.
    if now - entry.1 > 3600 {
        entry.0 = 0;
    }
    *entry = (entry.0 + 1, now);
}

fn too_many(wait: i64) -> AppError {
    AppError::new(429, "too_many_tries", format!("Too many wrong tries. Try again in {wait} seconds."))
}

fn accounts(conn: &Connection) -> Result<Vec<Account>> {
    let mut stmt = conn.prepare("SELECT id, name, admin, created_at, password IS NOT NULL, totp IS NOT NULL FROM users ORDER BY id")?;
    let rows = stmt.query_map([], |r| {
        Ok(Account { id: r.get(0)?, name: r.get(1)?, admin: r.get(2)?, created_at: r.get(3)?, has_password: r.get(4)?, two_step: r.get(5)? })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Takes an account's two-factor authentication away, with its recovery codes.
fn drop_two_step(conn: &Connection, user: i64) -> Result<()> {
    conn.execute("UPDATE users SET totp = NULL, totp_step = 0 WHERE id = ?1", [user])?;
    conn.execute("DELETE FROM recovery_codes WHERE user_id = ?1", [user])?;
    Ok(())
}

/// Adds an account under a number no account had before. The caller commits.
fn insert_account(tx: &Connection, name: &str, hash: &str, admin: bool) -> Result<i64> {
    let last: i64 = db::state(tx, LAST_ID)?.and_then(|text| text.parse().ok()).unwrap_or(0);
    // The owner's place is kept, even before the owner exists.
    let id: i64 = tx.query_row("SELECT max(1, ?1, coalesce(max(id), 0)) + 1 FROM users", [last], |r| r.get(0))?;
    tx.execute("INSERT INTO users (id, name, password, admin, created_at) VALUES (?1, ?2, ?3, ?4, ?5)", params![id, name, hash, admin, db::now()])
        .map_err(name_taken)?;
    db::set_state(tx, LAST_ID, &id.to_string())?;
    Ok(id)
}

/// Removes the owner's password and two-factor authentication, and signs the owner out. For
/// `mango reset-password`, which is for an owner who cannot get in.
pub fn reset(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE users SET password = NULL WHERE id = ?1", [OWNER])?;
    drop_two_step(conn, OWNER)?;
    conn.execute("DELETE FROM sessions WHERE user_id = ?1", [OWNER])?;
    Ok(())
}

impl Auth {
    /// `first_password` is used only while the owner has none.
    pub async fn open(db: Db, on: bool, port: u16, first_password: Option<String>, front: Front) -> Result<Arc<Self>> {
        let (accounts, sessions) = db
            .call(|conn| {
                conn.execute("DELETE FROM sessions WHERE expires_at <= ?1 OR user_id NOT IN (SELECT id FROM users)", [db::now()])?;
                conn.execute("DELETE FROM invites WHERE used_at IS NULL AND expires_at <= ?1", [db::now()])?;
                let mut stmt = conn.prepare("SELECT token_hash, user_id, expires_at FROM sessions")?;
                let sessions = stmt.query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?.collect::<rusqlite::Result<HashMap<String, (i64, i64)>>>()?;
                Ok((accounts(conn)?, sessions))
            })
            .await?;
        let has_password = accounts.iter().any(|account| account.id == OWNER && account.has_password);
        let auth = Arc::new(Self {
            db,
            on,
            cookie: format!("mango_{port}"),
            front,
            code: (on && !has_password && first_password.is_none()).then(new_code).transpose()?,
            instance: hex::encode(random::<32>()?),
            has_password: AtomicBool::new(has_password),
            accounts: Mutex::new(accounts),
            sessions: Mutex::new(sessions),
            basics: Mutex::new(HashMap::new()),
            strikes: Mutex::new(HashMap::new()),
            misses: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            hashing: tokio::sync::Mutex::new(()),
        });
        if let (false, Some(password)) = (has_password, first_password) {
            check_length(&password)?;
            auth.store_owner(password).await?;
            tracing::info!("the password was set from MANGO_PASSWORD");
        }
        Ok(auth)
    }

    /// Whether the owner has a password: until then nobody gets in.
    pub fn has_password(&self) -> bool {
        self.has_password.load(Ordering::Acquire)
    }

    /// Whether a request may be answered under the name it calls Mango by. While a
    /// request can act without signing in (login switched off, or no password set yet),
    /// a page on another site could reach Mango by pointing a name of its own at it; the
    /// browser then takes the two for one site. So until signing in stands in the way,
    /// only names such a page cannot have are answered.
    pub fn answers(&self, headers: &HeaderMap) -> bool {
        if self.on && self.has_password() {
            return true;
        }
        // A proxy may put its own name for Mango in `Host`, and the one the browser used beside it.
        let named = ["host", "x-forwarded-host"].into_iter().filter_map(|name| headers.get(name)?.to_str().ok());
        named.flat_map(|hosts| hosts.split(',')).all(|host| known_host(host, &self.front.hosts))
    }

    /// The code that setting the first password takes, while there is no password.
    pub fn setup_code(&self) -> Option<&str> {
        self.code.as_deref().filter(|_| !self.has_password())
    }

    /// What this Mango keeps in its data folder to be told by.
    pub fn instance(&self) -> &str {
        &self.instance
    }

    /// Shows that this is the Mango that wrote `instance` into its data folder, to someone
    /// who can read that file: `mango health`, and through it the desktop shell. Without
    /// it, whatever else listens on the port would pass for Mango.
    fn proof(&self, challenge: &str) -> Option<String> {
        let fits = challenge.len() == 32 && challenge.bytes().all(|b| b.is_ascii_hexdigit());
        fits.then(|| answer_to(&self.instance, challenge))
    }

    /// Whether a call comes from Mango's own machine, with no proxy on the way.
    pub fn local(peer: IpAddr, headers: &HeaderMap) -> bool {
        let relayed = ["forwarded", "x-forwarded-for", "x-forwarded-host", "x-real-ip"].into_iter().any(|name| headers.contains_key(name));
        bucket(peer).is_loopback() && !relayed
    }

    /// The address a call comes from: the one that connected or, behind a proxy the
    /// owner vouched for, the last one the proxy wrote down.
    pub fn caller(&self, peer: IpAddr, headers: &HeaderMap) -> IpAddr {
        let written = || {
            let lines = headers.get_all("x-forwarded-for").iter().filter_map(|value| value.to_str().ok());
            lines.flat_map(|line| line.split(',')).filter_map(|address| address.trim().parse::<IpAddr>().ok()).last()
        };
        self.front.proxied.then(written).flatten().unwrap_or(peer)
    }

    /// Every account's id. The owner's is there before the owner has set a password.
    pub fn ids(&self) -> Vec<i64> {
        let mut ids: Vec<i64> = locked(&self.accounts).iter().map(|account| account.id).collect();
        if !ids.contains(&OWNER) {
            ids.insert(0, OWNER);
        }
        ids
    }

    pub fn list(&self) -> Vec<Account> {
        locked(&self.accounts).clone()
    }

    /// `local` says whether the caller is on Mango's own machine; see `local`.
    pub fn status(&self, headers: &HeaderMap, local: bool, challenge: Option<&str>) -> Status {
        let who = self.who(headers);
        let setup = self.on && !self.has_password();
        Status {
            login: self.on,
            setup,
            setup_code: setup && !local,
            signup: self.signup() && !setup,
            proof: challenge.and_then(|challenge| self.proof(challenge)),
            signed_in: who.is_some(),
            named: self.on && locked(&self.accounts).len() > 1,
            admin: who.as_ref().is_some_and(|who| who.admin),
            account: who.as_ref().and(self.front.account.clone()),
            name: who.map(|who| who.name),
        }
    }

    /// Sets the owner's password and signs the caller in. Works while there is none.
    /// From another machine it takes the setup code: Mango wrote it where only someone
    /// who runs this Mango can read it, so the first to open the page is not the owner by that alone.
    pub async fn setup(&self, password: String, code: &str, local: bool, agent: String) -> Result<String> {
        if !self.on {
            return Err(AppError::bad_request("Login is switched off"));
        }
        check_length(&password)?;
        if !self.has_password() && !local && !self.code.as_deref().is_some_and(|kept| same_code(code, kept)) {
            let why = "That is not the setup code. Mango wrote it to its log when it started, and into the file setup-code in its data folder.";
            return Err(AppError::new(403, "setup_code", why));
        }
        // Claimed before the slow hashing, so two callers cannot both set it.
        if self.has_password.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return Err(AppError::new(403, "already_set", "A password is set already. Sign in with it."));
        }
        if let Err(why) = self.store_owner(password).await {
            self.has_password.store(false, Ordering::Release);
            return Err(why);
        }
        self.start_session(OWNER, agent).await
    }

    /// `name` may be left empty while there is one account only. An account with two-step
    /// sign-in takes `code` as well: one from its authenticator, or a recovery code.
    pub async fn login(&self, from: IpAddr, name: &str, password: String, code: &str, agent: String) -> Result<String> {
        if !self.has_password() {
            return Err(setup_required());
        }
        let who = self.check_password(from, name, password).await?;
        self.second_step(who.id, code).await?;
        self.start_session(who.id, agent).await
    }

    fn two_step(&self, user: i64) -> bool {
        self.account(user).is_ok_and(|account| account.two_step)
    }

    /// The second step for an account that has one. Wrong codes count against the
    /// account, wherever they come from: the password got them this far.
    async fn second_step(&self, user: i64, code: &str) -> Result<()> {
        if !self.two_step(user) {
            return Ok(());
        }
        if code.trim().is_empty() {
            return Err(AppError::new(401, "code_required", "Enter the code from your authenticator app"));
        }
        let wait = wait_in(&self.misses, &user);
        if wait > 0 {
            return Err(too_many(wait));
        }
        let (typed, now) = (code.to_owned(), db::now());
        let right = self
            .db
            .call(move |conn| {
                let kept = conn.query_row("SELECT totp, totp_step FROM users WHERE id = ?1", [user], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, i64>(1)?)));
                let Some((Some(secret), last)) = kept.optional()? else { return Ok(true) };
                let secret = hex::decode(secret).map_err(|e| AppError::internal(format!("a stored secret: {e}")))?;
                if let Some(step) = code_step(&secret, &typed, now) {
                    // A code opens once: one seen at the door is no good a second time.
                    return Ok(step > last && conn.execute("UPDATE users SET totp_step = ?1 WHERE id = ?2 AND totp_step < ?1", params![step, user])? == 1);
                }
                let spent = conn.execute("DELETE FROM recovery_codes WHERE user_id = ?1 AND code_hash = ?2", params![user, token_hash(&plain_code(&typed))])?;
                Ok(spent == 1)
            })
            .await?;
        if !right {
            strike_in(&self.misses, user);
            return Err(AppError::new(401, "wrong_code", "That code does not open this account"));
        }
        locked(&self.misses).remove(&user);
        Ok(())
    }

    // ---------- two-factor authentication, for the signed-in holder ----------

    pub async fn two_step_status(&self, headers: &HeaderMap) -> Result<TwoStep> {
        let who = self.session(headers).ok_or_else(signed_out)?;
        let left = self.db.call(move |conn| Ok(conn.query_row("SELECT count(*) FROM recovery_codes WHERE user_id = ?1", [who.id], |r| r.get(0))?)).await?;
        Ok(TwoStep { on: self.two_step(who.id), codes_left: left })
    }

    /// A new secret for an authenticator. It counts once a code from it is confirmed.
    pub fn enrol(&self, headers: &HeaderMap) -> Result<Enrolment> {
        let who = self.session(headers).ok_or_else(signed_out)?;
        if self.two_step(who.id) {
            return Err(AppError::new(409, "two_step_on", "Two-factor authentication is on already"));
        }
        let secret = random::<20>()?;
        let now = db::now();
        let mut pending = locked(&self.pending);
        pending.retain(|_, (_, until)| *until > now);
        pending.insert(who.id, (secret.to_vec(), now + PENDING_SECS));
        let text = base32(&secret);
        let label = crate::opds::percent(&format!("Mango:{}", who.name));
        Ok(Enrolment { uri: format!("otpauth://totp/{label}?secret={text}&issuer=Mango&algorithm=SHA1&digits=6&period={STEP_SECS}"), secret: text })
    }

    /// Switches two-factor authentication on with the first code from the new authenticator, signs
    /// out every other browser of the account, and answers with its recovery codes.
    pub async fn confirm(&self, headers: &HeaderMap, code: &str) -> Result<Vec<String>> {
        let who = self.session(headers).ok_or_else(signed_out)?;
        let keep = self.token(headers).map(|token| token_hash(&token));
        let now = db::now();
        let secret = locked(&self.pending).get(&who.id).filter(|(_, until)| *until > now).map(|(secret, _)| secret.clone());
        let Some(secret) = secret else {
            return Err(AppError::new(409, "no_secret", "Start again: that secret is too old"));
        };
        let wait = wait_in(&self.misses, &who.id);
        if wait > 0 {
            return Err(too_many(wait));
        }
        let Some(step) = code_step(&secret, code, now) else {
            strike_in(&self.misses, who.id);
            return Err(AppError::new(400, "wrong_code", "That is not the code the app shows. Check the phone's clock, and try the next one."));
        };
        locked(&self.pending).remove(&who.id);
        let codes = (0..RECOVERY_CODES).map(|_| new_code()).collect::<Result<Vec<_>>>()?;
        let hashes: Vec<String> = codes.iter().map(|code| token_hash(&plain_code(code))).collect();
        let user = who.id;
        self.db
            .call(move |conn| {
                let tx = conn.transaction()?;
                tx.execute("UPDATE users SET totp = ?1, totp_step = ?2 WHERE id = ?3", params![hex::encode(&secret), step, user])?;
                tx.execute("DELETE FROM recovery_codes WHERE user_id = ?1", [user])?;
                for hash in hashes {
                    tx.execute("INSERT INTO recovery_codes (user_id, code_hash) VALUES (?1, ?2)", params![user, hash])?;
                }
                tx.commit()?;
                Ok(())
            })
            .await?;
        self.reload().await?;
        self.sign_out(user, keep).await?;
        Ok(codes)
    }

    /// New recovery codes in place of the old ones. Takes the password.
    pub async fn new_codes(&self, from: IpAddr, headers: &HeaderMap, password: String) -> Result<Vec<String>> {
        let who = self.session(headers).ok_or_else(signed_out)?;
        self.check_password(from, &who.name, password).await?;
        if !self.two_step(who.id) {
            return Err(AppError::bad_request("Two-factor authentication is off"));
        }
        let codes = (0..RECOVERY_CODES).map(|_| new_code()).collect::<Result<Vec<_>>>()?;
        let hashes: Vec<String> = codes.iter().map(|code| token_hash(&plain_code(code))).collect();
        self.db
            .call(move |conn| {
                let tx = conn.transaction()?;
                tx.execute("DELETE FROM recovery_codes WHERE user_id = ?1", [who.id])?;
                for hash in hashes {
                    tx.execute("INSERT INTO recovery_codes (user_id, code_hash) VALUES (?1, ?2)", params![who.id, hash])?;
                }
                tx.commit()?;
                Ok(())
            })
            .await?;
        Ok(codes)
    }

    /// Switches two-factor authentication off. Takes the password.
    pub async fn leave_two_step(&self, from: IpAddr, headers: &HeaderMap, password: String) -> Result<()> {
        let who = self.session(headers).ok_or_else(signed_out)?;
        self.check_password(from, &who.name, password).await?;
        self.db.call(move |conn| drop_two_step(conn, who.id)).await?;
        self.reload().await
    }

    // ---------- app passwords ----------

    pub async fn apps(&self, headers: &HeaderMap) -> Result<Vec<AppPassword>> {
        let who = self.session(headers).ok_or_else(signed_out)?;
        self.db
            .call(move |conn| {
                let mut stmt = conn.prepare("SELECT id, name, created_at, used_at FROM app_passwords WHERE user_id = ?1 ORDER BY id DESC")?;
                let rows = stmt.query_map([who.id], |r| Ok(AppPassword { id: r.get(0)?, name: r.get(1)?, created_at: r.get(2)?, used_at: r.get(3)?, password: None }))?;
                Ok(rows.collect::<rusqlite::Result<_>>()?)
            })
            .await
    }

    /// Makes a password for one reading app. It is in this answer and nowhere else.
    pub async fn add_app(&self, headers: &HeaderMap, name: &str) -> Result<AppPassword> {
        let who = self.session(headers).ok_or_else(signed_out)?;
        let name: String = name.trim().chars().filter(|c| !c.is_control()).take(MAX_NOTE).collect();
        if name.is_empty() {
            return Err(AppError::bad_request("Name the app the password is for"));
        }
        let password: String = random::<APP_LENGTH>()?.iter().map(|byte| APP_LETTERS[usize::from(byte & 31)] as char).collect();
        let (hash, md5_hash, now) = (token_hash(&password), token_hash(&md5_hex(&password)), db::now());
        let kept = name.clone();
        let id = self
            .db
            .call(move |conn| {
                let count: i64 = conn.query_row("SELECT count(*) FROM app_passwords WHERE user_id = ?1", [who.id], |r| r.get(0))?;
                if count >= MAX_APPS {
                    return Err(AppError::bad_request(format!("An account has at most {MAX_APPS} app passwords. Revoke one first.")));
                }
                conn.execute(
                    "INSERT INTO app_passwords (user_id, name, hash, md5_hash, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![who.id, kept, hash, md5_hash, now],
                )?;
                Ok(conn.last_insert_rowid())
            })
            .await?;
        Ok(AppPassword { id, name, created_at: now, used_at: None, password: Some(password) })
    }

    /// Takes an app password back. The app it was for is out from its next call.
    pub async fn remove_app(&self, headers: &HeaderMap, id: i64) -> Result<()> {
        let who = self.session(headers).ok_or_else(signed_out)?;
        let gone = self.db.call(move |conn| Ok(conn.execute("DELETE FROM app_passwords WHERE id = ?1 AND user_id = ?2", [id, who.id])?)).await?;
        if gone == 0 {
            return Err(AppError::not_found("No such app password"));
        }
        locked(&self.basics).retain(|_, (owner, _)| *owner != who.id);
        Ok(())
    }

    /// The account an app password opens, when `name` is its account's. `column` is the
    /// hash it is looked up by: of the password, or of its MD5.
    async fn app_password(&self, column: &'static str, hash: String, name: &str) -> Result<Option<Who>> {
        let now = db::now();
        let found = self
            .db
            .call(move |conn| {
                let find = format!("SELECT id, user_id FROM app_passwords WHERE {column} = ?1");
                let Some((id, user)) = conn.query_row(&find, [hash], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).optional()? else {
                    return Ok(None);
                };
                conn.execute("UPDATE app_passwords SET used_at = ?1 WHERE id = ?2 AND (used_at IS NULL OR used_at < ?1 - 600)", params![now, id])?;
                Ok(Some(user))
            })
            .await?;
        let Some(account) = found.and_then(|user| self.account(user).ok()) else { return Ok(None) };
        let name = name.trim();
        let fits = if name.is_empty() { locked(&self.accounts).len() == 1 } else { account.name.eq_ignore_ascii_case(name) };
        Ok(fits.then(|| Who { id: account.id, name: account.name, admin: account.admin }))
    }

    /// Who KOReader's sync comes from: it sends the account's name and the MD5 of an app
    /// password, as it would of a password. A wrong pair slows its sender down.
    pub async fn kosync(&self, from: IpAddr, name: &str, key: &str) -> Result<Who> {
        let from = bucket(from);
        let wait = wait_in(&self.strikes, &from);
        if wait > 0 {
            return Err(too_many(wait));
        }
        match self.app_password("md5_hash", token_hash(&key.trim().to_ascii_lowercase()), name).await? {
            Some(who) => Ok(who),
            None => {
                strike_in(&self.strikes, from);
                Err(AppError::new(401, "wrong_password", "That name and app password do not match"))
            }
        }
    }

    pub async fn logout(&self, headers: &HeaderMap) -> Result<()> {
        let Some(hash) = self.token(headers).map(|token| token_hash(&token)) else { return Ok(()) };
        locked(&self.sessions).remove(&hash);
        self.db.call(move |conn| Ok(conn.execute("DELETE FROM sessions WHERE token_hash = ?1", [hash])?)).await?;
        Ok(())
    }

    /// Changes the caller's password and signs out every other browser of theirs.
    pub async fn change(&self, from: IpAddr, headers: &HeaderMap, current: String, password: String) -> Result<()> {
        let (Some(who), Some(keep)) = (self.session(headers), self.token(headers).map(|token| token_hash(&token))) else {
            return Err(signed_out());
        };
        check_length(&password)?;
        self.check_password(from, &who.name, current).await?;
        self.store(who.id, password).await?;
        self.sign_out(who.id, Some(keep)).await
    }

    // ---------- accounts, for an admin ----------

    pub async fn add(&self, name: &str, password: String, admin: bool) -> Result<Account> {
        let name = clean_name(name)?;
        check_length(&password)?;
        let hash = self.hash(password).await?;
        let id = self
            .db
            .call(move |conn| {
                let tx = conn.transaction()?;
                let id = insert_account(&tx, &name, &hash, admin)?;
                tx.commit()?;
                Ok(id)
            })
            .await?;
        self.reload().await?;
        self.account(id)
    }

    pub async fn edit(&self, id: i64, change: AccountChange) -> Result<Account> {
        self.account(id)?;
        if id == OWNER && change.admin == Some(false) {
            return Err(AppError::bad_request("The first account stays an admin"));
        }
        let name = change.name.as_deref().map(clean_name).transpose()?;
        if let Some(password) = &change.password {
            check_length(password)?;
        }
        if change.two_step == Some(true) {
            return Err(AppError::bad_request("Only the account's holder can switch two-factor authentication on"));
        }
        if change.two_step == Some(false) {
            self.db.call(move |conn| drop_two_step(conn, id)).await?;
        }
        if name.is_some() || change.admin.is_some() {
            self.db
                .call(move |conn| {
                    conn.execute("UPDATE users SET name = coalesce(?1, name), admin = coalesce(?2, admin) WHERE id = ?3", params![name, change.admin, id])
                        .map_err(name_taken)?;
                    Ok(())
                })
                .await?;
        }
        if let Some(password) = change.password {
            self.store(id, password).await?;
            self.sign_out(id, None).await?;
        }
        self.reload().await?;
        self.account(id)
    }

    /// Removes an account and signs it out. Its library is the caller's to delete.
    pub async fn remove(&self, id: i64) -> Result<()> {
        self.account(id)?;
        if id == OWNER {
            return Err(AppError::bad_request("The first account cannot be removed"));
        }
        self.db.call(move |conn| Ok(conn.execute("DELETE FROM users WHERE id = ?1", [id])?)).await?;
        self.sign_out(id, None).await?;
        self.reload().await
    }

    // ---------- invitations ----------

    /// Whether whoever holds an invitation may make an account here.
    pub fn signup(&self) -> bool {
        self.on && self.front.signup
    }

    /// Every invitation, the newest first. None of them carries its code.
    pub async fn invites(&self) -> Result<Vec<Invite>> {
        self.db
            .call(|conn| {
                let list = "SELECT i.id, i.note, i.created_at, i.expires_at, i.used_at, u.name
                            FROM invites i LEFT JOIN users u ON u.id = i.user_id ORDER BY i.id DESC";
                let mut stmt = conn.prepare(list)?;
                let rows = stmt.query_map([], |r| {
                    Ok(Invite { id: r.get(0)?, note: r.get(1)?, created_at: r.get(2)?, expires_at: r.get(3)?, used_at: r.get(4)?, used_by: r.get(5)?, code: None })
                })?;
                Ok(rows.collect::<rusqlite::Result<_>>()?)
            })
            .await
    }

    /// Makes an invitation. Its code is in this answer and nowhere else.
    pub async fn invite(&self, note: &str, days: Option<i64>) -> Result<Invite> {
        if !self.signup() {
            return Err(signup_off());
        }
        let note: String = note.trim().chars().filter(|c| !c.is_control()).take(MAX_NOTE).collect();
        let code = new_code()?;
        let (key, now) = (token_hash(&plain_code(&code)), db::now());
        let ends = now + days.unwrap_or(INVITE_DAYS).clamp(1, MAX_INVITE_DAYS) * 86_400;
        let kept = note.clone();
        let id = self
            .db
            .call(move |conn| {
                conn.execute("INSERT INTO invites (code_hash, note, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)", params![key, kept, now, ends])?;
                Ok(conn.last_insert_rowid())
            })
            .await?;
        Ok(Invite { id, note, created_at: now, expires_at: ends, used_at: None, used_by: None, code: Some(code) })
    }

    /// Takes an invitation back. An account that was made with it stays.
    pub async fn uninvite(&self, id: i64) -> Result<()> {
        let gone = self.db.call(move |conn| Ok(conn.execute("DELETE FROM invites WHERE id = ?1", [id])?)).await?;
        if gone == 0 {
            return Err(AppError::not_found("No such invitation"));
        }
        Ok(())
    }

    /// Makes a reader's account for whoever holds an open invitation, and spends it.
    /// A wrong code counts against its sender as a wrong password does.
    pub async fn join(&self, from: IpAddr, name: &str, password: String, code: &str) -> Result<Account> {
        if !self.signup() {
            return Err(signup_off());
        }
        if !self.has_password() {
            return Err(setup_required());
        }
        let from = bucket(from);
        let wait = wait_in(&self.strikes, &from);
        if wait > 0 {
            return Err(too_many(wait));
        }
        let name = clean_name(name)?;
        check_length(&password)?;
        let (key, now) = (token_hash(&plain_code(code)), db::now());
        let asked = key.clone();
        let open = self
            .db
            .call(move |conn| {
                let open = "SELECT 1 FROM invites WHERE code_hash = ?1 AND used_at IS NULL AND expires_at > ?2";
                Ok(conn.query_row(open, params![asked, now], |_| Ok(())).optional()?.is_some())
            })
            .await?;
        if !open {
            self.strike(from);
            return Err(bad_invite());
        }
        let hash = self.hash(password).await?;
        let id = self
            .db
            .call(move |conn| {
                let tx = conn.transaction()?;
                let id = insert_account(&tx, &name, &hash, false)?;
                // Spent in the step that makes the account: two callers with one code make one account.
                let spend = "UPDATE invites SET used_at = ?1, user_id = ?2 WHERE code_hash = ?3 AND used_at IS NULL AND expires_at > ?1";
                if tx.execute(spend, params![now, id, key])? != 1 {
                    return Err(bad_invite());
                }
                tx.commit()?;
                Ok(id)
            })
            .await?;
        locked(&self.strikes).remove(&from);
        self.reload().await?;
        self.account(id)
    }

    /// Opens an invitation again when the account made with it could not be kept.
    async fn unspend(&self, user: i64) -> Result<()> {
        self.db.call(move |conn| Ok(conn.execute("UPDATE invites SET used_at = NULL, user_id = NULL WHERE user_id = ?1", [user])?)).await?;
        Ok(())
    }

    fn account(&self, id: i64) -> Result<Account> {
        locked(&self.accounts).iter().find(|account| account.id == id).cloned().ok_or_else(|| AppError::not_found("No such account"))
    }

    async fn reload(&self) -> Result<()> {
        let fresh = self.db.call(|conn| accounts(conn)).await?;
        *locked(&self.accounts) = fresh;
        locked(&self.basics).clear();
        Ok(())
    }

    /// Ends an account's sessions, all of them or all but one.
    async fn sign_out(&self, user: i64, keep: Option<String>) -> Result<()> {
        locked(&self.sessions).retain(|hash, (owner, _)| *owner != user || keep.as_ref() == Some(hash));
        locked(&self.basics).retain(|_, (owner, _)| *owner != user);
        let keep = keep.unwrap_or_default();
        self.db.call(move |conn| Ok(conn.execute("DELETE FROM sessions WHERE user_id = ?1 AND token_hash <> ?2", params![user, keep])?)).await?;
        Ok(())
    }

    // ---------- passwords ----------

    /// The account `name` and `password` belong to. Wrong ones slow their sender down.
    async fn check_password(&self, from: IpAddr, name: &str, password: String) -> Result<Who> {
        let from = bucket(from);
        let wait = wait_in(&self.strikes, &from);
        if wait > 0 {
            return Err(AppError::new(429, "too_many_tries", format!("Too many wrong passwords. Try again in {wait} seconds.")));
        }
        let (found, several) = {
            let accounts = locked(&self.accounts);
            let name = name.trim();
            let found = match name.is_empty() {
                true => accounts.first().filter(|_| accounts.len() == 1),
                false => accounts.iter().find(|account| account.name.eq_ignore_ascii_case(name)),
            };
            (found.map(|account| Who { id: account.id, name: account.name.clone(), admin: account.admin }), accounts.len() > 1)
        };
        // A name nobody has costs as much time as a wrong password, so the wait does not tell them apart.
        let user = found.as_ref().map_or(0, |who| who.id);
        if password.len() <= MAX_PASSWORD && self.verify(user, password).await? {
            if let Some(who) = found {
                locked(&self.strikes).remove(&from);
                return Ok(who);
            }
        }
        self.strike(from);
        Err(AppError::new(401, "wrong_password", if several { "That name and password do not match" } else { "That is not the password" }))
    }

    async fn hash(&self, password: String) -> Result<String> {
        let _one = self.hashing.lock().await;
        tokio::task::spawn_blocking(move || hash_password(&password)).await.map_err(|e| AppError::internal(format!("hashing task: {e}")))?
    }

    async fn store(&self, user: i64, password: String) -> Result<()> {
        let hash = self.hash(password).await?;
        self.db.call(move |conn| Ok(conn.execute("UPDATE users SET password = ?1 WHERE id = ?2", params![hash, user])?)).await?;
        Ok(())
    }

    /// Gives the owner a password, and makes the owner's account when there is none yet.
    async fn store_owner(&self, password: String) -> Result<()> {
        let hash = self.hash(password).await?;
        self.db
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO users (id, name, password, admin, created_at) VALUES (?1, ?2, ?3, 1, ?4)
                     ON CONFLICT(id) DO UPDATE SET password = excluded.password, admin = 1",
                    params![OWNER, OWNER_NAME, hash, db::now()],
                )?;
                Ok(())
            })
            .await?;
        self.reload().await?;
        self.has_password.store(true, Ordering::Release);
        Ok(())
    }

    async fn verify(&self, user: i64, password: String) -> Result<bool> {
        let stored = self.db.call(move |conn| Ok(conn.query_row("SELECT password FROM users WHERE id = ?1", [user], |r| r.get::<_, Option<String>>(0)).optional()?)).await?;
        let _one = self.hashing.lock().await;
        tokio::task::spawn_blocking(move || match stored.flatten() {
            Some(stored) => PasswordHash::new(&stored).is_ok_and(|hash| Argon2::default().verify_password(password.as_bytes(), &hash).is_ok()),
            None => hash_password(&password).is_err(),
        })
        .await
        .map_err(|e| AppError::internal(format!("hashing task: {e}")))
    }

    // ---------- sessions ----------

    async fn start_session(&self, user: i64, agent: String) -> Result<String> {
        let token = hex::encode(random::<32>()?);
        let hash = token_hash(&token);
        let now = db::now();
        let ends = now + SESSION_SECS;
        let (row, agent) = (hash.clone(), agent.chars().take(200).collect::<String>());
        self.db
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO sessions (token_hash, created_at, seen_at, expires_at, agent, user_id) VALUES (?1, ?2, ?2, ?3, ?4, ?5)",
                    params![row, now, ends, agent, user],
                )?;
                Ok(())
            })
            .await?;
        locked(&self.sessions).insert(hash, (user, ends));
        Ok(token)
    }

    /// Who the request comes from. With login switched off that is always the owner.
    pub fn who(&self, headers: &HeaderMap) -> Option<Who> {
        if !self.on {
            let owner = locked(&self.accounts).iter().find(|account| account.id == OWNER).map(|account| account.name.clone());
            return Some(Who { id: OWNER, name: owner.unwrap_or_else(|| OWNER_NAME.into()), admin: true });
        }
        self.session(headers)
    }

    /// The account whose live session the request carries. Using a session keeps it alive.
    fn session(&self, headers: &HeaderMap) -> Option<Who> {
        let hash = self.token(headers).map(|token| token_hash(&token))?;
        let now = db::now();
        let user = {
            let mut sessions = locked(&self.sessions);
            match sessions.get(&hash).copied() {
                Some((user, ends)) if ends > now => {
                    if ends - now < SESSION_SECS - TOUCH_SECS {
                        let ends = now + SESSION_SECS;
                        sessions.insert(hash.clone(), (user, ends));
                        let db = self.db.clone();
                        tokio::spawn(async move {
                            let touch = "UPDATE sessions SET seen_at = ?1, expires_at = ?2 WHERE token_hash = ?3";
                            let _ = db.call(move |conn| Ok(conn.execute(touch, params![now, ends, hash])?)).await;
                        });
                    }
                    user
                }
                Some(_) => {
                    sessions.remove(&hash);
                    return None;
                }
                None => return None,
            }
        };
        self.account(user).ok().map(|account| Who { id: account.id, name: account.name, admin: account.admin })
    }

    /// Who a call that carries a name and password comes from: for reading apps, which
    /// keep no cookie. A pair that was right is not hashed again for a while.
    pub async fn basic(&self, from: IpAddr, headers: &HeaderMap) -> Result<Who> {
        if let Some(who) = self.who(headers) {
            return Ok(who);
        }
        let given = headers.get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()).and_then(|value| value.strip_prefix("Basic "));
        let Some(given) = given else { return Err(signed_out()) };
        let (key, now) = (token_hash(given), db::now());
        let known = locked(&self.basics).get(&key).copied().filter(|(_, until)| *until > now);
        if let Some(account) = known.and_then(|(user, _)| self.account(user).ok()) {
            return Ok(Who { id: account.id, name: account.name, admin: account.admin });
        }
        let text = base64::engine::general_purpose::STANDARD.decode(given.trim()).ok().and_then(|bytes| String::from_utf8(bytes).ok());
        let Some((name, password)) = text.as_deref().and_then(|text| text.split_once(':')) else { return Err(signed_out()) };
        if !self.has_password() {
            return Err(setup_required());
        }
        // An app password first: it costs no hashing. Then the account's own password,
        // which an account with two-factor authentication keeps for the browser.
        let who = match self.app_password("hash", token_hash(password.trim()), name).await? {
            Some(who) => who,
            None => {
                let who = self.check_password(from, name, password.to_owned()).await?;
                if self.two_step(who.id) {
                    let why = "This account signs in with a code. Give the app an app password, from Settings › Account.";
                    return Err(AppError::new(401, "app_password_required", why));
                }
                who
            }
        };
        let mut basics = locked(&self.basics);
        if basics.len() > 256 {
            basics.retain(|_, (_, until)| *until > now);
        }
        basics.insert(key, (who.id, now + BASIC_SECS));
        Ok(who)
    }

    fn token(&self, headers: &HeaderMap) -> Option<String> {
        let lines = headers.get_all(header::COOKIE).iter().filter_map(|value| value.to_str().ok());
        lines.flat_map(|line| line.split(';')).find_map(|pair| {
            let (name, value) = pair.trim().split_once('=')?;
            (name == self.cookie && value.len() == 64).then(|| value.to_owned())
        })
    }

    /// The header that hands a browser its session, or with an empty token takes it away.
    fn cookie_for(&self, token: &str, headers: &HeaderMap) -> HeaderValue {
        // Behind a proxy that speaks HTTPS, the cookie is for HTTPS only.
        let secure = headers.get("x-forwarded-proto").and_then(|value| value.to_str().ok()).is_some_and(|proto| proto.eq_ignore_ascii_case("https"));
        let age = if token.is_empty() { 0 } else { COOKIE_SECS };
        let text = format!("{}={token}; Path=/; Max-Age={age}; HttpOnly; SameSite=Lax{}", self.cookie, if secure { "; Secure" } else { "" });
        HeaderValue::from_str(&text).unwrap_or_else(|_| HeaderValue::from_static(""))
    }

    /// Counts a wrong password or code against the address it came from.
    fn strike(&self, from: IpAddr) {
        strike_in(&self.strikes, from);
    }
}

/// Whether `host`, as a `Host` header gives it, is a name nobody outside the network can
/// point here: an address, this machine's own name, a name without a dot, one that only
/// a local network resolves, or one of `named`.
fn known_host(host: &str, named: &[String]) -> bool {
    let host = host.trim();
    if host.starts_with('[') {
        return true;
    }
    let name = match host.rsplit_once(':') {
        Some((name, port)) if port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    };
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    name.parse::<IpAddr>().is_ok()
        || name == "localhost"
        || !name.contains('.')
        || LOCAL_ENDINGS.iter().any(|ending| name.ends_with(ending))
        || named.iter().any(|known| known.trim_end_matches('.').eq_ignore_ascii_case(&name))
}

/// A name that another account has reads as that, not as a database fault.
fn name_taken(error: rusqlite::Error) -> AppError {
    match error.sqlite_error_code() {
        Some(rusqlite::ErrorCode::ConstraintViolation) => AppError::new(409, "name_taken", "Another account has that name"),
        _ => error.into(),
    }
}

pub fn setup_required() -> AppError {
    AppError::new(401, "setup_required", "Set a password first")
}

pub fn signed_out() -> AppError {
    AppError::new(401, "signed_out", "Sign in first")
}

fn signup_off() -> AppError {
    AppError::new(403, "signup_off", "This Mango does not take new accounts by invitation")
}

/// One answer for a code that never was, one that was used and one that ran out.
fn bad_invite() -> AppError {
    AppError::new(403, "invite_code", "That invitation does not open: its code is mistyped, used already or too old.")
}

/// Lets a request through to the API only from a signed-in browser, and hands it
/// the library of the account that browser belongs to.
pub async fn require(State(server): State<Spaces>, mut request: Request, next: Next) -> Response {
    let auth = &server.auth;
    let Some(who) = auth.who(request.headers()) else {
        return if auth.has_password() { signed_out() } else { setup_required() }.into_response();
    };
    match server.of(who) {
        Ok(app) => {
            request.extensions_mut().insert(app);
            next.run(request).await
        }
        Err(why) => why.into_response(),
    }
}

/// The calls that work without a session. They sit under `/api/auth`.
pub fn routes() -> Router<Spaces> {
    Router::new()
        .route("/", get(status))
        .route("/setup", post(setup))
        .route("/login", post(login))
        .route("/signup", post(signup))
        .route("/logout", post(logout))
        .route("/password", put(change))
        .route("/two-step", get(two_step).post(enrol).put(confirm).delete(leave_two_step))
        .route("/two-step/codes", post(new_codes))
        .route("/apps", get(apps).post(add_app))
        .route("/apps/{id}", delete(remove_app))
}

#[derive(Deserialize)]
struct Password {
    password: String,
    /// The account. Signing in reads it when there are several; making an account always does.
    #[serde(default)]
    name: String,
    /// The password in use now. Only a change asks for it.
    #[serde(default)]
    current: String,
    /// The setup code, which setting the first password from another machine asks for,
    /// or an invitation's, which making an account asks for.
    #[serde(default)]
    code: String,
}

#[derive(Deserialize)]
struct Asked {
    /// 32 hex characters to answer with a proof; see `Auth::proof`.
    challenge: Option<String>,
}

fn agent(headers: &HeaderMap) -> String {
    headers.get(header::USER_AGENT).and_then(|value| value.to_str().ok()).unwrap_or("").to_owned()
}

fn with_cookie(auth: &Auth, token: &str, headers: &HeaderMap) -> Response {
    ([(header::SET_COOKIE, auth.cookie_for(token, headers))], Json(json!({ "ok": true }))).into_response()
}

async fn status(
    State(server): State<Spaces>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(asked): Query<Asked>,
    headers: HeaderMap,
) -> Json<Status> {
    Json(server.auth.status(&headers, Auth::local(peer.ip(), &headers), asked.challenge.as_deref()))
}

async fn setup(
    State(server): State<Spaces>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Password>,
) -> Result<Response> {
    let local = Auth::local(peer.ip(), &headers);
    let token = server.auth.setup(body.password, &body.code, local, agent(&headers)).await?;
    Ok(with_cookie(&server.auth, &token, &headers))
}

async fn login(
    State(server): State<Spaces>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Password>,
) -> Result<Response> {
    let from = server.auth.caller(peer.ip(), &headers);
    let token = server.auth.login(from, &body.name, body.password, &body.code, agent(&headers)).await?;
    Ok(with_cookie(&server.auth, &token, &headers))
}

async fn two_step(State(server): State<Spaces>, headers: HeaderMap) -> Result<Json<TwoStep>> {
    Ok(Json(server.auth.two_step_status(&headers).await?))
}

async fn enrol(State(server): State<Spaces>, headers: HeaderMap) -> Result<Json<Enrolment>> {
    Ok(Json(server.auth.enrol(&headers)?))
}

#[derive(Deserialize)]
struct Typed {
    #[serde(default)]
    code: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    name: String,
}

async fn confirm(State(server): State<Spaces>, headers: HeaderMap, Json(body): Json<Typed>) -> Result<Json<serde_json::Value>> {
    Ok(Json(json!({ "codes": server.auth.confirm(&headers, &body.code).await? })))
}

async fn new_codes(
    State(server): State<Spaces>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Typed>,
) -> Result<Json<serde_json::Value>> {
    let from = server.auth.caller(peer.ip(), &headers);
    Ok(Json(json!({ "codes": server.auth.new_codes(from, &headers, body.password).await? })))
}

async fn leave_two_step(
    State(server): State<Spaces>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Typed>,
) -> Result<Json<serde_json::Value>> {
    server.auth.leave_two_step(server.auth.caller(peer.ip(), &headers), &headers, body.password).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn apps(State(server): State<Spaces>, headers: HeaderMap) -> Result<Json<serde_json::Value>> {
    Ok(Json(json!({ "apps": server.auth.apps(&headers).await? })))
}

async fn add_app(State(server): State<Spaces>, headers: HeaderMap, Json(body): Json<Typed>) -> Result<Json<AppPassword>> {
    Ok(Json(server.auth.add_app(&headers, &body.name).await?))
}

async fn remove_app(State(server): State<Spaces>, headers: HeaderMap, Path(id): Path<i64>) -> Result<Json<serde_json::Value>> {
    server.auth.remove_app(&headers, id).await?;
    Ok(Json(json!({ "ok": true })))
}

/// Makes an account for the holder of an invitation and signs them in.
async fn signup(
    State(server): State<Spaces>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Password>,
) -> Result<Response> {
    let auth = &server.auth;
    let account = auth.join(auth.caller(peer.ip(), &headers), &body.name, body.password, &body.code).await?;
    // A new account starts with an empty library and no stores.
    if let Err(why) = server.create(account.id) {
        let _ = auth.unspend(account.id).await;
        let _ = auth.remove(account.id).await;
        return Err(why);
    }
    let token = auth.start_session(account.id, agent(&headers)).await?;
    Ok(with_cookie(auth, &token, &headers))
}

async fn logout(State(server): State<Spaces>, headers: HeaderMap) -> Result<Response> {
    server.auth.logout(&headers).await?;
    Ok(with_cookie(&server.auth, "", &headers))
}

async fn change(
    State(server): State<Spaces>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Password>,
) -> Result<Json<serde_json::Value>> {
    server.auth.change(server.auth.caller(peer.ip(), &headers), &headers, body.current, body.password).await?;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[tokio::test]
    async fn the_first_password_from_another_machine_takes_the_code() {
        let auth = Auth::open(Db::open(Path::new(":memory:")).unwrap(), true, 8787, None, Front::default()).await.unwrap();
        let code = auth.setup_code().expect("a Mango without a password has a code").to_owned();
        assert!(auth.status(&HeaderMap::new(), false, None).setup_code && !auth.status(&HeaderMap::new(), true, None).setup_code);
        for wrong in ["", "0000-0000-0000-0000", &code[1..]] {
            assert_eq!(auth.setup("correct horse".into(), wrong, false, String::new()).await.unwrap_err().code, "setup_code");
        }
        assert!(!auth.has_password(), "a wrong code sets nothing");
        auth.setup("correct horse".into(), &code.replace('-', " ").to_uppercase(), false, String::new()).await.unwrap();
        assert!(auth.setup_code().is_none(), "the code is spent");
        assert_eq!(auth.setup("another one".into(), &code, false, String::new()).await.unwrap_err().code, "already_set");

        // A call is from this machine when it connects from it and nothing passed it on.
        let relayed: HeaderMap = [(header::HeaderName::from_static("x-forwarded-for"), "203.0.113.9".parse().unwrap())].into_iter().collect();
        assert!(Auth::local("127.0.0.1".parse().unwrap(), &HeaderMap::new()) && Auth::local("::ffff:127.0.0.1".parse().unwrap(), &HeaderMap::new()));
        assert!(!Auth::local("10.0.0.1".parse().unwrap(), &HeaderMap::new()) && !Auth::local("127.0.0.1".parse().unwrap(), &relayed));

        // A password given at the start leaves nothing to set, so there is no code.
        let given = Auth::open(Db::open(Path::new(":memory:")).unwrap(), true, 8787, Some("correct horse".into()), Front::default()).await.unwrap();
        assert!(given.setup_code().is_none());
    }

    #[tokio::test]
    async fn callers_are_told_apart_by_network() {
        let peer: IpAddr = "10.0.0.1".parse().unwrap();
        let written: HeaderMap = [(header::HeaderName::from_static("x-forwarded-for"), "198.51.100.7, 203.0.113.9".parse().unwrap())].into_iter().collect();
        let direct = Auth::open(Db::open(Path::new(":memory:")).unwrap(), true, 8787, None, Front::default()).await.unwrap();
        assert_eq!(direct.caller(peer, &written), peer, "what a caller writes about itself counts for nothing");
        let front = Front { proxied: true, ..Front::default() };
        let proxied = Auth::open(Db::open(Path::new(":memory:")).unwrap(), true, 8787, None, front).await.unwrap();
        assert_eq!(proxied.caller(peer, &written), "203.0.113.9".parse::<IpAddr>().unwrap(), "the proxy's own entry is the last");
        assert_eq!(proxied.caller(peer, &HeaderMap::new()), peer);

        // One holder of an IPv6 network has all of its addresses, and one wait.
        let bucket_of = |text: &str| bucket(text.parse().unwrap());
        assert_eq!(bucket_of("2001:db8:1:2:aaaa::1"), bucket_of("2001:db8:1:2:bbbb::2"));
        assert_ne!(bucket_of("2001:db8:1:2::1"), bucket_of("2001:db8:1:3::1"));
        assert_eq!(bucket_of("::ffff:10.0.0.1"), bucket_of("10.0.0.1"));

        // Only the Mango that keeps the mark answers a challenge with it.
        let challenge = "0123456789abcdef0123456789abcdef";
        let proof = direct.status(&HeaderMap::new(), true, Some(challenge)).proof;
        assert_eq!(proof, Some(answer_to(direct.instance(), challenge)));
        assert_ne!(proof, proxied.status(&HeaderMap::new(), true, Some(challenge)).proof);
        assert!(direct.status(&HeaderMap::new(), true, Some("short")).proof.is_none());
    }

    fn with(auth: &Auth, token: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, format!("theme=dark; {}={token}", auth.cookie).parse().unwrap());
        headers
    }

    fn basic(pair: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(pair)).parse().unwrap());
        headers
    }

    #[tokio::test]
    async fn one_password_many_sessions() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let auth = Auth::open(db, true, 8787, None, Front::default()).await.unwrap();
        let here: IpAddr = [127, 0, 0, 1].into();
        assert!(auth.status(&HeaderMap::new(), true, None).setup);
        assert!(auth.setup("short".into(), "", true, String::new()).await.is_err());
        assert!(!auth.has_password(), "a refused password leaves setup open");

        let first = auth.setup("correct horse".into(), "", true, String::new()).await.unwrap();
        assert_eq!(auth.who(&with(&auth, &first)).unwrap(), Who { id: OWNER, name: "owner".into(), admin: true });
        assert!(auth.who(&with(&auth, &"0".repeat(64))).is_none());
        assert_eq!(auth.setup("another one".into(), "", true, String::new()).await.unwrap_err().code, "already_set");

        assert_eq!(auth.login(here, "", "wrong horse".into(), "", String::new()).await.unwrap_err().code, "wrong_password");
        let second = auth.login(here, "", "correct horse".into(), "", String::new()).await.unwrap();
        assert_ne!(first, second);

        // A change keeps the caller and signs the other browser out.
        let headers = with(&auth, &second);
        assert!(auth.change(here, &headers, "wrong horse".into(), "battery staple".into()).await.is_err());
        auth.change(here, &headers, "correct horse".into(), "battery staple".into()).await.unwrap();
        assert!(auth.who(&headers).is_some());
        assert!(auth.who(&with(&auth, &first)).is_none());
        assert!(auth.login(here, "", "correct horse".into(), "", String::new()).await.is_err());

        auth.logout(&headers).await.unwrap();
        assert!(auth.who(&headers).is_none());

        // Over plain HTTP the cookie is plain; behind an HTTPS proxy it is for HTTPS only.
        let plain = auth.cookie_for(&second, &HeaderMap::new());
        assert!(plain.to_str().unwrap().starts_with("mango_8787=") && !plain.to_str().unwrap().contains("Secure"));
        let mut proxied = HeaderMap::new();
        proxied.insert("x-forwarded-proto", "https".parse().unwrap());
        assert!(auth.cookie_for(&second, &proxied).to_str().unwrap().ends_with("; HttpOnly; SameSite=Lax; Secure"));
    }

    #[tokio::test]
    async fn several_accounts() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let auth = Auth::open(db, true, 8787, Some("correct horse".into()), Front::default()).await.unwrap();
        let here: IpAddr = [127, 0, 0, 1].into();
        assert!(!auth.status(&HeaderMap::new(), true, None).named, "one account signs in without a name");

        let kid = auth.add(" Kid ", "battery staple".into(), false).await.unwrap();
        assert_eq!((kid.id, kid.name.as_str(), kid.admin), (2, "Kid", false));
        assert_eq!(auth.add("kid", "battery staple".into(), false).await.unwrap_err().code, "name_taken");
        assert!(auth.add("a:b", "battery staple".into(), false).await.is_err());
        assert!(auth.status(&HeaderMap::new(), true, None).named);

        // With several accounts a name is needed, and each password opens its own account only.
        assert!(auth.login(here, "", "correct horse".into(), "", String::new()).await.is_err());
        assert!(auth.login(here, "kid", "correct horse".into(), "", String::new()).await.is_err());
        let token = auth.login(here, "KID", "battery staple".into(), "", String::new()).await.unwrap();
        let headers = with(&auth, &token);
        assert_eq!(auth.who(&headers).unwrap(), Who { id: 2, name: "Kid".into(), admin: false });
        assert!(auth.who(&headers).unwrap().admin().is_err());

        // A reading app sends name and password with the call.
        assert_eq!(auth.basic(here, &basic("kid:battery staple")).await.unwrap().id, 2);
        assert_eq!(auth.basic(here, &basic("kid:nope nope")).await.unwrap_err().code, "wrong_password");
        assert_eq!(auth.basic(here, &HeaderMap::new()).await.unwrap_err().code, "signed_out");

        // A new password from the admin signs the account out everywhere.
        auth.edit(2, AccountChange { name: Some("Sis".into()), password: Some("another staple".into()), admin: Some(true), ..Default::default() }).await.unwrap();
        assert!(auth.who(&headers).is_none());
        assert!(auth.basic(here, &basic("sis:battery staple")).await.is_err());
        assert!(auth.basic(here, &basic("sis:another staple")).await.unwrap().admin);

        assert!(auth.remove(OWNER).await.is_err());
        assert!(auth.edit(OWNER, AccountChange { admin: Some(false), ..Default::default() }).await.is_err());
        auth.remove(2).await.unwrap();
        assert!(auth.basic(here, &basic("sis:another staple")).await.is_err());
        assert_eq!(auth.ids(), [OWNER]);

        // The number of an account that is gone is not given to the next one: its folder may still be there.
        assert_eq!(auth.add("next", "battery staple".into(), false).await.unwrap().id, 3);
    }

    #[tokio::test]
    async fn only_known_names_before_signing_in_stands_in_the_way() {
        let named = |host: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::HOST, host.parse().unwrap());
            headers
        };
        let hosts = vec!["mango.example.org".to_owned()];
        for host in ["127.0.0.1:8787", "[::1]:8787", "localhost", "nas:8787", "Mango.local", "nas.home.arpa.", "mango.example.org:443"] {
            assert!(known_host(host, &hosts), "{host}");
        }
        for host in ["rebind.example.org:8787", "127.0.0.1.example.org", "mango.example.org.example.net", "local.example.org"] {
            assert!(!known_host(host, &hosts), "{host}");
        }

        // With login switched off every request is the owner's, so the name always counts.
        let front = Front { hosts: hosts.clone(), ..Front::default() };
        let open = Auth::open(Db::open(Path::new(":memory:")).unwrap(), false, 8787, None, front).await.unwrap();
        assert!(open.answers(&named("10.0.0.1:8787")) && open.answers(&named("mango.example.org")) && open.answers(&HeaderMap::new()));
        assert!(!open.answers(&named("rebind.example.org:8787")));
        let mut proxied = named("127.0.0.1:8787");
        proxied.insert("x-forwarded-host", "rebind.example.org".parse().unwrap());
        assert!(!open.answers(&proxied), "the name the browser used counts too");

        // With login on it counts until there is a password, which another site's page does not have.
        let auth = Auth::open(Db::open(Path::new(":memory:")).unwrap(), true, 8787, None, Front::default()).await.unwrap();
        assert!(!auth.answers(&named("rebind.example.org:8787")) && auth.answers(&named("localhost:8787")));
        auth.setup("correct horse".into(), "", true, String::new()).await.unwrap();
        assert!(auth.answers(&named("rebind.example.org:8787")));
    }

    #[tokio::test]
    async fn wrong_passwords_slow_down() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let auth = Auth::open(db, true, 8787, Some("correct horse".into()), Front::default()).await.unwrap();
        assert!(auth.has_password(), "the first password came from the environment");
        let (here, there): (IpAddr, IpAddr) = ([10, 0, 0, 1].into(), [10, 0, 0, 2].into());
        for _ in 0..FREE_TRIES {
            assert_eq!(auth.login(here, "", "nope nope".into(), "", String::new()).await.unwrap_err().code, "wrong_password");
        }
        // Even the right password waits now, but only from that address.
        assert_eq!(auth.login(here, "", "correct horse".into(), "", String::new()).await.unwrap_err().code, "too_many_tries");
        assert!(auth.login(there, "", "correct horse".into(), "", String::new()).await.is_ok());
    }

    #[tokio::test]
    async fn an_invitation_makes_one_account() {
        let here: IpAddr = [10, 0, 0, 1].into();
        let closed = Auth::open(Db::open(Path::new(":memory:")).unwrap(), true, 8787, Some("correct horse".into()), Front::default()).await.unwrap();
        assert!(!closed.status(&HeaderMap::new(), true, None).signup, "nobody makes an account unless the owner said so");
        assert_eq!(closed.invite("", None).await.unwrap_err().code, "signup_off");
        assert_eq!(closed.join(here, "kid", "battery staple".into(), "0000-0000-0000-0000").await.unwrap_err().code, "signup_off");

        let front = Front { signup: true, ..Front::default() };
        let auth = Auth::open(Db::open(Path::new(":memory:")).unwrap(), true, 8787, Some("correct horse".into()), front).await.unwrap();
        assert!(auth.status(&HeaderMap::new(), true, None).signup);
        let invite = auth.invite("  for the kid ", Some(1000)).await.unwrap();
        let code = invite.code.clone().expect("a new invitation shows its code");
        assert_eq!((invite.note.as_str(), invite.expires_at - invite.created_at), ("for the kid", MAX_INVITE_DAYS * 86_400));
        assert!(auth.invites().await.unwrap().iter().all(|kept| kept.code.is_none()), "the code is not kept");

        // A wrong code makes nothing; a short password or a taken name leaves the invitation open.
        assert_eq!(auth.join(here, "kid", "battery staple".into(), "0000-0000-0000-0000").await.unwrap_err().code, "invite_code");
        assert!(auth.join(here, "kid", "short".into(), &code).await.is_err());
        assert_eq!(auth.join(here, "OWNER", "battery staple".into(), &code).await.unwrap_err().code, "name_taken");
        assert_eq!(auth.ids(), [OWNER]);

        let kid = auth.join(here, " kid ", "battery staple".into(), &code.replace('-', " ").to_uppercase()).await.unwrap();
        assert_eq!((kid.id, kid.name.as_str(), kid.admin), (2, "kid", false), "an invited account is a reader");
        assert!(auth.login(here, "kid", "battery staple".into(), "", String::new()).await.is_ok());
        let kept = auth.invites().await.unwrap();
        assert!(kept[0].used_at.is_some() && kept[0].used_by.as_deref() == Some("kid"));
        assert_eq!(auth.join(here, "sis", "battery staple".into(), &code).await.unwrap_err().code, "invite_code", "a code opens once");

        // One that was taken back opens nothing, and wrong codes make their sender wait as wrong passwords do.
        let second = auth.invite("", None).await.unwrap();
        auth.uninvite(second.id).await.unwrap();
        assert!(auth.uninvite(second.id).await.is_err());
        let (taken_back, there): (String, IpAddr) = (second.code.unwrap(), [10, 0, 0, 2].into());
        for _ in 0..FREE_TRIES {
            assert_eq!(auth.join(there, "sis", "battery staple".into(), &taken_back).await.unwrap_err().code, "invite_code");
        }
        let third = auth.invite("", None).await.unwrap().code.unwrap();
        assert_eq!(auth.join(there, "sis", "battery staple".into(), &third).await.unwrap_err().code, "too_many_tries");
        assert_eq!(auth.list().len(), 2);

        // Until the owner has a password there is nobody to invite anyone.
        let front = Front { signup: true, ..Front::default() };
        let new = Auth::open(Db::open(Path::new(":memory:")).unwrap(), true, 8787, None, front).await.unwrap();
        assert!(!new.status(&HeaderMap::new(), true, None).signup);
        assert_eq!(new.join(here, "kid", "battery staple".into(), "0000-0000-0000-0000").await.unwrap_err().code, "setup_required");
    }

    #[tokio::test]
    async fn switched_off_lets_everyone_in() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let auth = Auth::open(db, false, 8787, None, Front::default()).await.unwrap();
        let status = auth.status(&HeaderMap::new(), true, None);
        assert!(status.signed_in && status.admin && !status.login && !status.setup);
        assert_eq!(auth.ids(), [OWNER]);
    }

    #[tokio::test]
    async fn a_reset_asks_for_a_new_password() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let auth = Auth::open(db.clone(), true, 8787, Some("correct horse".into()), Front::default()).await.unwrap();
        auth.add("kid", "battery staple".into(), false).await.unwrap();
        db.call(|conn| reset(conn)).await.unwrap();
        let again = Auth::open(db, true, 8787, None, Front::default()).await.unwrap();
        assert!(again.status(&HeaderMap::new(), true, None).setup);
        again.setup("a new password".into(), "", true, String::new()).await.unwrap();
        assert_eq!(again.list().len(), 2, "the other account is still there");
    }

    #[test]
    fn codes_follow_the_standard() {
        // RFC 6238's SHA-1 cases, cut to six digits.
        let secret = b"12345678901234567890";
        for (time, code) in [(59, 287_082), (1_111_111_109, 81_804), (1_234_567_890, 5_924), (2_000_000_000, 279_037), (20_000_000_000, 353_130)] {
            assert_eq!(totp(secret, time / STEP_SECS), code, "{time}");
        }
        assert_eq!(code_step(secret, "287 082", 59), Some(1));
        assert_eq!(code_step(secret, "287082", 89), Some(1), "a step late still counts");
        assert_eq!(code_step(secret, "287082", 149), None);
        assert_eq!(code_step(secret, "28708", 59), None);
        assert_eq!((base32(b"foobar").as_str(), base32(b"f").as_str()), ("MZXW6YTBOI", "MY"));
        assert_eq!(md5_hex("abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[tokio::test]
    async fn two_step_sign_in_and_app_passwords() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let auth = Auth::open(db, true, 8787, Some("correct horse".into()), Front::default()).await.unwrap();
        let here: IpAddr = [127, 0, 0, 1].into();
        let token = auth.login(here, "", "correct horse".into(), "", String::new()).await.unwrap();
        let other = auth.login(here, "", "correct horse".into(), "", String::new()).await.unwrap();
        let headers = with(&auth, &token);
        assert_eq!(auth.enrol(&HeaderMap::new()).err().unwrap().code, "signed_out");

        let offer = auth.enrol(&headers).unwrap();
        assert!(offer.uri.starts_with("otpauth://totp/Mango%3Aowner?secret=") && offer.secret.len() == 32);
        let secret = locked(&auth.pending)[&OWNER].0.clone();
        let step = db::now() / STEP_SECS;
        let code = |at: i64| format!("{:06}", totp(&secret, at));
        let wrong = format!("{:06}", (totp(&secret, step) + 1) % 1_000_000);
        assert_eq!(auth.confirm(&headers, &wrong).await.unwrap_err().code, "wrong_code");
        let codes = auth.confirm(&headers, &code(step)).await.unwrap();
        assert_eq!(codes.len(), RECOVERY_CODES);
        assert!(auth.list()[0].two_step);
        assert!(auth.who(&headers).is_some() && auth.who(&with(&auth, &other)).is_none(), "every other browser is signed out");

        // The password alone no longer opens the account, and a code opens it once.
        assert_eq!(auth.login(here, "", "correct horse".into(), "", String::new()).await.unwrap_err().code, "code_required");
        assert_eq!(auth.login(here, "", "correct horse".into(), &code(step), String::new()).await.unwrap_err().code, "wrong_code");
        assert!(auth.login(here, "", "correct horse".into(), &code(step + 1), String::new()).await.is_ok());
        assert_eq!(auth.login(here, "", "wrong horse".into(), &code(step + 1), String::new()).await.unwrap_err().code, "wrong_password");
        assert!(auth.login(here, "", "correct horse".into(), &codes[0].to_uppercase(), String::new()).await.is_ok());
        assert_eq!(auth.login(here, "", "correct horse".into(), &codes[0], String::new()).await.unwrap_err().code, "wrong_code", "a recovery code opens once");
        assert_eq!(auth.two_step_status(&headers).await.unwrap().codes_left, RECOVERY_CODES as i64 - 1);

        // A reading app needs an app password now, and gets nowhere else with it. Wrong
        // tries come from elsewhere, so that they do not hold up the holder's.
        let there: IpAddr = [10, 0, 0, 9].into();
        assert_eq!(auth.basic(here, &basic("owner:correct horse")).await.unwrap_err().code, "app_password_required");
        let app = auth.add_app(&headers, " KOReader ").await.unwrap();
        let password = app.password.clone().unwrap();
        assert_eq!((app.name.as_str(), password.len()), ("KOReader", APP_LENGTH));
        assert!(auth.apps(&headers).await.unwrap().iter().all(|kept| kept.password.is_none()), "the password is not kept");
        assert_eq!(auth.basic(here, &basic(&format!("Owner:{password}"))).await.unwrap().id, OWNER);
        assert_eq!(auth.basic(here, &basic(&format!(":{password}"))).await.unwrap().id, OWNER, "one account needs no name");
        assert_eq!(auth.kosync(here, "owner", &md5_hex(&password).to_uppercase()).await.unwrap().id, OWNER);
        assert_eq!(auth.kosync(there, "kid", &md5_hex(&password)).await.unwrap_err().code, "wrong_password");
        assert!(auth.login(there, "", password.clone(), "", String::new()).await.is_err(), "an app password opens no browser");
        auth.remove_app(&headers, app.id).await.unwrap();
        assert!(auth.basic(there, &basic(&format!("owner:{password}"))).await.is_err());
        assert!(auth.kosync(there, "owner", &md5_hex(&password)).await.is_err());

        // Switching it off takes the password; an admin can only take it away.
        assert!(auth.leave_two_step(here, &headers, "wrong horse".into()).await.is_err());
        assert!(auth.edit(OWNER, AccountChange { two_step: Some(true), ..Default::default() }).await.is_err());
        auth.leave_two_step(here, &headers, "correct horse".into()).await.unwrap();
        assert!(!auth.list()[0].two_step);
        assert!(auth.login(here, "", "correct horse".into(), "", String::new()).await.is_ok());
    }
}
