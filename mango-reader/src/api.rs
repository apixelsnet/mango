//! The HTTP API under `/api`. JSON in and out; errors are `{"error": {"code", "message"}}`.
//! Everything else the server is asked for is the web UI.

use crate::{
    auth::{self, Account, AccountChange, Auth, Invite, Who},
    backups::{Backups, Saved},
    db::Db,
    downloads::{Downloads, Orphan, QueueView, ReadFiles},
    updates::{Status, Updates, UpdatesView},
    version::{self, Releases},
    error::{AppError, Result},
    export, files,
    config::ProxySetting,
    host::{Host, Proxy, Solver},
    images::{Image, Images, MAX_COVER},
    import::{self, Importer, Preview},
    library::{self, Bulk, Category, LibraryView, Stats},
    migrate::{self, Comparison, Outcome},
    notify::Notifier,
    opds, push, settings,
    spaces::Spaces,
    store::{CatalogItem, CatalogQuery, Installed, StoreInfo, Stores},
    titles::{Browse, Edit, Listing, Namesake, PageMark, ReaderView, TitleView, Titles},
    trackers::{self, Change, Credentials, Kind, Pick, Track, Trackers},
    ui,
};
use axum::{
    body::{to_bytes, Body, Bytes},
    extract::{DefaultBodyLimit, FromRequestParts, Path, Query, Request, State},
    http::{header, request::Parts, HeaderMap, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use flate2::{write::GzEncoder, Compression};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{io::Write, sync::Arc};

/// For answers whose address changes when their content does.
const FOREVER: &str = "private, max-age=31536000, immutable";
/// Text shorter than this goes out as it is: gzip would save next to nothing on it.
const GZIP_FROM: usize = 1024;

/// What a request works with: the library of the account it comes from, and what
/// all accounts share. `spaces.rs` keeps one per account.
#[derive(Clone)]
pub struct AppState {
    /// The account asking.
    pub me: Who,
    pub db: Db,
    pub auth: Arc<Auth>,
    pub releases: Arc<Releases>,
    pub host: Arc<Host>,
    pub stores: Stores,
    pub titles: Titles,
    pub images: Images,
    pub downloads: Downloads,
    pub updates: Updates,
    pub importer: Importer,
    pub trackers: Trackers,
    pub notifier: Notifier,
    pub backups: Backups,
    /// The local source's folder, shown in Settings so the owner knows where to put files.
    pub local_dir: String,
}

/// The library of whoever is asking. `auth::require` put it there.
pub struct Me(pub AppState);

impl<S: Send + Sync> FromRequestParts<S> for Me {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self> {
        parts.extensions.remove::<AppState>().map(Me).ok_or_else(auth::signed_out)
    }
}

pub fn router(state: Spaces) -> Router {
    // Everything here wants a signed-in browser; `/auth` below is what works without one.
    let private = Router::new()
        .route("/health", get(health))
        .route("/version", get(about))
        .route("/version/check", post(check_version))
        .route("/settings", get(get_settings).put(put_settings))
        .route("/users", get(list_users).post(add_user))
        .route("/users/{id}", put(edit_user).delete(remove_user))
        .route("/invites", get(list_invites).post(add_invite))
        .route("/invites/{id}", delete(remove_invite))
        .route("/solver", get(get_solver).put(put_solver))
        .route("/proxy", get(get_proxy).put(put_proxy))
        .route("/stores", get(list_stores).post(add_store))
        .route("/stores/refresh", post(refresh_stores))
        .route("/stores/{id}", delete(remove_store))
        .route("/stores/{id}/refresh", post(refresh_store))
        .route("/stores/{id}/accept-key", post(accept_key))
        .route("/catalog", get(catalog))
        .route("/catalog/{store}/{pkg}", get(catalog_item))
        .route("/extensions", get(installed))
        .route("/extensions/update-all", post(update_all))
        .route("/extensions/{pkg}", delete(remove_extension))
        .route("/extensions/{pkg}/install", post(install))
        .route("/sources", get(sources))
        .route("/sources/{id}", put(change_source))
        .route("/sources/{id}/preferences", get(source_preferences).put(set_source_preference))
        .route("/sources/{id}/filters", get(source_filters))
        .route("/sources/{id}/{kind}", get(browse))
        .route("/manga/{id}", get(title))
        .route("/manga/{id}/refresh", post(refresh_title))
        .route("/manga/{id}/cover", get(cover).put(set_cover).layer(DefaultBodyLimit::max(MAX_COVER + 1024)))
        .route("/manga/{id}/details", put(edit_details).delete(unlock_details))
        .route("/manga/{id}/scanlators", put(set_scanlators))
        .route("/manga/{id}/merge/{other}", post(merge_title).delete(unmerge_title))
        .route("/manga/{id}/namesakes", get(namesakes))
        .route("/manga/{id}/marks", get(title_marks))
        .route("/manga/{id}/library", put(set_library))
        .route("/manga/{id}/reader", put(set_reader))
        .route("/manga/{id}/chapters", put(mark_chapters))
        .route("/chapters/{id}", get(reader))
        .route("/chapters/{id}/pages", get(pages))
        .route("/chapters/{id}/pages/{index}", get(page))
        .route("/chapters/{id}/progress", put(progress))
        .route("/chapters/{id}/marks/{page}", put(mark_page).delete(unmark_page))
        .route("/library", get(library))
        .route("/library/bulk", post(bulk))
        .route("/stats", get(stats))
        .route("/categories", get(list_categories).post(add_category).put(order_categories))
        .route("/categories/{id}", put(rename_category).delete(remove_category))
        .route("/manga/{id}/categories", put(set_categories))
        .route("/history", get(history).delete(clear_history))
        .route("/history/{chapter}", delete(forget))
        .route("/updates", get(updates))
        .route("/updates/fresh", get(fresh_updates))
        .route("/updates/run", post(run_updates))
        .route("/notify/test", post(test_notify))
        .route("/push", get(push_status).put(push_add).delete(push_remove))
        .route("/push/test", post(push_test))
        .route("/downloads", get(downloads).post(queue_downloads).delete(clear_downloads))
        .route("/downloads/order", put(order_downloads))
        .route("/downloads/pause", post(pause_downloads))
        .route("/downloads/resume", post(resume_downloads))
        .route("/downloads/retry", post(retry_downloads))
        .route("/downloads/orphans", get(orphans).delete(delete_orphans))
        .route("/downloads/read", get(read_files).delete(delete_read))
        .route("/cache", get(cache_size).delete(clear_cache))
        .route("/downloads/{chapter}", delete(cancel_download))
        .route("/downloads/{chapter}/retry", post(retry_download))
        .route("/chapters/{id}/download", delete(delete_download))
        .route("/chapters/{id}/file", get(chapter_file))
        .route("/manga/{id}/auto-download", put(set_auto_download))
        .route("/manga/{id}/migrate/{to}", get(compare_titles).post(migrate_title))
        .route("/import",get(import_status).post(start_import).layer(DefaultBodyLimit::max(import::MAX_UPLOAD)))
        .route("/import/preview", post(preview_import).layer(DefaultBodyLimit::max(import::MAX_UPLOAD)))
        .route("/export", get(export_library))
        .route("/export/files", get(export_files))
        .route("/export/files/size", get(files_size))
        .route("/backups", get(list_backups).post(make_backup))
        .route("/backups/{name}", get(backup_file).delete(remove_backup))
        .route("/trackers", get(list_trackers))
        .route("/trackers/{id}/authorize", post(authorize_tracker))
        .route("/trackers/{id}/login", post(login_tracker).delete(logout_tracker))
        .route("/trackers/{id}/search", get(search_tracker))
        .route("/manga/{id}/tracks", get(tracks))
        .route("/manga/{id}/tracks/{tracker}", post(bind_track).put(edit_track).delete(unbind_track))
        .route("/manga/{id}/tracks/{tracker}/refresh", post(refresh_track))
        .layer(middleware::from_fn_with_state(state.clone(), auth::require));
    let api = Router::new()
        .nest("/auth", auth::routes())
        .merge(private)
        .fallback(no_route)
        .layer(middleware::from_fn(same_site_only))
        .layer(middleware::from_fn(kept_nowhere))
        .with_state(state.clone());
    Router::new()
        .nest("/api", api)
        // A nested router answers `/opds` but not `/opds/`, which apps are as often given.
        .merge(opds::slashed(state.clone()))
        .nest("/opds", opds::routes(state.clone()))
        .fallback(ui::serve)
        // The inner one first: an answer that is not sent again is not gzipped either.
        .layer(middleware::from_fn(sent_once))
        .layer(middleware::from_fn(gzipped))
        .layer(middleware::from_fn_with_state(state, known_host_only))
}

/// An answer marked `no-cache` is asked for again every time it is wanted: the page, a
/// feed, a cover without its version. It gets a mark made from its content, and a caller
/// that shows the same mark is told that what it holds still stands, with nothing sent
/// after. Such answers are whole in memory already.
async fn sent_once(request: Request, next: Next) -> Response {
    let got = request.method() == Method::GET;
    let held = request.headers().get(header::IF_NONE_MATCH).cloned();
    let response = next.run(request).await;
    let asked_again = response.headers().get(header::CACHE_CONTROL).is_some_and(|kept| kept == "no-cache");
    if !got || !asked_again || response.status() != StatusCode::OK {
        return response;
    }
    let (mut head, body) = response.into_parts();
    let Ok(bytes) = to_bytes(body, usize::MAX).await else {
        return AppError::internal("an answer broke off").into_response();
    };
    // A weak mark: the gzipped answer and the plain one carry the same.
    let mark = format!("W/\"{}\"", hex::encode(&Sha256::digest(&bytes)[..8]));
    if let Ok(value) = HeaderValue::from_str(&mark) {
        head.headers.insert(header::ETAG, value);
    }
    if held.as_ref().and_then(|held| held.to_str().ok()).is_some_and(|held| names(held, &mark)) {
        head.status = StatusCode::NOT_MODIFIED;
        head.headers.remove(header::CONTENT_LENGTH);
        return Response::from_parts(head, Body::empty());
    }
    Response::from_parts(head, Body::from(bytes))
}

/// Whether a caller's `If-None-Match` names this mark. Weak or strong is the same here.
fn names(held: &str, mark: &str) -> bool {
    fn bare(mark: &str) -> &str {
        mark.trim().trim_start_matches("W/")
    }
    held.split(',').any(|one| one.trim() == "*" || bare(one) == bare(mark))
}

/// Text of some size goes out gzipped to a caller that reads gzip: the page's script and
/// styles, JSON, the feeds. Pictures and archives are packed already and pass untouched.
async fn gzipped(request: Request, next: Next) -> Response {
    let reads = reads_gzip(request.headers());
    let mut response = next.run(request).await;
    let kind = response.headers().get(header::CONTENT_TYPE).and_then(|kind| kind.to_str().ok()).unwrap_or("");
    let text = kind.starts_with("text/") || kind.contains("json") || kind.contains("xml");
    if !text || response.headers().contains_key(header::CONTENT_ENCODING) {
        return response;
    }
    // A cache between Mango and its callers has to keep the two forms apart.
    response.headers_mut().append(header::VARY, HeaderValue::from_static("Accept-Encoding"));
    if !reads {
        return response;
    }
    let (mut head, body) = response.into_parts();
    let Ok(bytes) = to_bytes(body, usize::MAX).await else {
        return AppError::internal("an answer broke off").into_response();
    };
    if bytes.len() < GZIP_FROM {
        return Response::from_parts(head, Body::from(bytes));
    }
    let plain = bytes.clone();
    let packed = tokio::task::spawn_blocking(move || {
        let mut gzip = GzEncoder::new(Vec::with_capacity(plain.len() / 4), Compression::default());
        gzip.write_all(&plain)?;
        gzip.finish()
    })
    .await;
    match packed {
        Ok(Ok(packed)) => {
            head.headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
            head.headers.remove(header::CONTENT_LENGTH);
            Response::from_parts(head, Body::from(packed))
        }
        _ => Response::from_parts(head, Body::from(bytes)),
    }
}

/// Whether a caller says it reads gzip, and does not take it back with `q=0`.
fn reads_gzip(headers: &HeaderMap) -> bool {
    headers.get_all(header::ACCEPT_ENCODING).iter().filter_map(|value| value.to_str().ok()).flat_map(|value| value.split(',')).any(|coding| {
        let mut parts = coding.split(';').map(str::trim);
        let named = parts.next().is_some_and(|name| name.eq_ignore_ascii_case("gzip"));
        named && !parts.any(|part| part.strip_prefix("q=").and_then(|q| q.parse::<f32>().ok()) == Some(0.0))
    })
}

/// Refuses a call that names Mango by a host it does not go by; see `Auth::answers`.
async fn known_host_only(State(server): State<Spaces>, request: Request, next: Next) -> Response {
    if !server.auth.answers(request.headers()) {
        let why = "Mango does not go by that name. Open it by its address, or add the name to MANGO_HOSTS.";
        return AppError::new(403, "unknown_host", why).into_response();
    }
    next.run(request).await
}

/// An answer of the API is one account's own and may differ the next moment. No browser
/// or proxy keeps a copy, unless the answer itself says how long it holds, as pictures do.
async fn kept_nowhere(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response.headers_mut().entry(header::CACHE_CONTROL).or_insert(HeaderValue::from_static("no-store"));
    response
}

/// Refuses changes that a page on another site sends through the owner's browser.
/// The session cookie is not sent along with them from another site either.
async fn same_site_only(request: Request, next: Next) -> Response {
    let changes_something = !matches!(*request.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if changes_something && from_elsewhere(request.headers()) {
        return AppError::new(403, "cross_site", "Requests from other sites are not accepted").into_response();
    }
    next.run(request).await
}

/// Whether a request was sent by a page that is not Mango's own. Browsers label such
/// requests, but only where the address is HTTPS or this machine's own. Without the
/// label, what tells is the page the request says it comes from.
fn from_elsewhere(headers: &HeaderMap) -> bool {
    let header = |name: &str| headers.get(name).and_then(|value| value.to_str().ok());
    match header("sec-fetch-site") {
        Some(site) => site != "same-origin" && site != "none",
        None => header("origin").is_some_and(|origin| {
            let from = origin.split_once("://").map_or(origin, |(_, host)| host);
            // A proxy may put its own name for Mango in `Host`, and the one the browser used beside it.
            !["host", "x-forwarded-host"].into_iter().filter_map(header).any(|host| host.eq_ignore_ascii_case(from))
        }),
    }
}

async fn no_route() -> AppError {
    AppError::not_found("No such endpoint")
}

fn done() -> Json<Value> {
    Json(json!({ "ok": true }))
}

fn picture(image: Image, cache: &'static str) -> Response {
    let headers =
        [(header::CONTENT_TYPE, image.content_type), (header::CACHE_CONTROL, cache), (header::X_CONTENT_TYPE_OPTIONS, "nosniff")];
    (headers, image.bytes).into_response()
}

async fn health(Me(app): Me) -> Json<Value> {
    let anyone_installs = app.stores.anyone_installs();
    Json(json!({ "version": version::VERSION, "host": app.host.status(), "localDir": app.local_dir, "anyoneInstalls": anyone_installs, "proxySetting": app.host.proxy_setting().as_str() }))
}

/// This Mango's version and what the release list said when it was last asked.
async fn about(Me(app): Me) -> Json<version::View> {
    Json(app.releases.view())
}

/// Asks the release list now.
async fn check_version(Me(app): Me) -> Result<Json<version::View>> {
    Ok(Json(app.releases.check().await?))
}

async fn get_settings(Me(app): Me) -> Result<Json<Value>> {
    Ok(Json(Value::Object(app.db.call(|conn| settings::all(conn)).await?)))
}

/// Stores the keys given; a null removes one. Keys not named are left alone.
async fn put_settings(Me(app): Me, Json(values): Json<Map<String, Value>>) -> Result<Json<Value>> {
    app.db.call(move |conn| settings::put(conn, &values)).await?;
    Ok(done())
}

// ---------- accounts ----------

async fn list_users(Me(app): Me) -> Result<Json<Value>> {
    app.me.admin()?;
    Ok(Json(json!({ "users": app.auth.list() })))
}

#[derive(Deserialize)]
struct NewUser {
    name: String,
    password: String,
    #[serde(default)]
    admin: bool,
}

/// A new account starts with an empty library and no stores.
async fn add_user(State(server): State<Spaces>, Me(app): Me, Json(body): Json<NewUser>) -> Result<Json<Account>> {
    app.me.admin()?;
    let account = app.auth.add(&body.name, body.password, body.admin).await?;
    if let Err(why) = server.create(account.id) {
        let _ = app.auth.remove(account.id).await;
        return Err(why);
    }
    Ok(Json(account))
}

async fn edit_user(Me(app): Me, Path(id): Path<i64>, Json(change): Json<AccountChange>) -> Result<Json<Account>> {
    app.me.admin()?;
    Ok(Json(app.auth.edit(id, change).await?))
}

/// Removes an account with its library, its stores, its downloads and its tracker sign-ins.
async fn remove_user(State(server): State<Spaces>, Me(app): Me, Path(id): Path<i64>) -> Result<Json<Value>> {
    app.me.admin()?;
    if id == app.me.id {
        return Err(AppError::bad_request("That is the account you are signed in with"));
    }
    app.auth.remove(id).await?;
    server.close(id).await;
    Ok(done())
}

// ---------- invitations ----------

async fn list_invites(Me(app): Me) -> Result<Json<Value>> {
    app.me.admin()?;
    Ok(Json(json!({ "invites": app.auth.invites().await? })))
}

#[derive(Deserialize)]
struct NewInvite {
    /// Who it is for. Only admins see it.
    #[serde(default)]
    note: String,
    /// How many days it holds. Left out, a week.
    days: Option<i64>,
}

/// Makes an invitation. The answer carries its code, this once.
async fn add_invite(Me(app): Me, Json(body): Json<NewInvite>) -> Result<Json<Invite>> {
    app.me.admin()?;
    Ok(Json(app.auth.invite(&body.note, body.days).await?))
}

async fn remove_invite(Me(app): Me, Path(id): Path<i64>) -> Result<Json<Value>> {
    app.me.admin()?;
    app.auth.uninvite(id).await?;
    Ok(done())
}

// ---------- the solver ----------

async fn get_solver(Me(app): Me) -> Result<Json<Solver>> {
    app.me.admin()?;
    Ok(Json(app.host.solver()))
}

/// Where the sources send a Cloudflare check to be passed. The extension host is
/// restarted to learn of it, at its next call.
async fn put_solver(State(server): State<Spaces>, Me(app): Me, Json(solver): Json<Solver>) -> Result<Json<Solver>> {
    app.me.admin()?;
    let solver = solver.checked()?;
    let kept = serde_json::to_value(&solver).map_err(|e| AppError::internal(e.to_string()))?;
    server.shared().call(move |conn| settings::set(conn, Solver::KEY, &kept)).await?;
    app.host.set_solver(solver.clone()).await;
    Ok(Json(solver))
}

/// Who may see and set the proxy: an admin, every account, or no one (`MANGO_PROXY_SETTING`).
fn may_set_proxy(app: &AppState) -> Result<()> {
    match app.host.proxy_setting() {
        ProxySetting::Admin => app.me.admin(),
        ProxySetting::Anyone => Ok(()),
        ProxySetting::Off => Err(AppError::new(403, "proxy_off", "The proxy is switched off here")),
    }
}

async fn get_proxy(Me(app): Me) -> Result<Json<serde_json::Value>> {
    may_set_proxy(&app)?;
    Ok(Json(app.host.proxy().shown()))
}

/// The proxy the sources reach sites through, all of them or those named. The extension
/// host is restarted to learn of it, at its next call. A password left empty keeps the one
/// already there, for the same name: it is never sent back to be sent again.
async fn put_proxy(State(server): State<Spaces>, Me(app): Me, Json(mut proxy): Json<Proxy>) -> Result<Json<serde_json::Value>> {
    may_set_proxy(&app)?;
    let before = app.host.proxy();
    if proxy.password.is_empty() && proxy.username.trim() == before.username {
        proxy.password = before.password;
    }
    let proxy = proxy.checked()?;
    let kept = serde_json::to_value(&proxy).map_err(|e| AppError::internal(e.to_string()))?;
    server.shared().call(move |conn| settings::set(conn, Proxy::KEY, &kept)).await?;
    app.host.set_proxy(proxy.clone()).await;
    Ok(Json(proxy.shown()))
}

// ---------- stores ----------

async fn list_stores(Me(app): Me) -> Result<Json<Value>> {
    Ok(Json(json!({ "stores": app.stores.list().await? })))
}

#[derive(Deserialize)]
struct NewStore {
    url: String,
}

async fn add_store(Me(app): Me, Json(body): Json<NewStore>) -> Result<Json<StoreInfo>> {
    Ok(Json(app.stores.add(&body.url).await?))
}

async fn refresh_stores(Me(app): Me) -> Result<Json<Value>> {
    Ok(Json(json!({ "stores": app.stores.refresh_all().await? })))
}

async fn refresh_store(Me(app): Me, Path(id): Path<i64>) -> Result<Json<StoreInfo>> {
    Ok(Json(app.stores.refresh(id).await?))
}

async fn accept_key(Me(app): Me, Path(id): Path<i64>) -> Result<Json<StoreInfo>> {
    Ok(Json(app.stores.accept_key(id).await?))
}

async fn remove_store(Me(app): Me, Path(id): Path<i64>) -> Result<Json<Value>> {
    app.stores.remove_store(id).await?;
    Ok(Json(json!({ "removed": true })))
}

// ---------- catalog ----------

#[derive(Deserialize, Default)]
#[serde(default)]
struct CatalogParams {
    /// Text to find in extension and source names.
    q: String,
    /// Languages, comma separated; any of them.
    lang: String,
    /// Any of safe, mixed, nsfw, comma separated. Left out: safe and mixed.
    content: String,
    /// installed, available or updates.
    state: Option<String>,
    store: Option<i64>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn catalog(Me(app): Me, Query(params): Query<CatalogParams>) -> Result<Json<Value>> {
    let words = |list: &str| list.split(',').map(str::trim).filter(|word| !word.is_empty()).map(str::to_owned).collect::<Vec<_>>();
    let mut levels = Vec::new();
    for word in words(&params.content) {
        match word.as_str() {
            "safe" => levels.extend([0, 1]),
            "mixed" => levels.push(2),
            "nsfw" => levels.push(3),
            _ => return Err(AppError::bad_request("content takes safe, mixed and nsfw")),
        }
    }
    if params.state.as_deref().is_some_and(|state| !matches!(state, "installed" | "available" | "updates")) {
        return Err(AppError::bad_request("state takes installed, available or updates"));
    }
    let query = CatalogQuery {
        text: params.q,
        langs: words(&params.lang),
        levels,
        state: params.state,
        store: params.store,
        limit: params.limit.unwrap_or(60),
        offset: params.offset.unwrap_or(0),
    };
    let (items, total) = app.stores.catalog(query).await?;
    Ok(Json(json!({ "items": items, "total": total })))
}

async fn catalog_item(Me(app): Me, Path((store, pkg)): Path<(i64, String)>) -> Result<Json<CatalogItem>> {
    Ok(Json(app.stores.catalog_item(store, pkg).await?))
}

// ---------- installed extensions and their sources ----------

async fn installed(Me(app): Me) -> Result<Json<Value>> {
    Ok(Json(json!({ "extensions": app.stores.installed().await? })))
}

#[derive(Deserialize)]
struct InstallParams {
    /// The store to install from, when more than one lists the extension.
    store: Option<i64>,
}

/// Installs, or updates to the store's version.
async fn install(Me(app): Me, Path(pkg): Path<String>, Query(params): Query<InstallParams>) -> Result<Json<Installed>> {
    Ok(Json(app.stores.install(&pkg, params.store, app.stores.open_to(&app.me)).await?))
}

async fn update_all(Me(app): Me) -> Result<Json<Value>> {
    Ok(Json(json!({ "results": app.stores.update_all(app.stores.open_to(&app.me)).await? })))
}

async fn remove_extension(Me(app): Me, Path(pkg): Path<String>) -> Result<Json<Value>> {
    app.stores.remove(&pkg).await?;
    Ok(Json(json!({ "removed": true })))
}

async fn sources(Me(app): Me) -> Result<Json<Value>> {
    Ok(Json(json!({ "sources": app.stores.sources().await? })))
}

#[derive(Deserialize)]
struct SourceChange {
    pinned: Option<bool>,
    hidden: Option<bool>,
    /// The folder on Browse; empty takes the source out of its folder.
    folder: Option<String>,
}

async fn change_source(Me(app): Me, Path(id): Path<i64>, Json(body): Json<SourceChange>) -> Result<Json<Value>> {
    app.stores.set_source(id, body.pinned, body.hidden, body.folder).await?;
    Ok(done())
}

/// What a source lets its user set. The host keeps one set per source for every account,
/// a sign-in to the source's site among them: where extensions are an admin's to bring
/// in, the settings are an admin's to see and change.
async fn source_preferences(Me(app): Me, Path(id): Path<i64>) -> Result<Json<Value>> {
    if !app.stores.open_to(&app.me) {
        return Ok(Json(json!({ "preferences": [] })));
    }
    Ok(Json(app.stores.preferences(id).await?))
}

/// Changes one of them: `{position, key, value}`. Answers with the list as it is afterwards.
async fn set_source_preference(Me(app): Me, Path(id): Path<i64>, Json(change): Json<Value>) -> Result<Json<Value>> {
    if !app.stores.open_to(&app.me) {
        app.me.admin()?;
    }
    Ok(Json(app.stores.set_preference(id, &change).await?))
}

// ---------- browse and titles ----------

#[derive(Deserialize, Default)]
#[serde(default)]
struct BrowseParams {
    page: Option<i64>,
    /// The search text. Only `search` reads it.
    q: String,
    /// The filters set to something else than the source starts them at: a JSON list of
    /// `{position, state}`. Only `search` reads it.
    filters: Option<String>,
}

/// `kind` is popular, latest or search.
async fn browse(Me(app): Me, Path((id, kind)): Path<(i64, String)>, Query(params): Query<BrowseParams>) -> Result<Json<Listing>> {
    let kind = Browse::parse(&kind).ok_or_else(|| AppError::not_found("No such endpoint"))?;
    let filters: Vec<Value> = match params.filters.as_deref() {
        Some(text) if !text.is_empty() => serde_json::from_str(text).map_err(|_| AppError::bad_request("filters is not a JSON list"))?,
        _ => Vec::new(),
    };
    Ok(Json(app.titles.browse(id, kind, params.page.unwrap_or(1), &params.q, &filters).await?))
}

/// What a source's search can be narrowed by: `{filters}`, each named by its place in the list.
async fn source_filters(Me(app): Me, Path(id): Path<i64>) -> Result<Json<Value>> {
    Ok(Json(app.titles.filters(id).await?))
}

async fn title(Me(app): Me, Path(id): Path<i64>) -> Result<Json<TitleView>> {
    Ok(Json(app.titles.get(id).await?))
}

async fn refresh_title(Me(app): Me, Path(id): Path<i64>) -> Result<Json<TitleView>> {
    Ok(Json(app.titles.refresh(id).await?))
}

#[derive(Deserialize)]
struct CoverWanted {
    v: Option<String>,
    /// `small` for the copy that grids and lists show.
    size: Option<String>,
}

/// The address carries a version, so with one the answer never changes.
async fn cover(Me(app): Me, Path(id): Path<i64>, Query(wanted): Query<CoverWanted>) -> Result<Response> {
    let image = app.images.cover(id, wanted.size.as_deref() == Some("small")).await?;
    Ok(picture(image, if wanted.v.is_some() { FOREVER } else { "no-cache" }))
}

/// A cover given by hand, as the picture itself, in place of the source's.
async fn set_cover(Me(app): Me, Path(id): Path<i64>, file: Bytes) -> Result<Json<TitleView>> {
    app.titles.get(id).await?;
    let mark = app.images.save_cover(id, file.to_vec()).await?;
    app.titles.set_cover(id, mark).await?;
    Ok(Json(app.titles.get(id).await?))
}

async fn edit_details(Me(app): Me, Path(id): Path<i64>, Json(edit): Json<Edit>) -> Result<Json<TitleView>> {
    app.titles.edit(id, edit).await?;
    Ok(Json(app.titles.get(id).await?))
}

/// Lets the details follow the source again, and asks the source for them.
async fn unlock_details(Me(app): Me, Path(id): Path<i64>) -> Result<Json<TitleView>> {
    app.titles.unlock(id).await?;
    Ok(Json(app.titles.refresh(id).await?))
}

#[derive(Deserialize)]
struct Groups {
    #[serde(default)]
    order: Vec<String>,
    #[serde(default)]
    dedupe: bool,
}

async fn set_scanlators(Me(app): Me, Path(id): Path<i64>, Json(body): Json<Groups>) -> Result<Json<TitleView>> {
    app.titles.set_scanlators(id, body.order, body.dedupe).await?;
    Ok(Json(app.titles.get(id).await?))
}

/// Merges another title into this one. The other's source is asked for its chapters first
/// when it never was, so they show at once.
async fn merge_title(Me(app): Me, Path((id, other)): Path<(i64, i64)>) -> Result<Json<TitleView>> {
    let found = app.titles.get(other).await?;
    if id != other && found.chapters_at.is_none() {
        app.titles.fetch(other, !found.initialized, true).await?;
    }
    app.titles.merge(id, other).await?;
    Ok(Json(app.titles.get(id).await?))
}

async fn unmerge_title(Me(app): Me, Path((id, other)): Path<(i64, i64)>) -> Result<Json<TitleView>> {
    app.titles.unmerge(id, other).await?;
    Ok(Json(app.titles.get(id).await?))
}

/// Library titles by the same name, to warn before one series is added twice.
async fn namesakes(Me(app): Me, Path(id): Path<i64>) -> Result<Json<Vec<Namesake>>> {
    Ok(Json(app.titles.namesakes(id).await?))
}

async fn title_marks(Me(app): Me, Path(id): Path<i64>) -> Result<Json<Vec<PageMark>>> {
    Ok(Json(app.titles.page_marks(id).await?))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Note {
    note: String,
}

async fn mark_page(Me(app): Me, Path((id, page)): Path<(i64, i64)>, Json(body): Json<Note>) -> Result<Json<Value>> {
    app.titles.mark_page(id, page, Some(body.note)).await?;
    Ok(done())
}

async fn unmark_page(Me(app): Me, Path((id, page)): Path<(i64, i64)>) -> Result<Json<Value>> {
    app.titles.mark_page(id, page, None).await?;
    Ok(done())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibraryChange {
    in_library: bool,
}

async fn set_library(Me(app): Me, Path(id): Path<i64>, Json(body): Json<LibraryChange>) -> Result<Json<Value>> {
    app.titles.set_library(id, body.in_library).await?;
    Ok(done())
}

#[derive(Deserialize)]
struct ReaderChange {
    mode: Option<String>,
    fit: Option<String>,
    /// Every other reader setting the title keeps for itself, as the UI shapes them.
    prefs: Option<Value>,
}

async fn set_reader(Me(app): Me, Path(id): Path<i64>, Json(body): Json<ReaderChange>) -> Result<Json<Value>> {
    app.titles.set_reader(id, body.mode, body.fit, body.prefs).await?;
    Ok(done())
}

#[derive(Deserialize)]
struct MarkChange {
    /// Left out: every chapter of the title.
    ids: Option<Vec<i64>>,
    /// Read or not, bookmarked or not: at least one of the two.
    read: Option<bool>,
    bookmark: Option<bool>,
}

async fn mark_chapters(Me(app): Me, Path(id): Path<i64>, Json(body): Json<MarkChange>) -> Result<Json<Value>> {
    if body.read.is_none() && body.bookmark.is_none() {
        return Err(AppError::bad_request("Say what to change: read or bookmark"));
    }
    if let Some(on) = body.bookmark {
        app.titles.bookmark(id, body.ids.clone(), on).await?;
    }
    if let Some(read) = body.read {
        app.titles.mark(id, body.ids, read).await?;
        if read {
            app.trackers.read_title(id);
        }
    }
    Ok(done())
}

// ---------- reader ----------

async fn reader(Me(app): Me, Path(id): Path<i64>) -> Result<Json<ReaderView>> {
    Ok(Json(app.titles.reader(id).await?))
}

async fn pages(Me(app): Me, Path(id): Path<i64>) -> Result<Json<Value>> {
    Ok(Json(json!({ "count": app.images.page_count(id).await? })))
}

async fn page(Me(app): Me, Path((id, index)): Path<(i64, i64)>) -> Result<Response> {
    Ok(picture(app.images.page(id, index).await?, FOREVER))
}

#[derive(Deserialize)]
struct Progress {
    page: i64,
    /// Set to mark the chapter finished or not. Left out, it keeps its state.
    read: Option<bool>,
}

async fn progress(Me(app): Me, Path(id): Path<i64>, Json(body): Json<Progress>) -> Result<Json<Value>> {
    app.titles.progress(id, body.page, body.read).await?;
    if body.read == Some(true) {
        app.downloads.finished_reading(id).await;
        app.trackers.read_chapter(id);
    }
    Ok(done())
}

/// A chapter as a CBZ file, to keep on this device or open in another app.
async fn chapter_file(Me(app): Me, Path(id): Path<i64>) -> Result<Response> {
    opds::chapter_file(&app, id).await
}

// ---------- updates and downloads ----------

/// New chapters of library titles, newest first, with the failures and state of the last run.
async fn updates(Me(app): Me, Query(slice): Query<Slice>) -> Result<Json<UpdatesView>> {
    Ok(Json(app.updates.view(slice.limit.unwrap_or(200)).await?))
}

/// Only the number beside Updates in the menu, which every screen change asks for.
async fn fresh_updates(Me(app): Me) -> Result<Json<Value>> {
    let fresh = app.db.call(|conn| crate::updates::fresh(conn)).await?;
    Ok(Json(json!({ "fresh": fresh })))
}

/// Starts a library update, unless one is running. Answers at once.
async fn run_updates(Me(app): Me) -> Json<Status> {
    Json(app.updates.start())
}

/// Sends a message to the address set for notifications, so it can be tried.
async fn test_notify(Me(app): Me) -> Result<Json<Value>> {
    app.notifier.test().await?;
    Ok(done())
}

/// The key a browser subscribes with, and how many devices hear of new chapters.
async fn push_status(Me(app): Me) -> Result<Json<Value>> {
    let (key, devices) = app.notifier.push.status().await?;
    Ok(Json(json!({ "key": key, "devices": devices })))
}

/// A device that turned notifications on, with its browser's subscription.
async fn push_add(Me(app): Me, headers: HeaderMap, Json(subscription): Json<push::Subscription>) -> Result<Json<Value>> {
    let origin = headers.get(header::ORIGIN).and_then(|origin| origin.to_str().ok()).map(str::to_owned);
    app.notifier.push.add(subscription, origin).await?;
    Ok(done())
}

#[derive(Deserialize)]
struct PushDevice {
    endpoint: String,
}

/// A device that turned notifications off.
async fn push_remove(Me(app): Me, Json(device): Json<PushDevice>) -> Result<Json<Value>> {
    app.notifier.push.remove(device.endpoint).await?;
    Ok(done())
}

/// Sends a note to every device of the account, so it can be tried.
async fn push_test(Me(app): Me) -> Result<Json<Value>> {
    let note = push::Note { title: "Mango", body: "Notifications reach this device.", url: "#/settings" };
    Ok(Json(json!({ "sent": app.notifier.push.send(&note).await? })))
}

async fn downloads(Me(app): Me) -> Result<Json<QueueView>> {
    Ok(Json(app.downloads.list().await?))
}

#[derive(Deserialize)]
struct ToQueue {
    /// Chapter ids.
    #[serde(default)]
    chapters: Vec<i64>,
    /// Title ids: their unread chapters are queued.
    #[serde(default)]
    titles: Vec<i64>,
}

async fn queue_downloads(Me(app): Me, Json(body): Json<ToQueue>) -> Result<Json<Value>> {
    let mut queued = app.downloads.enqueue(body.chapters).await?;
    if !body.titles.is_empty() {
        queued += app.downloads.enqueue_unread(body.titles).await?;
    }
    Ok(Json(json!({ "queued": queued })))
}

async fn clear_downloads(Me(app): Me) -> Result<Json<Value>> {
    app.downloads.cancel(None).await?;
    Ok(done())
}

async fn cancel_download(Me(app): Me, Path(chapter): Path<i64>) -> Result<Json<Value>> {
    app.downloads.cancel(Some(chapter)).await?;
    Ok(done())
}

async fn retry_download(Me(app): Me, Path(chapter): Path<i64>) -> Result<Json<Value>> {
    app.downloads.retry(chapter).await?;
    Ok(done())
}

/// Every chapter that gave up gets its tries again.
async fn retry_downloads(Me(app): Me) -> Result<Json<Value>> {
    Ok(Json(json!({ "queued": app.downloads.retry_all().await? })))
}

/// Titles outside the library that still have chapters saved.
async fn orphans(Me(app): Me) -> Result<Json<Vec<Orphan>>> {
    Ok(Json(app.downloads.orphans().await?))
}

async fn delete_orphans(Me(app): Me) -> Result<Json<Value>> {
    Ok(Json(json!({ "deleted": app.downloads.delete_orphans().await? })))
}

/// The saved chapters already read, and what their files take.
async fn read_files(Me(app): Me) -> Result<Json<ReadFiles>> {
    Ok(Json(app.downloads.read_files().await?))
}

/// What this account's page cache takes.
async fn cache_size(Me(app): Me) -> Json<Value> {
    Json(json!({ "bytes": app.images.cache_size().await }))
}

async fn clear_cache(Me(app): Me) -> Json<Value> {
    Json(json!({ "freed": app.images.clear_cache().await }))
}

async fn delete_read(Me(app): Me) -> Result<Json<Value>> {
    Ok(Json(json!({ "deleted": app.downloads.delete_read().await? })))
}

async fn order_downloads(Me(app): Me, Json(body): Json<Ids>) -> Result<Json<Value>> {
    app.downloads.reorder(body.ids).await?;
    Ok(done())
}

async fn pause_downloads(Me(app): Me) -> Result<Json<Value>> {
    app.downloads.set_paused(true).await?;
    Ok(done())
}

async fn resume_downloads(Me(app): Me) -> Result<Json<Value>> {
    app.downloads.set_paused(false).await?;
    Ok(done())
}

/// Deletes a downloaded chapter's file.
async fn delete_download(Me(app): Me, Path(id): Path<i64>) -> Result<Json<Value>> {
    app.downloads.delete(id).await?;
    Ok(done())
}

#[derive(Deserialize)]
struct Switch {
    on: bool,
}

async fn set_auto_download(Me(app): Me, Path(id): Path<i64>, Json(body): Json<Switch>) -> Result<Json<Value>> {
    app.titles.set_auto_download(id, body.on).await?;
    Ok(done())
}

// ---------- migration ----------

/// A title beside the one it would move to, and what would go along. The other title's
/// source is asked for its chapters first, so the two are compared as they are today.
async fn compare_titles(Me(app): Me, Path((id, to)): Path<(i64, i64)>) -> Result<Json<Comparison>> {
    let target = app.titles.get(to).await?;
    if id != to {
        app.titles.fetch(to, !target.initialized, true).await?;
    }
    Ok(Json(app.db.call(move |conn| migrate::compare(conn, id, to)).await?))
}

/// Moves a title's place in the library, its progress, categories and tracker links to
/// another title, or copies them. Its source is only asked when it never was.
async fn migrate_title(
    Me(app): Me,
    Path((id, to)): Path<(i64, i64)>,
    Json(options): Json<migrate::Options>,
) -> Result<Json<Outcome>> {
    let target = app.titles.get(to).await?;
    if id != to && target.chapters_at.is_none() {
        app.titles.fetch(to, !target.initialized, true).await?;
    }
    Ok(Json(app.db.call(move |conn| migrate::apply(conn, id, to, &options)).await?))
}

// ---------- import ----------

/// What a Mihon or Suwayomi backup holds. The body is the `.tachibk` file itself.
async fn preview_import(Me(app): Me, file: Bytes) -> Result<Json<Preview>> {
    Ok(Json(app.importer.preview(file.to_vec()).await?))
}

/// Starts importing the backup in the body. Answers at once; `GET /import` follows it.
async fn start_import(Me(app): Me, file: Bytes) -> Result<Json<import::Status>> {
    Ok(Json(app.importer.start(file.to_vec())?))
}

async fn import_status(Me(app): Me) -> Json<import::Status> {
    Json(app.importer.status())
}

/// The library as a `.tachibk` that Mihon, Suwayomi and Mango itself read.
async fn export_library(Me(app): Me) -> Result<Response> {
    let file = app.db.call(|conn| export::write(conn)).await?;
    let headers = [
        (header::CONTENT_TYPE, "application/octet-stream".to_owned()),
        (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", export::file_name())),
        (header::CACHE_CONTROL, "no-store".to_owned()),
    ];
    Ok((headers, file).into_response())
}

/// Every downloaded chapter as one zip, laid out as a local source reads it.
async fn export_files(Me(app): Me) -> Result<Response> {
    let titles = files::plan(&app.db, &app.images, app.downloads.dir()).await?;
    if titles.is_empty() {
        return Err(AppError::new(404, "nothing_downloaded", "No chapter is downloaded, so there are no files to export."));
    }
    let headers = [
        (header::CONTENT_TYPE, "application/zip".to_owned()),
        (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", files::file_name())),
        (header::CACHE_CONTROL, "no-store".to_owned()),
    ];
    Ok((headers, files::send(titles)).into_response())
}

async fn files_size(Me(app): Me) -> Result<Json<files::Size>> {
    Ok(Json(files::size(&files::plan(&app.db, &app.images, app.downloads.dir()).await?)))
}

// ---------- backups made on a schedule ----------

async fn list_backups(Me(app): Me) -> Json<Value> {
    Json(json!({ "backups": app.backups.list().await }))
}

async fn make_backup(Me(app): Me) -> Result<Json<Saved>> {
    Ok(Json(app.backups.make().await?))
}

async fn backup_file(Me(app): Me, Path(name): Path<String>) -> Result<Response> {
    let path = app.backups.file(&name)?;
    let file = tokio::task::spawn_blocking(move || std::fs::read(path)).await.map_err(|e| AppError::internal(e.to_string()))??;
    let headers = [
        (header::CONTENT_TYPE, "application/octet-stream".to_owned()),
        (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        (header::CACHE_CONTROL, "no-store".to_owned()),
    ];
    Ok((headers, file).into_response())
}

async fn remove_backup(Me(app): Me, Path(name): Path<String>) -> Result<Json<Value>> {
    app.backups.delete(&name).await?;
    Ok(done())
}

// ---------- trackers ----------

fn tracker(id: i64) -> Result<Kind> {
    Kind::from_id(id).ok_or_else(|| AppError::not_found("No such tracker"))
}

async fn list_trackers(Me(app): Me) -> Result<Json<Value>> {
    Ok(Json(json!({ "trackers": app.trackers.list().await? })))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Authorize {
    client_id: String,
    /// Where the tracker sends the browser back to: Mango's own address.
    redirect: String,
}

/// Starts a sign-in that goes through the tracker's own page.
async fn authorize_tracker(Me(app): Me, Path(id): Path<i64>, Json(body): Json<Authorize>) -> Result<Json<Value>> {
    Ok(Json(json!({ "url": app.trackers.authorize(tracker(id)?, body.client_id, body.redirect).await? })))
}

async fn login_tracker(Me(app): Me, Path(id): Path<i64>, Json(body): Json<Credentials>) -> Result<Json<Value>> {
    app.trackers.login(tracker(id)?, body).await?;
    Ok(done())
}

async fn logout_tracker(Me(app): Me, Path(id): Path<i64>) -> Result<Json<Value>> {
    app.trackers.logout(tracker(id)?).await?;
    Ok(done())
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Wanted {
    q: String,
}

async fn search_tracker(Me(app): Me, Path(id): Path<i64>, Query(wanted): Query<Wanted>) -> Result<Json<Value>> {
    Ok(Json(json!({ "results": app.trackers.search(tracker(id)?, &wanted.q).await? })))
}

/// The trackers and this title's links to them. Asks no tracker.
async fn tracks(Me(app): Me, Path(id): Path<i64>) -> Result<Json<trackers::View>> {
    Ok(Json(app.trackers.view(id).await?))
}

async fn bind_track(Me(app): Me, Path((id, kind)): Path<(i64, i64)>, Json(pick): Json<Pick>) -> Result<Json<Track>> {
    Ok(Json(app.trackers.bind(id, tracker(kind)?, pick).await?))
}

async fn edit_track(Me(app): Me, Path((id, kind)): Path<(i64, i64)>, Json(change): Json<Change>) -> Result<Json<Track>> {
    Ok(Json(app.trackers.edit(id, tracker(kind)?, change).await?))
}

async fn refresh_track(Me(app): Me, Path((id, kind)): Path<(i64, i64)>) -> Result<Json<Track>> {
    Ok(Json(app.trackers.refresh(id, tracker(kind)?).await?))
}

async fn unbind_track(Me(app): Me, Path((id, kind)): Path<(i64, i64)>) -> Result<Json<Value>> {
    app.trackers.unbind(id, tracker(kind)?).await?;
    Ok(done())
}

// ---------- library, categories, history ----------

/// Every library title with its counts, the categories, and where to go on reading.
async fn library(Me(app): Me) -> Result<Json<LibraryView>> {
    Ok(Json(app.db.call(|conn| library::read(conn)).await?))
}

async fn stats(Me(app): Me) -> Result<Json<Stats>> {
    Ok(Json(app.db.call(|conn| library::stats(conn)).await?))
}

#[derive(Deserialize)]
struct BulkChange {
    /// Title ids.
    ids: Vec<i64>,
    /// read, unread, remove, categories, or download (their unread chapters).
    action: String,
    #[serde(default)]
    categories: Vec<i64>,
    /// With `categories`: take the titles out of every other category.
    #[serde(default)]
    replace: bool,
}

async fn bulk(Me(app): Me, Json(body): Json<BulkChange>) -> Result<Json<Value>> {
    let action = match body.action.as_str() {
        "download" => {
            app.downloads.enqueue_unread(body.ids).await?;
            return Ok(done());
        }
        "read" => Bulk::Read,
        "unread" => Bulk::Unread,
        "remove" => Bulk::Remove,
        "categories" => Bulk::Categories { ids: body.categories, replace: body.replace },
        _ => return Err(AppError::bad_request("action takes read, unread, remove or categories")),
    };
    let (ids, read) = (body.ids.clone(), matches!(action, Bulk::Read));
    app.db.call(move |conn| library::bulk(conn, &body.ids, &action)).await?;
    if read {
        ids.into_iter().for_each(|id| app.trackers.read_title(id));
    }
    Ok(done())
}

#[derive(Deserialize)]
struct NewCategory {
    name: String,
    /// The category it goes inside.
    parent: Option<i64>,
}

/// A name, a place, or both. A parent of 0 puts the category at the top.
#[derive(Deserialize)]
struct CategoryChange {
    name: Option<String>,
    parent: Option<i64>,
}

async fn list_categories(Me(app): Me) -> Result<Json<Value>> {
    Ok(Json(json!({ "categories": app.db.call(|conn| library::categories(conn)).await? })))
}

async fn add_category(Me(app): Me, Json(body): Json<NewCategory>) -> Result<Json<Category>> {
    Ok(Json(app.db.call(move |conn| library::add_category(conn, &body.name, body.parent)).await?))
}

async fn rename_category(Me(app): Me, Path(id): Path<i64>, Json(body): Json<CategoryChange>) -> Result<Json<Value>> {
    app.db
        .call(move |conn| {
            if let Some(name) = &body.name {
                library::rename_category(conn, id, name)?;
            }
            if let Some(parent) = body.parent {
                library::set_parent(conn, id, Some(parent).filter(|parent| *parent > 0))?;
            }
            Ok(())
        })
        .await?;
    Ok(done())
}

async fn remove_category(Me(app): Me, Path(id): Path<i64>) -> Result<Json<Value>> {
    app.db.call(move |conn| library::remove_category(conn, id)).await?;
    Ok(done())
}

#[derive(Deserialize)]
struct Ids {
    ids: Vec<i64>,
}

async fn order_categories(Me(app): Me, Json(body): Json<Ids>) -> Result<Json<Value>> {
    app.db.call(move |conn| library::order_categories(conn, &body.ids)).await?;
    Ok(done())
}

async fn set_categories(Me(app): Me, Path(id): Path<i64>, Json(body): Json<Ids>) -> Result<Json<Value>> {
    app.db.call(move |conn| library::set_categories(conn, id, &body.ids)).await?;
    Ok(done())
}

#[derive(Deserialize)]
struct Slice {
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn history(Me(app): Me, Query(slice): Query<Slice>) -> Result<Json<Value>> {
    let entries = app.db.call(move |conn| library::history(conn, slice.limit.unwrap_or(100), slice.offset.unwrap_or(0))).await?;
    Ok(Json(json!({ "entries": entries })))
}

async fn forget(Me(app): Me, Path(chapter): Path<i64>) -> Result<Json<Value>> {
    app.db.call(move |conn| library::forget(conn, Some(chapter))).await?;
    Ok(done())
}

async fn clear_history(Me(app): Me) -> Result<Json<Value>> {
    app.db.call(|conn| library::forget(conn, None)).await?;
    Ok(done())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        pairs.iter().map(|(name, value)| (header::HeaderName::from_static(name), value.parse().unwrap())).collect()
    }

    #[test]
    fn requests_from_other_sites_are_told_apart() {
        // Where the browser labels a request, the label decides.
        assert!(from_elsewhere(&headers(&[("sec-fetch-site", "cross-site")])));
        assert!(from_elsewhere(&headers(&[("sec-fetch-site", "same-site"), ("origin", "https://mango.example.org"), ("host", "mango.example.org")])));
        assert!(!from_elsewhere(&headers(&[("sec-fetch-site", "same-origin")])));
        assert!(!from_elsewhere(&headers(&[("sec-fetch-site", "none")])));

        // Over plain HTTP there is no label, and the page it comes from has to be Mango's own.
        let at = ("host", "10.0.0.1:8787");
        assert!(!from_elsewhere(&headers(&[("origin", "http://10.0.0.1:8787"), at])));
        assert!(from_elsewhere(&headers(&[("origin", "http://other.example.org"), at])));
        assert!(from_elsewhere(&headers(&[("origin", "http://10.0.0.1:3000"), at])), "another port is another site's page");
        assert!(from_elsewhere(&headers(&[("origin", "null"), at])));
        let proxied = [("origin", "http://mango.example.org"), ("host", "127.0.0.1:8787"), ("x-forwarded-host", "mango.example.org")];
        assert!(!from_elsewhere(&headers(&proxied)), "behind a proxy that names Mango its own way");

        // A reading app or a script sends neither, and is not a page at all.
        assert!(!from_elsewhere(&headers(&[at])));
    }

    #[test]
    fn gzip_goes_to_who_reads_it() {
        assert!(reads_gzip(&headers(&[("accept-encoding", "gzip, deflate, br")])));
        assert!(reads_gzip(&headers(&[("accept-encoding", "br;q=1.0, GZIP;q=0.5")])));
        assert!(!reads_gzip(&headers(&[("accept-encoding", "br, gzip;q=0")])));
        assert!(!reads_gzip(&headers(&[("accept-encoding", "identity")])));
        assert!(!reads_gzip(&headers(&[])));
    }

    #[test]
    fn a_mark_is_known_again() {
        let mark = "W/\"abc\"";
        assert!(names("W/\"abc\"", mark));
        assert!(names("\"other\", \"abc\"", mark), "one of several, and strong for weak");
        assert!(names("*", mark));
        assert!(!names("\"abd\"", mark));
        assert!(!names("", mark));
    }
}
