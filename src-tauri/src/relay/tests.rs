//! The relay's inbound and outbound logic without the network: pairing, who is heard, a question
//! queued with its buttons and scrubbed, and a reply that answers it. Nothing here calls Telegram;
//! the sender thread isn't running.

use std::sync::Arc;

use chorus_core::relay::render::Act;
use chorus_core::relay::{Message, Pairing};
use chorus_core::run_loop::{self as engine, Limits};

use super::{db, inbound, outbound, state};
use crate::run_loop::db as loop_db;
use crate::sessions::SessionManager;
use crate::store::{Store, now_ms};

fn msg(from: i64, chat: i64, text: &str) -> Message {
    Message {
        message_id: 900,
        chat_id: chat,
        private: chat == from,
        from_id: from,
        from_bot: false,
        from_name: "Phone".into(),
        date: now_ms() / 1000,
        text: text.into(),
        reply_to: None,
        forwarded: false,
    }
}

fn outbox(m: &SessionManager) -> Vec<db::Out> {
    db::pending(m.store(), 100).unwrap()
}

#[test]
fn pairing_questions_and_replies_without_the_network() {
    let dir = std::env::temp_dir().join(format!("chorus-relay-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let m: Arc<SessionManager> = SessionManager::new(Store::open(&dir.join("chorus.db")).unwrap(), dir.clone(), Box::new(|_| {}));
    // A token in memory only; nothing here sends it anywhere.
    state().token = Some(format!("{}:{}", "123456789", "A".repeat(35)));
    state().pairing = Some(Pairing { code: "123456".into(), expires_ms: now_ms() + 60_000, wrong: 0 });

    // Strangers and groups are dropped, without a reply.
    inbound::message(&m, msg(7, 7, "hello"));
    inbound::message(&m, msg(8, -100, "/pair 123456"));
    assert!(db::users(m.store()).unwrap().is_empty());
    assert!(outbox(&m).is_empty());

    // A wrong code, then the right one.
    inbound::message(&m, msg(7, 7, "/pair 000000"));
    assert!(outbox(&m).last().unwrap().html.contains("4 tries left"));
    inbound::message(&m, msg(7, 7, "/pair 123456"));
    assert_eq!(db::users(m.store()).unwrap().iter().map(|u| u.telegram_id).collect::<Vec<_>>(), [7]);
    assert!(outbox(&m).last().unwrap().html.starts_with("Paired."));
    inbound::message(&m, msg(7, 7, "/pair 123456"));
    assert_eq!(outbox(&m).last().unwrap().html, "This phone is already paired.");

    // A loop question goes out with its options as buttons, and a key in it is scrubbed.
    let config = loop_db::RunConfig { roles: Vec::new(), checks: Vec::new(), limits: Limits::default(), milestone_every: 3, script: None, phone_status_only: false };
    let run = loop_db::insert_run(m.store(), "F1", "C:\\x", "main", "run/f1", &config).unwrap();
    let phase = loop_db::insert_phase(m.store(), run, 1, "Sum", "01-sum", &engine::start(0).phase).unwrap();
    let step = loop_db::insert_step(
        m.store(),
        &loop_db::NewStep { run_id: run, phase_id: phase, kind: loop_db::StepKind::Build, task: Some("T1"), attempt: 0, role: None, bot_id: None, model_asked: None },
    )
    .unwrap();
    let key = format!("sk-or-v1-{}", "d4".repeat(32));
    let options = vec!["offset".to_owned(), "cursor".to_owned()];
    let q = loop_db::insert_question(
        m.store(),
        &loop_db::NewQuestion {
            run_id: run,
            step_id: step,
            phase: 1,
            task: Some("T1"),
            class: "timed",
            question: &format!("Offset or cursor? (not {key})"),
            options: &options,
            default_answer: Some("cursor"),
            deadline_at: Some(now_ms() + 60_000),
        },
    )
    .unwrap();
    outbound::question_asked(&m, q);
    let card = outbox(&m).into_iter().find(|o| o.kind == "question").unwrap();
    assert!(card.html.contains("<b>[P01 T1]</b> F1") && card.html.contains("B) cursor (default)"), "{}", card.html);
    assert!(!card.html.contains(&key), "scrubbed");
    assert_eq!(card.buttons.len(), 3);
    let b = db::take_button(m.store(), &card.buttons[1].1).unwrap().unwrap();
    assert_eq!((b.act, b.question_id), (Act::Answer { text: "cursor".into(), rule: false }, Some(q)));
    assert!(db::take_button(m.store(), &card.buttons[1].1).unwrap().is_none(), "a button works once");

    // A plain message isn't taken as the answer: the hint names the open question.
    inbound::message(&m, msg(7, 7, "offset"));
    assert!(outbox(&m).last().unwrap().html.starts_with("To answer [P01 T1] in F1, reply to its message"), "{}", outbox(&m).last().unwrap().html);

    // Once sent, a reply to it answers the question, and the phone's copy is queued to change.
    db::record_message(m.store(), 7, 500, "question", db::About { run_id: Some(run), question_id: Some(q), ..db::About::default() }).unwrap();
    inbound::message(&m, Message { reply_to: Some(500), ..msg(7, 7, "offset, please") });
    let (_, answered) = loop_db::question(m.store(), q).unwrap().unwrap();
    assert_eq!((answered.state.as_str(), answered.answer.as_deref()), ("answered", Some("offset, please")));
    assert!(outbox(&m).iter().any(|o| o.kind == "reply" && o.html.contains("Answered [P01 T1]")));
    assert!(outbox(&m).iter().any(|o| o.kind == "settled" && o.html.contains("<b>Answered:</b> offset, please")));
    // A second answer is refused in words.
    inbound::message(&m, Message { reply_to: Some(500), ..msg(7, 7, "cursor") });
    assert!(outbox(&m).last().unwrap().html.contains("already answered"));

    // A forwarded message from the paired phone is never acted on.
    let before = outbox(&m).len();
    inbound::message(&m, Message { forwarded: true, ..msg(7, 7, "/stop") });
    assert_eq!(outbox(&m).len(), before);

    // A milestone's Approve button goes once it is settled, and the message says how, once.
    outbound::milestone(&m, run, "F1", 1, "Sum", "phase-01", true);
    let ms = outbox(&m).into_iter().find(|o| o.kind == "milestone").unwrap();
    assert_eq!(ms.buttons.len(), 1);
    db::finish_out(m.store(), ms.id, None).unwrap();
    db::record_message(m.store(), 7, 501, "milestone", db::About::run(run)).unwrap();
    outbound::milestone_settled(&m, run, "Approved.");
    let edit = outbox(&m).into_iter().find(|o| o.kind == "edit").unwrap();
    assert_eq!((edit.chat_id, edit.reply_to, edit.buttons.len()), (Some(7), Some(501), 0));
    assert!(edit.html.starts_with("<b>MILESTONE</b>") && edit.html.ends_with("<b>Approved.</b>"), "{}", edit.html);
    outbound::milestone_settled(&m, run, "Stopped.");
    assert_eq!(outbox(&m).iter().filter(|o| o.kind == "edit").count(), 1, "settled once");
    // One settled before it was sent goes without its button.
    outbound::milestone(&m, run, "F1", 3, "Help", "phase-03", false);
    outbound::milestone_settled(&m, run, "Approved.");
    let last = outbox(&m).into_iter().rfind(|o| o.kind == "milestone-settled").unwrap();
    assert!(last.buttons.is_empty() && last.html.ends_with("<b>Approved.</b>"), "{}", last.html);

    // Sessions (D101), routed without a real one: commands answer, a plain message with no /use
    // session says how to pick one, and a reply about a session that's gone says so.
    let last_reply = |m: &SessionManager| outbox(m).into_iter().rfind(|o| o.kind == "reply").unwrap().html;
    inbound::message(&m, msg(7, 7, "/sessions"));
    assert_eq!(last_reply(&m), "No sessions are open.");
    inbound::message(&m, msg(7, 7, "/use 1"));
    assert!(last_reply(&m).starts_with("There is no session 1"), "{}", last_reply(&m));
    inbound::message(&m, msg(7, 7, "/use"));
    assert!(last_reply(&m).starts_with("Plain messages go nowhere yet"));
    inbound::message(&m, msg(7, 7, "/digest"));
    assert!(last_reply(&m).contains("Nothing needs you"));
    inbound::message(&m, msg(7, 7, "run the tests"));
    assert!(last_reply(&m).starts_with("To send a session a prompt"), "{}", last_reply(&m));
    db::record_message(m.store(), 7, 502, "session-done", db::About::session(99)).unwrap();
    inbound::message(&m, Message { reply_to: Some(502), ..msg(7, 7, "now the docs") });
    assert_eq!(last_reply(&m), "That session no longer exists.");
    // A stale prompt isn't sent.
    inbound::message(&m, Message { reply_to: Some(502), date: now_ms() / 1000 - 600, ..msg(7, 7, "late") });
    assert!(last_reply(&m).starts_with("That message waited too long"));
    // A session's button for an ask that isn't held says it's too late, in words.
    assert!(super::sessions::press(&m, &Act::Allow { session: 99, run: 1, ask: 1 }).starts_with("Too late for the phone"));

    state().token = None;
    let _ = std::fs::remove_dir_all(&dir);
}
