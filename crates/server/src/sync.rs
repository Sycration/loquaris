//! `GET /api/projects/:id/sync`: the WebSocket the browser's stock
//! `y-websocket` provider connects to. The upgrade checks `Origin` (against
//! cross-site WebSocket hijacking); the session and membership are checked
//! after upgrading, so a refused client learns why from the close code (see
//! [`crate::rooms::close`]) instead of a bare failed handshake.
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::http::header::{HOST, ORIGIN};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Router};
use futures_util::{SinkExt, StreamExt};

use crate::AppState;
use crate::auth::CurrentSession;
use crate::config::Config;
use crate::db::Role;
use crate::error::ApiError;
use crate::projects::member_role;
use crate::rooms::{CloseReason, Outbound, Rooms, close};

/// Largest WebSocket message accepted (a full sync of a big project).
const MAX_MESSAGE: usize = 16 << 20;

pub fn router() -> Router<AppState> {
    Router::new().route("/{id}/sync", get(upgrade))
}

/// True if a browser on `origin` may open the socket: the public URL's
/// origin, or the host the request was sent to. Non-browser clients send no
/// `Origin` and cannot be hijacked.
fn origin_allowed(config: &Config, headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(ORIGIN).and_then(|value| value.to_str().ok()) else {
        return true;
    };
    if origin.eq_ignore_ascii_case(config.public_origin()) {
        return true;
    }
    let host = headers.get(HOST).and_then(|value| value.to_str().ok());
    let origin_host = origin.split_once("://").map(|(_, host)| host);
    matches!((origin_host, host), (Some(a), Some(b)) if a.eq_ignore_ascii_case(b))
}

async fn upgrade(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    headers: HeaderMap,
    session: Option<Extension<CurrentSession>>,
    ws: WebSocketUpgrade,
) -> Response {
    if !origin_allowed(&state.config, &headers) {
        return ApiError::forbidden("Cross-origin connection refused").into_response();
    }
    let ws = ws.max_message_size(MAX_MESSAGE).max_frame_size(MAX_MESSAGE);
    let authorized = match session {
        None => Err(CloseReason(close::UNAUTHENTICATED, "Not signed in".into())),
        Some(Extension(session)) => match member_role(&state.db, &project_id, &session.user.id).await {
            Ok(Some(role)) => Ok((session, role)),
            Ok(None) => Err(CloseReason(close::NOT_FOUND, "Project not found".into())),
            Err(error) => {
                tracing::error!("membership check failed: {error}");
                Err(CloseReason(close::INTERNAL, "Server error".into()))
            }
        },
    };
    let rooms = state.rooms.clone();
    ws.on_upgrade(move |socket| async move {
        match authorized {
            Ok((session, role)) => serve(socket, rooms, project_id, session, role).await,
            Err(CloseReason(code, reason)) => refuse(socket, code, reason).await,
        }
    })
}

async fn refuse(mut socket: WebSocket, code: u16, reason: String) {
    let frame = CloseFrame {
        code,
        reason: reason.into(),
    };
    let _ = socket.send(Message::Close(Some(frame))).await;
}

async fn serve(socket: WebSocket, rooms: Arc<Rooms>, project_id: String, session: CurrentSession, role: Role) {
    let joined = match rooms.join(&project_id, &session.user.id, &session.session_id, role).await {
        Ok(joined) => joined,
        Err(error) => {
            tracing::warn!(project = project_id, "cannot open room: {error:#}");
            return refuse(socket, close::RESTART, "Project unavailable".into()).await;
        }
    };
    let (room, conn, mut rx) = (joined.room, joined.conn, joined.rx);
    let (mut sink, mut stream) = socket.split();

    // The writer forwards queued messages; when the room drops this peer
    // its queue closes and the writer ends the connection.
    let mut writer = tokio::spawn(async move {
        let mut frame = CloseFrame {
            code: close::TOO_SLOW,
            reason: "Disconnected".into(),
        };
        while let Some(outbound) = rx.recv().await {
            match outbound {
                Outbound::Binary(bytes) => {
                    if sink.send(Message::Binary(bytes.into())).await.is_err() {
                        return;
                    }
                }
                Outbound::Close(code, reason) => {
                    frame = CloseFrame {
                        code,
                        reason: reason.into(),
                    };
                    break;
                }
            }
        }
        let _ = sink.send(Message::Close(Some(frame))).await;
        let _ = sink.close().await;
    });

    let mut writer_done = false;
    loop {
        tokio::select! {
            message = stream.next() => match message {
                Some(Ok(Message::Binary(data))) => {
                    if let Err(CloseReason(code, reason)) = rooms.receive(&room, conn, &data).await {
                        rooms.disconnect_conn(&room, conn, code, &reason).await;
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                // Pings are answered by the WebSocket layer; text is not part
                // of the protocol.
                Some(Ok(_)) => {}
            },
            _ = &mut writer => {
                writer_done = true;
                break;
            }
        }
    }
    rooms.leave(&room, conn).await;
    // Give the writer a moment to send a close frame, then stop it.
    if !writer_done && tokio::time::timeout(Duration::from_secs(5), &mut writer).await.is_err() {
        writer.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use std::path::PathBuf;

    #[test]
    fn checks_origin() {
        let mut config = Config::new(PathBuf::from("x"));
        config.public_url = "https://loquaris.example".into();
        let headers = |origin: Option<&str>, host: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(HOST, HeaderValue::from_str(host).unwrap());
            if let Some(origin) = origin {
                headers.insert(ORIGIN, HeaderValue::from_str(origin).unwrap());
            }
            headers
        };
        assert!(origin_allowed(&config, &headers(Some("https://loquaris.example"), "internal:8080")));
        assert!(origin_allowed(&config, &headers(Some("http://localhost:8080"), "localhost:8080")));
        assert!(origin_allowed(&config, &headers(None, "localhost:8080")));
        assert!(!origin_allowed(&config, &headers(Some("https://evil.example"), "loquaris.example")));
    }
}
