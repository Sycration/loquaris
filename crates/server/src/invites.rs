//! Invite links: an owner creates a link with a role, optional expiry and
//! use limit; anyone signed in who opens it joins the project. The token is
//! shown once and stored hashed.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;

use crate::AppState;
use crate::auth::AuthUser;
use crate::db::{self, Role};
use crate::error::{ApiError, ApiResult, JsonBody};
use crate::projects::{Access, UserRef};
use crate::util::{DAY_MS, hash_token, iso, new_id, new_token, now_ms};

/// Routes under `/api/projects`.
pub fn project_router() -> Router<AppState> {
    Router::new()
        .route("/{id}/invites", post(create))
        .route("/{id}/invites/{invite_id}", delete(revoke))
}

/// Routes under `/api/invites`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{token}", get(preview))
        .route("/{token}/accept", post(accept))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteJson {
    id: String,
    role: Role,
    expires_at: Option<String>,
    max_uses: Option<i64>,
    uses: i64,
    created_at: String,
    created_by: UserRef,
}

type InviteRow = (String, String, Option<i64>, Option<i64>, i64, i64, String, String);

/// The shared start of this module's queries (a macro, so `concat!` keeps
/// the SQL a literal).
macro_rules! invite_query {
    () => {
        "
    SELECT invites.id, invites.role, invites.expires_at, invites.max_uses, invites.uses,
           invites.created_at, invites.created_by, users.display_name
    FROM invites JOIN users ON users.id = invites.created_by
    WHERE invites.project_id = ? AND invites.revoked_at IS NULL"
    };
}

fn invite((id, role, expires, max_uses, uses, created, created_by, creator): InviteRow) -> InviteJson {
    InviteJson {
        id,
        role: db::role(&role),
        expires_at: expires.map(iso),
        max_uses,
        uses,
        created_at: iso(created),
        created_by: UserRef {
            id: created_by,
            display_name: creator,
        },
    }
}

