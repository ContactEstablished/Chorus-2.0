//! P6's backend action boundary. Reply variants are typed now, but cannot claim a response
//! channel until it is implemented and verified. They never turn into guessed PTY keystrokes.

use chorus_core::actions::{ActionCapabilities, SessionAction, capabilities};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{SessionManager, commands::Manager, delivery::deliver_prompt};
use crate::store::delivery::Delivered;

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActionRequest {
    pub run_id: u32,
    pub request_id: String,
    pub action: SessionAction,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionCapabilities {
    pub run_id: Option<u32>,
    pub capabilities: ActionCapabilities,
}

pub struct ActionRouter;

impl ActionRouter {
    pub fn dispatch(manager: &SessionManager, session: u32, request: ActionRequest) -> Result<Delivered, String> {
        manager.row(session)?;
        match request.action {
            SessionAction::Prompt { text, source } => deliver_prompt(manager, session, request.run_id, &request.request_id, &text, source),
            SessionAction::Answer { ask_id, text } => {
                if ask_id == 0 || text.len() > chorus_core::actions::MAX_PROMPT_BYTES { return Err("Invalid question reply.".into()); }
                Err("Structured question replies are not wired yet. Answer the agent in Raw.".into())
            }
            SessionAction::Approval { ask_id, message, .. } => {
                if ask_id == 0 || message.as_ref().is_some_and(|s| s.len() > chorus_core::actions::MAX_PROMPT_BYTES) { return Err("Invalid approval reply.".into()); }
                Err("Chorus currently observes approvals only. Approve or deny in Raw.".into())
            }
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn session_action_capabilities(state: Manager<'_>, session_id: u32) -> Result<SessionCapabilities, String> {
    let row = state.row(session_id)?;
    let run_id = state.open_live(session_id).ok().and_then(|l| l.current_run()).map(|r| r.id);
    Ok(SessionCapabilities { run_id, capabilities: capabilities(row.agent) })
}

#[tauri::command]
#[specta::specta]
pub async fn session_action(state: Manager<'_>, session_id: u32, request: ActionRequest) -> Result<Delivered, String> {
    let m = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || ActionRouter::dispatch(&m, session_id, request)).await.map_err(|e| e.to_string())?
}
