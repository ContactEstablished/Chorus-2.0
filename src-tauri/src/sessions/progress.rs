//! The Progress panel's read (P16): the session's plan with step times, skill chips and the current
//! step's tool calls. Every rule lives in `chorus_core::step_times`; this only gathers bounded inputs.
use chorus_core::step_times::{self, MAX_SKILL_CALLS, MAX_SNAPSHOTS, MAX_WINDOW_CALLS, SessionProgress};

use crate::store::progress as rows;

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
        Ok(progress)
    }
}
