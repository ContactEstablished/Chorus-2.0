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
    claude_settings_allowing(url, &[])
}

/// [`claude_settings`], with `allow` added to its permission rules: Chorus's own tools run
/// without asking (D135).
pub fn claude_settings_allowing(url: &str, allow: &[&str]) -> String {
    let hook = json!([{
        "hooks": [{
            "type": "http",
            "url": url,
            "headers": { "Authorization": format!("Bearer ${TOKEN_VAR}") },
            "allowedEnvVars": [TOKEN_VAR],
            "timeout": CLAUDE_HOOK_TIMEOUT_SECS,
        }]
    }]);
    let hooks: serde_json::Map<String, Value> = CLAUDE_HOOK_EVENTS.iter().map(|e| {
        let mut entry = hook.clone();
        if matches!(*e, "PermissionRequest" | "PreToolUse") {
            entry[0]["hooks"][0]["timeout"] = json!(crate::replies::REPLY_WAIT_SECS + 5);
        }
        ((*e).to_owned(), entry)
    }).collect();
    let mut settings = json!({ "hooks": hooks });
    if !allow.is_empty() {
        settings["permissions"] = json!({ "allow": allow });
    }
    settings.to_string()
}

/// Chorus's MCP server for one interactive session (D135): its tools at
/// `/mcp/session/<id>` on the listener, with the session's hook token in a header that Claude
/// Code fills from its environment (the form spike S3 checked for loop steps).
pub fn session_mcp_config(port: u16, session_id: u32) -> String {
    json!({"mcpServers": {"chorus": {
        "type": "http",
        "url": format!("http://127.0.0.1:{port}/mcp/session/{session_id}"),
        "headers": {"Authorization": format!("Bearer ${{{TOKEN_VAR}}}")},
    }}})
    .to_string()
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
    let relay = relay.replace('\\', "/");
    // Unquoted when it can be: Codex may run hooks under PowerShell, where a quoted path followed
    // by an argument is a parse error.
    let command = toml_string(&if relay.contains(char::is_whitespace) { format!("\"{relay}\" {url}") } else { format!("{relay} {url}") });
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
    fn chorus_tools_can_be_allowed_and_the_session_server_takes_the_token_from_the_environment() {
        let plain: Value = serde_json::from_str(&claude_settings("u")).unwrap();
        assert!(plain.get("permissions").is_none());
        let s: Value = serde_json::from_str(&claude_settings_allowing("u", &["mcp__chorus__maus_send"])).unwrap();
        assert_eq!(s["permissions"]["allow"][0], "mcp__chorus__maus_send");
        let m: Value = serde_json::from_str(&session_mcp_config(4100, 7)).unwrap();
        let server = &m["mcpServers"]["chorus"];
        assert_eq!(server["url"], "http://127.0.0.1:4100/mcp/session/7");
        assert_eq!(server["headers"]["Authorization"], "Bearer ${CHORUS_HOOK_TOKEN}");
        assert!(!m.to_string().contains("omb_"), "no key, only the variable's name");
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
        // Without spaces, no quotes: PowerShell can't parse a quoted path followed by an argument.
        let plain = codex_hook_args(r"C:\Chorus\chorus-hook.exe", "http://127.0.0.1:4100/hook/7");
        assert!(plain[1].contains(r#"command="C:/Chorus/chorus-hook.exe http://127.0.0.1:4100/hook/7""#), "{}", plain[1]);
        assert_eq!(args.len(), CODEX_HOOK_EVENTS.len() * 2 + 1);
    }
}
