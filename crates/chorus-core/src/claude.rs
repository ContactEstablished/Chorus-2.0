//! What Chorus reads from Claude Code's own config, never writes. Claude Code 2.1.300 asks whether
//! to trust a folder it hasn't seen, and the default answer, No, exits (LESSONS L33). Its config
//! (`.claude.json`) records the answer per folder, `projects["C:/path"].hasTrustDialogAccepted`,
//! and a folder inside a trusted one isn't asked (measured on 2.1.300).

use serde_json::Value;

pub const TRUST_NOTE_BEFORE: &str =
    "Claude Code hasn't been told it can trust this folder, so it will ask first. Its default answer is No, which exits: choose Yes.";
pub const TRUST_NOTE_AFTER: &str =
    "Claude Code exited at its question about trusting this folder (its default answer is No). Restart and choose Yes.";

fn norm(path: &str) -> String {
    path.replace('\\', "/").trim_end_matches('/').to_lowercase()
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
}
