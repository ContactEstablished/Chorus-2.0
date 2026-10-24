//! Bounded app-server event interpretation. No transport or persistence lives here.
use std::collections::{HashMap, HashSet};
use serde_json::Value;
use crate::{events::{AgentEvent, tool_target}, progress::{Progress, MAX_TEXT}, replies::{HookRequest, ReplyDetails}};

/// Keep display/control notifications; legacy duplicates, reasoning and token counters are not UI work.
pub fn relevant(event: &Value) -> bool {
    event.get("id").is_some() || matches!(event["method"].as_str(),Some(
        "thread/started" | "turn/started" | "turn/completed" | "turn/plan/updated" |
        "item/agentMessage/delta" | "item/plan/delta" | "item/started" | "item/completed" |
        "serverRequest/resolved"))
}

pub fn changed_root(thread: &str, event: &Value, received_at: i64, attached_at: i64) -> bool {
    received_at > attached_at && event["method"]=="thread/started"
        && event.pointer("/params/thread/ephemeral").and_then(Value::as_bool)==Some(false)
        && event.pointer("/params/thread/parentThreadId").is_none_or(Value::is_null)
        && event.pointer("/params/thread/forkedFromId").is_none_or(Value::is_null)
        && event.pointer("/params/thread/id").and_then(Value::as_str).is_some_and(|id|id!=thread)
}

/// Hydrate only the current/latest turn when attaching after the first prompt materializes it.
pub fn hydrate(thread: &str, response: &Value, include_completed: bool) -> Vec<Value> {
    let Some(turn) = response.pointer("/thread/turns").and_then(Value::as_array).and_then(|t|t.last()) else { return vec![] };
    if !include_completed && turn["status"]!="inProgress" { return vec![]; }
    let Some(id) = turn["id"].as_str() else { return vec![] };
    let mut events=vec![serde_json::json!({"method":"turn/started","params":{"threadId":thread,"turn":{"id":id}}})];
    for item in turn["items"].as_array().into_iter().flatten().take(2000) {
        let method=if item["status"]=="inProgress" { "item/started" } else { "item/completed" };
        events.push(serde_json::json!({"method":method,"params":{"threadId":thread,"turnId":id,"item":item}}));
    }
    if turn["status"]!="inProgress" {
        events.push(serde_json::json!({"method":"turn/completed","params":{"threadId":thread,"turn":turn}}));
    }
    events
}

pub enum Update {
    Event(AgentEvent),
    Progress { turn: String, key: String, content: Progress },
    Prompt { turn: String, text: String },
    Resolved(String),
    Request { id: Value, request: HookRequest, observed: AgentEvent },
    Unsupported,
}

#[derive(Default)]
pub struct Stream {
    turn: Option<String>, ended: bool, final_message: Option<String>,
    messages: HashMap<String, (String, i64)>,
    completed: HashSet<String>, requests: HashSet<String>,
}

