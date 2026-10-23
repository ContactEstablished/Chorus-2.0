//! The shared prompt-delivery path (HANDOFF-COMMON §4), used by the ActionRouter and later P8/P13.
//! The caller must explicitly submit. Raw writes/paste never call this function.

use chorus_core::actions::{ActionCapability, PromptSource, capabilities, clean_prompt, prompt_blocker, prompt_write};

use super::SessionManager;
use crate::store::delivery::{self, Delivered, DeliveryStatus};

// ConPTY can coalesce adjacent writes despite flush(). The observed Claude TUI needs
// a separate input batch for Enter; 200 ms matches the successful isolated runtime probe.
const SUBMIT_GAP: std::time::Duration = std::time::Duration::from_millis(200);

fn write_prompt(writer: &mut dyn std::io::Write, text: &str) -> std::io::Result<()> {
    writer.write_all(prompt_write(text).as_bytes())?;
    writer.flush()?;
    std::thread::sleep(SUBMIT_GAP);
    writer.write_all(b"\r")?;
    writer.flush()
}

/// Delivers once, scoped to the run the caller reviewed. A request id is retained across retries.
/// The extra run/request identity extends the shared contract with restart and retry protection.
pub fn deliver_prompt(manager: &SessionManager, session_id: u32, run_id: u32, request_id: &str, text: &str, source: PromptSource) -> Result<Delivered, String> {
    if !chorus_core::agent::is_uuid(request_id) { return Err("A delivery request needs a UUID.".into()); }
    let text = clean_prompt(text)?;
    let live = manager.open_live(session_id)?;
    // Serialises structured delivery with Close/Restart/Lock; a stale view can't target a new run.
    let _action = live.action.lock().expect("action lock");
    let result = deliver_locked(manager, session_id, run_id, request_id, &text, source, &live);
    drop(_action);
    manager.notify_id(session_id);
    result
}

/// Caller holds the live action lock, also used by Raw, cancellation and restart.
#[allow(clippy::too_many_arguments)]
pub(super) fn deliver_locked(manager: &SessionManager, session_id: u32, run_id: u32, request_id: &str, text: &str, source: PromptSource, live: &super::Live) -> Result<Delivered, String> {
    let row = manager.row(session_id)?;
    manager.refuse_if_locked(&row, "send to")?;
    let run = live.current_run().ok_or("Session has no live run. Restart it first.")?;
    let recorder = run.recorder.lock().expect("recorder lock");
    let scrubbed = recorder.clean(text);
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
    if let Some(reason) = chorus_core::queue::blocker(run_id, run.id, recorder.activity(), run.raw_empty.load(std::sync::atomic::Ordering::SeqCst)) { return Err(reason.into()); }
    crate::store::queue::validate_delivery(&manager.store, session_id, run_id, request_id, &scrubbed, source)?;
    let mut receipt = delivery::begin(&manager.store, session_id, run_id, request_id, &scrubbed, source, crate::store::now_ms())?;
    // Keep the writer across both writes so Raw keystrokes cannot interleave the text and Enter.
    // Keep the recorder too: an immediate hook can't race persistence or matching registration.
    let result = {
        let mut writer = run.writer.lock().expect("writer lock");
        write_prompt(&mut **writer, text)
    };
    if result.is_ok() && delivery::written(&manager.store, receipt.prompt_id).is_ok() {
        receipt.status = DeliveryStatus::Written;
    }
    // Failure may be after a partial write. Return the durable uncertain receipt, never retry.
    drop(recorder);
    Ok(receipt)
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::Instant;

    #[derive(Default)]
    struct InputBatches {
        chunks: Vec<Vec<u8>>,
        text_flushed_at: Option<Instant>,
        fail_text_flush: bool,
    }

    impl Write for InputBatches {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes == b"\r" {
                let flushed = self.text_flushed_at.expect("text must flush before Enter");
                assert!(flushed.elapsed() >= SUBMIT_GAP, "Enter must not share the text input batch");
            }
            self.chunks.push(bytes.to_vec());
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            if self.fail_text_flush { return Err(std::io::Error::other("injected text flush failure")); }
            if self.text_flushed_at.is_none() { self.text_flushed_at = Some(Instant::now()); }
            Ok(())
        }
    }

    #[test]
    fn enter_is_delayed_after_the_text_flush_without_combining_writes() {
        let mut writer = InputBatches::default();
        write_prompt(&mut writer, "test instruction").unwrap();
        assert_eq!(writer.chunks, [b"test instruction".to_vec(), b"\r".to_vec()]);
    }

    #[test]
    fn a_failed_text_flush_never_attempts_enter() {
        let mut writer = InputBatches { fail_text_flush: true, ..Default::default() };
        assert!(write_prompt(&mut writer, "partial instruction").is_err());
        assert_eq!(writer.chunks, [b"partial instruction".to_vec()]);
    }
}
