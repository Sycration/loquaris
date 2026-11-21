//! Version history: named versions on request, automatic ones at most hourly
//! while a project is edited (taken by its room), and restoring, which writes
//! the old content into the live document as ordinary changes.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use yrs::Transact;

use crate::db::Role;
use crate::error::{ApiError, ApiResult, JsonBody};
use crate::projects::{Access, UserRef};
use crate::util::{iso, new_id, now_ms};
use crate::{AppState, doc};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{id}/versions", get(list).post(create))
        .route("/{id}/versions/{version_id}/restore", post(restore))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionJson {
    id: String,
    label: Option<String>,
    auto: bool,
    created_at: String,
    created_by: Option<UserRef>,
}

/// Stores a version of a project's document state and returns its ID.
pub async fn insert(
    pool: &SqlitePool,
    project_id: &str,
    label: Option<&str>,
    auto: bool,
    data: &[u8],
    created_by: Option<&str>,
) -> sqlx::Result<String> {
    let id = new_id();
    sqlx::query(
        "INSERT INTO versions (id, project_id, label, auto, data, created_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(project_id)
    .bind(label)
    .bind(auto)
    .bind(data)
    .bind(created_by)
    .bind(now_ms())
    .execute(pool)
    .await?;
    Ok(id)
}

type VersionRow = (String, Option<String>, bool, i64, Option<String>, Option<String>);

/// The shared start of this module's queries (a macro, so `concat!` keeps
/// the SQL a literal).
macro_rules! version_query {
    () => {
        "
    SELECT versions.id, versions.label, versions.auto, versions.created_at, users.id, users.display_name
    FROM versions LEFT JOIN users ON users.id = versions.created_by
    WHERE versions.project_id = ?"
    };
}

fn version((id, label, auto, created, user_id, user_name): VersionRow) -> VersionJson {
    VersionJson {
        id,
        label,
        auto,
        created_at: iso(created),
        created_by: user_id.zip(user_name).map(|(id, display_name)| UserRef { id, display_name }),
    }
}

/// Newest first.
async fn list(State(state): State<AppState>, access: Access) -> ApiResult<Json<Vec<VersionJson>>> {
    let rows: Vec<VersionRow> =
        sqlx::query_as(concat!(version_query!(), " ORDER BY versions.created_at DESC, versions.rowid DESC"))
            .bind(&access.project_id)
            .fetch_all(&state.db)
            .await?;
    Ok(Json(rows.into_iter().map(version).collect()))
}

#[derive(Deserialize)]
struct CreateVersionRequest {
    label: String,
}

async fn create(
    State(state): State<AppState>,
    access: Access,
    JsonBody(body): JsonBody<CreateVersionRequest>,
) -> ApiResult<(StatusCode, Json<VersionJson>)> {
    access.require(Role::Editor)?;
    let label = body.label.trim();
    if label.is_empty() || label.chars().count() > 200 {
        return Err(ApiError::unprocessable("Enter a version name of up to 200 characters"));
    }
    let data = state.rooms.read(&access.project_id, doc::encode_state).await?;
    let id = insert(&state.db, &access.project_id, Some(label), false, &data, Some(access.user_id())).await?;
    let row: VersionRow = sqlx::query_as(concat!(version_query!(), " AND versions.id = ?"))
        .bind(&access.project_id)
        .bind(&id)
        .fetch_one(&state.db)
        .await?;
    Ok((StatusCode::CREATED, Json(version(row))))
}

/// Makes the live document match the version. The current state is saved
/// first as an automatic version labelled "Before restore", so a restore
/// can itself be undone from the history.
async fn restore(
    State(state): State<AppState>,
    access: Access,
    Path((_, version_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    access.require(Role::Editor)?;
    let data: Option<(Vec<u8>,)> = sqlx::query_as("SELECT data FROM versions WHERE id = ? AND project_id = ?")
        .bind(&version_id)
        .bind(&access.project_id)
        .fetch_optional(&state.db)
        .await?;
    let Some((data,)) = data else {
        return Err(ApiError::not_found("No such version"));
    };
    let target = doc::read(&doc::decode([data.as_slice()])?.transact());
    let current = state.rooms.read(&access.project_id, doc::encode_state).await?;
    insert(&state.db, &access.project_id, Some("Before restore"), true, &current, Some(access.user_id())).await?;
    state
        .rooms
        .change(&access.project_id, Some(access.user_id()), |txn| doc::write(txn, &target))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
