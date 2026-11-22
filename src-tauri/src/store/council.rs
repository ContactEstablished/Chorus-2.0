//! Council members, runs, turns and tool calls (migration 0032).

use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use specta::Type;

use chorus_core::council::{TurnOutcome, Phase, CouncilRole};

use super::{Store, now_ms};

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MemberRow {
    pub id: u32,
    pub label: String,
    pub model: String,
    pub role: CouncilRole,
    pub max_tokens: Option<u32>,
}

fn member(r: &Row<'_>) -> rusqlite::Result<MemberRow> {
    let role: String = r.get(3)?;
    Ok(MemberRow {
        id: r.get(0)?,
        label: r.get(1)?,
        model: r.get(2)?,
        role: CouncilRole::parse(&role).unwrap_or(CouncilRole::Member),
        max_tokens: r.get(4)?,
    })
}

pub fn members(store: &Store) -> Result<Vec<MemberRow>, String> {
    store.with(|c| {
        let mut s = c.prepare("SELECT id, label, model, role, max_tokens FROM council_members ORDER BY role DESC, label")?;
        s.query_map([], member)?.collect()
    })
}

/// Inserts (no id) or updates a member. The label is unique without case.
pub fn save_member(store: &Store, id: Option<u32>, label: &str, model: &str, role: CouncilRole, max_tokens: Option<u32>) -> Result<u32, String> {
    let now = now_ms();
    let taken: Option<u32> = store.with(|c| {
        c.query_row("SELECT id FROM council_members WHERE label = ?1 COLLATE NOCASE", [label], |r| r.get(0)).optional()
    })?;
    if taken.is_some_and(|t| Some(t) != id) {
        return Err(format!("A council member named '{label}' already exists."));
    }
    match id {
        Some(id) => {
            let n = store.with(|c| {
                c.execute(
                    "UPDATE council_members SET label = ?2, model = ?3, role = ?4, max_tokens = ?5, updated_at = ?6 WHERE id = ?1",
                    params![id, label, model, role.key(), max_tokens, now],
                )
            })?;
            if n == 0 {
                return Err("That council member no longer exists.".into());
            }
            Ok(id)
        }
        None => store.with(|c| {
            c.execute(
                "INSERT INTO council_members (label, model, role, max_tokens, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![label, model, role.key(), max_tokens, now],
            )?;
            Ok(c.last_insert_rowid() as u32)
        }),
    }
}

pub fn delete_member(store: &Store, id: u32) -> Result<(), String> {
    store.with(|c| c.execute("DELETE FROM council_members WHERE id = ?1", [id]).map(|_| ()))
}

/// A participant as the run recorded it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RosterEntry {
    pub id: u32,
    pub label: String,
    pub model: String,
    pub role: CouncilRole,
    pub max_tokens: u32,
    /// The model's context length from the catalog, when known.
    pub context_length: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum RunStatus {
    Running,
    Complete,
    Failed,
    Cancelled,
    Abandoned,
}

impl RunStatus {
    fn key(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Complete => "complete",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Abandoned => "abandoned",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "complete" => Self::Complete,
            "cancelled" => Self::Cancelled,
            "abandoned" => Self::Abandoned,
            _ => Self::Failed,
        }
    }
}

pub struct NewRun<'a> {
    pub project_id: u32,
    pub project_folder: &'a str,
    pub brief_path: &'a str,
    pub brief_text: &'a str,
    pub roster: &'a [RosterEntry],
    pub limits_json: &'a str,
    pub provider_id: u32,
}

/// Opens a run, refusing a second while one is running (the single-flight guard, in the same
/// statement so two windows can't both pass a check).
pub fn open_run(store: &Store, r: &NewRun<'_>) -> Result<u32, String> {
    let roster = serde_json::to_string(r.roster).map_err(|e| e.to_string())?;
    let id: Option<u32> = store.with(|c| {
        c.query_row(
            "INSERT INTO council_runs (project_id, project_folder, brief_path, brief_text, roster_json, limits_json, status, provider_id, started_at)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, 'running', ?7, ?8 WHERE NOT EXISTS (SELECT 1 FROM council_runs WHERE status = 'running')
             RETURNING id",
            params![r.project_id, r.project_folder, r.brief_path, r.brief_text, roster, r.limits_json, r.provider_id, now_ms()],
            |row| row.get(0),
        )
        .optional()
    })?;
    id.ok_or_else(|| "A council is already running. Wait for it to finish, or cancel it.".to_owned())
}

