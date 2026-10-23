//! Structured progress only: successful task tools and assistant transcript text. Thinking,
//! tool output, sidechains and proposed (not yet successful) task edits are never messages.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

pub const MAX_MESSAGE_BYTES: usize = 32 * 1024;
pub const MAX_PLAN_STEPS: usize = 128;
pub const MAX_TITLE_BYTES: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum PlanStatus { Pending, InProgress, Completed }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PlanStep { pub id: String, pub title: String, pub status: PlanStatus }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanChange {
    Upsert { id: String, title: Option<String>, status: Option<PlanStatus> },
    Delete { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantText { pub id: String, pub text: String }

fn text<'a>(v: &'a Value, key: &str, max: usize) -> Option<&'a str> {
    v.get(key)?.as_str().filter(|s| !s.trim().is_empty() && s.len() <= max)
}
fn status(s: &str) -> Option<PlanStatus> {
    match s { "pending" => Some(PlanStatus::Pending), "in_progress" => Some(PlanStatus::InProgress), "completed" => Some(PlanStatus::Completed), _ => None }
}

/// Successful PostToolUse payloads only. TaskCreate's response carries the allocated task ID;
/// TaskUpdate without a subject preserves the existing title. Deleted tasks leave the checklist.
pub fn claude_plan(p: &Value) -> Result<Option<PlanChange>, &'static str> {
    if p.get("hook_event_name").and_then(Value::as_str) != Some("PostToolUse")
        || !matches!(p.get("tool_name").and_then(Value::as_str), Some("TaskCreate" | "TaskUpdate")) { return Ok(None); }
    // Owner, dependency and description changes do not alter the checklist's displayed fields.
    if p.get("tool_name").and_then(Value::as_str) == Some("TaskUpdate")
        && p.pointer("/tool_response/success") == Some(&Value::Bool(true))
        && let Some(input) = p.get("tool_input").and_then(Value::as_object)
        && input.contains_key("taskId") && !input.contains_key("subject") && !input.contains_key("status") { return Ok(None); }
    parse_claude_plan(p).map(Some).ok_or("A task update could not be understood. The recorded checklist may be incomplete; check Raw.")
}

fn parse_claude_plan(p: &Value) -> Option<PlanChange> {
    if p.get("hook_event_name")?.as_str()? != "PostToolUse" { return None; }
    let input = p.get("tool_input")?;
    let response = p.get("tool_response")?;
    if response.get("success") == Some(&Value::Bool(false)) || response.get("is_error") == Some(&Value::Bool(true)) { return None; }
    match p.get("tool_name")?.as_str()? {
        "TaskCreate" => {
            let task = response.get("task")?;
            Some(PlanChange::Upsert {
                id: text(task, "id", 256)?.into(), title: Some(text(input, "subject", MAX_TITLE_BYTES)?.into()), status: Some(PlanStatus::Pending),
            })
        }
        "TaskUpdate" => {
            if response.get("success") != Some(&Value::Bool(true)) { return None; }
            let id = text(input, "taskId", 256)?.into();
            if input.get("status").and_then(Value::as_str) == Some("deleted") { return Some(PlanChange::Delete { id }); }
            let status = match input.get("status") { Some(v) => Some(status(v.as_str()?)?), None => None };
            let title = match input.get("subject") { Some(_) => Some(text(input, "subject", MAX_TITLE_BYTES)?.into()), None => None };
            (status.is_some() || title.is_some()).then_some(PlanChange::Upsert { id, title, status })
        }
        _ => None,
    }
}

/// Returns None for a no-op; an unknown task's status-only update is an explicit error. Never
/// invent a title or claim a task completed from a turn ending.
pub fn apply_plan(current: &[PlanStep], change: PlanChange) -> Result<Option<Vec<PlanStep>>, &'static str> {
    let mut next = current.to_vec();
    match change {
        PlanChange::Delete { id } => {
            if !next.iter().any(|s| s.id == id) { return Err("A deleted task was not recorded."); }
            next.retain(|s| s.id != id);
        }
        PlanChange::Upsert { id, title, status } => {
            if let Some(step) = next.iter_mut().find(|s| s.id == id) {
                if let Some(title) = title { step.title = title; }
                if let Some(status) = status { step.status = status; }
            } else if let Some(title) = title {
                if next.len() >= MAX_PLAN_STEPS { return Err("The plan exceeds the capture limit."); }
                next.push(PlanStep { id, title, status: status.unwrap_or(PlanStatus::Pending) });
            } else {
                return Err("A task update refers to a task that was not recorded.");
            }
        }
    }
    Ok((next != current).then_some(next))
}

