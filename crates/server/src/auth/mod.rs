//! Accounts and sessions: password hashing, the session cookie, the
//! middleware that resolves it, the CSRF check, and the extractors handlers
//! use to require a signed-in user or an admin.
mod routes;

use std::sync::LazyLock;

use anyhow::anyhow;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::extract::{FromRequestParts, Request, State};
use axum::http::header::{COOKIE, SET_COOKIE, USER_AGENT};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use sqlx::SqliteConnection;

use crate::AppState;
use crate::error::{ApiError, ApiResult};
use crate::util::{DAY_MS, hash_token, iso, new_id, new_token, now_ms};

pub use routes::{clean_display_name, router};

pub const COOKIE_NAME: &str = "loq_session";
const SESSION_DAYS: i64 = 30;
/// Sessions are extended at most this often, to avoid a write per request.
const REFRESH_AFTER_MS: i64 = 60 * 60 * 1000;
const RESET_LINK_DAYS: i64 = 7;

pub const MIN_PASSWORD: usize = 8;
const MAX_PASSWORD: usize = 1024;

/// A user row as most handlers need it.
#[derive(Clone, Debug, sqlx::FromRow)]
pub struct UserRow {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub is_admin: bool,
    pub disabled: bool,
    pub settings: String,
}

/// `GET /api/auth/me`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserJson {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub is_admin: bool,
    pub settings: serde_json::Value,
}

impl From<&UserRow> for UserJson {
    fn from(user: &UserRow) -> Self {
        Self {
            id: user.id.clone(),
            email: user.email.clone(),
            display_name: user.display_name.clone(),
            is_admin: user.is_admin,
            settings: serde_json::from_str(&user.settings).unwrap_or_else(|_| serde_json::json!({})),
        }
    }
}

/// The signed-in user and session, put in request extensions by
/// [`session_layer`].
#[derive(Clone, Debug)]
pub struct CurrentSession {
    pub session_id: String,
    pub user: UserRow,
}

/// Extracts the signed-in user, or answers 401.
pub struct AuthUser(pub CurrentSession);

impl<S: Send + Sync> FromRequestParts<S> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> ApiResult<Self> {
        parts
            .extensions
            .get::<CurrentSession>()
            .cloned()
            .map(AuthUser)
            .ok_or_else(ApiError::unauthorized)
    }
}

impl AuthUser {
    pub fn id(&self) -> &str {
        &self.0.user.id
    }

    pub fn user(&self) -> &UserRow {
        &self.0.user
    }
}

/// Extracts a signed-in admin, or answers 401/403.
pub struct AdminUser(pub CurrentSession);

impl<S: Send + Sync> FromRequestParts<S> for AdminUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> ApiResult<Self> {
        let AuthUser(session) = AuthUser::from_request_parts(parts, state).await?;
        if !session.user.is_admin {
            return Err(ApiError::forbidden("Admins only"));
        }
        Ok(Self(session))
    }
}

/// Resolves the session cookie into a [`CurrentSession`] extension and
/// slides the session's expiry forward (re-sending the cookie) once an hour.
pub async fn session_layer(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    let mut refreshed = None;
    if let Some(token) = session_token(request.headers()) {
        match lookup_session(&state, &token).await {
            Ok(Some((session, refresh))) => {
                request.extensions_mut().insert(session);
                if refresh {
                    refreshed = Some(token);
                }
            }
            Ok(None) => {}
            Err(error) => return ApiError::from(error).into_response(),
        }
    }
    let mut response = next.run(request).await;
    if let Some(token) = refreshed
        && !response.headers().contains_key(SET_COOKIE)
    {
        response
            .headers_mut()
            .append(SET_COOKIE, session_cookie(&state, &token));
    }
    response
}

/// Rejects state-changing requests without `X-Loquaris: 1`, a header a
/// cross-site form or image cannot send.
pub async fn csrf_layer(request: Request, next: Next) -> Response {
    let safe = matches!(*request.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    let marked = request.headers().get("x-loquaris").is_some_and(|value| value == "1");
    if safe || marked {
        next.run(request).await
    } else {
        ApiError::forbidden("Missing X-Loquaris header").into_response()
    }
}

pub fn session_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE_NAME)
        .map(|(_, value)| value.to_owned())
        .filter(|value| !value.is_empty())
}

