//! P6 action contracts. A capability describes a wired channel, never a CLI's advertised feature.
//! Observation hooks are not approval handlers. Unsupported replies stay in the real terminal.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{activity::Activity, agent::AgentKind};

pub const MAX_PROMPT_BYTES: usize = 16 * 1024;
pub const ECHO_WINDOW_MS: i64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum PromptSource { Composer, Automation, Telegram }

impl PromptSource {
    pub fn as_str(self) -> &'static str {
        match self { Self::Composer => "composer", Self::Automation => "automation", Self::Telegram => "telegram" }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalDecision { Allow, Deny }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SessionAction {
    Prompt { text: String, source: PromptSource },
    Answer { ask_id: u32, answers: Vec<String> },
    Approval { ask_id: u32, decision: ApprovalDecision, message: Option<String> },
    Release { ask_id: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "channel", rename_all = "kebab-case")]
pub enum ActionCapability {
    Pty,
    HeldHook,
    Unavailable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActionCapabilities {
    pub prompt: ActionCapability,
    pub question: ActionCapability,
    pub approval: ActionCapability,
    pub plan: ActionCapability,
    pub queue: ActionCapability,
}

pub fn capabilities(agent: AgentKind) -> ActionCapabilities {
    let unavailable = |reason: &str| ActionCapability::Unavailable { reason: reason.into() };
    ActionCapabilities {
        // The first slice only enables delivery for the two hooked TUIs. Other adapters need
        // their structured delivery channel verified before Chorus promises a safe send.
        prompt: match agent {
            AgentKind::Claude | AgentKind::Codex => ActionCapability::Pty,
            _ => unavailable("Prompt delivery is not wired for this agent. Use Raw."),
        },
        question: unavailable("Structured question replies are not wired yet. Answer the agent in Raw."),
        approval: unavailable("Chorus currently observes approvals only. Approve or deny in Raw."),
        plan: unavailable(if agent == AgentKind::Codex {
            "Codex plan updates are not connected in this session. Read its plan in Raw."
        } else {
            "Plan capture is not wired yet. Read the agent's plan in Raw."
        }),
        queue: match agent {
            AgentKind::Claude | AgentKind::Codex => ActionCapability::Pty,
            _ => unavailable("The queue needs a verified prompt channel. Use Raw for this agent."),
        },
    }
}

/// State guard for the PTY fallback. Unknown is not idle; a live question is not a text prompt.
pub fn prompt_blocker(locked: bool, live: bool, activity: Activity, pending_question: bool, unobserved: bool) -> Option<&'static str> {
    if locked { Some("Session is locked. Unlock it before sending an instruction.") }
    else if !live { Some("Session has no live run. Restart it first.") }
    else if activity == Activity::AwaitingApproval { Some("Session is waiting for an approval. Answer it in Raw first.") }
    else if pending_question { Some("Session is waiting for a structured answer. Answer it in Raw first.") }
    else if unobserved { Some("The previous delivery has not been observed. Check Raw before sending again.") }
    else if activity == Activity::Working { Some("Session is busy. Queue the instruction for its next confirmed pause.") }
    else if !matches!(activity, Activity::Ready | Activity::NeedsAnswer) { Some("No confirmed prompt is available. Check Raw first.") }
    else { None }
}

/// Drop terminal control sequences, not merely ESC bytes (which could leave an injected Enter).
/// CRLF/CR are normalised to LF so multiline prompts use bracketed paste.
pub fn clean_prompt(input: &str) -> Result<String, String> {
    if input.len() > MAX_PROMPT_BYTES * 4 { return Err("Instruction is too large (maximum 16 KiB).".into()); }
    let mut out = String::new();
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' | '\u{009b}' | '\u{009d}' => {
                let introducer = if c == '\x1b' { chars.next() } else if c == '\u{009b}' { Some('[') } else { Some(']') };
                match introducer {
                    Some('[') => { for c in chars.by_ref() { if ('@'..='~').contains(&c) { break; } } }
                    Some(']' | 'P' | 'X' | '^' | '_') => {
                        while let Some(c) = chars.next() {
                            if c == '\x07' || c == '\u{009c}' { break; }
                            if c == '\x1b' && chars.peek() == Some(&'\\') { chars.next(); break; }
                        }
                    }
                    Some(' '..='/') => { for c in chars.by_ref() { if ('0'..='~').contains(&c) { break; } } }
                    _ => {}
                }
            }
            '\r' => { if chars.peek() == Some(&'\n') { chars.next(); } out.push('\n'); }
            '\n' | '\t' => out.push(c),
            c if !c.is_control() => out.push(c),
            _ => {}
        }
    }
    let out = out.trim().to_owned();
    if out.is_empty() || out.len() > MAX_PROMPT_BYTES { return Err("Instruction must contain 1 to 16 KiB of text.".into()); }
    Ok(out)
}

