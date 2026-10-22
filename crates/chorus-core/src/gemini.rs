//! Gemini CLI's chat records, as far as Chorus needs them (P14). The CLI keeps one JSONL file per
//! session under `<home>\.gemini\tmp\<project>\chats\`, and decides whether it can resume a session
//! from the record itself (Gemini CLI 0.64.0, `loadConversationRecord`): a chat is resumable when
//! it holds a user message that isn't one of its own markers, or a model message with anything in
//! it. A session that ended before its first prompt has neither, and `--resume` on it fails with
//! exit 42, while `--session-id` on the same id fails because the file already exists. So Chorus
//! has to know which of the three states a chat is in before it picks an argument.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TrustLevel {
    TrustFolder,
    TrustParent,
    DoNotTrust,
}

/// A rule read from Gemini 0.64.0's trustedFolders.json. The shell resolves `path` to its real
/// effective folder (the parent for TRUST_PARENT) before matching. Specificity uses the original
/// rule's length, as Gemini does, even when the effective folder is its parent.
#[derive(Debug)]
pub struct TrustRule {
    pub path: String,
    pub level: TrustLevel,
    pub specificity: usize,
}

pub fn trust_rules(config: &str) -> Option<Vec<TrustRule>> {
    let rules: std::collections::BTreeMap<String, TrustLevel> = serde_json::from_str(config).ok()?;
    Some(rules.into_iter().map(|(path, level)| TrustRule { specificity: path.encode_utf16().count(), path, level }).collect())
}

/// The most specific matching rule wins. An explicit refusal wins an ambiguous tie, so Chorus
/// never widens trust because two rules differ only in case or path spelling.
pub fn folder_trust(rules: &[TrustRule], cwd: &str) -> Option<bool> {
    rules.iter().filter(|r| crate::folders::inside(cwd, &r.path))
        .max_by_key(|r| (r.specificity, r.level == TrustLevel::DoNotTrust))
        .map(|r| r.level != TrustLevel::DoNotTrust)
}

/// A managed worktree may inherit its repository's trust; a refusal on the actual worktree
/// always takes precedence. An unrecognised or absent rule never grants trust.
pub fn launch_trusted(rules: &[TrustRule], cwd: &str, repository: Option<&str>) -> bool {
    folder_trust(rules, cwd).or_else(|| repository.and_then(|p| folder_trust(rules, p))) == Some(true)
}

/// What the CLI would find in a chat file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatState {
    /// No usable record: the id is free to create.
    Empty,
    /// At least one turn: `--resume <id>` continues it.
    Resumable,
}

/// The text of a message's content, whether it is a list of parts or a plain string.
fn text_of(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect(),
        _ => String::new(),
    }
}

/// A user record the CLI ignores when deciding what to resume: blank, a slash command, a `?` help
/// line, or the context blocks it writes itself.
fn ignored_user(text: &str) -> bool {
    let t = text.trim();
    t.is_empty() || t.starts_with('/') || t.starts_with('?') || t.starts_with("<session_context>") || t.starts_with("<hook_context>")
}

