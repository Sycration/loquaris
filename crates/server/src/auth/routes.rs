//! `/api/auth/*`: registration, sign-in and out, the current user, passwords
//! and sessions.
use axum::extract::{Path, State};
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use super::*;
use crate::config::Registration;
use crate::error::JsonBody;
use crate::ratelimit::ClientIp;
use crate::rooms::close;
use crate::util::normalize_email;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me).patch(update_me))
        .route("/password", post(change_password))
        .route("/reset", post(reset_password))
        .route("/sessions", get(sessions).delete(revoke_other_sessions))
        .route("/sessions/{id}", delete(revoke_session))
}

/// A display name, trimmed; 1–100 characters without control characters.
pub fn clean_display_name(name: &str) -> ApiResult<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 || name.chars().any(char::is_control) {
        return Err(ApiError::unprocessable("Enter a name of up to 100 characters"));
    }
    Ok(name.to_owned())
}

fn signed_in(cookie: HeaderValue, user: &UserRow) -> Response {
    ([(SET_COOKIE, cookie)], Json(UserJson::from(user))).into_response()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegisterRequest {
    email: String,
    display_name: String,
    password: String,
    invite: Option<String>,
}

async fn register(
    State(state): State<AppState>,
    ip: ClientIp,
    headers: HeaderMap,
    JsonBody(body): JsonBody<RegisterRequest>,
) -> ApiResult<Response> {
    let email = normalize_email(&body.email).ok_or_else(|| ApiError::unprocessable("Enter a valid email address"))?;
    state.limiter.check_both(&state, "register", ip, Some(&email))?;
    let display_name = clean_display_name(&body.display_name)?;
    check_password_policy(&body.password)?;
    let invite = body.invite.filter(|token| !token.trim().is_empty());
    match state.config.registration {
        Registration::Open => {}
        Registration::Invite if invite.is_some() => {}
        Registration::Invite => {
            return Err(ApiError::forbidden("Registration needs an invite link"));
        }
        Registration::Closed => {
            return Err(ApiError::forbidden("Registration is closed; ask an administrator for an account"));
        }
    }
    let password_hash = hash_password(&body.password).await?;

    let mut tx = state.db.begin().await?;
    let taken: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE email = ?")
        .bind(&email)
        .fetch_optional(&mut *tx)
        .await?;
    if taken.is_some() {
        return Err(ApiError::conflict("An account with this email already exists"));
    }
    // The first account administers the server.
    let (users,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users").fetch_one(&mut *tx).await?;
    let user = UserRow {
        id: new_id(),
        email,
        display_name,
        is_admin: users == 0,
        disabled: false,
        settings: "{}".into(),
    };
    sqlx::query(
        "INSERT INTO users (id, email, display_name, password_hash, is_admin, settings, created_at)
         VALUES (?, ?, ?, ?, ?, '{}', ?)",
    )
    .bind(&user.id)
    .bind(&user.email)
    .bind(&user.display_name)
    .bind(&password_hash)
    .bind(user.is_admin)
    .bind(now_ms())
    .execute(&mut *tx)
    .await?;
    if let Some(token) = invite {
        crate::invites::accept_in(&mut tx, &token, &user.id).await?;
    }
    let cookie = create_session(&state, &mut tx, &user.id, &headers).await?;
    tx.commit().await?;
    tracing::info!(user = %user.id, admin = user.is_admin, "account registered");
    Ok(signed_in(cookie, &user))
}

#[derive(Deserialize)]
struct LoginRequest {
    email: String,
    password: String,
}

async fn login(
    State(state): State<AppState>,
    ip: ClientIp,
    headers: HeaderMap,
    JsonBody(body): JsonBody<LoginRequest>,
) -> ApiResult<Response> {
    let email = normalize_email(&body.email).unwrap_or_default();
    state.limiter.check_both(&state, "login", ip, Some(&email))?;
    let row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT id, password_hash FROM users WHERE email = ?")
            .bind(&email)
            .fetch_optional(&state.db)
            .await?;
    let (user_id, hash) = row.map_or((None, None), |(id, hash)| (Some(id), hash));
    let valid = verify_password(&body.password, hash).await?;
    let Some(user_id) = user_id.filter(|_| valid) else {
        return Err(ApiError::Unauthorized("Incorrect email or password".into()));
    };
    let mut conn = state.db.acquire().await?;
    let user = user_by_id(&mut conn, &user_id).await?.ok_or_else(ApiError::unauthorized)?;
    if user.disabled {
        return Err(ApiError::forbidden("This account is disabled"));
    }
    let cookie = create_session(&state, &mut conn, &user.id, &headers).await?;
    Ok(signed_in(cookie, &user))
}

async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    if let Some(token) = session_token(&headers) {
        let id: Option<(String,)> = sqlx::query_as("DELETE FROM sessions WHERE token_hash = ? RETURNING id")
            .bind(hash_token(&token))
            .fetch_optional(&state.db)
            .await?;
        if let Some((id,)) = id {
            state.rooms.disconnect(|peer| peer.session_id == id, close::UNAUTHENTICATED, "Signed out").await;
        }
    }
    Ok((StatusCode::NO_CONTENT, [(SET_COOKIE, clear_cookie(&state))]).into_response())
}

async fn me(user: AuthUser) -> Json<UserJson> {
    Json(UserJson::from(user.user()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateMeRequest {
    display_name: Option<String>,
    settings: Option<serde_json::Value>,
}

async fn update_me(
    State(state): State<AppState>,
    user: AuthUser,
    JsonBody(body): JsonBody<UpdateMeRequest>,
) -> ApiResult<Json<UserJson>> {
    let mut updated = user.user().clone();
    if let Some(name) = body.display_name {
        updated.display_name = clean_display_name(&name)?;
    }
    if let Some(settings) = body.settings {
        if !settings.is_object() {
            return Err(ApiError::unprocessable("Settings must be an object"));
        }
        updated.settings = settings.to_string();
        if updated.settings.len() > 64 * 1024 {
            return Err(ApiError::TooLarge("Settings too large".into()));
        }
    }
    sqlx::query("UPDATE users SET display_name = ?, settings = ? WHERE id = ?")
        .bind(&updated.display_name)
        .bind(&updated.settings)
        .bind(&updated.id)
        .execute(&state.db)
        .await?;
    Ok(Json(UserJson::from(&updated)))
}

#[derive(Deserialize)]
struct ChangePasswordRequest {
    current: String,
    new: String,
}

async fn change_password(
    State(state): State<AppState>,
    ip: ClientIp,
    user: AuthUser,
    JsonBody(body): JsonBody<ChangePasswordRequest>,
) -> ApiResult<StatusCode> {
    state.limiter.check_both(&state, "password", ip, Some(user.id()))?;
    let (hash,): (Option<String>,) = sqlx::query_as("SELECT password_hash FROM users WHERE id = ?")
        .bind(user.id())
        .fetch_one(&state.db)
        .await?;
    if !verify_password(&body.current, hash).await? {
        return Err(ApiError::forbidden("The current password is not correct"));
    }
    check_password_policy(&body.new)?;
    let hash = hash_password(&body.new).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(hash)
        .bind(user.id())
        .execute(&mut *tx)
        .await?;
    // Other sessions may belong to whoever learned the old password.
    sqlx::query("DELETE FROM sessions WHERE user_id = ? AND id != ?")
        .bind(user.id())
        .bind(&user.0.session_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let (id, session) = (user.id().to_owned(), user.0.session_id.clone());
    state
        .rooms
        .disconnect(|peer| peer.user_id == id && peer.session_id != session, close::UNAUTHENTICATED, "Signed out")
        .await;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct ResetPasswordRequest {
    token: String,
    password: String,
}

async fn reset_password(
    State(state): State<AppState>,
    ip: ClientIp,
    headers: HeaderMap,
    JsonBody(body): JsonBody<ResetPasswordRequest>,
) -> ApiResult<Response> {
    state.limiter.check_both(&state, "reset", ip, None)?;
    check_password_policy(&body.password)?;
    let hash = hash_password(&body.password).await?;
    let now = now_ms();
    let mut tx = state.db.begin().await?;
    let used: Option<(String,)> = sqlx::query_as(
        "UPDATE password_resets SET used_at = ?
         WHERE token_hash = ? AND used_at IS NULL AND expires_at > ? RETURNING user_id",
    )
    .bind(now)
    .bind(hash_token(body.token.trim()))
    .bind(now)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((user_id,)) = used else {
        return Err(ApiError::not_found("This link is invalid, used or expired; ask an administrator for a new one"));
    };
    let user = user_by_id(&mut tx, &user_id).await?.ok_or_else(|| ApiError::not_found("No such account"))?;
    if user.disabled {
        return Err(ApiError::forbidden("This account is disabled"));
    }
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(hash)
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    let cookie = create_session(&state, &mut tx, &user.id, &headers).await?;
    tx.commit().await?;
    state.rooms.disconnect(|peer| peer.user_id == user_id, close::UNAUTHENTICATED, "Signed out").await;
    Ok(signed_in(cookie, &user))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionJson {
    id: String,
    created_at: String,
    last_seen_at: String,
    user_agent: Option<String>,
    current: bool,
}

async fn sessions(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<SessionJson>>> {
    let rows: Vec<(String, i64, i64, Option<String>)> = sqlx::query_as(
        "SELECT id, created_at, last_seen_at, user_agent FROM sessions
         WHERE user_id = ? AND expires_at > ? ORDER BY last_seen_at DESC",
    )
    .bind(user.id())
    .bind(now_ms())
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, created, seen, user_agent)| SessionJson {
                current: id == user.0.session_id,
                id,
                created_at: iso(created),
                last_seen_at: iso(seen),
                user_agent,
            })
            .collect(),
    ))
}

async fn revoke_session(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let deleted = sqlx::query("DELETE FROM sessions WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(user.id())
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(ApiError::not_found("No such session"));
    }
    state.rooms.disconnect(|peer| peer.session_id == id, close::UNAUTHENTICATED, "Signed out").await;
    if id == user.0.session_id {
        return Ok((StatusCode::NO_CONTENT, [(SET_COOKIE, clear_cookie(&state))]).into_response());
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn revoke_other_sessions(State(state): State<AppState>, user: AuthUser) -> ApiResult<StatusCode> {
    sqlx::query("DELETE FROM sessions WHERE user_id = ? AND id != ?")
        .bind(user.id())
        .bind(&user.0.session_id)
        .execute(&state.db)
        .await?;
    let (id, session) = (user.id().to_owned(), user.0.session_id.clone());
    state
        .rooms
        .disconnect(|peer| peer.user_id == id && peer.session_id != session, close::UNAUTHENTICATED, "Signed out")
        .await;
    Ok(StatusCode::NO_CONTENT)
}
