//! One event model for every agent (ARCHITECTURE §5, PLAN P3). Hooks (Claude Code, Codex) and
//! opencode's server send their own shapes; these functions turn each into [`AgentEvent`]s. They
//! read only the fields the state machine and history need, and ignore the rest, so a CLI that adds
//! fields keeps working. Shapes are from S5's captures and Codex 0.162.1's hook schemas.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

/// What an agent said happened. `turn` is the agent's own id for the turn where it gives one
/// (Claude Code's `prompt_id`, Codex's `turn_id`), which pairs a turn's start and end exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AgentEvent {
    /// A prompt was submitted. Claude Code reports a prompt typed while a turn runs at once, so a
    /// turn can start before the previous one ends.
    TurnStarted { turn: Option<String>, prompt: Option<String> },
    TurnEnded { turn: Option<String>, message: Option<String> },
    /// The user stopped the turn.
    TurnInterrupted { turn: Option<String> },
    TurnFailed { turn: Option<String>, error: String },
    ToolStarted { turn: Option<String>, call: Option<String>, tool: String, target: Option<String> },
    ToolFinished { turn: Option<String>, call: Option<String>, tool: String, target: Option<String>, failed: bool },
    /// The agent waits for permission to act.
    ApprovalAsked { turn: Option<String>, ask: Option<String>, tool: String, target: Option<String> },
    ApprovalAnswered { ask: Option<String>, allowed: bool },
    /// The agent asked the user a question and waits for the answer.
    QuestionAsked { turn: Option<String>, ask: Option<String>, question: String },
    QuestionAnswered { ask: Option<String>, answer: Option<String> },
    /// The agent says it is idle and waiting for input.
    Idle,
}

/// Where a tool call points, short enough for a list: a file, a command, a pattern or an address.
pub const MAX_TARGET_CHARS: usize = 160;

/// The most telling field of a tool's input, on one line.
pub fn tool_target(input: &Value) -> Option<String> {
    let input = match input {
        // Some hooks send the input as JSON text.
        Value::String(s) => serde_json::from_str::<Value>(s).unwrap_or(Value::String(s.clone())),
        v => v.clone(),
    };
    let text = match &input {
        Value::String(s) => Some(s.clone()),
        Value::Object(o) => ["file_path", "path", "filePath", "command", "cmd", "pattern", "url", "query", "description"]
            .iter()
            .find_map(|k| o.get(*k).and_then(Value::as_str).map(str::to_owned)),
        _ => None,
    }?;
    let one_line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut short: String = one_line.chars().take(MAX_TARGET_CHARS).collect();
    if one_line.chars().count() > MAX_TARGET_CHARS {
        short.push('…');
    }
    (!short.is_empty()).then_some(short)
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned)
}

/// The hook's event name, for the raw record.
pub fn hook_event_name(payload: &Value) -> Option<String> {
    text(payload, "hook_event_name")
}

/// A Claude Code 2.1.300 hook payload (HTTP hooks, D36).
pub fn from_claude_hook(p: &Value) -> Vec<AgentEvent> {
    let turn = text(p, "prompt_id");
    let tool = text(p, "tool_name").unwrap_or_default();
    let input = p.get("tool_input").cloned().unwrap_or(Value::Null);
    let call = text(p, "tool_use_id");
    match hook_event_name(p).as_deref() {
        Some("UserPromptSubmit") => vec![AgentEvent::TurnStarted { turn, prompt: text(p, "prompt") }],
        // AskUserQuestion is Claude asking the user; it arrives as a tool call (S5).
        Some("PreToolUse") if tool == "AskUserQuestion" => {
            vec![AgentEvent::QuestionAsked { turn, ask: call, question: ask_user_question(&input).unwrap_or_default() }]
        }
        Some("PostToolUse") if tool == "AskUserQuestion" => vec![AgentEvent::QuestionAnswered {
            ask: call, answer: p.pointer("/tool_response/answers").or_else(|| input.get("answers")).and_then(question_answers),
        }],
        Some("PostToolUseFailure") if tool == "AskUserQuestion" => vec![AgentEvent::QuestionAnswered { ask: call, answer: Some("question failed or cancelled".into()) }],
        Some("PreToolUse") => vec![AgentEvent::ToolStarted { turn, call, tool, target: tool_target(&input) }],
        Some("PostToolUse") => vec![AgentEvent::ToolFinished { turn, call, tool, target: tool_target(&input), failed: false }],
        Some("PostToolUseFailure") => vec![AgentEvent::ToolFinished { turn, call, tool, target: tool_target(&input), failed: true }],
        // Releasing the question's PreToolUse hook lets Claude open its native question UI.
        // Its PermissionRequest is the same question, not an additional approval to grant.
        Some("PermissionRequest") if tool == "AskUserQuestion" => Vec::new(),
        Some("PermissionRequest") => vec![AgentEvent::ApprovalAsked { turn, ask: call, tool, target: tool_target(&input) }],
        Some("Notification") if text(p, "notification_type").as_deref() == Some("idle_prompt") => vec![AgentEvent::Idle],
        Some("Stop") => vec![AgentEvent::TurnEnded { turn, message: text(p, "last_assistant_message") }],
        Some("StopFailure") => vec![AgentEvent::TurnFailed {
            turn,
            error: text(p, "error").or_else(|| text(p, "message")).unwrap_or_else(|| "the turn failed".into()),
        }],
        _ => Vec::new(),
    }
}