/// Claude Code 2.1.308 JSONL: one UUID identifies a transcript record (message IDs can repeat
/// across content blocks). Keep text blocks only, never thinking, tool_use, or tool_result.
pub fn claude_message(line: &str) -> Option<AssistantText> {
    let p: Value = serde_json::from_str(line).ok()?;
    if p.get("type")?.as_str()? != "assistant" || p.get("isSidechain") == Some(&Value::Bool(true)) { return None; }
    let id = text(&p, "uuid", 256)?.to_owned();
    let content = p.pointer("/message/content")?.as_array()?;
    let parts = content.iter().filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str)).collect::<Vec<_>>();
    let text = parts.join("\n");
    if text.trim().is_empty() { return None; }
    Some(AssistantText { id, text })
}

/// Bound after scrubbing so truncation cannot split a credential before it is redacted.
pub fn bounded_text(text: &str, max: usize) -> String {
    if text.len() <= max { return text.into(); }
    let mut end = max;
    while !text.is_char_boundary(end) { end -= 1; }
    format!("{}\n[Message truncated]", &text[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn task_success_updates_preserve_titles_and_replay_is_a_noop() {
        let create = json!({"hook_event_name":"PostToolUse","tool_name":"TaskCreate","tool_input":{"subject":"Review"},"tool_response":{"task":{"id":"1"}}});
        let first = apply_plan(&[], claude_plan(&create).unwrap().unwrap()).unwrap().unwrap();
        assert!(apply_plan(&first, claude_plan(&create).unwrap().unwrap()).unwrap().is_none());
        let update = json!({"hook_event_name":"PostToolUse","tool_name":"TaskUpdate","tool_input":{"taskId":"1","status":"completed"},"tool_response":{"success":true}});
        let done = apply_plan(&first, claude_plan(&update).unwrap().unwrap()).unwrap().unwrap();
        assert_eq!(done[0], PlanStep { id: "1".into(), title: "Review".into(), status: PlanStatus::Completed });
        assert!(apply_plan(&[], claude_plan(&update).unwrap().unwrap()).is_err());
        let mut failed = update.clone(); failed["tool_response"]["success"] = json!(false);
        assert!(claude_plan(&failed).is_err());
        failed["hook_event_name"] = json!("PreToolUse"); assert!(claude_plan(&failed).unwrap().is_none());
        assert_eq!(apply_plan(&done, PlanChange::Delete { id: "1".into() }).unwrap(), Some(vec![]));
    }

    #[test]
    fn only_visible_assistant_text_with_stable_identity_is_kept() {
        let mut v = json!({"type":"assistant","uuid":"a1","message":{"content":[{"type":"thinking","thinking":"private"},{"type":"text","text":"Checking files."},{"type":"tool_use","input":{"text":"output"}}]}});
        assert_eq!(claude_message(&v.to_string()).unwrap().text, "Checking files.");
        v["isSidechain"] = json!(true); assert!(claude_message(&v.to_string()).is_none());
        v["isSidechain"] = json!(false); v["type"] = json!("user"); assert!(claude_message(&v.to_string()).is_none());
        assert!(claude_message("{broken").is_none());
        assert_eq!(bounded_text("éé", 3), "é\n[Message truncated]");
    }

    #[test]
    fn malformed_and_oversized_tasks_fail_visibly() {
        let mut p = json!({"hook_event_name":"PostToolUse","tool_name":"TaskUpdate","tool_input":{"taskId":"1","status":"unknown"},"tool_response":{"success":true}});
        assert!(claude_plan(&p).is_err());
        p["tool_input"]["status"] = json!("completed"); assert!(claude_plan(&p).unwrap().is_some());
        let full = (0..MAX_PLAN_STEPS).map(|i| PlanStep { id: i.to_string(), title: "Step".into(), status: PlanStatus::Pending }).collect::<Vec<_>>();
        assert!(apply_plan(&full, PlanChange::Upsert { id: "extra".into(), title: Some("Overflow".into()), status: None }).is_err());
    }
}
