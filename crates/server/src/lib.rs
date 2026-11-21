//! loquaris-server: the HTTP API, accounts, collaboration rooms, persistence
//! and blob store of the Loquaris web app, and the built browser app itself.
//! docs/DESIGN.md ("Server") is the specification.
pub mod admin;
pub mod audio;
pub mod auth;
pub mod blobs;
pub mod config;
pub mod db;
pub mod doc;
pub mod error;
pub mod export;
pub mod invites;
pub mod members;
pub mod projects;
pub mod ratelimit;
pub mod rooms;
pub mod static_files;
pub mod storage;
pub mod sync;
pub mod util;
pub mod versions;

use std::future::Future;
use std::net::SocketAddr;
use std::ops::Deref;
use std::sync::Arc;

use anyhow::Context;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::HeaderValue;
use axum::http::header::{REFERRER_POLICY, X_CONTENT_TYPE_OPTIONS};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use sqlx::SqlitePool;
use tokio::net::TcpListener;
use tower_http::CompressionLevel;
use tower_http::compression::CompressionLayer;
use tower_http::trace::TraceLayer;

use crate::blobs::{BlobStore, FsBlobStore};
use crate::config::{Config, Registration};
use crate::error::ApiError;
use crate::ratelimit::Limiter;
use crate::rooms::Rooms;

/// Shared by every handler.
pub struct Services {
    pub config: Arc<Config>,
    pub db: SqlitePool,
    pub blobs: Arc<dyn BlobStore>,
    pub rooms: Arc<Rooms>,
    pub limiter: Limiter,
}

#[derive(Clone)]
pub struct AppState(Arc<Services>);

impl Deref for AppState {
    type Target = Services;

    fn deref(&self) -> &Services {
        &self.0
    }
}

impl AppState {
    /// Opens (creating if needed) the data directory, database and blob store.
    pub async fn open(config: Config) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&config.data_dir)
            .with_context(|| format!("cannot create {}", config.data_dir.display()))?;
        check_writable(&config.data_dir)?;
        let db = db::open(&config.database_path()).await?;
        let blobs = FsBlobStore::new(config.blob_dir()).context("cannot create the blob directory")?;
        let config = Arc::new(config);
        Ok(Self(Arc::new(Services {
            rooms: Rooms::new(db.clone(), config.clone()),
            config,
            db,
            blobs: Arc::new(blobs),
            limiter: Limiter::default(),
        })))
    }
}

/// Fails with an explanation when the data directory can't be written,
/// which SQLite would otherwise report only as "unable to open database
/// file". Usually the directory belongs to another user, as with volumes
/// created by a NAS.
fn check_writable(dir: &std::path::Path) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;

    let probe = dir.join(".loquaris-write-test");
    let Err(error) = std::fs::write(&probe, b"").and_then(|()| std::fs::remove_file(&probe)) else {
        return Ok(());
    };
    // /proc/self belongs to the process's own user.
    let me = std::fs::metadata("/proc/self").map(|m| format!("{}:{}", m.uid(), m.gid()));
    let owner = std::fs::metadata(dir).map(|m| format!("{}:{}", m.uid(), m.gid()));
    anyhow::bail!(
        "cannot write to the data directory {} ({error}). The server runs as user {} and the \
         directory belongs to {}. Give the server's user write access to it, or run the server \
         as the directory's owner (in Docker, set LOQ_UID and LOQ_GID).",
        dir.display(),
        me.as_deref().unwrap_or("?"),
        owner.as_deref().unwrap_or("?"),
    )
}

/// The whole application: `/api` and the web app.
pub fn router(state: AppState) -> Router {
    let projects = projects::router()
        .merge(members::router())
        .merge(invites::project_router())
        .merge(versions::router())
        .merge(audio::upload_router())
        .merge(export::router())
        .merge(sync::router());
    let api = Router::new()
        .route("/config", get(server_config))
        .nest("/auth", auth::router())
        .nest("/projects", projects)
        .nest("/invites", invites::router())
        .nest("/audio", audio::router())
        .nest("/admin", admin::router())
        .fallback(|| async { ApiError::not_found("No such API endpoint") })
        .layer(DefaultBodyLimit::max(1 << 20))
        .layer(middleware::from_fn(auth::csrf_layer))
        .layer(middleware::from_fn_with_state(state.clone(), auth::session_layer));
    // The app's files are compressed (the analyzer's wasm shrinks from about
    // 14 MB to 3–4 MB). API responses are not: audio is served in byte
    // ranges, which compression would break.
    let web = Router::new()
        .fallback(static_files::serve)
        .layer(CompressionLayer::new().quality(CompressionLevel::Fastest));
    Router::new()
        .nest("/api", api)
        .merge(web)
        .layer(middleware::from_fn(security_headers))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    // Invite and reset links carry tokens in the path; keep them out of
    // Referer headers sent elsewhere.
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("same-origin"));
    response
}

#[derive(Serialize)]
struct ServerConfig {
    registration: Registration,
    version: &'static str,
}

async fn server_config(State(state): State<AppState>) -> Json<ServerConfig> {
    Json(ServerConfig {
        registration: state.config.registration,
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// Serves until `shutdown` completes, then closes every collaboration
/// connection, flushes the rooms and closes the database.
pub async fn serve(
    listener: TcpListener,
    state: AppState,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let app = router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
    let rooms = state.rooms.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown.await;
            // WebSockets would otherwise keep the server waiting.
            rooms.shutdown().await;
        })
        .await?;
    state.rooms.shutdown().await;
    state.db.close().await;
    Ok(())
}
