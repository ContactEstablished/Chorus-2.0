//! Friendly drafts and queue. The backend owns dispatch even when no view is attached.
use super::{SessionManager, commands::Manager};
use crate::store::{composer as db, delivery};
use chorus_core::{
    actions::{ActionCapability, PromptSource, capabilities},
    composer::send_blocker,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::{Arc, atomic::Ordering};

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ComposerState {
    pub draft: db::Draft,
    pub items: Vec<db::QueuedPrompt>,
    pub run_id: Option<u32>,
    pub input_epoch: u32,
    pub send_reason: Option<String>,
    pub next_send_reason: Option<String>,
    pub queue_reason: Option<String>,
    pub can_confirm_ready: bool,
    pub dispatch_reason: Option<String>,
    pub image_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ComposerSubmit {
    pub run_id: u32,
    pub input_epoch: u32,
    pub request_id: String,
    pub draft_revision: u32,
    pub queue: bool,
    pub confirm_ready: bool,
}

fn available(manager: &SessionManager, session: u32, run: Option<&super::Run>) -> Result<(), String> {
    let row = manager.row(session)?;
    manager.refuse_if_locked(&row, "send to")?;
    if let ActionCapability::Unavailable { reason } = capabilities(row.agent).prompt {
        return Err(reason);
    }
    let run = run.ok_or("Session has no live run. Restart it first.")?;
    if run.codex_bridge.as_ref().is_some_and(|b| !b.connected.load(Ordering::SeqCst) || !b.ready.load(Ordering::SeqCst)) {
        return Err("Codex Friendly control is not ready. Finish startup in Raw, or restart if the connection was lost.".into());
    }
    if run.exit.lock().expect("exit lock").is_some()
        || run.requested.lock().expect("requested lock").is_some()
        || manager.quitting.load(Ordering::SeqCst)
    {
        return Err("The run is ending. The draft and queue are saved.".into());
    }
    Ok(())
}

pub(super) fn state(manager: &SessionManager, session: u32) -> Result<ComposerState, String> {
    let row = manager.row(session)?;
    let run = manager.open_live(session).ok().and_then(|l| l.current_run());
    let unavailable = available(manager, session, run.as_deref()).err();
    let mut send_reason = unavailable.clone();
    let mut dispatch_reason = unavailable.clone();
    let mut can_confirm_ready = false;
    let mut input_epoch = 0;
    if let Some(run) = &run {
        let recorder = run.recorder.lock().expect("recorder lock");
        input_epoch = recorder.input_epoch;
        let unobserved = delivery::unobserved(&manager.store, run.id)?;
        let reason = |confirmed, automatic| {
            send_blocker(recorder.activity(), recorder.pending_question(), recorder.raw_dirty, unobserved, confirmed, automatic)
                .map(str::to_owned)
        };
        if unavailable.is_none() {
            send_reason = reason(false, false);
            dispatch_reason = reason(false, true);
            can_confirm_ready = send_reason.is_some() && reason(true, false).is_none();
        }
    }
    let next_send_reason = send_reason.clone();
    if db::oldest(&manager.store, session)?.is_some() {
        send_reason = Some("An earlier instruction is waiting. Queue this one, or cancel the earlier instruction.".into());
    }
    Ok(ComposerState {
        image_reason: super::images::unsupported(row.agent),
        draft: db::draft(&manager.store, session)?,
        items: db::list(&manager.store, session)?,
        run_id: run.map(|r| r.id),
        input_epoch,
        send_reason,
        next_send_reason,
        queue_reason: unavailable,
        can_confirm_ready,
        dispatch_reason,
    })
}

pub(super) fn save(manager: &SessionManager, session: u32, revision: u32, text: &str) -> Result<db::Draft, String> {
    manager.row(session)?;
    if text.len() > chorus_core::actions::MAX_PROMPT_BYTES {
        return Err("Draft is too large (maximum 16 KiB).".into());
    }
    // Drafts can be saved while stopped, and include all vault keys, not just this run's key.
    let (keys, unreadable) = crate::vault::all_for_scrubbing(&manager.store);
    if unreadable != 0 {
        return Err("A vault key could not be read for scrubbing. Draft was not saved.".into());
    }
    let clean = String::from_utf8_lossy(&chorus_core::scrub::scrub_all(text.as_bytes(), &keys, &chorus_core::scrub::builtin_patterns()))
        .into_owned();
    let clean = if let Some(run) = manager.open_live(session).ok().and_then(|l| l.current_run()) {
        run.recorder.lock().expect("recorder lock").clean(&clean)
    } else {
        clean
    };
    db::save(&manager.store, session, revision, &clean)
}

pub(super) fn submit(manager: &SessionManager, session: u32, request: ComposerSubmit) -> Result<(), String> {
    if !chorus_core::agent::is_uuid(&request.request_id) {
        return Err("A prompt request needs a UUID.".into());
    }
    // An already accepted submit is a lookup, even if the run has since ended.
    if db::check_request(&manager.store, session, &request.request_id, request.run_id, request.draft_revision, request.queue)? {
        return Ok(());
    }
    let live = manager.open_live(session)?;
    let _action = live.action.lock().expect("action lock");
    if db::check_request(&manager.store, session, &request.request_id, request.run_id, request.draft_revision, request.queue)? {
        return Ok(());
    }
    let run = live.current_run();
    available(manager, session, run.as_deref())?;
    let run = run.expect("checked live run");
    if run.id != request.run_id {
        return Err("The session restarted. Review the current run before sending.".into());
    }
    let mut recorder = run.recorder.lock().expect("recorder lock");
    if !request.queue {
        if request.input_epoch != recorder.input_epoch {
            return Err("Raw input changed. Review its prompt before sending.".into());
        }
        if db::oldest(&manager.store, session)?.is_some() {
            return Err("An earlier instruction is waiting. Queue this one instead.".into());
        }
        if let Some(reason) = send_blocker(
            recorder.activity(),
            recorder.pending_question(),
            recorder.raw_dirty,
            delivery::unobserved(&manager.store, run.id)?,
            request.confirm_ready,
            false,
        ) {
            return Err(reason.into());
        }
    }
    let draft = db::draft(&manager.store, session)?;
    if draft.revision != request.draft_revision {
        return Err("The saved draft changed. Review it before sending.".into());
    }
    let paths = super::images::materialize(manager, session, &draft.images)?;
    let text = recorder.clean(&chorus_core::attachments::image_prompt(&draft.text, &paths)?);
    let id = db::enqueue(&manager.store, session, run.id, &request.request_id, draft.revision, &text, request.queue)?;
    if !request.queue {
        let receipt = delivery::begin_with_queue(
            &manager.store,
            session,
            run.id,
            &request.request_id,
            &text,
            PromptSource::Composer,
            crate::store::now_ms(),
            Some(id),
        );
        match receipt {
            Ok(receipt) => {
                recorder.raw_dirty = false;
                super::delivery::write_attempt(manager, &run, &text, receipt);
            }
            Err(_) => {
                db::hold(&manager.store, session, "Could not record the delivery attempt. Review before resuming.")?;
            }
        }
    }
    Ok(())
}

pub(super) fn tick(manager: &SessionManager, session: u32) -> Result<(), String> {
    let Some(item) = db::oldest(&manager.store, session)? else { return Ok(()) };
    if item.status != "queued" {
        return Ok(());
    }
    let Ok(live) = manager.open_live(session) else { return db::hold(&manager.store, session, "Session stopped. Review before resuming.") };
    let _action = live.action.lock().expect("action lock");
    let Some(item) = db::oldest(&manager.store, session)? else { return Ok(()) };
    if item.status != "queued" {
        return Ok(());
    }
    let run = live.current_run();
    if run.as_ref().is_none_or(|r| r.id != item.run_id) {
        return db::hold(&manager.store, session, "The run changed or ended. Review and resume for the current run.");
    }
    if available(manager, session, run.as_deref()).is_err() {
        return Ok(());
    }
    let run = run.expect("checked live run");
    let recorder = run.recorder.lock().expect("recorder lock");
    if send_blocker(
        recorder.activity(),
        recorder.pending_question(),
        recorder.raw_dirty,
        delivery::unobserved(&manager.store, run.id)?,
        false,
        true,
    )
    .is_some()
    {
        return Ok(());
    }
    super::images::materialize(manager, session, &item.images)?;
    let receipt = delivery::begin_with_queue(
        &manager.store,
        session,
        run.id,
        &item.request_id,
        &item.text,
        PromptSource::Composer,
        crate::store::now_ms(),
        Some(item.id),
    )?;
    super::delivery::write_attempt(manager, &run, &item.text, receipt);
    Ok(())
}

pub fn start(manager: &Arc<SessionManager>) -> Result<(), String> {
    db::recover(&manager.store)?;
    let weak = Arc::downgrade(manager);
    std::thread::Builder::new()
        .name("prompt-queue".into())
        .spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let Some(m) = weak.upgrade() else { break };
                if m.quitting.load(Ordering::SeqCst) {
                    break;
                }
                let sessions = m.store.with(|c| {
                    c.prepare("SELECT DISTINCT session_id FROM prompt_queue WHERE status='queued'")?
                        .query_map([], |r| r.get::<_, u32>(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()
                });
                if let Ok(sessions) = sessions {
                    for session in sessions {
                        if tick(&m, session).is_err() {
                            let _ = db::hold(&m.store, session, "Queue dispatch failed. Review before resuming.");
                        }
                    }
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn session_composer(state: Manager<'_>, session_id: u32) -> Result<ComposerState, String> {
    let m = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || self::state(&m, session_id)).await.map_err(|e| e.to_string())?
}
#[tauri::command]
#[specta::specta]
pub async fn session_draft_save(state: Manager<'_>, session_id: u32, revision: u32, text: String) -> Result<db::Draft, String> {
    let m = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || save(&m, session_id, revision, &text)).await.map_err(|e| e.to_string())?
}
#[tauri::command]
#[specta::specta]
pub async fn session_composer_submit(state: Manager<'_>, session_id: u32, request: ComposerSubmit) -> Result<ComposerState, String> {
    let m = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        submit(&m, session_id, request)?;
        self::state(&m, session_id)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
#[specta::specta]
pub async fn session_queue_change(
    state: Manager<'_>,
    session_id: u32,
    item_id: u32,
    resume_run_id: Option<u32>,
) -> Result<ComposerState, String> {
    let m = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        m.row(session_id)?;
        let live = m.open_live(session_id).ok();
        let _action = live.as_ref().map(|l| l.action.lock().expect("action lock"));
        if let Some(id) = resume_run_id {
            let run = live.as_ref().and_then(|l| l.current_run());
            available(&m, session_id, run.as_deref())?;
            if run.as_ref().map(|r| r.id) != Some(id) {
                return Err("The run changed. Review it again.".into());
            }
            db::resume(&m.store, session_id, item_id, id)?;
        } else {
            db::cancel(&m.store, session_id, item_id)?;
        }
        self::state(&m, session_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
pub async fn session_queue_send(
    state: Manager<'_>,
    session_id: u32,
    item_id: u32,
    run_id: u32,
    input_epoch: u32,
    confirm_ready: bool,
) -> Result<ComposerState, String> {
    let m = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let live = m.open_live(session_id)?;
        let _action = live.action.lock().expect("action lock");
        let run = live.current_run();
        available(&m, session_id, run.as_deref())?;
        let run = run.expect("checked live run");
        if run.id != run_id {
            return Err("The run changed. Review it again.".into());
        }
        let mut recorder = run.recorder.lock().expect("recorder lock");
        if recorder.input_epoch != input_epoch {
            return Err("Raw input changed. Review its prompt again.".into());
        }
        if let Some(reason) = send_blocker(
            recorder.activity(),
            recorder.pending_question(),
            recorder.raw_dirty,
            delivery::unobserved(&m.store, run_id)?,
            confirm_ready,
            false,
        ) {
            return Err(reason.into());
        }
        let item = db::get(&m.store, session_id, item_id)?;
        if db::oldest(&m.store, session_id)?.is_none_or(|q| q.id != item_id) {
            return Err("Only the first unsent instruction can be sent.".into());
        }
        if item.status == "held" {
            db::resume(&m.store, session_id, item_id, run_id)?;
        } else if item.run_id != run_id {
            return Err("This instruction belongs to an earlier run. Wait for it to be held, then review it.".into());
        }
        super::images::materialize(&m, session_id, &item.images)?;
        let receipt = delivery::begin_with_queue(
            &m.store,
            session_id,
            run_id,
            &item.request_id,
            &item.text,
            PromptSource::Composer,
            crate::store::now_ms(),
            Some(item_id),
        )?;
        recorder.raw_dirty = false;
        super::delivery::write_attempt(&m, &run, &item.text, receipt);
        drop(recorder);
        self::state(&m, session_id)
    })
    .await
    .map_err(|e| e.to_string())?
}
