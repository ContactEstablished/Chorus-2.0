//! Chorus updates (D135): short messages to Matthew through OpenMausBot, in one conversation
//! ("Chorus updates") with the bot chosen in Settings → MausBot (Jammie by default).
//!
//! - **Ad hoc:** a Claude session calls `maus_send` on its MCP server (`/mcp/session/<id>` on the
//!   loopback listener, authorised by the session's hook token). The result says sent, or why not.
//! - **Automatic:** while Matthew is away, a session's turn ending (done, failed, needs you), its
//!   close, and a loop run stopping, finishing or waiting for review each send one notice, after
//!   the source has been quiet for [`SETTLE_MS`]. Loop steps never do: the run's own notice covers
//!   them.
//!
//! Every message is scrubbed, capped at 1 KB, rate-limited and logged (migration 0031) before it
//! goes. A failure is skipped, never retried in a loop, and its reason shown in Settings and
//! returned to a session. MausBot's key stays in this process: only `maus_client` puts it in a
//! header.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use chorus_core::activity::Activity;
use chorus_core::maus::Bot;
use chorus_core::maus_updates::{self as core, Debounce, Kind, LIMITS, Limiter, NoticeFacts, Origin, SETTLE_MS, Source};
use chorus_core::run_loop::tools::ToolResult;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use super::connection;
use super::maus_client::{self as client, Failure};
use super::refresh::Scrubber;
use crate::run_loop::db as loop_db;
use crate::sessions::{SessionManager, SessionStatus, SessionSummary};
use crate::store::{self, maus_updates as log, now_ms, settings};

const SETTINGS_KEY: &str = "mausbot.updates";

/// Settings → MausBot's switches and the conversation, kept as plain settings (nothing secret).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Stored {
    /// `None` until Matthew switches it: on by default.
    pub(super) enabled: Option<bool>,
    pub(super) away: bool,
    /// The bot Matthew chose; `None` is the one named Jammie.
    pub(super) bot_id: Option<String>,
    /// The "Chorus updates" conversation, and the bot it belongs to.
    pub(super) thread_bot: Option<String>,
    pub(super) thread_id: Option<String>,
}

impl Stored {
    fn enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
}

pub(super) fn stored(m: &SessionManager) -> Stored {
    settings::get(m.store(), SETTINGS_KEY).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub(super) fn save(m: &SessionManager, s: &Stored) -> Result<(), String> {
    settings::set(m.store(), SETTINGS_KEY, &serde_json::to_string(s).map_err(|e| e.to_string())?)
}

/// MausBot as the sender sees it, so tests can stand in for it.
pub trait MausApi: Send + Sync {
    fn token(&self, m: &SessionManager) -> Result<Option<String>, String>;
    fn bots(&self) -> Result<Vec<Bot>, Failure>;
    fn create(&self, token: &str, bot_id: &str, title: &str) -> Result<String, Failure>;
    fn send(&self, token: &str, bot_id: &str, thread_id: &str, text: &str, send_id: &str) -> Result<(), Failure>;
}

/// The running OpenMausBot.
pub struct Live;

impl MausApi for Live {
    fn token(&self, m: &SessionManager) -> Result<Option<String>, String> {
        connection::token(m)
    }
    fn bots(&self) -> Result<Vec<Bot>, Failure> {
        client::bots()
    }
    fn create(&self, token: &str, bot_id: &str, title: &str) -> Result<String, Failure> {
        client::create_conversation(token, bot_id, None, title)
    }
    fn send(&self, token: &str, bot_id: &str, thread_id: &str, text: &str, send_id: &str) -> Result<(), Failure> {
        client::send(token, bot_id, thread_id, text, send_id)
    }
}

static LIMITER: Mutex<Option<Limiter>> = Mutex::new(None);
/// One message at a time: two at once could each make a "Chorus updates" conversation.
static SENDING: Mutex<()> = Mutex::new(());
static QUEUE: OnceLock<Mutex<Sender<Msg>>> = OnceLock::new();
/// Each session's last seen (working, closed), to see a turn end or a close.
static LAST: Mutex<Option<HashMap<u32, (bool, bool)>>> = Mutex::new(None);

/// Settings → MausBot reads the view again (a message was logged, or a switch moved).
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct MausUpdatesChanged {
    pub away: bool,
}

fn changed(m: &SessionManager) {
    let away = stored(m).away;
    crate::tray::away_checked(away);
    if let Some(app) = super::APP.get() {
        let _ = MausUpdatesChanged { away }.emit(app);
    }
}

/// Where a message comes from, for the log and the limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Whence {
    pub source: Source,
    pub session_id: Option<u32>,
    pub run_id: Option<u32>,
}

