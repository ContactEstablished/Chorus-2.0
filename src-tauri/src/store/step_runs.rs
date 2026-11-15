//! Finished, timed runs of named steps (migration 0027, D121, K8), for "usual time" markers. The
//! recorder writes a turn's skill estimates when the turn closes, through the same rule the panel
//! shows (`step_times::skill_estimates`); the panel reads up to 20 earlier runs per skill name.
use chorus_core::activity::TurnEnd;
use chorus_core::step_times::{self, EndedBy, MAX_SKILL_CALLS, MAX_SNAPSHOTS, SKILL_TOOL};
use rusqlite::params;

use super::Store;

pub struct StepRun<'a> {
    pub kind: &'a str,
    pub key: &'a str,
    pub session_id: u32,
    pub turn_id: Option<u32>,
    pub source_ref: String,
    pub started_ms: i64,
    pub ended_ms: i64,
    pub estimate: bool,
}

/// Writes one run once: a second write with the same source is ignored. True when written.
pub fn insert(store: &Store, run: &StepRun) -> Result<bool, String> {
    store.with(|c| c.execute(
        "INSERT OR IGNORE INTO step_runs (key_kind, key, session_id, turn_id, source_ref, started_at, ended_at, duration_ms, estimate)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![run.kind, run.key, run.session_id, run.turn_id, run.source_ref, run.started_ms, run.ended_ms,
            (run.ended_ms - run.started_ms).max(0), run.estimate],
    )).map(|n| n > 0)
}

/// The newest `limit` runs of one key that ended at or before `before_ms`, newest first: what was
/// usual when this run started, never this run itself.
pub fn past(store: &Store, kind: &str, key: &str, before_ms: i64, limit: u32) -> Result<Vec<i64>, String> {
    store.with(|c| c.prepare(
        "SELECT duration_ms FROM step_runs WHERE key_kind = ?1 AND key = ?2 AND ended_at <= ?3
         ORDER BY ended_at DESC, id DESC LIMIT ?4",
    )?.query_map(params![kind, key, before_ms, limit], |r| r.get(0))?.collect())
}

/// Whether a run from this source finished cleanly and was stored (the UNIQUE index answers it).
pub fn stored(store: &Store, kind: &str, source_ref: &str) -> Result<bool, String> {
    store.with(|c| c.query_row(
        "SELECT EXISTS (SELECT 1 FROM step_runs WHERE key_kind = ?1 AND source_ref = ?2)", params![kind, source_ref], |r| r.get(0),
    ))
}

/// Stores the named skill estimates a closed turn finished cleanly: ended by the next step or the
/// next skill, or by the turn itself only when it was done. A run's end, an interrupt, a failure or
/// a lost turn cut the skill off, so its time says nothing about how long it usually takes.
/// Runs under the recorder's lock, so a turn without a `Skill` call returns after one indexed read.
pub fn persist_turn(store: &Store, session_id: u32, run_id: u32, turn_id: i64, end: TurnEnd) -> Result<u32, String> {
    let any: bool = store.with(|c| c.query_row(
        "SELECT EXISTS (SELECT 1 FROM tool_calls WHERE turn_id = ?1 AND tool = ?2)", params![turn_id, SKILL_TOOL], |r| r.get(0),
    ))?;
    if !any { return Ok(0); }
    let (snapshots, _) = super::progress::snapshots(store, session_id, MAX_SNAPSHOTS)?;
    let runs = super::progress::runs(store, session_id)?;
    let calls = super::progress::skill_calls(store, session_id, MAX_SKILL_CALLS)?;
    let turn = u32::try_from(turn_id).ok();
    let mut written = 0;
    for e in step_times::skill_estimates(&snapshots, &runs, Some(run_id), &calls) {
        let clean = match e.ended_by {
            Some(EndedBy::StepStart | EndedBy::NextSkill) => true,
            Some(EndedBy::TurnEnd) => end == TurnEnd::Done,
            Some(EndedBy::RunEnd) | None => false,
        };
        let (Some(name), Some(ended_ms)) = (e.name.as_deref(), e.ended_ms) else { continue };
        if e.turn_id != turn || !clean || name.trim().is_empty() { continue; }
        let run = StepRun {
            kind: "skill", key: name, session_id, turn_id: e.turn_id, source_ref: e.call_id.to_string(),
            started_ms: e.started_ms, ended_ms, estimate: true,
        };
        if insert(store, &run)? { written += 1; }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::sessions::{self as rows, NewSession};
    use chorus_core::agent::{AgentKind, LaunchOptions};

    fn session(s: &Store) -> u32 {
        rows::insert(s, &NewSession {
            name: "t", agent: AgentKind::Claude, cwd: "C:\\w", credential_id: None, credential_env: None, cols: 80, rows: 24,
            options: &LaunchOptions::default(), agent_session_id: None, provider_id: None, description: None,
            project_id: None, worktree_id: None, routing: None,
        }).unwrap()
    }
    fn run<'a>(id: u32, kind: &'a str, key: &'a str, source: &str, from: i64, to: i64) -> StepRun<'a> {
        StepRun { kind, key, session_id: id, turn_id: None, source_ref: source.into(), started_ms: from, ended_ms: to, estimate: true }
    }

    #[test]
    fn a_run_is_written_once_per_call() {
        let s = Store::in_memory();
        let id = session(&s);
        assert!(insert(&s, &run(id, "skill", "pdf", "7", 1_000, 4_000)).unwrap());
        assert!(!insert(&s, &run(id, "skill", "pdf", "7", 1_000, 9_000)).unwrap(), "the same call again is ignored");
        assert!(insert(&s, &run(id, "chorus", "pdf", "7", 1_000, 2_000)).unwrap(), "another kind is another run");
        assert_eq!(past(&s, "skill", "pdf", i64::MAX, 20).unwrap(), [3_000]);
        assert!(stored(&s, "skill", "7").unwrap());
        assert!(!stored(&s, "skill", "8").unwrap());
        assert!(s.with(|c| c.execute("INSERT INTO step_runs (key_kind, key, session_id, source_ref, started_at, ended_at, duration_ms, estimate) VALUES ('step', 'x', ?1, 'y', 0, 1, 1, 1)", [id])).is_err(), "free-text steps have no kind");
    }

    #[test]
    fn past_runs_are_those_that_ended_before() {
        let s = Store::in_memory();
        let id = session(&s);
        for (i, (from, to)) in [(0, 10_000), (20_000, 25_000), (30_000, 60_000), (70_000, 71_000)].into_iter().enumerate() {
            insert(&s, &run(id, "skill", "arch", &i.to_string(), from, to)).unwrap();
        }
        insert(&s, &run(id, "skill", "other", "x", 0, 1_000)).unwrap();
        // Newest first, only those ended by 60 s, bounded by the limit.
        assert_eq!(past(&s, "skill", "arch", 60_000, 20).unwrap(), [30_000, 5_000, 10_000]);
        assert_eq!(past(&s, "skill", "arch", 60_000, 2).unwrap(), [30_000, 5_000]);
        assert_eq!(past(&s, "skill", "arch", 9_999, 20).unwrap(), Vec::<i64>::new());
    }
}
