//! The web UI, built by Vite into `ui/dist` and compiled into the binary.

use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "ui/dist"]
struct Assets;

/// The page may load its own scripts, styles and fonts, and images from Mango only.
/// Inline styles are allowed because the UI sets colours and sizes through them.
const POLICY: &str = "default-src 'self'; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; \
                      object-src 'none'; base-uri 'self'; frame-ancestors 'self'";

/// Serves a built file, or the app's page for any path that is not a file.
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if let Some(file) = Assets::get(path) {
        let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
        return ([(header::CONTENT_TYPE, content_type(path)), (header::CACHE_CONTROL, cache)], file.data).into_response();
    }
    // A missing file stays missing; only page addresses fall through to the app.
    if path.rsplit('/').next().is_some_and(|name| name.contains('.')) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match Assets::get("index.html") {
        Some(page) => (
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (header::CACHE_CONTROL, "no-cache"),
                (header::CONTENT_SECURITY_POLICY, POLICY),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
            page.data,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        _ => "application/octet-stream",
    }
}
