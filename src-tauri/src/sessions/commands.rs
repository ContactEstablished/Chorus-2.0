//! Session commands. Inputs are checked in `SessionManager` (shape by serde, then semantics: the
//! session exists, the folder exists, sizes are bounded). Commands that may wait (launching,
//! ending a process tree, hashing a PIN) are async and run on a blocking thread, because Tauri runs
//! synchronous commands on the main thread.

use std::sync::Arc;

use tauri::State;
use tauri::ipc::Channel;

use super::{CloseReport, CreateSession, PtyBytes, SessionManager, SessionSummary};

type Manager<'a> = State<'a, Arc<SessionManager>>;

async fn blocking<T: Send + 'static>(
    m: &Manager<'_>,
    f: impl FnOnce(Arc<SessionManager>) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let m = m.inner().clone();
    tauri::async_runtime::spawn_blocking(move || f(m)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
pub fn sessions_list(state: Manager<'_>) -> Result<Vec<SessionSummary>, String> {
    state.list()
}

#[tauri::command]
#[specta::specta]
pub async fn session_create(state: Manager<'_>, request: CreateSession) -> Result<SessionSummary, String> {
    blocking(&state, move |m| m.create(request)).await
}

/// Starts sending a session's output to `output`: its saved scrollback first, then live bytes.
/// Returns a view id for `session_detach`.
#[tauri::command]
#[specta::specta]
pub fn session_attach(state: Manager<'_>, session_id: u32, output: Channel<PtyBytes>) -> Result<u32, String> {
    state.attach(session_id, Box::new(move |bytes: &[u8]| output.send(PtyBytes(bytes.to_vec())).is_ok()))
}

#[tauri::command]
#[specta::specta]
pub fn session_detach(state: Manager<'_>, session_id: u32, view_id: u32) {
    state.detach(session_id, view_id);
}

#[tauri::command]
#[specta::specta]
pub fn session_write(state: Manager<'_>, session_id: u32, data: String) -> Result<(), String> {
    state.write(session_id, &data)
}

#[tauri::command]
#[specta::specta]
pub fn session_resize(state: Manager<'_>, session_id: u32, cols: u16, rows: u16) -> Result<(), String> {
    state.resize(session_id, cols, rows)
}

#[tauri::command]
#[specta::specta]
pub async fn session_restart(state: Manager<'_>, session_id: u32) -> Result<SessionSummary, String> {
    blocking(&state, move |m| m.restart(session_id)).await
}

#[tauri::command]
#[specta::specta]
pub async fn session_close(state: Manager<'_>, session_id: u32) -> Result<CloseReport, String> {
    blocking(&state, move |m| m.close(session_id)).await
}

/// Locking never asks for the PIN; unlocking does when one is set.
#[tauri::command]
#[specta::specta]
pub async fn session_set_locked(
    state: Manager<'_>,
    session_id: u32,
    locked: bool,
    pin: Option<String>,
) -> Result<SessionSummary, String> {
    blocking(&state, move |m| m.set_locked(session_id, locked, pin.as_deref())).await
}
