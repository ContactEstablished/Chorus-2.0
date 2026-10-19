//! What a session's agent is doing (ARCHITECTURE §6), and which turns are open, from its
//! [`AgentEvent`]s. Pure: the app feeds events and the time, and records the turn changes this
//! returns.
//!
//! Turns pair by the agent's own turn id where it gives one, else first in, first out. A turn
//! still open when a later one ends was interrupted (Claude Code sends no `Stop` for an interrupted
//! turn, LESSONS L24). Output silence never makes a session look busy: a Working session with
//! neither events nor output for [`STALE_MS`] is shown as "no signal", not changed.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::events::AgentEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Activity {
    /// No event yet, or the agent has no event channel (Grok, a shell).
    Unknown,
    /// A turn is in progress.
    Working,
    /// The agent waits for permission to act.
    AwaitingApproval,
    /// The agent asked a question, or ended its turn with one.
    NeedsAnswer,
    /// The turn ended and the agent waits for the next prompt.
    Ready,
    /// The turn ended in an error.
    Failed,
}

/// How a turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum TurnEnd {
    Done,
    Interrupted,
    Failed,
    /// The run ended with the turn still open.
    Lost,
}

impl TurnEnd {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
            Self::Lost => "lost",
        }
    }
}

/// What the app records. `key` is the tracker's own number for a turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnChange {
    Opened { key: u64, turn: Option<String>, prompt: Option<String> },
    Closed { key: u64, end: TurnEnd, message: Option<String> },
}

/// No event and no output for this long while Working is shown as "no signal".
pub const STALE_MS: i64 = 120_000;

#[derive(Debug, Clone)]
struct OpenTurn {
    key: u64,
    turn: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Tracker {
    activity: Activity,
    open: VecDeque<OpenTurn>,
    next_key: u64,
    last_event_ms: i64,
}

impl Default for Tracker {
    fn default() -> Self {
        Self { activity: Activity::Unknown, open: VecDeque::new(), next_key: 1, last_event_ms: 0 }
    }
}

/// Whether a final message ends by asking the user something (its last line ends with `?`). The
/// narrator replaces this guess in P7.
pub fn looks_like_question(message: &str) -> bool {
    message
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .map(|l| l.trim_end_matches(['*', '_', '`', ')', '"', '\'']))
        .is_some_and(|l| l.ends_with('?'))
}

impl Tracker {
    pub fn activity(&self) -> Activity {
        self.activity
    }

    pub fn open_turns(&self) -> usize {
        self.open.len()
    }

    /// Working with neither an event nor output since `STALE_MS` ago.
    pub fn stale(&self, now_ms: i64, last_output_ms: i64) -> bool {
        self.activity == Activity::Working && now_ms - self.last_event_ms.max(last_output_ms) > STALE_MS
    }

    fn open_turn(&mut self, turn: Option<String>, prompt: Option<String>, out: &mut Vec<TurnChange>) -> u64 {
        let key = self.next_key;
        self.next_key += 1;
        self.open.push_back(OpenTurn { key, turn: turn.clone() });
        out.push(TurnChange::Opened { key, turn, prompt });
        key
    }

    /// Closes the turn `turn` names (or the oldest), and as interrupted every turn opened before
    /// it. With nothing open, the end is recorded as a turn of its own.
    fn close(&mut self, turn: Option<&str>, end: TurnEnd, message: Option<String>, out: &mut Vec<TurnChange>) {
        let at = turn.and_then(|t| self.open.iter().position(|o| o.turn.as_deref() == Some(t))).or(if self.open.is_empty() {
            None
        } else {
            Some(0)
        });
        let Some(at) = at else {
            let key = self.open_turn(turn.map(str::to_owned), None, out);
            self.open.pop_back();
            out.push(TurnChange::Closed { key, end, message });
            return;
        };
        for earlier in self.open.drain(..at).collect::<Vec<_>>() {
            out.push(TurnChange::Closed { key: earlier.key, end: TurnEnd::Interrupted, message: None });
        }
        let closing = self.open.pop_front().expect("position found");
        out.push(TurnChange::Closed { key: closing.key, end, message });
    }

