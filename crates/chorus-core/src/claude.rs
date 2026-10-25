//! What Chorus reads from Claude Code's own config. Chorus writes nothing there (D100). Claude Code
//! asks whether to trust a folder it hasn't seen, and the default answer, No, exits (LESSONS L33).
//! Its config (`.claude.json`) records the answer per folder,
//! `projects["C:/path"].hasTrustDialogAccepted`. Measured on 2.1.310 (L66): inside a git repository,
//! worktrees included, only the main working tree's own entry counts; a plain folder is also trusted
//! through a trusted folder above it.

use serde_json::Value;

use crate::agent::{LaunchOptions, Spec};
use crate::opencode::Observed;

pub const TRUST_NOTE_BEFORE: &str =
    "Claude Code hasn't been told it can trust this folder, so it will ask first. Its default answer is No, which exits: choose Yes.";
pub const TRUST_NOTE_AFTER: &str =
    "Claude Code exited at its question about trusting this folder (its default answer is No). Restart and choose Yes.";

/// Whether a line of Claude Code's transcript records the user interrupting a turn. Claude Code
/// sends no hook for that (LESSONS L24); its transcript gets a user message
/// `[Request interrupted by user]` (2.1.300).
pub fn transcript_line_interrupts(line: &str) -> bool {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return false };
    if v.get("type").and_then(Value::as_str) != Some("user") {
        return false;
    }
    let starts = |t: &str| t.starts_with("[Request interrupted by user");
    match v.pointer("/message/content") {
        Some(Value::String(s)) => starts(s),
        Some(Value::Array(parts)) => parts.iter().any(|p| p.get("text").and_then(Value::as_str).is_some_and(starts)),
        _ => false,
    }
}

/// What a transcript line says a turn ran with, and when: the model and effort of a reply on the
/// main thread (a subagent's reply is its own). Claude Code 2.1.308 records both on every reply
/// (`message.model`, `effort`); a reply it made up for an error says `<synthetic>`.
pub fn reply_ran(line: &str) -> Option<(i64, Observed)> {
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("type").and_then(Value::as_str) != Some("assistant") || v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let model = v.pointer("/message/model").and_then(Value::as_str).filter(|m| !m.is_empty() && !m.starts_with('<'))?;
    let at = v.get("timestamp").and_then(Value::as_str).and_then(crate::codex::parse_utc_ms)?;
    let effort = v.get("effort").and_then(Value::as_str).map(str::to_owned);
    Some((at, Observed { model: model.to_owned(), variant: effort, agent: None }))
}

/// How a turn differs from what the session asked for, one phrase each; empty when it matches. A
/// family name (`opus`, `sonnet`, `haiku`) matches any model of that family and a full model id
/// only itself; other names Claude Code resolves by its own rules (`opusplan`) aren't compared.
/// Effort is compared only when the transcript records one.
pub fn differences(spec: &Spec, o: &LaunchOptions, seen: &Observed) -> Vec<String> {
    let mut out = Vec::new();
    // A context-size suffix (`sonnet[1m]`) doesn't change the model asked for.
    let base = |m: &str| m.split('[').next().unwrap_or_default().to_owned();
    if let Some(asked) = o.model.as_deref() {
        let wanted = base(asked);
        let matches = if ["opus", "sonnet", "haiku"].contains(&wanted.as_str()) {
            Some(seen.model.split('-').any(|part| part == wanted))
        } else if wanted.starts_with("claude-") {
            Some(base(&seen.model) == wanted)
        } else {
            None
        };
        if matches == Some(false) {
            out.push(format!("ran {}, not {asked}", seen.model));
        }
    }
    let wanted = o.effort.and_then(|e| spec.effort_args(e)).and_then(|args| args.last().copied());
    if let (Some(wanted), Some(ran)) = (wanted, seen.variant.as_deref())
        && ran != wanted
    {
        out.push(format!("ran with effort {ran}, not {wanted}"));
    }
    out
}

