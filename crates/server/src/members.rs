//! `/api/projects/:id/members`: who belongs to a project and with which
//! role. Changes take effect on live connections at once (see
//! [`crate::rooms::Rooms::set_role`]).
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;

use crate::AppState;
use crate::db::{self, Role};
use crate::error::{ApiError, ApiResult, JsonBody};
use crate::projects::Access;
use crate::util::{iso, normalize_email, now_ms};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{id}/members", get(members).post(add))
        .route("/{id}/members/{user_id}", put(update).delete(remove))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberJson {
    user_id: String,
    email: String,
    display_name: String,
    role: Role,
    added_at: String,
}

/// The shared start of this module's queries (a macro, so `concat!` keeps
/// the SQL a literal).
macro_rules! member_query {
    () => {
        "
    SELECT members.user_id, users.email, users.display_name, members.role, members.added_at
    FROM members JOIN users ON users.id = members.user_id
    WHERE members.project_id = ?"
    };
}

type MemberRow = (String, String, String, String, i64);

fn member((user_id, email, display_name, role, added): MemberRow) -> MemberJson {
    MemberJson {
        user_id,
        email,
        display_name,
        role: db::role(&role),
        added_at: iso(added),
    }
}

/// Members, owner first, then by when they joined.
pub async fn list(conn: &mut SqliteConnection, project_id: &str) -> sqlx::Result<Vec<MemberJson>> {
    let rows: Vec<MemberRow> = sqlx::query_as(concat!(member_query!(), " ORDER BY members.role = 'owner' DESC, members.added_at, users.display_name"))
    .bind(project_id)
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().map(member).collect())
}

async fn one(conn: &mut SqliteConnection, project_id: &str, user_id: &str) -> ApiResult<MemberJson> {
    let row: MemberRow = sqlx::query_as(concat!(member_query!(), " AND members.user_id = ?"))
        .bind(project_id)
        .bind(user_id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::not_found("Not a member of this project"))?;
    Ok(member(row))
}

async fn members(State(state): State<AppState>, access: Access) -> ApiResult<Json<Vec<MemberJson>>> {
    let mut conn = state.db.acquire().await?;
    Ok(Json(list(&mut conn, &access.project_id).await?))
}

#[derive(Deserialize)]
struct AddMemberRequest {
    email: String,
    role: Role,
}

async fn add(
    State(state): State<AppState>,
    access: Access,
    JsonBody(body): JsonBody<AddMemberRequest>,
) -> ApiResult<(StatusCode, Json<MemberJson>)> {
    access.require(Role::Owner)?;
    if body.role == Role::Owner {
        return Err(ApiError::unprocessable("Add members as editors or viewers, then transfer ownership"));
    }
    let email = normalize_email(&body.email).ok_or_else(|| ApiError::unprocessable("Enter a valid email address"))?;
    let mut tx = state.db.begin().await?;
    let user: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE email = ? AND disabled = 0")
        .bind(&email)
        .fetch_optional(&mut *tx)
        .await?;
    let Some((user_id,)) = user else {
        return Err(ApiError::not_found("No account uses that email address; send an invite link instead"));
    };
    let added = sqlx::query(
        "INSERT INTO members (project_id, user_id, role, added_at) VALUES (?, ?, ?, ?) ON CONFLICT DO NOTHING",
    )
    .bind(&access.project_id)
    .bind(&user_id)
    .bind(body.role.as_str())
    .bind(now_ms())
    .execute(&mut *tx)
    .await?;
    if added.rows_affected() == 0 {
        return Err(ApiError::conflict("Already a member of this project"));
    }
    let member = one(&mut tx, &access.project_id, &user_id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(member)))
}

#[derive(Deserialize)]
struct UpdateMemberRequest {
    role: Role,
}

/// Changes a member's role. Making someone the owner transfers ownership:
/// the previous owner becomes an editor.
async fn update(
    State(state): State<AppState>,
    access: Access,
    Path((_, user_id)): Path<(String, String)>,
    JsonBody(body): JsonBody<UpdateMemberRequest>,
) -> ApiResult<Json<MemberJson>> {
    access.require(Role::Owner)?;
    let mut tx = state.db.begin().await?;
    let current = one(&mut tx, &access.project_id, &user_id).await?;
    if current.role == body.role {
        return Ok(Json(current));
    }
    if user_id == access.user_id() {
        return Err(ApiError::conflict("Transfer ownership to another member first"));
    }
    let set_role = |user: String, role: Role| {
        sqlx::query("UPDATE members SET role = ? WHERE project_id = ? AND user_id = ?")
            .bind(role.as_str())
            .bind(access.project_id.clone())
            .bind(user)
    };
    set_role(user_id.clone(), body.role).execute(&mut *tx).await?;
    let transfer = body.role == Role::Owner;
    if transfer {
        set_role(access.user_id().to_owned(), Role::Editor).execute(&mut *tx).await?;
        sqlx::query("UPDATE projects SET owner_id = ? WHERE id = ?")
            .bind(&user_id)
            .bind(&access.project_id)
            .execute(&mut *tx)
            .await?;
    }
    let member = one(&mut tx, &access.project_id, &user_id).await?;
    tx.commit().await?;
    state.rooms.set_role(&access.project_id, &user_id, Some(body.role)).await;
    if transfer {
        state.rooms.set_role(&access.project_id, access.user_id(), Some(Role::Editor)).await;
    }
    Ok(Json(member))
}

/// Removes a member; members may remove themselves (leave), except the
/// owner, who must transfer ownership first.
async fn remove(
    State(state): State<AppState>,
    access: Access,
    Path((_, user_id)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    let leaving = user_id == access.user_id();
    if leaving && access.role == Role::Owner {
        return Err(ApiError::conflict("Transfer ownership to another member before leaving"));
    }
    if !leaving {
        access.require(Role::Owner)?;
    }
    let removed = sqlx::query("DELETE FROM members WHERE project_id = ? AND user_id = ?")
        .bind(&access.project_id)
        .bind(&user_id)
        .execute(&state.db)
        .await?;
    if removed.rows_affected() == 0 {
        return Err(ApiError::not_found("Not a member of this project"));
    }
    state.rooms.set_role(&access.project_id, &user_id, None).await;
    Ok(StatusCode::NO_CONTENT)
}
