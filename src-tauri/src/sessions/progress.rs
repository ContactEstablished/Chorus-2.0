//! The Progress panel's read (P16): the session's plan with step times, skill chips, the current
//! step's tool calls and where the newest run's time went. Every rule lives in
//! `chorus_core::step_times` and `chorus_core::time_split`; this only gathers bounded inputs.
use chorus_core::step_times::{self, MAX_SKILL_CALLS, MAX_SNAPSHOTS, MAX_WINDOW_CALLS, SessionProgress};
use chorus_core::time_split;

use crate::store::{activity_log, progress as rows};

impl super::SessionManager {
    pub fn progress(&self, id: u32) -> Result<SessionProgress, String> {
        self.row(id)?;
        let live_run = self.live.lock().expect("live lock").get(&id).and_then(|l| l.current_run()).map(|r| r.id);
        let (snapshots, plan_turn) = rows::snapshots(&self.store, id, MAX_SNAPSHOTS)?;
        let runs = rows::runs(&self.store, id)?;
        let skills = rows::skill_calls(&self.store, id, MAX_SKILL_CALLS)?;
        let (mut progress, window) = step_times::compute(&snapshots, &runs, live_run, &skills);
        if let Some(w) = window {
            let calls = rows::calls_between(&self.store, id, w.from_ms, w.to_ms, MAX_WINDOW_CALLS)?;
            progress.current_tools = step_times::tool_groups(&calls, w.to_ms.is_none());
        }
        let newest = rows::newest_turn(&self.store, id)?;
        for turn in [plan_turn, newest].into_iter().flatten() {
            if rows::capped(&self.store, turn)? { progress.capped = true; }
        }
        // The newest run, with the end `runs` already corrected for a crash (L75), so time while
        // Chorus was down never counts as waiting.
        if let Some(run) = runs.last() {
            let marks = activity_log::marks_for_run(&self.store, run.id)?;
            let turns = activity_log::turns_in_run(&self.store, id, run.id)?;
            let live = live_run == Some(run.id);
            progress.split = Some(time_split::split(run.started_ms, run.ended_ms, live, &marks, crate::store::now_ms(), turns));
        }
        Ok(progress)
    }
}
