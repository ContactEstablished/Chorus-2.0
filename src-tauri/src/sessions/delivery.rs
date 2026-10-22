//! The shared prompt-delivery path (HANDOFF-COMMON §4), used by the ActionRouter and later P8/P13.
//! The caller must explicitly submit. Raw writes/paste never call this function.

use chorus_core::actions::{ActionCapability, PromptSource, capabilities, clean_prompt, prompt_blocker, prompt_write};

use super::SessionManager;
use crate::store::delivery::{self, Delivered, DeliveryStatus};

/// Delivers once, scoped to the run the caller reviewed. A request id is retained across retries.
/// The extra run/request identity extends the shared contract with restart and retry protection.
pub fn deliver_prompt(manager: &SessionManager, session_id: u32, run_id: u32, request_id: &str, text: &str, source: PromptSource) -> Result<Delivered, String> {
    if !chorus_core::agent::is_uuid(request_id) { return Err("A delivery request needs a UUID.".into()); }
    let text = clean_prompt(text)?;
    let live = manager.open_live(session_id)?;
    // Serialises structured delivery with Close/Restart/Lock; a stale view can't target a new run.
    let _action = live.action.lock().expect("action lock");
    let row = manager.row(session_id)?;
    manager.refuse_if_locked(&row, "send to")?;
    let run = live.current_run().ok_or("Session has no live run. Restart it first.")?;
    let recorder = run.recorder.lock().expect("recorder lock");
    let scrubbed = recorder.clean(&text);
    if let Some(receipt) = delivery::existing(&manager.store, session_id, request_id, &scrubbed, source)? {
        return Ok(receipt);
    }
    if run.id != run_id { return Err("The session restarted. Review the current run before sending.".into()); }
    if let ActionCapability::Unavailable { reason } = capabilities(row.agent).prompt { return Err(reason); }
    let is_live = run.exit.lock().expect("exit lock").is_none()
        && run.requested.lock().expect("requested lock").is_none()
        && !manager.quitting.load(std::sync::atomic::Ordering::SeqCst);
    if let Some(reason) = prompt_blocker(false, is_live, recorder.activity(), recorder.pending_question(), delivery::unobserved(&manager.store, run_id)?) {
        return Err(reason.into());
    }
    let mut receipt = delivery::begin(&manager.store, session_id, run_id, request_id, &scrubbed, source, crate::store::now_ms())?;
    // Keep the writer across both writes so Raw keystrokes cannot interleave the text and Enter.
    // Keep the recorder too: an immediate hook can't race persistence or matching registration.
    let result = {
        use std::io::Write;
        let mut writer = run.writer.lock().expect("writer lock");
        writer.write_all(prompt_write(&text).as_bytes()).and_then(|()| writer.flush())
            .and_then(|()| writer.write_all(b"\r")).and_then(|()| writer.flush())
    };
    if result.is_ok() && delivery::written(&manager.store, receipt.prompt_id).is_ok() {
        receipt.status = DeliveryStatus::Written;
    }
    // Failure may be after a partial write. Return the durable uncertain receipt, never retry.
    drop(recorder);
    drop(_action);
    manager.notify_id(session_id);
    Ok(receipt)
}
