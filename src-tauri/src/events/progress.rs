//! Bounded transcript cursor shared by hook recording and the poller. Starting at the first
//! hook's current end avoids importing a resumed conversation as a new turn. Hooks drain before
//! advancing turns, so a fast final message is captured even between polling ticks.

use crate::store::{Store, progress};
use chorus_core::progress as core;

#[derive(Default)]
pub struct Capture { cursor: Option<(String, u64)>, backlog: bool, pub error: Option<String> }

impl super::recorder::Recorder {
    pub fn capture_progress(&mut self, store: &Store, source: &str, raw: &serde_json::Value) {
        if source != "claude-hook" { return; }
        self.read_progress(store);
        let change = match core::claude_plan(&self.clean_value(raw)) {
            Ok(Some(change)) => change, Ok(None) => return,
            Err(reason) => { self.plan_error(store, reason); return; }
        };
        let Some(reference) = raw.get("tool_use_id").and_then(serde_json::Value::as_str).filter(|s| !s.is_empty() && s.len() <= 256) else {
            self.plan_error(store, "A task update has no stable identity. The recorded checklist may be incomplete; check Raw."); return;
        };
        // IDs are also untrusted text. Scrub them without ever logging a raw event reference.
        let reference = self.clean(reference);
        let turn = match raw.get("prompt_id").and_then(serde_json::Value::as_str) {
            Some(id) => self.turn_rows.get(id).copied(), None => self.progress_turn(),
        };
        let result = progress::record_plan(store, self.session_id, self.run_id, turn, &reference, change);
        if result.is_err() { self.plan_error(store, "Plan capture could not be saved: an earlier task may be missing, the checklist may exceed its limit, or storage failed. Check Raw."); }
    }

    fn plan_error(&mut self, store: &Store, reason: &str) {
        self.progress.error = Some(reason.into());
        let _ = progress::mark_plan_incomplete(store, self.run_id);
    }

    fn progress_turn(&self) -> Option<i64> {
        // A queued native prompt can overlap an earlier turn. Do not guess which turn owns text.
        (!self.progress.backlog && self.tracker.open_turns() <= 1).then_some(self.newest_turn).flatten()
    }

