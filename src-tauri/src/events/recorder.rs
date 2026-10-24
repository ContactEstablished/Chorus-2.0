//! A run's events, recorded (D7): the state machine in `chorus_core::activity` decides which turns
//! open and close, and this writes them, their tool calls and asks to the history tables. Every
//! piece of text is scrubbed first, with the same secrets the run's output is scrubbed with.

use std::collections::HashMap;
use std::sync::Arc;

use chorus_core::activity::{Activity, Tracker, TurnChange};
use chorus_core::events::{AgentEvent, compact_for_log, hook_event_name};
use chorus_core::scrub::{Pattern, scrub_all};
use serde_json::Value;

use crate::store::{Store, history};

/// An approval or question waiting for its answer.
struct OpenAsk {
    row: i64,
    kind: &'static str,
    ask_ref: Option<String>,
    tool: Option<String>,
    target: Option<String>,
    turn_row: Option<i64>,
}

pub struct Recorder {
    pub(crate) raw_dirty: bool,
    pub(crate) input_epoch: u32,
    session_id: u32,
    run_id: u32,
    /// `"hook"` for prompts seen through Claude Code's or Codex's hooks, `"server"` for opencode.
    prompt_source: &'static str,
    tracker: Tracker,
    redact: Arc<Vec<Vec<u8>>>,
    patterns: Arc<Vec<Pattern>>,
    /// Tracker key → turn row.
    turns: HashMap<u64, i64>,
    /// The agent's turn id → turn row, to file tool calls under the right turn.
    turn_rows: HashMap<String, i64>,
    newest_turn: Option<i64>,
    /// Call id → tool row; calls without an id are matched by tool name, oldest first.
    tools: HashMap<String, i64>,
    unnamed_tools: Vec<(String, i64)>,
    asks: Vec<OpenAsk>,
    pub(crate) replies: HashMap<i64, super::replies::HeldReply>,
    /// The agent's full transcript, from the first hook payload that names it.
    transcript: Option<String>,
    plan: Vec<chorus_core::progress::PlanStep>,
    progress_sequence: u64,
}

impl Recorder {
    pub fn new(session_id: u32, run_id: u32, prompt_source: &'static str, redact: Arc<Vec<Vec<u8>>>, patterns: Arc<Vec<Pattern>>) -> Self {
        Self {
            raw_dirty: false,
            input_epoch: 0,
            session_id,
            run_id,
            prompt_source,
            tracker: Tracker::default(),
            redact,
            patterns,
            turns: HashMap::new(),
            turn_rows: HashMap::new(),
            newest_turn: None,
            tools: HashMap::new(),
            unnamed_tools: Vec::new(),
            asks: Vec::new(),
            replies: HashMap::new(),
            transcript: None,
            plan: Vec::new(),
            progress_sequence: 0,
        }
    }

    pub fn transcript_path(&self) -> Option<String> {
        self.transcript.clone()
    }

    pub fn progress(&mut self, store: &Store, turn: Option<&str>, key: &str, content: chorus_core::progress::Progress, at: i64) {
        let Some(row) = self.turn_for(turn) else { return };
        self.progress_at(store, row, key, content, at);
    }

    pub fn progress_at(&mut self, store: &Store, row: i64, key: &str, content: chorus_core::progress::Progress, at: i64) {
        let Ok(value) = serde_json::to_value(content) else { return };
        let Ok(content) = serde_json::from_value::<chorus_core::progress::Progress>(self.clean_value(&value)) else { return };
        if let chorus_core::progress::Progress::Plan { steps } = &content { self.plan = steps.clone(); }
        let _ = history::progress(store, row, key, &content, at);
    }

    pub fn has_open_turn(&self) -> bool {
        self.tracker.open_turns() > 0
    }

    pub fn activity(&self) -> Activity {
        self.tracker.activity()
    }

    pub fn stale(&self, now_ms: i64, last_output_ms: i64) -> bool {
        self.tracker.stale(now_ms, last_output_ms)
    }

    pub(crate) fn clean(&self, text: &str) -> String {
        String::from_utf8_lossy(&scrub_all(text.as_bytes(), &self.redact, &self.patterns)).into_owned()
    }

    pub(crate) fn clean_value(&self, value: &Value) -> Value {
        chorus_core::scrub::scrub_json(value, &self.redact, &self.patterns)
    }

