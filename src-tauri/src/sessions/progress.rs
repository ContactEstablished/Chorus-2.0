//! The Progress panel's read (P16): the session's plan with step times, skill chips, the current
//! step's tool calls, where the newest run's time went and usual-time markers. Every rule lives in
//! `chorus_core::step_times`, `time_split` and `usual_time`; this only gathers bounded inputs.
use chorus_core::step_times::{self, MAX_SKILL_CALLS, MAX_SNAPSHOTS, MAX_WINDOW_CALLS, SessionProgress};
use chorus_core::{time_split, usual_time};

use crate::store::{activity_log, progress as rows, step_runs};

/// The most skill uses a read looks up earlier runs for, newest first; older ones get no marker.
const MAX_USUAL_LOOKUPS: usize = 20;

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
        self.mark_usual(&mut progress);
        Ok(progress)
    }

    /// Sets each named skill use's marker from the runs of that skill that ended before it started
    /// (16-4, D121). Two indexed reads per use, for the newest 20 uses. Only a run that finished
    /// cleanly (one `persist_turn` stored) can be "shorter"; one cut off by an interrupt, a failure or
    /// the run's end can only be "longer" (spec §6). A failed read leaves the marker off, never the panel.
    fn mark_usual(&self, progress: &mut SessionProgress) {
        let now = crate::store::now_ms();
        let mut uses: Vec<&mut step_times::SkillUse> = progress.steps.iter_mut()
            .flat_map(|s| s.skills.iter_mut()).filter(|u| u.name.is_some()).collect();
        uses.sort_by_key(|u| std::cmp::Reverse((u.at_secs, u.call_id)));
        for u in uses.into_iter().take(MAX_USUAL_LOOKUPS) {
            let Some(name) = u.name.as_deref() else { continue };
            let started_ms = i64::from(u.at_secs) * 1000;
            let read = step_runs::past(&self.store, "skill", name, started_ms, usual_time::WINDOW)
                .and_then(|past| Ok((past, step_runs::stored(&self.store, "skill", &u.call_id.to_string())?)));
            let (past, finished) = match read {
                Ok(r) => r,
                Err(e) => { eprintln!("chorus: usual time for skill {name}: {e}"); continue }
            };
            let this_ms = match u.running_since_secs {
                Some(since) => (now - i64::from(since) * 1000).max(0),
                None => i64::from(u.secs) * 1000,
            };
            u.usual = usual_time::marker(&past, this_ms, finished);
        }
    }
}