fn norm(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

/// Whether Claude Code trusts a plain folder (not in a git repository): `cwd` or a folder above it.
/// `None` when the config can't be read.
pub fn folder_trusted(config: &str, cwd: &str) -> Option<bool> {
    let v: Value = serde_json::from_str(config).ok()?;
    let Some(projects) = v.get("projects").and_then(Value::as_object) else { return Some(false) };
    let trusted: Vec<String> = projects
        .iter()
        .filter(|(_, p)| p.get("hasTrustDialogAccepted").and_then(Value::as_bool) == Some(true))
        .map(|(k, _)| norm(k))
        .collect();
    let mut here = norm(cwd);
    loop {
        if trusted.contains(&here) {
            return Some(true);
        }
        match here.rfind('/') {
            Some(i) => here.truncate(i),
            None => return Some(false),
        }
    }
}

/// Whether Claude Code trusts a git repository, and so its worktrees: the main working tree's own
/// entry, not a trusted folder above it. `None` when the config can't be read.
pub fn repo_trusted(config: &str, main_root: &str) -> Option<bool> {
    let v: Value = serde_json::from_str(config).ok()?;
    let Some(projects) = v.get("projects").and_then(Value::as_object) else { return Some(false) };
    let want = norm(main_root);
    Some(projects.iter().any(|(k, p)| norm(k) == want && p.get("hasTrustDialogAccepted").and_then(Value::as_bool) == Some(true)))
}

/// Whether Claude Code will start in `cwd` without asking: through its repository's main working
/// tree when `cwd` is in one (`main_root`), else through `cwd` or a folder above it.
pub fn trusted_at(config: &str, cwd: &str, main_root: Option<&str>) -> Option<bool> {
    match main_root {
        Some(root) => repo_trusted(config, root),
        None => folder_trusted(config, cwd),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repository_needs_its_own_entry() {
        let config = r#"{"projects":{"C:/tmp/p15":{"hasTrustDialogAccepted":true},"C:/tmp/p15/b":{"hasTrustDialogAccepted":true}}}"#;
        // A repository inside a trusted folder, and a worktree of it: both asked (2.1.310).
        assert_eq!(trusted_at(config, r"C:\tmp\p15\a", Some(r"C:\tmp\p15\a")), Some(false));
        assert_eq!(trusted_at(config, r"C:\tmp\p15\.chorus2\a\x", Some(r"C:\tmp\p15\a")), Some(false));
        // Its main working tree trusted: every worktree is.
        assert_eq!(trusted_at(config, r"C:\tmp\p15\.chorus2\b\x", Some(r"c:\TMP\p15\b\")), Some(true));
        // A plain folder inside a trusted one isn't asked.
        assert_eq!(trusted_at(config, r"C:\tmp\p15\plain", None), Some(true));
        assert_eq!(repo_trusted("{}", r"C:\x"), Some(false));
        assert_eq!(repo_trusted("nope", r"C:\x"), None);
    }

    const CONFIG: &str = r#"{"projects":{
        "C:/Users/a/work": {"hasTrustDialogAccepted": true},
        "C:/Users/a/scratch": {"hasTrustDialogAccepted": false},
        "D:/other": {}}}"#;

    #[test]
    fn a_folder_or_any_folder_above_it_can_be_trusted() {
        assert_eq!(folder_trusted(CONFIG, r"C:\Users\a\work"), Some(true));
        assert_eq!(folder_trusted(CONFIG, r"c:\users\A\Work\app\src\"), Some(true));
        assert_eq!(folder_trusted(CONFIG, r"C:\Users\a\scratch"), Some(false));
        assert_eq!(folder_trusted(CONFIG, r"C:\Users\a\workshop"), Some(false), "a sibling with a longer name isn't inside it");
        assert_eq!(folder_trusted(CONFIG, r"D:\other"), Some(false));
        assert_eq!(folder_trusted("{}", r"C:\x"), Some(false));
        assert_eq!(folder_trusted("not json", r"C:\x"), None);
    }

    #[test]
    fn an_interrupt_is_spotted_in_the_transcript() {
        assert!(transcript_line_interrupts(
            r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#
        ));
        assert!(transcript_line_interrupts(r#"{"type":"user","message":{"content":"[Request interrupted by user for tool use]"}}"#));
        assert!(!transcript_line_interrupts(r#"{"type":"user","message":{"content":"please [Request interrupted by user]"}}"#));
        assert!(!transcript_line_interrupts(r#"{"type":"assistant","message":{"content":"[Request interrupted by user]"}}"#));
        assert!(!transcript_line_interrupts("{partial"));
    }

    #[test]
    fn a_reply_says_which_model_and_effort_ran() {
        // Shaped like Claude Code 2.1.308's reply lines, without their content.
        let mut v = serde_json::json!({"type":"assistant","isSidechain":false,"timestamp":"2026-10-24T23:18:50.915Z",
            "requestedModel":"claude-opus-5-5","effort":"xhigh","message":{"model":"claude-opus-5-5","role":"assistant","content":[]}});
        let (at, seen) = reply_ran(&v.to_string()).unwrap();
        assert_eq!(at, crate::codex::parse_utc_ms("2026-10-24T23:18:50.915Z").unwrap());
        assert_eq!((seen.model.as_str(), seen.variant.as_deref(), seen.agent), ("claude-opus-5-5", Some("xhigh"), None));
        v.as_object_mut().unwrap().remove("effort");
        assert_eq!(reply_ran(&v.to_string()).unwrap().1.variant, None, "an older transcript without effort");
        v["isSidechain"] = serde_json::json!(true);
        assert!(reply_ran(&v.to_string()).is_none(), "a subagent's reply");
        v["isSidechain"] = serde_json::json!(false);
        v["message"]["model"] = serde_json::json!("<synthetic>");
        assert!(reply_ran(&v.to_string()).is_none());
        assert!(reply_ran(r#"{"type":"user","timestamp":"2026-10-24T23:18:50Z","message":{"model":"x"}}"#).is_none());
        assert!(reply_ran("{partial").is_none());
    }

    #[test]
    fn a_turn_is_compared_with_the_model_and_effort_asked_for() {
        use crate::agent::{AgentKind, Effort};
        let spec = AgentKind::Claude.spec();
        let asked = |model: &str, effort: Option<Effort>| LaunchOptions { model: Some(model.into()), effort, ..Default::default() };
        let seen = |model: &str, effort: Option<&str>| Observed { model: model.into(), variant: effort.map(str::to_owned), agent: None };
        assert!(differences(spec, &asked("opus", Some(Effort::Deep)), &seen("claude-opus-5-5", Some("xhigh"))).is_empty());
        assert_eq!(differences(spec, &asked("opus", Some(Effort::Deep)), &seen("claude-sonnet-5-5", Some("medium"))), [
            "ran claude-sonnet-5-5, not opus",
            "ran with effort medium, not xhigh"
        ]);
        assert!(differences(spec, &asked("opus", Some(Effort::Deep)), &seen("claude-opus-5-5", None)).is_empty(), "no effort recorded");
        assert!(differences(spec, &asked("sonnet[1m]", None), &seen("claude-sonnet-5-5", Some("high"))).is_empty());
        assert!(differences(spec, &asked("claude-haiku-5-5", None), &seen("claude-haiku-5-5", None)).is_empty());
        assert_eq!(differences(spec, &asked("claude-haiku-5-5", None), &seen("claude-haiku-5-4", None)), ["ran claude-haiku-5-4, not claude-haiku-5-5"]);
        assert!(differences(spec, &asked("opusplan", None), &seen("claude-sonnet-5-5", None)).is_empty(), "Claude Code's own rule");
    }
}
