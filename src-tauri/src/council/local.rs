//! A council turn on a local subscription (D140). The CLI (Claude Code or Codex) runs headless in
//! an empty folder of Chorus's own, with its own file tools switched off, and reads the code only
//! through Chorus's MCP server for this turn (`/mcp/council/<turn>`): the same three tools, the
//! same scrubbing and the same limits as an OpenRouter member. The server is authorised by a
//! token that only the child's environment holds, and exists only while the turn runs.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use chorus_core::agent::AgentKind;
use chorus_core::council::local::{self as core, Reply, RunsOn};
use chorus_core::council::tools::{self, Budget, CouncilLimits};
use chorus_core::council::TurnOutcome;
use chorus_core::env::{ChildEnvInput, compose_child_env};
use chorus_core::run_loop::tools::{ToolResult, respond_with};
use serde_json::Value;

use super::run::{Shared, TurnCtx};
use crate::sessions::SessionManager;
use crate::sessions::job::Job;
use crate::store::council::{self as db, RosterEntry, Usage};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// A local turn that runs this long is stuck, not thinking.
const TURN_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// More output than this is cut (a turn's answer is a few kilobytes).
const MAX_OUTPUT_BYTES: usize = 8_000_000;
/// How much of the CLI's own error output is kept for the reason a turn failed.
const KEEP_ERR_BYTES: usize = 2_000;

/// A turn whose tools are being served.
struct Open {
    token: String,
    shared: Arc<Shared>,
    manager: Arc<SessionManager>,
    notify: Arc<dyn Fn(u32) + Send + Sync>,
    run_id: u32,
    turn: u32,
    limits: CouncilLimits,
    budget: Mutex<Budget>,
}

static OPEN: LazyLock<Mutex<HashMap<u32, Arc<Open>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn open_turns() -> std::sync::MutexGuard<'static, HashMap<u32, Arc<Open>>> {
    OPEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Removes the turn's server, and its config file, however the turn ends.
struct Closing {
    turn: u32,
    file: Option<PathBuf>,
}

impl Drop for Closing {
    fn drop(&mut self) {
        open_turns().remove(&self.turn);
        if let Some(f) = &self.file {
            let _ = std::fs::remove_file(f);
        }
    }
}

/// One MCP request for a turn. `Err(())` is unauthorised: no such turn running, or the wrong
/// token.
pub fn serve(turn: u32, token: &str, body: &str) -> Result<Result<Option<String>, String>, ()> {
    let open = open_turns().get(&turn).cloned().ok_or(())?;
    if token.is_empty() || !crate::events::same_token(&open.token, token) {
        return Err(());
    }
    Ok(respond_with(body, &core::mcp_tools(), |name, args| open.call(name, args)))
}

impl Open {
    fn call(&self, name: &str, args: &Value) -> ToolResult {
        let store = self.manager.store();
        let mut budget = self.budget.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(reason) = budget.spent(&self.limits, None, None) {
            let _ = db::add_tool_call(store, self.turn, &format!("{name} (after the limit)"), false, 0, Some(&reason));
            return ToolResult { text: tools::answer_now(&reason), is_error: true };
        }
        let (described, ran) = match tools::parse(name, &args.to_string()) {
            Ok(c) => (c.describe(), self.shared.code.run(&c)),
            Err(e) => (format!("{name} (refused)"), super::code::Ran { ok: false, text: e }),
        };
        budget.calls += 1;
        if ran.ok {
            budget.bytes += ran.text.len() as u64;
        }
        drop(budget);
        let note = (!ran.ok).then_some(ran.text.as_str());
        let _ = db::add_tool_call(store, self.turn, &described, ran.ok, if ran.ok { ran.text.len() } else { 0 }, note);
        (self.notify)(self.run_id);
        ToolResult { text: ran.text, is_error: !ran.ok }
    }
}

fn agent(runs_on: RunsOn) -> Option<(AgentKind, &'static str, &'static str)> {
    match runs_on {
        RunsOn::Claude => Some((AgentKind::Claude, "Claude Code", core::CLAUDE_VERIFIED)),
        RunsOn::Codex => Some((AgentKind::Codex, "Codex", core::CODEX_VERIFIED)),
        RunsOn::Openrouter => None,
    }
}

/// Why a local member can't take part now (its CLI isn't found), or none.
pub fn problem(m: &SessionManager, runs_on: RunsOn) -> Option<String> {
    let (kind, name, _) = agent(runs_on)?;
    match m.agents().get(kind) {
        Some(d) if d.found => None,
        Some(d) => Some(format!("{name} isn't available: {}", d.problem.unwrap_or_else(|| "not found".into()))),
        None => Some(format!("{name} hasn't been looked for yet. Open Settings → Agents to check for it.")),
    }
}

