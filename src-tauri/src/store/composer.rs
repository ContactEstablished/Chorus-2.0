//! Durable drafts and FIFO instructions. Claiming an instruction shares delivery's transaction.
use super::Store;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use specta::Type;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub text: String,
    pub revision: u32,
    pub images: Vec<super::images::ComposerImage>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QueuedPrompt {
    pub id: u32,
    pub run_id: u32,
    pub request_id: String,
    pub text: String,
    pub status: String,
    pub reason: Option<String>,
    pub prompt_id: Option<u32>,
    pub images: Vec<super::images::ComposerImage>,
}

pub fn draft(store: &Store, session: u32) -> Result<Draft, String> {
    let (text, revision) = store.with(|c| c.query_row("SELECT text,revision FROM composer_drafts WHERE session_id=?1", [session], |r| Ok((r.get(0)?,r.get(1)?))).optional())?.unwrap_or((String::new(),0));
    Ok(Draft { text, revision, images: super::images::list(store, session, None)? })
}

pub fn save(store: &Store, session: u32, revision: u32, text: &str) -> Result<Draft, String> {
    let changed = store.with(|c| {
        let tx = c.unchecked_transaction()?;
        tx.execute("INSERT OR IGNORE INTO composer_drafts(session_id) VALUES (?1)", [session])?;
        let n = tx.execute(
            "UPDATE composer_drafts SET text=?3,revision=revision+1 WHERE session_id=?1 AND revision=?2",
            params![session, revision, text],
        )?;
        tx.commit()?;
        Ok(n)
    })?;
    if changed == 0 {
        return Err("The draft changed in another view. Reload the saved draft before editing again.".into());
    }
    draft(store, session)
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<QueuedPrompt> {
    Ok(QueuedPrompt {
        id: r.get(0)?,
        run_id: r.get(1)?,
        request_id: r.get(2)?,
        text: r.get(3)?,
        status: r.get(4)?,
        reason: r.get(5)?,
        prompt_id: r.get(6)?,
        images: Vec::new(),
    })
}
const SELECT: &str = "SELECT q.id,q.run_id,q.request_id,q.text,CASE WHEN q.status='attempted' THEN COALESCE(d.status,'uncertain') ELSE q.status END,q.reason,q.prompt_id FROM prompt_queue q LEFT JOIN prompt_deliveries d ON d.prompt_id=q.prompt_id";

pub fn list(store: &Store, session: u32) -> Result<Vec<QueuedPrompt>, String> {
    let mut items: Vec<QueuedPrompt> = store.with(|c| c.prepare(&format!("{SELECT} WHERE q.session_id=?1 AND (q.status IN ('queued','held') OR q.id IN (SELECT id FROM prompt_queue WHERE session_id=?1 ORDER BY id DESC LIMIT 100)) ORDER BY q.id DESC"))?.query_map([session], row)?.collect())?;
    for item in &mut items { item.images = super::images::list(store, session, Some(item.id))?; }
    Ok(items)
}
pub fn get(store: &Store, session: u32, id: u32) -> Result<QueuedPrompt, String> {
    let mut item = store.with(|c| c.query_row(&format!("{SELECT} WHERE q.session_id=?1 AND q.id=?2"), params![session, id], row))?;
    item.images = super::images::list(store, session, Some(id))?;
    Ok(item)
}
pub fn check_request(store: &Store, session: u32, request: &str, run: u32, revision: u32, queue: bool) -> Result<bool, String> {
    let old = store.with(|c| {
        c.query_row(
            "SELECT submitted_run_id,draft_revision,was_queued FROM prompt_queue WHERE session_id=?1 AND request_id=?2",
            params![session, request],
            |r| Ok((r.get::<_, u32>(0)?, r.get::<_, u32>(1)?, r.get::<_, bool>(2)?)),
        )
        .optional()
    })?;
    match old {
        Some(value) if value != (run, revision, queue) => Err("This request ID belongs to a different submission.".into()),
        Some(_) => Ok(true),
        None => Ok(false),
    }
}
#[allow(clippy::too_many_arguments)]
pub fn enqueue(store: &Store, session: u32, run: u32, request: &str, revision: u32, text: &str, queue: bool) -> Result<u32, String> {
    // All callers hold the session action lock, including cancellation and delivery.
    let count: u32 = store.with(|c| {
        c.query_row("SELECT count(*) FROM prompt_queue WHERE session_id=?1 AND status IN ('queued','held')", [session], |r| r.get(0))
    })?;
    if count >= chorus_core::composer::MAX_QUEUED {
        return Err("The queue is full (32 instructions). Cancel an instruction first.".into());
    }
    store.with(|c| {
        let tx = c.unchecked_transaction()?;
        let n = tx.execute("UPDATE composer_drafts SET text='',revision=revision+1 WHERE session_id=?1 AND revision=?2", params![session,revision])?;
        if n != 1 { return Err(rusqlite::Error::InvalidQuery); }
        tx.execute("INSERT INTO prompt_queue(session_id,run_id,request_id,text,status,created_at,submitted_run_id,draft_revision,was_queued) VALUES (?1,?2,?3,?4,'queued',?5,?2,?6,?7)", params![session,run,request,text,super::now_ms(),revision,queue])?;
        let id = tx.last_insert_rowid() as u32;
        tx.execute("UPDATE composer_images SET queue_id=?2 WHERE session_id=?1 AND queue_id IS NULL", params![session,id])?;
        tx.commit()?;
        Ok(id)
    })
}
pub fn oldest(store: &Store, session: u32) -> Result<Option<QueuedPrompt>, String> {
    let mut item = store.with(|c| c.query_row(&format!("{SELECT} WHERE q.session_id=?1 AND q.status IN ('queued','held') ORDER BY q.id LIMIT 1"), [session], row).optional())?;
    if let Some(q) = &mut item { q.images = super::images::list(store, session, Some(q.id))?; }
    Ok(item)
}
pub fn hold(store: &Store, session: u32, reason: &str) -> Result<(), String> {
    store.with(|c| {
        c.execute("UPDATE prompt_queue SET status='held',reason=?2 WHERE session_id=?1 AND status='queued'", params![session, reason])
            .map(|_| ())
    })
}
pub fn recover(store: &Store) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE prompt_queue SET status='held',reason='Chorus restarted. Review and resume this instruction for the current run.' WHERE status='queued'", []).map(|_| ()))
}
pub fn cancel(store: &Store, session: u32, id: u32) -> Result<(), String> {
    let n = store.with(|c| c.execute("UPDATE prompt_queue SET status='cancelled',reason=NULL WHERE session_id=?1 AND id=?2 AND status IN ('queued','held','cancelled')", params![session,id]))?;
    if n == 0 { Err("This instruction was already attempted and cannot be cancelled. Check Raw.".into()) } else { Ok(()) }
}
pub fn resume(store: &Store, session: u32, id: u32, run: u32) -> Result<(), String> {
    let n = store.with(|c| c.execute("UPDATE prompt_queue SET status='queued',reason=NULL,run_id=?3 WHERE session_id=?1 AND id=?2 AND status='held' AND prompt_id IS NULL", params![session,id,run]))?;
    if n == 0 { Err("Only a held, unsent instruction can be resumed.".into()) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> Store {
        let s = Store::in_memory();
        s.with(|c| c.execute_batch("INSERT INTO sessions(id,name,agent,cwd,state,cols,rows,created_at) VALUES(1,'test','codex','scratch','running',100,30,0); INSERT INTO runs(id,session_id,program,args,started_at) VALUES(1,1,'test','[]',0),(2,1,'test','[]',1);")).unwrap();
        s
    }
    #[test]
    fn draft_revision_prevents_stale_save_and_submit_is_atomic() {
        let s = setup();
        let d = save(&s, 1, 0, "first").unwrap();
        assert!(save(&s, 1, 0, "stale").is_err());
        assert!(enqueue(&s, 1, 1, "a", 0, "first", true).is_err());
        assert_eq!(draft(&s, 1).unwrap().text, "first");
        enqueue(&s, 1, 1, "a", d.revision, "first", true).unwrap();
        assert_eq!(draft(&s, 1).unwrap().text, "");
        assert!(enqueue(&s, 1, 1, "a", d.revision, "first", true).is_err());
        assert_eq!(list(&s, 1).unwrap().len(), 1);
    }
    #[test]
    fn recovery_keeps_order_and_requires_explicit_resume_and_cancel() {
        let s = setup();
        let d = save(&s, 1, 0, "one").unwrap();
        let a = enqueue(&s, 1, 1, "a", d.revision, "one", true).unwrap();
        let d = save(&s, 1, 2, "two").unwrap();
        let b = enqueue(&s, 1, 1, "b", d.revision, "two", true).unwrap();
        recover(&s).unwrap();
        assert_eq!(oldest(&s, 1).unwrap().unwrap().status, "held");
        resume(&s, 1, b, 2).unwrap();
        assert_eq!(oldest(&s, 1).unwrap().unwrap().id, a);
        cancel(&s, 1, a).unwrap();
        cancel(&s, 1, a).unwrap();
        assert_eq!(oldest(&s, 1).unwrap().unwrap().id, b);
        assert!(resume(&s, 1, a, 2).is_err());
    }
}
