//! Conservative queue gates: terminal output and silence never prove a safe prompt.
use crate::activity::Activity;

pub fn blocker(reviewed_run: u32, current_run: u32, activity: Activity, raw_empty: bool) -> Option<&'static str> {
    if reviewed_run != current_run { Some("The session restarted. Review this instruction before rearming it.") }
    else if !raw_empty { Some("Raw may contain a draft. Check Raw, clear or submit its draft, then confirm its prompt is empty.") }
    else if activity != Activity::Ready { Some("Waiting for a structured Ready state. Questions and approvals must be answered first.") }
    else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_reviewed_ready_empty_prompt_can_dispatch() {
        assert!(blocker(1, 1, Activity::Ready, true).is_none());
        for state in [Activity::Unknown, Activity::Working, Activity::NeedsAnswer, Activity::AwaitingApproval, Activity::Failed] {
            assert!(blocker(1, 1, state, true).is_some());
        }
        assert!(blocker(1, 2, Activity::Ready, true).is_some());
        assert!(blocker(1, 1, Activity::Ready, false).is_some());
    }
}
