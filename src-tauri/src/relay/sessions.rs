//! Ordinary sessions on the phone (P15 slice 4, HANDOFF-P13's P13a, D101). The relay's own
//! subscription to session summaries (HANDOFF-COMMON §3: not through P7's notifications): `observe`
//! only queues a summary, because summaries are published while session locks are held, and this
//! module's thread does the rest. It sends an ask (with Allow once / Deny or option buttons while
//! Claude Code holds it), a failure, and a finished turn when the phone started it or the desk is
//! idle; settles an ask's card however it ended; delivers prompts from the phone through P6's
//! delivery path; and answers `/sessions`, `/use` and `/digest`, with a daily digest if one is set.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use chorus_core::actions::{ApprovalDecision, PromptSource, SessionAction};
use chorus_core::attention::Attention;
use chorus_core::relay::render::{self, Act};
use chorus_core::relay::sessions::{self as view, AskCard, Change, DigestFacts, Held, SessionLine, SessionRef};
use chorus_core::replies::ReplyDetails;
use chorus_core::schedule::{self as sched, Schedule};

use super::db::{self, About, Subject};
use super::outbound::{self, listening};
use crate::events::replies::PendingReply;
use crate::sessions::actions::{self, ActionRequest, ActionRouter};
use crate::sessions::{SessionManager, SessionStatus, SessionSummary};
use crate::store::now_ms;
use crate::vault;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

static QUEUE: OnceLock<Mutex<Sender<SessionSummary>>> = OnceLock::new();
/// Sessions whose current turn the phone started: their finished turn is always sent.
static PHONE_TURNS: Mutex<Option<HashSet<u32>>> = Mutex::new(None);
/// The last `/sessions` listing, so `/use 2` means the session numbered 2 there.
static LAST_LIST: Mutex<Vec<SessionLine>> = Mutex::new(Vec::new());

fn phone_turns<T>(f: impl FnOnce(&mut HashSet<u32>) -> T) -> T {
    let mut guard = PHONE_TURNS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(guard.get_or_insert_with(HashSet::new))
}

/// Every session summary, from `lib.rs`'s notify closure. Never blocks.
pub fn observe(s: &SessionSummary) {
    if let Some(q) = QUEUE.get() {
        let _ = q.lock().unwrap_or_else(std::sync::PoisonError::into_inner).send(s.clone());
    }
}

pub fn start(m: &Arc<SessionManager>) -> Result<(), String> {
    let (tx, rx) = channel();
    let _ = QUEUE.set(Mutex::new(tx));
    let watcher = m.clone();
    std::thread::Builder::new().name("relay-sessions".into()).spawn(move || watch(&watcher, &rx)).map_err(|e| e.to_string())?;
    let digester = m.clone();
    std::thread::Builder::new().name("relay-digest".into()).spawn(move || digest_loop(&digester)).map_err(|e| e.to_string())?;
    Ok(())
}

/// A loop step's session: the loop tells the phone about its own steps.
fn loop_step(s: &SessionSummary) -> bool {
    s.tags.iter().any(|t| t == "loop")
}

fn watch(m: &Arc<SessionManager>, rx: &Receiver<SessionSummary>) {
    let mut prev: HashMap<u32, Attention> = HashMap::new();
    while !m.is_quitting() {
        let s = match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(s) => s,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if loop_step(&s) {
            continue;
        }
        let before = prev.get(&s.id).copied();
        let now = if matches!(s.status, SessionStatus::Closed) { Attention::Done } else { s.attention };
        prev.insert(s.id, now);
        if !listening(m) {
            continue;
        }
        let seen = view::change(before, now);
        if seen.settle {
            outbound::settle(m, Subject::Session(s.id), "session-ask", view::NO_LONGER_WAITING);
        }
        let sent = match seen.send {
            Some(Change::Asked(a)) => ask(m, &s, a),
            Some(Change::Failed) => send(m, &s, "session-failed", &view::failed(&reference(m, &s).as_ref(), s.note.as_deref())),
            Some(Change::TurnDone) => turn_done(m, &s),
            None => Ok(()),
        };
        if let Err(e) = sent {
            eprintln!("chorus: relay: session {}: {e}", s.id);
        }
    }
}

