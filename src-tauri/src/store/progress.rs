//! Durable structured progress. The recorder scrubs decoded strings before these writes.

use chorus_core::progress::{PlanChange, PlanStep, apply_plan};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use specta::Type;
use super::{Store, now_ms};

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanSnapshot {
    pub id: u32,
    pub run_id: u32,
    pub turn_id: Option<u32>,
    pub steps: Vec<PlanStep>,
    pub incomplete: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProgressMessage {
    pub id: u32,
    pub run_id: u32,
    pub turn_id: Option<u32>,
    pub text: String,
}

pub fn record_plan(store: &Store, session: u32, run: u32, turn: Option<i64>, event_ref: &str, change: PlanChange) -> Result<(), String> {
    store.with(|c| {
        let tx = c.unchecked_transaction()?;
        let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM plan_snapshots WHERE run_id = ?1 AND event_ref = ?2)", params![run, event_ref], |r| r.get(0))?;
        if exists { return Ok(()); }
        let previous: Option<String> = tx.query_row("SELECT steps FROM plan_snapshots WHERE run_id = ?1 ORDER BY id DESC LIMIT 1", [run], |r| r.get(0)).optional()?;
        let steps: Vec<PlanStep> = previous.map(|s| serde_json::from_str(&s)).transpose().map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?.unwrap_or_default();
        let next = apply_plan(&steps, change).map_err(|_| rusqlite::Error::InvalidQuery)?.unwrap_or(steps);
        let steps = serde_json::to_string(&next).map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        tx.execute("INSERT INTO plan_snapshots (session_id, run_id, turn_id, event_ref, steps, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![session, run, turn, event_ref, steps, now_ms()])?;
        tx.commit()
    })
}

pub fn latest_plan(store: &Store, session: u32) -> Result<Option<PlanSnapshot>, String> {
    store.with(|c| c.query_row("SELECT id, run_id, turn_id, steps, EXISTS(SELECT 1 FROM plan_capture_issues i WHERE i.run_id = plan_snapshots.run_id) FROM plan_snapshots WHERE session_id = ?1 ORDER BY id DESC LIMIT 1", [session], |r| {
        let json: String = r.get(3)?;
        let steps = serde_json::from_str(&json).map_err(|e| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e)))?;
        Ok(PlanSnapshot { id: r.get(0)?, run_id: r.get(1)?, turn_id: r.get(2)?, steps, incomplete: r.get(4)? })
    }).optional())
}

pub fn mark_plan_incomplete(store: &Store, run: u32) -> Result<(), String> {
    store.with(|c| c.execute("INSERT INTO plan_capture_issues (run_id) VALUES (?1) ON CONFLICT(run_id) DO NOTHING", [run]).map(|_| ()))
}

pub fn record_message(store: &Store, session: u32, run: u32, turn: Option<i64>, reference: &str, text: &str) -> Result<(), String> {
    store.with(|c| c.execute("INSERT INTO progress_messages (session_id, run_id, turn_id, message_ref, text, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(session_id, message_ref) DO NOTHING", params![session, run, turn, reference, text, now_ms()]).map(|_| ()))
}

pub fn recent_messages(store: &Store, session: u32, limit: u32) -> Result<Vec<ProgressMessage>, String> {
    store.with(|c| {
        let mut q = c.prepare("SELECT id, run_id, turn_id, text FROM progress_messages WHERE session_id = ?1 ORDER BY id DESC LIMIT ?2")?;
        let mut out = q.query_map(params![session, limit.clamp(1, 200)], |r| Ok(ProgressMessage { id: r.get(0)?, run_id: r.get(1)?, turn_id: r.get(2)?, text: r.get(3)? }))?.collect::<rusqlite::Result<Vec<_>>>()?;
        out.reverse(); Ok(out)
    })
}
