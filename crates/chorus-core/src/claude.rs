//! What Chorus reads from Claude Code's own config, and the one thing it writes there. Claude Code
//! 2.1.300 asks whether to trust a folder it hasn't seen, and the default answer, No, exits (LESSONS
//! L33). Its config (`.claude.json`) records the answer per folder,
//! `projects["C:/path"].hasTrustDialogAccepted`, and a folder inside a trusted one isn't asked
//! (measured on 2.1.300). A worktree is always a new folder, so Chorus records trust for the folder
//! its worktrees live in, and only when Claude Code already trusts their repository (D55).

use serde_json::Value;

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

fn norm(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

/// How Claude Code names a folder in its config: forward slashes, no trailing one.
pub fn project_key(folder: &str) -> String {
    folder.trim().replace('\\', "/").trim_end_matches('/').to_owned()
}

/// The config with `folder` marked trusted and everything else kept, as Claude Code writes it
/// (two-space indent, no final newline). `None` when the config isn't a JSON object, or its
/// `projects` or that folder's entry isn't one.
pub fn with_folder_trusted(config: &str, folder: &str) -> Option<String> {
    let mut v: Value = serde_json::from_str(config).ok()?;
    let projects = v.as_object_mut()?.entry("projects").or_insert_with(|| Value::Object(Default::default()));
    let entry = projects.as_object_mut()?.entry(project_key(folder)).or_insert_with(|| Value::Object(Default::default()));
    entry.as_object_mut()?.insert("hasTrustDialogAccepted".into(), Value::Bool(true));
    serde_json::to_string_pretty(&v).ok()
}

/// Whether Claude Code trusts `cwd` or a folder above it. `None` when the config can't be read.
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn trusting_a_folder_keeps_everything_else() {
        let config = r#"{"numStartups":12,"projects":{"C:/Users/a/work":{"hasTrustDialogAccepted":true,"allowedTools":["Bash"]},
            "C:/Users/a/.chorus2/ledger":{"allowedTools":[],"hasTrustDialogAccepted":false}},"theme":"dark"}"#;
        let before: Value = serde_json::from_str(config).unwrap();
        let after_text = with_folder_trusted(config, r"C:\Users\a\.chorus2\ledger\").unwrap();
        assert!(after_text.starts_with("{\n  \"") && !after_text.ends_with('\n'), "Claude Code's own layout");
        let mut after: Value = serde_json::from_str(&after_text).unwrap();
        let entry = after.pointer_mut("/projects/C:~1Users~1a~1.chorus2~1ledger").unwrap();
        assert_eq!(entry["hasTrustDialogAccepted"], Value::Bool(true));
        assert_eq!(entry["allowedTools"], serde_json::json!([]), "its other fields stay");
        entry["hasTrustDialogAccepted"] = Value::Bool(false);
        assert_eq!(after, before, "nothing else changed");
        assert_eq!(folder_trusted(&with_folder_trusted("{}", r"D:\x").unwrap(), r"D:\x\wt\1a2b"), Some(true));
        assert_eq!(with_folder_trusted("[]", r"D:\x"), None);
        assert_eq!(with_folder_trusted(r#"{"projects":[]}"#, r"D:\x"), None);
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
}