pub fn set_key_ledger(store: &Store, run: u32, ledger: u32) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE council_runs SET key_ledger_id = ?2 WHERE id = ?1", params![run, ledger]).map(|_| ()))
}

pub fn set_summary(store: &Store, run: u32, summary_json: &str) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE council_runs SET summary_json = ?2 WHERE id = ?1", params![run, summary_json]).map(|_| ()))
}

pub fn set_findings(store: &Store, run: u32, path: Option<&str>, error: Option<&str>) -> Result<(), String> {
    store.with(|c| {
        c.execute("UPDATE council_runs SET findings_path = ?2, findings_error = ?3 WHERE id = ?1", params![run, path, error]).map(|_| ())
    })
}

pub fn finish_run(store: &Store, run: u32, status: RunStatus, reason: Option<&str>) -> Result<(), String> {
    store.with(|c| {
        c.execute(
            "UPDATE council_runs SET status = ?2, reason = ?3, ended_at = ?4 WHERE id = ?1 AND status = 'running'",
            params![run, status.key(), reason, now_ms()],
        )
        .map(|_| ())
    })
}

/// At start-up: a run still marked running was cut off when Chorus stopped. Its turns in flight
/// are closed too. (Its key is revoked by the start-up key sweep.)
pub fn heal(store: &Store) -> Result<(), String> {
    store.with(|c| {
        c.execute_batch(&format!(
            "UPDATE council_turns SET outcome = 'cancelled', content = COALESCE(content, 'Chorus stopped during this turn.'), ended_at = {now}
               WHERE outcome IS NULL;
             UPDATE council_runs SET status = 'abandoned', reason = 'Chorus stopped while this council was running.', ended_at = {now}
               WHERE status = 'running';",
            now = now_ms()
        ))
    })
}

pub fn delete_run(store: &Store, run: u32) -> Result<bool, String> {
    store.with(|c| c.execute("DELETE FROM council_runs WHERE id = ?1 AND status != 'running'", [run]).map(|n| n > 0))
}

#[derive(Debug, Clone)]
pub struct RunRow {
    pub id: u32,
    pub project_id: u32,
    pub project_folder: String,
    pub brief_path: String,
    pub brief_text: String,
    pub roster: Vec<RosterEntry>,
    pub limits_json: String,
    pub status: RunStatus,
    pub reason: Option<String>,
    pub summary_json: Option<String>,
    pub findings_path: Option<String>,
    pub findings_error: Option<String>,
    pub key_ledger_id: Option<u32>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

const RUN_COLUMNS: &str = "id, project_id, project_folder, brief_path, brief_text, roster_json, limits_json, status, reason, \
                           summary_json, findings_path, findings_error, key_ledger_id, started_at, ended_at";

fn run_row(r: &Row<'_>) -> rusqlite::Result<RunRow> {
    let roster: String = r.get(5)?;
    let status: String = r.get(7)?;
    Ok(RunRow {
        id: r.get(0)?,
        project_id: r.get(1)?,
        project_folder: r.get(2)?,
        brief_path: r.get(3)?,
        brief_text: r.get(4)?,
        roster: serde_json::from_str(&roster).unwrap_or_default(),
        limits_json: r.get(6)?,
        status: RunStatus::parse(&status),
        reason: r.get(8)?,
        summary_json: r.get(9)?,
        findings_path: r.get(10)?,
        findings_error: r.get(11)?,
        key_ledger_id: r.get(12)?,
        started_at: r.get(13)?,
        ended_at: r.get(14)?,
    })
}

pub fn run(store: &Store, id: u32) -> Result<Option<RunRow>, String> {
    store.with(|c| c.query_row(&format!("SELECT {RUN_COLUMNS} FROM council_runs WHERE id = ?1"), [id], run_row).optional())
}

pub fn runs_for_project(store: &Store, project_id: u32) -> Result<Vec<RunRow>, String> {
    store.with(|c| {
        let mut s = c.prepare(&format!("SELECT {RUN_COLUMNS} FROM council_runs WHERE project_id = ?1 ORDER BY started_at DESC, id DESC"))?;
        s.query_map([project_id], run_row)?.collect()
    })
}

pub fn running(store: &Store) -> Result<Option<u32>, String> {
    store.with(|c| c.query_row("SELECT id FROM council_runs WHERE status = 'running' LIMIT 1", [], |r| r.get(0)).optional())
}

/// Opens a turn with its (scrubbed) prompt, before the first request goes.
pub fn open_turn(store: &Store, run: u32, member: u32, phase: Phase, prompt: &str) -> Result<u32, String> {
    store.with(|c| {
        c.execute(
            "INSERT INTO council_turns (run_id, member_id, phase, prompt, started_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![run, member, phase.key(), prompt, now_ms()],
        )?;
        Ok(c.last_insert_rowid() as u32)
    })
}

#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
    pub tokens_cached: Option<u64>,
    pub cost_usd: Option<f64>,
}

