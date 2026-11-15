//! The stored inputs of the Progress panel (P16): plan snapshots, runs and tool calls, each bounded.
//! `chorus_core::step_times` turns them into rows; nothing here decides what a row means.
use chorus_core::progress::{PlanStep, Progress};
use chorus_core::step_times::{Call, PlanSnapshot, RunSpan, SKILL_TOOL};
use rusqlite::{OptionalExtension, params};

use super::Store;

/// A plan's stored body starts with its tag (`Progress` is internally tagged, so serde writes it first).
const PLAN_PREFIX: &str = r#"{"kind":"plan"%"#;
/// A turn stops storing new progress keys at this many rows (`history::progress`).
pub const ROWS_PER_TURN: u32 = 500;

fn steps(body: &str) -> Option<Vec<PlanStep>> {
    match serde_json::from_str::<Progress>(body).ok()? { Progress::Plan { steps } => Some(steps), _ => None }
}

/// The newest `limit` plan snapshots of a session, oldest first, with each one's run and turn.
pub fn snapshots(store: &Store, session_id: u32, limit: u32) -> Result<(Vec<PlanSnapshot>, Option<u32>), String> {
    store.with(|c| {
        let mut stmt = c.prepare(
            "SELECT tp.at, tp.body, t.run_id, t.id FROM turn_progress tp JOIN turns t ON t.id = tp.turn_id
             WHERE t.session_id = ?1 AND tp.body LIKE ?2 ORDER BY tp.at DESC, tp.id DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![session_id, PLAN_PREFIX, limit], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<u32>>(2)?, r.get::<_, u32>(3)?))
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        let newest_turn = rows.first().map(|r| r.3);
        let mut out: Vec<PlanSnapshot> = rows.into_iter()
            .filter_map(|(at_ms, body, run_id, _)| steps(&body).map(|steps| PlanSnapshot { at_ms, run_id, steps }))
            .collect();
        out.reverse();
        Ok((out, newest_turn))
    })
}

/// Run ends that are stamped at the next launch, not when the run really stopped.
const LATE_ENDS: [&str; 2] = ["chorus-crashed", "system-shutdown"];

/// Every run of a session, oldest first. `ended_ms` is when the run stopped counting: its exit, or,
/// for a run Chorus only found ended at its next launch, the last thing Chorus recorded in it, so time
/// while Chorus was down never counts (K4).
pub fn runs(store: &Store, session_id: u32) -> Result<Vec<RunSpan>, String> {
    store.with(|c| {
        c.prepare(
            "SELECT r.id, r.started_at, r.ended_at, r.end_reason, (SELECT MAX(x) FROM (
                 SELECT MAX(t.started_at) AS x FROM turns t WHERE t.session_id = ?1 AND t.run_id = r.id
                 UNION ALL SELECT MAX(t.ended_at) FROM turns t WHERE t.session_id = ?1 AND t.run_id = r.id AND t.end_reason IS NOT 'lost'
                 UNION ALL SELECT MAX(tp.at) FROM turn_progress tp JOIN turns t ON t.id = tp.turn_id WHERE t.session_id = ?1 AND t.run_id = r.id
                 UNION ALL SELECT MAX(COALESCE(c.ended_at, c.started_at)) FROM tool_calls c JOIN turns t ON t.id = c.turn_id WHERE t.session_id = ?1 AND t.run_id = r.id))
             FROM runs r WHERE r.session_id = ?1 ORDER BY r.started_at, r.id",
        )?
        .query_map([session_id], |r| {
            let (id, started_ms): (u32, i64) = (r.get(0)?, r.get(1)?);
            let (ended, reason, seen): (Option<i64>, Option<String>, Option<i64>) = (r.get(2)?, r.get(3)?, r.get(4)?);
            let late = reason.as_deref().is_some_and(|x| LATE_ENDS.contains(&x));
            let ended_ms = if late { Some(seen.unwrap_or(started_ms).max(started_ms)) } else { ended };
            Ok(RunSpan { id, started_ms, ended_ms })
        })?
        .collect()
    })
}

fn call(r: &rusqlite::Row<'_>) -> rusqlite::Result<Call> {
    Ok(Call {
        id: r.get(0)?, turn_id: r.get(1)?, run_id: r.get(2)?, turn_ended_ms: r.get(3)?, tool: r.get(4)?,
        target: r.get(5)?, started_ms: r.get(6)?, ended_ms: r.get(7)?, failed: r.get(8)?,
    })
}

