//! Durable FIFO queue. Polling only evaluates structured state; elapsed time never means idle.
use chorus_core::actions::{capabilities, clean_prompt, prompt_blocker, ActionCapability, PromptSource};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::atomic::Ordering;
use super::{commands::Manager, SessionManager};
use crate::store::{delivery, queue::{self, QueuedInstruction}};

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QueueRequest { pub run_id: u32, pub request_id: String, pub text: String }

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QueueSnapshot {
    pub run_id: Option<u32>,
    pub raw_empty: bool,
    pub blocked_reason: Option<String>,
    pub entries: Vec<QueuedInstruction>,
}

pub fn snapshot(manager: &SessionManager, session: u32) -> Result<QueueSnapshot, String> {
    let row = manager.row(session)?;
    let run = manager.open_live(session).ok().and_then(|l| l.current_run());
    let entries = queue::list(&manager.store, session)?;
    let raw_empty = run.as_ref().is_some_and(|r| r.raw_empty.load(Ordering::SeqCst));
    let reason = if let ActionCapability::Unavailable { reason } = capabilities(row.agent).queue { Some(reason) }
    else if let Some(run) = &run {
        let recorder = run.recorder.lock().expect("recorder lock");
        let live = run.exit.lock().expect("exit lock").is_none() && run.requested.lock().expect("requested lock").is_none();
        prompt_blocker(row.locked, live, recorder.activity(), recorder.pending_question(), delivery::unobserved(&manager.store, run.id)?)
            .or_else(|| chorus_core::queue::blocker(entries.iter().filter(|e| e.status == "queued").min_by_key(|e| e.id).map_or(run.id, |e| e.run_id), run.id, recorder.activity(), raw_empty)).map(str::to_owned).or_else(|| run.queue_error.lock().expect("queue error lock").clone())
    } else { Some("No live run. Pending instructions remain saved for review after restart.".into()) };
    Ok(QueueSnapshot { run_id: run.map(|r| r.id), raw_empty, blocked_reason: reason, entries })
}

pub fn enqueue(manager: &SessionManager, session: u32, request: QueueRequest) -> Result<(), String> {
    if !chorus_core::agent::is_uuid(&request.request_id) { return Err("A queue request needs a UUID.".into()); }
    let text = clean_prompt(&request.text)?;
    let live = manager.open_live(session)?;
    let _action = live.action.lock().expect("action lock");
    let row = manager.row(session)?;
    manager.refuse_if_locked(&row, "queue instructions for")?;
    if let ActionCapability::Unavailable { reason } = capabilities(row.agent).queue { return Err(reason); }
    let run = live.current_run().ok_or("No live run. Restart the session first.")?;
    if run.exit.lock().expect("exit lock").is_some() || run.requested.lock().expect("requested lock").is_some() { return Err("The run ended. Restart and review before queueing.".into()); }
    if run.id != request.run_id { return Err("The session restarted. Review the current run first.".into()); }
    let recorder = run.recorder.lock().expect("recorder lock");
    let text = recorder.clean(&text);
    if delivery::existing(&manager.store, session, &request.request_id, &text, PromptSource::Composer)?.is_some() {
        return Err("This instruction request was already delivered. It cannot be queued again.".into());
    }
    queue::enqueue(&manager.store, session, run.id, &request.request_id, &text)
}

/// Holds the same lock as Raw writes, cancellation, lock, restart and immediate sends.
pub fn tick(manager: &SessionManager, session: u32) {
    let result = dispatch(manager, session);
    if let Ok(run) = manager.running(session) {
        // Live state blockers are recomputed by snapshot; retain storage failures only.
        let error = result.err().filter(|e| e.starts_with("database:")).map(|e| run.recorder.lock().expect("recorder lock").clean(&e));
        *run.queue_error.lock().expect("queue error lock") = error;
    }
}

pub fn dispatch(manager: &SessionManager, session: u32) -> Result<(), String> {
    let live = manager.open_live(session)?;
    let action = live.action.lock().expect("action lock");
    let Some(item) = queue::first(&manager.store, session)? else { return Ok(()) };
    let run = live.current_run().ok_or("No live run.")?;
    if item.run_id != run.id { return Ok(()) }
    super::delivery::deliver_locked(manager, session, item.run_id, &item.request_id, &item.text, PromptSource::Composer, &live)?;
    drop(action);
    manager.notify_id(session);
    Ok(())
}

pub fn change(manager: &SessionManager, session: u32, id: u32, run_id: Option<u32>) -> Result<(), String> {
    let live = manager.open_live(session).ok();
    let _action = live.as_ref().map(|l| l.action.lock().expect("action lock"));
    manager.refuse_if_locked(&manager.row(session)?, "change queued instructions for")?;
    if let Some(reviewed) = run_id {
        let run = live.as_ref().and_then(|l| l.current_run()).ok_or("No live run. Restart first.")?;
        if run.id != reviewed { return Err("The session restarted again. Review the current run.".into()); }
    }
    queue::change(&manager.store, session, id, run_id)
}

pub fn confirm_empty(manager: &SessionManager, session: u32, run_id: u32) -> Result<(), String> {
    let live = manager.open_live(session)?;
    let _action = live.action.lock().expect("action lock");
    manager.refuse_if_locked(&manager.row(session)?, "confirm the Raw prompt for")?;
    let run = live.current_run().ok_or("No live run.")?;
    if run.id != run_id { return Err("The session restarted. Inspect Raw in the current run.".into()); }
    run.raw_empty.store(true, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn session_queue(state: Manager<'_>, session_id: u32) -> Result<QueueSnapshot, String> { snapshot(&state, session_id) }
#[tauri::command]
#[specta::specta]
pub fn enqueue_instruction(state: Manager<'_>, session_id: u32, request: QueueRequest) -> Result<(), String> { enqueue(&state, session_id, request) }
#[tauri::command]
#[specta::specta]
pub fn change_queued_instruction(state: Manager<'_>, session_id: u32, instruction_id: u32, reviewed_run_id: Option<u32>) -> Result<(), String> { change(&state, session_id, instruction_id, reviewed_run_id) }
#[tauri::command]
#[specta::specta]
pub fn confirm_raw_prompt_empty(state: Manager<'_>, session_id: u32, run_id: u32) -> Result<(), String> { confirm_empty(&state, session_id, run_id) }
