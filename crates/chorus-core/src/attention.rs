//! What a session shows in the Overview and the rail (DESIGN.md "Status vocabulary"): one word
//! for whether it needs you, is busy, or is finished. Computed once here, so cards, badges and
//! (in P7) the tray always agree.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::activity::Activity;
use crate::session::{RunEnd, SessionState};

/// In urgency order, most urgent first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Attention {
    /// The agent waits for permission to act.
    Approval,
    /// The agent asked a question.
    Input,
    /// The last turn failed, the process exited with an error, or it couldn't start.
    Failed,
    Working,
    /// The turn ended; the agent waits for the next prompt.
    Ready,
    /// Running, with no event channel to say more (a shell, Grok, or an agent before its first
    /// event). Never shown as working (ARCHITECTURE §6).
    Running,
    /// Exited cleanly, or closed.
    Done,
}

/// Where a session's card goes in the Overview when grouped by status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum AttentionGroup {
    NeedsYou,
    InFlight,
    Finished,
}

impl Attention {
    pub fn needs_you(self) -> bool {
        matches!(self, Self::Approval | Self::Input | Self::Failed)
    }

    pub fn group(self) -> AttentionGroup {
        match self {
            Self::Approval | Self::Input | Self::Failed => AttentionGroup::NeedsYou,
            Self::Working | Self::Ready | Self::Running => AttentionGroup::InFlight,
            Self::Done => AttentionGroup::Finished,
        }
    }
}

/// `last_end` and `last_exit_code` are the newest run's. An exited session whose newest run
/// didn't end by exiting couldn't start again, unless Chorus closed its agent after its
/// automation run finished (D84): the agent was stopped, so its exit code says nothing.
pub fn attention(state: SessionState, live: bool, activity: Activity, last_end: Option<RunEnd>, last_exit_code: Option<u32>) -> Attention {
    match state {
        SessionState::Running if live => match activity {
            Activity::AwaitingApproval => Attention::Approval,
            Activity::NeedsAnswer => Attention::Input,
            Activity::Failed => Attention::Failed,
            Activity::Working => Attention::Working,
            Activity::Ready => Attention::Ready,
            Activity::Unknown => Attention::Running,
        },
        // Between runs, for a moment, while a Restart or a relaunch is under way.
        SessionState::Running => Attention::Running,
        SessionState::Exited => match (last_end, last_exit_code) {
            (Some(RunEnd::Exited), Some(0)) | (Some(RunEnd::Closed), _) => Attention::Done,
            _ => Attention::Failed,
        },
        SessionState::Closed => Attention::Done,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_live_session_shows_its_activity() {
        let live = |a| attention(SessionState::Running, true, a, None, None);
        assert_eq!(live(Activity::AwaitingApproval), Attention::Approval);
        assert_eq!(live(Activity::NeedsAnswer), Attention::Input);
        assert_eq!(live(Activity::Working), Attention::Working);
        assert_eq!(live(Activity::Unknown), Attention::Running, "no channel is never 'working'");
        assert_eq!(attention(SessionState::Running, false, Activity::Working, None, None), Attention::Running);
    }

    #[test]
    fn an_exit_is_done_only_when_clean() {
        let exited = |end, code| attention(SessionState::Exited, false, Activity::Unknown, end, code);
        assert_eq!(exited(Some(RunEnd::Exited), Some(0)), Attention::Done);
        assert_eq!(exited(Some(RunEnd::Exited), Some(137)), Attention::Failed);
        assert_eq!(exited(Some(RunEnd::Restarted), Some(0)), Attention::Failed, "a Restart that couldn't start");
        assert_eq!(exited(None, None), Attention::Failed, "never started");
        assert_eq!(exited(Some(RunEnd::Closed), Some(1)), Attention::Done, "closed after its automation run finished");
    }

    #[test]
    fn urgency_and_groups_follow_the_design() {
        let mut all = [Attention::Done, Attention::Running, Attention::Working, Attention::Failed, Attention::Approval, Attention::Ready, Attention::Input];
        all.sort();
        assert_eq!(
            all,
            [Attention::Approval, Attention::Input, Attention::Failed, Attention::Working, Attention::Ready, Attention::Running, Attention::Done]
        );
        assert_eq!(all.iter().filter(|a| a.needs_you()).count(), 3);
        assert_eq!(Attention::Running.group(), AttentionGroup::InFlight);
    }
}
