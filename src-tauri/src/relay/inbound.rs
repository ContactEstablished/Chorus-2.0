//! The poller: long-polls `getUpdates`, keeps the offset, and acts on what a paired phone sends:
//! `/pair` from anyone in a private chat, then, from paired users only, answers (a button, or a
//! reply to a question) and commands. Everything else is dropped and audited, never acted on.

use std::sync::Arc;
use std::time::Duration;

use chorus_core::relay::render::{self, Act};
use chorus_core::relay::{self, Admit, Callback, Command, Incoming, Message, PairOutcome};

use super::client::{self, Failure};
use super::db::About;
use super::{Conn, active_token, db, outbound, sessions, set_conn, state};
use crate::sessions::SessionManager;
use crate::store::now_ms;

/// A command or prompt older than this isn't acted on: it may have waited while Chorus was closed.
const BACKLOG_SECS: i64 = 120;
const STALE_PROMPT: &str = "That message waited too long to arrive, so it wasn't sent. Send it again.";
const NO_TARGET: &str = "To send a session a prompt, reply to one of its messages, or pick it with /use (see /sessions).";

/// `/status`: the loop's runs, and how many sessions need you.
fn status(m: &SessionManager) -> String {
    let mut html = render::status(&crate::run_loop::status_lines(m)).html;
    let waiting = sessions::lines(m).iter().filter(|l| l.attention.needs_you()).count();
    if waiting > 0 {
        let s = if waiting == 1 { "session needs" } else { "sessions need" };
        html.push_str(&format!("{}{waiting} {s} you: /sessions lists them.", if html.is_empty() { "" } else { "\n\n" }));
    }
    html
}

pub fn poll(m: &Arc<SessionManager>) {
    let mut backoff = 1u64;
    // The bot's command menu, set once per start so a newer Chorus's commands show on the phone.
    let mut menu_set = false;
    while !m.is_quitting() {
        let Some(token) = active_token(m) else {
            if state().token.is_none() {
                set_conn(Conn::NoToken);
            }
            std::thread::sleep(Duration::from_secs(2));
            continue;
        };
        let offset = db::config(m.store()).map(|c| c.poll_offset).unwrap_or(0);
        match client::get_updates(&token, offset) {
            Ok(updates) => {
                set_conn(Conn::Connected);
                backoff = 1;
                if !menu_set {
                    menu_set = client::set_commands(&token).is_ok();
                }
                for (id, incoming) in updates {
                    handle(m, &token, incoming);
                    let _ = db::set_offset(m.store(), id + 1);
                }
            }
            Err(Failure::Api(e)) if e.code == 409 => {
                set_conn(Conn::Conflict);
                std::thread::sleep(Duration::from_secs(15));
            }
            Err(Failure::Api(e)) if e.code == 401 || e.code == 404 => {
                set_conn(Conn::Rejected(e.description));
                std::thread::sleep(Duration::from_secs(30));
            }
            Err(Failure::Api(e)) if e.retry_after.is_some() => {
                std::thread::sleep(Duration::from_secs(e.retry_after.unwrap_or(5).min(120)));
            }
            Err(f) => {
                set_conn(Conn::Offline(f.text()));
                std::thread::sleep(Duration::from_secs(backoff));
                backoff = (backoff * 2).min(60);
            }
        }
    }
}

fn paired(m: &SessionManager) -> Vec<i64> {
    db::users(m.store()).unwrap_or_default().into_iter().map(|u| u.telegram_id).collect()
}

fn handle(m: &Arc<SessionManager>, token: &str, incoming: Incoming) {
    match incoming {
        Incoming::Message(msg) => message(m, msg),
        Incoming::Callback(cb) => callback(m, token, &cb),
        Incoming::Ignored => {}
    }
}