/// What a sent message reached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Sent {
    pub bot_name: String,
}

/// Sends `body` (scrubbed here, then capped) to the bot's "Chorus updates" conversation. `Err` is
/// the reason it was skipped, in words, already logged. Never retries beyond one new conversation
/// when the stored one has gone.
pub fn deliver(m: &SessionManager, api: &dyn MausApi, from: Whence, kind: &str, body: &str) -> Result<Sent, String> {
    let outcome = deliver_logged(m, api, from, kind, body);
    changed(m);
    outcome
}

fn deliver_logged(m: &SessionManager, api: &dyn MausApi, from: Whence, kind: &str, body: &str) -> Result<Sent, String> {
    let store = m.store();
    let skip = |text: &str, reason: String| -> Result<Sent, String> {
        log::skipped(store, from.session_id, from.run_id, kind, text, &reason)?;
        Err(reason)
    };
    let scrubber = match Scrubber::new(m) {
        Ok(s) => s,
        // The text can't be kept unscrubbed, so the log says only why.
        Err(e) => return skip("(not kept: Chorus couldn't scrub it)", e),
    };
    let (kept, full) = core::finish(&scrubber.clean(body));
    let settings = stored(m);
    if !settings.enabled() {
        return skip(&kept, "Messages to MausBot are switched off in Settings → MausBot.".into());
    }
    {
        let mut guard = LIMITER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Err(reason) = guard.get_or_insert_with(Limiter::default).check(from.source, LIMITS, now_ms()) {
            return skip(&kept, reason);
        }
    }
    let id = log::begin(store, from.session_id, from.run_id, kind, &kept)?;
    let outcome = send_once(m, api, from.source, id, &full).map_err(|reason| scrubber.clean(&reason));
    log::finish(store, id, outcome.as_ref().err().map(String::as_str))?;
    outcome
}

fn send_once(m: &SessionManager, api: &dyn MausApi, source: Source, log_id: i64, full: &str) -> Result<Sent, String> {
    let _one = SENDING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let token = match api.token(m) {
        Ok(Some(t)) => t,
        Ok(None) => return Err("Chorus isn't paired with MausBot. Pair it in Settings → MausBot.".into()),
        Err(e) => return Err(format!("Chorus can't read its MausBot key: {e}")),
    };
    let bots = api.bots().map_err(|f| f.text())?;
    let mut settings = stored(m);
    let bot = core::pick_bot(&bots, settings.bot_id.as_deref()).ok_or_else(|| match &settings.bot_id {
        Some(_) => "The bot chosen in Settings → MausBot is no longer in MausBot. Choose another.".to_owned(),
        None => format!("MausBot has no bot named {}. Choose one in Settings → MausBot.", core::DEFAULT_BOT_NAME),
    })?;
    // It counts from here: MausBot is up, and a message is about to reach it.
    LIMITER.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get_or_insert_with(Limiter::default).record(source, now_ms());
    let nonce = || crate::vault::random_bytes(8).iter().map(|b| format!("{b:02x}")).collect::<String>();
    let known = settings.thread_bot.as_deref() == Some(bot.id.as_str()) && settings.thread_id.as_deref().is_some_and(|t| core::has_thread(bot, t));
    let mut thread = if known { settings.thread_id.clone().unwrap_or_default() } else { new_thread(m, api, &token, bot, &mut settings)? };
    match api.send(&token, &bot.id, &thread, full, &core::send_id(log_id, &nonce())) {
        Ok(()) => return Ok(Sent { bot_name: bot.name.clone() }),
        Err(Failure::Refused(e)) => {
            // Deleted in MausBot since the list was read: one new conversation, one more send.
            let gone = api.bots().ok().is_some_and(|now| now.iter().find(|b| b.id == bot.id).is_some_and(|b| !core::has_thread(b, &thread)));
            if !gone {
                return Err(Failure::Refused(e).text());
            }
            thread = new_thread(m, api, &token, bot, &mut settings)?;
        }
        Err(f) => return Err(f.text()),
    }
    api.send(&token, &bot.id, &thread, full, &core::send_id(log_id, &nonce())).map(|()| Sent { bot_name: bot.name.clone() }).map_err(|f| f.text())
}

