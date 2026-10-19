//! Sessions and runs (ARCHITECTURE §4, §7). A session is long-lived; each launch is a run.

use chorus_core::agent::{AgentKind, LaunchOptions};
use chorus_core::session::{RunEnd, SessionState};
use rusqlite::{OptionalExtension, Row, params};

use super::{Store, now_ms};

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
    /// Resolved: defaults filled in, checked against the agent's declaration.
    pub options: LaunchOptions,
    pub agent_session_id: Option<String>,
    /// The provider the agent runs on (`store::providers`), if not its own login or key.
    pub provider_id: Option<u32>,
}

pub struct NewSession<'a> {
    pub name: &'a str,
    pub agent: AgentKind,
    pub cwd: &'a str,
    pub credential_id: Option<u32>,
    pub credential_env: Option<&'a str>,
    pub cols: u16,
    pub rows: u16,
    pub options: &'a LaunchOptions,
    pub agent_session_id: Option<&'a str>,
    pub provider_id: Option<u32>,
}

pub struct NewRun<'a> {
    pub session_id: u32,
    pub pid: Option<u32>,
    pub program: &'a str,
    pub args: &'a [String],
    pub cli_version: Option<&'a str>,
    pub resumed: bool,
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
    pub cli_version: Option<String>,
    pub resumed: bool,
}

const SESSION_COLUMNS: &str = "id, name, agent, cwd, state, credential_id, credential_env, cols, rows, locked, note, \
                               created_at, options, agent_session_id, provider_id";

fn session_from(r: &Row<'_>) -> rusqlite::Result<SessionRow> {
    let bad = |i: usize, what: &str, v: String| {
        rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, format!("unknown {what} {v:?}").into())
    };
    let agent: String = r.get(2)?;
    let state: String = r.get(4)?;
    let options: String = r.get(12)?;
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
        options: serde_json::from_str(&options).map_err(|_| bad(12, "options", options.clone()))?,
        agent_session_id: r.get(13)?,
        provider_id: r.get(14)?,
    })
}

pub fn insert(store: &Store, s: &NewSession<'_>) -> Result<u32, String> {
    let options = serde_json::to_string(s.options).map_err(|e| e.to_string())?;
    store.with(|c| {
        c.execute(
            "INSERT INTO sessions (name, agent, cwd, state, credential_id, credential_env, cols, rows, created_at,
                                   options, agent_session_id, provider_id)
             VALUES (?1, ?2, ?3, 'running', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                s.name,
                s.agent.as_str(),
                s.cwd,
                s.credential_id,
                s.credential_env,
                s.cols,
                s.rows,
                now_ms(),
                options,
                s.agent_session_id,
                s.provider_id
            ],
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

/// Records the agent's conversation id once it is known (Codex names it after launch).
pub fn set_agent_session_id(store: &Store, id: u32, agent_session_id: &str) -> Result<(), String> {
    store.with(|c| {
        c.execute("UPDATE sessions SET agent_session_id = ?1 WHERE id = ?2", params![agent_session_id, id]).map(|_| ())
    })
}

pub fn start_run(store: &Store, run: &NewRun<'_>) -> Result<u32, String> {
    let args = serde_json::to_string(run.args).map_err(|e| e.to_string())?;
    store.with(|c| {
        c.execute(
            "INSERT INTO runs (session_id, pid, program, args, started_at, cli_version, resumed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![run.session_id, run.pid, run.program, args, now_ms(), run.cli_version, run.resumed],
        )?;
        Ok(c.last_insert_rowid() as u32)
    })
}

/// Fills in the CLI version on open runs of `agent` that started before the version was detected
/// (relaunches at start-up don't wait for detection).
pub fn fill_cli_version(store: &Store, agent: AgentKind, version: &str) -> Result<(), String> {
    store.with(|c| {
        c.execute(
            "UPDATE runs SET cli_version = ?1 WHERE cli_version IS NULL AND ended_at IS NULL
               AND session_id IN (SELECT id FROM sessions WHERE agent = ?2)",
            params![version, agent.as_str()],
        )
        .map(|_| ())
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
            "SELECT id, pid, started_at, ended_at, exit_code, end_reason, cli_version, resumed FROM runs
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
                    cli_version: r.get(6)?,
                    resumed: r.get(7)?,
                })
            },
        )
        .optional()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NO_OPTIONS: LaunchOptions = LaunchOptions { effort: None, permission: None, model: None };

    fn new<'a>() -> NewSession<'a> {
        NewSession {
            name: "Shell 1",
            agent: AgentKind::Shell,
            cwd: "C:\\work",
            credential_id: None,
            credential_env: None,
            cols: 120,
            rows: 30,
            options: &NO_OPTIONS,
            agent_session_id: None,
            provider_id: None,
        }
    }

    fn run<'a>(session_id: u32) -> NewRun<'a> {
        NewRun { session_id, pid: Some(42), program: "cmd.exe", args: &[], cli_version: None, resumed: false }
    }

    #[test]
    fn options_and_the_conversation_id_are_kept() {
        use chorus_core::agent::{Effort, Permission};
        let store = Store::in_memory();
        let options = LaunchOptions { effort: Some(Effort::Deep), permission: Some(Permission::Plan), model: Some("opus".into()) };
        let id = insert(&store, &NewSession { agent: AgentKind::Claude, options: &options, ..new() }).unwrap();
        assert_eq!(get(&store, id).unwrap().unwrap().options, options);
        set_agent_session_id(&store, id, "0f8fad5b-d9cb-469f-a165-70867728950e").unwrap();
        let row = get(&store, id).unwrap().unwrap();
        assert_eq!(row.agent_session_id.as_deref(), Some("0f8fad5b-d9cb-469f-a165-70867728950e"));
        start_run(&store, &NewRun { cli_version: Some("2.1.300"), resumed: true, ..run(id) }).unwrap();
        let last = last_run(&store, id).unwrap().unwrap();
        assert_eq!((last.cli_version.as_deref(), last.resumed), (Some("2.1.300"), true));
    }

    #[test]
    fn session_lifecycle() {
        let store = Store::in_memory();
        let id = insert(&store, &new()).unwrap();
        let row = get(&store, id).unwrap().unwrap();
        assert_eq!((row.state, row.agent, row.cols, row.locked), (SessionState::Running, AgentKind::Shell, 120, false));
        assert_eq!(row.options, NO_OPTIONS);
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
        let run = start_run(&store, &run(sid)).unwrap();
        assert_eq!(open_runs(&store).unwrap(), vec![run]);
        assert!(end_run(&store, run, RunEnd::Closed, Some(1)).unwrap());
        assert!(!end_run(&store, run, RunEnd::ChorusClosed, None).unwrap());
        let last = last_run(&store, sid).unwrap().unwrap();
        assert_eq!((last.end_reason, last.exit_code, last.pid), (Some(RunEnd::Closed), Some(1), Some(42)));
        assert!(open_runs(&store).unwrap().is_empty());
    }
}