const CALL_COLUMNS: &str = "c.id, c.turn_id, t.run_id, t.ended_at, c.tool, c.target, c.started_at, c.ended_at, c.failed";

/// The newest `limit` `Skill` calls of a session, oldest first.
pub fn skill_calls(store: &Store, session_id: u32, limit: u32) -> Result<Vec<Call>, String> {
    store.with(|c| {
        let mut out = c.prepare(&format!(
            "SELECT {CALL_COLUMNS} FROM tool_calls c JOIN turns t ON t.id = c.turn_id
             WHERE t.session_id = ?1 AND c.tool = ?2 ORDER BY c.started_at DESC, c.id DESC LIMIT ?3"
        ))?.query_map(params![session_id, SKILL_TOOL, limit], call)?.collect::<rusqlite::Result<Vec<_>>>()?;
        out.reverse();
        Ok(out)
    })
}

/// The calls a session started in `[from, to)`; `to` `None`: until now. At most `limit`.
pub fn calls_between(store: &Store, session_id: u32, from_ms: i64, to_ms: Option<i64>, limit: u32) -> Result<Vec<Call>, String> {
    store.with(|c| {
        c.prepare(&format!(
            "SELECT {CALL_COLUMNS} FROM tool_calls c JOIN turns t ON t.id = c.turn_id
             WHERE t.session_id = ?1 AND (t.ended_at IS NULL OR t.ended_at >= ?2)
               AND c.started_at >= ?2 AND (?3 IS NULL OR c.started_at < ?3) ORDER BY c.started_at, c.id LIMIT ?4"
        ))?.query_map(params![session_id, from_ms, to_ms, limit], call)?.collect()
    })
}

/// Whether a turn reached the row cap, so later plan updates in it weren't stored.
pub fn capped(store: &Store, turn_id: u32) -> Result<bool, String> {
    store.with(|c| c.query_row(
        "SELECT EXISTS (SELECT 1 FROM turn_progress WHERE turn_id = ?1 LIMIT 1 OFFSET ?2)",
        params![turn_id, ROWS_PER_TURN - 1], |r| r.get(0),
    ))
}

/// The newest turn of a session, if any.
pub fn newest_turn(store: &Store, session_id: u32) -> Result<Option<u32>, String> {
    store.with(|c| c.query_row("SELECT id FROM turns WHERE session_id = ?1 ORDER BY id DESC LIMIT 1", [session_id], |r| r.get(0)).optional())
}

