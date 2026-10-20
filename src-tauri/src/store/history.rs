//! Durable history (migration 0005, D7). Callers pass text already scrubbed; this module only
//! writes and reads rows.

use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use specta::Type;

use super::{Store, now_ms};

pub fn insert_prompt(store: &Store, session_id: u32, run_id: Option<u32>, source: &str, text: &str) -> Result<i64, String> {
    store.with(|c| {
        c.execute(
            "INSERT INTO prompts (session_id, run_id, source, text, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, run_id, source, text, now_ms()],
        )?;
        Ok(c.last_insert_rowid())
    })
}

pub fn open_turn(store: &Store, session_id: u32, run_id: Option<u32>, agent_turn: Option<&str>, prompt_id: Option<i64>) -> Result<i64, String> {
    store.with(|c| {
        c.execute(
            "INSERT INTO turns (session_id, run_id, agent_turn, prompt_id, started_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, run_id, agent_turn, prompt_id, now_ms()],
        )?;
        Ok(c.last_insert_rowid())
    })
}

pub fn close_turn(store: &Store, turn_id: i64, end: &str, final_message: Option<&str>) -> Result<(), String> {
    store.with(|c| {
        c.execute(
            "UPDATE turns SET ended_at = ?2, end_reason = ?3, final_message = COALESCE(?4, final_message)
             WHERE id = ?1 AND ended_at IS NULL",
            params![turn_id, now_ms(), end, final_message],
        )
        .map(|_| ())
    })
}

pub fn tool_started(store: &Store, session_id: u32, turn_id: Option<i64>, call_ref: Option<&str>, tool: &str, target: Option<&str>) -> Result<i64, String> {
    store.with(|c| {
        c.execute(
            "INSERT INTO tool_calls (session_id, turn_id, call_ref, tool, target, started_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![session_id, turn_id, call_ref, tool, target, now_ms()],
        )?;
        Ok(c.last_insert_rowid())
    })
}

/// A call ends; its target is filled in if the start didn't have it (opencode sends the input
/// after the first event).
pub fn tool_finished(store: &Store, tool_call_id: i64, failed: bool, target: Option<&str>) -> Result<(), String> {
    store.with(|c| {
        c.execute(
            "UPDATE tool_calls SET ended_at = ?2, failed = ?3, target = COALESCE(target, ?4) WHERE id = ?1",
            params![tool_call_id, now_ms(), failed, target],
        )
        .map(|_| ())
    })
}

pub fn open_ask(store: &Store, session_id: u32, turn_id: Option<i64>, kind: &str, ask_ref: Option<&str>, subject: &str) -> Result<i64, String> {
    store.with(|c| {
        c.execute(
            "INSERT INTO asks (session_id, turn_id, kind, ask_ref, subject, asked_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![session_id, turn_id, kind, ask_ref, subject, now_ms()],
        )?;
        Ok(c.last_insert_rowid())
    })
}

pub fn answer_ask(store: &Store, ask_id: i64, answer: &str) -> Result<(), String> {
    store.with(|c| {
        c.execute("UPDATE asks SET answered_at = ?2, answer = ?3 WHERE id = ?1 AND answered_at IS NULL", params![ask_id, now_ms(), answer])
            .map(|_| ())
    })
}

pub fn record_event(store: &Store, session_id: u32, run_id: Option<u32>, source: &str, name: Option<&str>, body: &str) -> Result<(), String> {
    store.with(|c| {
        c.execute(
            "INSERT INTO agent_events (session_id, run_id, at, source, name, body) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![session_id, run_id, now_ms(), source, name, body],
        )
        .map(|_| ())
    })
}

/// Where a run's full transcript is, as its agent reported it.
pub fn set_run_transcript(store: &Store, run_id: u32, path: &str) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE runs SET transcript_path = ?2 WHERE id = ?1", params![run_id, path]).map(|_| ()))
}

