//! Displayable agent output only: never reasoning, tool results, or guessed checklists.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep { pub id: String, pub text: String, pub status: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Progress {
    Message { text: String },
    Plan { steps: Vec<PlanStep> },
    ProposedPlan { text: String },
}

pub const MAX_TEXT: usize = 32 * 1024;
pub fn current_interrupt(line: &str, launched_ms: i64) -> bool {
    crate::claude::transcript_line_interrupts(line)
        && serde_json::from_str::<Value>(line).ok().and_then(|v|v.get("timestamp").and_then(Value::as_str).and_then(crate::codex::parse_utc_ms)).is_some_and(|at|at>=launched_ms)
}
fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)?.as_str().filter(|s| !s.trim().is_empty() && s.len() <= MAX_TEXT).map(str::to_owned)
}
fn status(value: &str) -> Option<&'static str> {
    match value { "pending" => Some("pending"), "in_progress" | "inProgress" => Some("in-progress"), "completed" => Some("completed"), _ => None }
}

pub fn plan(items: &Value) -> Option<Progress> {
    let items = items.as_array()?;
    if items.len() > 100 { return None; }
    let steps = items.iter().enumerate().map(|(i, p)| Some(PlanStep {
        id: text(p, "id").unwrap_or_else(|| i.to_string()),
        text: text(p, "step").or_else(|| text(p, "content"))?,
        status: status(p.get("status")?.as_str()?)?.into(),
    })).collect::<Option<Vec<_>>>()?;
    Some(Progress::Plan { steps })
}

/// Apply only successful task-tool output. Failed or unknown tool shapes leave the plan intact.
pub fn claude_plan(payload: &Value, previous: &[PlanStep]) -> Option<Progress> {
    if payload.get("hook_event_name")?.as_str()? != "PostToolUse" { return None; }
    let input = payload.get("tool_input")?;
    match payload.get("tool_name")?.as_str()? {
        "TodoWrite" => plan(input.get("todos")?),
        "TaskCreate" | "TaskUpdate" => {
            let response = payload.get("tool_response").cloned().unwrap_or(Value::Null);
            let response = if let Some(s) = response.as_str() { serde_json::from_str(s).ok()? } else { response };
            let task = response.get("task").unwrap_or(&response);
            let id = text(input, "taskId").or_else(|| text(task, "id"))?;
            let mut steps = previous.to_vec();
            let at = steps.iter().position(|s| s.id == id);
            if input.get("status").and_then(Value::as_str) == Some("deleted") {
                if let Some(i) = at { steps.remove(i); }
            } else {
                let title = text(input, "subject").or_else(|| text(task, "subject")).or_else(|| at.map(|i| steps[i].text.clone()))?;
                let value = input.get("status").or_else(|| task.get("status")).and_then(Value::as_str);
                let state = match value { Some(s) => status(s)?.to_owned(), None => at.map(|i| steps[i].status.clone()).unwrap_or_else(|| "pending".into()) };
                let step = PlanStep { id, text: title, status: state };
                match at { Some(i) => steps[i] = step, None if steps.len() < 100 => steps.push(step), _ => return None }
            }
            Some(Progress::Plan { steps })
        }
        _ => None,
    }
}

/// Claude's main transcript assistant text. Stable UUID + block index permits idempotent polling.
pub fn claude_message(line: &str) -> Vec<(String, i64, Progress)> {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return Vec::new() };
    if v.get("type").and_then(Value::as_str) != Some("assistant") || v.get("isSidechain").and_then(Value::as_bool) == Some(true) { return Vec::new(); }
    let Some(id) = text(&v, "uuid") else { return Vec::new() };
    let Some(at) = v.get("timestamp").and_then(Value::as_str).and_then(crate::codex::parse_utc_ms) else { return Vec::new() };
    v.pointer("/message/content").and_then(Value::as_array).into_iter().flatten().enumerate().filter_map(|(i, part)| {
        if part.get("type")?.as_str()? != "text" { return None; }
        Some((format!("{id}:{i}"), at, Progress::Message { text: text(part, "text")? }))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn transcript_excludes_reasoning_tools_subagents_and_invalid_dates() {
        let mut v = json!({"type":"assistant","uuid":"m1","timestamp":"2026-10-23T10:00:00Z","message":{"content":[{"type":"thinking","thinking":"private"},{"type":"text","text":"Checking the tests."},{"type":"tool_use","name":"Read"}]}});
        let result = claude_message(&v.to_string());
        assert_eq!(result.len(),1); assert_eq!(result[0].0,"m1:1");
        v["isSidechain"] = json!(true); assert!(claude_message(&v.to_string()).is_empty());
        v["isSidechain"] = json!(false); v["timestamp"] = json!("bad"); assert!(claude_message(&v.to_string()).is_empty());
        let interrupted=json!({"type":"user","timestamp":"2026-10-23T10:00:00Z","message":{"content":"[Request interrupted by user]"}}).to_string();
        let at=crate::codex::parse_utc_ms("2026-10-23T10:00:00Z").unwrap();
        assert!(current_interrupt(&interrupted,at));
        assert!(!current_interrupt(&interrupted,at+1));
    }
    #[test]
    fn task_updates_preserve_other_steps_and_failed_calls_do_not_change_them() {
        let original = vec![PlanStep { id:"1".into(),text:"Check".into(),status:"pending".into() }];
        let mut p = json!({"hook_event_name":"PostToolUse","tool_name":"TaskUpdate","tool_input":{"taskId":"1","status":"completed"}});
        assert_eq!(claude_plan(&p,&original),Some(Progress::Plan { steps:vec![PlanStep { status:"completed".into(),..original[0].clone() }] }));
        p["hook_event_name"] = json!("PostToolUseFailure");assert!(claude_plan(&p,&original).is_none());
        assert!(plan(&json!([{"step":"Don't guess","status":"unknown"}])).is_none());
    }
}
