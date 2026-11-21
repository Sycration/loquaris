//! `/api/projects`: listing, creating, importing, renaming, duplicating and
//! deleting projects, and [`Access`], the extractor every project route uses
//! to check membership and role.
use axum::extract::{DefaultBodyLimit, FromRequestParts, RawPathParams, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum::body::Bytes;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use yrs::Transact;

use crate::auth::{AuthUser, CurrentSession};
use crate::blobs::{self, BlobInfo};
use crate::db::{self, Role};
use crate::error::{ApiError, ApiResult, JsonBody};
use crate::invites::{self, InviteJson};
use crate::members::{self, MemberJson};
use crate::util::{iso, new_id, now_ms, sha256_hex};
use crate::{AppState, doc, storage};

/// Largest `.loqr` archive accepted for import.
const MAX_IMPORT: usize = 1 << 30;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/import", post(import).layer(DefaultBodyLimit::max(MAX_IMPORT)))
        .route("/{id}", get(detail).patch(update).delete(remove))
        .route("/{id}/duplicate", post(duplicate))
}

/// The signed-in user's membership in the project named by the `{id}` path
/// parameter. Non-members get 404, so project IDs are not confirmed to
/// outsiders.
pub struct Access {
    pub project_id: String,
    pub session: CurrentSession,
    pub role: Role,
}

impl FromRequestParts<AppState> for Access {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> ApiResult<Self> {
        let AuthUser(session) = AuthUser::from_request_parts(parts, state).await?;
        let params = RawPathParams::from_request_parts(parts, state)
            .await
            .map_err(|_| ApiError::not_found("Project not found"))?;
        let project_id = params
            .iter()
            .find(|(name, _)| *name == "id")
            .map(|(_, value)| value.to_owned())
            .ok_or_else(|| ApiError::not_found("Project not found"))?;
        let role = member_role(&state.db, &project_id, &session.user.id)
            .await?
            .ok_or_else(|| ApiError::not_found("Project not found"))?;
        Ok(Self {
            project_id,
            session,
            role,
        })
    }
}

impl Access {
    pub fn user_id(&self) -> &str {
        &self.session.user.id
    }

    pub fn require(&self, role: Role) -> ApiResult<()> {
        if self.role >= role {
            return Ok(());
        }
        Err(ApiError::forbidden(match role {
            Role::Owner => "Only the project's owner can do this",
            _ => "Viewers cannot change this project",
        }))
    }
}

/// The user's role in a live (not deleted) project.
pub async fn member_role(pool: &sqlx::SqlitePool, project_id: &str, user_id: &str) -> sqlx::Result<Option<Role>> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT members.role FROM members JOIN projects ON projects.id = members.project_id
         WHERE members.project_id = ? AND members.user_id = ? AND projects.deleted_at IS NULL",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(role,)| db::role(&role)))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserRef {
    pub id: String,
    pub display_name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    id: String,
    name: String,
    role: Role,
    owner: UserRef,
    member_count: i64,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDetail {
    id: String,
    name: String,
    role: Role,
    owner: UserRef,
    members: Vec<MemberJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    invites: Option<Vec<InviteJson>>,
    created_at: String,
    updated_at: String,
}

type SummaryRow = (String, String, String, String, String, i64, i64, i64);

/// The shared start of this module's queries (a macro, so `concat!` keeps
/// the SQL a literal).
macro_rules! summary_query {
    () => {
        "
    SELECT projects.id, projects.name, members.role, projects.owner_id, owner.display_name,
           (SELECT COUNT(*) FROM members AS m WHERE m.project_id = projects.id),
           projects.created_at, projects.updated_at
    FROM projects
    JOIN members ON members.project_id = projects.id AND members.user_id = ?
    JOIN users AS owner ON owner.id = projects.owner_id
    WHERE projects.deleted_at IS NULL"
    };
}