    pub fn read_progress(&mut self, store: &Store) {
        let Some(path) = self.transcript_path() else { return; };
        let len = match std::fs::metadata(&path) {
            Ok(m) => m.len(),
            Err(_) => {
                if self.progress.cursor.is_none() { self.progress.cursor = Some((path, 0)); }
                self.progress.error = Some("The agent transcript cannot be read. Intermediate messages may be missing; use Raw.".into()); return;
            }
        };
        let from = match &self.progress.cursor {
            Some((p, offset)) if p == &path && *offset <= len => *offset,
            Some(_) => { self.progress.backlog = true; self.progress.error = Some("The agent transcript changed or was truncated. Earlier intermediate messages may be missing.".into()); 0 },
            None => { self.progress.cursor = Some((path, len)); return; }
        };
        if len == from { return; }
        let Some((lines, consumed)) = crate::sessions::read_new_lines(&path, from, len) else {
            self.progress.error = Some("The agent transcript cannot be read. Intermediate messages may be missing; use Raw.".into()); return;
        };
        if consumed == 0 && len - from >= crate::sessions::MAX_TRANSCRIPT_READ {
            self.progress.error = Some("A transcript record exceeds the read limit. Intermediate capture is paused; use Raw.".into()); return;
        }
        // A backlog can span a subsequent prompt. Keep its text without claiming a turn owner.
        if len - from > crate::sessions::MAX_TRANSCRIPT_READ {
            self.progress.backlog = true;
            self.progress.error = Some("Transcript capture fell behind. Some messages have no identified turn; check Raw for ordering.".into());
        }
        for line in lines {
            if let Some(message) = core::claude_message(&line) {
                let text = core::bounded_text(&self.clean(&message.text), core::MAX_MESSAGE_BYTES);
                let reference = self.clean(&message.id);
                if progress::record_message(store, self.session_id, self.run_id, self.progress_turn(), &reference, &text).is_err() {
                    self.progress.error = Some("Intermediate messages could not be saved. Check Raw; history may be incomplete.".into());
                    return; // Retry the same complete lines; stable IDs make this idempotent.
                }
            }
        }
        self.progress.cursor = Some((path, from + consumed));
        if from + consumed == len { self.progress.backlog = false; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::recorder::Recorder;
    use crate::store::{history, sessions::{self as rows, NewSession}};
    use chorus_core::{agent::{AgentKind, LaunchOptions}, events::from_claude_hook};
    use serde_json::{Value, json};
    use std::{io::Write, sync::Arc};

    fn setup(store: &Store) -> (u32, u32) {
        let session = rows::insert(store, &NewSession {
            name: "progress", agent: AgentKind::Claude, cwd: "C:\\scratch", credential_id: None,
            credential_env: None, cols: 80, rows: 24, options: &LaunchOptions::default(), agent_session_id: None,
            provider_id: None, description: None, project_id: None, worktree_id: None, routing: None,
        }).unwrap();
        let run = rows::start_run(store, &rows::NewRun { session_id: session, pid: None, program: "claude.exe", args: &[], cli_version: None, resumed: false }).unwrap();
        (session, run)
    }
    fn recorder(session: u32, run: u32) -> Recorder {
        Recorder::new(session, run, "hook", Arc::new(vec![b"private\"value\\with-newline\nend".to_vec()]), Arc::new(vec![]))
    }
    fn feed(r: &mut Recorder, s: &Store, payload: Value) {
        r.record(s, "claude-hook", &payload, &from_claude_hook(&payload), 1);
    }
    fn create(call: &str, title: &str) -> Value {
        json!({"hook_event_name":"PostToolUse","prompt_id":"p1","tool_use_id":call,"tool_name":"TaskCreate","tool_input":{"subject":title},"tool_response":{"task":{"id":"1"}}})
    }

    #[test]
    fn task_snapshots_survive_reopen_dedupe_replay_and_do_not_cross_runs() {
        let path = std::env::temp_dir().join(format!("chorus-progress-{}-{}.sqlite", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let (session, run, turn);
        {
            let s = Store::open(&path).unwrap(); (session, run) = setup(&s);
            let mut r = recorder(session, run);
            feed(&mut r, &s, json!({"hook_event_name":"UserPromptSubmit","prompt_id":"p1","prompt":"Check sample"}));
            feed(&mut r, &s, create("c1", "Check sample"));
            feed(&mut r, &s, json!({"hook_event_name":"PostToolUse","prompt_id":"p1","tool_use_id":"u1","tool_name":"TaskUpdate","tool_input":{"taskId":"1","status":"completed"},"tool_response":{"success":true}}));
            feed(&mut r, &s, create("c1", "Check sample"));
            let plan = progress::latest_plan(&s, session).unwrap().unwrap();
            assert_eq!(plan.steps[0].status, core::PlanStatus::Completed);
            turn = plan.turn_id.unwrap();
            assert_eq!(history::recent_turns(&s, session, 10).unwrap()[0].id, turn);
            let new_run = rows::start_run(&s, &rows::NewRun { session_id: session, pid: None, program: "claude.exe", args: &[], cli_version: None, resumed: true }).unwrap();
            let mut restarted = recorder(session, new_run);
            feed(&mut restarted, &s, json!({"hook_event_name":"PostToolUse","tool_use_id":"new-u1","tool_name":"TaskUpdate","tool_input":{"taskId":"1","status":"completed"},"tool_response":{"success":true}}));
            assert!(restarted.progress.error.is_some());
            assert_eq!(progress::latest_plan(&s, session).unwrap().unwrap().run_id, run);
        }
        let s = Store::open(&path).unwrap();
        let plan = progress::latest_plan(&s, session).unwrap().unwrap();
        assert_eq!((plan.run_id, plan.turn_id), (run, Some(turn)));
        assert_eq!(plan.steps[0].status, core::PlanStatus::Completed);
        drop(s); let _ = std::fs::remove_file(path);
    }

    #[test]
    fn fast_transcript_messages_are_scrubbed_before_bounds_and_drain_before_stop() {
        let s = Store::in_memory(); let (session, run) = setup(&s);
        let path = std::env::temp_dir().join(format!("chorus-progress-{}-{}.jsonl", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let message = |id: &str, text: &str| json!({"type":"assistant","uuid":id,"message":{"content":[{"type":"thinking","thinking":"never retain this"},{"type":"text","text":text}]}}).to_string() + "\n";
        std::fs::write(&path, message("old", "Previous run")).unwrap();
        let mut r = recorder(session, run);
        feed(&mut r, &s, json!({"hook_event_name":"UserPromptSubmit","prompt_id":"p1","prompt":"Check sample","transcript_path":path}));
        let line = message("a1", "Checking private\"value\\with-newline\nend now.");
        std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(line.as_bytes()).unwrap();
        feed(&mut r, &s, json!({"hook_event_name":"Stop","prompt_id":"p1","last_assistant_message":"Done"}));
        let messages = progress::recent_messages(&s, session, 200).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "Checking [REDACTED-CREDENTIAL] now.");
        assert!(messages[0].turn_id.is_some());
        // A duplicate transcript record and another poll cannot duplicate its durable row.
        std::fs::OpenOptions::new().append(true).open(&path).unwrap().write_all(line.as_bytes()).unwrap();
        r.read_progress(&s); r.read_progress(&s);
        assert_eq!(progress::recent_messages(&s, session, 200).unwrap().len(), 1);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn malformed_task_and_quoted_credentials_never_look_like_a_complete_plan() {
        let s = Store::in_memory(); let (session, run) = setup(&s); let mut r = recorder(session, run);
        feed(&mut r, &s, create("c1", "Check private\"value\\with-newline\nend"));
        assert_eq!(progress::latest_plan(&s, session).unwrap().unwrap().steps[0].title, "Check [REDACTED-CREDENTIAL]");
        let mut bad = create("c2", "bad"); bad["tool_response"] = json!({"new-format":true});
        feed(&mut r, &s, bad);
        assert!(r.progress.error.as_deref().unwrap().contains("incomplete"));
        let plan = progress::latest_plan(&s, session).unwrap().unwrap();
        assert_eq!(plan.steps.len(), 1);
        assert!(plan.incomplete);
    }
}
