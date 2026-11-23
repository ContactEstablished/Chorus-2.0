//! One Council run, on a thread of its own: a capped key, the rounds the protocol asks for (each
//! round's turns in parallel), every turn a tool loop over the project's code, the findings file,
//! and the key revoked and its cost read at the end.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chorus_core::council::{Action, TurnOutcome, Participant, Phase, PlannedRun, CouncilRole, State, Turn, local::RunsOn, protocol, tools};
use chorus_core::scrub::{self, Pattern};
use serde_json::{Value, json};

use super::code::Code;
use crate::sessions::SessionManager;
use crate::store::council::{self as db, RosterEntry, RunRow, RunStatus, Usage};
use crate::store::{Store, providers as provider_rows};
use crate::{providers, vault};

/// A request this long is a stuck provider, not a slow model.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);
/// A response bigger than this is refused rather than parsed.
const MAX_RESPONSE_BYTES: usize = 4_000_000;

pub struct Ctx {
    pub manager: Arc<SessionManager>,
    pub cancel: Arc<AtomicBool>,
    pub notify: Arc<dyn Fn(u32) + Send + Sync>,
    pub run_id: u32,
    pub project_name: String,
    pub limits: tools::CouncilLimits,
    /// None when every participant runs on a local subscription.
    pub provider_id: Option<u32>,
}

impl Ctx {
    fn store(&self) -> &Store {
        self.manager.store()
    }
    fn stopping(&self) -> bool {
        self.cancel.load(Ordering::SeqCst) || self.manager.is_quitting()
    }
}

/// Everything a turn needs that is the same for the whole run.
pub(super) struct Shared {
    /// The run's capped OpenRouter key; None when no participant runs on OpenRouter.
    key: Option<String>,
    pub(super) code: Code,
    secrets: Vec<Vec<u8>>,
    patterns: Vec<Pattern>,
}

impl Shared {
    fn scrub(&self, s: &str) -> String {
        String::from_utf8_lossy(&scrub::scrub_all(s.as_bytes(), &self.secrets, &self.patterns)).into_owned()
    }
}

/// Drives the run to its end. Never panics out: every failure ends the run with a reason.
pub fn drive(ctx: Ctx) {
    let run = match db::run(ctx.store(), ctx.run_id) {
        Ok(Some(r)) => r,
        _ => return,
    };
    let mut ledger = None;
    let ended = drive_inner(&ctx, &run, &mut ledger);
    let (status, reason) = match ended {
        Ok(()) => (RunStatus::Complete, None),
        Err(_) if ctx.cancel.load(Ordering::SeqCst) => (RunStatus::Cancelled, Some("Cancelled.".to_owned())),
        Err(_) if ctx.manager.is_quitting() => (RunStatus::Abandoned, Some("Chorus quit while this council was running.".to_owned())),
        Err(reason) => (RunStatus::Failed, Some(reason)),
    };
    let _ = db::finish_run(ctx.store(), ctx.run_id, status, reason.as_deref());
    // The empty working folder local members ran in (D140).
    let _ = std::fs::remove_dir_all(ctx.manager.data_dir().join("council").join(format!("run-{}", ctx.run_id)));
    (ctx.notify)(ctx.run_id);
    // Revoke the key, then read its cost from analytics (LESSONS L16). This blocks for up to half
    // a minute; the screen shows the run as ended meanwhile.
    if let Some(ledger) = ledger {
        if ctx.manager.is_quitting() {
            providers::keys::revoke_now(ctx.store(), ledger);
        } else {
            providers::keys::settle(ctx.store(), ledger);
        }
        (ctx.notify)(ctx.run_id);
    }
}