fn new_thread(m: &SessionManager, api: &dyn MausApi, token: &str, bot: &Bot, settings: &mut Stored) -> Result<String, String> {
    let thread = api.create(token, &bot.id, core::CONVERSATION_TITLE).map_err(|f| format!("Chorus couldn't make the \"{}\" conversation: {}", core::CONVERSATION_TITLE, f.text()))?;
    settings.thread_bot = Some(bot.id.clone());
    settings.thread_id = Some(thread.clone());
    save(m, settings)?;
    Ok(thread)
}

// ---- Where a message comes from ----

fn session_origin(m: &SessionManager, session_id: u32) -> Result<Origin, String> {
    let row = store::sessions::get(m.store(), session_id)?.ok_or("That session no longer exists.")?;
    let project = match row.project_id {
        Some(p) => store::projects::get(m.store(), p)?.map(|p| p.name),
        None => None,
    };
    let branch = crate::git::facts(Path::new(&row.cwd)).ok().flatten().and_then(|f| f.branch);
    Ok(Origin { project, session: Some(row.name), branch })
}

fn run_origin(run: &loop_db::RunRow) -> Origin {
    let project = Path::new(&run.repo_root).file_name().map(|n| n.to_string_lossy().into_owned());
    Origin { project, session: Some(format!("loop {}", run.name)), branch: Some(run.run_branch.clone()) }
}

/// The run's newest checks, as a line.
fn run_tests(m: &SessionManager, run_id: u32) -> Option<String> {
    let steps = loop_db::steps(m.store(), run_id).ok()?;
    steps.iter().rev().find_map(|s| {
        let checks = loop_db::checks(m.store(), s.id).ok()?;
        core::tests_line(&checks.iter().map(|c| (c.name.clone(), c.passed)).collect::<Vec<_>>())
    })
}

// ---- Ad hoc: the session's tool ----

/// One POST from a session's agent to its MCP server: `Err(())` when the token isn't the session's.
pub fn serve(m: &SessionManager, session_id: u32, token: &str, body: &str) -> Result<Result<Option<String>, String>, ()> {
    if !m.hook_token_matches(session_id, token) {
        return Err(());
    }
    Ok(chorus_core::run_loop::tools::respond_with(body, &core::tool_list(), |name, args| tool_call(m, session_id, name, args)))
}

fn tool_call(m: &SessionManager, session_id: u32, name: &str, args: &serde_json::Value) -> ToolResult {
    if name != core::TOOL_NAME {
        return ToolResult::error(format!("There is no tool called {name}."));
    }
    let message = args.get("message").and_then(serde_json::Value::as_str).unwrap_or("");
    let body = match session_origin(m, session_id).and_then(|o| core::ad_hoc(&o, message)) {
        Ok(b) => b,
        Err(e) => return ToolResult::error(format!("Not sent: {e}")),
    };
    let from = Whence { source: Source::Session(session_id), session_id: Some(session_id), run_id: None };
    match deliver(m, &Live, from, "ad-hoc", &body) {
        Ok(sent) => ToolResult::ok(format!(
            "Sent to {} in MausBot's \"{}\" conversation. Matthew's phone is notified when {} replies.",
            sent.bot_name,
            core::CONVERSATION_TITLE,
            sent.bot_name
        )),
        Err(reason) => ToolResult::error(format!("Not sent: {reason}")),
    }
}

// ---- Automatic notices ----

enum Msg {
    Turn { session: u32, kind: Kind, closed: bool },
    Working { session: u32 },
    Run { run: u32, kind: Kind },
}

#[derive(Debug, Clone, Copy)]
pub(super) enum Pending {
    Session { session: u32, kind: Kind, closed: bool },
    Run { run: u32, kind: Kind },
}

fn queue(e: Msg) {
    if let Some(q) = QUEUE.get() {
        let _ = q.lock().unwrap_or_else(std::sync::PoisonError::into_inner).send(e);
    }
}

