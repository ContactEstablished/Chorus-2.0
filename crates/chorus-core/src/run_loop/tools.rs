//! Chorus's tools for a loop step's agent (P15 slice 3, D98 decision 2): `ask`, `log_assumption`
//! and `task_done`, served over MCP's streamable HTTP transport on Chorus's loopback listener. This
//! module answers the JSON-RPC itself; the caller runs the tools. Spike S3 confirmed Claude Code
//! 2.1.308, Codex 0.163.0 and opencode 2.1.4 against the same shape: plain JSON responses, 202 for
//! notifications, and "method not found" for anything else (Claude Code's `server/discover`).
//!
//! `ask` never waits for an answer (P15-D2): Claude Code gives up on a tool call after about 60 s,
//! and a question may wait hours. It records the question and tells the agent to end its turn; the
//! answer, or a timed default, arrives as its next prompt.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;

pub const SERVER_NAME: &str = "chorus";
/// The names Claude Code gives these tools, for its `--allowedTools`.
pub const CLAUDE_TOOL_NAMES: [&str; 3] = ["mcp__chorus__ask", "mcp__chorus__log_assumption", "mcp__chorus__task_done"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AskClass {
    /// Anything under "Always ask" in decisions.md: the task waits for Matthew, with no timeout.
    Blocking,
    /// Ambiguous behaviour or two reasonable ways: Matthew has a while to answer, then the default.
    Timed,
    /// Naming, internal structure: the agent decides and the choice is logged, nothing is sent.
    LogOnly,
}

impl AskClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Blocking => "blocking",
            Self::Timed => "timed",
            Self::LogOnly => "log_only",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().replace(['-', ' '], "_").as_str() {
            "blocking" => Some(Self::Blocking),
            "timed" => Some(Self::Timed),
            "log_only" | "logonly" => Some(Self::LogOnly),
            _ => None,
        }
    }
}

/// A question as the agent asked it, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub class: AskClass,
    pub question: String,
    pub options: Vec<String>,
    pub default: Option<String>,
}

pub const MAX_QUESTION: usize = 1000;
pub const MAX_OPTIONS: usize = 6;
pub const MAX_OPTION: usize = 200;

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

/// Reads `ask`'s arguments. A timed question needs a default; a default must be one of the options
/// when there are options.
pub fn parse_ask(args: &Value) -> Result<Ask, String> {
    let class = text(args, "class").and_then(|c| AskClass::parse(&c)).ok_or("class must be blocking, timed or log_only.")?;
    let question = text(args, "question").ok_or("question is required.")?;
    if question.chars().count() > MAX_QUESTION {
        return Err(format!("Keep the question under {MAX_QUESTION} characters."));
    }
    let options: Vec<String> = match args.get("options") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect(),
        Some(_) => return Err("options must be a list of strings.".into()),
    };
    if options.len() > MAX_OPTIONS || options.iter().any(|o| o.chars().count() > MAX_OPTION) {
        return Err(format!("Give at most {MAX_OPTIONS} options of up to {MAX_OPTION} characters."));
    }
    let default = text(args, "default");
    if let Some(d) = &default
        && !options.is_empty()
        && !options.iter().any(|o| o == d)
    {
        return Err("default must be one of the options.".into());
    }
    if class == AskClass::Timed && default.is_none() {
        return Err("A timed question needs a default, taken if nobody answers in time.".into());
    }
    if class == AskClass::LogOnly && default.is_none() && options.is_empty() {
        return Err("A log_only question needs the choice you made, as default.".into());
    }
    Ok(Ask { class, question, options, default })
}

/// What `ask` tells the agent.
pub fn ask_reply(a: &Ask, timed_minutes: u32) -> String {
    match a.class {
        AskClass::LogOnly => "Logged in assumptions.md. Go ahead with your choice.".into(),
        AskClass::Timed => format!(
            "Asked. Finish anything that doesn't depend on the answer, then end your turn. The answer comes as your next message: Matthew's, or the default ({}) if he hasn't answered in {timed_minutes} minutes.",
            a.default.as_deref().unwrap_or("")
        ),
        AskClass::Blocking => {
            "Asked. Finish anything that doesn't depend on the answer, then end your turn. Matthew's answer comes as your next message; there is no default."
                .into()
        }
    }
}

/// The tools, as `tools/list` describes them.
pub fn tool_list() -> Value {
    json!([
        {
            "name": "ask",
            "description": "Ask Matthew a question about this task. Read decisions.md first: if it answers the question, follow it instead. class: blocking for anything under \"Always ask\" in decisions.md (the task waits for him, with no timeout); timed for ambiguous behaviour or two reasonable ways to do it (give options and a default, taken if he doesn't answer in time); log_only for naming, internal structure or which helper to use (give your choice as default; it is logged and you go ahead). After a blocking or timed question, end your turn: the answer comes as your next message.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "class": {"type": "string", "enum": ["blocking", "timed", "log_only"]},
                    "question": {"type": "string", "description": "One question, answerable without reading the code."},
                    "options": {"type": "array", "items": {"type": "string"}, "description": "Up to 6 short options."},
                    "default": {"type": "string", "description": "The option to take if nobody answers (timed), or your choice (log_only)."}
                },
                "required": ["class", "question"]
            }
        },
        {
            "name": "log_assumption",
            "description": "Record a choice you made on your own, in one line, in the run's assumptions.md, so Matthew can skim it later.",
            "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}
        },
        {
            "name": "task_done",
            "description": "Say the step is finished, with a summary of what you did in one or two sentences. Then end your turn.",
            "inputSchema": {"type": "object", "properties": {"summary": {"type": "string"}}, "required": ["summary"]}
        }
    ])
}

