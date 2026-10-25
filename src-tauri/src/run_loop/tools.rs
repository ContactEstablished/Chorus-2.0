//! Chorus's tools for loop steps (P15 slice 3): the MCP endpoint `POST /mcp/<step>` on the loopback
//! listener, the token each step's agent gets in its environment (never in argv or a file, spike
//! S3), and what `ask`, `log_assumption` and `task_done` do.

use std::path::Path;
use std::sync::Mutex;

use chorus_core::run_loop::prompts::RUN_DIR;
use chorus_core::run_loop::questions;
use chorus_core::run_loop::tools::{self as core_tools, AskClass, ToolResult};
use chorus_core::scrub::{builtin_patterns, scrub_all};

use super::db::{self, NewQuestion, StepState};
use super::worker;
use crate::sessions::SessionManager;
use crate::store::now_ms;
use crate::vault;

/// The environment variable a step's agent finds its token in.
pub const TOKEN_VAR: &str = "CHORUS_LOOP_TOKEN";

/// Each step's current token. Issued when its agent starts or restarts; gone with Chorus.
static TOKENS: Mutex<Vec<(u32, String)>> = Mutex::new(Vec::new());
/// One writer at a time for a run's documents.
static FILES: Mutex<()> = Mutex::new(());

pub fn issue(step_id: u32) -> String {
    let token: String = vault::random_bytes(24).iter().map(|b| format!("{b:02x}")).collect();
    let mut tokens = TOKENS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    tokens.retain(|(s, _)| *s != step_id);
    tokens.push((step_id, token.clone()));
    token
}

fn authorised(step_id: u32, token: &str) -> bool {
    !token.is_empty() && TOKENS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().any(|(s, t)| *s == step_id && t == token)
}

/// The MCP configuration for a step's Claude Code, in Chorus's data: the step's URL, and a header
/// that names the token's variable. Claude Code fills it from its environment (spike S3).
pub fn config_file(m: &SessionManager, step_id: u32) -> Result<String, String> {
    let port = m.listener_port().ok_or("Chorus's listener isn't running, so a step can't reach its tools.")?;
    let dir = m.data_dir().join("loop");
    std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let path = dir.join(format!("mcp-step-{step_id}.json"));
    let config = serde_json::json!({"mcpServers": {core_tools::SERVER_NAME: {
        "type": "http",
        "url": format!("http://127.0.0.1:{port}/mcp/{step_id}"),
        "headers": {"Authorization": format!("Bearer ${{{TOKEN_VAR}}}")},
    }}});
    std::fs::write(&path, config.to_string()).map_err(|e| format!("writing {}: {e}", path.display()))?;
    Ok(path.display().to_string())
}

#[derive(Debug)]
pub enum Refused {
    Unauthorised,
    Bad(String),
}

/// Answers one POST from a step's agent.
pub fn serve(m: &SessionManager, step_id: u32, token: &str, body: &str) -> Result<Option<String>, Refused> {
    if !authorised(step_id, token) {
        return Err(Refused::Unauthorised);
    }
    core_tools::respond(body, |name, args| call(m, step_id, name, args)).map_err(Refused::Bad)
}

fn scrub(m: &SessionManager, text: &str) -> String {
    let (keys, _) = vault::all_for_scrubbing(m.store());
    String::from_utf8_lossy(&scrub_all(text.as_bytes(), &keys, &builtin_patterns())).into_owned()
}

/// Appends a line to the run's `assumptions.md`.
pub fn log_assumption(worktree: &Path, line: &str) -> Result<(), String> {
    let _one = FILES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = worktree.join(RUN_DIR).join("assumptions.md");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, questions::append_assumption(&existing, line)).map_err(|e| format!("writing assumptions.md: {e}"))
}

/// Adds a rule from one of Matthew's answers to the run's `decisions.md`.
pub fn add_rule(worktree: &Path, question: &str, answer: &str, tag: &str) -> Result<(), String> {
    let _one = FILES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = worktree.join(RUN_DIR).join("decisions.md");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    std::fs::write(&path, questions::append_rule(&existing, question, answer, tag)).map_err(|e| format!("writing decisions.md: {e}"))
}

fn call(m: &SessionManager, step_id: u32, name: &str, args: &serde_json::Value) -> ToolResult {
    match run_tool(m, step_id, name, args) {
        Ok(r) => r,
        Err(e) => ToolResult::error(e),
    }
}

fn run_tool(m: &SessionManager, step_id: u32, name: &str, args: &serde_json::Value) -> Result<ToolResult, String> {
    let store = m.store();
    let step = db::step(store, step_id)?.ok_or("This step no longer exists.")?;
    if step.state != StepState::Running {
        return Ok(ToolResult::error("This step has ended; nothing was recorded."));
    }
    let (run_id, phase) = db::step_place(store, step_id)?.ok_or("This step's run no longer exists.")?;
    let run = db::run(store, run_id)?.ok_or("This step's run no longer exists.")?;
    let worktree = run.worktree_path.clone().ok_or("The run has no worktree.")?;
    let tag = questions::tag(phase, step.task.as_deref());
    let text = |key: &str| args.get(key).and_then(serde_json::Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(|s| scrub(m, s));
    match name {
        "ask" => {
            let ask = core_tools::parse_ask(args).map_err(|e| format!("Not asked: {e}"))?;
            let reply = core_tools::ask_reply(&ask, run.config.limits.timed_minutes);
            let question = scrub(m, &ask.question);
            if ask.class == AskClass::LogOnly {
                let choice = ask.default.clone().or_else(|| ask.options.first().cloned()).unwrap_or_default();
                log_assumption(Path::new(&worktree), &questions::assumption_line(&tag, &format!("{question} Chose: {}", scrub(m, &choice))))?;
                return Ok(ToolResult::ok(reply));
            }
            let options: Vec<String> = ask.options.iter().map(|o| scrub(m, o)).collect();
            let default = ask.default.as_deref().map(|d| scrub(m, d));
            let deadline = (ask.class == AskClass::Timed).then(|| now_ms() + i64::from(run.config.limits.timed_minutes) * 60_000);
            db::insert_question(
                store,
                &NewQuestion {
                    run_id,
                    step_id,
                    phase,
                    task: step.task.as_deref(),
                    class: ask.class.as_str(),
                    question: &question,
                    options: &options,
                    default_answer: default.as_deref(),
                    deadline_at: deadline,
                },
            )?;
            worker::notify(run_id);
            Ok(ToolResult::ok(reply))
        }
        "log_assumption" => {
            let t = text("text").ok_or("text is required.")?;
            if t.chars().count() > 1000 {
                return Err("Keep it to one line, under 1000 characters.".into());
            }
            log_assumption(Path::new(&worktree), &questions::assumption_line(&tag, &t))?;
            Ok(ToolResult::ok("Logged in assumptions.md."))
        }
        "task_done" => {
            let s = text("summary").ok_or("summary is required.")?;
            let s: String = s.chars().take(1000).collect();
            db::set_step_summary(store, step_id, &s)?;
            worker::notify(run_id);
            Ok(ToolResult::ok("Recorded. End your turn now."))
        }
        other => Ok(ToolResult::error(format!("There is no tool called {other}."))),
    }
}