    fn reply_details(&self, request: &chorus_core::replies::HookRequest) -> Option<chorus_core::replies::ReplyDetails> {
        use chorus_core::replies::ReplyDetails;
        Some(match &request.details {
            ReplyDetails::Approval { tool, .. } => ReplyDetails::Approval {
                tool: self.clean(tool), input: serde_json::to_string_pretty(&self.clean_value(&request.input)).ok()?,
            },
            details => serde_json::from_value(self.clean_value(&serde_json::to_value(details).ok()?)).ok()?,
        })
    }

    pub fn pending_question(&self) -> bool {
        self.asks.iter().any(|a| a.kind == "question")
    }

    pub fn newest_ask(&self) -> Option<i64> { self.asks.last().map(|a| a.row) }

    /// Only called just after recording the corresponding ask, under this recorder's lock.
    pub fn hold_reply(&mut self, store: &Store, payload: &Value) -> Option<super::replies::HookReceiver> {
        let request = chorus_core::replies::claude_request(payload)?;
        self.hold_request(store, request)
    }

    pub fn hold_request(&mut self, store: &Store, request: chorus_core::replies::HookRequest) -> Option<super::replies::HookReceiver> {
        let ask = self.asks.last()?;
        let kind = match request.details { chorus_core::replies::ReplyDetails::Question { .. } => "question", _ => "approval" };
        if ask.kind != kind || self.replies.len() >= 16 || self.replies.contains_key(&ask.row) { return None; }
        let details = serde_json::to_string(&self.reply_details(&request)?).ok()?;
        store.with(|c| c.execute("UPDATE asks SET details = ?2 WHERE id = ?1", rusqlite::params![ask.row, details])).ok()?;
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.replies.insert(ask.row, super::replies::HeldReply { request, sender: Some(sender), opened: std::time::Instant::now() });
        Some(receiver)
    }

    pub fn pending_replies(&self) -> Vec<super::replies::PendingReply> {
        self.asks.iter().filter_map(|a| {
            let reply = self.replies.get(&a.row)?;
            let details = self.reply_details(&reply.request)?;
            Some(super::replies::PendingReply {
                ask_id: u32::try_from(a.row).ok()?, run_id: self.run_id, details,
                denial_reason_supported: reply.request.protocol.is_none(),
                remaining_secs: chorus_core::replies::REPLY_WAIT_SECS.saturating_sub(reply.opened.elapsed().as_secs()) as u32,
                blocked_reason: (!reply.available()).then(|| if reply.sender.is_none() {
                    "Reply attempted or returned to Raw. Waiting for the agent; no automatic resend.".into()
                } else { "Reply channel expired or disconnected. Answer the agent in Raw.".into() }),
            })
        }).collect()
    }

    fn turn_for(&self, turn: Option<&str>) -> Option<i64> {
        turn.and_then(|t| self.turn_rows.get(t).copied()).or(self.newest_turn)
    }

    pub fn server_prompt(&mut self, store: &Store, turn: &str, text: &str, at: i64) {
        let Some(row) = self.turn_rows.get(turn).copied() else { return };
        if store.with(|c| c.query_row("SELECT prompt_id IS NOT NULL FROM turns WHERE id=?1", [row], |r| r.get::<_,bool>(0))).unwrap_or(true) { return; }
        let text = self.clean(text);
        let existing = crate::store::delivery::observe(store, self.run_id, &text, at);
        let prompt = match existing {
            Ok(Some(id)) => Some(id),
            Ok(None) => history::insert_prompt(store, self.session_id, Some(self.run_id), "server", &text).ok(),
            Err(_) => None,
        };
        if let Some(id) = prompt {
            let _ = store.with(|c| c.execute("UPDATE turns SET prompt_id=?2 WHERE id=?1 AND prompt_id IS NULL", rusqlite::params![row,id]));
        }
    }

    pub fn resolved_request(&mut self, store: &Store, request: &str) {
        self.answer(store, |a| a.ask_ref.as_deref() == Some(request), "Request resolved by agent.");
        // Resolution is not evidence that an approval was granted.
        let _ = self.tracker.apply(&AgentEvent::QuestionAnswered { ask: Some(request.into()), answer: None }, crate::store::now_ms());
    }