/// A note when the installed CLI isn't the version these settings were checked with.
pub fn version_note(m: &SessionManager, runs_on: RunsOn) -> Option<String> {
    let (kind, name, verified) = agent(runs_on)?;
    let installed = m.agents().get(kind)?.version?;
    (installed != verified).then(|| format!("Chorus checked council turns with {name} {verified}; {installed} is installed. It runs, and stops a turn that reaches for a tool it shouldn't."))
}

/// Runs one local turn to its end. The outcome and content are stored by the caller.
pub(super) fn converse(ctx: &TurnCtx, shared: &Arc<Shared>, who: &RosterEntry, turn: u32, prompt: &str, usage: &mut Usage) -> (TurnOutcome, String) {
    match run_turn(ctx, shared, who, turn, prompt) {
        Ok(Ended::Cancelled) => (TurnOutcome::Cancelled, "Cancelled before this turn finished.".into()),
        Ok(Ended::Replied(r)) => {
            usage.add(&Usage { tokens_in: r.tokens_in, tokens_out: r.tokens_out, tokens_cached: r.tokens_cached, cost_usd: None });
            match r.text {
                Some(text) => (TurnOutcome::Answered, text),
                None => (TurnOutcome::Refused, r.problem.unwrap_or_else(|| "The model returned no answer.".into())),
            }
        }
        Err(reason) => {
            let note = version_note(&ctx.manager, who.runs_on).map(|n| format!(" ({n})")).unwrap_or_default();
            (TurnOutcome::Refused, format!("{reason}{note}"))
        }
    }
}

enum Ended {
    Cancelled,
    Replied(Reply),
}

fn run_turn(ctx: &TurnCtx, shared: &Arc<Shared>, who: &RosterEntry, turn: u32, prompt: &str) -> Result<Ended, String> {
    let (kind, name, _) = agent(who.runs_on).ok_or("This member runs on OpenRouter.")?;
    let m = &ctx.manager;
    let port = m.listener_port().ok_or("Chorus's local listener isn't running, so a local member couldn't read the code.")?;
    if let Some(p) = problem(m, who.runs_on) {
        return Err(p);
    }
    let parent: Vec<(String, String)> = std::env::vars().collect();
    let dir = m.data_dir().join("council").join(format!("run-{}", ctx.run_id));
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make the turn's working folder: {e}"))?;
    let launch = crate::agents::resolve(kind, &parent, &dir)?;
    let url = core::mcp_url(port, turn);

    let token: String = crate::vault::random_bytes(24).iter().map(|b| format!("{b:02x}")).collect();
    let mut closing = Closing { turn, file: None };
    let mut args = launch.args.clone();
    match who.runs_on {
        RunsOn::Claude => {
            let file = dir.join(format!("mcp-turn-{turn}.json"));
            std::fs::write(&file, core::claude_mcp_config(&url).to_string()).map_err(|e| format!("Couldn't write the turn's MCP config: {e}"))?;
            args.extend(core::claude_args(&who.model, &file.display().to_string()));
            closing.file = Some(file);
        }
        _ => args.extend(core::codex_args(&who.model, &url, &dir.display().to_string())),
    }
    open_turns().insert(
        turn,
        Arc::new(Open {
            token: token.clone(),
            shared: shared.clone(),
            manager: m.clone(),
            notify: ctx.notify.clone(),
            run_id: ctx.run_id,
            turn,
            limits: ctx.limits,
            budget: Mutex::new(Budget::default()),
        }),
    );

    // No API key reaches the child, so the CLI uses its own subscription sign-in.
    let additions: Vec<(&str, &str)> = launch.additions.iter().map(|(k, v)| (*k, v.as_str())).chain([("NO_COLOR", "1")]).collect();
    let env = compose_child_env(&ChildEnvInput { parent: &parent, required: &[], additions: &additions, secrets: &[(core::TOKEN_ENV.to_owned(), token)] });
    let mut child = Command::new(&launch.program)
        .args(&args)
        .current_dir(&dir)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("Couldn't start {name}: {e}"))?;
    // Dropping the job ends everything the CLI started.
    let job = Job::new().map_err(|e| format!("Couldn't create a job for {name}: {e}"))?;
    let _ = job.assign(windows::Win32::Foundation::HANDLE(child.as_raw_handle()));

    let mut stdin = child.stdin.take();
    let text = prompt.to_owned();
    std::thread::spawn(move || {
        if let Some(mut s) = stdin.take() {
            let _ = s.write_all(text.as_bytes());
        }
    });
    let forbidden = Arc::new(AtomicBool::new(false));
    let out = child.stdout.take().map(|p| {
        let forbidden = forbidden.clone();
        let watch = who.runs_on == RunsOn::Codex;
        std::thread::spawn(move || read_output(p, watch.then_some(forbidden)))
    });
    let err = child.stderr.take().map(|mut p| {
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let _ = p.read_to_end(&mut kept);
            let start = kept.len().saturating_sub(KEEP_ERR_BYTES);
            String::from_utf8_lossy(&kept[start..]).into_owned()
        })
    });

    let started = Instant::now();
    let status = loop {
        if forbidden.load(Ordering::SeqCst) {
            let _ = job.terminate(1);
        }
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if ctx.stopping() => {
                let _ = job.terminate(1);
                let _ = child.wait();
                return Ok(Ended::Cancelled);
            }
            Ok(None) if started.elapsed() > TURN_TIMEOUT => {
                let _ = job.terminate(1);
                let _ = child.wait();
                return Err(format!("{name} didn't finish this turn within {} minutes.", TURN_TIMEOUT.as_secs() / 60));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(_) => break None,
        }
    };
    let stdout = out.and_then(|h| h.join().ok()).unwrap_or_default();
    let stderr = err.and_then(|h| h.join().ok()).unwrap_or_default();
    drop(closing);
    if forbidden.load(Ordering::SeqCst) {
        let what = stdout.lines().find_map(|l| match core::codex_event(l) {
            core::CodexEvent::Forbidden(w) => Some(w),
            _ => None,
        });
        return Err(core::forbidden(what.as_deref().unwrap_or("command")));
    }
    let reply = match who.runs_on {
        RunsOn::Claude => core::parse_claude(&stdout),
        _ => core::parse_codex(&stdout),
    };
    if reply.text.is_none() && !status.is_some_and(|s| s.success()) {
        let code = status.and_then(|s| s.code()).map_or_else(|| "no exit code".to_owned(), |c| format!("exit code {c}"));
        let said = last_line(&stderr);
        let why = reply.problem.clone().filter(|p| !p.contains("no result") && !p.contains("empty answer"));
        return Err(match (why, said) {
            (Some(p), _) => p,
            (None, Some(s)) => format!("{name} stopped ({code}): {s}"),
            (None, None) => format!("{name} stopped ({code}) without an answer."),
        });
    }
    Ok(Ended::Replied(reply))
}

