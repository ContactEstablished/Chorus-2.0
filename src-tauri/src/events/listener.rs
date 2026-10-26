//! The loopback listener agent hooks post to: `POST 127.0.0.1:<port>/hook/<session-id>` with the
//! run's token as `Authorization: Bearer …` (D36, D37). Observation returns empty 204. Verified
//! Claude asks hold a bounded response; only an explicit user action supplies a decision.

use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, Method, StatusCode, header::AUTHORIZATION};
use axum::routing::{any, post};
use axum::response::{IntoResponse, Response};

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
        // P15: Chorus's tools for a loop step's agent, over MCP's streamable HTTP.
        .route("/mcp/{step}", any(mcp))
        .layer(DefaultBodyLimit::max(MAX_HOOK_BODY))
        .with_state(manager);
    tauri::async_runtime::spawn(async move {
        if let Ok(listener) = tokio::net::TcpListener::from_std(listener) {
            let _ = axum::serve(listener, app).await;
        }
    });
    Ok(port)
}

fn bearer(headers: &HeaderMap) -> String {
    headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).unwrap_or_default().to_owned()
}

/// JSON-RPC over POST, plain JSON answers, 202 for notifications. GET (a server-sent stream) and
/// DELETE (end of session) get 405, which every client checked in spike S3 accepts.
async fn mcp(State(m): State<Arc<SessionManager>>, Path(step): Path<u32>, method: Method, headers: HeaderMap, body: Bytes) -> Response {
    let token = bearer(&headers);
    let body = String::from_utf8_lossy(&body).into_owned();
    let post = method == Method::POST;
    let served = tauri::async_runtime::spawn_blocking(move || {
        let r = crate::run_loop::tools::serve(&m, step, &token, if post { &body } else { "[]" });
        (post, r)
    })
    .await;
    use crate::run_loop::tools::Refused;
    match served {
        Ok((_, Err(Refused::Unauthorised))) => StatusCode::UNAUTHORIZED.into_response(),
        Ok((false, _)) => (StatusCode::METHOD_NOT_ALLOWED, [(axum::http::header::ALLOW, "POST")]).into_response(),
        Ok((true, Ok(Some(json)))) => ([(axum::http::header::CONTENT_TYPE, "application/json")], json).into_response(),
        Ok((true, Ok(None))) => StatusCode::ACCEPTED.into_response(),
        Ok((true, Err(Refused::Bad(e)))) => (StatusCode::BAD_REQUEST, e).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

async fn hook(State(m): State<Arc<SessionManager>>, Path(session): Path<u32>, headers: HeaderMap, body: Bytes) -> Response {
    let token = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or_default()
        .to_owned();
    match tauri::async_runtime::spawn_blocking(move || m.receive_hook(session, &token, &body)).await {
        Ok(Ok(Some(receiver))) => {
            match tokio::time::timeout(std::time::Duration::from_secs(chorus_core::replies::REPLY_WAIT_SECS), receiver).await {
                Ok(Ok(value)) if value != serde_json::json!({}) => (
                    [(axum::http::header::CONTENT_TYPE, "application/json")], value.to_string()
                ).into_response(),
                _ => StatusCode::NO_CONTENT.into_response(),
            }
        }
        Ok(Ok(None)) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(HookRefused::NotFound)) => StatusCode::NOT_FOUND.into_response(),
        Ok(Err(HookRefused::Unauthorized)) => StatusCode::UNAUTHORIZED.into_response(),
        Ok(Err(HookRefused::BadBody)) => StatusCode::BAD_REQUEST.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
