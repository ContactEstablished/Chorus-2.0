//! The loopback listener agent hooks post to: `POST 127.0.0.1:<port>/hook/<session-id>` with the
//! run's token as `Authorization: Bearer …` (D36, D37). It only listens: every accepted post gets
//! an empty 204, so an agent never reads a decision Chorus didn't make (LESSONS L2). The reply is
//! sent after the event is written, so a prompt is durable before the agent acts on it.

use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode, header::AUTHORIZATION};
use axum::routing::post;

use super::HookRefused;
use crate::sessions::SessionManager;

const MAX_HOOK_BODY: usize = 2 * 1024 * 1024;

/// Binds a free loopback port and serves on Tauri's runtime. Returns the port.
pub fn start(manager: Arc<SessionManager>) -> Result<u16, String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("binding the event listener: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let app = Router::new()
        .route("/hook/{session}", post(hook))
        .layer(DefaultBodyLimit::max(MAX_HOOK_BODY))
        .with_state(manager);
    tauri::async_runtime::spawn(async move {
        if let Ok(listener) = tokio::net::TcpListener::from_std(listener) {
            let _ = axum::serve(listener, app).await;
        }
    });
    Ok(port)
}

async fn hook(State(m): State<Arc<SessionManager>>, Path(session): Path<u32>, headers: HeaderMap, body: Bytes) -> StatusCode {
    let token = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default()
        .to_owned();
    match tauri::async_runtime::spawn_blocking(move || m.receive_hook(session, &token, &body)).await {
        Ok(Ok(())) => StatusCode::NO_CONTENT,
        Ok(Err(HookRefused::NotFound)) => StatusCode::NOT_FOUND,
        Ok(Err(HookRefused::Unauthorized)) => StatusCode::UNAUTHORIZED,
        Ok(Err(HookRefused::BadBody)) => StatusCode::BAD_REQUEST,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