/// The question text of an `AskUserQuestion` call: `questions[0].question`.
fn ask_user_question(input: &Value) -> Option<String> {
    let input = match input {
        Value::String(s) => serde_json::from_str(s).ok()?,
        v => v.clone(),
    };
    input.pointer("/questions/0/question").and_then(Value::as_str).map(str::to_owned)
}

fn question_answers(value: &Value) -> Option<String> {
    // Decoded text reaches the scrubber; serialising here could hide embedded credentials.
    Some(value.as_object()?.iter().filter_map(|(q, a)| a.as_str().map(|a| format!("{q}: {a}"))).collect::<Vec<_>>().join("\n"))
}

/// A Codex 0.162.1 command hook payload, relayed by `chorus-hook` (D37).
pub fn from_codex_hook(p: &Value) -> Vec<AgentEvent> {
    let turn = text(p, "turn_id");
    let tool = text(p, "tool_name").unwrap_or_default();
    let input = p.get("tool_input").cloned().unwrap_or(Value::Null);
    let call = text(p, "tool_use_id");
    match hook_event_name(p).as_deref() {
        Some("UserPromptSubmit") => vec![AgentEvent::TurnStarted { turn, prompt: text(p, "prompt") }],
        Some("PreToolUse") => vec![AgentEvent::ToolStarted { turn, call, tool, target: tool_target(&input) }],
        Some("PostToolUse") => vec![AgentEvent::ToolFinished { turn, call, tool, target: tool_target(&input), failed: false }],
        Some("PermissionRequest") => vec![AgentEvent::ApprovalAsked { turn, ask: None, tool, target: tool_target(&input) }],
        Some("Stop") => vec![AgentEvent::TurnEnded { turn, message: text(p, "last_assistant_message") }],
        Some("Interrupt") => vec![AgentEvent::TurnInterrupted { turn }],
        _ => Vec::new(),
    }
}

/// A raw event kept for diagnosis is cut to this many bytes.
pub const MAX_LOGGED_EVENT_BYTES: usize = 8 * 1024;
const MAX_LOGGED_STRING_CHARS: usize = 1000;

/// A raw event as the diagnosis log keeps it: tool output dropped (D7 keeps none), long strings
/// cut, and the whole cut to [`MAX_LOGGED_EVENT_BYTES`]. The caller scrubs it.
pub fn compact_for_log(v: &Value) -> String {
    fn walk(v: &Value) -> Value {
        match v {
            Value::String(s) if s.chars().count() > MAX_LOGGED_STRING_CHARS => {
                Value::String(s.chars().take(MAX_LOGGED_STRING_CHARS).collect::<String>() + "…")
            }
            Value::Array(a) => Value::Array(a.iter().map(walk).collect()),
            Value::Object(o) => Value::Object(
                o.iter()
                    .filter(|(k, _)| !matches!(k.as_str(), "tool_response" | "toolResponse" | "output"))
                    .map(|(k, v)| (k.clone(), walk(v)))
                    .collect(),
            ),
            v => v.clone(),
        }
    }
    let mut s = walk(v).to_string();
    if s.len() > MAX_LOGGED_EVENT_BYTES {
        let mut cut = MAX_LOGGED_EVENT_BYTES;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push('…');
    }
    s
}

/// The Codex hook events Chorus subscribes to, with each one's timeout in seconds. A hook blocks
/// Codex until it returns, and Chorus only listens, so the timeouts stay short; Interrupt is
/// capped at three seconds by Codex itself.
pub const CODEX_HOOK_EVENTS: [(&str, u32); 6] =
    [("UserPromptSubmit", 5), ("PreToolUse", 5), ("PostToolUse", 5), ("PermissionRequest", 5), ("Stop", 5), ("Interrupt", 3)];

