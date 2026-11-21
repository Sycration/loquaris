//! Uploading clips (`POST /api/projects/:id/audio`) and serving them
//! (`GET /api/audio/:sha256`) with range requests, which browsers use for
//! media elements.
use axum::body::{Body, Bytes};
use axum::extract::rejection::BytesRejection;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::header::{
    ACCEPT_RANGES, CACHE_CONTROL, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, ETAG, IF_NONE_MATCH, RANGE,
};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use loquaris_core::AudioRef;

use crate::AppState;
use crate::auth::AuthUser;
use crate::blobs::{self, BlobInfo};
use crate::db::Role;
use crate::error::{ApiError, ApiResult};
use crate::projects::Access;
use crate::util::sha256_hex;

/// Largest WAV upload (10 minutes of 48 kHz stereo 16-bit is about 110 MB,
/// but recordings are capped and imports limited to this).
pub const MAX_UPLOAD: usize = 100 << 20;

/// Routes under `/api/projects`.
pub fn upload_router() -> Router<AppState> {
    Router::new().route("/{id}/audio", post(upload).layer(DefaultBodyLimit::max(MAX_UPLOAD)))
}

/// Routes under `/api/audio`.
pub fn router() -> Router<AppState> {
    Router::new().route("/{sha256}", get(download))
}

async fn upload(
    State(state): State<AppState>,
    access: Access,
    body: Result<Bytes, BytesRejection>,
) -> ApiResult<(StatusCode, Json<AudioRef>)> {
    access.require(Role::Editor)?;
    let body = body?;
    if body.is_empty() {
        return Err(ApiError::unprocessable("The file is empty"));
    }
    let store = state.blobs.clone();
    let (sha256, info) = tokio::task::spawn_blocking(move || -> ApiResult<_> {
        let info = loquaris_export::wav::validate(&body)
            .map_err(|error| ApiError::unprocessable(format!("Not a usable WAV file: {error}")))?;
        let sha256 = sha256_hex(&body);
        store.put(&sha256, &body)?;
        Ok((sha256, (body.len() as u64, info)))
    })
    .await??;
    let (size, info) = info;
    let mut conn = state.db.acquire().await?;
    let record = BlobInfo {
        sha256: &sha256,
        size,
        sample_rate: info.sample_rate,
        frames: info.frames,
        channels: info.channels,
    };
    blobs::register(&mut conn, &record, &access.project_id, access.user_id()).await?;
    Ok((
        StatusCode::CREATED,
        Json(AudioRef {
            blob: sha256,
            sample_rate: info.sample_rate,
            frames: info.frames,
            channels: info.channels,
        }),
    ))
}

/// The clip, if the user belongs to a project that may read it (404
/// otherwise, so blob existence is not revealed).
async fn download(
    State(state): State<AppState>,
    user: AuthUser,
    Path(sha256): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let not_found = || ApiError::not_found("No such audio");
    if !blobs::is_sha256(&sha256) {
        return Err(not_found());
    }
    let allowed: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM project_blobs
         JOIN members ON members.project_id = project_blobs.project_id AND members.user_id = ?
         JOIN projects ON projects.id = project_blobs.project_id AND projects.deleted_at IS NULL
         WHERE project_blobs.sha256 = ? LIMIT 1",
    )
    .bind(user.id())
    .bind(&sha256)
    .fetch_optional(&state.db)
    .await?;
    if allowed.is_none() {
        return Err(not_found());
    }

    let etag = format!("\"{sha256}\"");
    let common = [
        (CONTENT_TYPE, "audio/wav".to_owned()),
        (CACHE_CONTROL, "private, max-age=31536000, immutable".to_owned()),
        (ETAG, etag.clone()),
        (ACCEPT_RANGES, "bytes".to_owned()),
    ];
    if headers.get(IF_NONE_MATCH).is_some_and(|value| value.as_bytes() == etag.as_bytes()) {
        return Ok((StatusCode::NOT_MODIFIED, common).into_response());
    }

    let store = state.blobs.clone();
    let key = sha256.clone();
    let size = tokio::task::spawn_blocking(move || store.size(&key))
        .await??
        .ok_or_else(not_found)?;
    let range = headers.get(RANGE).and_then(|value| value.to_str().ok());
    let (status, start, len) = match range.map(|range| parse_range(range, size)) {
        None | Some(Range::Ignore) => (StatusCode::OK, 0, size),
        Some(Range::Bytes(start, end)) => (StatusCode::PARTIAL_CONTENT, start, end - start + 1),
        Some(Range::Unsatisfiable) => {
            return Ok((
                StatusCode::RANGE_NOT_SATISFIABLE,
                [(CONTENT_RANGE, format!("bytes */{size}"))],
            )
                .into_response());
        }
    };
    let store = state.blobs.clone();
    let bytes = tokio::task::spawn_blocking(move || store.get_range(&sha256, start, len)).await??;
    let mut response = (status, common, Body::from(bytes)).into_response();
    let headers = response.headers_mut();
    headers.insert(CONTENT_LENGTH, len.into());
    if status == StatusCode::PARTIAL_CONTENT {
        let value = format!("bytes {start}-{}/{size}", start + len - 1);
        headers.insert(CONTENT_RANGE, value.parse().expect("ASCII header"));
    }
    Ok(response)
}

#[derive(Debug, PartialEq, Eq)]
enum Range {
    /// Inclusive byte range.
    Bytes(u64, u64),
    /// Answer with the whole body (malformed or multi-range requests).
    Ignore,
    Unsatisfiable,
}

/// One `bytes=` range of a body of `size` bytes (RFC 9110 §14.1.2).
fn parse_range(header: &str, size: u64) -> Range {
    let Some(spec) = header.trim().strip_prefix("bytes=") else { return Range::Ignore };
    if spec.contains(',') {
        return Range::Ignore;
    }
    let Some((start, end)) = spec.trim().split_once('-') else { return Range::Ignore };
    let (start, end) = (start.trim(), end.trim());
    let parsed = match (start.parse::<u64>(), end.parse::<u64>()) {
        (Ok(start), Ok(end)) if start <= end => Some((start, end.min(size.saturating_sub(1)))),
        (Ok(start), Err(_)) if end.is_empty() => Some((start, size.saturating_sub(1))),
        (Err(_), Ok(suffix)) if start.is_empty() => {
            if suffix == 0 {
                return Range::Unsatisfiable;
            }
            Some((size.saturating_sub(suffix), size.saturating_sub(1)))
        }
        _ => None,
    };
    match parsed {
        Some((start, end)) if start < size && start <= end => Range::Bytes(start, end),
        Some(_) => Range::Unsatisfiable,
        None => Range::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ranges() {
        assert_eq!(parse_range("bytes=0-99", 1000), Range::Bytes(0, 99));
        assert_eq!(parse_range("bytes=500-", 1000), Range::Bytes(500, 999));
        assert_eq!(parse_range("bytes=-100", 1000), Range::Bytes(900, 999));
        assert_eq!(parse_range("bytes=900-5000", 1000), Range::Bytes(900, 999));
        assert_eq!(parse_range("bytes=-5000", 1000), Range::Bytes(0, 999));
        assert_eq!(parse_range("bytes=1000-", 1000), Range::Unsatisfiable);
        assert_eq!(parse_range("bytes=0-1,5-6", 1000), Range::Ignore);
        assert_eq!(parse_range("items=0-1", 1000), Range::Ignore);
        assert_eq!(parse_range("bytes=5-1", 1000), Range::Ignore);
    }
}