impl Stream {
    pub fn accept(&mut self, thread: &str, event: &Value, at: i64) -> Vec<Update> {
        let p = &event["params"];
        if p["threadId"].as_str() != Some(thread) { return vec![]; }
        let Some(method) = event["method"].as_str() else { return vec![] };
        let mut out = Vec::new();
        if method == "turn/started" {
            if let Some(id) = p.pointer("/turn/id").and_then(Value::as_str).filter(|id| id.len() <= 256)
                && self.turn.as_deref() != Some(id) {
                *self = Self { turn: Some(id.into()), ..Self::default() };
                out.push(Update::Event(AgentEvent::TurnStarted { turn: self.turn.clone(), prompt: None }));
            }
            return out;
        }
        if method == "serverRequest/resolved" {
            if let Some(id) = p.get("requestId") { out.push(Update::Resolved(id.to_string())); }
            return out;
        }
        let turn = p["turnId"].as_str().or_else(|| p.pointer("/turn/id").and_then(Value::as_str));
        if self.ended || turn.is_none() || turn != self.turn.as_deref() { return out; }
        let turn = turn.unwrap().to_owned();
        match method {
            "turn/completed" => {
                self.ended = true;
                out.push(Update::Event(match p.pointer("/turn/status").and_then(Value::as_str) {
                    Some("interrupted") => AgentEvent::TurnInterrupted { turn: Some(turn.clone()) },
                    Some("failed") => AgentEvent::TurnFailed { turn: Some(turn.clone()), error: "Codex turn failed. Check Raw for details.".into() },
                    _ => AgentEvent::TurnEnded { turn: Some(turn.clone()), message: self.final_message.take() },
                }));
            }
            "turn/plan/updated" => if let Some(content) = crate::progress::plan(&p["plan"]) {
                out.push(Update::Progress { turn: turn.clone(), key: "codex-plan".into(), content });
            },
            "item/agentMessage/delta" | "item/plan/delta" => {
                if let (Some(id), Some(delta)) = (p["itemId"].as_str(), p["delta"].as_str())
                    && id.len() <= 256 && !self.completed.contains(id)
                    && (self.messages.contains_key(id) || self.messages.len() < 100) {
                    let (text,last) = self.messages.entry(id.into()).or_default();
                    if text.len() + delta.len() <= MAX_TEXT {
                        text.push_str(delta);
                        if at.saturating_sub(*last) >= 250 {
                            *last = at;
                            out.push(Update::Progress { turn: turn.clone(), key: id.into(), content: if method == "item/plan/delta" { Progress::ProposedPlan { text: text.clone() } } else { Progress::Message { text: text.clone() } } });
                        }
                    }
                }
            }
            "item/started" | "item/completed" => {
                let item = &p["item"];
                let Some(id) = item["id"].as_str().filter(|id| id.len() <= 256) else { return out };
                if method == "item/completed" {
                    if self.completed.len() >= 2000 || !self.completed.insert(id.into()) { return out; }
                    self.messages.remove(id);
                }
                match item["type"].as_str() {
                    Some("userMessage") if method == "item/completed" => {
                        let text = item["content"].as_array().into_iter().flatten().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n");
                        if text.len() <= MAX_TEXT { out.push(Update::Prompt { turn: turn.clone(), text }); }
                    }
                    Some("agentMessage" | "plan") if method == "item/completed" => {
                        if let Some(text) = item["text"].as_str().filter(|t| t.len() <= MAX_TEXT) {
                            if item["phase"] == "final_answer" { self.final_message = Some(text.into()); }
                            out.push(Update::Progress { turn: turn.clone(), key: id.into(), content: if item["type"] == "plan" { Progress::ProposedPlan { text: text.into() } } else { Progress::Message { text: text.into() } } });
                        }
                    }
                    Some("commandExecution" | "fileChange" | "mcpToolCall") => {
                        let tool = item["type"].as_str().unwrap().to_owned();
                        let target = tool_target(item);
                        out.push(Update::Event(if method == "item/started" { AgentEvent::ToolStarted { turn: Some(turn.clone()), call: Some(id.into()), tool, target } }
                            else { AgentEvent::ToolFinished { turn: Some(turn.clone()), call: Some(id.into()), tool, target, failed: item["status"] == "failed" || item["status"] == "declined" } }));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        if let Some(id) = event.get("id") {
            if self.requests.len() >= 100 || !self.requests.insert(id.to_string()) { return out; }
            if let Some(request) = crate::codex_protocol::request(method,p) {
                let ask = Some(id.to_string());
                let observed = match &request.details {
                    ReplyDetails::Question { questions } => AgentEvent::QuestionAsked { turn: Some(turn), ask, question: questions[0].question.clone() },
                    ReplyDetails::Approval { tool,.. } => AgentEvent::ApprovalAsked { turn: Some(turn), ask, tool: tool.clone(), target: tool_target(p) },
                };
                out.push(Update::Request { id: id.clone(), request, observed });
            } else { out.push(Update::Unsupported); }
        }
        out
    }
}

#[cfg(test)] mod tests {
    use super::*;
    use serde_json::json;
    fn event(method: &str, params: Value) -> Value { json!({"method":method,"params":params}) }
    #[test] fn startup_backlog_and_child_threads_are_not_raw_conversation_switches() {
        let mut e=event("thread/started",json!({"thread":{"id":"other","ephemeral":false}}));
        assert!(!changed_root("one",&e,100,200));
        assert!(changed_root("one",&e,300,200));
        e["params"]["thread"]["ephemeral"]=json!(true);
        assert!(!changed_root("one",&e,300,200));
        e["params"]["thread"]["ephemeral"]=json!(false);
        e["params"]["thread"]["parentThreadId"]=json!("one");
        assert!(!changed_root("one",&e,300,200));
        assert!(!relevant(&event("item/reasoning/textDelta",json!({"delta":"private"}))));
    }
    #[test] fn restarting_does_not_record_an_old_completed_turn_again() {
        let response=json!({"thread":{"turns":[{"id":"old","status":"completed","items":[]}]}});
        assert!(hydrate("one",&response,false).is_empty());
        assert_eq!(hydrate("one",&response,true).len(),2);
    }
    #[test] fn duplicate_starts_and_foreign_turns_cannot_reassign_history() {
        let mut s = Stream::default();
        let start = event("turn/started",json!({"threadId":"one","turn":{"id":"t"}}));
        assert!(s.accept("other",&start,0).is_empty());
        assert_eq!(s.accept("one",&start,0).len(),1);
        assert!(s.accept("one",&start,0).is_empty());
        assert!(s.accept("one",&event("item/agentMessage/delta",json!({"threadId":"one","turnId":"old","itemId":"m","delta":"wrong"})),500).is_empty());
    }
    #[test] fn streaming_is_throttled_but_completed_text_is_always_flushed_once() {
        let mut s = Stream::default();
        s.accept("one",&event("turn/started",json!({"threadId":"one","turn":{"id":"t"}})),0);
        let d = event("item/agentMessage/delta",json!({"threadId":"one","turnId":"t","itemId":"m","delta":"hello"}));
        assert_eq!(s.accept("one",&d,1000).len(),1);
        assert!(s.accept("one",&d,1100).is_empty());
        let done = event("item/completed",json!({"threadId":"one","turnId":"t","item":{"type":"agentMessage","id":"m","text":"hellohello","phase":"final_answer"}}));
        assert_eq!(s.accept("one",&done,1101).len(),1);
        assert!(s.accept("one",&done,1102).is_empty());
        let end = event("turn/completed",json!({"threadId":"one","turn":{"id":"t","status":"completed"}}));
        assert!(matches!(&s.accept("one",&end,1200)[0],Update::Event(AgentEvent::TurnEnded {message:Some(m),..}) if m == "hellohello"));
        assert!(s.accept("one",&end,1300).is_empty());
    }
}
