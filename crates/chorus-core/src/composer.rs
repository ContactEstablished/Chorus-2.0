//! Composer policy. A queued instruction cannot answer a question or infer an idle terminal.
use crate::activity::Activity;

pub const MAX_QUEUED: u32 = 32;

/// `count` numbers separated by `;`.
fn numbers(s: &str, count: usize) -> bool {
    let parts: Vec<_> = s.split(';').collect();
    parts.len() == count && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Focus reports, cursor/device replies and mouse reports are terminal protocol, not edits to its
/// prompt. Claude Code 2.1.308 turns on any-motion mouse tracking, so merely moving the pointer
/// over Raw sends reports.
pub fn raw_changes_prompt(data: &str) -> bool {
    if data.is_empty() || matches!(data, "\x1b[I" | "\x1b[O") {
        return false;
    }
    if let Some(body) = data.strip_prefix("\x1b[") {
        if body.strip_suffix('R').is_some_and(|coords| numbers(coords, 2)) {
            return false;
        }
        // SGR (1006) reports, then the older form: three bytes for the button and position.
        if body.strip_prefix('<').and_then(|s| s.strip_suffix(['M', 'm'])).is_some_and(|report| numbers(report, 3)) {
            return false;
        }
        if body.strip_prefix('M').is_some_and(|b| b.chars().count() == 3 && b.chars().all(|c| c >= ' ')) {
            return false;
        }
        if let Some(attributes) = body.strip_prefix('?').and_then(|s| s.strip_suffix('c'))
            && !attributes.is_empty()
            && attributes.bytes().all(|b| b.is_ascii_digit() || b == b';')
        {
            return false;
        }
    }
    true
}

pub fn send_blocker(
    activity: Activity,
    pending: bool,
    dirty: bool,
    unobserved: bool,
    confirmed: bool,
    automatic: bool,
) -> Option<&'static str> {
    if pending || activity == Activity::AwaitingApproval {
        Some("Answer the pending question or approval first.")
    } else if unobserved {
        Some("The previous delivery is awaiting confirmation. Check Raw; it will not be sent again.")
    } else if dirty && !confirmed {
        Some("Raw input may contain a draft. Check that its prompt is empty before sending.")
    } else if activity == Activity::Working {
        Some("The agent is working. Queue this instruction for its next pause.")
    } else if automatic && activity != Activity::Ready {
        Some("Queue paused: waiting for a confirmed ready state without a question.")
    } else if !matches!(activity, Activity::Ready | Activity::NeedsAnswer) && !confirmed {
        Some("Check that the agent has finished starting and its Raw prompt is empty and ready.")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_reports_are_not_user_drafts_but_mixed_input_is() {
        // Mouse: pointer motion and a release as xterm.js reported them, the wheel, an older-form press.
        for data in ["", "\x1b[I", "\x1b[O", "\x1b[1;2R", "\x1b[?1;2c", "\x1b[<35;73;23M", "\x1b[<0;10;5m", "\x1b[<64;1;1M", "\x1b[M #!"] {
            assert!(!raw_changes_prompt(data), "{data:?}");
        }
        for data in ["hello", "\r", "\x03", "\x1b[A", "\x1b[Ihello", "\x1b[1;2Rhello", "\x1b[<35;73;23Mhello", "\x1b[<35;73M", "\x1b[Mab"] {
            assert!(raw_changes_prompt(data), "{data:?}");
        }
    }
    #[test]
    fn automatic_delivery_requires_ready_and_never_answers_a_question() {
        for activity in [Activity::Unknown, Activity::Working, Activity::NeedsAnswer, Activity::AwaitingApproval, Activity::Failed] {
            assert!(send_blocker(activity, false, false, false, false, true).is_some());
        }
        assert!(send_blocker(Activity::Ready, false, false, false, false, true).is_none());
        for (pending, dirty, unobserved) in [(true, false, false), (false, true, false), (false, false, true)] {
            assert!(send_blocker(Activity::Ready, pending, dirty, unobserved, false, true).is_some());
        }
    }
    #[test]
    fn explicit_confirmation_only_resolves_unknown_readiness_and_raw_drafts() {
        assert!(send_blocker(Activity::Unknown, false, true, false, true, false).is_none());
        for activity in [Activity::Working, Activity::AwaitingApproval] {
            assert!(send_blocker(activity, false, true, false, true, false).is_some());
        }
        assert!(send_blocker(Activity::Ready, true, false, false, true, false).is_some());
        assert!(send_blocker(Activity::Ready, false, false, true, true, false).is_some());
    }
}