/// Whether a chat record holds a turn the CLI will resume. Lines that aren't JSON (its `$set`
/// patches) and its metadata header are skipped.
pub fn chat_state(record: &str) -> ChatState {
    let resumable = record.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).any(|v| match v.get("type").and_then(Value::as_str) {
        Some("user") => !ignored_user(&text_of(v.get("content"))),
        Some("gemini") => {
            !text_of(v.get("content")).trim().is_empty()
                || v.get("toolCalls").and_then(Value::as_array).is_some_and(|a| !a.is_empty())
                || v.get("thoughts").and_then(Value::as_array).is_some_and(|a| !a.is_empty())
        }
        _ => false,
    });
    if resumable { ChatState::Resumable } else { ChatState::Empty }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_trust_requires_a_trusted_repository_and_respects_refusals() {
        let mut rules = trust_rules(r#"{"C:/work/repo":"TRUST_FOLDER","C:/work/repo/private":"DO_NOT_TRUST"}"#).unwrap();
        assert_eq!(folder_trust(&rules, r"c:\work\repo\src"), Some(true));
        assert_eq!(folder_trust(&rules, r"C:\work\repo\private\src"), Some(false));
        assert_eq!(folder_trust(&rules, r"C:\work\repo-other"), None);
        let wt = "C:/work/.chorus2/repo/1234";
        assert!(!launch_trusted(&rules, wt, None));
        assert!(launch_trusted(&rules, wt, Some("C:/work/repo")));
        rules.extend(trust_rules(r#"{"C:/work/.chorus2":"DO_NOT_TRUST"}"#).unwrap());
        assert!(!launch_trusted(&rules, wt, Some("C:/work/repo")));
        assert!(trust_rules("not json").is_none());
        assert!(trust_rules(r#"{"C:/work":"UNKNOWN"}"#).is_none());
        assert!(trust_rules("[]").is_none());
    }

    #[test]
    fn parent_trust_uses_the_original_rules_specificity() {
        let mut rules = trust_rules(r#"{"C:/work/repo":"DO_NOT_TRUST","C:/work/repo/child":"TRUST_PARENT"}"#).unwrap();
        let parent = rules.iter_mut().find(|r| r.level == TrustLevel::TrustParent).unwrap();
        parent.path = "C:/work/repo".into(); // The shell resolves the parent before matching.
        assert_eq!(folder_trust(&rules, "C:/work/repo/other"), Some(true));
    }

    // The record Gemini CLI 0.64.0 wrote for a session that exited before its first prompt
    // (trimmed; the full one is 1,316 bytes).
    const UNUSED: &str = concat!(
        r#"{"sessionId":"0f8fad5b-d9cb-469f-a165-70867728950e","projectHash":"20a3","startTime":"2026-10-20T18:08:27.483Z","lastUpdated":"2026-10-20T18:08:27.483Z","kind":"main"}"#,
        "\n",
        r#"{"id":"d0","timestamp":"2026-10-20T18:08:27.486Z","type":"user","content":[{"text":"<session_context>\nThis is the Gemini CLI. We are setting up the context for our chat.\nToday's date is Tuesday"}]}"#,
        "\n",
        r#"{"$set":{"lastUpdated":"2026-10-20T18:08:27.488Z"}}"#,
    );

    #[test]
    fn a_chat_with_no_turn_is_empty() {
        assert_eq!(chat_state(UNUSED), ChatState::Empty);
        assert_eq!(chat_state(""), ChatState::Empty);
        assert_eq!(chat_state("not json\n"), ChatState::Empty);
        // The CLI's own markers don't count as a turn.
        let slash = r#"{"type":"user","content":[{"text":"/help"}]}"#;
        assert_eq!(chat_state(slash), ChatState::Empty);
        let hook = r#"{"type":"user","content":[{"text":"<hook_context>x</hook_context>"}]}"#;
        assert_eq!(chat_state(hook), ChatState::Empty);
    }

    #[test]
    fn a_prompt_or_a_reply_makes_it_resumable() {
        let prompt = format!("{UNUSED}\n{}", r#"{"id":"u1","type":"user","content":[{"text":"list the files"}]}"#);
        assert_eq!(chat_state(&prompt), ChatState::Resumable);
        let reply = format!("{UNUSED}\n{}", r#"{"id":"g1","type":"gemini","content":[{"text":"Here they are."}]}"#);
        assert_eq!(chat_state(&reply), ChatState::Resumable);
        let tool = format!("{UNUSED}\n{}", r#"{"id":"g2","type":"gemini","content":[],"toolCalls":[{"name":"ls"}]}"#);
        assert_eq!(chat_state(&tool), ChatState::Resumable);
        // A plain-string content is the older shape.
        assert_eq!(chat_state(r#"{"type":"user","content":"hello"}"#), ChatState::Resumable);
    }
}
