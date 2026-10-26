//! What the relay sends, and the thread that sends it. The loop queues a message for each of its
//! events (a question, its answer, a stop, a milestone, a finished phase), and sessions for theirs
//! (`sessions.rs`); the sender delivers them to every paired phone, about one a second per chat,
//! honouring Telegram's `retry_after`. Every text is scrubbed with every stored key after rendering
//! (P13 §7). Nothing is queued while no phone is paired, so a backlog never floods a phone paired
//! later.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chorus_core::relay::Bucket;
use chorus_core::relay::render::{self, Act, Outgoing, QuestionCard};
use chorus_core::scrub::{builtin_patterns, scrub_all};

use super::client::{self, Failure};
use super::db::{About, Subject};
use super::{active_token, db};
use crate::sessions::SessionManager;
use crate::store::now_ms;
use crate::vault;

/// A message nobody could be sent within a day is dropped.
const MAX_AGE_MS: i64 = 24 * 60 * 60 * 1000;
const MAX_ATTEMPTS: u32 = 30;

pub(super) fn scrub(m: &SessionManager, html: &str) -> String {
    let (keys, _) = vault::all_for_scrubbing(m.store());
    String::from_utf8_lossy(&scrub_all(html.as_bytes(), &keys, &builtin_patterns())).into_owned()
}

/// Whether anything should be queued: the relay is on, has a token, and a phone is paired.
pub(super) fn listening(m: &SessionManager) -> bool {
    active_token(m).is_some() && db::users(m.store()).is_ok_and(|u| !u.is_empty())
}

/// Queues a rendered message to every paired phone, giving each button a token.
pub(super) fn enqueue(m: &SessionManager, kind: &str, about: About, out: &Outgoing) -> Result<(), String> {
    let mut buttons = Vec::new();
    for (label, act) in &out.buttons {
        let token = chorus_core::relay::token_from(&vault::random_bytes(12));
        db::add_button(m.store(), &token, act, about.run_id, about.question_id)?;
        buttons.push((label.clone(), token));
    }
    db::enqueue(m.store(), &db::NewOut { kind, about, chat_id: None, reply_to: None, html: &scrub(m, &out.html), silent: out.silent, buttons: &buttons })?;
    Ok(())
}

/// Queues plain text (already in words, no HTML of its own) to every paired phone.
pub fn enqueue_text(m: &SessionManager, kind: &str, about: About, text: &str, silent: bool) -> Result<(), String> {
    let out = Outgoing { html: render::escape(text), silent, buttons: Vec::new() };
    enqueue(m, kind, about, &out)
}

/// A reply to one chat, to a message of theirs.
pub fn reply(m: &SessionManager, chat: i64, to: Option<i64>, html: &str) {
    let _ = db::enqueue(
        m.store(),
        &db::NewOut { kind: "reply", about: About::default(), chat_id: Some(chat), reply_to: to, html: &scrub(m, html), silent: true, buttons: &[] },
    );
}

/// The QUESTION card for a loop question, as it stands now.
fn question_card(m: &SessionManager, question_id: u32) -> Result<Option<(u32, Outgoing)>, String> {
    let Some((run_id, q)) = crate::run_loop::db::question(m.store(), question_id)? else { return Ok(None) };
    let Some(run) = crate::run_loop::db::run(m.store(), run_id)? else { return Ok(None) };
    let answered = match (q.state.as_str(), q.answer.as_deref()) {
        ("answered", Some(a)) => Some((a, q.made_rule, false)),
        ("defaulted", Some(a)) => Some((a, false, true)),
        _ => None,
    };
    let card = QuestionCard {
        run: &run.name,
        phase: q.phase,
        task: q.task.as_deref(),
        class: &q.class,
        question: &q.question,
        options: &q.options,
        default: q.default_answer.as_deref(),
        timed_minutes: run.config.limits.timed_minutes,
        status_only: run.config.phone_status_only,
        answered,
    };
    let mut out = render::question(&card);
    if q.state == "cancelled" {
        out.html.push_str("\n\nClosed without an answer.");
        out.buttons.clear();
    }
    Ok(Some((run_id, out)))
}

fn question_about(run_id: u32, question_id: u32) -> About {
    About { run_id: Some(run_id), question_id: Some(question_id), ..About::default() }
}

/// A step asked a blocking or timed question.
pub fn question_asked(m: &SessionManager, question_id: u32) {
    if !listening(m) {
        return;
    }
    if let Ok(Some((run_id, out))) = question_card(m, question_id)
        && let Err(e) = enqueue(m, "question", question_about(run_id, question_id), &out)
    {
        eprintln!("chorus: relay: queueing question {question_id}: {e}");
    }
}

/// A question was answered, defaulted or closed: the phone's copy shows it, without buttons.
pub fn question_settled(m: &SessionManager, question_id: u32) {
    if !listening(m) || db::messages_of_question(m.store(), question_id).map_or(true, |v| v.is_empty()) {
        return;
    }
    if let Ok(Some((run_id, out))) = question_card(m, question_id) {
        let _ = db::enqueue(
            m.store(),
            &db::NewOut {
                kind: "settled",
                about: question_about(run_id, question_id),
                chat_id: None,
                reply_to: None,
                html: &scrub(m, &out.html),
                silent: true,
                buttons: &[],
            },
        );
    }
}

pub fn run_stopped(m: &SessionManager, run_id: u32, run: &str, phase: u32, title: &str, why: &str, last_good: &str) {
    if listening(m) {
        let _ = enqueue(m, "stopped", About::run(run_id), &render::stopped(run, phase, title, why, last_good));
    }
}

