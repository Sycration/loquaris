//! Downloads rendered from the server's copy of the document, so every
//! collaborator gets the same current result: a PDF script, lesson audio
//! (one WAV, or a ZIP of one per lesson) and the desktop `.loqr` archive.
use std::io::{Cursor, Write};
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use loquaris_core::{AudioRef, Project};
use loquaris_export::{loqr, pdf, wav};
use serde::Deserialize;
use yrs::Transact;

use crate::blobs::BlobStore;
use crate::error::{ApiError, ApiResult};
use crate::projects::Access;
use crate::util::{attachment, file_stem};
use crate::{AppState, doc};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{id}/export/pdf", get(export_pdf))
        .route("/{id}/export/wav", get(export_wav))
        .route("/{id}/export/loqr", get(export_loqr))
}

async fn current_project(state: &AppState, project_id: &str) -> ApiResult<Project> {
    Ok(state.rooms.read(project_id, |doc| doc::read_project(&doc.transact())).await?)
}

/// The lesson IDs of a `lessons=a,b` parameter in script order, or all
/// lessons when absent; unknown IDs are an error.
fn select_lessons(project: &Project, lessons: Option<&str>) -> ApiResult<Option<Vec<String>>> {
    let Some(lessons) = lessons else { return Ok(None) };
    let wanted: Vec<&str> = lessons.split(',').map(str::trim).filter(|id| !id.is_empty()).collect();
    if let Some(unknown) = wanted.iter().find(|id| !project.lessons.iter().any(|lesson| lesson.id == **id)) {
        return Err(ApiError::not_found(format!("No lesson {unknown} in this project")));
    }
    Ok(Some(
        project
            .lessons
            .iter()
            .filter(|lesson| wanted.contains(&lesson.id.as_str()))
            .map(|lesson| lesson.id.clone())
            .collect(),
    ))
}

fn download(content_type: &str, file_name: &str, bytes: Vec<u8>) -> Response {
    (
        [
            (CONTENT_TYPE, content_type.to_owned()),
            (CONTENT_DISPOSITION, attachment(file_name)),
            (CACHE_CONTROL, "no-store".to_owned()),
        ],
        bytes,
    )
        .into_response()
}

/// Reads clips from the blob store for the renderers.
fn loader(store: Arc<dyn BlobStore>) -> impl FnMut(&AudioRef) -> Result<Vec<u8>, String> {
    move |audio| store.get(&audio.blob).map_err(|error| format!("audio {} unavailable: {error}", audio.blob))
}

fn render_error(error: String) -> ApiError {
    ApiError::unprocessable(error)
}

#[derive(Deserialize)]
struct PdfQuery {
    lessons: Option<String>,
}

async fn export_pdf(State(state): State<AppState>, access: Access, Query(query): Query<PdfQuery>) -> ApiResult<Response> {
    let project = current_project(&state, &access.project_id).await?;
    let lessons = select_lessons(&project, query.lessons.as_deref())?;
    let file_name = format!("{}.pdf", file_stem(&project.name));
    let bytes = tokio::task::spawn_blocking(move || pdf::render(&project, lessons.as_deref()))
        .await?
        .map_err(render_error)?;
    Ok(download("application/pdf", &file_name, bytes))
}

#[derive(Deserialize)]
struct WavQuery {
    lessons: Option<String>,
    teacher: Option<bool>,
    student: Option<bool>,
}

async fn export_wav(State(state): State<AppState>, access: Access, Query(query): Query<WavQuery>) -> ApiResult<Response> {
    let project = current_project(&state, &access.project_id).await?;
    let options = wav::Options {
        include_teacher: query.teacher.unwrap_or(true),
        include_student: query.student.unwrap_or(true),
    };
    if !options.include_teacher && !options.include_student {
        return Err(ApiError::unprocessable("Include the teacher, the student or both"));
    }
    let lessons = select_lessons(&project, query.lessons.as_deref())?
        .unwrap_or_else(|| project.lessons.iter().map(|lesson| lesson.id.clone()).collect());
    if lessons.is_empty() {
        return Err(ApiError::unprocessable("Choose at least one lesson"));
    }
    let store = state.blobs.clone();
    let stem = file_stem(&project.name);
    let (content_type, file_name, bytes) = tokio::task::spawn_blocking(move || -> ApiResult<_> {
        let names = wav::file_names(&project).map_err(render_error)?;
        let mut load = loader(store);
        let mut files = Vec::new();
        for id in &lessons {
            let index = project.lessons.iter().position(|lesson| &lesson.id == id).unwrap_or_default();
            let bytes = wav::render_lesson(&project, id, options, &mut load).map_err(render_error)?;
            files.push((names[index].clone(), bytes));
        }
        if let [(name, _)] = files.as_slice() {
            let name = name.clone();
            let (_, bytes) = files.pop().expect("one file");
            return Ok(("audio/wav", name, bytes));
        }
        Ok(("application/zip", format!("{stem}.zip"), zip(files)?))
    })
    .await??;
    Ok(download(content_type, &file_name, bytes))
}

/// An uncompressed ZIP (WAV audio does not compress usefully).
fn zip(files: Vec<(String, Vec<u8>)>) -> ApiResult<Vec<u8>> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored)
        .large_file(files.iter().map(|(_, bytes)| bytes.len() as u64).sum::<u64>() >= u32::MAX as u64);
    for (name, bytes) in files {
        zip.start_file(name, options).map_err(anyhow::Error::from)?;
        zip.write_all(&bytes)?;
    }
    Ok(zip.finish().map_err(anyhow::Error::from)?.into_inner())
}

async fn export_loqr(State(state): State<AppState>, access: Access) -> ApiResult<Response> {
    let project = current_project(&state, &access.project_id).await?;
    let file_name = format!("{}.loqr", file_stem(&project.name));
    let store = state.blobs.clone();
    let bytes = tokio::task::spawn_blocking(move || loqr::write(&project, &mut loader(store)))
        .await?
        .map_err(render_error)?;
    Ok(download("application/octet-stream", &file_name, bytes))
}