async fn lookup_session(state: &AppState, token: &str) -> sqlx::Result<Option<(CurrentSession, bool)>> {
    let now = now_ms();
    #[derive(sqlx::FromRow)]
    struct Found {
        session_id: String,
        last_seen_at: i64,
        #[sqlx(flatten)]
        user: UserRow,
    }
    let found: Option<Found> = sqlx::query_as(
        "SELECT sessions.id AS session_id, sessions.last_seen_at,
                users.id, users.email, users.display_name, users.is_admin, users.disabled, users.settings
         FROM sessions JOIN users ON users.id = sessions.user_id
         WHERE sessions.token_hash = ? AND sessions.expires_at > ? AND users.disabled = 0",
    )
    .bind(hash_token(token))
    .bind(now)
    .fetch_optional(&state.db)
    .await?;
    let Some(Found {
        session_id,
        last_seen_at: last_seen,
        user,
    }) = found
    else {
        return Ok(None);
    };
    let refresh = now - last_seen > REFRESH_AFTER_MS;
    if refresh {
        sqlx::query("UPDATE sessions SET last_seen_at = ?, expires_at = ? WHERE id = ?")
            .bind(now)
            .bind(now + SESSION_DAYS * DAY_MS)
            .bind(&session_id)
            .execute(&state.db)
            .await?;
    }
    Ok(Some((CurrentSession { session_id, user }, refresh)))
}

/// Starts a session, returning its `Set-Cookie` header.
pub async fn create_session(
    state: &AppState,
    conn: &mut SqliteConnection,
    user_id: &str,
    headers: &HeaderMap,
) -> sqlx::Result<HeaderValue> {
    let token = new_token();
    let now = now_ms();
    let user_agent = headers
        .get(USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(|agent| agent.chars().take(300).collect::<String>());
    sqlx::query(
        "INSERT INTO sessions (id, token_hash, user_id, created_at, last_seen_at, expires_at, user_agent)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(new_id())
    .bind(hash_token(&token))
    .bind(user_id)
    .bind(now)
    .bind(now)
    .bind(now + SESSION_DAYS * DAY_MS)
    .bind(user_agent)
    .execute(conn)
    .await?;
    Ok(session_cookie(state, &token))
}

pub fn session_cookie(state: &AppState, token: &str) -> HeaderValue {
    let secure = if state.config.secure_cookies() { "; Secure" } else { "" };
    let max_age = SESSION_DAYS * 24 * 60 * 60;
    HeaderValue::from_str(&format!(
        "{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}{secure}"
    ))
    .expect("cookie is ASCII")
}

pub fn clear_cookie(state: &AppState) -> HeaderValue {
    let secure = if state.config.secure_cookies() { "; Secure" } else { "" };
    HeaderValue::from_str(&format!("{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}"))
        .expect("cookie is ASCII")
}

pub fn check_password_policy(password: &str) -> ApiResult<()> {
    let length = password.chars().count();
    if length < MIN_PASSWORD {
        return Err(ApiError::unprocessable(format!(
            "Passwords need at least {MIN_PASSWORD} characters"
        )));
    }
    if password.len() > MAX_PASSWORD {
        return Err(ApiError::unprocessable("Password too long"));
    }
    Ok(())
}

/// An argon2id PHC string. Runs on the blocking pool: hashing takes tens of
/// milliseconds by design.
pub async fn hash_password(password: &str) -> ApiResult<String> {
    let password = password.to_owned();
    tokio::task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|hash| hash.to_string())
            .map_err(|error| ApiError::Internal(anyhow!("password hashing failed: {error}")))
    })
    .await?
}

/// Checks a password against a stored hash. Without a hash (unknown account,
/// or no password set yet) a dummy hash is checked, so the response time
/// does not reveal which accounts exist.
pub async fn verify_password(password: &str, hash: Option<String>) -> ApiResult<bool> {
    static DUMMY: LazyLock<String> = LazyLock::new(|| {
        Argon2::default()
            .hash_password(b"not a real password")
            .expect("hashing a constant works")
            .to_string()
    });
    let password = password.to_owned();
    tokio::task::spawn_blocking(move || {
        let real = hash.is_some();
        let hash = hash.unwrap_or_else(|| DUMMY.clone());
        let ok = PasswordHash::new(&hash)
            .is_ok_and(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok());
        Ok(real && ok)
    })
    .await?
}

/// `POST /api/admin/users` and `/reset-link`: a one-time link to set a password.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetLink {
    pub url: String,
    pub expires_at: String,
}

/// Creates a 7-day one-time link with which `user_id` can set a password.
pub async fn create_reset_link(
    conn: &mut SqliteConnection,
    public_url: &str,
    user_id: &str,
) -> sqlx::Result<ResetLink> {
    let token = new_token();
    let now = now_ms();
    let expires = now + RESET_LINK_DAYS * DAY_MS;
    sqlx::query("INSERT INTO password_resets (token_hash, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)")
        .bind(hash_token(&token))
        .bind(user_id)
        .bind(now)
        .bind(expires)
        .execute(conn)
        .await?;
    Ok(ResetLink {
        url: format!("{public_url}/reset/{token}"),
        expires_at: iso(expires),
    })
}

pub async fn user_by_id(conn: &mut SqliteConnection, user_id: &str) -> sqlx::Result<Option<UserRow>> {
    sqlx::query_as("SELECT id, email, display_name, is_admin, disabled, settings FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(conn)
        .await
}
