//! The session engine against real `cmd.exe` processes: output reaches a view and the mirror
//! scrubbed, a planted key appears nowhere, exits and Restart are recorded, and a quit followed by
//! a restore relaunches what was running.

use super::*;
use chorus_core::scrub::{PLACEHOLDER, longest_shared_run};
use chorus_core::session::RunEnd;

struct Fixture {
    dir: PathBuf,
    manager: Arc<SessionManager>,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("chorus-sessions-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
        Self { manager: Self::manager(&dir), dir }
    }

    fn manager(dir: &Path) -> Arc<SessionManager> {
        let store = Store::open(&dir.join("chorus.db")).unwrap();
        SessionManager::new(store, dir.to_path_buf(), Box::new(|_| {}))
    }

    fn shell(&self, credential: Option<CredentialUse>) -> u32 {
        let req = CreateSession {
            agent: AgentKind::Shell,
            cwd: self.dir.join("work").display().to_string(),
            name: None,
            credential,
            cols: 100,
            rows: 30,
            options: Default::default(),
            provider_id: None,
            description: None,
            project_id: None,
            tags: Vec::new(),
            workspace: Default::default(),
            routing: None,
            bot_id: None,
            first_prompt: None,
            loop_step: None,
        };
        let s = self.manager.create(req).unwrap();
        assert!(s.live, "shell should be running: {:?}", s.note);
        s.id
    }

    fn view(&self, id: u32) -> Arc<Mutex<Vec<u8>>> {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        self.manager
            .attach(
                id,
                Box::new(move |b: &[u8]| {
                    sink.lock().unwrap().extend_from_slice(b);
                    true
                }),
            )
            .unwrap();
        seen
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        for s in self.manager.list().unwrap_or_default() {
            if let Some(live) = self.manager.live.lock().unwrap().get(&s.id).cloned() {
                self.manager.end_run_for(&live, ExitCause::UserClose);
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn wait_for(what: &str, mut ok: impl FnMut() -> bool) {
    wait_for_or(what, &mut ok, String::new);
}

/// A real run with an injected writer: exercises persistence/order without sending test text
/// to a model or depending on a CLI login. Events here are synthetic; this is not P6 acceptance.
struct DeliveryWriter {
    manager: std::sync::Weak<SessionManager>,
    chunks: DeliveryChunks,
    fail_enter: bool,
    // Dropping ConPTY's real input handle sends EOF to cmd.exe. Keep it alive while intercepting.
    _original: Box<dyn Write + Send>,
}

type DeliveryChunks = Arc<Mutex<Vec<Vec<u8>>>>;

impl Write for DeliveryWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let manager = self.manager.upgrade().unwrap();
        let count: u32 = manager.store.with(|c| c.query_row("SELECT count(*) FROM prompt_deliveries", [], |r| r.get(0))).unwrap();
        assert!(count > 0, "the durable receipt must precede every write");
        if self.fail_enter && data == b"\r" { return Err(std::io::Error::other("injected write failure")); }
        self.chunks.lock().unwrap().push(data.to_vec());
        Ok(data.len())
    }
    fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
}

fn delivery_fixture(f: &Fixture, fail_enter: bool) -> (u32, Arc<Run>, DeliveryChunks) {
    let id = f.shell(None);
    let seen = f.view(id);
    // Finish ConPTY's opening cursor handshake before replacing its writer.
    f.manager.write(id, "echo delivery-fixture-ready\r").unwrap();
    wait_text(&seen, "delivery-fixture-ready", 2);
    let run = f.manager.running(id).unwrap();
    f.manager.store.with(|c| c.execute("UPDATE sessions SET agent='claude' WHERE id=?1", [id])).unwrap();
    let chunks = Arc::new(Mutex::new(Vec::new()));
    {
        let mut writer = run.writer.lock().unwrap();
        let original = std::mem::replace(&mut *writer, Box::new(std::io::sink()));
        *writer = Box::new(DeliveryWriter { manager: Arc::downgrade(&f.manager), chunks: chunks.clone(), fail_enter, _original: original });
    }
    run.recorder.lock().unwrap().record(&f.manager.store, "test", &serde_json::json!({}), &[chorus_core::events::AgentEvent::Idle], crate::store::now_ms());
    // The controlled shell command has completed; this fixture has no pending Raw draft.
    run.recorder.lock().unwrap().raw_dirty = false;
    (id, run, chunks)
}

const DELIVERY_REQUEST: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";
const OTHER_REQUEST: &str = "7c9e6679-7425-40de-944b-e07fc1f90ae7";

#[test]
fn composer_images_survive_queueing_and_recreate_files_before_delivery() {
    use crate::store::{composer as db, images};
    let f = Fixture::new("composer-images");
    let (id, run, chunks) = delivery_fixture(&f, false);
    let png = include_bytes!("../../icons/32x32.png");
    images::add(&f.manager.store, id, 0, "reference.png", png).unwrap();
    let request = compose(&f, id, &run, "", DELIVERY_REQUEST, true);
    composer::submit(&f.manager, id, request.clone()).unwrap();
    let item = db::oldest(&f.manager.store, id).unwrap().unwrap();
    assert_eq!(item.images.len(), 1);
    let path = f.dir.join("images").join(id.to_string()).join(format!("{}.png", item.images[0].id));
    assert_eq!(std::fs::read(&path).unwrap(), png);
    std::fs::remove_file(&path).unwrap();
    assert!(db::draft(&f.manager.store, id).unwrap().images.is_empty());
    composer::tick(&f.manager, id).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), png);
    assert_eq!(chunks.lock().unwrap().len(), 2);
    assert!(item.text.contains("Image 1:"));
    composer::submit(&f.manager, id, request).unwrap();
    composer::tick(&f.manager, id).unwrap();
    assert_eq!(chunks.lock().unwrap().len(), 2, "images must not cause a replay");
}

fn compose(f: &Fixture, id: u32, run: &Run, text: &str, request: &str, queue: bool) -> composer::ComposerSubmit {
    let draft = crate::store::composer::draft(&f.manager.store, id).unwrap();
    let saved = composer::save(&f.manager, id, draft.revision, text).unwrap();
    composer::ComposerSubmit { run_id: run.id, input_epoch: run.recorder.lock().unwrap().input_epoch,
        request_id: request.into(), draft_revision: saved.revision, queue, confirm_ready: false }
}