impl Usage {
    /// Adds one response's usage; a field stays None until some response reports it.
    pub fn add(&mut self, other: &Usage) {
        fn sum<T: std::ops::Add<Output = T> + Copy>(a: Option<T>, b: Option<T>) -> Option<T> {
            match (a, b) {
                (Some(a), Some(b)) => Some(a + b),
                (a, b) => a.or(b),
            }
        }
        self.tokens_in = sum(self.tokens_in, other.tokens_in);
        self.tokens_out = sum(self.tokens_out, other.tokens_out);
        self.tokens_cached = sum(self.tokens_cached, other.tokens_cached);
        self.cost_usd = sum(self.cost_usd, other.cost_usd);
    }
}

pub fn finish_turn(store: &Store, turn: u32, outcome: TurnOutcome, content: &str, usage: &Usage) -> Result<(), String> {
    store.with(|c| {
        c.execute(
            "UPDATE council_turns SET outcome = ?2, content = ?3, tokens_in = ?4, tokens_out = ?5, tokens_cached = ?6, cost_usd = ?7, ended_at = ?8
             WHERE id = ?1",
            params![
                turn,
                outcome.key(),
                content,
                usage.tokens_in.map(|n| n as i64),
                usage.tokens_out.map(|n| n as i64),
                usage.tokens_cached.map(|n| n as i64),
                usage.cost_usd,
                now_ms()
            ],
        )
        .map(|_| ())
    })
}