/// The newest transcript path a session's runs recorded.
pub fn session_transcript(store: &Store, session_id: u32) -> Result<Option<String>, String> {
    store.with(|c| {
        c.query_row(
            "SELECT transcript_path FROM runs WHERE session_id = ?1 AND transcript_path IS NOT NULL ORDER BY id DESC LIMIT 1",
            [session_id],
            |r| r.get(0),
        )
        .optional()
    })
}

/// Drops raw events older than `older_than_ms`; the history rows stay.
pub fn prune_events(store: &Store, older_than_ms: i64) -> Result<usize, String> {
    store.with(|c| c.execute("DELETE FROM agent_events WHERE at < ?1", [now_ms() - older_than_ms]))
}

/// Turns a run left open (Chorus closed or crashed mid-turn) are lost.
pub fn close_open_turns(store: &Store, session_id: u32) -> Result<usize, String> {
    store.with(|c| {
        c.execute(
            "UPDATE turns SET ended_at = ?2, end_reason = 'lost' WHERE session_id = ?1 AND ended_at IS NULL",
            params![session_id, now_ms()],
        )
    })
}

/// A turn as the session's history shows it.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TurnEntry {
    pub id: u32,
    pub prompt: Option<String>,
    pub prompt_source: Option<String>,
    pub started_at_secs: u32,
    pub ended_at_secs: Option<u32>,
    pub end_reason: Option<String>,
    pub final_message: Option<String>,
    pub tools: Vec<ToolEntry>,
    pub asks: Vec<AskEntry>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ToolEntry {
    pub tool: String,
    pub target: Option<String>,
    pub failed: bool,
    pub finished: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AskEntry {
    pub kind: String,
    pub subject: String,
    pub answer: Option<String>,
}

fn secs(ms: i64) -> u32 {
    u32::try_from(ms / 1000).unwrap_or(0)
}

/// The newest `limit` turns of a session, newest last, each with its tool calls and asks.
pub fn recent_turns(store: &Store, session_id: u32, limit: u32) -> Result<Vec<TurnEntry>, String> {
    store.with(|c| {
        let mut stmt = c.prepare(
            "SELECT t.id, p.text, p.source, t.started_at, t.ended_at, t.end_reason, t.final_message
             FROM turns t LEFT JOIN prompts p ON p.id = t.prompt_id
             WHERE t.session_id = ?1 ORDER BY t.id DESC LIMIT ?2",
        )?;
        let mut turns: Vec<TurnEntry> = stmt
            .query_map(params![session_id, limit], |r| {
                Ok(TurnEntry {
                    id: r.get(0)?,
                    prompt: r.get(1)?,
                    prompt_source: r.get(2)?,
                    started_at_secs: secs(r.get(3)?),
                    ended_at_secs: r.get::<_, Option<i64>>(4)?.map(secs),
                    end_reason: r.get(5)?,
                    final_message: r.get(6)?,
                    tools: Vec::new(),
                    asks: Vec::new(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        turns.reverse();
        let mut tools = c.prepare("SELECT tool, target, failed, ended_at IS NOT NULL FROM tool_calls WHERE turn_id = ?1 ORDER BY id")?;
        let mut asks = c.prepare("SELECT kind, subject, answer FROM asks WHERE turn_id = ?1 ORDER BY id")?;
        for t in &mut turns {
            t.tools = tools
                .query_map([t.id], |r| Ok(ToolEntry { tool: r.get(0)?, target: r.get(1)?, failed: r.get(2)?, finished: r.get(3)? }))?
                .collect::<rusqlite::Result<_>>()?;
            t.asks = asks
                .query_map([t.id], |r| Ok(AskEntry { kind: r.get(0)?, subject: r.get(1)?, answer: r.get(2)? }))?
                .collect::<rusqlite::Result<_>>()?;
        }
        Ok(turns)
    })
}

/// A session's recent turns and how complete its records are.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionHistory {
    pub turns: Vec<TurnEntry>,
    /// All turns recorded for the session.
    pub total: u32,
    /// Turns with both a start and an end.
    pub ended: u32,
    /// Turns whose prompt was recorded.
    pub with_prompt: u32,
    /// Where the agent keeps the full transcript (Claude Code's JSONL, Codex's rollout).
    pub transcript_path: Option<String>,
}

/// How complete a session's turn records are: (turns, with a start and an end, with a prompt).
pub fn turn_counts(store: &Store, session_id: u32) -> Result<(u32, u32, u32), String> {
    store.with(|c| {
        c.query_row(
            "SELECT COUNT(*), COUNT(ended_at), COUNT(prompt_id) FROM turns WHERE session_id = ?1",
            [session_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
    })
}

/// What a session last said or was asked: its newest turn's final message, else that turn's
/// prompt, with when the turn started (ms). Shown on Overview cards.
pub fn latest(store: &Store, session_id: u32) -> Result<Option<(String, i64)>, String> {
    store.with(|c| {
        c.query_row(
            "SELECT COALESCE(t.final_message, p.text), t.started_at FROM turns t LEFT JOIN prompts p ON p.id = t.prompt_id
             WHERE t.session_id = ?1 AND COALESCE(t.final_message, p.text) IS NOT NULL ORDER BY t.id DESC LIMIT 1",
            [session_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
    })
}

/// The prompt text recorded for a turn, if any (tests and checks).
#[cfg_attr(not(test), allow(dead_code, reason = "used by tests"))]
pub fn prompt_of_turn(store: &Store, turn_id: i64) -> Result<Option<String>, String> {
    store.with(|c| {
        c.query_row("SELECT p.text FROM turns t JOIN prompts p ON p.id = t.prompt_id WHERE t.id = ?1", [turn_id], |r| r.get(0)).optional()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::sessions::{self as rows, NewSession};
    use chorus_core::agent::{AgentKind, LaunchOptions};

    fn session(s: &Store) -> u32 {
        rows::insert(
            s,
            &NewSession {
                name: "t",
                agent: AgentKind::Claude,
                cwd: "C:\\w",
                credential_id: None,
                credential_env: None,
                cols: 80,
                rows: 24,
                options: &LaunchOptions::default(),
                agent_session_id: None,
                provider_id: None,
                description: None,
                project_id: None,
            },
        )
        .unwrap()
    }

    #[test]
    fn a_turn_keeps_its_prompt_tools_asks_and_message() {
        let s = Store::in_memory();
        let id = session(&s);
        let prompt = insert_prompt(&s, id, None, "hook", "make a.txt").unwrap();
        let turn = open_turn(&s, id, None, Some("p1"), Some(prompt)).unwrap();
        let call = tool_started(&s, id, Some(turn), Some("t1"), "Write", Some("a.txt")).unwrap();
        let ask = open_ask(&s, id, Some(turn), "approval", None, "Write a.txt").unwrap();
        answer_ask(&s, ask, "allowed").unwrap();
        tool_finished(&s, call, false, None).unwrap();
        close_turn(&s, turn, "done", Some("Done.")).unwrap();
        close_turn(&s, turn, "lost", None).unwrap();

        let turns = recent_turns(&s, id, 10).unwrap();
        assert_eq!(turns.len(), 1);
        let t = &turns[0];
        assert_eq!((t.prompt.as_deref(), t.end_reason.as_deref(), t.final_message.as_deref()), (Some("make a.txt"), Some("done"), Some("Done.")));
        assert_eq!((t.tools.len(), t.tools[0].finished, t.asks[0].answer.as_deref()), (1, true, Some("allowed")));
        assert_eq!(turn_counts(&s, id).unwrap(), (1, 1, 1));
        assert_eq!(prompt_of_turn(&s, turn).unwrap().as_deref(), Some("make a.txt"));
    }

    #[test]
    fn open_turns_are_lost_and_old_events_pruned() {
        let s = Store::in_memory();
        let id = session(&s);
        open_turn(&s, id, None, None, None).unwrap();
        assert_eq!(close_open_turns(&s, id).unwrap(), 1);
        assert_eq!(recent_turns(&s, id, 5).unwrap()[0].end_reason.as_deref(), Some("lost"));
        record_event(&s, id, None, "claude-hook", Some("Stop"), "{}").unwrap();
        assert_eq!(prune_events(&s, -1_000).unwrap(), 1);
    }
}