/// The project's links that were not revoked, newest first.
pub async fn list(conn: &mut SqliteConnection, project_id: &str) -> sqlx::Result<Vec<InviteJson>> {
    let rows: Vec<InviteRow> = sqlx::query_as(concat!(invite_query!(), " ORDER BY invites.created_at DESC"))
        .bind(project_id)
        .fetch_all(conn)
        .await?;
    Ok(rows.into_iter().map(invite).collect())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateInviteRequest {
    role: Role,
    expires_in_days: Option<f64>,
    max_uses: Option<i64>,
}

#[derive(Serialize)]
struct CreateInviteResponse {
    invite: InviteJson,
    url: String,
}

async fn create(
    State(state): State<AppState>,
    access: Access,
    JsonBody(body): JsonBody<CreateInviteRequest>,
) -> ApiResult<(StatusCode, Json<CreateInviteResponse>)> {
    access.require(Role::Owner)?;
    if body.role == Role::Owner {
        return Err(ApiError::unprocessable("Invites are for editors or viewers"));
    }
    let now = now_ms();
    let expires_at = match body.expires_in_days {
        Some(days) if days.is_finite() && days > 0.0 => Some(now + (days.min(3650.0) * DAY_MS as f64) as i64),
        Some(_) => return Err(ApiError::unprocessable("The expiry must be a positive number of days")),
        None => None,
    };
    if body.max_uses.is_some_and(|uses| uses < 1) {
        return Err(ApiError::unprocessable("The use limit must be at least 1"));
    }
    let token = new_token();
    let id = new_id();
    sqlx::query(
        "INSERT INTO invites (id, token_hash, project_id, role, created_by, created_at, expires_at, max_uses)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(hash_token(&token))
    .bind(&access.project_id)
    .bind(body.role.as_str())
    .bind(access.user_id())
    .bind(now)
    .bind(expires_at)
    .bind(body.max_uses)
    .execute(&state.db)
    .await?;
    let invite = InviteJson {
        id,
        role: body.role,
        expires_at: expires_at.map(iso),
        max_uses: body.max_uses,
        uses: 0,
        created_at: iso(now),
        created_by: UserRef {
            id: access.user_id().to_owned(),
            display_name: access.session.user.display_name.clone(),
        },
    };
    let url = format!("{}/invite/{token}", state.config.public_url);
    Ok((StatusCode::CREATED, Json(CreateInviteResponse { invite, url })))
}

async fn revoke(
    State(state): State<AppState>,
    access: Access,
    Path((_, invite_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    access.require(Role::Owner)?;
    let revoked = sqlx::query(
        "UPDATE invites SET revoked_at = ? WHERE id = ? AND project_id = ? AND revoked_at IS NULL",
    )
    .bind(now_ms())
    .bind(&invite_id)
    .bind(&access.project_id)
    .execute(&state.db)
    .await?;
    if revoked.rows_affected() == 0 {
        return Err(ApiError::not_found("No such invite"));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InvitePreview {
    project_name: String,
    role: Role,
    invited_by: UserRef,
    valid: bool,
}

/// Public, so the invite page can say what joining means before sign-in.
async fn preview(State(state): State<AppState>, Path(token): Path<String>) -> ApiResult<Json<InvitePreview>> {
    type PreviewRow = (String, String, String, String, Option<i64>, Option<i64>, i64, Option<i64>);
    let row: Option<PreviewRow> = sqlx::query_as(
        "SELECT projects.name, invites.role, invites.created_by, users.display_name,
                invites.expires_at, invites.max_uses, invites.uses, invites.revoked_at
         FROM invites
         JOIN projects ON projects.id = invites.project_id AND projects.deleted_at IS NULL
         JOIN users ON users.id = invites.created_by
         WHERE invites.token_hash = ?",
    )
    .bind(hash_token(&token))
    .fetch_optional(&state.db)
    .await?;
    let Some((project_name, role, created_by, creator, expires, max_uses, uses, revoked)) = row else {
        return Err(ApiError::not_found("This invite link does not exist"));
    };
    let valid =
        revoked.is_none() && expires.is_none_or(|at| at > now_ms()) && max_uses.is_none_or(|max| uses < max);
    Ok(Json(InvitePreview {
        project_name,
        role: db::role(&role),
        invited_by: UserRef {
            id: created_by,
            display_name: creator,
        },
        valid,
    }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AcceptInviteResponse {
    project_id: String,
}

async fn accept(
    State(state): State<AppState>,
    user: AuthUser,
    Path(token): Path<String>,
) -> ApiResult<Json<AcceptInviteResponse>> {
    let mut tx = state.db.begin().await?;
    let project_id = accept_in(&mut tx, &token, user.id()).await?;
    tx.commit().await?;
    Ok(Json(AcceptInviteResponse { project_id }))
}

/// Joins `user_id` to the invite's project, using up one use, and returns
/// the project. Existing members keep their role and use nothing.
pub async fn accept_in(conn: &mut SqliteConnection, token: &str, user_id: &str) -> ApiResult<String> {
    let token_hash = hash_token(token.trim());
    let now = now_ms();
    let found: Option<(String, String)> = sqlx::query_as(
        "SELECT invites.project_id, invites.role FROM invites
         JOIN projects ON projects.id = invites.project_id AND projects.deleted_at IS NULL
         WHERE invites.token_hash = ?",
    )
    .bind(&token_hash)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((project_id, role)) = found else {
        return Err(ApiError::not_found("This invite link does not exist"));
    };
    let member: Option<(String,)> = sqlx::query_as("SELECT role FROM members WHERE project_id = ? AND user_id = ?")
        .bind(&project_id)
        .bind(user_id)
        .fetch_optional(&mut *conn)
        .await?;
    if member.is_some() {
        return Ok(project_id);
    }
    // Check and count the use in one statement, so concurrent acceptances
    // cannot exceed the limit.
    let used = sqlx::query(
        "UPDATE invites SET uses = uses + 1
         WHERE token_hash = ? AND revoked_at IS NULL
           AND (expires_at IS NULL OR expires_at > ?) AND (max_uses IS NULL OR uses < max_uses)",
    )
    .bind(&token_hash)
    .bind(now)
    .execute(&mut *conn)
    .await?;
    if used.rows_affected() == 0 {
        return Err(ApiError::Gone("This invite link has expired, been used up or been revoked".into()));
    }
    sqlx::query("INSERT INTO members (project_id, user_id, role, added_at) VALUES (?, ?, ?, ?)")
        .bind(&project_id)
        .bind(user_id)
        .bind(&role)
        .bind(now)
        .execute(&mut *conn)
        .await?;
    Ok(project_id)
}