    /// Records one payload from an agent and the events it carried. The writes are done before
    /// this returns, so a prompt is durable before the agent acts on it.
    pub fn record(&mut self, store: &Store, source: &str, raw: &Value, events: &[AgentEvent], now_ms: i64) {
        if self.transcript.is_none()
            && let Some(path) = raw.get("transcript_path").and_then(Value::as_str).filter(|p| p.len() <= 1024 && std::path::Path::new(p).is_absolute())
        {
            self.transcript = Some(path.to_owned());
            let _ = history::set_run_transcript(store, self.run_id, path);
        }
        let body = compact_for_log(&self.clean_value(raw));
        let _ = history::record_event(store, self.session_id, Some(self.run_id), source, hook_event_name(raw).as_deref(), &body);
        for event in events {
            let changes = self.tracker.apply(event, now_ms);
            for change in changes {
                self.apply_change(store, change, now_ms);
            }
            self.apply_detail(store, event);
        }
        if let Some(plan) = chorus_core::progress::claude_plan(raw, &self.plan) {
            self.progress_sequence += 1;
            let key = raw.get("tool_use_id").and_then(Value::as_str).map(str::to_owned)
                .unwrap_or_else(|| format!("task-{}", self.progress_sequence));
            self.progress(store, raw.get("prompt_id").and_then(Value::as_str), &key, plan, now_ms);
        }
    }

    fn apply_change(&mut self, store: &Store, change: TurnChange, now_ms: i64) {
        match change {
            TurnChange::Opened { key, turn, prompt } => {
                let prompt_row = prompt
                    .filter(|p| !p.trim().is_empty())
                    .and_then(|p| {
                        let text = self.clean(&p);
                        // Shared delivery already persisted this prompt; consume its echo once.
                        match crate::store::delivery::observe(store, self.run_id, &text, now_ms) {
                            Ok(Some(id)) => Some(id),
                            Ok(None) => history::insert_prompt(store, self.session_id, Some(self.run_id), self.prompt_source, &text).ok(),
                            Err(_) => None,
                        }
                    });
                if let Ok(row) = history::open_turn(store, self.session_id, Some(self.run_id), turn.as_deref(), prompt_row) {
                    self.turns.insert(key, row);
                    if let Some(t) = turn {
                        self.turn_rows.insert(t, row);
                    }
                    self.newest_turn = Some(row);
                }
            }
            TurnChange::Closed { key, end, message } => {
                let Some(row) = self.turns.remove(&key) else { return };
                let message = message.map(|m| self.clean(&m));
                let _ = history::close_turn(store, row, end.as_str(), message.as_deref());
                // Asks still open when their turn ends: nothing answered them through a channel.
                let answer = match end {
                    chorus_core::activity::TurnEnd::Interrupted => "turn interrupted",
                    chorus_core::activity::TurnEnd::Lost => "run ended",
                    _ => "no answer seen",
                };
                for ask in self.asks.iter().filter(|a| a.turn_row == Some(row)) {
                    let _ = history::answer_ask(store, ask.row, answer);
                }
                self.asks.retain(|a| a.turn_row != Some(row));
                self.replies.retain(|id, _| self.asks.iter().any(|a| a.row == *id));
            }
        }
    }

    fn answer(&mut self, store: &Store, pick: impl Fn(&OpenAsk) -> bool, answer: &str) {
        if let Some(i) = self.asks.iter().position(pick) {
            let ask = self.asks.remove(i);
            self.replies.remove(&ask.row);
            let _ = history::answer_ask(store, ask.row, answer);
        }
    }