fn drive_inner(ctx: &Ctx, run: &RunRow, ledger: &mut Option<u32>) -> Result<(), String> {
    let (secrets, failed) = vault::all_for_scrubbing(ctx.store());
    if failed > 0 {
        return Err("Some saved keys couldn't be loaded, so Chorus couldn't make sure none reaches a model. Nothing was sent.".into());
    }
    let patterns = scrub::builtin_patterns();
    let findings_target = findings_path(Path::new(&run.brief_path));
    let exclude: Vec<String> = relative_to(Path::new(&run.project_folder), &findings_target).into_iter().collect();
    let code = Code::load(Path::new(&run.project_folder), &exclude, secrets.clone(), patterns.clone())?;
    if code.file_count() == 0 {
        return Err("The folder has no readable files, so the council would have no code to read.".into());
    }
    if ctx.stopping() {
        return Err("Cancelled.".into());
    }

    // A key is minted only for OpenRouter participants; local ones use their own sign-in (D140).
    let key = if run.roster.iter().any(|r| r.runs_on == RunsOn::Openrouter) {
        let (key, ledger_id) = mint(ctx)?;
        *ledger = Some(ledger_id);
        db::set_key_ledger(ctx.store(), ctx.run_id, ledger_id)?;
        Some(key)
    } else {
        None
    };
    let shared = Arc::new(Shared { key, code, secrets, patterns });

    let planned = planned_run(run)?;
    let started_at = chorus_core::providers::iso_utc_secs(run.started_at);
    let mut summary_saved = false;
    for _ in 0..8 {
        let state = State {
            run: planned.clone(),
            transcript: transcript(ctx.store(), ctx.run_id)?,
            cancelled: ctx.stopping(),
            run_id: ctx.run_id,
            project_name: ctx.project_name.clone(),
            started_at: started_at.clone(),
        };
        // The glance strip, as soon as the positions are in (empty until two have answered).
        if !summary_saved {
            let summary = protocol::summary(&state);
            if !summary.is_empty() {
                db::set_summary(ctx.store(), ctx.run_id, &serde_json::to_string(&summary).unwrap_or_default())?;
                summary_saved = true;
            }
        }
        let actions = protocol::next_action(&state);
        match actions.first() {
            Some(Action::Complete { summary, findings }) => {
                db::set_summary(ctx.store(), ctx.run_id, &serde_json::to_string(summary).unwrap_or_default())?;
                let text = shared.scrub(findings);
                match write_findings(Path::new(&run.brief_path), &text) {
                    Ok(path) => db::set_findings(ctx.store(), ctx.run_id, Some(&path.display().to_string()), None)?,
                    Err(e) => db::set_findings(ctx.store(), ctx.run_id, None, Some(&e))?,
                }
                return Ok(());
            }
            Some(Action::Abort { reason }) => return Err(reason.clone()),
            None => return Err("The protocol had nothing to do.".into()),
            Some(Action::Ask { .. }) => {}
        }
        let roster = &run.roster;
        let handles: Vec<_> = actions
            .into_iter()
            .filter_map(|a| match a {
                Action::Ask { member_id, phase, prompt } => Some((member_id, phase, prompt)),
                _ => None,
            })
            .map(|(member_id, phase, prompt)| {
                let entry = roster.iter().find(|r| r.id == member_id).cloned();
                let shared = shared.clone();
                let ctx = TurnCtx {
                    manager: ctx.manager.clone(),
                    cancel: ctx.cancel.clone(),
                    notify: ctx.notify.clone(),
                    run_id: ctx.run_id,
                    limits: ctx.limits,
                };
                std::thread::Builder::new()
                    .name(format!("council-{}-{member_id}", phase.key()))
                    .spawn(move || match entry {
                        Some(entry) => take_turn(&ctx, &shared, &entry, phase, &prompt),
                        None => Err("A participant left the roster.".to_owned()),
                    })
                    .map_err(|e| format!("Couldn't start a turn: {e}"))
            })
            .collect::<Result<_, _>>()?;
        for h in handles {
            // A turn's own failure is already stored as its outcome; only storage errors come back.
            h.join().map_err(|_| "A turn stopped unexpectedly.".to_owned())??;
        }
        (ctx.notify)(ctx.run_id);
    }
    Err("The protocol didn't finish in the expected number of rounds.".into())
}

fn planned_run(run: &RunRow) -> Result<PlannedRun, String> {
    let to_p = |r: &RosterEntry| Participant { id: r.id, label: r.label.clone(), model: r.model.clone() };
    let arbiter = run.roster.iter().find(|r| r.role == CouncilRole::Arbiter).ok_or("The run has no arbiter.")?;
    Ok(PlannedRun {
        members: run.roster.iter().filter(|r| r.role == CouncilRole::Member).map(to_p).collect(),
        arbiter: to_p(arbiter),
        brief_text: run.brief_text.clone(),
    })
}

fn transcript(store: &Store, run: u32) -> Result<Vec<Turn>, String> {
    Ok(db::turns(store, run)?
        .into_iter()
        .filter_map(|t| {
            Some(Turn { member_id: Some(t.member_id), phase: t.phase, content: t.content.unwrap_or_default(), outcome: t.outcome? })
        })
        .collect())
}

