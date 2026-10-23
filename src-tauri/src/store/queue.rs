//! Scrubbed FIFO instructions. Attempts share the prompt delivery transaction.
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use specta::Type;
use super::Store;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QueuedInstruction {
    pub id: u32,
    pub run_id: u32,
    pub request_id: String,
    pub text: String,
    pub status: String,
    pub delivery_status: Option<String>,
}
fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<QueuedInstruction> {
    Ok(QueuedInstruction { id: r.get(0)?, run_id: r.get(1)?, request_id: r.get(2)?, text: r.get(3)?, status: r.get(4)?, delivery_status: r.get(5)? })
}
pub fn list(store: &Store, session: u32) -> Result<Vec<QueuedInstruction>, String> {
    store.with(|c| c.prepare("SELECT q.id,q.run_id,q.request_id,q.text,q.status,d.status FROM instruction_queue q LEFT JOIN prompt_deliveries d ON d.prompt_id=q.prompt_id WHERE q.session_id=?1 ORDER BY (q.status='queued') DESC,q.id DESC LIMIT 150")?
        .query_map([session], row)?.collect())
}
pub fn first(store: &Store, session: u32) -> Result<Option<QueuedInstruction>, String> {
    store.with(|c| c.query_row("SELECT id,run_id,request_id,text,status,NULL FROM instruction_queue WHERE session_id=?1 AND status='queued' ORDER BY id LIMIT 1", [session], row).optional())
}
pub fn enqueue(store: &Store, session: u32, run: u32, request: &str, text: &str) -> Result<(), String> {
    let old = store.with(|c| c.query_row("SELECT text FROM instruction_queue WHERE session_id=?1 AND request_id=?2", params![session,request], |r| r.get::<_,String>(0)).optional())?;
    if let Some(old) = old { return if old == text { Ok(()) } else { Err("This queue request ID belongs to a different instruction.".into()) }; }
    let count: u32 = store.with(|c| c.query_row("SELECT count(*) FROM instruction_queue WHERE session_id=?1 AND status='queued'", [session], |r| r.get(0)))?;
    if count >= 50 { return Err("The queue contains 50 pending instructions. Cancel or deliver one first.".into()); }
    store.with(|c| c.execute("INSERT INTO instruction_queue(session_id,run_id,request_id,text,status,created_at) VALUES (?1,?2,?3,?4,'queued',?5)", params![session,run,request,text,super::now_ms()]).map(|_| ()))
}
pub fn change(store: &Store, session: u32, id: u32, rearm: Option<u32>) -> Result<(), String> {
    let changed = store.with(|c| if let Some(run) = rearm {
        c.execute("UPDATE instruction_queue SET run_id=?3 WHERE session_id=?1 AND id=?2 AND status='queued'",params![session,id,run])
    } else { c.execute("UPDATE instruction_queue SET status='cancelled' WHERE session_id=?1 AND id=?2 AND status='queued'",params![session,id]) })?;
    if changed != 1 { return Err("This instruction was already attempted, cancelled, or is unavailable. Refresh the queue.".into()); }
    Ok(())
}

/// A direct ActionRouter request cannot steal a queued/cancelled request's identity.
pub fn validate_delivery(store: &Store, session: u32, run: u32, request: &str, text: &str, source: chorus_core::actions::PromptSource) -> Result<(), String> {
    if first(store, session)?.is_some_and(|head| head.request_id != request) {
        return Err("An earlier queued instruction must be handled first. Add this instruction to the queue or cancel the pending instructions.".into());
    }
    let item = store.with(|c| c.query_row("SELECT id,run_id,request_id,text,status,NULL FROM instruction_queue WHERE session_id=?1 AND request_id=?2", params![session,request], row).optional())?;
    if let Some(item) = item {
        if item.status != "queued" || item.run_id != run || item.text != text || source != chorus_core::actions::PromptSource::Composer {
            return Err("This request belongs to a different, cancelled or attempted queued instruction.".into());
        }
        if first(store, session)?.is_none_or(|first| first.id != item.id) {
            return Err("An earlier queued instruction must be handled first.".into());
        }
    }
    Ok(())
}