/// Every session summary, from `lib.rs`'s notify closure. Never blocks and does no IO.
pub fn observe(s: &SessionSummary) {
    if s.tags.iter().any(|t| t == "loop") {
        return;
    }
    let now = (s.activity == Activity::Working, matches!(s.status, SessionStatus::Closed));
    let before = LAST.lock().unwrap_or_else(std::sync::PoisonError::into_inner).get_or_insert_with(HashMap::new).insert(s.id, now);
    if now.0 && before.is_some_and(|(working, _)| !working) {
        queue(Msg::Working { session: s.id });
        return;
    }
    if !super::triggers(before, now) {
        return;
    }
    if now.1 {
        let failed = s.last_exit_code.is_some_and(|c| c != 0) && s.last_end.as_deref() == Some("exited");
        queue(Msg::Turn { session: s.id, kind: if failed { Kind::Failed } else { Kind::Done }, closed: true });
    } else if let Some(kind) = Kind::of_turn_end(s.activity) {
        queue(Msg::Turn { session: s.id, kind, closed: false });
    }
}

/// A loop run stopped on its own, finished, or waits for Matthew's review.
pub fn run_changed(run: u32, state: loop_db::RunState) {
    use loop_db::RunState as S;
    let kind = match state {
        S::Done => Kind::Done,
        S::Stopped => Kind::Failed,
        S::Waiting => Kind::NeedsYou,
        S::Draft | S::Running | S::Paused => return,
    };
    queue(Msg::Run { run, kind });
}

pub fn start(m: &Arc<SessionManager>) -> Result<(), String> {
    let (tx, rx) = channel();
    let _ = QUEUE.set(Mutex::new(tx));
    let worker = m.clone();
    std::thread::Builder::new().name("maus-updates".into()).spawn(move || run(&worker, &rx)).map(drop).map_err(|e| e.to_string())
}

fn run(m: &Arc<SessionManager>, rx: &Receiver<Msg>) {
    let mut pending: Debounce<Pending> = Debounce::default();
    while !m.is_quitting() {
        let wait = pending.next_at().map_or(1000, |at| (at - now_ms()).clamp(0, 1000));
        match rx.recv_timeout(Duration::from_millis(wait as u64)) {
            Ok(Msg::Turn { session, kind, closed }) => pending.queue(Source::Session(session), Pending::Session { session, kind, closed }, now_ms(), SETTLE_MS),
            Ok(Msg::Working { session }) => pending.cancel(Source::Session(session)),
            Ok(Msg::Run { run, kind }) => pending.queue(Source::Run(run), Pending::Run { run, kind }, now_ms(), SETTLE_MS),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        for (source, notice) in pending.due(now_ms()) {
            let settings = stored(m);
            // Only while Matthew is away: at the desk he sees it in Chorus.
            if !settings.enabled() || !settings.away {
                continue;
            }
            let (from, kind, body) = match notice_body(m, notice) {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("maus-updates: {e}");
                    continue;
                }
            };
            let _ = deliver(m, &Live, Whence { source, ..from }, kind.as_str(), &body);
        }
    }
}

pub(super) fn notice_body(m: &SessionManager, n: Pending) -> Result<(Whence, Kind, String), String> {
    match n {
        Pending::Session { session, kind, closed } => {
            let origin = session_origin(m, session)?;
            let reply = store::handoff::latest_reply(m.store(), session)?.map(|(t, _)| t);
            let plan: Vec<(String, String)> = store::progress::latest_plan(m.store(), session)?.unwrap_or_default().into_iter().map(|s| (s.text, s.status)).collect();
            let what = core::what_finished(reply.as_deref(), &plan);
            let what = if closed { Some(what.map_or_else(|| "Session closed.".to_owned(), |w| format!("Session closed. {w}"))) } else { what };
            let from = Whence { source: Source::Session(session), session_id: Some(session), run_id: None };
            Ok((from, kind, core::notice(&origin, kind, &NoticeFacts { what, tests: None })))
        }
        Pending::Run { run, kind } => {
            let row = loop_db::run(m.store(), run)?.ok_or("That loop run no longer exists.")?;
            let facts = NoticeFacts { what: row.note.clone(), tests: run_tests(m, run) };
            let from = Whence { source: Source::Run(run), session_id: None, run_id: Some(run) };
            Ok((from, kind, core::notice(&run_origin(&row), kind, &facts)))
        }
    }
}

