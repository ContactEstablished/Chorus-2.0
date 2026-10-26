//! Persist-before-write receipts and prompt echo matching. No automatic retry after uncertainty.

use chorus_core::actions::{PromptSource, echo_matches_within, echo_window};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::Store;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum DeliveryStatus { Uncertain, Written, Observed }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Delivered {
    pub prompt_id: u32,
    pub run_id: u32,
    pub status: DeliveryStatus,
}

fn receipt(row: &rusqlite::Row<'_>) -> rusqlite::Result<Delivered> {
    let status: String = row.get(2)?;
    Ok(Delivered { prompt_id: row.get(0)?, run_id: row.get(1)?, status: match status.as_str() {
        "written" => DeliveryStatus::Written,
        "observed" => DeliveryStatus::Observed,
        _ => DeliveryStatus::Uncertain,
    } })
}

/// Reusing a request id is a lookup, never another write. A different payload is an error.
pub fn existing(store: &Store, session: u32, request: &str, text: &str, source: PromptSource) -> Result<Option<Delivered>, String> {
    let row = store.with(|c| c.query_row(
        "SELECT d.prompt_id,d.run_id,d.status,p.text,p.source FROM prompt_deliveries d
         JOIN prompts p ON p.id=d.prompt_id WHERE d.session_id=?1 AND d.request_id=?2",
        params![session, request], |r| Ok((receipt(r)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?)),
    ).optional())?;
    match row {
        Some((receipt, old, origin)) if old == text && origin == source.as_str() => Ok(Some(receipt)),
        Some(_) => Err("This request id already belongs to a different instruction.".into()),
        None => Ok(None),
    }
}

pub fn begin(store: &Store, session: u32, run: u32, request: &str, text: &str, source: PromptSource, at: i64) -> Result<Delivered, String> {
    begin_with_queue(store, session, run, request, text, source, at, None)
}

#[allow(clippy::too_many_arguments)]
pub fn begin_with_queue(store: &Store, session: u32, run: u32, request: &str, text: &str, source: PromptSource, at: i64, queued: Option<u32>) -> Result<Delivered, String> {
    store.with(|c| {
        let tx = c.unchecked_transaction()?;
        tx.execute("INSERT INTO prompts (session_id,run_id,source,text,created_at) VALUES (?1,?2,?3,?4,?5)",
            params![session, run, source.as_str(), text, at])?;
        let prompt_id: u32 = tx.query_row("SELECT last_insert_rowid()", [], |r| r.get(0))?;
        tx.execute("INSERT INTO prompt_deliveries (prompt_id,session_id,run_id,request_id,status,attempted_at) VALUES (?1,?2,?3,?4,'uncertain',?5)",
            params![prompt_id, session, run, request, at])?;
        if let Some(id) = queued {
            let n = tx.execute("UPDATE prompt_queue SET status='attempted',prompt_id=?4,reason=NULL WHERE id=?1 AND session_id=?2 AND run_id=?3 AND status='queued'", params![id,session,run,prompt_id])?;
            if n != 1 { return Err(rusqlite::Error::InvalidQuery); }
        }
        tx.commit()?;
        Ok(Delivered { prompt_id, run_id: run, status: DeliveryStatus::Uncertain })
    })
}

pub fn written(store: &Store, prompt_id: u32) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE prompt_deliveries SET status='written' WHERE prompt_id=?1 AND status='uncertain'", [prompt_id]).map(|_| ()))
}

pub fn unobserved(store: &Store, run: u32) -> Result<bool, String> {
    store.with(|c| c.query_row("SELECT EXISTS(SELECT 1 FROM prompt_deliveries WHERE run_id=?1 AND status!='observed')", [run], |r| r.get(0)))
}

/// Called under the run recorder lock, once for each opened turn. Consume oldest matching echo.
pub fn observe(store: &Store, run: u32, text: &str, now: i64) -> Result<Option<i64>, String> {
    store.with(|c| {
        let mut stmt = c.prepare("SELECT d.prompt_id,p.text,d.attempted_at,p.source FROM prompt_deliveries d
            JOIN prompts p ON p.id=d.prompt_id WHERE d.run_id=?1 AND d.status!='observed' ORDER BY d.prompt_id")?;
        let rows = stmt.query_map([run], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, String>(3)?)))?;
        for row in rows {
            let (id, expected, at, source) = row?;
            if echo_matches_within(run, run, &expected, text, at, now, echo_window(&source)) {
                c.execute("UPDATE prompt_deliveries SET status='observed' WHERE prompt_id=?1", [id])?;
                return Ok(Some(id));
            }
        }
        Ok(None)
    })
}