/// A tool call's outcome: text for the agent, and whether it is an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    pub text: String,
    pub is_error: bool,
}

impl ToolResult {
    pub fn ok(text: impl Into<String>) -> Self {
        Self { text: text.into(), is_error: false }
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self { text: text.into(), is_error: true }
    }
}

const PROTOCOL: &str = "2025-06-18";

fn answer(msg: &Value, call: &mut impl FnMut(&str, &Value) -> ToolResult) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => json!({
            "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL),
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": SERVER_NAME, "version": "1"},
        }),
        "ping" => json!({}),
        "tools/list" => json!({"tools": tool_list()}),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let r = call(name, &args);
            json!({"content": [{"type": "text", "text": r.text}], "isError": r.is_error})
        }
        _ => return Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("no method {method}")}})),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

/// The response to a POSTed body: one message or a batch. `None` when every message was a
/// notification (HTTP 202, no body); `Err` for a body that isn't JSON-RPC (HTTP 400).
pub fn respond(body: &str, mut call: impl FnMut(&str, &Value) -> ToolResult) -> Result<Option<String>, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("not JSON: {e}"))?;
    match v {
        Value::Array(msgs) => {
            let out: Vec<Value> = msgs.iter().filter_map(|m| answer(m, &mut call)).collect();
            Ok((!out.is_empty()).then(|| Value::Array(out).to_string()))
        }
        Value::Object(_) => Ok(answer(&v, &mut call).map(|r| r.to_string())),
        _ => Err("not a JSON-RPC message".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_are_checked() {
        let ok = parse_ask(&json!({"class": "timed", "question": " Offset or cursor? ", "options": ["cursor", "offset"], "default": "cursor"})).unwrap();
        assert_eq!((ok.class, ok.question.as_str(), ok.options.len(), ok.default.as_deref()), (AskClass::Timed, "Offset or cursor?", 2, Some("cursor")));
        assert!(parse_ask(&json!({"class": "timed", "question": "x"})).unwrap_err().contains("needs a default"));
        assert!(parse_ask(&json!({"class": "soon", "question": "x"})).unwrap_err().contains("class"));
        assert!(parse_ask(&json!({"class": "blocking", "question": "x", "options": ["a"], "default": "b"})).unwrap_err().contains("one of the options"));
        assert!(parse_ask(&json!({"class": "blocking"})).unwrap_err().contains("question"));
        assert!(parse_ask(&json!({"class": "log-only", "question": "name?"})).unwrap_err().contains("the choice you made"));
        assert_eq!(parse_ask(&json!({"class": "Log Only", "question": "name?", "default": "x"})).unwrap().class, AskClass::LogOnly);
        assert!(parse_ask(&json!({"class": "blocking", "question": "x", "options": ["a","b","c","d","e","f","g"]})).is_err());
    }

    #[test]
    fn replies_say_what_happens_next() {
        let a = Ask { class: AskClass::Timed, question: "q".into(), options: vec![], default: Some("cursor".into()) };
        assert!(ask_reply(&a, 15).contains("the default (cursor) if he hasn't answered in 15 minutes"));
        assert!(ask_reply(&Ask { class: AskClass::Blocking, ..a.clone() }, 15).contains("no default"));
        assert!(ask_reply(&Ask { class: AskClass::LogOnly, ..a }, 15).starts_with("Logged"));
    }

    #[test]
    fn json_rpc() {
        let none = |_: &str, _: &Value| ToolResult::ok("unused");
        let init = respond(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#, none).unwrap().unwrap();
        let v: Value = serde_json::from_str(&init).unwrap();
        assert_eq!((v["id"].as_i64(), v["result"]["protocolVersion"].as_str(), v["result"]["serverInfo"]["name"].as_str()), (Some(1), Some("2025-03-26"), Some("chorus")));
        assert_eq!(respond(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, none).unwrap(), None);
        let list: Value = serde_json::from_str(&respond(r#"{"jsonrpc":"2.0","id":"a","method":"tools/list"}"#, none).unwrap().unwrap()).unwrap();
        assert_eq!(list["result"]["tools"].as_array().unwrap().len(), 3);
        let unknown: Value = serde_json::from_str(&respond(r#"{"jsonrpc":"2.0","id":2,"method":"server/discover"}"#, none).unwrap().unwrap()).unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
        assert!(respond("nope", none).is_err());
    }

    #[test]
    fn tool_calls_reach_the_caller_and_batches_work() {
        let mut seen = Vec::new();
        let body = r#"[{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"log_assumption","arguments":{"text":"x"}}},{"jsonrpc":"2.0","method":"notifications/x"}]"#;
        let out = respond(body, |name, args| {
            seen.push((name.to_owned(), args["text"].as_str().unwrap_or("").to_owned()));
            ToolResult::error("nope")
        })
        .unwrap()
        .unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!((v[0]["result"]["isError"].as_bool(), v[0]["result"]["content"][0]["text"].as_str()), (Some(true), Some("nope")));
        assert_eq!(seen, [("log_assumption".to_owned(), "x".to_owned())]);
    }
}