fn summary((id, name, role, owner_id, owner_name, member_count, created, updated): SummaryRow) -> ProjectSummary {
    ProjectSummary {
        id,
        name,
        role: db::role(&role),
        owner: UserRef {
            id: owner_id,
            display_name: owner_name,
        },
        member_count,
        created_at: iso(created),
        updated_at: iso(updated),
    }
}

async fn load_summary(conn: &mut SqliteConnection, user_id: &str, project_id: &str) -> ApiResult<ProjectSummary> {
    let row: SummaryRow = sqlx::query_as(concat!(summary_query!(), " AND projects.id = ?"))
        .bind(user_id)
        .bind(project_id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::not_found("Project not found"))?;
    Ok(summary(row))
}

async fn load_detail(state: &AppState, access: &Access) -> ApiResult<ProjectDetail> {
    let mut conn = state.db.acquire().await?;
    let summary = load_summary(&mut conn, access.user_id(), &access.project_id).await?;
    let members = members::list(&mut conn, &access.project_id).await?;
    let invites = match summary.role {
        Role::Owner => Some(invites::list(&mut conn, &access.project_id).await?),
        _ => None,
    };
    Ok(ProjectDetail {
        id: summary.id,
        name: summary.name,
        role: summary.role,
        owner: summary.owner,
        members,
        invites,
        created_at: summary.created_at,
        updated_at: summary.updated_at,
    })
}

async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Vec<ProjectSummary>>> {
    let rows: Vec<SummaryRow> = sqlx::query_as(concat!(summary_query!(), " ORDER BY projects.updated_at DESC"))
        .bind(user.id())
        .fetch_all(&state.db)
        .await?;
    Ok(Json(rows.into_iter().map(summary).collect()))
}

fn clean_name(name: &str) -> ApiResult<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 200 {
        return Err(ApiError::unprocessable("Enter a project name of up to 200 characters"));
    }
    Ok(name.to_owned())
}

/// Stores a new project with its first document state; the creator owns it.
async fn insert_project(
    conn: &mut SqliteConnection,
    project_id: &str,
    name: &str,
    owner_id: &str,
    state: &[u8],
) -> sqlx::Result<()> {
    let now = now_ms();
    sqlx::query("INSERT INTO projects (id, name, owner_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?)")
        .bind(project_id)
        .bind(name)
        .bind(owner_id)
        .bind(now)
        .bind(now)
        .execute(&mut *conn)
        .await?;
    sqlx::query("INSERT INTO members (project_id, user_id, role, added_at) VALUES (?, ?, 'owner', ?)")
        .bind(project_id)
        .bind(owner_id)
        .bind(now)
        .execute(&mut *conn)
        .await?;
    storage::write_snapshot(conn, project_id, 0, state).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateProjectRequest {
    name: String,
    #[serde(default)]
    l1_name: String,
}

async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    JsonBody(body): JsonBody<CreateProjectRequest>,
) -> ApiResult<(StatusCode, Json<ProjectSummary>)> {
    let name = clean_name(&body.name)?;
    let l1_name = match body.l1_name.trim() {
        "" => "English",
        l1 => l1,
    };
    let initial = doc::encode_state(&doc::new_doc(&doc::initial_project(&name, l1_name)));
    let project_id = new_id();
    let mut tx = state.db.begin().await?;
    insert_project(&mut tx, &project_id, &name, user.id(), &initial).await?;
    let summary = load_summary(&mut tx, user.id(), &project_id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(summary)))
}

async fn detail(State(state): State<AppState>, access: Access) -> ApiResult<Json<ProjectDetail>> {
    Ok(Json(load_detail(&state, &access).await?))
}

#[derive(Deserialize)]
struct UpdateProjectRequest {
    name: Option<String>,
}

