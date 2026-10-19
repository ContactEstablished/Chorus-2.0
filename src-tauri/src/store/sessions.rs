//! Sessions and runs (ARCHITECTURE §4, §7). A session is long-lived; each launch is a run.

use chorus_core::session::{RunEnd, SessionState};
use rusqlite::{OptionalExtension, Row, params};

use super::{Store, now_ms};
use crate::agents::AgentKind;

#[derive(Debug, Clone)]
pub struct SessionRow {
    pub id: u32,
    pub name: String,
    pub agent: AgentKind,
    pub cwd: String,
    pub state: SessionState,
    pub credential_id: Option<u32>,
    pub credential_env: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub locked: bool,
    pub note: Option<String>,
    pub created_at: i64,
}

pub struct NewSession<'a> {
    pub name: &'a str,
    pub agent: AgentKind,
    pub cwd: &'a str,
    pub credential_id: Option<u32>,
    pub credential_env: Option<&'a str>,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone)]
pub struct RunRow {
    #[cfg_attr(not(test), allow(dead_code, reason = "read by tests"))]
    pub id: u32,
    #[cfg_attr(not(test), allow(dead_code, reason = "read by tests"))]
    pub pid: Option<u32>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub exit_code: Option<u32>,
    pub end_reason: Option<RunEnd>,
}

const SESSION_COLUMNS: &str =
    "id, name, agent, cwd, state, credential_id, credential_env, cols, rows, locked, note, created_at";

fn session_from(r: &Row<'_>) -> rusqlite::Result<SessionRow> {
    let bad = |i: usize, what: &str, v: String| {
        rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, format!("unknown {what} {v:?}").into())
    };
    let agent: String = r.get(2)?;
    let state: String = r.get(4)?;
    Ok(SessionRow {
        id: r.get(0)?,
        name: r.get(1)?,
        agent: AgentKind::parse(&agent).ok_or_else(|| bad(2, "agent", agent.clone()))?,
        cwd: r.get(3)?,
        state: SessionState::parse(&state).ok_or_else(|| bad(4, "state", state.clone()))?,
        credential_id: r.get(5)?,
        credential_env: r.get(6)?,
        cols: r.get(7)?,
        rows: r.get(8)?,
        locked: r.get(9)?,
        note: r.get(10)?,
        created_at: r.get(11)?,
    })
}

pub fn insert(store: &Store, s: &NewSession<'_>) -> Result<u32, String> {
    store.with(|c| {
        c.execute(
            "INSERT INTO sessions (name, agent, cwd, state, credential_id, credential_env, cols, rows, created_at)
             VALUES (?1, ?2, ?3, 'running', ?4, ?5, ?6, ?7, ?8)",
            params![s.name, s.agent.as_str(), s.cwd, s.credential_id, s.credential_env, s.cols, s.rows, now_ms()],
        )?;
        Ok(c.last_insert_rowid() as u32)
    })
}

/// How many sessions of this agent have ever been created (for default names like "Shell 3").
pub fn count_by_agent(store: &Store, agent: AgentKind) -> Result<u32, String> {
    store.with(|c| c.query_row("SELECT COUNT(*) FROM sessions WHERE agent = ?1", [agent.as_str()], |r| r.get(0)))
}

pub fn get(store: &Store, id: u32) -> Result<Option<SessionRow>, String> {
    store.with(|c| c.query_row(&format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE id = ?1"), [id], session_from).optional())
}

/// Every session that isn't closed, oldest first.
pub fn list_open(store: &Store) -> Result<Vec<SessionRow>, String> {
    store.with(|c| {
        let mut stmt = c.prepare(&format!("SELECT {SESSION_COLUMNS} FROM sessions WHERE state != 'closed' ORDER BY id"))?;
        stmt.query_map([], session_from)?.collect()
    })
}

pub fn set_state(store: &Store, id: u32, state: SessionState, note: Option<&str>) -> Result<(), String> {
    store.with(|c| {
        let closed_at = (state == SessionState::Closed).then(now_ms);
        c.execute(
            "UPDATE sessions SET state = ?1, note = ?2, closed_at = COALESCE(?3, closed_at) WHERE id = ?4",
            params![state.as_str(), note, closed_at, id],
        )
        .map(|_| ())
    })
}

pub fn set_note(store: &Store, id: u32, note: Option<&str>) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE sessions SET note = ?1 WHERE id = ?2", params![note, id]).map(|_| ()))
}

pub fn set_size(store: &Store, id: u32, cols: u16, rows: u16) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE sessions SET cols = ?1, rows = ?2 WHERE id = ?3", params![cols, rows, id]).map(|_| ()))
}

