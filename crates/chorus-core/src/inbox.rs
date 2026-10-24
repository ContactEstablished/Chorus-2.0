//! Waiting ages use persisted facts, never the time a view attached.

use crate::attention::Attention;

#[derive(Debug, Default)]
pub struct WaitingFacts {
    pub approval_at: Option<i64>,
    pub question_at: Option<i64>,
    pub question_turn_ended_at: Option<i64>,
    pub failed_turn_ended_at: Option<i64>,
    pub run_ended_at: Option<i64>,
}

/// Missing evidence stays unknown. A stopped process takes precedence over its last turn.
pub fn needs_you_since(attention: Attention, facts: &WaitingFacts) -> Option<i64> {
    let at = match attention {
        Attention::Approval => facts.approval_at,
        Attention::Input => facts.question_at.or(facts.question_turn_ended_at),
        Attention::Failed => facts.run_ended_at.or(facts.failed_turn_ended_at),
        _ => None,
    };
    at.filter(|at| *at >= 0)
}

/// The kind of waiting a notification reports, as stored in `notifications.kind`.
pub fn notice_kind(attention: Attention) -> Option<&'static str> {
    match attention {
        Attention::Approval => Some("approval"),
        Attention::Input => Some("input"),
        Attention::Failed => Some("failed"),
        _ => None,
    }
}

/// What a session's current state says about notifying.
pub struct NoticeFacts<'a> {
    pub session: u32,
    pub name: &'a str,
    pub attention: Attention,
    /// `needs_you_since`, when known.
    pub since_ms: Option<i64>,
    /// When the current (or last) run started, or the session was made: the episode's fallback
    /// identity when its start is unknown.
    pub fallback_at: i64,
    /// What it last said or was asked (already scrubbed), or why it stopped.
    pub detail: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub kind: &'static str,
    pub title: String,
    pub body: String,
    /// One notification per waiting episode: the same key is never stored twice, so a Restart,
    /// an app restart or a repeated summary never notifies again.
    pub dedupe_key: String,
}

/// Longest body kept, in characters.
pub const NOTICE_BODY_CHARS: usize = 240;

/// Entering Awaiting approval, Needs your answer or Failed notifies (P7, decision 5); Ready and
/// Completed never do. The episode is the session, the kind and when it started waiting.
pub fn notice_for(f: &NoticeFacts) -> Option<Notice> {
    let kind = notice_kind(f.attention)?;
    let title = match f.attention {
        Attention::Approval => format!("{} needs your approval", f.name),
        Attention::Input => format!("{} needs your answer", f.name),
        _ => format!("{} failed", f.name),
    };
    let episode = f.since_ms.map_or_else(|| format!("run{}", f.fallback_at), |at| at.to_string());
    let body: String = f.detail.map(str::trim).unwrap_or_default().chars().take(NOTICE_BODY_CHARS).collect();
    Some(Notice { kind, title, body, dedupe_key: format!("{}:{kind}:{episode}", f.session) })
}

/// Notifications older than this are deleted at start.
pub const NOTICE_KEEP_MS: i64 = 30 * 24 * 60 * 60 * 1000;

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(attention: Attention, since_ms: Option<i64>) -> NoticeFacts<'static> {
        NoticeFacts { session: 7, name: "Ada", attention, since_ms, fallback_at: 5, detail: Some("  Run the tests?  ") }
    }

    #[test]
    fn only_needs_you_states_notify() {
        assert_eq!(notice_for(&facts(Attention::Approval, Some(10))).unwrap().title, "Ada needs your approval");
        assert_eq!(notice_for(&facts(Attention::Input, Some(10))).unwrap().kind, "input");
        assert_eq!(notice_for(&facts(Attention::Failed, Some(10))).unwrap().title, "Ada failed");
        for a in [Attention::Working, Attention::Ready, Attention::Running, Attention::Done] {
            assert!(notice_for(&facts(a, Some(10))).is_none());
        }
    }

    #[test]
    fn one_key_per_episode() {
        let a = notice_for(&facts(Attention::Approval, Some(10))).unwrap();
        assert_eq!(a.dedupe_key, "7:approval:10");
        assert_eq!(a, notice_for(&facts(Attention::Approval, Some(10))).unwrap());
        assert_ne!(a.dedupe_key, notice_for(&facts(Attention::Approval, Some(11))).unwrap().dedupe_key);
        assert_ne!(a.dedupe_key, notice_for(&facts(Attention::Input, Some(10))).unwrap().dedupe_key);
        // An unknown start falls back to the run, so it still notifies once.
        assert_eq!(notice_for(&facts(Attention::Failed, None)).unwrap().dedupe_key, "7:failed:run5");
    }

    #[test]
    fn bodies_are_trimmed_and_bounded() {
        assert_eq!(notice_for(&facts(Attention::Input, Some(1))).unwrap().body, "Run the tests?");
        let long = "x".repeat(1000);
        let f = NoticeFacts { detail: Some(&long), ..facts(Attention::Input, Some(1)) };
        assert_eq!(notice_for(&f).unwrap().body.chars().count(), NOTICE_BODY_CHARS);
        let none = NoticeFacts { detail: None, ..facts(Attention::Input, Some(1)) };
        assert_eq!(notice_for(&none).unwrap().body, "");
    }

    #[test]
    fn facts_belong_to_the_current_attention_kind() {
        let facts = WaitingFacts {
            approval_at: Some(10),
            question_at: Some(20),
            question_turn_ended_at: Some(30),
            failed_turn_ended_at: Some(40),
            run_ended_at: Some(50),
        };
        assert_eq!(needs_you_since(Attention::Approval, &facts), Some(10));
        assert_eq!(needs_you_since(Attention::Input, &facts), Some(20));
        assert_eq!(needs_you_since(Attention::Failed, &facts), Some(50));
        for attention in [
            Attention::Working,
            Attention::Ready,
            Attention::Running,
            Attention::Done,
        ] {
            assert_eq!(needs_you_since(attention, &facts), None);
        }
    }

    #[test]
    fn final_turns_are_fallbacks_and_missing_evidence_is_unknown() {
        let facts = WaitingFacts {
            question_turn_ended_at: Some(30),
            failed_turn_ended_at: Some(40),
            ..Default::default()
        };
        assert_eq!(needs_you_since(Attention::Input, &facts), Some(30));
        assert_eq!(needs_you_since(Attention::Failed, &facts), Some(40));
        assert_eq!(needs_you_since(Attention::Approval, &facts), None);
        assert_eq!(
            needs_you_since(Attention::Failed, &WaitingFacts::default()),
            None
        );
    }
}
