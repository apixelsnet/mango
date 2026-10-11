//! Login: accounts, passwords, session cookies.
//!
//! A password is kept as an Argon2 hash. A browser that signs in gets a random
//! token in a cookie; the database holds only the token's hash. The first account
//! is the owner's: until it has a password, the API answers nothing but the call
//! that sets one. Reading apps that cannot keep a cookie send name and password
//! with every call instead; see `basic`.

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
    extract::{ConnectInfo, Query, Request, State},
    http::{header, HeaderMap, HeaderValue},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use base64::Engine;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
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
/// Where the highest number an account ever had is kept. A number is not given out
/// twice: it names the account's folder, which may outlive the account for a moment.
const LAST_ID: &str = "users.last_id";
/// Endings of names that only a local network resolves.
const LOCAL_ENDINGS: [&str; 5] = [".localhost", ".local", ".lan", ".internal", ".home.arpa"];

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
}

/// What an admin may change about an account. Whatever is left out stays.
#[derive(Deserialize, Default)]
pub struct AccountChange {
    name: Option<String>,
    password: Option<String>,
    admin: Option<bool>,
}

/// How Mango is reached, as the owner described it.
#[derive(Clone, Default)]
pub struct Front {
    /// Names Mango goes by; see `Auth::answers`.
    pub hosts: Vec<String>,
    /// A proxy stands in front and writes each caller's address into `X-Forwarded-For`.
    pub proxied: bool,
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
    signed_in: bool,
    /// More than one account exists, so signing in takes a name.
    named: bool,
    name: Option<String>,
    admin: bool,
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

/// Whether two setup codes are the same, however they were typed. They are compared by
/// hash, so the time it takes says nothing about where they differ.
fn same_code(given: &str, code: &str) -> bool {
    let plain = |text: &str| text.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect::<String>();
    token_hash(&plain(given)) == token_hash(&plain(code))
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

fn accounts(conn: &Connection) -> Result<Vec<Account>> {
    let mut stmt = conn.prepare("SELECT id, name, admin, created_at, password IS NOT NULL FROM users ORDER BY id")?;
    let rows = stmt.query_map([], |r| Ok(Account { id: r.get(0)?, name: r.get(1)?, admin: r.get(2)?, created_at: r.get(3)?, has_password: r.get(4)? }))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Removes the owner's password and signs the owner out. For `mango reset-password`.
pub fn reset(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE users SET password = NULL WHERE id = ?1", [OWNER])?;
    conn.execute("DELETE FROM sessions WHERE user_id = ?1", [OWNER])?;
    Ok(())
}

impl Auth {
    /// `first_password` is used only while the owner has none.
    pub async fn open(db: Db, on: bool, port: u16, first_password: Option<String>, front: Front) -> Result<Arc<Self>> {
        let (accounts, sessions) = db
            .call(|conn| {
                conn.execute("DELETE FROM sessions WHERE expires_at <= ?1 OR user_id NOT IN (SELECT id FROM users)", [db::now()])?;
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
            proof: challenge.and_then(|challenge| self.proof(challenge)),
            signed_in: who.is_some(),
            named: self.on && locked(&self.accounts).len() > 1,
            admin: who.as_ref().is_some_and(|who| who.admin),
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

    /// `name` may be left empty while there is one account only.
    pub async fn login(&self, from: IpAddr, name: &str, password: String, agent: String) -> Result<String> {
        if !self.has_password() {
            return Err(setup_required());
        }
        let who = self.check_password(from, name, password).await?;
        self.start_session(who.id, agent).await
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
                let last: i64 = db::state(&tx, LAST_ID)?.and_then(|text| text.parse().ok()).unwrap_or(0);
                // The owner's place is kept, even before the owner exists.
                let id: i64 = tx.query_row("SELECT max(1, ?1, coalesce(max(id), 0)) + 1 FROM users", [last], |r| r.get(0))?;
                tx.execute("INSERT INTO users (id, name, password, admin, created_at) VALUES (?1, ?2, ?3, ?4, ?5)", params![id, name, hash, admin, db::now()])
                    .map_err(name_taken)?;
                db::set_state(&tx, LAST_ID, &id.to_string())?;
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
        let wait = self.wait(from);
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
        let who = self.check_password(from, name, password.to_owned()).await?;
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

    /// Seconds this address must still wait before it may try a password.
    fn wait(&self, from: IpAddr) -> i64 {
        let Some((count, last)) = locked(&self.strikes).get(&from).copied() else { return 0 };
        if count < FREE_TRIES {
            return 0;
        }
        let pause = (1i64 << (count - FREE_TRIES).min(9)).min(LONGEST_WAIT);
        (last + pause - db::now()).max(0)
    }

    fn strike(&self, from: IpAddr) {
        let now = db::now();
        let mut strikes = locked(&self.strikes);
        if strikes.len() > 4096 {
            strikes.retain(|_, (_, last)| now - *last < 3600);
        }
        let entry = strikes.entry(from).or_insert((0, now));
        // An hour of quiet wipes the slate.
        if now - entry.1 > 3600 {
            entry.0 = 0;
        }
        *entry = (entry.0 + 1, now);
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
        .route("/logout", post(logout))
        .route("/password", put(change))
}

#[derive(Deserialize)]
struct Password {
    password: String,
    /// The account. Only signing in reads it, and only when there are several.
    #[serde(default)]
    name: String,
    /// The password in use now. Only a change asks for it.
    #[serde(default)]
    current: String,
    /// The setup code. Only setting the first password from another machine asks for it.
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
    let token = server.auth.login(from, &body.name, body.password, agent(&headers)).await?;
    Ok(with_cookie(&server.auth, &token, &headers))
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
        let front = Front { hosts: Vec::new(), proxied: true };
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

        assert_eq!(auth.login(here, "", "wrong horse".into(), String::new()).await.unwrap_err().code, "wrong_password");
        let second = auth.login(here, "", "correct horse".into(), String::new()).await.unwrap();
        assert_ne!(first, second);

        // A change keeps the caller and signs the other browser out.
        let headers = with(&auth, &second);
        assert!(auth.change(here, &headers, "wrong horse".into(), "battery staple".into()).await.is_err());
        auth.change(here, &headers, "correct horse".into(), "battery staple".into()).await.unwrap();
        assert!(auth.who(&headers).is_some());
        assert!(auth.who(&with(&auth, &first)).is_none());
        assert!(auth.login(here, "", "correct horse".into(), String::new()).await.is_err());

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
        assert!(auth.login(here, "", "correct horse".into(), String::new()).await.is_err());
        assert!(auth.login(here, "kid", "correct horse".into(), String::new()).await.is_err());
        let token = auth.login(here, "KID", "battery staple".into(), String::new()).await.unwrap();
        let headers = with(&auth, &token);
        assert_eq!(auth.who(&headers).unwrap(), Who { id: 2, name: "Kid".into(), admin: false });
        assert!(auth.who(&headers).unwrap().admin().is_err());

        // A reading app sends name and password with the call.
        assert_eq!(auth.basic(here, &basic("kid:battery staple")).await.unwrap().id, 2);
        assert_eq!(auth.basic(here, &basic("kid:nope nope")).await.unwrap_err().code, "wrong_password");
        assert_eq!(auth.basic(here, &HeaderMap::new()).await.unwrap_err().code, "signed_out");

        // A new password from the admin signs the account out everywhere.
        auth.edit(2, AccountChange { name: Some("Sis".into()), password: Some("another staple".into()), admin: Some(true) }).await.unwrap();
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
        let front = Front { hosts: hosts.clone(), proxied: false };
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
            assert_eq!(auth.login(here, "", "nope nope".into(), String::new()).await.unwrap_err().code, "wrong_password");
        }
        // Even the right password waits now, but only from that address.
        assert_eq!(auth.login(here, "", "correct horse".into(), String::new()).await.unwrap_err().code, "too_many_tries");
        assert!(auth.login(there, "", "correct horse".into(), String::new()).await.is_ok());
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
}