pub fn set_locked(store: &Store, id: u32, locked: bool) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE sessions SET locked = ?1 WHERE id = ?2", params![locked, id]).map(|_| ()))
}

pub fn start_run(store: &Store, session_id: u32, pid: Option<u32>, program: &str, args: &[String]) -> Result<u32, String> {
    let args = serde_json::to_string(args).map_err(|e| e.to_string())?;
    store.with(|c| {
        c.execute(
            "INSERT INTO runs (session_id, pid, program, args, started_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, pid, program, args, now_ms()],
        )?;
        Ok(c.last_insert_rowid() as u32)
    })
}

/// Records how a run ended. A run already ended keeps its first answer (so a Close that is still
/// tearing down when Chorus quits is not relabelled).
pub fn end_run(store: &Store, run_id: u32, reason: RunEnd, exit_code: Option<u32>) -> Result<bool, String> {
    store.with(|c| {
        c.execute(
            "UPDATE runs SET ended_at = ?1, end_reason = ?2, exit_code = ?3 WHERE id = ?4 AND ended_at IS NULL",
            params![now_ms(), reason.as_str(), exit_code, run_id],
        )
        .map(|n| n > 0)
    })
}

/// Runs with no recorded end.
pub fn open_runs(store: &Store) -> Result<Vec<u32>, String> {
    store.with(|c| {
        let mut stmt = c.prepare("SELECT id FROM runs WHERE ended_at IS NULL ORDER BY id")?;
        stmt.query_map([], |r| r.get(0))?.collect()
    })
}

pub fn last_run(store: &Store, session_id: u32) -> Result<Option<RunRow>, String> {
    store.with(|c| {
        c.query_row(
            "SELECT id, pid, started_at, ended_at, exit_code, end_reason FROM runs
             WHERE session_id = ?1 ORDER BY id DESC LIMIT 1",
            [session_id],
            |r| {
                let reason: Option<String> = r.get(5)?;
                Ok(RunRow {
                    id: r.get(0)?,
                    pid: r.get(1)?,
                    started_at: r.get(2)?,
                    ended_at: r.get(3)?,
                    exit_code: r.get(4)?,
                    end_reason: reason.as_deref().and_then(RunEnd::parse),
                })
            },
        )
        .optional()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new<'a>() -> NewSession<'a> {
        NewSession {
            name: "Shell 1",
            agent: AgentKind::Shell,
            cwd: "C:\\work",
            credential_id: None,
            credential_env: None,
            cols: 120,
            rows: 30,
        }
    }

    #[test]
    fn session_lifecycle() {
        let store = Store::in_memory();
        let id = insert(&store, &new()).unwrap();
        let row = get(&store, id).unwrap().unwrap();
        assert_eq!((row.state, row.agent, row.cols, row.locked), (SessionState::Running, AgentKind::Shell, 120, false));
        set_size(&store, id, 90, 20).unwrap();
        set_locked(&store, id, true).unwrap();
        set_state(&store, id, SessionState::Exited, Some("claude.exe not found")).unwrap();
        let row = get(&store, id).unwrap().unwrap();
        assert_eq!((row.cols, row.rows, row.locked, row.state), (90, 20, true, SessionState::Exited));
        assert_eq!(row.note.as_deref(), Some("claude.exe not found"));
        assert_eq!(list_open(&store).unwrap().len(), 1);
        set_state(&store, id, SessionState::Closed, None).unwrap();
        assert!(list_open(&store).unwrap().is_empty());
    }

    #[test]
    fn a_run_ends_once() {
        let store = Store::in_memory();
        let sid = insert(&store, &new()).unwrap();
        let run = start_run(&store, sid, Some(42), "cmd.exe", &["/K".into()]).unwrap();
        assert_eq!(open_runs(&store).unwrap(), vec![run]);
        assert!(end_run(&store, run, RunEnd::Closed, Some(1)).unwrap());
        assert!(!end_run(&store, run, RunEnd::ChorusClosed, None).unwrap());
        let last = last_run(&store, sid).unwrap().unwrap();
        assert_eq!((last.end_reason, last.exit_code, last.pid), (Some(RunEnd::Closed), Some(1), Some(42)));
        assert!(open_runs(&store).unwrap().is_empty());
    }
}
