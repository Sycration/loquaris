//! `/api/admin/users`: accounts for administrators. New accounts and
//! password resets are one-time links the admin passes on (no email in the
//! first release).
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;

use crate::AppState;
use crate::auth::{AdminUser, ResetLink, clean_display_name, create_reset_link};
use crate::error::{ApiError, ApiResult, JsonBody};
use crate::rooms::close;
use crate::util::{iso, new_id, normalize_email, now_ms};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users", get(users).post(create_user))
        .route("/users/{id}", patch(update_user))
        .route("/users/{id}/reset-link", post(reset_link))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AdminUserJson {
    id: String,
    email: String,
    display_name: String,
    is_admin: bool,
    disabled: bool,
    created_at: String,
    project_count: i64,
}

type AdminUserRow = (String, String, String, bool, bool, i64, i64);

/// The shared start of this module's queries (a macro, so `concat!` keeps
/// the SQL a literal).
macro_rules! user_query {
    () => {
        "
    SELECT users.id, users.email, users.display_name, users.is_admin, users.disabled, users.created_at,
           (SELECT COUNT(*) FROM members JOIN projects ON projects.id = members.project_id
            WHERE members.user_id = users.id AND projects.deleted_at IS NULL)
    FROM users"
    };
}

fn admin_user((id, email, display_name, is_admin, disabled, created, project_count): AdminUserRow) -> AdminUserJson {
    AdminUserJson {
        id,
        email,
        display_name,
        is_admin,
        disabled,
        created_at: iso(created),
        project_count,
    }
}

async fn one(conn: &mut SqliteConnection, id: &str) -> ApiResult<AdminUserJson> {
    let row: AdminUserRow = sqlx::query_as(concat!(user_query!(), " WHERE users.id = ?"))
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::not_found("No such user"))?;
    Ok(admin_user(row))
}

async fn users(State(state): State<AppState>, _: AdminUser) -> ApiResult<Json<Vec<AdminUserJson>>> {
    let rows: Vec<AdminUserRow> = sqlx::query_as(concat!(user_query!(), " ORDER BY users.created_at"))
        .fetch_all(&state.db)
        .await?;
    Ok(Json(rows.into_iter().map(admin_user).collect()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateUserRequest {
    email: String,
    display_name: String,
    #[serde(default)]
    is_admin: bool,
}

/// Creates an account without a password and returns the link with which
/// its owner sets one.
async fn create_user(
    State(state): State<AppState>,
    _: AdminUser,
    JsonBody(body): JsonBody<CreateUserRequest>,
) -> ApiResult<(StatusCode, Json<ResetLink>)> {
    let email = normalize_email(&body.email).ok_or_else(|| ApiError::unprocessable("Enter a valid email address"))?;
    let display_name = clean_display_name(&body.display_name)?;
    let mut tx = state.db.begin().await?;
    let link = create_account(&mut tx, &state.config.public_url, &email, &display_name, body.is_admin).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(link)))
}

/// Inserts a password-less account and returns its set-password link (also
/// used by the `create-admin` command).
pub async fn create_account(
    conn: &mut SqliteConnection,
    public_url: &str,
    email: &str,
    display_name: &str,
    is_admin: bool,
) -> ApiResult<ResetLink> {
    let id = new_id();
    let inserted = sqlx::query(
        "INSERT INTO users (id, email, display_name, is_admin, created_at) VALUES (?, ?, ?, ?, ?)
         ON CONFLICT (email) DO NOTHING",
    )
    .bind(&id)
    .bind(email)
    .bind(display_name)
    .bind(is_admin)
    .bind(now_ms())
    .execute(&mut *conn)
    .await?;
    if inserted.rows_affected() == 0 {
        return Err(ApiError::conflict("An account with this email already exists"));
    }
    Ok(create_reset_link(conn, public_url, &id).await?)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateUserRequest {
    display_name: Option<String>,
    is_admin: Option<bool>,
    disabled: Option<bool>,
}

async fn update_user(
    State(state): State<AppState>,
    _: AdminUser,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<UpdateUserRequest>,
) -> ApiResult<Json<AdminUserJson>> {
    let mut tx = state.db.begin().await?;
    let current = one(&mut tx, &id).await?;
    let display_name = match body.display_name {
        Some(name) => clean_display_name(&name)?,
        None => current.display_name.clone(),
    };
    let is_admin = body.is_admin.unwrap_or(current.is_admin);
    let disabled = body.disabled.unwrap_or(current.disabled);
    // Someone must remain able to administer the server.
    let was_active_admin = current.is_admin && !current.disabled;
    if was_active_admin && !(is_admin && !disabled) {
        let (others,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM users WHERE is_admin = 1 AND disabled = 0 AND id != ?")
                .bind(&id)
                .fetch_one(&mut *tx)
                .await?;
        if others == 0 {
            return Err(ApiError::conflict("This is the last administrator"));
        }
    }
    sqlx::query("UPDATE users SET display_name = ?, is_admin = ?, disabled = ? WHERE id = ?")
        .bind(&display_name)
        .bind(is_admin)
        .bind(disabled)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    if disabled && !current.disabled {
        sqlx::query("DELETE FROM sessions WHERE user_id = ?").bind(&id).execute(&mut *tx).await?;
    }
    let updated = one(&mut tx, &id).await?;
    tx.commit().await?;
    if disabled && !current.disabled {
        state.rooms.disconnect(|peer| peer.user_id == id, close::UNAUTHENTICATED, "Account disabled").await;
    }
    Ok(Json(updated))
}

async fn reset_link(State(state): State<AppState>, _: AdminUser, Path(id): Path<String>) -> ApiResult<Json<ResetLink>> {
    let mut conn = state.db.acquire().await?;
    one(&mut conn, &id).await?;
    Ok(Json(create_reset_link(&mut conn, &state.config.public_url, &id).await?))
}