async fn update(
    State(state): State<AppState>,
    access: Access,
    JsonBody(body): JsonBody<UpdateProjectRequest>,
) -> ApiResult<Json<ProjectDetail>> {
    access.require(Role::Owner)?;
    if let Some(name) = body.name {
        let name = clean_name(&name)?;
        // The document is the source of truth; the room mirrors meta.name
        // into the listing too, but do it now so the response is current.
        let written = name.clone();
        state
            .rooms
            .change(&access.project_id, Some(access.user_id()), move |txn| doc::write_name(txn, &written))
            .await?;
        sqlx::query("UPDATE projects SET name = ?, updated_at = ? WHERE id = ?")
            .bind(&name)
            .bind(now_ms())
            .bind(&access.project_id)
            .execute(&state.db)
            .await?;
    }
    Ok(Json(load_detail(&state, &access).await?))
}

async fn remove(State(state): State<AppState>, access: Access) -> ApiResult<StatusCode> {
    access.require(Role::Owner)?;
    sqlx::query("UPDATE projects SET deleted_at = ? WHERE id = ?")
        .bind(now_ms())
        .bind(&access.project_id)
        .execute(&state.db)
        .await?;
    state.rooms.close_project(&access.project_id).await;
    Ok(StatusCode::NO_CONTENT)
}

/// A copy of the current document, owned by the caller, with access to the
/// same audio.
async fn duplicate(State(state): State<AppState>, access: Access) -> ApiResult<(StatusCode, Json<ProjectSummary>)> {
    let current = state.rooms.read(&access.project_id, doc::encode_state).await?;
    let copy = doc::decode([current.as_slice()])?;
    let name = format!("{} (copy)", doc::read_name(&copy.transact()).trim());
    doc::write_name(&mut copy.transact_mut(), &name);
    let project_id = new_id();
    let mut tx = state.db.begin().await?;
    insert_project(&mut tx, &project_id, &name, access.user_id(), &doc::encode_state(&copy)).await?;
    sqlx::query(
        "INSERT INTO project_blobs (project_id, sha256) SELECT ?, sha256 FROM project_blobs WHERE project_id = ?",
    )
    .bind(&project_id)
    .bind(&access.project_id)
    .execute(&mut *tx)
    .await?;
    let summary = load_summary(&mut tx, access.user_id(), &project_id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(summary)))
}

/// A desktop `.loqr` archive becomes a new project; its clips are validated
/// and stored like uploads.
async fn import(
    State(state): State<AppState>,
    user: AuthUser,
    body: Result<Bytes, axum::extract::rejection::BytesRejection>,
) -> ApiResult<(StatusCode, Json<ProjectSummary>)> {
    let body = body?;
    let store = state.blobs.clone();
    let (project, blobs) = tokio::task::spawn_blocking(move || -> ApiResult<_> {
        let imported = loquaris_export::loqr::read(&body)
            .map_err(|error| ApiError::unprocessable(format!("Not a readable .loqr file: {error}")))?;
        let mut blobs = Vec::new();
        for (sha256, bytes) in &imported.blobs {
            let computed = sha256_hex(bytes);
            if &computed != sha256 {
                return Err(ApiError::unprocessable("The archive's audio does not match its index"));
            }
            let info = loquaris_export::wav::validate(bytes)
                .map_err(|error| ApiError::unprocessable(format!("Invalid audio in the archive: {error}")))?;
            store.put(sha256, bytes)?;
            blobs.push((computed, bytes.len() as u64, info));
        }
        Ok((imported.project, blobs))
    })
    .await??;

    let name = match project.name.trim() {
        "" => "Imported project".to_owned(),
        name => name.chars().take(200).collect(),
    };
    let mut project = project;
    project.name = name.clone();
    let state_bytes = doc::encode_state(&doc::new_doc(&project));
    let project_id = new_id();
    let mut tx = state.db.begin().await?;
    insert_project(&mut tx, &project_id, &name, user.id(), &state_bytes).await?;
    for (sha256, size, info) in &blobs {
        let info = BlobInfo {
            sha256,
            size: *size,
            sample_rate: info.sample_rate,
            frames: info.frames,
            channels: info.channels,
        };
        blobs::register(&mut tx, &info, &project_id, user.id()).await?;
    }
    let summary = load_summary(&mut tx, user.id(), &project_id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(summary)))
}