/// The Claude Code hook events Chorus subscribes to (HTTP hooks; `SessionStart` isn't available
/// over HTTP, LESSONS L24).
pub const CLAUDE_HOOK_EVENTS: [&str; 8] =
    ["UserPromptSubmit", "PreToolUse", "PostToolUse", "PostToolUseFailure", "PermissionRequest", "Notification", "Stop", "StopFailure"];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_hooks_become_events() {
        // Field lists as Claude Code 2.1.300 sent them in S5.
        let base = json!({"session_id":"8844cd15-4f1e-46d6-b980-6edca518a4cc","transcript_path":"x.jsonl","cwd":"C:\\work","prompt_id":"p1","permission_mode":"default"});
        let with = |extra: Value| {
            let mut v = base.clone();
            v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            from_claude_hook(&v)
        };
        assert_eq!(
            with(json!({"hook_event_name":"UserPromptSubmit","prompt":"fix it"})),
            [AgentEvent::TurnStarted { turn: Some("p1".into()), prompt: Some("fix it".into()) }]
        );
        assert_eq!(
            with(json!({"hook_event_name":"PreToolUse","tool_name":"Write","tool_use_id":"t1","tool_input":{"file_path":"C:\\work\\a.txt","content":"hi"}})),
            [AgentEvent::ToolStarted { turn: Some("p1".into()), call: Some("t1".into()), tool: "Write".into(), target: Some("C:\\work\\a.txt".into()) }]
        );
        assert_eq!(
            with(json!({"hook_event_name":"PermissionRequest","tool_name":"PowerShell","tool_input":{"command":"dir\n  /s"}})),
            [AgentEvent::ApprovalAsked { turn: Some("p1".into()), ask: None, tool: "PowerShell".into(), target: Some("dir /s".into()) }]
        );
        assert_eq!(
            with(json!({"hook_event_name":"PreToolUse","tool_name":"AskUserQuestion","tool_use_id":"q1","tool_input":{"questions":[{"question":"Which file?"}]}})),
            [AgentEvent::QuestionAsked { turn: Some("p1".into()), ask: Some("q1".into()), question: "Which file?".into() }]
        );
        assert!(with(json!({"hook_event_name":"PermissionRequest","tool_name":"AskUserQuestion","tool_use_id":"q1","tool_input":{"questions":[{"question":"Which file?"}]}})).is_empty());
        assert_eq!(
            with(json!({"hook_event_name":"Stop","stop_hook_active":false,"last_assistant_message":"Done."})),
            [AgentEvent::TurnEnded { turn: Some("p1".into()), message: Some("Done.".into()) }]
        );
        assert_eq!(with(json!({"hook_event_name":"Notification","notification_type":"idle_prompt","message":"waiting"})), [AgentEvent::Idle]);
        // A permission notification repeats PermissionRequest; subagent and task events aren't turns.
        for ignored in ["Notification", "SubagentStop", "TaskCreated", "SessionEnd"] {
            assert!(with(json!({"hook_event_name": ignored, "notification_type":"permission_prompt"})).is_empty(), "{ignored}");
        }
    }

    #[test]
    fn codex_hooks_become_events() {
        let p = |v: Value| from_codex_hook(&v);
        assert_eq!(
            p(json!({"hook_event_name":"UserPromptSubmit","turn_id":"t9","prompt":"go","session_id":"s","cwd":"C:\\w","model":"m","permission_mode":"default"})),
            [AgentEvent::TurnStarted { turn: Some("t9".into()), prompt: Some("go".into()) }]
        );
        assert_eq!(
            p(json!({"hook_event_name":"PermissionRequest","turn_id":"t9","tool_name":"Bash","tool_input":{"command":"Set-Content a.txt hi"}})),
            [AgentEvent::ApprovalAsked { turn: Some("t9".into()), ask: None, tool: "Bash".into(), target: Some("Set-Content a.txt hi".into()) }]
        );
        assert_eq!(p(json!({"hook_event_name":"Interrupt","turn_id":"t9"})), [AgentEvent::TurnInterrupted { turn: Some("t9".into()) }]);
        assert_eq!(
            p(json!({"hook_event_name":"Stop","turn_id":"t9","last_assistant_message":null})),
            [AgentEvent::TurnEnded { turn: Some("t9".into()), message: None }]
        );
        assert!(p(json!({"hook_event_name":"SessionStart"})).is_empty());
    }

    #[test]
    fn logged_events_drop_tool_output_and_stay_small() {
        let v = json!({"hook_event_name":"PostToolUse","tool_response":{"stdout":"secret file contents"},"tool_input":{"content":"x".repeat(5000)}});
        let s = compact_for_log(&v);
        assert!(!s.contains("tool_response") && !s.contains("secret file contents"));
        assert!(s.len() < 1200);
        let big = json!({"a": (0..2000).map(|i| format!("item-{i}")).collect::<Vec<_>>()});
        assert!(compact_for_log(&big).len() <= MAX_LOGGED_EVENT_BYTES + "…".len());
    }

    #[test]
    fn tool_targets_are_short_single_lines() {
        assert_eq!(tool_target(&json!({"command": "a\n b"})).as_deref(), Some("a b"));
        assert_eq!(tool_target(&json!("{\"file_path\":\"x.rs\"}")).as_deref(), Some("x.rs"));
        let long = tool_target(&json!({"command": "x".repeat(400)})).unwrap();
        assert_eq!(long.chars().count(), MAX_TARGET_CHARS + 1);
        assert_eq!(tool_target(&json!({"other": 1})), None);
    }
}
