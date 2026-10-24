//! P6's backend action boundary. Claude's held HTTP replies and the shared prompt-delivery
//! path have separate lifetime checks. Unsupported requests never become guessed keystrokes.

use chorus_core::actions::{ActionCapabilities, SessionAction, capabilities};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{SessionManager, commands::Manager, delivery::deliver_prompt};
use crate::store::delivery::Delivered;
use crate::store::replies::{self, ReplyReceipt};
use crate::events::replies::PendingReply;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ActionResult { Prompt { receipt: Delivered }, Reply { receipt: ReplyReceipt } }

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
    pub fn dispatch(manager: &SessionManager, session: u32, request: ActionRequest) -> Result<ActionResult, String> {
        manager.row(session)?;
        match request.action {
            SessionAction::Prompt { text, source } => deliver_prompt(manager, session, request.run_id, &request.request_id, &text, source).map(|receipt| ActionResult::Prompt { receipt }),
            _ => Self::reply(manager, session, request).map(|receipt| ActionResult::Reply { receipt }),
        }
    }

    fn reply(manager: &SessionManager, session: u32, request: ActionRequest) -> Result<ReplyReceipt, String> {
        let ask = match &request.action {
            SessionAction::Answer { ask_id, .. } | SessionAction::Approval { ask_id, .. } | SessionAction::Release { ask_id } => *ask_id,
            _ => return Err("Expected a reply.".into()),
        };
        if ask == 0 || !chorus_core::agent::is_uuid(&request.request_id) { return Err("A reply needs an ask ID and a UUID request ID.".into()); }
        let text = serde_json::to_string(&request.action).map_err(|e| e.to_string())?;
        if text.len() > chorus_core::replies::MAX_REQUEST_BYTES { return Err("Reply is too large.".into()); }
        let live = manager.open_live(session)?;
        let _action = live.action.lock().expect("action lock");
        let row = manager.row(session)?;
        manager.refuse_if_locked(&row, "reply to")?;
        let run = live.current_run().ok_or("No live run. Use Raw after restarting.")?;
        let mut recorder = run.recorder.lock().expect("recorder lock");
        let scrubbed = recorder.clean_value(&serde_json::to_value(&request.action).map_err(|e| e.to_string())?).to_string();
        if let Some(receipt) = replies::existing(&manager.store, session, request.run_id, ask, &request.request_id, &scrubbed)? { return Ok(receipt); }
        if run.id != request.run_id { return Err("The session restarted. Review its current request.".into()); }
        if run.exit.lock().expect("exit lock").is_some() || run.requested.lock().expect("requested lock").is_some()
            || manager.quitting.load(std::sync::atomic::Ordering::SeqCst) { return Err("The run ended. Its reply channel is closed.".into()); }
        let held = recorder.replies.get_mut(&i64::from(ask)).ok_or("This request is no longer available in Friendly. Check Raw.")?;
        if !held.available() { return Err("This reply channel expired, disconnected, or was already used. Check Raw.".into()); }
        let body = chorus_core::replies::response(&held.request, &request.action)?;
        let receipt = replies::begin(&manager.store, session, run.id, ask, &request.request_id, &scrubbed)?;
        // A receiver disconnect racing this send is uncertain, never permission to retry. The
        // agent can also answer in Raw first. Only later agent events settle the history ask.
        if let Some(sender) = held.sender.take() { let _ = sender.send(body); }
        drop(recorder);
        drop(_action);
        manager.notify_id(session);
        Ok(receipt)
    }
}

#[tauri::command]
#[specta::specta]
pub fn session_action_capabilities(state: Manager<'_>, session_id: u32) -> Result<SessionCapabilities, String> {
    let row = state.row(session_id)?;
    let run = state.open_live(session_id).ok().and_then(|l| l.current_run());
    let mut caps = capabilities(row.agent);
    if row.agent == chorus_core::agent::AgentKind::Claude && run.as_ref().is_some_and(|r| r.hook_token.is_some()) {
        caps.question = chorus_core::actions::ActionCapability::HeldHook;
        caps.approval = chorus_core::actions::ActionCapability::HeldHook;
        caps.plan = chorus_core::actions::ActionCapability::Transcript;
    }
    if run.as_ref().and_then(|r|r.codex_bridge.as_ref()).is_some_and(|b|b.connected.load(std::sync::atomic::Ordering::SeqCst) && b.ready.load(std::sync::atomic::Ordering::SeqCst)) {
        caps.question = chorus_core::actions::ActionCapability::Server;
        caps.approval = chorus_core::actions::ActionCapability::Server;
        caps.plan = chorus_core::actions::ActionCapability::Server;
    }
    Ok(SessionCapabilities { run_id: run.map(|r| r.id), capabilities: caps })
}

#[tauri::command]
#[specta::specta]
pub fn session_pending_replies(state: Manager<'_>, session_id: u32) -> Result<Vec<PendingReply>, String> {
    let row = state.row(session_id)?;
    let Some(run) = state.open_live(session_id).ok().and_then(|l| l.current_run()) else { return Ok(Vec::new()) };
    let mut pending = run.recorder.lock().expect("recorder lock").pending_replies();
    if row.locked { for p in &mut pending { p.blocked_reason = Some("Session is locked. Unlock it to reply.".into()); } }
    Ok(pending)
}

#[tauri::command]
#[specta::specta]
pub async fn session_action(state: Manager<'_>, session_id: u32, request: ActionRequest) -> Result<ActionResult, String> {
    let m = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || ActionRouter::dispatch(&m, session_id, request)).await.map_err(|e| e.to_string())?
}