#[test]
fn composer_queues_fifo_waits_for_ack_and_cancel_never_writes() {
    use chorus_core::events::AgentEvent;
    use crate::store::composer as db;
    let f=Fixture::new("composer-fifo");
    let (id,run,chunks)=delivery_fixture(&f,false);
    let event=|event| run.recorder.lock().unwrap().record(&f.manager.store,"test",&serde_json::json!({}),&[event],crate::store::now_ms());
    event(AgentEvent::TurnStarted { turn:Some("busy".into()), prompt:Some("working".into()) });
    let a=compose(&f,id,&run,"one",DELIVERY_REQUEST,true);
    composer::submit(&f.manager,id,a.clone()).unwrap();
    composer::submit(&f.manager,id,a).unwrap();
    let b=compose(&f,id,&run,"two",OTHER_REQUEST,true);
    composer::submit(&f.manager,id,b).unwrap();
    composer::tick(&f.manager,id).unwrap();
    assert!(chunks.lock().unwrap().is_empty());
    event(AgentEvent::TurnEnded { turn:Some("busy".into()), message:Some("Done".into()) });
    composer::tick(&f.manager,id).unwrap();
    composer::tick(&f.manager,id).unwrap();
    assert_eq!(*chunks.lock().unwrap(),[b"one".to_vec(),b"\r".to_vec()]);
    let second=db::oldest(&f.manager.store,id).unwrap().unwrap();
    db::cancel(&f.manager.store,id,second.id).unwrap();
    event(AgentEvent::TurnStarted { turn:Some("one".into()), prompt:Some("one".into()) });
    event(AgentEvent::TurnEnded { turn:Some("one".into()), message:Some("Done".into()) });
    composer::tick(&f.manager,id).unwrap();
    assert_eq!(chunks.lock().unwrap().len(),2);
    assert_eq!(db::list(&f.manager.store,id).unwrap()[1].status,"observed");
}

#[test]
fn composer_partial_write_and_database_failure_cannot_replay() {
    use crate::store::composer as db;
    let f=Fixture::new("composer-uncertain");
    let (id,run,chunks)=delivery_fixture(&f,true);
    let request=compose(&f,id,&run,"one",DELIVERY_REQUEST,false);
    composer::submit(&f.manager,id,request.clone()).unwrap();
    composer::submit(&f.manager,id,request).unwrap();
    assert_eq!(chunks.lock().unwrap().len(),1);
    assert_eq!(db::list(&f.manager.store,id).unwrap()[0].status,"uncertain");
    db::recover(&f.manager.store).unwrap();
    composer::tick(&f.manager,id).unwrap();
    assert_eq!(chunks.lock().unwrap().len(),1);
    assert!(db::cancel(&f.manager.store,id,db::list(&f.manager.store,id).unwrap()[0].id).is_err());
    let request=compose(&f,id,&run,"two",OTHER_REQUEST,true);
    composer::submit(&f.manager,id,request).unwrap();
    // Simulate a fresh run's ledger with a storage failure before dispatch.
    f.manager.store.with(|c| c.execute_batch("UPDATE prompt_deliveries SET status='observed'; CREATE TRIGGER refuse_composer BEFORE INSERT ON prompt_deliveries BEGIN SELECT RAISE(ABORT,'injected'); END;")).unwrap();
    assert!(composer::tick(&f.manager,id).is_err());
    assert_eq!(chunks.lock().unwrap().len(),1);
    let prompts:u32=f.manager.store.with(|c|c.query_row("SELECT count(*) FROM prompts",[],|r|r.get(0))).unwrap();
    assert_eq!(prompts,1,"failed claim rolls back its prompt");
}

#[test]
fn composer_rechecks_lock_run_raw_input_and_scrubs_saved_drafts() {
    use crate::store::composer as db;
    let f=Fixture::new("composer-guards");
    let (id,run,chunks)=delivery_fixture(&f,false);
    let request=compose(&f,id,&run,"one",DELIVERY_REQUEST,false);
    f.manager.set_locked(id,true,None).unwrap();
    assert!(composer::submit(&f.manager,id,request.clone()).is_err());
    f.manager.set_locked(id,false,None).unwrap();
    let mut stale=request.clone(); stale.run_id+=1;
    assert!(composer::submit(&f.manager,id,stale).is_err());
    { let mut r=run.recorder.lock().unwrap(); r.raw_dirty=true; r.input_epoch+=1; }
    let mut confirmed=request.clone(); confirmed.confirm_ready=true;
    assert!(composer::submit(&f.manager,id,confirmed).is_err(),"old Raw review cannot authorise a send");
    assert!(chunks.lock().unwrap().is_empty());
    assert_eq!(db::draft(&f.manager.store,id).unwrap().text,"one");
    let secret="private-composer-test-value";
    *run.recorder.lock().unwrap()=Recorder::new(id,run.id,"hook",Arc::new(vec![secret.as_bytes().to_vec()]),Arc::new(builtin_patterns()));
    let d=composer::save(&f.manager,id,request.draft_revision,secret).unwrap();
    assert!(!d.text.contains(secret));
    assert_eq!(db::draft(&Store::open(&f.dir.join("chorus.db")).unwrap(),id).unwrap().text,d.text);
}

#[test]
fn composer_recovery_holds_unsent_instructions_and_rejects_changed_request_identity() {
    use crate::store::composer as db;
    let f=Fixture::new("composer-recovery");
    let (id,run,chunks)=delivery_fixture(&f,false);
    let request=compose(&f,id,&run,"one",DELIVERY_REQUEST,true);
    composer::submit(&f.manager,id,request.clone()).unwrap();
    let mut changed=request.clone(); changed.draft_revision+=1;
    assert!(composer::submit(&f.manager,id,changed).is_err());
    db::recover(&f.manager.store).unwrap();
    composer::tick(&f.manager,id).unwrap();
    assert!(chunks.lock().unwrap().is_empty());
    let item=db::oldest(&f.manager.store,id).unwrap().unwrap();
    assert_eq!(item.status,"held");
    db::resume(&f.manager.store,id,item.id,run.id).unwrap();
    composer::tick(&f.manager,id).unwrap();
    composer::submit(&f.manager,id,request).unwrap();
    assert_eq!(chunks.lock().unwrap().len(),2);
}

fn held_approval(f: &Fixture, run: &Run, call: &str) -> (u32, crate::events::replies::HookReceiver) {
    let payload = serde_json::json!({"hook_event_name":"PermissionRequest","tool_name":"Write","tool_use_id":call,"tool_input":{"file_path":"test.txt","content":"hello"}});
    let mut r = run.recorder.lock().unwrap();
    r.record(&f.manager.store, "test", &payload, &chorus_core::events::from_claude_hook(&payload), crate::store::now_ms());
    let receiver = r.hold_reply(&f.manager.store, &payload).unwrap();
    (r.newest_ask().unwrap() as u32, receiver)
}