/// A key for this run that stops at the Council's cap.
fn mint(ctx: &Ctx) -> Result<(String, u32), String> {
    let provider_id = ctx.provider_id.ok_or("Choose the Council's OpenRouter provider in Settings → Council: a member runs on OpenRouter.")?;
    let provider = provider_rows::get(ctx.store(), provider_id)?.ok_or("The Council's OpenRouter provider was deleted.")?;
    let management_id = provider
        .management_credential_id
        .ok_or("The Council's OpenRouter provider has no management key, which a council needs to cap its spend.")?;
    let management = vault::decrypt(ctx.store(), management_id)?;
    // session_id 0: no session owns it; council_runs.key_ledger_id points here.
    let ledger_id = provider_rows::open_capped_key(ctx.store(), provider.id, 0, ctx.limits.cap_usd)?;
    match providers::mint(&management, ledger_id, ctx.limits.cap_usd) {
        Ok(minted) => {
            provider_rows::set_capped_hash(ctx.store(), ledger_id, &minted.hash)?;
            Ok((minted.key, ledger_id))
        }
        Err(e) => {
            let _ = provider_rows::close_capped_key(ctx.store(), ledger_id, Some(&format!("Not minted: {e}")));
            Err(format!("Couldn't create this run's capped OpenRouter key: {e}. Nothing was sent."))
        }
    }
}

pub(super) struct TurnCtx {
    pub(super) manager: Arc<SessionManager>,
    cancel: Arc<AtomicBool>,
    pub(super) notify: Arc<dyn Fn(u32) + Send + Sync>,
    pub(super) run_id: u32,
    pub(super) limits: tools::CouncilLimits,
}

impl TurnCtx {
    pub(super) fn stopping(&self) -> bool {
        self.cancel.load(Ordering::SeqCst) || self.manager.is_quitting()
    }
}

/// One participant's turn: the prompt is stored first, then the tool loop runs until the model
/// answers. Its outcome is stored whatever happens; an `Err` is a storage failure only.
fn take_turn(ctx: &TurnCtx, shared: &Arc<Shared>, who: &RosterEntry, phase: Phase, prompt: &str) -> Result<(), String> {
    let store = ctx.manager.store();
    let prompt = shared.scrub(prompt);
    let turn = db::open_turn(store, ctx.run_id, who.id, phase, &prompt)?;
    (ctx.notify)(ctx.run_id);
    let mut usage = Usage::default();
    let (outcome, content) = if who.runs_on.is_local() {
        super::local::converse(ctx, shared, who, turn, &prompt, &mut usage)
    } else {
        converse(ctx, shared, who, turn, &prompt, &mut usage)
    };
    db::finish_turn(store, turn, outcome, &shared.scrub(&content), &usage)?;
    (ctx.notify)(ctx.run_id);
    Ok(())
}

