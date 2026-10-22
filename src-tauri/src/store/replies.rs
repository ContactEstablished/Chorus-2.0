//! A reply attempt is durable before the HTTP response. It is not proof the agent took it.
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use specta::Type;
use super::{Store, now_ms};

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReplyReceipt { pub ask_id: u32, pub run_id: u32 }

pub fn existing(store: &Store, session: u32, run: u32, ask: u32, request: &str, action: &str) -> Result<Option<ReplyReceipt>, String> {
    let found: Option<(u32, u32, String)> = store.with(|c| c.query_row(
        "SELECT run_id, ask_id, action FROM reply_attempts WHERE session_id = ?1 AND request_id = ?2",
        params![session, request], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).optional())?;
    match found {
        Some((r, a, text)) if r == run && a == ask && text == action => Ok(Some(ReplyReceipt { ask_id: a, run_id: r })),
        Some(_) => Err("That request ID already belongs to a different reply.".into()),
        None => Ok(None),
    }
}

pub fn begin(store: &Store, session: u32, run: u32, ask: u32, request: &str, action: &str) -> Result<ReplyReceipt, String> {
    store.with(|c| c.execute(
        "INSERT INTO reply_attempts(session_id, run_id, ask_id, request_id, action, attempted_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![session, run, ask, request, action, now_ms()],
    ))?;
    Ok(ReplyReceipt { ask_id: ask, run_id: run })
}