pub fn prompt_write(text: &str) -> String {
    if text.contains('\n') { format!("\x1b[200~{text}\x1b[201~") } else { text.into() }
}

pub fn echo_matches(expected_run: u32, actual_run: u32, expected: &str, actual: &str, sent_ms: i64, now_ms: i64) -> bool {
    expected_run == actual_run && expected == actual && (0..=ECHO_WINDOW_MS).contains(&now_ms.saturating_sub(sent_ms))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strips_control_sequences_and_preserves_safe_multiline_text() {
        let text = clean_prompt(" \x1b[31mone\x1b[0m\r\ntwo\x1b]52;c;hidden\x07\x03\tend ").unwrap();
        assert_eq!(text, "one\ntwo\tend");
        assert_eq!(prompt_write(&text), "\x1b[200~one\ntwo\tend\x1b[201~");
        assert!(!prompt_write(&text).contains('\r'));
        assert_eq!(clean_prompt("a\x1bPdiscard\x1b\\b\u{009b}31mc").unwrap(), "abc");
        assert_eq!(clean_prompt("x\x1b[200~y\x1b[201~").unwrap(), "xy");
    }
    #[test]
    fn bounds_bytes_not_characters_and_rejects_empty_control_input() {
        assert!(clean_prompt("\x1b[31m\x03 \t").is_err());
        assert!(clean_prompt(&"é".repeat(MAX_PROMPT_BYTES)).is_err());
        assert!(clean_prompt(&"a".repeat(MAX_PROMPT_BYTES)).is_ok());
    }
    #[test]
    fn matches_only_same_run_text_and_bounded_time() {
        assert!(echo_matches(1, 1, "hi", "hi", 100, 60_100));
        assert!(!echo_matches(1, 2, "hi", "hi", 100, 101));
        assert!(!echo_matches(1, 1, "hi", "hi", 100, 99));
        assert!(!echo_matches(1, 1, "hi", "hi", 100, 60_101));
        assert!(!echo_matches(1, 1, "hi", "bye", 100, 101));
    }
    #[test]
    fn guarded_delivery_cannot_answer_an_approval_or_guess_idle() {
        for state in [Activity::Unknown, Activity::Working, Activity::AwaitingApproval, Activity::Failed] {
            assert!(prompt_blocker(false, true, state, false, false).is_some());
        }
        assert!(prompt_blocker(false, true, Activity::NeedsAnswer, true, false).is_some());
        assert!(prompt_blocker(false, true, Activity::Ready, false, true).is_some());
        assert!(prompt_blocker(true, true, Activity::Ready, false, false).is_some());
        assert!(prompt_blocker(false, false, Activity::Ready, false, false).is_some());
        assert!(prompt_blocker(false, true, Activity::NeedsAnswer, false, false).is_none());
    }
    #[test]
    fn observation_is_not_a_reply_capability() {
        for agent in AgentKind::ALL {
            let caps = capabilities(agent);
            assert!(matches!(caps.approval, ActionCapability::Unavailable { .. }));
            assert!(matches!(caps.question, ActionCapability::Unavailable { .. }));
            assert!(matches!(caps.plan, ActionCapability::Unavailable { .. }));
        }
    }
}
