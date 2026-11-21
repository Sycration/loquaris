//! The built web app (`web/dist`), embedded in release builds and read from
//! disk in debug builds. Paths that are not files get `index.html`, so the
//! browser app's routes work on reload.
use axum::body::Body;
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct WebApp;

const NOT_BUILT: &str = r#"<!doctype html>
<meta charset="utf-8">
<title>Loquaris</title>
<body style="font-family: system-ui, sans-serif; max-width: 40rem; margin: 4rem auto; line-height: 1.5">
<h1>The web app is not built</h1>
<p>This is the Loquaris API server. In development, run <code>npm run dev</code> in
<code>web/</code> and open <a href="http://localhost:5173">http://localhost:5173</a>;
Vite forwards <code>/api</code> to this server.</p>
<p>To serve the app from here, run <code>npm run build</code> in <code>web/</code>
(and rebuild the server in release mode, which embeds it).</p>
</body>"#;

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if let Some(file) = (!path.is_empty()).then(|| WebApp::get(path)).flatten() {
        let mime = file.metadata.mimetype();
        // Vite puts content-hashed files under assets/; they never change.
        let cache = if path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        return ([(CONTENT_TYPE, mime), (CACHE_CONTROL, cache)], Body::from(file.data)).into_response();
    }
    // A missing asset is usually an old file name requested by a page loaded
    // before a new build; answering with index.html would hand the browser
    // HTML in place of a script or stylesheet.
    if path.starts_with("assets/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    match WebApp::get("index.html") {
        Some(index) => ([(CACHE_CONTROL, "no-cache")], Html(Body::from(index.data))).into_response(),
        None => (StatusCode::SERVICE_UNAVAILABLE, Html(NOT_BUILT)).into_response(),
    }
}