    fn apply_detail(&mut self, store: &Store, event: &AgentEvent) {
        match event {
            AgentEvent::ToolStarted { turn, call, tool, target } => {
                // PreToolUse precedes permission checks. A second Read starting must not
                // discard the first Read's still-held permission request.
                let target = target.as_deref().map(|t| self.clean(t));
                if let Ok(row) = history::tool_started(store, self.session_id, self.turn_for(turn.as_deref()), call.as_deref(), tool, target.as_deref()) {
                    match call {
                        Some(c) => {
                            self.tools.insert(c.clone(), row);
                        }
                        None => self.unnamed_tools.push((tool.clone(), row)),
                    }
                }
            }
            AgentEvent::ToolFinished { turn, call, tool, target, failed } => {
                self.answer(store, |a| a.kind == "approval" && a.tool.as_deref() == Some(tool)
                    && chorus_core::replies::finished_approval(a.ask_ref.as_deref(), call.as_deref(), a.target.as_deref(), target.as_deref()),
                    if *failed { "tool failed; approval outcome not observed" } else { "allowed" });
                let row = call.as_ref().and_then(|c| self.tools.remove(c)).or_else(|| {
                    let i = self.unnamed_tools.iter().position(|(t, _)| t == tool)?;
                    Some(self.unnamed_tools.remove(i).1)
                });
                let row = match row {
                    Some(r) => Some(r),
                    // Seen only finishing (hooks started mid-call): record it whole.
                    None => {
                        let target = target.as_deref().map(|t| self.clean(t));
                        history::tool_started(store, self.session_id, self.turn_for(turn.as_deref()), call.as_deref(), tool, target.as_deref()).ok()
                    }
                };
                if let Some(r) = row {
                    let target = target.as_deref().map(|t| self.clean(t));
                    let _ = history::tool_finished(store, r, *failed, target.as_deref());
                }
            }
            AgentEvent::ApprovalAsked { turn, ask, tool, target } => {
                let subject = self.clean(&match target {
                    Some(t) => format!("{tool}: {t}"),
                    None => tool.clone(),
                });
                let turn_row = self.turn_for(turn.as_deref());
                if let Ok(row) = history::open_ask(store, self.session_id, turn_row, "approval", ask.as_deref(), &subject) {
                    self.asks.push(OpenAsk { row, kind: "approval", ask_ref: ask.clone(), tool: Some(tool.clone()), target: target.clone(), turn_row });
                }
            }
            AgentEvent::ApprovalAnswered { ask, allowed } => {
                let answer = if *allowed { "allowed" } else { "rejected" };
                self.answer(store, |a| a.kind == "approval" && (ask.is_none() || a.ask_ref == *ask), answer);
            }
            AgentEvent::QuestionAsked { turn, ask, question } => {
                let turn_row = self.turn_for(turn.as_deref());
                if let Ok(row) = history::open_ask(store, self.session_id, turn_row, "question", ask.as_deref(), &self.clean(question)) {
                    self.asks.push(OpenAsk { row, kind: "question", ask_ref: ask.clone(), tool: None, target: None, turn_row });
                }
            }
            AgentEvent::QuestionAnswered { ask, answer } => {
                let text = answer.as_deref().map(|a| self.clean(a)).unwrap_or_else(|| "answered".into());
                self.answer(store, |a| a.kind == "question" && (ask.is_none() || a.ask_ref == *ask), &text);
            }
            _ => {}
        }
    }

