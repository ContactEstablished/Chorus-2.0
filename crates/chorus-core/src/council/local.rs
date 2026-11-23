//! Council members that run on a local subscription (D140) rather than through OpenRouter:
//! Claude Code (`claude -p`) or Codex (`codex exec`), headless. Every built-in tool that could
//! read a file is switched off, and Chorus's three read-only tools (D137) are offered over MCP
//! instead, so a local member sees the code exactly as an OpenRouter one does: blocked names
//! left out, saved keys scrubbed, the same limits. Verified against Claude Code 2.1.330 and
//! Codex 0.169.0 (LESSONS L87, L88). Pure: the arguments, the config and the output parsing.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;

use super::tools;

/// Where a member's turns run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum RunsOn {
    /// OpenRouter's API, paid from the run's capped key.
    #[default]
    Openrouter,
    /// The local Claude Code, on its own sign-in (a Claude subscription).
    Claude,
    /// The local Codex, on its own sign-in (a ChatGPT subscription).
    Codex,
}

impl RunsOn {
    pub fn key(self) -> &'static str {
        match self {
            Self::Openrouter => "openrouter",
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Openrouter, Self::Claude, Self::Codex].into_iter().find(|r| r.key() == s)
    }

    pub fn is_local(self) -> bool {
        self != Self::Openrouter
    }

    /// How the screens name it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Openrouter => "OpenRouter",
            Self::Claude => "Claude subscription",
            Self::Codex => "ChatGPT subscription",
        }
    }
}

/// The child's environment variable that carries the turn's MCP token. The token is never in
/// argv or a file: Claude's config names the variable, and Codex reads it by name.
pub const TOKEN_ENV: &str = "CHORUS_COUNCIL_TOKEN";
/// The MCP server's name, so the tools are `mcp__council__read_file` and so on.
pub const SERVER: &str = "council";
/// The versions these arguments were verified against. Others run, with a note (CLAUDE.md:
/// "try and report").
pub const CLAUDE_VERIFIED: &str = "2.1.330";
pub const CODEX_VERIFIED: &str = "0.169.0";

/// Codex features that are switched off for a council turn. `shell_tool` and `unified_exec` are
/// its shell: Codex's read-only sandbox still lets the shell read any file on the computer
/// (L87). The rest are tools a council turn has no use for. Codex refuses to start when one of
/// these names is unknown, so a rename fails closed.
pub const CODEX_DISABLED: &[&str] = &[
    "shell_tool",
    "unified_exec",
    "view_image",
    "apps",
    "plugins",
    "browser_use",
    "computer_use",
    "multi_agent",
    "multi_agent_v2",
    "goals",
    "image_generation",
    "memories",
];

/// The three tools in MCP's shape, from the same definitions OpenRouter members get.
pub fn mcp_tools() -> Value {
    let defs = tools::definitions();
    Value::Array(
        defs.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|d| d.get("function"))
                    .map(|f| json!({"name": f["name"], "description": f["description"], "inputSchema": f["parameters"]}))
                    .collect()
            })
            .unwrap_or_default(),
    )
}

/// The turn's MCP endpoint on Chorus's loopback listener.
pub fn mcp_url(port: u16, turn: u32) -> String {
    format!("http://127.0.0.1:{port}/mcp/council/{turn}")
}

/// Claude's `--mcp-config` file: the one server, its token taken from the environment.
pub fn claude_mcp_config(url: &str) -> Value {
    json!({"mcpServers": {SERVER: {"type": "http", "url": url, "headers": {"Authorization": format!("Bearer ${{{TOKEN_ENV}}}")}}}})
}