    /// The state after an event, and the turns it opened or closed.
    pub fn apply(&mut self, event: &AgentEvent, now_ms: i64) -> Vec<TurnChange> {
        self.last_event_ms = now_ms;
        let mut out = Vec::new();
        let after_close = |t: &Self, message: Option<&str>| {
            if !t.open.is_empty() {
                Activity::Working
            } else if message.is_some_and(looks_like_question) {
                Activity::NeedsAnswer
            } else {
                Activity::Ready
            }
        };
        match event {
            AgentEvent::TurnStarted { turn, prompt } => {
                self.open_turn(turn.clone(), prompt.clone(), &mut out);
                self.activity = Activity::Working;
            }
            AgentEvent::TurnEnded { turn, message } => {
                self.close(turn.as_deref(), TurnEnd::Done, message.clone(), &mut out);
                self.activity = after_close(self, message.as_deref());
            }
            AgentEvent::TurnInterrupted { turn } => {
                self.close(turn.as_deref(), TurnEnd::Interrupted, None, &mut out);
                self.activity = after_close(self, None);
            }
            AgentEvent::TurnFailed { turn, error } => {
                self.close(turn.as_deref(), TurnEnd::Failed, Some(error.clone()), &mut out);
                self.activity = if self.open.is_empty() { Activity::Failed } else { Activity::Working };
            }
            AgentEvent::ApprovalAsked { .. } => self.activity = Activity::AwaitingApproval,
            AgentEvent::QuestionAsked { .. } => self.activity = Activity::NeedsAnswer,
            // An answer, or the agent getting on with tools, means it is working again.
            AgentEvent::ApprovalAnswered { .. }
            | AgentEvent::QuestionAnswered { .. }
            | AgentEvent::ToolStarted { .. }
            | AgentEvent::ToolFinished { .. } => {
                if !self.open.is_empty() || matches!(self.activity, Activity::AwaitingApproval | Activity::NeedsAnswer) {
                    self.activity = Activity::Working;
                }
            }
            // Idle with turns still open: they were interrupted.
            AgentEvent::Idle => {
                while let Some(o) = self.open.pop_front() {
                    out.push(TurnChange::Closed { key: o.key, end: TurnEnd::Interrupted, message: None });
                }
                if self.activity != Activity::NeedsAnswer {
                    self.activity = Activity::Ready;
                }
            }
        }
        out
    }