/// Reads the CLI's output; for Codex, raises `forbidden` the moment it starts a command, so the
/// turn is stopped before the result can reach the model.
fn read_output(mut pipe: std::process::ChildStdout, forbidden: Option<Arc<AtomicBool>>) -> String {
    let mut kept: Vec<u8> = Vec::new();
    let mut scanned = 0;
    let mut buf = [0u8; 16 * 1024];
    while let Ok(n) = pipe.read(&mut buf) {
        if n == 0 {
            break;
        }
        if kept.len() < MAX_OUTPUT_BYTES {
            kept.extend_from_slice(&buf[..n]);
        }
        if let Some(flag) = &forbidden {
            while let Some(end) = kept[scanned..].iter().position(|&b| b == b'\n') {
                let line = String::from_utf8_lossy(&kept[scanned..scanned + end]).into_owned();
                if matches!(core::codex_event(&line), core::CodexEvent::Forbidden(_)) {
                    flag.store(true, Ordering::SeqCst);
                }
                scanned += end + 1;
            }
        }
    }
    String::from_utf8_lossy(&kept).into_owned()
}

fn last_line(s: &str) -> Option<String> {
    s.lines().map(str::trim).rfind(|l| !l.is_empty()).map(|l| l.chars().take(400).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use serde_json::json;

    fn call(turn: u32, token: &str, id: u32, name: &str, args: Value) -> Value {
        let body = json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": name, "arguments": args}}).to_string();
        let out = serve(turn, token, &body).expect("authorised").expect("JSON-RPC").expect("a reply");
        serde_json::from_str::<Value>(&out).unwrap()["result"].clone()
    }

    #[test]
    fn a_turns_tools_need_its_token_and_read_only_what_a_member_may() {
        let dir = std::env::temp_dir().join(format!("chorus-council-local-{}-{}", std::process::id(), crate::store::now_ms()));
        std::fs::create_dir_all(dir.join("code")).unwrap();
        std::fs::write(dir.join("code/main.rs"), "fn main() {\n    let key = \"SECRET-VALUE-123\";\n}\n").unwrap();
        std::fs::write(dir.join("code/.env"), "PASSWORD=hunter2").unwrap();
        let manager = SessionManager::new(Store::open(&dir.join("chorus.db")).unwrap(), dir.clone(), Box::new(|_| {}));
        let code = super::super::code::Code::load(&dir.join("code"), &[], vec![b"SECRET-VALUE-123".to_vec()], vec![]).unwrap();
        let shared = Arc::new(Shared::for_test(code, vec![]));
        let turn = 900_001;
        let _closing = Closing { turn, file: None };
        open_turns().insert(
            turn,
            Arc::new(Open {
                token: "right-token".into(),
                shared,
                manager,
                notify: Arc::new(|_| {}),
                run_id: 1,
                turn,
                limits: CouncilLimits { max_calls: 3, ..CouncilLimits::default() },
                budget: Mutex::new(Budget::default()),
            }),
        );

        // No token, the wrong one, or another turn's number: refused before anything is read.
        let list = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string();
        assert!(serve(turn, "", &list).is_err());
        assert!(serve(turn, "wrong-token", &list).is_err());
        assert!(serve(turn + 1, "right-token", &list).is_err());

        let tools = serve(turn, "right-token", &list).unwrap().unwrap().unwrap();
        assert!(tools.contains("read_file") && tools.contains("search_code") && tools.contains("list_files"));

        // A file is read with the saved key scrubbed; .env is never read.
        let read = call(turn, "right-token", 2, "read_file", json!({"path": "main.rs"}));
        let text = read["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("fn main") && !text.contains("SECRET-VALUE-123"), "{text}");
        assert_eq!(read["isError"], false);
        let env = call(turn, "right-token", 3, "read_file", json!({"path": ".env"}));
        assert_eq!(env["isError"], true);
        assert!(!env.to_string().contains("hunter2"));

        // After the turn's limit, a call is told to answer instead.
        let _ = call(turn, "right-token", 4, "list_files", json!({}));
        let over = call(turn, "right-token", 5, "list_files", json!({}));
        assert_eq!(over["isError"], true);
        assert!(over["content"][0]["text"].as_str().unwrap().to_lowercase().contains("answer"));

        // Once the turn ends its server is gone.
        drop(_closing);
        assert!(serve(turn, "right-token", &list).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The whole path with the real CLIs: Chorus starts each one, it reads the code through
    /// Chorus's listener, answers, and never sees the .env or a file outside the folder. Run by
    /// hand: `cargo test -p chorus --lib live_local_turns -- --ignored --nocapture`.
    #[test]
    #[ignore = "runs the real Claude Code and Codex on their subscriptions (two small turns)"]
    fn live_local_turns_read_only_through_chorus() {
        use chorus_core::council::CouncilRole;
        let dir = std::env::temp_dir().join(format!("chorus-council-live-{}-{}", std::process::id(), crate::store::now_ms()));
        std::fs::create_dir_all(dir.join("code")).unwrap();
        std::fs::write(dir.join("code/main.rs"), "fn council_probe_fn() -> u32 {
    42
}
").unwrap();
        std::fs::write(dir.join("code/.env"), "PASSWORD=LIVE-CANARY-ENV").unwrap();
        std::fs::write(dir.join("outside.txt"), "LIVE-CANARY-OUTSIDE").unwrap();
        let manager = SessionManager::new(Store::open(&dir.join("chorus.db")).unwrap(), dir.clone(), Box::new(|_| {}));
        manager.set_hook_listener(crate::events::listener::start(manager.clone()));
        manager.refresh_agents();
        let code = super::super::code::Code::load(&dir.join("code"), &[], vec![], vec![]).unwrap();
        let shared = Arc::new(Shared::for_test(code, vec![]));
        let ctx = TurnCtx::for_test(manager.clone(), 1);
        let prompt = "Use your tools to find the name of the function defined in main.rs, and reply with that name.                       Also try to read .env and ../outside.txt, and say what you got.";
        for (i, (runs_on, model)) in [(RunsOn::Claude, "claude-haiku-5-5"), (RunsOn::Codex, "gpt-6-astra")].into_iter().enumerate() {
            let who = RosterEntry { id: 1, label: "Probe".into(), model: model.into(), role: CouncilRole::Member, max_tokens: 4000, context_length: None, runs_on };
            let mut usage = Usage::default();
            let started = Instant::now();
            let (outcome, text) = converse(&ctx, &shared, &who, 990_000 + i as u32, prompt, &mut usage);
            println!("--- {runs_on:?} {model}: {outcome:?} in {:?}, tokens in {:?} out {:?}
{text}
", started.elapsed(), usage.tokens_in, usage.tokens_out);
            assert_eq!(outcome, TurnOutcome::Answered, "{text}");
            assert!(text.contains("council_probe_fn"), "{text}");
            assert!(!text.contains("LIVE-CANARY"), "{text}");
            assert!(usage.tokens_out.is_some());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
