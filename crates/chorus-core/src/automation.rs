//! Automation runs (P8): when a session an automation started blocks its next run (D85), and when
//! a run finished and its agent can be closed (D84). `src-tauri/src/automations.rs` reads the facts.

use crate::activity::Activity;

/// What Chorus knows about one live session an automation started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunSession {
    pub activity: Activity,
    /// Its current run's first prompt hasn't been seen by the agent yet: it is still starting.
    pub starting: bool,
    /// Its current run is the one the automation launched, not a Restart or a relaunch.
    pub first_run: bool,
    /// Someone sent a prompt other than the automation's.
    pub other_prompts: bool,
    /// Raw input changed the agent's prompt line during this run.
    pub typed: bool,
    /// Friendly holds a draft or queued prompts.
    pub drafted: bool,
    pub locked: bool,
}

/// D85: a busy session blocks the next run, whichever run started it: working, asking, or still
/// starting. Unknown counts only while starting; an idle session doesn't block.
pub fn busy(s: &RunSession) -> bool {
    matches!(s.activity, Activity::Working | Activity::AwaitingApproval | Activity::NeedsAnswer)
        || (s.activity == Activity::Unknown && s.starting)
}

/// D84: the automation's turn ended cleanly (not failed, not asking) and nobody has taken the
/// session over, so its agent can be closed. A turn ending alone isn't enough: any prompt, Raw
/// typing, draft, queued prompt, lock or later run keeps it open.
pub fn finished(s: &RunSession) -> bool {
    s.activity == Activity::Ready && !s.starting && s.first_run && !s.other_prompts && !s.typed && !s.drafted && !s.locked
}

#[cfg(test)]
mod tests {
    use super::*;

    const DONE: RunSession = RunSession {
        activity: Activity::Ready,
        starting: false,
        first_run: true,
        other_prompts: false,
        typed: false,
        drafted: false,
        locked: false,
    };

    #[test]
    fn a_clean_untouched_end_is_finished() {
        assert!(finished(&DONE));
        assert!(!busy(&DONE));
    }

    #[test]
    fn failing_asking_or_working_is_not_finished() {
        for activity in [Activity::Failed, Activity::NeedsAnswer, Activity::AwaitingApproval, Activity::Working, Activity::Unknown] {
            assert!(!finished(&RunSession { activity, ..DONE }), "{activity:?}");
        }
    }

    #[test]
    fn any_takeover_keeps_it_open() {
        let taken = [
            RunSession { starting: true, ..DONE },
            RunSession { first_run: false, ..DONE },
            RunSession { other_prompts: true, ..DONE },
            RunSession { typed: true, ..DONE },
            RunSession { drafted: true, ..DONE },
            RunSession { locked: true, ..DONE },
        ];
        for s in taken {
            assert!(!finished(&s), "{s:?}");
        }
    }

    #[test]
    fn busy_is_working_asking_or_starting() {
        for activity in [Activity::Working, Activity::AwaitingApproval, Activity::NeedsAnswer] {
            assert!(busy(&RunSession { activity, ..DONE }), "{activity:?}");
        }
        assert!(busy(&RunSession { activity: Activity::Unknown, starting: true, ..DONE }));
        assert!(!busy(&RunSession { activity: Activity::Unknown, ..DONE }));
        assert!(!busy(&RunSession { activity: Activity::Failed, ..DONE }));
    }
}