    /// The run ended: open turns are lost, and the next run starts unknown.
    pub fn run_ended(&mut self) -> Vec<TurnChange> {
        let out = self.open.drain(..).map(|o| TurnChange::Closed { key: o.key, end: TurnEnd::Lost, message: None }).collect();
        self.activity = Activity::Unknown;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started(turn: &str) -> AgentEvent {
        AgentEvent::TurnStarted { turn: Some(turn.into()), prompt: Some(format!("prompt {turn}")) }
    }
    fn ended(turn: Option<&str>, message: &str) -> AgentEvent {
        AgentEvent::TurnEnded { turn: turn.map(str::to_owned), message: Some(message.into()) }
    }

    #[test]
    fn a_turn_runs_and_ends() {
        let mut t = Tracker::default();
        assert_eq!(t.activity(), Activity::Unknown);
        assert_eq!(t.apply(&started("a"), 1), [TurnChange::Opened { key: 1, turn: Some("a".into()), prompt: Some("prompt a".into()) }]);
        assert_eq!(t.activity(), Activity::Working);
        let tool = AgentEvent::ToolStarted { turn: None, call: None, tool: "Write".into(), target: None };
        assert!(t.apply(&tool, 2).is_empty());
        assert_eq!(t.apply(&ended(Some("a"), "Done."), 3), [TurnChange::Closed { key: 1, end: TurnEnd::Done, message: Some("Done.".into()) }]);
        assert_eq!(t.activity(), Activity::Ready);
    }

    #[test]
    fn a_queued_prompt_waits_for_its_own_end() {
        // Claude Code reports a prompt typed during a turn at once (LESSONS L24).
        let mut t = Tracker::default();
        t.apply(&started("a"), 1);
        t.apply(&started("b"), 2);
        t.apply(&ended(Some("a"), "first"), 3);
        assert_eq!(t.activity(), Activity::Working);
        assert_eq!(t.open_turns(), 1);
        t.apply(&ended(Some("b"), "second"), 4);
        assert_eq!((t.activity(), t.open_turns()), (Activity::Ready, 0));
    }

    #[test]
    fn a_turn_left_behind_a_later_end_was_interrupted() {
        let mut t = Tracker::default();
        t.apply(&started("a"), 1);
        t.apply(&started("b"), 2);
        let changes = t.apply(&ended(Some("b"), "ok"), 3);
        assert_eq!(
            changes,
            [
                TurnChange::Closed { key: 1, end: TurnEnd::Interrupted, message: None },
                TurnChange::Closed { key: 2, end: TurnEnd::Done, message: Some("ok".into()) }
            ]
        );
    }

    #[test]
    fn without_ids_turns_pair_in_order() {
        let mut t = Tracker::default();
        t.apply(&AgentEvent::TurnStarted { turn: None, prompt: None }, 1);
        t.apply(&AgentEvent::TurnStarted { turn: None, prompt: None }, 2);
        assert_eq!(t.apply(&ended(None, "x"), 3)[0], TurnChange::Closed { key: 1, end: TurnEnd::Done, message: Some("x".into()) });
        // An id nobody opened closes the oldest open turn.
        assert_eq!(t.apply(&ended(Some("zz"), "y"), 4)[0], TurnChange::Closed { key: 2, end: TurnEnd::Done, message: Some("y".into()) });
        // An end with nothing open is recorded as a turn of its own.
        let orphan = t.apply(&ended(Some("q"), "z"), 5);
        assert!(matches!(orphan[..], [TurnChange::Opened { .. }, TurnChange::Closed { end: TurnEnd::Done, .. }]));
        assert_eq!(t.open_turns(), 0);
    }

    #[test]
    fn approvals_questions_and_failures() {
        let mut t = Tracker::default();
        t.apply(&started("a"), 1);
        t.apply(&AgentEvent::ApprovalAsked { turn: None, ask: None, tool: "Bash".into(), target: None }, 2);
        assert_eq!(t.activity(), Activity::AwaitingApproval);
        t.apply(&AgentEvent::ToolFinished { turn: None, call: None, tool: "Bash".into(), target: None, failed: false }, 3);
        assert_eq!(t.activity(), Activity::Working);
        t.apply(&AgentEvent::QuestionAsked { turn: None, ask: None, question: "Which?".into() }, 4);
        assert_eq!(t.activity(), Activity::NeedsAnswer);
        t.apply(&ended(Some("a"), "Shall I also update the tests?"), 5);
        assert_eq!(t.activity(), Activity::NeedsAnswer, "a final question needs an answer");
        t.apply(&started("b"), 6);
        t.apply(&AgentEvent::TurnFailed { turn: Some("b".into()), error: "rate limited".into() }, 7);
        assert_eq!(t.activity(), Activity::Failed);
    }

    #[test]
    fn idle_closes_interrupted_turns_and_a_run_end_loses_open_ones() {
        let mut t = Tracker::default();
        t.apply(&started("a"), 1);
        assert_eq!(t.apply(&AgentEvent::Idle, 2), [TurnChange::Closed { key: 1, end: TurnEnd::Interrupted, message: None }]);
        assert_eq!(t.activity(), Activity::Ready);
        t.apply(&started("b"), 3);
        assert_eq!(t.run_ended(), [TurnChange::Closed { key: 2, end: TurnEnd::Lost, message: None }]);
        assert_eq!(t.activity(), Activity::Unknown);
    }

    #[test]
    fn silence_is_shown_never_assumed() {
        let mut t = Tracker::default();
        assert!(!t.stale(1_000_000, 0), "not working: never stale");
        t.apply(&started("a"), 0);
        assert!(!t.stale(STALE_MS, 0));
        assert!(t.stale(STALE_MS + 1, 0));
        assert!(!t.stale(STALE_MS + 1, 5_000), "recent output keeps it live");
        assert_eq!(t.activity(), Activity::Working, "staleness never changes the state");
    }

    #[test]
    fn questions_are_spotted_at_the_end() {
        assert!(looks_like_question("Done.\n\nShould I commit it?"));
        assert!(looks_like_question("Want me to continue?**\n\n"));
        assert!(!looks_like_question("Is this right? I checked it."));
        assert!(!looks_like_question(""));
    }
}