/// A session's name and project name, owned.
struct Named {
    name: String,
    project: Option<String>,
}

impl Named {
    fn as_ref(&self) -> SessionRef<'_> {
        SessionRef { name: &self.name, project: self.project.as_deref() }
    }
}

fn project_name(m: &SessionManager, project: Option<u32>) -> Option<String> {
    project.and_then(|id| crate::store::projects::get(m.store(), id).ok().flatten()).map(|p| p.name)
}

fn reference(m: &SessionManager, s: &SessionSummary) -> Named {
    Named { name: s.name.clone(), project: project_name(m, s.project_id) }
}

/// The project opted in to full text on the phone.
fn full_text(m: &SessionManager, s: &SessionSummary) -> bool {
    s.project_id.is_some_and(|p| db::full_text_projects(m.store()).is_ok_and(|set| set.contains(&p)))
}

fn send(m: &SessionManager, s: &SessionSummary, kind: &str, out: &render::Outgoing) -> Result<(), String> {
    outbound::enqueue(m, kind, About::session(s.id), out)
}

/// The held reply matching what the session waits on, if the agent holds one. The summary can be
/// published a moment before the ask is registered, so it is looked for a few times.
fn held_reply(m: &SessionManager, session: u32, attention: Attention) -> Option<PendingReply> {
    for _ in 0..3 {
        let found = actions::pending_replies(m, session).unwrap_or_default().into_iter().find(|p| match p.details {
            ReplyDetails::Approval { .. } => attention == Attention::Approval,
            ReplyDetails::Question { .. } => attention == Attention::Input,
        });
        if found.is_some() {
            return found;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    None
}

fn ask(m: &SessionManager, s: &SessionSummary, attention: Attention) -> Result<(), String> {
    let pending = held_reply(m, s.id, attention);
    let named = reference(m, s);
    let held = pending.as_ref().filter(|p| p.blocked_reason.is_none()).map(|p| Held { session: s.id, run: p.run_id, ask: p.ask_id });
    let card = AskCard {
        session: named.as_ref(),
        attention,
        details: pending.as_ref().map(|p| &p.details),
        held,
        full: full_text(m, s),
        latest: s.latest.as_deref(),
    };
    let about = About { session_id: Some(s.id), ask_id: pending.as_ref().map(|p| p.ask_id), ..About::default() };
    outbound::enqueue(m, "session-ask", about, &view::ask_card(&card))
}

fn turn_done(m: &SessionManager, s: &SessionSummary) -> Result<(), String> {
    let phone = phone_turns(|t| t.remove(&s.id));
    if !view::tell_turn_done(phone, desk_idle_secs()) {
        return Ok(());
    }
    send(m, s, "session-done", &view::turn_done(&reference(m, s).as_ref(), s.latest.as_deref(), full_text(m, s)))
}

/// Seconds since the last keyboard or mouse input on this desktop.
#[cfg(windows)]
fn desk_idle_secs() -> u64 {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    // SAFETY: `info` is a LASTINPUTINFO with its size set, alive for the call.
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return 0;
    }
    // SAFETY: no arguments; the tick count wraps after 49 days, as dwTime does.
    u64::from(unsafe { GetTickCount() }.wrapping_sub(info.dwTime)) / 1000
}

#[cfg(not(windows))]
fn desk_idle_secs() -> u64 {
    0
}

fn new_request_id() -> String {
    let bytes: [u8; 16] = vault::random_bytes(16).try_into().unwrap_or([7; 16]);
    chorus_core::agent::uuid_v4(bytes)
}

/// A prompt from the phone to a session, through the same delivery path as the desk's Friendly
/// composer, recorded with source `telegram`. Returns the phone's reply.
pub fn prompt(m: &SessionManager, session: u32, text: &str) -> String {
    let Ok(s) = m.summary(session) else { return "That session no longer exists.".into() };
    if matches!(s.status, SessionStatus::Closed) {
        return format!("{} is closed.", render::escape(&s.name));
    }
    if let Some(why) = view::prompt_refusal(&s.name, s.locked, s.live, s.activity) {
        return render::escape(&why);
    }
    let Some(run) = actions::current_run_id(m, session) else { return format!("{} isn't running. Restart it at your desk first.", render::escape(&s.name)) };
    match crate::sessions::delivery::deliver_prompt(m, session, run, &new_request_id(), text, PromptSource::Telegram) {
        Ok(_) => {
            phone_turns(|t| t.insert(session));
            format!("Sent to <b>{}</b>.", render::escape(&s.name))
        }
        Err(e) => format!("Couldn't send to {}: {}", render::escape(&s.name), render::escape(&e)),
    }
}

/// A button on a session's ask: P6's action router answers the held ask, once. Returns the press's
/// short note.
pub fn press(m: &SessionManager, act: &Act) -> String {
    let (session, run, ask, action) = match act {
        Act::Allow { session, run, ask } => (*session, *run, *ask, SessionAction::Approval { ask_id: *ask, decision: ApprovalDecision::Allow, message: None }),
        Act::Deny { session, run, ask } => (*session, *run, *ask, SessionAction::Approval { ask_id: *ask, decision: ApprovalDecision::Deny, message: None }),
        Act::Choose { session, run, ask, answer } => (*session, *run, *ask, SessionAction::Answer { ask_id: *ask, answers: vec![answer.clone()] }),
        _ => return "That button no longer does anything.".into(),
    };
    answer(m, session, run, ask, action, &view::answered_from_phone(act)).map_or_else(|e| e, |()| outbound::act_label(act).to_owned())
}

fn answer(m: &SessionManager, session: u32, run: u32, ask: u32, action: SessionAction, outcome: &str) -> Result<(), String> {
    let request = ActionRequest { run_id: run, request_id: new_request_id(), action };
    match ActionRouter::dispatch(m, session, request) {
        Ok(_) => {
            db::audit(m.store(), "session-answer", None, &format!("session {session} ask {ask}"));
            outbound::settle(m, Subject::Session(session), "session-ask", outcome);
            Ok(())
        }
        Err(e) => Err(format!("Too late for the phone: {e} Answer it at your desk.")),
    }
}

/// A reply in words to a session's message: the answer to its held question, or else its next
/// prompt.
pub fn reply(m: &SessionManager, session: u32, kind: &str, ask: Option<u32>, text: &str) -> String {
    if kind == "session-ask"
        && let Some(ask) = ask
    {
        let pending = actions::pending_replies(m, session).unwrap_or_default();
        return match pending.iter().find(|p| p.ask_id == ask) {
            Some(p) => match &p.details {
                ReplyDetails::Approval { .. } => "Tap Allow once or Deny, or answer it at your desk.".into(),
                ReplyDetails::Question { questions } if questions.len() == 1 => {
                    let action = SessionAction::Answer { ask_id: ask, answers: vec![text.to_owned()] };
                    let outcome = format!("Answered from the phone: {}", render::clip(text, 80));
                    match answer(m, session, p.run_id, ask, action, &outcome) {
                        Ok(()) => format!("Answered: {}", render::escape(&render::clip(text, 200))),
                        Err(e) => render::escape(&e),
                    }
                }
                ReplyDetails::Question { .. } => "It asked several things: answer them at your desk.".into(),
            },
            None => "That ask is no longer waiting.".into(),
        };
    }
    prompt(m, session, text)
}

/// Open sessions, in `/sessions` order, without the loop's steps.
pub fn lines(m: &SessionManager) -> Vec<SessionLine> {
    let now = now_ms();
    let mut lines: Vec<SessionLine> = m
        .list()
        .unwrap_or_default()
        .into_iter()
        .filter(|s| !loop_step(s) && !matches!(s.status, SessionStatus::Closed))
        .map(|s| SessionLine {
            id: s.id,
            project: project_name(m, s.project_id),
            name: s.name,
            attention: s.attention,
            waiting_secs: s.needs_you_since.map(|at| u64::try_from(now - at as i64).unwrap_or(0) / 1000),
        })
        .collect();
    view::order(&mut lines);
    lines
}

/// `/sessions`: numbered, and remembered for `/use`.
pub fn list_command(m: &SessionManager) -> String {
    let lines = lines(m);
    let target = db::config(m.store()).ok().and_then(|c| c.target_session);
    let text = view::sessions_list(&lines, target);
    *LAST_LIST.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = lines;
    text
}

/// `/use`: where plain messages go.
pub fn use_command(m: &SessionManager, arg: &str) -> String {
    let store = m.store();
    let fresh = lines(m);
    if arg.trim().is_empty() {
        let target = db::config(store).ok().and_then(|c| c.target_session);
        return match target.and_then(|t| fresh.iter().find(|l| l.id == t)) {
            Some(l) => format!("Plain messages go to <b>{}</b>. /use off stops.", render::escape(&l.name)),
            None => "Plain messages go nowhere yet. /sessions lists the sessions; /use 2 picks number 2.".into(),
        };
    }
    // A number means the one shown by the last /sessions; a name, any open session.
    let listed = LAST_LIST.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    let by_number = arg.trim().parse::<usize>().is_ok() && !listed.is_empty();
    let found = view::resolve_use(if by_number { &listed } else { &fresh }, arg);
    match found {
        Ok(Some(id)) => match fresh.iter().find(|l| l.id == id) {
            Some(l) => {
                let _ = db::set_target(store, Some(id));
                format!("Plain messages now go to <b>{}</b>. /use off stops.", render::escape(&l.name))
            }
            None => "That session has closed since /sessions. Send /sessions again.".into(),
        },
        Ok(None) => {
            let _ = db::set_target(store, None);
            "Plain messages go nowhere now. Reply to a session's message to send it a prompt.".into()
        }
        Err(e) => render::escape(&e),
    }
}

/// Where a plain message goes: the `/use` session, if it is still open.
pub fn target(m: &SessionManager) -> Option<u32> {
    let id = db::config(m.store()).ok()?.target_session?;
    m.summary(id).ok().filter(|s| !matches!(s.status, SessionStatus::Closed)).map(|s| s.id)
}

/// The start of today, local time, in ms.
fn today_start(m: &SessionManager) -> i64 {
    let now = now_ms();
    let offset = i64::from(crate::automations::offset_at(m.store(), now)) * 60_000;
    let local = now + offset;
    local - local.rem_euclid(DAY_MS) - offset
}

pub fn digest(m: &SessionManager) -> render::Outgoing {
    let all = lines(m);
    let since = today_start(m) / 1000;
    let summaries: HashMap<u32, SessionSummary> = m.list().unwrap_or_default().into_iter().map(|s| (s.id, s)).collect();
    let mut needs: Vec<SessionLine> = all.iter().filter(|l| l.attention.needs_you() && l.attention != Attention::Failed).cloned().collect();
    needs.sort_by_key(|l| std::cmp::Reverse(l.waiting_secs));
    let finished = all
        .iter()
        .filter(|l| matches!(l.attention, Attention::Ready | Attention::Done) && summaries.get(&l.id).is_some_and(|s| i64::from(s.latest_at_secs) >= since))
        .map(|l| l.name.clone())
        .collect();
    let failed = all.iter().filter(|l| l.attention == Attention::Failed).map(|l| l.name.clone()).collect();
    view::digest(&DigestFacts { needs, finished, failed })
}

/// Sends the digest at its daily time, while Chorus runs; a time missed while it was closed isn't
/// caught up.
fn digest_loop(m: &Arc<SessionManager>) {
    let mut planned: Option<(String, i64)> = None;
    while !m.is_quitting() {
        std::thread::sleep(Duration::from_secs(20));
        let at = db::config(m.store()).ok().and_then(|c| c.digest_at);
        let Some((hour, minute)) = at.as_deref().and_then(view::parse_time) else {
            planned = None;
            continue;
        };
        let now = now_ms();
        let next = match &planned {
            Some((was, next)) if Some(was.as_str()) == at.as_deref() => *next,
            _ => sched::next_run(&Schedule::Daily { hour, minute }, now, now, &|t| crate::automations::offset_at(m.store(), t)),
        };
        if now >= next {
            if listening(m) {
                let _ = outbound::enqueue(m, "digest", About::default(), &digest(m));
            }
            planned = Some((at.unwrap_or_default(), sched::next_run(&Schedule::Daily { hour, minute }, now, now, &|t| crate::automations::offset_at(m.store(), t))));
        } else {
            planned = Some((at.unwrap_or_default(), next));
        }
    }
}
