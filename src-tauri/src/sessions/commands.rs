//! Session commands. Inputs are checked in `SessionManager` (shape by serde, then semantics: the
//! session exists, the folder exists, sizes are bounded). Commands that may wait (launching,
//! ending a process tree, hashing a PIN) are async and run on a blocking thread, because Tauri runs
//! synchronous commands on the main thread.

use std::sync::Arc;

use chorus_core::agent::AgentKind;
use tauri::State;
use tauri::ipc::Channel;

use super::workspace::{CloseCheck, FolderCheck, TagUse};
use super::{CloseReport, CreateSession, LaunchOutcome, LaunchRequest, PresetChoice, PtyBytes, SessionManager, SessionSummary};

/// How many names a suggestion may be asked to skip.
const MAX_SKIP: usize = 64;

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

/// Open only this session's recorded working directory; the frontend cannot supply an arbitrary path.
#[tauri::command]
#[specta::specta]
pub async fn session_open_folder(app: tauri::AppHandle, state: Manager<'_>, session_id: u32) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    blocking(&state, move |m| {
        let folder = m.row(session_id)?.cwd;
        if !std::path::Path::new(&folder).is_dir() {
            return Err("The session folder no longer exists or is unavailable.".into());
        }
        app.opener().open_path(folder, None::<&str>).map_err(|e| format!("opening the session folder: {e}"))
    }).await
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
pub async fn session_close(state: Manager<'_>, session_id: u32, remove_worktree: bool) -> Result<CloseReport, String> {
    blocking(&state, move |m| m.close(session_id, remove_worktree)).await
}

/// What closing would do to the session's worktree, if it has one: asked before Close.
#[tauri::command]
#[specta::specta]
pub async fn session_close_check(state: Manager<'_>, session_id: u32) -> Result<CloseCheck, String> {
    blocking(&state, move |m| m.close_check(session_id)).await
}

#[tauri::command]
#[specta::specta]
pub fn session_set_tags(state: Manager<'_>, session_id: u32, tags: Vec<String>) -> Result<SessionSummary, String> {
    state.set_tags(session_id, &tags)
}

/// Every tag used before, the most used first: New session's suggestions.
#[tauri::command]
#[specta::specta]
pub fn tags_known(state: Manager<'_>) -> Result<Vec<TagUse>, String> {
    state.known_tags()
}

/// A name for a new session that no open session has, nor any in `skip`.
#[tauri::command]
#[specta::specta]
pub fn session_suggest_name(state: Manager<'_>, skip: Vec<String>) -> String {
    let skip: Vec<String> = skip.into_iter().take(MAX_SKIP).collect();
    state.suggest_name(&skip)
}

/// What New session shows about a folder: whether it can be used, its git branch, and running
/// sessions already working there.
#[tauri::command]
#[specta::specta]
pub async fn folder_check(state: Manager<'_>, folder: String) -> Result<FolderCheck, String> {
    blocking(&state, move |m| Ok(m.check_folder(&folder))).await
}

#[tauri::command]
#[specta::specta]
pub async fn worktrees_list(state: Manager<'_>, folder: Option<String>) -> Result<Vec<super::worktrees::WorktreeEntry>, String> {
    blocking(&state, move |m| m.worktrees_list(folder.as_deref())).await
}

#[tauri::command]
#[specta::specta]
pub async fn session_git_status(state: Manager<'_>, session_id: u32) -> Result<Option<crate::git::Snapshot>, String> {
    blocking(&state, move |m| m.session_git_status(session_id)).await
}

/// Recently used folders that still exist, newest first.
#[tauri::command]
#[specta::specta]
pub fn folders_recent(state: Manager<'_>) -> Vec<String> {
    state.recent_folders()
}

/// Windows' folder picker, over the main window, starting at `start` if it exists. None when
/// the user cancels.
#[tauri::command]
#[specta::specta]
pub async fn folder_pick(app: tauri::AppHandle, start: Option<String>) -> Option<String> {
    use tauri::Manager as _;
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app.dialog().file().set_title("Choose a folder");
        if let Some(start) = start.filter(|s| std::path::Path::new(s).is_dir()) {
            dialog = dialog.set_directory(start);
        }
        if let Some(window) = app.get_webview_window("main") {
            dialog = dialog.set_parent(&window);
        }
        dialog.blocking_pick_folder().and_then(|p| p.into_path().ok()).map(|p| p.display().to_string())
    })
    .await
    .ok()
    .flatten()
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