/// Arguments for `claude`, after the program. The prompt goes on stdin. `--tools ""` leaves no
/// built-in tool; `--restricted` and empty setting sources keep the user's settings, hooks and
/// plugins out; `--strict-mcp-config` keeps their MCP servers out. `--mcp-config` takes several
/// values (L83), so an option follows it.
pub fn claude_args(model: &str, mcp_config_path: &str) -> Vec<String> {
    let allowed = tools_named(&format!("mcp__{SERVER}__")).join(",");
    [
        "-p",
        "--output-format",
        "json",
        "--no-session-persistence",
        "--restricted",
        "--tools",
        "",
        "--strict-mcp-config",
        "--mcp-config",
        mcp_config_path,
        "--allowedTools",
        &allowed,
        "--setting-sources",
        "",
        "--disable-slash-commands",
        "--model",
        model,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

/// Arguments for `codex`, after the program and its launch arguments. The prompt goes on stdin
/// (`-`). `dir` is an empty working folder: the code is reached only through the tools.
pub fn codex_args(model: &str, url: &str, dir: &str) -> Vec<String> {
    let mut a: Vec<String> = ["exec", "--json", "--ephemeral", "--skip-git-repo-check", "--ignore-user-config", "--ignore-rules", "-s", "read-only", "-C", dir, "-m", model]
        .into_iter()
        .map(str::to_owned)
        .collect();
    for f in CODEX_DISABLED {
        a.extend(["--disable".to_owned(), (*f).to_owned()]);
    }
    for c in [
        "web_search=\"disabled\"".to_owned(),
        format!("mcp_servers.{SERVER}.url=\"{url}\""),
        format!("mcp_servers.{SERVER}.bearer_token_env_var=\"{TOKEN_ENV}\""),
        // `codex exec` never asks, so an unapproved tool call is refused (L87).
        format!("mcp_servers.{SERVER}.default_tools_approval_mode=\"approve\""),
    ] {
        a.extend(["-c".to_owned(), c]);
    }
    a.push("-".to_owned());
    a
}

fn tools_named(prefix: &str) -> Vec<String> {
    mcp_tools().as_array().map(|a| a.iter().filter_map(|t| t["name"].as_str()).map(|n| format!("{prefix}{n}")).collect()).unwrap_or_default()
}

/// What a local turn produced.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reply {
    /// The answer, or `None` with `problem` saying why there is none.
    pub text: Option<String>,
    pub problem: Option<String>,
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
    pub tokens_cached: Option<u64>,
}

/// Claude's `--output-format json` result (one object).
pub fn parse_claude(stdout: &str) -> Reply {
    let Some(v) = stdout.lines().rev().find_map(|l| serde_json::from_str::<Value>(l.trim()).ok().filter(|v| v.get("type").and_then(Value::as_str) == Some("result")))
    else {
        return Reply { problem: Some("Claude Code gave no result.".into()), ..Reply::default() };
    };
    let u = &v["usage"];
    let n = |k: &str| u.get(k).and_then(Value::as_u64);
    let tokens_in = match (n("input_tokens"), n("cache_creation_input_tokens"), n("cache_read_input_tokens")) {
        (None, None, None) => None,
        (a, b, c) => Some(a.unwrap_or(0) + b.unwrap_or(0) + c.unwrap_or(0)),
    };
    let text = v.get("result").and_then(Value::as_str).map(str::trim).unwrap_or("").to_owned();
    let failed = v.get("is_error").and_then(Value::as_bool).unwrap_or(false) || v.get("subtype").and_then(Value::as_str).is_some_and(|s| s != "success");
    let (text, problem) = match (failed, text.is_empty()) {
        (true, _) => (None, Some(if text.is_empty() { "Claude Code reported an error.".to_owned() } else { format!("Claude Code: {text}") })),
        (false, true) => (None, Some("The model returned an empty answer.".to_owned())),
        (false, false) => (Some(text), None),
    };
    Reply { text, problem, tokens_in, tokens_out: n("output_tokens"), tokens_cached: n("cache_read_input_tokens") }
}

/// One line of Codex's `--json` stream, as far as a council turn cares.
#[derive(Debug, Clone, PartialEq)]
pub enum CodexEvent {
    /// It started running something other than Chorus's tools: a command, a file change or a
    /// web search. The shell is switched off, so this means the switches no longer hold; the
    /// turn is stopped (fail closed).
    Forbidden(String),
    Message(String),
    Usage { tokens_in: Option<u64>, tokens_out: Option<u64>, tokens_cached: Option<u64> },
    Failed(String),
    Other,
}

pub fn codex_event(line: &str) -> CodexEvent {
    let Ok(v) = serde_json::from_str::<Value>(line.trim()) else {
        return CodexEvent::Other;
    };
    let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
    let item = &v["item"];
    let item_kind = item.get("type").and_then(Value::as_str).unwrap_or("");
    match kind {
        "item.started" | "item.completed" if matches!(item_kind, "command_execution" | "file_change" | "web_search") => {
            CodexEvent::Forbidden(item_kind.to_owned())
        }
        "item.completed" if item_kind == "agent_message" => CodexEvent::Message(item.get("text").and_then(Value::as_str).unwrap_or("").to_owned()),
        "turn.completed" => {
            let u = &v["usage"];
            let n = |k: &str| u.get(k).and_then(Value::as_u64);
            CodexEvent::Usage { tokens_in: n("input_tokens"), tokens_out: n("output_tokens"), tokens_cached: n("cached_input_tokens") }
        }
        "turn.failed" => CodexEvent::Failed(v.pointer("/error/message").and_then(Value::as_str).unwrap_or("Codex reported an error.").to_owned()),
        _ => CodexEvent::Other,
    }
}

/// A Codex turn's reply from its whole stream: the last message is the answer.
pub fn parse_codex(stdout: &str) -> Reply {
    let mut r = Reply::default();
    let mut last: Option<String> = None;
    for line in stdout.lines() {
        match codex_event(line) {
            CodexEvent::Forbidden(what) => {
                return Reply { problem: Some(forbidden(&what)), ..r };
            }
            CodexEvent::Message(t) => last = Some(t),
            CodexEvent::Usage { tokens_in, tokens_out, tokens_cached } => {
                r.tokens_in = tokens_in;
                r.tokens_out = tokens_out;
                r.tokens_cached = tokens_cached;
            }
            CodexEvent::Failed(e) => r.problem = Some(format!("Codex: {e}")),
            CodexEvent::Other => {}
        }
    }
    let text = last.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
    match (text, r.problem.is_some()) {
        (Some(t), false) => r.text = Some(t),
        (None, false) => r.problem = Some("The model returned an empty answer.".into()),
        (_, true) => {}
    }
    r
}

/// Why a Codex turn was stopped when it reached for a tool it shouldn't have.
pub fn forbidden(what: &str) -> String {
    format!(
        "Codex started a {} although Chorus switches its shell and other tools off, so the turn was stopped. This Codex may handle those switches differently; \
         run this member on OpenRouter until Chorus is updated for it.",
        what.replace('_', " ")
    )
}

/// A local model name: what the CLI's `--model`/`-m` takes, never an option.
pub fn valid_model(name: &str) -> bool {
    crate::agent::valid_model(name) && !name.contains('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_on_round_trips_and_defaults_to_openrouter() {
        for r in [RunsOn::Openrouter, RunsOn::Claude, RunsOn::Codex] {
            assert_eq!(RunsOn::parse(r.key()), Some(r));
        }
        assert_eq!(RunsOn::parse("gemini"), None);
        assert!(!RunsOn::default().is_local());
        // A roster stored before D140 has no runs_on: it reads as OpenRouter.
        #[derive(Deserialize)]
        struct Old {
            #[serde(default)]
            runs_on: RunsOn,
        }
        assert_eq!(serde_json::from_str::<Old>("{}").unwrap().runs_on, RunsOn::Openrouter);
    }

    #[test]
    fn the_mcp_tools_are_the_council_tools() {
        let t = mcp_tools();
        let names: Vec<&str> = t.as_array().unwrap().iter().map(|x| x["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["list_files", "read_file", "search_code"]);
        assert_eq!(t[1]["inputSchema"]["required"], json!(["path"]));
    }

    #[test]
    fn claude_runs_with_no_built_in_tools_and_only_chorus_mcp() {
        let a = claude_args("claude-opus-5-5", "C:\\data\\council\\t7.json");
        let at = |flag: &str| a[a.iter().position(|x| x == flag).unwrap() + 1].clone();
        assert_eq!(at("--tools"), "");
        assert_eq!(at("--setting-sources"), "");
        assert_eq!(at("--model"), "claude-opus-5-5");
        assert!(a.contains(&"--restricted".to_owned()) && a.contains(&"--strict-mcp-config".to_owned()));
        assert_eq!(at("--allowedTools"), "mcp__council__list_files,mcp__council__read_file,mcp__council__search_code");
        // --mcp-config is variadic (L83): an option must follow its file.
        assert!(at("--mcp-config").ends_with("t7.json"));
        assert!(a[a.iter().position(|x| x == "--mcp-config").unwrap() + 2].starts_with("--"));
        // No prompt in argv: it goes on stdin.
        assert!(!a.iter().any(|x| x.contains("brief")));
        let cfg = claude_mcp_config("http://127.0.0.1:5/mcp/council/7");
        assert_eq!(cfg["mcpServers"]["council"]["headers"]["Authorization"], "Bearer ${CHORUS_COUNCIL_TOKEN}");
    }

    #[test]
    fn codex_runs_without_its_shell_and_reads_its_token_from_the_environment() {
        let a = codex_args("gpt-6-astra", "http://127.0.0.1:5/mcp/council/7", "C:\\data\\council\\run3");
        let disabled: Vec<&str> = a.windows(2).filter(|w| w[0] == "--disable").map(|w| w[1].as_str()).collect();
        assert!(disabled.contains(&"shell_tool") && disabled.contains(&"unified_exec"));
        let at = |flag: &str| a[a.iter().position(|x| x == flag).unwrap() + 1].clone();
        assert_eq!(at("-s"), "read-only");
        assert_eq!(at("-m"), "gpt-6-astra");
        assert_eq!(a.first().map(String::as_str), Some("exec"));
        assert_eq!(a.last().map(String::as_str), Some("-"));
        assert!(a.contains(&"--ignore-user-config".to_owned()) && a.contains(&"--ephemeral".to_owned()));
        assert!(a.contains(&"mcp_servers.council.bearer_token_env_var=\"CHORUS_COUNCIL_TOKEN\"".to_owned()));
        assert!(a.contains(&"mcp_servers.council.default_tools_approval_mode=\"approve\"".to_owned()));
        assert!(!a.iter().any(|x| x.contains("Bearer")));
    }

    #[test]
    fn claude_results_are_read() {
        let ok = r#"{"type":"result","subtype":"success","is_error":false,"result":" The answer. ","usage":{"input_tokens":4,"cache_creation_input_tokens":10,"cache_read_input_tokens":100,"output_tokens":50},"total_cost_usd":0.5}"#;
        let r = parse_claude(ok);
        assert_eq!(r.text.as_deref(), Some("The answer."));
        assert_eq!((r.tokens_in, r.tokens_out, r.tokens_cached), (Some(114), Some(50), Some(100)));
        let limit = r#"{"type":"result","subtype":"success","is_error":true,"result":"You've hit your usage limit."}"#;
        assert_eq!(parse_claude(limit).problem.as_deref(), Some("Claude Code: You've hit your usage limit."));
        assert!(parse_claude("").problem.unwrap().contains("no result"));
        assert!(parse_claude(r#"{"type":"result","subtype":"success","is_error":false,"result":""}"#).problem.unwrap().contains("empty"));
    }

    #[test]
    fn codex_streams_are_read_and_a_command_stops_the_turn() {
        let ok = [
            r#"{"type":"thread.started","thread_id":"x"}"#,
            r#"{"type":"item.completed","item":{"id":"i0","type":"agent_message","text":"Looking."}}"#,
            r#"{"type":"item.completed","item":{"id":"i1","type":"mcp_tool_call","server":"council","tool":"read_file"}}"#,
            r#"{"type":"item.completed","item":{"id":"i2","type":"agent_message","text":"Final answer."}}"#,
            r#"{"type":"turn.completed","usage":{"input_tokens":900,"cached_input_tokens":800,"output_tokens":40}}"#,
        ]
        .join("\n");
        let r = parse_codex(&ok);
        assert_eq!(r.text.as_deref(), Some("Final answer."));
        assert_eq!((r.tokens_in, r.tokens_out, r.tokens_cached), (Some(900), Some(40), Some(800)));

        let shell = r#"{"type":"item.started","item":{"id":"i2","type":"command_execution","command":"pwsh -c Get-Content .env","status":"in_progress"}}"#;
        assert_eq!(codex_event(shell), CodexEvent::Forbidden("command_execution".into()));
        let r = parse_codex(&format!("{shell}\n{}", r#"{"type":"item.completed","item":{"type":"agent_message","text":"PASSWORD=x"}}"#));
        assert_eq!(r.text, None);
        assert!(r.problem.unwrap().contains("command execution"));

        let failed = r#"{"type":"turn.failed","error":{"message":"usage limit reached"}}"#;
        assert_eq!(parse_codex(failed).problem.as_deref(), Some("Codex: usage limit reached"));
        assert!(parse_codex("").problem.unwrap().contains("empty"));
    }

    #[test]
    fn local_models_are_plain_names() {
        assert!(valid_model("claude-opus-5-5") && valid_model("gpt-6-astra"));
        assert!(!valid_model("anthropic/claude-opus-5.5"));
        assert!(!valid_model("--dangerously-skip-permissions"));
        assert!(!valid_model(""));
    }
}