pub(super) fn message(m: &Arc<SessionManager>, msg: Message) {
    let store = m.store();
    let command = relay::parse_command(&msg.text);
    // Pairing is the one thing an unpaired user may do, and only in a private chat.
    if let Some(Command::Pair(code)) = &command
        && msg.private
        && !msg.from_bot
        && msg.chat_id == msg.from_id
        && !msg.forwarded
    {
        if paired(m).contains(&msg.from_id) {
            outbound::reply(m, msg.chat_id, Some(msg.message_id), "This phone is already paired.");
            return;
        }
        let outcome = relay::try_pair(&mut state().pairing, code, now_ms());
        db::audit(store, "pair", Some(msg.from_id), &format!("{outcome:?}"));
        let text = match outcome {
            PairOutcome::Paired => {
                let name = if msg.from_name.trim().is_empty() { "phone".to_owned() } else { msg.from_name.clone() };
                let _ = db::add_user(store, msg.from_id, &name);
                "Paired. This chat now gets Chorus's loop questions, stops and milestones. Send /help to see what you can send.".to_owned()
            }
            PairOutcome::Wrong { left } => format!("That code isn't right. {left} tries left."),
            PairOutcome::Expired => "That code has expired. Make a new one in Chorus, Settings → Phone.".to_owned(),
            PairOutcome::LockedOut => "Too many wrong codes, so that one is gone. Make a new one in Chorus, Settings → Phone.".to_owned(),
            PairOutcome::NoPairing => "No pairing code is waiting. Make one in Chorus, Settings → Phone.".to_owned(),
        };
        outbound::reply(m, msg.chat_id, Some(msg.message_id), &render::escape(&text));
        return;
    }
    match relay::admit(&msg, &paired(m)) {
        Admit::Accept => {}
        other => {
            db::audit(store, &format!("dropped-{other:?}").to_ascii_lowercase(), Some(msg.from_id), "");
            return;
        }
    }
    let stale = now_ms() / 1000 - msg.date > BACKLOG_SECS;
    if let Some(to) = msg.reply_to {
        let text = relay::sanitize(&msg.text);
        // A reply answers the loop question it replies to, or goes to the session it replies about.
        let reply = match db::message_about(store, msg.chat_id, to).ok().flatten() {
            _ if text.is_empty() => "Write it as text.".to_owned(),
            Some((_, About { question_id: Some(q), .. })) => match crate::run_loop::answer_from_phone(m, q, &text, false) {
                Ok(tag) => format!("Answered {tag}: {}", render::escape(&render::clip(&text, 200))),
                Err(e) => render::escape(&e),
            },
            Some((_, About { session_id: Some(_), .. })) if stale => STALE_PROMPT.to_owned(),
            Some((kind, About { session_id: Some(session), ask_id, .. })) => sessions::reply(m, session, &kind, ask_id, &text),
            _ => "That message isn't one I can answer. Reply to a question, or to a session's message.".to_owned(),
        };
        db::audit(store, "reply", Some(msg.from_id), "");
        outbound::reply(m, msg.chat_id, Some(msg.message_id), &reply);
        return;
    }
    let reply = match command {
        Some(_) if stale => "That command waited too long to arrive, so it wasn't acted on. Send it again.".to_owned(),
        Some(Command::Help | Command::Start) => render::escape(render::HELP),
        Some(Command::Status) => status(m),
        Some(Command::Sessions) => sessions::list_command(m),
        Some(Command::Use(arg)) => sessions::use_command(m, &arg),
        Some(Command::Digest) => sessions::digest(m).html,
        Some(Command::Unknown(c)) => format!("There is no /{} command. Send /help.", render::escape(&c)),
        Some(c) => render::escape(&crate::run_loop::phone_command(m, &c)),
        None if stale => STALE_PROMPT.to_owned(),
        // A plain message is a prompt to the `/use` session, if there is one.
        None => match sessions::target(m) {
            Some(session) => sessions::prompt(m, session, &relay::sanitize(&msg.text)),
            None => {
                let open = crate::run_loop::open_question_tags(m);
                if open.is_empty() { NO_TARGET.to_owned() } else { render::answer_hint(&open) }
            }
        },
    };
    db::audit(store, "command", Some(msg.from_id), "");
    outbound::reply(m, msg.chat_id, Some(msg.message_id), &reply);
}

fn callback(m: &Arc<SessionManager>, token: &str, cb: &Callback) {
    let store = m.store();
    if !relay::admit_callback(cb, &paired(m)) {
        db::audit(store, "dropped-callback", Some(cb.from_id), "");
        let _ = client::answer_callback(token, &cb.id, "This bot answers only its paired phone.");
        return;
    }
    let Some(found) = relay::callback_token(&cb.data).and_then(|t| db::take_button(store, t).ok().flatten()) else {
        let _ = client::answer_callback(token, &cb.id, "That button has expired or was already used.");
        return;
    };
    let note = match (found.act, found.run_id, found.question_id) {
        (Act::Answer { text, rule }, _, Some(q)) => match crate::run_loop::answer_from_phone(m, q, &text, rule) {
            Ok(_) => outbound::act_label(&Act::Answer { text, rule }).to_owned(),
            Err(e) => e,
        },
        (Act::Approve, Some(run), _) => crate::run_loop::phone_command_for(m, run, &Command::Approve),
        (act @ (Act::Allow { .. } | Act::Deny { .. } | Act::Choose { .. }), _, _) => sessions::press(m, &act),
        _ => "That button no longer does anything.".to_owned(),
    };
    db::audit(store, "button", Some(cb.from_id), "");
    let _ = client::answer_callback(token, &cb.id, &render::clip(&note, 190));
}