fn converse(ctx: &TurnCtx, shared: &Shared, who: &RosterEntry, turn: u32, prompt: &str, usage: &mut Usage) -> (TurnOutcome, String) {
    let store = ctx.manager.store();
    let Some(key) = shared.key.as_deref() else {
        return (TurnOutcome::Refused, "This run has no OpenRouter key, so an OpenRouter member couldn't answer.".into());
    };
    let mut messages = vec![json!({"role": "user", "content": prompt})];
    let mut budget = tools::Budget::default();
    let mut last_prompt_tokens: Option<u64> = None;
    let mut told_to_answer = false;
    // Calls, plus a few answers that came back as tool calls after the limit.
    for _ in 0..ctx.limits.max_calls + 4 {
        if ctx.stopping() {
            return (TurnOutcome::Cancelled, "Cancelled before this turn finished.".into());
        }
        let spent = budget.spent(&ctx.limits, last_prompt_tokens, who.context_length.map(u64::from));
        if let Some(reason) = &spent
            && !told_to_answer
        {
            messages.push(json!({"role": "user", "content": tools::answer_now(reason)}));
            told_to_answer = true;
        }
        let body = json!({
            "model": who.model,
            "messages": messages,
            "max_tokens": who.max_tokens,
            "tools": tools::definitions(),
            "tool_choice": if spent.is_some() { "none" } else { "auto" },
            "usage": {"include": true},
        });
        let response = match call(ctx, key, &body) {
            Ok(r) => r,
            Err(Failure::Cancelled) => return (TurnOutcome::Cancelled, "Cancelled before this turn finished.".into()),
            Err(Failure::Refused(reason)) => return (TurnOutcome::Refused, reason),
        };
        let reported = response_usage(&response);
        last_prompt_tokens = reported.tokens_in;
        usage.add(&reported);
        let Some(message) = response.pointer("/choices/0/message").cloned() else {
            let reason = response
                .pointer("/error/message")
                .map_or("The provider returned no answer.", |_| "The provider reported an error partway through the response.");
            return (TurnOutcome::Refused, reason.into());
        };
        let calls = message.get("tool_calls").and_then(Value::as_array).cloned().unwrap_or_default();
        if calls.is_empty() {
            let text = message.get("content").and_then(Value::as_str).unwrap_or("").trim().to_owned();
            if text.is_empty() {
                let why = if response.pointer("/choices/0/finish_reason").and_then(Value::as_str) == Some("length") {
                    "The model ran out of output tokens before it answered (its reasoning may have used them). Raise its max tokens in Settings → Council."
                } else {
                    "The model returned an empty answer."
                };
                return (TurnOutcome::Refused, why.into());
            }
            return (TurnOutcome::Answered, text);
        }
        if spent.is_some() {
            // It was told not to call tools and did anyway.
            let text = message.get("content").and_then(Value::as_str).unwrap_or("").trim().to_owned();
            return if text.is_empty() {
                (TurnOutcome::Refused, "The model kept calling tools after its limit and gave no answer.".into())
            } else {
                (TurnOutcome::Answered, text)
            };
        }
        // Kept whole (reasoning details included): some providers need them back with the results.
        messages.push(message);
        for call in calls {
            let id = call.get("id").and_then(Value::as_str).unwrap_or("").to_owned();
            let name = call.pointer("/function/name").and_then(Value::as_str).unwrap_or("");
            let args = call.pointer("/function/arguments").and_then(Value::as_str).unwrap_or("");
            let (described, ran) = match tools::parse(name, args) {
                Ok(c) => (c.describe(), shared.code.run(&c)),
                Err(e) => (format!("{name} (refused)"), super::code::Ran { ok: false, text: e }),
            };
            budget.calls += 1;
            if ran.ok {
                budget.bytes += ran.text.len() as u64;
            }
            let note = (!ran.ok).then_some(ran.text.as_str());
            let _ = db::add_tool_call(store, turn, &described, ran.ok, if ran.ok { ran.text.len() } else { 0 }, note);
            messages.push(json!({"role": "tool", "tool_call_id": id, "content": ran.text}));
        }
        (ctx.notify)(ctx.run_id);
    }
    (TurnOutcome::Refused, "The turn didn't finish within its tool limit.".into())
}

fn response_usage(v: &Value) -> Usage {
    let n = |p: &str| v.pointer(p).and_then(Value::as_u64);
    Usage {
        tokens_in: n("/usage/prompt_tokens"),
        tokens_out: n("/usage/completion_tokens"),
        tokens_cached: n("/usage/prompt_tokens_details/cached_tokens"),
        cost_usd: chorus_core::narrator::reported_cost(v),
    }
}

enum Failure {
    Cancelled,
    Refused(String),
}

/// A fixed sentence for each failure; the provider's body is never shown (it may echo input).
fn status_failure(status: u16) -> String {
    match status {
        400 => "The provider refused the request (HTTP 400). The model may not take tools, or the conversation got too long.".into(),
        401 | 403 => "The run's key was refused (HTTP 401/403).".into(),
        402 => "OpenRouter refused for credit (HTTP 402): the run's spend cap or the account's balance is used up.".into(),
        404 => "The provider doesn't know this model (HTTP 404). Check its name in Settings → Council.".into(),
        408 | 504 => format!("The provider timed out (HTTP {status})."),
        429 => "Rate limited by the provider (HTTP 429).".into(),
        500..=599 => format!("The provider returned an error (HTTP {status})."),
        s => format!("Unexpected response (HTTP {s})."),
    }
}

/// One chat completion, on a thread of its own so a cancel is seen within a quarter second. A
/// cancelled request is left to end by itself (its result is dropped; the run's key is revoked).
fn call(ctx: &TurnCtx, key: &str, body: &Value) -> Result<Value, Failure> {
    let (tx, rx) = std::sync::mpsc::channel();
    let key = key.to_owned();
    let body = body.clone();
    std::thread::Builder::new()
        .name("council-request".into())
        .spawn(move || {
            let _ = tx.send(request(&key, &body));
        })
        .map_err(|_| Failure::Refused("Couldn't start the request.".into()))?;
    loop {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(r) => return r.map_err(Failure::Refused),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) if ctx.stopping() => return Err(Failure::Cancelled),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Err(Failure::Refused("The request stopped unexpectedly.".into())),
        }
    }
}

