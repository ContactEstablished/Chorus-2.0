//! Read-only progress boundary, separate from actions and their delivery receipts.

use serde::Serialize;
use specta::Type;
use chorus_core::agent::AgentKind;
use crate::store::progress::{self, PlanSnapshot, ProgressMessage};
use super::commands::Manager;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionProgress {
    pub plan: Option<PlanSnapshot>,
    pub messages: Vec<ProgressMessage>,
    pub current_run_id: Option<u32>,
    pub note: String,
    pub error: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub fn session_progress(manager: Manager<'_>, session_id: u32) -> Result<SessionProgress, String> {
    let row = manager.row(session_id)?;
    let run = manager.open_live(session_id).ok().and_then(|live| live.current_run());
    let error = run.as_ref().and_then(|r| r.recorder.lock().expect("recorder lock").progress.error.clone());
    let note = match row.agent {
        AgentKind::Claude => "Checklists appear when Claude uses its task tools. Only visible assistant text is captured; thinking and tool output stay out of history. Showing up to 200 recent messages.",
        AgentKind::Codex => "Codex plan and intermediate-message capture are not connected. Final messages remain in history; use Raw for live progress.",
        _ => "Plan and intermediate-message capture are not connected for this agent. Use Raw for live progress.",
    }.into();
    Ok(SessionProgress { plan: progress::latest_plan(&manager.store, session_id)?, messages: progress::recent_messages(&manager.store, session_id, 200)?, current_run_id: run.map(|r| r.id), note, error })
}
