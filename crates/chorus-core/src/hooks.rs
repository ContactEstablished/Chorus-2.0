//! How each agent is told to send its hook events to Chorus's loopback listener (ARCHITECTURE §5,
//! D36, D37). The session's token travels only in the child's environment as [`TOKEN_VAR`]: Claude
//! Code expands it into a header itself, and `chorus-hook` reads it for Codex. The settings file
//! and argv hold the address only.

use serde_json::{Value, json};

use crate::events::{CLAUDE_HOOK_EVENTS, CODEX_HOOK_EVENTS};

pub const TOKEN_VAR: &str = "CHORUS_HOOK_TOKEN";
/// How long Claude Code waits for an HTTP hook before going on without it.
pub const CLAUDE_HOOK_TIMEOUT_SECS: u32 = 5;

/// The listener's address for one session.
pub fn hook_url(port: u16, session_id: u32) -> String {
    format!("http://127.0.0.1:{port}/hook/{session_id}")
}

/// Claude Code's settings file (passed with `--settings <file>`, LESSONS L1): an HTTP hook per
/// event, with the token in a header filled from the child's environment (`allowedEnvVars`).
pub fn claude_settings(url: &str) -> String {
    let hook = json!([{
        "hooks": [{
            "type": "http",
            "url": url,
            "headers": { "Authorization": format!("Bearer ${TOKEN_VAR}") },
            "allowedEnvVars": [TOKEN_VAR],
            "timeout": CLAUDE_HOOK_TIMEOUT_SECS,
        }]
    }]);
    let hooks: serde_json::Map<String, Value> = CLAUDE_HOOK_EVENTS.iter().map(|e| ((*e).to_owned(), hook.clone())).collect();
    json!({ "hooks": hooks }).to_string()
}

/// A TOML basic string.
fn toml_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Codex's arguments: a command hook per event running `"<relay>" <url>` (Codex runs it under
/// `cmd`, so the path is quoted for spaces), and the switch that runs hooks it hasn't been asked to
/// trust (LESSONS L25). They go before any `resume` subcommand. The path uses forward slashes:
/// Codex 0.162.1 unescapes a `-c` value's backslashes twice, which turned
/// `C:\Projects\x.exe` into `C:Projectsx.exe` (seen in the real app; LESSONS L37).
pub fn codex_hook_args(relay: &str, url: &str) -> Vec<String> {
    let command = toml_string(&format!("\"{}\" {url}", relay.replace('\\', "/")));
    let mut args: Vec<String> = CODEX_HOOK_EVENTS
        .iter()
        .flat_map(|(event, timeout)| ["-c".to_owned(), format!("hooks.{event}=[{{hooks=[{{type=\"command\",command={command},timeout={timeout}}}]}}]")])
        .collect();
    args.push("--dangerously-bypass-hook-trust".into());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_gets_a_header_from_its_environment_not_the_token() {
        let s: Value = serde_json::from_str(&claude_settings(&hook_url(4100, 7))).unwrap();
        let stop = &s["hooks"]["Stop"][0]["hooks"][0];
        assert_eq!(stop["type"], "http");
        assert_eq!(stop["url"], "http://127.0.0.1:4100/hook/7");
        assert_eq!(stop["headers"]["Authorization"], "Bearer $CHORUS_HOOK_TOKEN");
        assert_eq!(stop["allowedEnvVars"][0], "CHORUS_HOOK_TOKEN");
        assert_eq!(s["hooks"].as_object().unwrap().len(), CLAUDE_HOOK_EVENTS.len());
    }

    #[test]
    fn codex_runs_the_relay_with_a_quoted_path() {
        let args = codex_hook_args(r"C:\Program Files\Chorus\chorus-hook.exe", "http://127.0.0.1:4100/hook/7");
        assert_eq!(args.last().unwrap(), "--dangerously-bypass-hook-trust");
        assert_eq!(args[0], "-c");
        assert_eq!(
            args[1],
            r#"hooks.UserPromptSubmit=[{hooks=[{type="command",command="\"C:/Program Files/Chorus/chorus-hook.exe\" http://127.0.0.1:4100/hook/7",timeout=5}]}]"#
        );
        assert!(args.iter().any(|a| a.starts_with("hooks.Interrupt=") && a.ends_with("timeout=3}]}]")));
        assert_eq!(args.len(), CODEX_HOOK_EVENTS.len() * 2 + 1);
    }
}