#[test]
fn held_replies_persist_once_and_never_guess_agent_acknowledgement() {
    use chorus_core::actions::{SessionAction, ApprovalDecision};
    let f = Fixture::new("reply-once");
    let (id, run, chunks) = delivery_fixture(&f, false);
    let (ask, mut receiver) = held_approval(&f, &run, "a1");
    let request = actions::ActionRequest { run_id: run.id, request_id: DELIVERY_REQUEST.into(), action: SessionAction::Approval {
        ask_id: ask, decision: ApprovalDecision::Allow, message: None,
    }};
    actions::ActionRouter::dispatch(&f.manager, id, request.clone()).unwrap();
    let stored: (u32, Option<String>) = f.manager.store.with(|c| c.query_row(
        "SELECT (SELECT count(*) FROM reply_attempts), answer FROM asks WHERE id=?1", [ask], |r| Ok((r.get(0)?, r.get(1)?)),
    )).unwrap();
    assert_eq!(stored, (1, None), "attempt is durable but not a confirmed answer");
    assert_eq!(receiver.try_recv().unwrap()["hookSpecificOutput"]["decision"]["behavior"], "allow");
    actions::ActionRouter::dispatch(&f.manager, id, request.clone()).unwrap();
    let mut changed = request.clone();
    changed.action = SessionAction::Approval { ask_id: ask, decision: ApprovalDecision::Deny, message: None };
    assert!(actions::ActionRouter::dispatch(&f.manager, id, changed).is_err());
    let mut duplicate = request;
    duplicate.request_id = OTHER_REQUEST.into();
    assert!(actions::ActionRouter::dispatch(&f.manager, id, duplicate).is_err());
    assert!(chunks.lock().unwrap().is_empty());
    let reopened = Store::open(&f.dir.join("chorus.db")).unwrap();
    let count: u32 = reopened.with(|c| c.query_row("SELECT count(*) FROM reply_attempts", [], |r| r.get(0))).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn expired_raw_answered_locked_and_stale_requests_cannot_reply() {
    use chorus_core::actions::{SessionAction, ApprovalDecision};
    let f = Fixture::new("reply-lifetime");
    let (id, run, _) = delivery_fixture(&f, false);
    let (ask, mut receiver) = held_approval(&f, &run, "a1");
    let req = actions::ActionRequest { run_id: run.id, request_id: DELIVERY_REQUEST.into(), action: SessionAction::Approval { ask_id: ask, decision: ApprovalDecision::Allow, message: None }};
    f.manager.set_locked(id, true, None).unwrap();
    assert!(actions::ActionRouter::dispatch(&f.manager, id, req.clone()).is_err());
    f.manager.set_locked(id, false, None).unwrap();
    let mut stale = req.clone(); stale.run_id += 1;
    assert!(actions::ActionRouter::dispatch(&f.manager, id, stale).is_err());
    // A different parallel call of the same tool must not consume this request.
    {
        let mut r = run.recorder.lock().unwrap();
        let event = |call: &str| chorus_core::events::AgentEvent::ToolFinished { turn: None, call: Some(call.into()), tool: "Write".into(), target: None, failed: false };
        r.record(&f.manager.store, "test", &serde_json::json!({}), &[event("different")], 1);
        assert!(r.replies.contains_key(&i64::from(ask)));
        r.record(&f.manager.store, "test", &serde_json::json!({}), &[event("a1")], 2);
    }
    assert!(actions::ActionRouter::dispatch(&f.manager, id, req).is_err());
    assert_eq!(receiver.try_recv(), Err(tokio::sync::oneshot::error::TryRecvError::Closed));
    let (ask, receiver) = held_approval(&f, &run, "a2");
    let request = actions::ActionRequest { run_id: run.id, request_id: OTHER_REQUEST.into(), action: SessionAction::Release { ask_id: ask }};
    run.recorder.lock().unwrap().replies.get_mut(&i64::from(ask)).unwrap().opened -= Duration::from_secs(121);
    assert!(actions::ActionRouter::dispatch(&f.manager, id, request.clone()).is_err());
    assert!(run.recorder.lock().unwrap().pending_replies()[0].blocked_reason.is_some());
    drop(receiver);
    assert!(actions::ActionRouter::dispatch(&f.manager, id, request).is_err());
    let count: u32 = f.manager.store.with(|c| c.query_row("SELECT count(*) FROM reply_attempts", [], |r| r.get(0))).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn reply_database_failure_does_not_send_and_release_makes_no_decision() {
    use chorus_core::actions::SessionAction;
    let f = Fixture::new("reply-store-failure");
    let (id, run, _) = delivery_fixture(&f, false);
    let (ask, mut receiver) = held_approval(&f, &run, "a1");
    let req = actions::ActionRequest { run_id: run.id, request_id: DELIVERY_REQUEST.into(), action: SessionAction::Release { ask_id: ask }};
    f.manager.store.with(|c| c.execute_batch("CREATE TRIGGER refuse_reply BEFORE INSERT ON reply_attempts BEGIN SELECT RAISE(ABORT, 'injected'); END;")).unwrap();
    assert!(actions::ActionRouter::dispatch(&f.manager, id, req.clone()).is_err());
    assert_eq!(receiver.try_recv(), Err(tokio::sync::oneshot::error::TryRecvError::Empty));
    f.manager.store.with(|c| c.execute_batch("DROP TRIGGER refuse_reply;")).unwrap();
    actions::ActionRouter::dispatch(&f.manager, id, req).unwrap();
    assert_eq!(receiver.try_recv().unwrap(), serde_json::json!({}));
    let (_, mut receiver) = held_approval(&f, &run, "a2");
    f.manager.close(id, false).unwrap();
    assert_eq!(receiver.try_recv(), Err(tokio::sync::oneshot::error::TryRecvError::Closed));
}

#[test]
fn held_question_scrubs_decoded_values_but_returns_the_actual_answer() {
    use chorus_core::actions::SessionAction;
    let f = Fixture::new("reply-scrub");
    let (id, run, _) = delivery_fixture(&f, false);
    let secret = "private\"quoted\ncredential";
    let payload = serde_json::json!({"hook_event_name":"PreToolUse","tool_name":"AskUserQuestion","tool_use_id":"q1",
        "tool_input":{"questions":[{"question":"Which?","options":[{"label":"choice","description":secret}]}]}});
    let (ask, mut receiver) = {
        let mut r = run.recorder.lock().unwrap();
        *r = Recorder::new(id, run.id, "hook", Arc::new(vec![secret.as_bytes().to_vec()]), Arc::new(builtin_patterns()));
        r.record(&f.manager.store, "test", &payload, &chorus_core::events::from_claude_hook(&payload), 1);
        let receiver = r.hold_reply(&f.manager.store, &payload).unwrap();
        let pending = serde_json::to_string(&r.pending_replies()).unwrap();
        assert!(!pending.contains("quoted"));
        (r.newest_ask().unwrap() as u32, receiver)
    };
    let req = actions::ActionRequest { run_id: run.id, request_id: DELIVERY_REQUEST.into(), action: SessionAction::Answer { ask_id: ask, answers: vec![secret.into()] }};
    actions::ActionRouter::dispatch(&f.manager, id, req).unwrap();
    assert_eq!(receiver.try_recv().unwrap()["hookSpecificOutput"]["updatedInput"]["answers"]["Which?"], secret);
    let stored: (String, String, String) = f.manager.store.with(|c| c.query_row(
        "SELECT details, (SELECT action FROM reply_attempts LIMIT 1), (SELECT body FROM agent_events ORDER BY id DESC LIMIT 1) FROM asks WHERE id=?1",
        [ask], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )).unwrap();
    for text in [stored.0, stored.1, stored.2] { assert!(!text.contains("quoted") && text.contains("REDACTED")); }
}

#[test]
fn delivery_persists_before_two_writes_and_links_one_scrubbed_prompt() {
    use chorus_core::actions::PromptSource;
    use crate::store::delivery::DeliveryStatus;
    let f = Fixture::new("delivery-once");
    let (id, run, chunks) = delivery_fixture(&f, false);
    let result = delivery::deliver_prompt(&f.manager, id, run.id, DELIVERY_REQUEST, "one\r\ntwo", PromptSource::Composer).unwrap();
    assert_eq!(result.status, DeliveryStatus::Written);
    assert_eq!(*chunks.lock().unwrap(), [b"\x1b[200~one\ntwo\x1b[201~".to_vec(), b"\r".to_vec()]);
    let again = delivery::deliver_prompt(&f.manager, id, run.id, DELIVERY_REQUEST, "one\r\ntwo", PromptSource::Composer).unwrap();
    assert_eq!(again, result);
    assert_eq!(chunks.lock().unwrap().len(), 2);
    assert!(delivery::deliver_prompt(&f.manager, id, run.id, OTHER_REQUEST, "next", PromptSource::Composer).is_err());
    run.recorder.lock().unwrap().record(&f.manager.store, "test", &serde_json::json!({}), &[
        chorus_core::events::AgentEvent::TurnStarted { turn: Some("turn1".into()), prompt: Some("one\ntwo".into()) },
        chorus_core::events::AgentEvent::TurnEnded { turn: Some("turn1".into()), message: Some("Done".into()) },
    ], crate::store::now_ms());
    let again = delivery::deliver_prompt(&f.manager, id, run.id, DELIVERY_REQUEST, "one\ntwo", PromptSource::Composer).unwrap();
    assert_eq!(again.status, DeliveryStatus::Observed);
    let (count, source, prompt): (u32, String, u32) = f.manager.store.with(|c| c.query_row(
        "SELECT (SELECT count(*) FROM prompts),source,(SELECT prompt_id FROM turns LIMIT 1) FROM prompts LIMIT 1", [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )).unwrap();
    assert_eq!((count, source.as_str(), prompt), (1, "composer", result.prompt_id));
    assert!(delivery::deliver_prompt(&f.manager, id, run.id, DELIVERY_REQUEST, "changed", PromptSource::Composer).is_err());
    assert!(delivery::deliver_prompt(&f.manager, id, run.id + 1, OTHER_REQUEST, "next", PromptSource::Composer).is_err());
    f.manager.set_locked(id, true, None).unwrap();
    assert!(delivery::deliver_prompt(&f.manager, id, run.id, OTHER_REQUEST, "next", PromptSource::Composer).is_err());
    f.manager.set_locked(id, false, None).unwrap();
    f.manager.close(id, false).unwrap();
    assert!(f.manager.restart(id).is_err());
    assert!(f.manager.open_live(id).is_err(), "refused restart must not recreate a closed live session");
}

#[test]
fn partial_delivery_is_durable_uncertain_and_never_retried() {
    use chorus_core::actions::PromptSource;
    use crate::store::delivery::{self as receipts, DeliveryStatus};
    let f = Fixture::new("delivery-failure");
    let (id, run, chunks) = delivery_fixture(&f, true);
    let result = delivery::deliver_prompt(&f.manager, id, run.id, DELIVERY_REQUEST, "hello", PromptSource::Telegram).unwrap();
    assert_eq!(result.status, DeliveryStatus::Uncertain);
    assert_eq!(chunks.lock().unwrap().len(), 1);
    assert_eq!(delivery::deliver_prompt(&f.manager, id, run.id, DELIVERY_REQUEST, "hello", PromptSource::Telegram).unwrap(), result);
    assert_eq!(chunks.lock().unwrap().len(), 1);
    // A new connection (as after restart) sees the same uncertain receipt.
    let reopened = Store::open(&f.dir.join("chorus.db")).unwrap();
    assert_eq!(receipts::existing(&reopened, id, DELIVERY_REQUEST, "hello", PromptSource::Telegram).unwrap(), Some(result));
    // A rejected duplicate insertion rolls back the prompt too.
    assert!(receipts::begin(&reopened, id, run.id, DELIVERY_REQUEST, "hello", PromptSource::Telegram, 0).is_err());
    let count: u32 = reopened.with(|c| c.query_row("SELECT count(*) FROM prompts", [], |r| r.get(0))).unwrap();
    assert_eq!(count, 1);
}

#[test]
fn unsupported_replies_and_observed_approvals_never_write_to_pty() {
    use chorus_core::actions::{ApprovalDecision, PromptSource, SessionAction};
    let f = Fixture::new("delivery-approval");
    let (id, run, chunks) = delivery_fixture(&f, false);
    for action in [
        SessionAction::Approval { ask_id: 1, decision: ApprovalDecision::Allow, message: None },
        SessionAction::Answer { ask_id: 1, answers: vec!["yes".into()] },
    ] {
        let req = actions::ActionRequest { run_id: run.id, request_id: DELIVERY_REQUEST.into(), action };
        assert!(actions::ActionRouter::dispatch(&f.manager, id, req).unwrap_err().contains("Raw"));
    }
    run.recorder.lock().unwrap().record(&f.manager.store, "test", &serde_json::json!({}), &[
        chorus_core::events::AgentEvent::ApprovalAsked { turn: None, ask: None, tool: "Write".into(), target: None },
    ], crate::store::now_ms());
    let error = delivery::deliver_prompt(&f.manager, id, run.id, DELIVERY_REQUEST, "yes", PromptSource::Composer).unwrap_err();
    assert!(error.contains("approval"), "unexpected refusal: {error}");
    assert!(chunks.lock().unwrap().is_empty());
}

#[test]
fn delivered_secrets_are_scrubbed_and_echoes_are_run_and_time_scoped() {
    use chorus_core::actions::PromptSource;
    use crate::store::delivery as receipts;
    let f = Fixture::new("delivery-scrub");
    let (id, run, _) = delivery_fixture(&f, false);
    *run.recorder.lock().unwrap() = Recorder::new(id, run.id, "hook", Arc::new(vec![b"example-private-value".to_vec()]), Arc::new(builtin_patterns()));
    run.recorder.lock().unwrap().record(&f.manager.store, "test", &serde_json::json!({}), &[chorus_core::events::AgentEvent::Idle], 0);
    let result = delivery::deliver_prompt(&f.manager, id, run.id, DELIVERY_REQUEST, "use example-private-value", PromptSource::Composer).unwrap();
    let stored: String = f.manager.store.with(|c| c.query_row("SELECT text FROM prompts WHERE id=?1", [result.prompt_id], |r| r.get(0))).unwrap();
    assert!(!stored.contains("example-private-value"));
    assert!(receipts::observe(&f.manager.store, run.id + 1, &stored, crate::store::now_ms()).unwrap().is_none());
    assert!(receipts::observe(&f.manager.store, run.id, &stored, crate::store::now_ms() + 61_000).unwrap().is_none());
    assert_eq!(receipts::observe(&f.manager.store, run.id, &stored, crate::store::now_ms()).unwrap(), Some(i64::from(result.prompt_id)));
    assert!(receipts::observe(&f.manager.store, run.id, &stored, crate::store::now_ms()).unwrap().is_none());
}

fn wait_for_or(what: &str, ok: &mut impl FnMut() -> bool, context: impl Fn() -> String) {
    let t0 = Instant::now();
    while !ok() {
        assert!(t0.elapsed() < Duration::from_secs(15), "timed out waiting for {what}; saw {:?}", context());
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Waits until the view has shown `needle` at least `times` times (a typed command is echoed,
/// so its output is the second occurrence).
fn wait_text(seen: &Arc<Mutex<Vec<u8>>>, needle: &str, times: usize) {
    wait_for_or(needle, &mut || text(seen).matches(needle).count() >= times, || text(seen));
}

fn text(seen: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8_lossy(&seen.lock().unwrap()).into_owned()
}

#[test]
fn a_planted_key_is_redacted_in_the_view_the_mirror_and_the_database() {
    let f = Fixture::new("leak");
    // Built at run time so the repo holds no key-shaped string (the secret scan would flag it).
    let key = &format!("sk-or-v1-{}", "0123456789abcdef".repeat(4));
    let cred = vault::add(f.manager.store(), "test", key).unwrap();
    let id = f.shell(Some(CredentialUse { id: cred, env_var: "CHORUS_TEST_KEY".into() }));
    let seen = f.view(id);
    f.manager.write(id, "echo begin:%CHORUS_TEST_KEY%:end").unwrap();
    f.manager.write(id, "\r").unwrap();
    wait_text(&seen, ":end", 2);

    let shown = text(&seen);
    assert!(shown.contains(std::str::from_utf8(PLACEHOLDER).unwrap()), "placeholder missing: {shown}");
    assert_eq!(longest_shared_run(key.as_bytes(), shown.as_bytes(), 8), 0, "key leaked to the view");

    let mirror = std::fs::read(f.dir.join("scrollback").join(format!("{id}.log"))).unwrap();
    assert!(mirror.windows(PLACEHOLDER.len()).any(|w| w == PLACEHOLDER));
    assert_eq!(longest_shared_run(key.as_bytes(), &mirror, 8), 0, "key leaked to the scrollback file");

    for name in ["chorus.db", "chorus.db-wal"] {
        if let Ok(bytes) = std::fs::read(f.dir.join(name)) {
            assert_eq!(longest_shared_run(key.as_bytes(), &bytes, 8), 0, "key leaked to {name}");
        }
    }
}

#[test]
fn a_late_view_gets_the_history_then_live_output() {
    let f = Fixture::new("attach");
    let id = f.shell(None);
    f.manager.write(id, "echo first-line").unwrap();
    f.manager.write(id, "\r").unwrap();
    let early = f.view(id);
    wait_text(&early, "first-line", 2);

    let late = f.view(id);
    assert!(text(&late).contains("first-line"), "history not replayed");
    f.manager.write(id, "echo second-line").unwrap();
    f.manager.write(id, "\r").unwrap();
    wait_text(&late, "second-line", 2);
    std::thread::sleep(Duration::from_millis(500));
    // The command as typed, and its output: twice, never more.
    let shown = text(&late);
    assert_eq!(shown.matches("second-line").count(), 2, "live output repeated: {shown:?}");
}

#[test]
fn exiting_by_itself_is_recorded_and_restart_runs_again() {
    let f = Fixture::new("exit");
    let id = f.shell(None);
    let seen = f.view(id);
    f.manager.write(id, "exit 3").unwrap();
    f.manager.write(id, "\r").unwrap();
    wait_for("exit", || f.manager.summary(id).is_ok_and(|s| !s.live && s.status == SessionStatus::Exited));
    let s = f.manager.summary(id).unwrap();
    assert_eq!(s.last_exit_code, Some(3));
    wait_text(&seen, "exited with code 3", 1);

    let s = f.manager.restart(id).unwrap();
    assert!(s.live && s.status == SessionStatus::Running);
    let last = rows::last_run(f.manager.store(), id).unwrap().unwrap();
    assert!(last.ended_at.is_none());
}

#[test]
fn restart_and_close_are_refused_while_locked() {
    let f = Fixture::new("lock");
    let id = f.shell(None);
    f.manager.set_locked(id, true, None).unwrap();
    assert!(f.manager.close(id, false).unwrap_err().contains("locked"));
    assert!(f.manager.restart(id).unwrap_err().contains("locked"));
    crate::lock::set_pin(f.manager.store(), "4321").unwrap();
    assert!(f.manager.set_locked(id, false, Some("0000")).is_err());
    f.manager.set_locked(id, false, Some("4321")).unwrap();
    let report = f.manager.close(id, false).unwrap();
    assert!(report.killed >= 1);
    assert_eq!(rows::get(f.manager.store(), id).unwrap().unwrap().state, SessionState::Closed);
    assert!(!f.dir.join("scrollback").join(format!("{id}.log")).exists(), "mirror outlived Close");
    let last = rows::last_run(f.manager.store(), id).unwrap().unwrap();
    assert_eq!(last.end_reason, Some(RunEnd::Closed));
}

#[test]
fn restart_and_send_checks_the_draft_and_gives_it_back_when_nothing_started() {
    // D96 on a stopped Claude Code session with no run left to restart (the test can't launch
    // Claude Code): each refusal keeps the draft, and a restart that starts nothing sends nothing.
    let f = Fixture::new("restart-send");
    let store = f.manager.store();
    assert!(composer::restart_send(&f.manager, f.shell(None), 0).unwrap_err().contains("Claude Code"));
    let cwd = f.dir.join("work").display().to_string();
    let options = LaunchOptions::default();
    let id = rows::insert(store, &NewSession {
        name: "Tip script review #7", agent: AgentKind::Claude, cwd: &cwd, credential_id: None, credential_env: None,
        cols: 100, rows: 30, options: &options, agent_session_id: None, provider_id: None, description: None,
        project_id: None, worktree_id: None, routing: None,
    })
    .unwrap();
    rows::set_state(store, id, SessionState::Exited, None).unwrap();
    let draft = |text: &str| {
        let d = crate::store::composer::draft(store, id).unwrap();
        crate::store::composer::save(store, id, d.revision, text).unwrap()
    };
    let prompts = || store.with(|c| c.query_row("SELECT COUNT(*) FROM prompts WHERE session_id = ?1", [id], |r| r.get::<_, u32>(0))).unwrap();

    // Checked like an automation's prompt: a single word could be read as a CLI command.
    let d = draft("update");
    assert!(composer::restart_send(&f.manager, id, d.revision).unwrap_err().contains("sentence"));
    let d = draft("Make the fix you proposed.");
    assert!(composer::restart_send(&f.manager, id, d.revision - 1).unwrap_err().contains("draft changed"));
    rows::set_locked(store, id, true).unwrap();
    assert!(composer::restart_send(&f.manager, id, d.revision).unwrap_err().contains("locked"));
    rows::set_locked(store, id, false).unwrap();
    let e = composer::restart_send(&f.manager, id, d.revision).unwrap_err();
    assert!(e.contains("Nothing was sent"), "{e}");
    assert_eq!(prompts(), 0, "a prompt no run passed on stayed staged");
    assert_eq!(crate::store::composer::draft(store, id).unwrap().text, "Make the fix you proposed.");

    // What a launch would pass: the composer's staged prompt, until a run takes it.
    let p = crate::automations::stage(store, id, "composer", "Make the fix you proposed.").unwrap();
    let row = rows::get(store, id).unwrap().unwrap();
    assert_eq!(crate::automations::staged(store, &row).map(|s| s.text).as_deref(), Some("Make the fix you proposed."));
    assert!(!crate::automations::passed(store, p));
}

#[test]
fn a_quit_then_restore_relaunches_with_history() {
    let f = Fixture::new("restore");
    let id = f.shell(None);
    let seen = f.view(id);
    f.manager.write(id, "echo before-quit").unwrap();
    f.manager.write(id, "\r").unwrap();
    wait_text(&seen, "before-quit", 2);
    let old_run = rows::last_run(f.manager.store(), id).unwrap().unwrap().id;

    // Quit: record the runs, then the processes die with their jobs.
    f.manager.shutdown();
    if let Some(live) = f.manager.live.lock().unwrap().get(&id).cloned() {
        live.current_run().unwrap().end_processes();
    }
    let ended = rows::last_run(f.manager.store(), id).unwrap().unwrap();
    assert_eq!((ended.id, ended.end_reason), (old_run, Some(RunEnd::ChorusClosed)));

    let next = Fixture::manager(&f.dir);
    next.restore().unwrap();
    let s = next.summary(id).unwrap();
    assert!(s.live && s.status == SessionStatus::Running, "not relaunched: {:?}", s.note);
    let replay = Arc::new(Mutex::new(Vec::new()));
    let sink = replay.clone();
    next.attach(id, Box::new(move |b: &[u8]| {
        sink.lock().unwrap().extend_from_slice(b);
        true
    }))
    .unwrap();
    let shown = text(&replay);
    let old = shown.find("before-quit").expect("history kept");
    let line = shown.find("session relaunched").expect("boundary shown");
    assert!(old < line);
    if let Some(live) = next.live.lock().unwrap().get(&id).cloned() {
        next.end_run_for(&live, ExitCause::UserClose);
    }
}

#[test]
fn runs_left_open_by_a_crash_are_healed_at_restore() {
    let f = Fixture::new("crash");
    let id = f.shell(None);
    let run = rows::last_run(f.manager.store(), id).unwrap().unwrap().id;
    // A crash: no shutdown() record; the processes die with the job.
    if let Some(live) = f.manager.live.lock().unwrap().get(&id).cloned() {
        *live.current_run().unwrap().requested.lock().unwrap() = Some(ExitCause::ChorusQuitting);
        live.current_run().unwrap().end_processes();
        live.current_run().map(|r| r.wait_finished(FINISH_TIMEOUT));
    }
    // The watcher of this (still running) manager recorded it; simulate a crash that didn't.
    f.manager.store().with(|c| c.execute("UPDATE runs SET ended_at = NULL, end_reason = NULL WHERE id = ?1", [run])).unwrap();

    let next = Fixture::manager(&f.dir);
    next.restore().unwrap();
    let healed = next.store().with(|c| {
        c.query_row("SELECT end_reason FROM runs WHERE id = ?1", [run], |r| r.get::<_, String>(0))
    });
    assert_eq!(healed.unwrap(), "chorus-crashed");
    assert!(next.summary(id).unwrap().live);
    if let Some(live) = next.live.lock().unwrap().get(&id).cloned() {
        next.end_run_for(&live, ExitCause::UserClose);
    }
}

#[test]
fn bad_input_is_refused_at_the_boundary() {
    let f = Fixture::new("input");
    let mut req = CreateSession {
        agent: AgentKind::Shell,
        cwd: "relative\\path".into(),
        name: None,
        credential: None,
        cols: 100,
        rows: 30,
        options: Default::default(),
        provider_id: None,
        description: None,
        project_id: None,
        tags: Vec::new(),
        workspace: Default::default(),
        routing: None,
        bot_id: None,
        first_prompt: None,
        loop_step: None,
    };
    assert!(f.manager.create(req.clone()).unwrap_err().contains("full path"));
    req.cwd = f.dir.join("missing").display().to_string();
    assert!(f.manager.create(req.clone()).unwrap_err().contains("not a folder"));
    req.cwd = f.dir.join("work").display().to_string();
    req.cols = 0;
    assert!(f.manager.create(req.clone()).is_err());
    req.cols = 100;
    req.credential = Some(CredentialUse { id: 99, env_var: "OK_NAME".into() });
    assert!(f.manager.create(req.clone()).unwrap_err().contains("no longer exists"));
    req.credential = Some(CredentialUse { id: 99, env_var: "PATH".into() });
    assert!(f.manager.create(req).is_err());
    assert!(f.manager.write(12345, "x").is_err());
}

#[test]
fn a_preset_launch_names_and_notes_its_sessions() {
    let f = Fixture::new("launch");
    let session = CreateSession {
        agent: AgentKind::Shell,
        cwd: f.dir.join("work").display().to_string(),
        name: Some("Fix login".into()),
        credential: None,
        cols: 100,
        rows: 30,
        options: Default::default(),
        provider_id: None,
        description: Some("  Ticket 42  ".into()),
        project_id: None,
        tags: Vec::new(),
        workspace: Default::default(),
        routing: None,
        bot_id: None,
        first_prompt: None,
        loop_step: None,
    };
    // A shell can't build: Workbench and Pair say so, and nothing starts.
    let refused = f.manager.launch(LaunchRequest { preset: Preset::Workbench, session: session.clone(), count: None }).unwrap_err();
    assert!(refused.contains("other than Shell"), "{refused}");
    assert!(f.manager.list().unwrap().is_empty());
    let swarm = f.manager.presets(AgentKind::Codex).into_iter().find(|p| p.preset == Preset::Swarm).unwrap();
    assert!(swarm.reason.is_none() && swarm.slots.len() == launch_rules::SWARM_MIN as usize);

    let out = f.manager.launch(LaunchRequest { preset: Preset::Solo, session, count: None }).unwrap();
    assert_eq!(out.len(), 1);
    let s = out[0].session.clone().unwrap();
    assert_eq!((s.name.as_str(), s.description.as_deref()), ("Fix login", Some("Ticket 42")));

    let renamed = f.manager.rename(s.id, "Fix the login page").unwrap();
    assert_eq!(renamed.name, "Fix the login page");
    assert!(f.manager.rename(s.id, "  ").is_err());
    assert_eq!(f.manager.set_description(s.id, None).unwrap().description, None);
    assert!(f.manager.set_description(s.id, Some(&"x".repeat(501))).is_err());
}

/// Runs git for a test repository, with an identity so commits work anywhere.
fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=Chorus test", "-c", "user.email=test@example.invalid"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn worktree_request(f: &Fixture) -> CreateSession {
    let repo = f.dir.join("repo");
    std::fs::create_dir_all(repo.join("web")).unwrap();
    std::fs::write(repo.join("web/file.txt"), "one\ntwo\n").unwrap();
    std::fs::write(repo.join("old.txt"), "rename me\n").unwrap();
    std::fs::write(repo.join("image.bin"), [0, 1, 2]).unwrap();
    git_in(&repo, &["init", "-q", "-b", "main"]);
    git_in(&repo, &["add", "."]);
    git_in(&repo, &["commit", "-q", "-m", "first"]);
    CreateSession {
        agent: AgentKind::Shell, cwd: repo.display().to_string(), name: None, credential: None,
        cols: 100, rows: 30, options: Default::default(), provider_id: None, description: None,
        project_id: None, tags: Vec::new(), workspace: Workspace::NewWorktree, routing: None, bot_id: None, first_prompt: None, loop_step: None,
    }
}

#[test]
fn retained_worktrees_are_reused_and_shared_close_preserves_them() {
    let f = Fixture::new("reuse-worktree");
    let request = worktree_request(&f);
    let original = f.manager.create(request.clone()).unwrap();
    let w = original.worktree.unwrap();
    let draft = Path::new(&w.path).join("draft.txt");
    std::fs::write(&draft, "keep this work").unwrap();
    assert!(f.manager.close(original.id, true).unwrap().worktree_note.unwrap().contains("uncommitted"));
    let retained = f.manager.worktrees_list(Some(&request.cwd)).unwrap();
    assert_eq!(retained.len(), 1);
    assert_eq!((retained[0].id, retained[0].open_sessions), (w.id, 0));
    let reuse = CreateSession { workspace: Workspace::ExistingWorktree(w.id), ..request.clone() };
    let second = f.manager.create(reuse.clone()).unwrap();
    assert!(second.live, "{:?}", second.note);
    assert_eq!(second.worktree.unwrap().id, w.id);
    assert_eq!(std::fs::read_to_string(&draft).unwrap(), "keep this work");
    let third = f.manager.create(CreateSession { cwd: w.path.clone(), workspace: Workspace::Here, ..request.clone() }).unwrap();
    assert_eq!(third.worktree.unwrap().id, w.id, "direct launches also protect shared trees");
    assert!(f.manager.close(second.id, true).unwrap().worktree_note.unwrap().contains("another session"));
    let joined = f.manager.create_in(CreateSession { cwd: third.cwd, workspace: Workspace::Here, ..request.clone() }, Some(w.id)).unwrap();
    assert_eq!(joined.worktree.unwrap().id, w.id, "Pair/Workbench join path");
    assert!(f.manager.close(third.id, true).unwrap().worktree_note.unwrap().contains("another session"));
    assert!(f.manager.close(joined.id, true).unwrap().worktree_note.unwrap().contains("uncommitted"));

    // A request cannot choose a record from another repository or escape into a nested repository.
    let other = f.dir.join("other");
    std::fs::create_dir_all(&other).unwrap();
    git_in(&other, &["init", "-q", "-b", "main"]);
    git_in(&other, &["commit", "-q", "--allow-empty", "-m", "first"]);
    assert!(f.manager.create(CreateSession { cwd: other.display().to_string(), ..reuse.clone() }).unwrap_err().contains("different repository"));
    assert!(f.manager.create(CreateSession { workspace: Workspace::ExistingWorktree(u32::MAX), ..request.clone() }).is_err());
    let missing = Path::new(&request.cwd).join("untracked-folder");
    std::fs::create_dir_all(&missing).unwrap();
    assert!(f.manager.create(CreateSession { cwd: missing.display().to_string(), ..reuse.clone() }).unwrap_err().contains("not a folder"));
    let nested = Path::new(&w.path).join("web");
    git_in(&nested, &["init", "-q", "-b", "nested"]);
    git_in(&nested, &["commit", "-q", "--allow-empty", "-m", "nested"]);
    assert!(f.manager.create(CreateSession { cwd: Path::new(&request.cwd).join("web").display().to_string(), ..reuse }).unwrap_err().contains("different working tree"));
    assert!(Path::new(&w.path).exists());
}

#[test]
fn boot_reconcile_keeps_history_and_does_not_relaunch_a_removed_worktree() {
    let f = Fixture::new("worktree-reconcile");
    let request = worktree_request(&f);
    let original = f.manager.create(request.clone()).unwrap();
    let w = original.worktree.unwrap();
    f.manager.shutdown();
    let live = f.manager.live.lock().unwrap().get(&original.id).cloned().unwrap();
    f.manager.end_run_for(&live, ExitCause::ChorusQuitting);
    git_in(Path::new(&request.cwd), &["worktree", "remove", &w.path]);
    // A replacement ordinary folder must not be accepted as the old tree.
    std::fs::create_dir_all(&w.path).unwrap();
    let external = f.dir.join("external");
    git_in(Path::new(&request.cwd), &["worktree", "add", "-b", "external", &external.display().to_string()]);
    let next = Fixture::manager(&f.dir);
    next.restore().unwrap();
    let s = next.summary(original.id).unwrap();
    assert!(!s.live && s.worktree.unwrap().removed);
    assert!(s.note.unwrap().contains("removed"));
    assert_eq!(s.cwd, original.cwd, "no cwd fallback");
    assert!(rows::last_run(next.store(), original.id).unwrap().is_some());
    assert!(git_in(Path::new(&request.cwd), &["branch", "--list", &w.branch]).contains(&w.branch));
    assert!(git_in(Path::new(&request.cwd), &["worktree", "list"]).contains("external"));
    assert!(next.create(CreateSession { workspace: Workspace::ExistingWorktree(w.id), ..request }).is_err());
}

#[test]
fn unreadable_git_is_unknown_and_deleted_folders_are_reconciled() {
    let f = Fixture::new("worktree-unknown");
    let request = worktree_request(&f);
    let s = f.manager.create(request.clone()).unwrap();
    let w = s.worktree.unwrap();
    f.manager.close(s.id, false).unwrap();
    let root = Path::new(&request.cwd);
    std::fs::rename(root.join(".git"), root.join(".git-unavailable")).unwrap();
    let unknown = f.manager.worktrees_list(None).unwrap();
    assert!(!unknown[0].removed && unknown[0].problem.is_some() && unknown[0].status.is_none());
    std::fs::rename(root.join(".git-unavailable"), root.join(".git")).unwrap();
    std::fs::remove_dir_all(&w.path).unwrap();
    assert!(git_in(root, &["worktree", "list"]).contains(&w.branch), "Git still has its prunable record");
    f.manager.reconcile_worktrees().unwrap();
    assert!(worktree_rows::get(f.manager.store(), w.id).unwrap().unwrap().removed);
    assert!(git_in(root, &["branch", "--list", &w.branch]).contains(&w.branch));
}

#[test]
fn git_summary_reports_net_tracked_lines_and_all_change_kinds() {
    let f = Fixture::new("worktree-summary");
    let request = worktree_request(&f);
    let s = f.manager.create(request.clone()).unwrap();
    let w = s.worktree.unwrap();
    let path = Path::new(&w.path);
    let clean = f.manager.session_git_status(s.id).unwrap().unwrap();
    assert_eq!((clean.changes.files, clean.changes.ahead), (0, None));
    git_in(path, &["branch", "--set-upstream-to", "main"]);
    git_in(path, &["commit", "-q", "--allow-empty", "-m", "ahead"]);
    git_in(Path::new(&request.cwd), &["commit", "-q", "--allow-empty", "-m", "behind"]);
    std::fs::write(path.join("web/file.txt"), "one\nthree\n").unwrap();
    git_in(path, &["add", "web/file.txt"]);
    std::fs::write(path.join("web/file.txt"), "one\nthree\nfour\n").unwrap();
    git_in(path, &["mv", "old.txt", "new name.txt"]);
    std::fs::write(path.join("image.bin"), [0, 2, 3]).unwrap();
    std::fs::write(path.join("untracked.txt"), "not included in line counts\n").unwrap();
    let snap = f.manager.session_git_status(s.id).unwrap().unwrap();
    let c = snap.changes;
    assert_eq!((c.ahead, c.behind), (Some(1), Some(1)));
    assert_eq!((c.files, c.staged, c.unstaged, c.untracked), (4, 2, 2, 1));
    let d = snap.diff.unwrap();
    assert_eq!((d.additions, d.deletions, d.binary), (2, 1, 1));
    assert!(snap.diff_note.is_none());
    f.manager.close(s.id, false).unwrap();
}

#[test]
fn git_summary_handles_unborn_and_detached_heads() {
    let f = Fixture::new("git-initial");
    let root = f.dir.join("work");
    git_in(&root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("first.txt"), "first\nsecond\n").unwrap();
    git_in(&root, &["add", "."]);
    let initial = crate::git::snapshot(&root).unwrap();
    assert_eq!(initial.changes.staged, 1);
    assert_eq!(initial.diff.unwrap().additions, 2);
    git_in(&root, &["commit", "-q", "-m", "first"]);
    git_in(&root, &["checkout", "-q", "--detach"]);
    let detached = crate::git::snapshot(&root).unwrap();
    assert_eq!((detached.changes.branch, detached.changes.ahead), (None, None));
    assert_eq!(detached.changes.files, 0);
}

#[test]
fn a_folder_given_in_short_names_is_kept_in_full() {
    // GitHub's Windows runner has its temp folder at C:\Users\RUNNER~1\…, which git writes in full.
    let f = Fixture::new("short-names");
    let long = f.dir.join("a folder with a long name");
    std::fs::create_dir_all(&long).unwrap();
    let full = check_cwd(&long.display().to_string()).unwrap();
    assert!(!full.contains('~'), "{full}");
    // On a volume without short names this is the same spelling.
    let short = crate::agents::short_path(&long).unwrap().display().to_string();
    assert_eq!(check_cwd(&short).unwrap(), full);
}

#[test]
fn a_session_can_work_in_a_worktree_of_its_own() {
    let f = Fixture::new("worktree");
    let repo = f.dir.join("ledger");
    std::fs::create_dir_all(repo.join("web")).unwrap();
    std::fs::write(repo.join("web").join("index.txt"), "hello").unwrap();
    git_in(&repo, &["init", "-q", "-b", "main"]);
    git_in(&repo, &["add", "."]);
    git_in(&repo, &["commit", "-q", "-m", "first"]);
    let folder = repo.join("web").display().to_string();
    let request = CreateSession {
        agent: AgentKind::Shell,
        cwd: folder.clone(),
        name: None,
        credential: None,
        cols: 100,
        rows: 30,
        options: Default::default(),
        provider_id: None,
        description: None,
        project_id: None,
        tags: vec!["Spike".into(), "spike".into(), "review".into()],
        workspace: Workspace::Here,
        routing: None,
        bot_id: None,
        first_prompt: None,
        loop_step: None,
    };
    let first = f.manager.create(request.clone()).unwrap();
    assert_eq!(first.tags, ["spike", "review"]);
    assert_eq!(f.manager.known_tags().unwrap().iter().map(|t| t.tag.as_str()).collect::<Vec<_>>(), ["review", "spike"]);

    // A second session for that folder hears who's already there.
    let check = f.manager.check_folder(&folder);
    let git = check.git.unwrap();
    assert_eq!((git.branch.as_deref(), git.repo.as_str(), git.in_worktree), (Some("main"), "ledger", false));
    assert!(git.new_branch.unwrap().starts_with("chorus2/ledger/"));
    assert_eq!(check.sharing.iter().map(|s| s.id).collect::<Vec<_>>(), [first.id]);

    let second = f.manager.create(CreateSession { workspace: Workspace::NewWorktree, tags: Vec::new(), ..request.clone() }).unwrap();
    let wt = second.worktree.clone().unwrap();
    assert!(wt.branch.starts_with("chorus2/ledger/") && wt.base_branch.as_deref() == Some("main"), "{wt:?}");
    let beside = long_path(&f.dir.join(".chorus2").join("ledger").display().to_string());
    assert!(second.cwd.starts_with(&beside) && second.cwd.ends_with(r"\web"), "{}", second.cwd);
    assert!(second.live, "{:?}", second.note);
    let inside = f.manager.check_folder(&second.cwd);
    assert!(inside.git.unwrap().in_worktree);
    assert_eq!(inside.sharing.iter().map(|s| s.id).collect::<Vec<_>>(), [second.id], "the first session isn't in this tree");

    // Uncommitted work is never removed.
    let draft = Path::new(&wt.path).join("notes.txt");
    std::fs::write(&draft, "draft").unwrap();
    assert_eq!(f.manager.close_check(second.id).unwrap().worktree.unwrap().clean, Some(false));
    std::fs::remove_file(&draft).unwrap();
    let check = f.manager.close_check(second.id).unwrap().worktree.unwrap();
    assert_eq!((check.clean, check.others), (Some(true), 0));
    let note = f.manager.close(second.id, true).unwrap().worktree_note.unwrap();
    assert!(note.contains("Removed"), "{note}");
    assert!(!Path::new(&wt.path).exists());
    assert!(f.manager.summary(second.id).unwrap().worktree.unwrap().removed);
    assert!(git_in(&repo, &["branch", "--list", &wt.branch]).contains(&wt.branch), "the branch is kept");
    assert_eq!(f.manager.recent_folders(), [long_path(&folder)]);

    // A worktree folder deleted outside Chorus: Close clears git's record of it.
    let third = f.manager.create(CreateSession { workspace: Workspace::NewWorktree, ..request }).unwrap();
    let gone = third.worktree.clone().unwrap();
    let live = f.manager.live.lock().unwrap().get(&third.id).cloned().unwrap();
    f.manager.end_run_for(&live, ExitCause::UserClose);
    std::fs::remove_dir_all(&gone.path).unwrap();
    let check = f.manager.close_check(third.id).unwrap().worktree.unwrap();
    assert!(check.gone && check.clean.is_none());
    let note = f.manager.close(third.id, true).unwrap().worktree_note.unwrap();
    assert!(note.contains("already gone"), "{note}");
    assert!(!git_in(&repo, &["worktree", "list"]).contains(&gone.branch));
}

#[test]
fn a_worktree_git_removed_only_in_part_is_reported_so() {
    let f = Fixture::new("worktree-in-use");
    let repo = f.dir.join("ledger");
    std::fs::create_dir_all(repo.join("web")).unwrap();
    std::fs::write(repo.join("web").join("index.txt"), "hello").unwrap();
    git_in(&repo, &["init", "-q", "-b", "main"]);
    git_in(&repo, &["add", "."]);
    git_in(&repo, &["commit", "-q", "-m", "first"]);
    let s = f
        .manager
        .create(CreateSession {
            agent: AgentKind::Shell,
            cwd: repo.display().to_string(),
            name: None,
            credential: None,
            cols: 100,
            rows: 30,
            options: Default::default(),
            provider_id: None,
            description: None,
            project_id: None,
            tags: Vec::new(),
            workspace: Workspace::NewWorktree,
            routing: None,
            bot_id: None,
            first_prompt: None,
            loop_step: None,
        })
        .unwrap();
    let wt = s.worktree.clone().unwrap();

    // A process outside the session works in the worktree, so git can't delete that folder (L44).
    let mut holder = std::process::Command::new("cmd.exe")
        .arg("/k")
        .current_dir(Path::new(&wt.path).join("web"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let note = f.manager.close(s.id, true).unwrap().worktree_note.unwrap();
    let _ = holder.kill();
    let _ = holder.wait();
    assert!(note.starts_with("Removed the worktree from git, but couldn't delete all of its folder"), "{note}");
    assert!(f.manager.summary(s.id).unwrap().worktree.unwrap().removed);
    assert!(!git_in(&repo, &["worktree", "list"]).contains(&wt.branch));
}
