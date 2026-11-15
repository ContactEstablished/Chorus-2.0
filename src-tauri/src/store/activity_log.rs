//! Each change of a session's activity, timed by Chorus's clock (migration 0026, D119). The recorder
//! writes a row where the tracker's activity changes, and an `unknown` row when a run starts;
//! `chorus_core::time_split` turns a run's rows into "Where the time went".
use chorus_core::activity::Activity;
use chorus_core::time_split::{MAX_MARKS, Mark};
use rusqlite::params;

use super::Store;

/// `guessed`: a NeedsAnswer that only came from the final message ending with `?`.
pub fn record(store: &Store, session_id: u32, run_id: u32, activity: Activity, guessed: bool, at: i64) -> Result<(), String> {
    store.with(|c| c.execute(
        "INSERT INTO activity_log (session_id, run_id, activity, guessed, at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![session_id, run_id, activity.as_str(), guessed, at],
    )).map(drop)
}

/// A run's changes in order, at most one more than [`MAX_MARKS`], so `split` can tell a run that
/// has too many. A row whose activity doesn't parse is skipped.
pub fn marks_for_run(store: &Store, run_id: u32) -> Result<Vec<Mark>, String> {
    store.with(|c| {
        let rows = c.prepare("SELECT activity, at, guessed FROM activity_log WHERE run_id = ?1 ORDER BY at, id LIMIT ?2")?
            .query_map(params![run_id, MAX_MARKS + 1], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, bool>(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows.into_iter().filter_map(|(a, at_ms, guessed)| Activity::parse(&a).map(|activity| Mark { at_ms, activity, guessed })).collect())
    })
}

pub fn turns_in_run(store: &Store, session_id: u32, run_id: u32) -> Result<u32, String> {
    store.with(|c| c.query_row(
        "SELECT count(*) FROM turns WHERE session_id = ?1 AND run_id = ?2", params![session_id, run_id], |r| r.get(0),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::sessions::{self as rows, NewRun, NewSession};
    use chorus_core::agent::{AgentKind, LaunchOptions};

    fn session(s: &Store) -> u32 {
        rows::insert(s, &NewSession {
            name: "t", agent: AgentKind::Claude, cwd: "C:\\w", credential_id: None, credential_env: None, cols: 80, rows: 24,
            options: &LaunchOptions::default(), agent_session_id: None, provider_id: None, description: None,
            project_id: None, worktree_id: None, routing: None,
        }).unwrap()
    }
    fn run(s: &Store, id: u32) -> u32 {
        rows::start_run(s, &NewRun { session_id: id, pid: None, program: "claude.exe", args: &[], cli_version: None, resumed: false }).unwrap()
    }

    #[test]
    fn marks_come_back_in_order_for_one_run() {
        let s = Store::in_memory();
        let id = session(&s);
        let (a, b) = (run(&s, id), run(&s, id));
        record(&s, id, a, Activity::Working, false, 300).unwrap();
        record(&s, id, a, Activity::Unknown, false, 100).unwrap();
        record(&s, id, b, Activity::Ready, false, 200).unwrap();
        record(&s, id, a, Activity::NeedsAnswer, true, 300).unwrap();
        s.with(|c| c.execute("INSERT INTO turns (session_id, run_id, started_at) VALUES (?1, ?2, 1)", params![id, a])).unwrap();
        let marks = marks_for_run(&s, a).unwrap();
        assert_eq!(
            marks.iter().map(|m| (m.at_ms, m.activity, m.guessed)).collect::<Vec<_>>(),
            [(100, Activity::Unknown, false), (300, Activity::Working, false), (300, Activity::NeedsAnswer, true)],
            "ordered by time, then by the order written"
        );
        assert_eq!((turns_in_run(&s, id, a).unwrap(), turns_in_run(&s, id, b).unwrap()), (1, 0));
        assert!(s.with(|c| c.execute("INSERT INTO activity_log (session_id, run_id, activity, at) VALUES (?1, ?2, 'busy', 1)", params![id, a])).is_err(), "unknown names are refused");
        assert!(record(&s, id, a, Activity::Ready, true, 400).is_err(), "only a question can be a guess");
    }
}