// ---- Settings → MausBot ----

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BotChoice {
    pub id: String,
    pub name: String,
}

/// Settings → MausBot's "Messages to you" part.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdatesView {
    pub enabled: bool,
    pub away: bool,
    /// The bot Matthew chose; `None` means the one named Jammie.
    pub chosen_bot_id: Option<String>,
    /// The bot messages go to now, if MausBot lists it.
    pub bot: Option<BotChoice>,
    pub bots: Vec<BotChoice>,
    pub default_bot_name: String,
    pub conversation_title: String,
    pub has_conversation: bool,
    /// Why messages can't go now, if Chorus can tell.
    pub problem: Option<String>,
    pub recent: Vec<log::UpdateRow>,
}

pub fn view(m: &SessionManager) -> Result<UpdatesView, String> {
    let s = stored(m);
    let scrubber = Scrubber::new(m)?;
    let (bots, mut problem) = match client::bots() {
        Ok(list) => (list, None),
        Err(f) => (Vec::new(), Some(scrubber.clean(&f.text()))),
    };
    let bot = core::pick_bot(&bots, s.bot_id.as_deref());
    if problem.is_none() && bot.is_none() {
        problem = Some(match s.bot_id {
            Some(_) => "The bot chosen here is no longer in MausBot. Choose another.".into(),
            None => format!("MausBot has no bot named {}. Choose one below.", core::DEFAULT_BOT_NAME),
        });
    }
    if problem.is_none() && !matches!(connection::token(m), Ok(Some(_))) {
        problem = Some("Chorus isn't paired with MausBot, so it can't send. Pair it above.".into());
    }
    let choice = |b: &Bot| BotChoice { id: b.id.clone(), name: scrubber.clean(&b.name) };
    Ok(UpdatesView {
        enabled: s.enabled(),
        away: s.away,
        chosen_bot_id: s.bot_id.clone(),
        bot: bot.map(choice),
        has_conversation: bot.is_some_and(|b| s.thread_bot.as_deref() == Some(b.id.as_str()) && s.thread_id.as_deref().is_some_and(|t| core::has_thread(b, t))),
        bots: bots.iter().map(choice).collect(),
        default_bot_name: core::DEFAULT_BOT_NAME.into(),
        conversation_title: core::CONVERSATION_TITLE.into(),
        problem,
        recent: log::recent(m.store(), 20)?,
    })
}

/// Settings → MausBot's changes. `bot_id`: `Some("")` goes back to the default bot.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdatesChange {
    pub enabled: Option<bool>,
    pub away: Option<bool>,
    pub bot_id: Option<String>,
}

pub fn change(m: &SessionManager, c: &UpdatesChange) -> Result<(), String> {
    let mut s = stored(m);
    if let Some(e) = c.enabled {
        s.enabled = Some(e);
    }
    if let Some(a) = c.away {
        s.away = a;
    }
    if let Some(id) = &c.bot_id {
        if id.is_empty() {
            s.bot_id = None;
        } else if chorus_core::maus::valid_id(id) {
            s.bot_id = Some(id.clone());
        } else {
            return Err("That isn't a MausBot bot id.".into());
        }
    }
    save(m, &s)?;
    changed(m);
    Ok(())
}

pub fn is_away(m: &SessionManager) -> bool {
    stored(m).away
}

/// Settings → MausBot's Send test message.
pub fn test(m: &SessionManager) -> Result<String, String> {
    let body = format!("{}\nTest message from Settings → MausBot. If your phone showed this, Chorus can reach you.", core::prefix(&Origin::default()));
    let sent = deliver(m, &Live, Whence { source: Source::Test, session_id: None, run_id: None }, "test", &body)?;
    Ok(format!(
        "Sent to {name}. Your phone should show \"{name} finished\" once {name} replies; if it doesn't, check that {name}'s notifications are on in MausBot.",
        name = sent.bot_name
    ))
}

#[cfg(test)]
pub(super) fn forget_all() {
    *LIMITER.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    *LAST.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}