fn request(key: &str, body: &Value) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Couldn't create a connection.".to_owned())?;
    let url = format!("{}/chat/completions", chorus_core::providers::OPENROUTER_API);
    tauri::async_runtime::block_on(async {
        let mut r = client
            .post(&url)
            .bearer_auth(key)
            .json(body)
            .send()
            .await
            .map_err(|e| if e.is_timeout() { "The request timed out.".to_owned() } else { "Couldn't reach OpenRouter.".to_owned() })?;
        if !r.status().is_success() {
            return Err(status_failure(r.status().as_u16()));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = r.chunk().await.map_err(|_| "The response ended unexpectedly.".to_owned())? {
            if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err("The response exceeded its size limit.".to_owned());
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice::<Value>(&bytes).map_err(|_| "The provider returned an unreadable response.".to_owned())
    })
}

/// `<brief dir>/<stem>-Findings.md`.
pub fn findings_path(brief: &Path) -> PathBuf {
    let stem = brief.file_stem().map_or_else(|| "brief".into(), |s| s.to_string_lossy().into_owned());
    brief.with_file_name(format!("{stem}-Findings.md"))
}

fn relative_to(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

/// Writes beside the brief, never over an existing file: `-Findings.md`, then `-Findings-2.md` …
/// `-Findings-99.md`.
fn write_findings(brief: &Path, text: &str) -> Result<PathBuf, String> {
    use std::io::Write as _;
    let first = findings_path(brief);
    let stem = brief.file_stem().map_or_else(|| "brief".into(), |s| s.to_string_lossy().into_owned());
    let candidates = std::iter::once(first).chain((2..=99).map(|n| brief.with_file_name(format!("{stem}-Findings-{n}.md"))));
    for path in candidates {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                f.write_all(text.as_bytes()).and_then(|()| f.sync_all()).map_err(|e| format!("Writing {}: {e}", path.display()))?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("Couldn't write the findings beside the brief: {e}. They are shown in Chorus.")),
        }
    }
    Err("Findings files -Findings.md to -Findings-99.md all exist beside this brief; nothing was overwritten. The findings are shown in Chorus.".into())
}

/// The findings text for the screen: the file if it's there, or the reason it isn't.
pub fn read_findings(run: &RunRow) -> (Option<String>, Option<String>) {
    match &run.findings_path {
        Some(p) => match std::fs::metadata(p) {
            Ok(m) if m.len() > 4_000_000 => (None, Some(format!("{p} is too large to show here; open it instead."))),
            Ok(_) => match std::fs::read_to_string(p) {
                Ok(t) => (Some(t), None),
                Err(e) => (None, Some(format!("Couldn't read {p}: {e}"))),
            },
            Err(_) => (None, Some(format!("The findings file is no longer at {p}."))),
        },
        None => (None, run.findings_error.clone()),
    }
}

#[cfg(test)]
impl Shared {
    /// A run's shared state with no key, for the local-turn server's tests.
    pub(super) fn for_test(code: Code, secrets: Vec<Vec<u8>>) -> Self {
        Self { key: None, code, secrets, patterns: vec![] }
    }
}

#[cfg(test)]
impl TurnCtx {
    /// A turn's context outside a run, for the local-turn tests.
    pub(super) fn for_test(manager: Arc<SessionManager>, run_id: u32) -> Self {
        Self { manager, cancel: Arc::new(AtomicBool::new(false)), notify: Arc::new(|_| {}), run_id, limits: tools::CouncilLimits::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn findings_never_overwrite() {
        let dir = std::env::temp_dir().join(format!("chorus-council-findings-{}-{}", std::process::id(), crate::store::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let brief = dir.join("CouncilBrief-X.md");
        assert_eq!(findings_path(&brief), dir.join("CouncilBrief-X-Findings.md"));
        let a = write_findings(&brief, "one").unwrap();
        let b = write_findings(&brief, "two").unwrap();
        assert_eq!(a, dir.join("CouncilBrief-X-Findings.md"));
        assert_eq!(b, dir.join("CouncilBrief-X-Findings-2.md"));
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "one");
        assert_eq!(relative_to(&dir, &a).as_deref(), Some("CouncilBrief-X-Findings.md"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn usage_reads_cost_and_cache() {
        let u = response_usage(&json!({"usage": {"prompt_tokens": 10, "completion_tokens": 3, "prompt_tokens_details": {"cached_tokens": 4}, "cost": 0.002}}));
        assert_eq!((u.tokens_in, u.tokens_out, u.tokens_cached, u.cost_usd), (Some(10), Some(3), Some(4), Some(0.002)));
        assert!(response_usage(&json!({})).cost_usd.is_none());
    }
}