pub fn milestone(m: &SessionManager, run_id: u32, run: &str, phase: u32, title: &str, tag: &str, more: bool) {
    if listening(m) {
        let _ = enqueue(m, "milestone", About::run(run_id), &render::milestone(run, phase, title, tag, more));
    }
}

/// Settles the newest message of a kind for a run or session, however that happened: it says how
/// and loses its buttons (edited if sent, rewritten if still queued), once.
pub(super) fn settle(m: &SessionManager, subject: Subject, kind: &str, outcome: &str) {
    if !listening(m) {
        return;
    }
    let store = m.store();
    let Ok(Some((id, html, queued_at, sent))) = db::last_out(store, subject, kind) else { return };
    let html = scrub(m, &render::with_outcome(&html, outcome));
    if !sent {
        let _ = db::rewrite_unsent(store, id, &html);
        return;
    }
    let about = match subject {
        Subject::Run(run) => About::run(run),
        Subject::Session(session) => About::session(session),
    };
    for (chat, message) in db::sent_messages(store, subject, kind, queued_at).unwrap_or_default() {
        let _ = db::enqueue(store, &db::NewOut { kind: "edit", about, chat_id: Some(chat), reply_to: Some(message), html: &html, silent: true, buttons: &[] });
    }
    let _ = db::settle_messages(store, subject, kind);
}

/// Takes the Approve button off the milestone a run waited on once it is approved or the run is
/// stopped, from the phone or the desk, and says which.
pub fn milestone_settled(m: &SessionManager, run_id: u32, outcome: &str) {
    settle(m, Subject::Run(run_id), "milestone", outcome);
}

pub fn phase_done(m: &SessionManager, run_id: u32, run: &str, phase: u32, title: &str, tag: &str) {
    if listening(m) {
        let _ = enqueue(m, "phase-done", About::run(run_id), &render::phase_done(run, phase, title, tag));
    }
}

pub fn run_done(m: &SessionManager, run_id: u32, run: &str, tag: &str) {
    if listening(m) {
        let _ = enqueue(m, "done", About::run(run_id), &render::run_done(run, tag));
    }
}

/// What a button press did, for the press's short note on the phone.
pub fn act_label(act: &Act) -> &'static str {
    match act {
        Act::Answer { .. } | Act::Choose { .. } => "Answered",
        Act::Approve => "Approved",
        Act::Allow { .. } => "Allowed once",
        Act::Deny { .. } => "Denied",
    }
}

/// The sender: every second, the oldest waiting messages go out.
pub fn send_loop(m: &Arc<SessionManager>) {
    let mut buckets: HashMap<i64, Bucket> = HashMap::new();
    while !m.is_quitting() {
        std::thread::sleep(Duration::from_secs(1));
        let Some(token) = active_token(m) else { continue };
        let Ok(users) = db::users(m.store()) else { continue };
        for out in db::pending(m.store(), 20).unwrap_or_default() {
            if now_ms() - out.created_at > MAX_AGE_MS || out.attempts >= MAX_ATTEMPTS {
                let _ = db::finish_out(m.store(), out.id, Some("given up: not sent within a day or 30 tries"));
                continue;
            }
            match send_one(m, &token, &out, &users, &mut buckets) {
                Ok(()) => {
                    let _ = db::finish_out(m.store(), out.id, None);
                }
                // Telegram refused this message for good: it is dropped with the reason.
                Err(Failure::Api(e)) if e.retry_after.is_none() && e.code != 0 && e.code != 409 => {
                    let _ = db::finish_out(m.store(), out.id, Some(&format!("Telegram said {}: {}", e.code, e.description)));
                }
                Err(f) => {
                    let wait = match &f {
                        Failure::Api(e) => e.retry_after.unwrap_or(5),
                        Failure::Network(_) => 5,
                    };
                    let _ = db::retry_out(m.store(), out.id, &f.text());
                    std::thread::sleep(Duration::from_secs(wait.min(60)));
                    break;
                }
            }
        }
    }
}

fn send_one(m: &SessionManager, token: &str, out: &db::Out, users: &[db::User], buckets: &mut HashMap<i64, Bucket>) -> Result<(), Failure> {
    let chats: Vec<i64> = match out.chat_id {
        Some(c) => vec![c],
        None => users.iter().map(|u| u.telegram_id).collect(),
    };
    if out.kind == "settled" {
        let qid = out.about.question_id.unwrap_or(0);
        for (chat, message) in db::messages_of_question(m.store(), qid).unwrap_or_default() {
            pace(buckets, chat);
            client::edit(token, chat, message, &out.html)?;
        }
        return Ok(());
    }
    if out.kind == "edit" {
        if let (Some(chat), Some(message)) = (out.chat_id, out.reply_to) {
            pace(buckets, chat);
            client::edit(token, chat, message, &out.html)?;
        }
        return Ok(());
    }
    for chat in chats {
        pace(buckets, chat);
        let id = client::send(token, chat, &out.html, out.silent, &out.buttons, out.reply_to)?;
        let _ = db::record_message(m.store(), chat, id, &out.kind, out.about);
    }
    Ok(())
}

fn pace(buckets: &mut HashMap<i64, Bucket>, chat: i64) {
    let bucket = buckets.entry(chat).or_insert_with(|| Bucket::new(now_ms()));
    while let Err(wait) = bucket.take(now_ms()) {
        std::thread::sleep(Duration::from_millis(wait.min(2000)));
    }
}