pub fn add_tool_call(store: &Store, turn: u32, call: &str, ok: bool, bytes: usize, note: Option<&str>) -> Result<(), String> {
    store.with(|c| {
        c.execute(
            "INSERT INTO council_tool_calls (turn_id, call, ok, bytes, note, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![turn, call, ok, bytes as i64, note, now_ms()],
        )
        .map(|_| ())
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TurnRow {
    pub id: u32,
    pub member_id: u32,
    pub phase: Phase,
    /// None while in flight.
    pub outcome: Option<TurnOutcome>,
    pub content: Option<String>,
    pub tokens_in: Option<u32>,
    pub tokens_out: Option<u32>,
    pub cost_usd: Option<f64>,
    pub tool_calls: u32,
    pub tool_bytes: u32,
    pub started_at: f64,
    pub ended_at: Option<f64>,
}

pub fn turns(store: &Store, run: u32) -> Result<Vec<TurnRow>, String> {
    store.with(|c| {
        let mut s = c.prepare(
            "SELECT t.id, t.member_id, t.phase, t.outcome, t.content, t.tokens_in, t.tokens_out, t.cost_usd,
                    (SELECT COUNT(*) FROM council_tool_calls k WHERE k.turn_id = t.id),
                    (SELECT COALESCE(SUM(k.bytes), 0) FROM council_tool_calls k WHERE k.turn_id = t.id AND k.ok = 1),
                    t.started_at, t.ended_at
             FROM council_turns t WHERE t.run_id = ?1 ORDER BY t.id",
        )?;
        s.query_map([run], |r| {
            let phase: String = r.get(2)?;
            let outcome: Option<String> = r.get(3)?;
            Ok(TurnRow {
                id: r.get(0)?,
                member_id: r.get(1)?,
                phase: Phase::parse(&phase).unwrap_or(Phase::Positions),
                outcome: outcome.as_deref().and_then(TurnOutcome::parse),
                content: r.get(4)?,
                tokens_in: r.get(5)?,
                tokens_out: r.get(6)?,
                cost_usd: r.get(7)?,
                tool_calls: r.get(8)?,
                tool_bytes: r.get(9)?,
                started_at: r.get::<_, i64>(10)? as f64,
                ended_at: r.get::<_, Option<i64>>(11)?.map(|t| t as f64),
            })
        })?
        .collect()
    })
}

pub fn turn_prompt(store: &Store, turn: u32) -> Result<Option<(u32, String)>, String> {
    store.with(|c| c.query_row("SELECT run_id, prompt FROM council_turns WHERE id = ?1", [turn], |r| Ok((r.get(0)?, r.get(1)?))).optional())
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRow {
    pub call: String,
    pub ok: bool,
    pub bytes: u32,
    pub note: Option<String>,
}

pub fn tool_calls(store: &Store, turn: u32) -> Result<Vec<ToolCallRow>, String> {
    store.with(|c| {
        let mut s = c.prepare("SELECT call, ok, bytes, note FROM council_tool_calls WHERE turn_id = ?1 ORDER BY id")?;
        s.query_map([turn], |r| Ok(ToolCallRow { call: r.get(0)?, ok: r.get(1)?, bytes: r.get(2)?, note: r.get(3)? }))?.collect()
    })
}

/// The latest tool call of a turn still in flight, for the live view.
pub fn last_tool_call(store: &Store, turn: u32) -> Result<Option<String>, String> {
    store.with(|c| c.query_row("SELECT call FROM council_tool_calls WHERE turn_id = ?1 ORDER BY id DESC LIMIT 1", [turn], |r| r.get(0)).optional())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(store: &Store) -> u32 {
        store
            .with(|c| {
                c.execute("INSERT INTO projects (name, colour, state, position, created_at) VALUES ('P', 'teal', 'active', 0, 1)", [])?;
                Ok(c.last_insert_rowid() as u32)
            })
            .unwrap()
    }

    fn new_run<'a>(project_id: u32, roster: &'a [RosterEntry]) -> NewRun<'a> {
        NewRun { project_id, project_folder: "C:\\p", brief_path: "C:\\p\\b.md", brief_text: "b", roster, limits_json: "{}", provider_id: 1 }
    }

    #[test]
    fn members_are_unique_without_case() {
        let store = Store::in_memory();
        let a = save_member(&store, None, "Alpha", "x/y", CouncilRole::Member, None).unwrap();
        assert!(save_member(&store, None, "alpha", "x/z", CouncilRole::Member, None).unwrap_err().contains("already exists"));
        assert_eq!(save_member(&store, Some(a), "ALPHA", "x/z", CouncilRole::Arbiter, Some(4000)).unwrap(), a);
        let m = members(&store).unwrap();
        assert_eq!((m[0].label.as_str(), m[0].role, m[0].max_tokens), ("ALPHA", CouncilRole::Arbiter, Some(4000)));
    }

    #[test]
    fn one_run_at_a_time_and_heal() {
        let store = Store::in_memory();
        let p = project(&store);
        let r = open_run(&store, &new_run(p, &[])).unwrap();
        assert!(open_run(&store, &new_run(p, &[])).unwrap_err().contains("already running"));
        let t = open_turn(&store, r, 1, Phase::Positions, "prompt").unwrap();
        add_tool_call(&store, t, "read a.rs", true, 120, None).unwrap();
        assert!(!delete_run(&store, r).unwrap(), "a running run can't be removed");
        heal(&store).unwrap();
        let row = run(&store, r).unwrap().unwrap();
        assert_eq!(row.status, RunStatus::Abandoned);
        let turns = turns(&store, r).unwrap();
        assert_eq!((turns[0].outcome, turns[0].tool_calls, turns[0].tool_bytes), (Some(TurnOutcome::Cancelled), 1, 120));
        assert!(open_run(&store, &new_run(p, &[])).is_ok());
        assert!(delete_run(&store, r).unwrap());
        assert!(tool_calls(&store, t).unwrap().is_empty(), "turns and calls go with the run");
    }

    #[test]
    fn usage_sums_and_keeps_unknown_apart_from_zero() {
        let mut u = Usage::default();
        u.add(&Usage { tokens_in: Some(10), cost_usd: None, ..Default::default() });
        u.add(&Usage { tokens_in: Some(5), cost_usd: Some(0.01), ..Default::default() });
        assert_eq!((u.tokens_in, u.tokens_out, u.cost_usd), (Some(15), None, Some(0.01)));
    }
}