/// The newest plan in the session's newest 20 turns, to seed a resumed run's recorder.
pub fn latest_plan(store: &Store, session_id: u32) -> Result<Option<Vec<PlanStep>>, String> {
    let body: Option<String> = store.with(|c| c.query_row(
        "SELECT body FROM turn_progress WHERE turn_id IN
           (SELECT id FROM turns WHERE session_id = ?1 ORDER BY id DESC LIMIT 20)
         AND body LIKE ?2 ORDER BY at DESC, id DESC LIMIT 1",
        params![session_id, PLAN_PREFIX], |r| r.get(0),
    ).optional())?;
    Ok(body.as_deref().and_then(steps))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::history;
    use crate::store::sessions::{self as rows, NewSession};
    use chorus_core::agent::{AgentKind, LaunchOptions};

    fn session(s: &Store) -> u32 {
        rows::insert(s, &NewSession {
            name: "t", agent: AgentKind::Claude, cwd: "C:\\w", credential_id: None, credential_env: None, cols: 80, rows: 24,
            options: &LaunchOptions::default(), agent_session_id: None, provider_id: None, description: None,
            project_id: None, worktree_id: None, routing: None,
        }).unwrap()
    }
    fn plan(text: &str, status: &str) -> Progress {
        Progress::Plan { steps: vec![PlanStep { id: "1".into(), text: text.into(), status: status.into() }] }
    }

    #[test]
    fn plan_rows_are_found_by_their_serialised_prefix() {
        assert!(serde_json::to_string(&plan("A", "pending")).unwrap().starts_with(r#"{"kind":"plan""#));
        let s = Store::in_memory();
        let id = session(&s);
        let turn = history::open_turn(&s, id, None, None, None).unwrap();
        history::progress(&s, turn, "k1", &plan("A", "in-progress"), 1_000).unwrap();
        history::progress(&s, turn, "m", &Progress::Message { text: "hi".into() }, 2_000).unwrap();
        history::progress(&s, turn, "k2", &plan("A", "completed"), 3_000).unwrap();
        let (snaps, newest) = snapshots(&s, id, 10).unwrap();
        assert_eq!(snaps.iter().map(|s| s.at_ms).collect::<Vec<_>>(), [1_000, 3_000]);
        assert_eq!(newest, u32::try_from(turn).ok());
        assert_eq!(snapshots(&s, id, 1).unwrap().0[0].at_ms, 3_000);
    }

    #[test]
    fn latest_plan_is_the_newest_snapshot() {
        let s = Store::in_memory();
        let id = session(&s);
        assert_eq!(latest_plan(&s, id).unwrap(), None);
        let turn = history::open_turn(&s, id, None, None, None).unwrap();
        history::progress(&s, turn, "a", &plan("Old", "completed"), 1_000).unwrap();
        history::progress(&s, turn, "b", &plan("New", "in-progress"), 2_000).unwrap();
        assert_eq!(latest_plan(&s, id).unwrap().unwrap()[0].text, "New");
    }

    #[test]
    fn the_500_row_cap_is_reported() {
        let s = Store::in_memory();
        let id = session(&s);
        let turn = history::open_turn(&s, id, None, None, None).unwrap();
        let t = u32::try_from(turn).unwrap();
        for i in 0..ROWS_PER_TURN - 1 { history::progress(&s, turn, &format!("k{i}"), &plan("A", "pending"), i64::from(i)).unwrap(); }
        assert!(!capped(&s, t).unwrap());
        history::progress(&s, turn, "last", &plan("A", "pending"), 10_000).unwrap();
        assert!(capped(&s, t).unwrap());
    }

    #[test]
    fn a_crashed_run_ends_at_the_last_thing_recorded_in_it() {
        let s = Store::in_memory();
        let id = session(&s);
        s.with(|c| c.execute_batch(&format!(
            "INSERT INTO runs(id, session_id, program, args, started_at, ended_at, end_reason) VALUES (1, {id}, 'x', '[]', 1000, 90000, 'chorus-crashed'), (2, {id}, 'x', '[]', 90000, 95000, 'closed');
             INSERT INTO turns(id, session_id, run_id, started_at, ended_at, end_reason) VALUES (1, {id}, 1, 2000, 90000, 'lost');
             INSERT INTO tool_calls(session_id, turn_id, tool, started_at) VALUES ({id}, 1, 'Bash', 7000);
             INSERT INTO turn_progress(turn_id, source_key, body, at) VALUES (1, 'k', '{{\"kind\":\"plan\",\"steps\":[]}}', 5000);"
        ))).unwrap();
        let r = runs(&s, id).unwrap();
        assert_eq!(r[0].ended_ms, Some(7000), "the Bash call was the last thing seen, not the next launch");
        assert_eq!(r[1].ended_ms, Some(95000));
    }

    #[test]
    fn skill_calls_and_windows_are_read_with_their_turns() {
        let s = Store::in_memory();
        let id = session(&s);
        let turn = history::open_turn(&s, id, None, None, None).unwrap();
        s.with(|c| c.execute(
            "INSERT INTO tool_calls(session_id, turn_id, tool, target, started_at, ended_at) VALUES (?1, ?2, 'Skill', 'grilling', 1000, 1100), (?1, ?2, 'Read', 'a.rs', 2000, 2100), (?1, ?2, 'Bash', 'x', 5000, NULL)",
            params![id, turn],
        )).unwrap();
        let skills = skill_calls(&s, id, 10).unwrap();
        assert_eq!((skills.len(), skills[0].target.as_deref()), (1, Some("grilling")));
        let window = calls_between(&s, id, 1500, Some(4000), 10).unwrap();
        assert_eq!(window.iter().map(|c| c.tool.as_str()).collect::<Vec<_>>(), ["Read"]);
        assert_eq!(calls_between(&s, id, 1500, None, 10).unwrap().len(), 2);
    }
}
