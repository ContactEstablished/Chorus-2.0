//! Session commands. Inputs are checked in `SessionManager` (shape by serde, then semantics: the
//! session exists, the folder exists, sizes are bounded). Commands that may wait (launching,
//! ending a process tree, hashing a PIN) are async and run on a blocking thread, because Tauri runs
//! synchronous commands on the main thread.

use std::sync::Arc;

use chorus_core::agent::AgentKind;
use tauri::State;
use tauri::ipc::Channel;

use super::{CloseReport, CreateSession, LaunchOutcome, LaunchRequest, PresetChoice, PtyBytes, SessionManager, SessionSummary};

pub type Manager<'a> = State<'a, Arc<SessionManager>>;

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

/// Starts the sessions a preset makes (one for Solo, two for Pair and Workbench).
#[tauri::command]
#[specta::specta]
pub async fn sessions_launch(state: Manager<'_>, request: LaunchRequest) -> Result<Vec<LaunchOutcome>, String> {
    blocking(&state, move |m| m.launch(request)).await
}

/// Every preset as it would work for `agent` on this machine, with the reason when it can't.
#[tauri::command]
#[specta::specta]
pub fn launch_presets(state: Manager<'_>, agent: AgentKind) -> Vec<PresetChoice> {
    state.presets(agent)
}

#[tauri::command]
#[specta::specta]
pub fn session_rename(state: Manager<'_>, session_id: u32, name: String) -> Result<SessionSummary, String> {
    state.rename(session_id, &name)
}

#[tauri::command]
#[specta::specta]
pub fn session_set_description(state: Manager<'_>, session_id: u32, description: Option<String>) -> Result<SessionSummary, String> {
    state.set_description(session_id, description.as_deref())
}

/// A session's newest turns with their prompts, final messages, tool calls and asks.
#[tauri::command]
#[specta::specta]
pub fn session_history(state: Manager<'_>, session_id: u32, limit: u32) -> Result<crate::store::history::SessionHistory, String> {
    state.history(session_id, limit)
}