    /// The run ended: its open turns are lost.
    pub fn run_ended(&mut self, store: &Store) {
        self.replies.clear();
        for change in self.tracker.run_ended() {
            self.apply_change(store, change, crate::store::now_ms());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::sessions::{self as rows, NewSession};
    use chorus_core::agent::{AgentKind, LaunchOptions};
    use chorus_core::events::from_claude_hook;
    use serde_json::json;

    #[test]
    fn progress_is_scrubbed_and_updated_in_place_and_server_prompts_are_idempotent() {
        let store=Store::in_memory();
        store.with(|c| c.execute_batch("INSERT INTO sessions(id,name,agent,cwd,state,cols,rows,created_at) VALUES(1,'test','codex','scratch','running',100,30,0); INSERT INTO runs(id,session_id,program,args,started_at) VALUES(1,1,'test','[]',0);")).unwrap();
        let mut r=Recorder::new(1,1,"server",Arc::new(vec![b"private-value-123".to_vec()]),Arc::new(vec![]));
        r.record(&store,"test",&json!({}),&[AgentEvent::TurnStarted{turn:Some("t".into()),prompt:None}],1);
        r.server_prompt(&store,"t","hello",2);
        r.server_prompt(&store,"t","hello",3);
        for text in ["partial", "done private-value-123"] { r.progress(&store,Some("t"),"m",chorus_core::progress::Progress::Message{text:text.into()},4); }
        let turns=history::recent_turns(&store,1,5).unwrap();
        assert_eq!(turns[0].progress.len(),1);
        assert_eq!(turns[0].prompt.as_deref(),Some("hello"));
        assert!(!serde_json::to_string(&turns).unwrap().contains("private-value-123"));
        assert_eq!(store.with(|c|c.query_row("SELECT count(*) FROM prompts",[],|r|r.get::<_,i64>(0))).unwrap(),1);
    }

    #[test]
    fn concurrent_image_reads_keep_each_approval_available() {
        let store = Store::in_memory();
        store.with(|c| c.execute_batch("INSERT INTO sessions(id,name,agent,cwd,state,cols,rows,created_at) VALUES(1,'test','claude','scratch','running',100,30,0); INSERT INTO runs(id,session_id,program,args,started_at) VALUES(1,1,'test','[]',0);")).unwrap();
        let mut r = Recorder::new(1, 1, "hook", Arc::new(vec![]), Arc::new(vec![]));
        let permission = |path: &str| json!({"hook_event_name":"PermissionRequest","tool_name":"Read","tool_input":{"file_path":path}});
        let a = permission("1.png");
        r.record(&store, "test", &a, &from_claude_hook(&a), 1);
        let _receiver_a = r.hold_reply(&store, &a).unwrap();
        let pre = json!({"hook_event_name":"PreToolUse","tool_name":"Read","tool_use_id":"second","tool_input":{"file_path":"2.png"}});
        r.record(&store, "test", &pre, &from_claude_hook(&pre), 2);
        assert_eq!(r.pending_replies().len(), 1);
        let b = permission("2.png");
        r.record(&store, "test", &b, &from_claude_hook(&b), 3);
        let _receiver_b = r.hold_reply(&store, &b).unwrap();
        assert_eq!(r.pending_replies().len(), 2);
        let post = json!({"hook_event_name":"PostToolUse","tool_name":"Read","tool_use_id":"second","tool_input":{"file_path":"2.png"}});
        r.record(&store, "test", &post, &from_claude_hook(&post), 4);
        let pending = r.pending_replies();
        assert_eq!(pending.len(), 1);
        assert!(serde_json::to_string(&pending[0].details).unwrap().contains("1.png"));
    }

    #[test]
    fn hook_payloads_become_history_with_secrets_scrubbed() {
        let store = Store::in_memory();
        let id = rows::insert(
            &store,
            &NewSession {
                name: "t",
                agent: AgentKind::Claude,
                cwd: "C:\\w",
                credential_id: None,
                credential_env: None,
                cols: 80,
                rows: 24,
                options: &LaunchOptions::default(),
                agent_session_id: None,
                provider_id: None,
                description: None,
                project_id: None,
                worktree_id: None,
                routing: None,
            },
        )
        .unwrap();
        let run = rows::start_run(&store, &rows::NewRun { session_id: id, pid: None, program: "claude.exe", args: &[], cli_version: None, resumed: false }).unwrap();
        let secret = b"s3cr3t-value-123456".to_vec();
        let mut r = Recorder::new(id, run, "hook", Arc::new(vec![secret]), Arc::new(chorus_core::scrub::builtin_patterns()));
        let feed = |r: &mut Recorder, v: Value, t: i64| {
            let events = from_claude_hook(&v);
            r.record(&store, "claude-hook", &v, &events, t);
        };
        feed(&mut r, json!({"hook_event_name":"UserPromptSubmit","prompt_id":"p1","prompt":"use key s3cr3t-value-123456 please"}), 1);
        feed(&mut r, json!({"hook_event_name":"PermissionRequest","prompt_id":"p1","tool_name":"Write","tool_input":{"file_path":"a.txt"}}), 2);
        assert_eq!(r.activity(), Activity::AwaitingApproval);
        feed(&mut r, json!({"hook_event_name":"PreToolUse","prompt_id":"p1","tool_name":"Write","tool_use_id":"t1","tool_input":{"file_path":"a.txt"}}), 3);
        feed(&mut r, json!({"hook_event_name":"PostToolUse","prompt_id":"p1","tool_name":"Write","tool_use_id":"t1","tool_input":{"file_path":"a.txt"},"tool_response":{"ok":true}}), 4);
        feed(&mut r, json!({"hook_event_name":"Stop","prompt_id":"p1","last_assistant_message":"Wrote a.txt."}), 5);
        assert_eq!(r.activity(), Activity::Ready);

        let turns = history::recent_turns(&store, id, 5).unwrap();
        assert_eq!(turns.len(), 1);
        let t = &turns[0];
        assert_eq!(t.prompt.as_deref(), Some("use key [REDACTED-CREDENTIAL] please"));
        assert_eq!((t.end_reason.as_deref(), t.final_message.as_deref()), (Some("done"), Some("Wrote a.txt.")));
        assert_eq!((t.tools.len(), t.tools[0].finished), (1, true));
        assert_eq!((t.asks[0].subject.as_str(), t.asks[0].answer.as_deref()), ("Write: a.txt", Some("allowed")));
        let logged: String = store.with(|c| c.query_row("SELECT group_concat(body, ' ') FROM agent_events", [], |r| r.get(0))).unwrap();
        assert!(!logged.contains("s3cr3t-value-123456") && !logged.contains("tool_response"));

        feed(&mut r, json!({"hook_event_name":"UserPromptSubmit","prompt_id":"p2","prompt":"next"}), 6);
        r.run_ended(&store);
        assert_eq!(history::recent_turns(&store, id, 5).unwrap()[1].end_reason.as_deref(), Some("lost"));
    }
}
