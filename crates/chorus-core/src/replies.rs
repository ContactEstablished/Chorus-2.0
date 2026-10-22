//! Claude's held HTTP hooks. Build replies only for the exact request the user reviewed.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;

use crate::actions::{ApprovalDecision, SessionAction, clean_prompt};

pub const REPLY_WAIT_SECS: u64 = 120;
pub const MAX_REQUEST_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct QuestionOption { pub label: String, pub description: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    pub question: String,
    #[serde(default)]
    pub options: Vec<QuestionOption>,
    #[serde(default)]
    pub multi_select: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ReplyDetails {
    Approval { tool: String, input: String },
    Question { questions: Vec<Question> },
}

/// Kept only in memory, including the original input. History/UI receive a scrubbed copy.
pub struct HookRequest { pub details: ReplyDetails, pub input: Value }

pub fn claude_request(payload: &Value) -> Option<HookRequest> {
    let input = payload.get("tool_input")?.clone();
    if !input.is_object() || input.to_string().len() > MAX_REQUEST_BYTES { return None; }
    let tool = payload.get("tool_name")?.as_str()?;
    let details = match payload.get("hook_event_name")?.as_str()? {
        "PermissionRequest" if tool != "AskUserQuestion" && !tool.is_empty() => ReplyDetails::Approval {
            tool: tool.into(), input: serde_json::to_string_pretty(&input).ok()?,
        },
        "PreToolUse" if tool == "AskUserQuestion" => {
            let questions: Vec<Question> = serde_json::from_value(input.get("questions")?.clone()).ok()?;
            if questions.is_empty() || questions.len() > 4 || questions.iter().enumerate().any(|(i, q)|
                q.question.trim().is_empty() || q.question.len() > 4096 || q.options.len() > 8 || questions[..i].iter().any(|p| p.question == q.question)
            ) { return None; }
            ReplyDetails::Question { questions }
        }
        _ => return None,
    };
    Some(HookRequest { details, input })
}

pub fn response(request: &HookRequest, action: &SessionAction) -> Result<Value, String> {
    match (&request.details, action) {
        (_, SessionAction::Release { .. }) => Ok(json!({})),
        (ReplyDetails::Approval { .. }, SessionAction::Approval { decision, message, .. }) => {
            let mut decision_json = json!({"behavior": match decision { ApprovalDecision::Allow => "allow", ApprovalDecision::Deny => "deny" }});
            if let Some(message) = message.as_deref().filter(|s| !s.trim().is_empty()) {
                if *decision == ApprovalDecision::Allow { return Err("An allow reply cannot send a denial message.".into()); }
                decision_json["message"] = json!(clean_prompt(message)?);
            }
            Ok(json!({"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":decision_json}}))
        }
        (ReplyDetails::Question { questions }, SessionAction::Answer { answers, .. }) => {
            if answers.len() != questions.len() { return Err("Answer every question in this request.".into()); }
            let mut input = request.input.clone();
            let mut mapped = serde_json::Map::new();
            for (q, answer) in questions.iter().zip(answers) { mapped.insert(q.question.clone(), json!(clean_prompt(answer)?)); }
            input["answers"] = Value::Object(mapped);
            Ok(json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":input}}))
        }
        _ => Err("That action does not match this request. Refresh the card.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn questions_preserve_original_input_and_require_all_answers() {
        let input = json!({"questions":[{"question":"File?","options":[],"multiSelect":false},{"question":"Colour?"}],"metadata":{"x":1}});
        let r = claude_request(&json!({"hook_event_name":"PreToolUse","tool_name":"AskUserQuestion","tool_input":input})).unwrap();
        assert!(response(&r, &SessionAction::Answer { ask_id: 1, answers: vec!["a".into()] }).is_err());
        let out = response(&r, &SessionAction::Answer { ask_id: 1, answers: vec!["a.txt".into(), "blue".into()] }).unwrap();
        assert_eq!(out["hookSpecificOutput"]["updatedInput"]["questions"], input["questions"]);
        assert_eq!(out["hookSpecificOutput"]["updatedInput"]["answers"], json!({"File?":"a.txt","Colour?":"blue"}));
        assert_eq!(out["hookSpecificOutput"]["updatedInput"]["metadata"], input["metadata"]);
        assert!(response(&r, &SessionAction::Approval { ask_id: 1, decision: ApprovalDecision::Allow, message: None }).is_err());
    }
    #[test]
    fn approvals_are_once_only_and_release_has_no_decision() {
        let r = claude_request(&json!({"hook_event_name":"PermissionRequest","tool_name":"Write","tool_input":{"file_path":"a"}})).unwrap();
        let out = response(&r, &SessionAction::Approval { ask_id: 1, decision: ApprovalDecision::Deny, message: Some("Please stop".into()) }).unwrap();
        assert_eq!(out["hookSpecificOutput"]["decision"], json!({"behavior":"deny","message":"Please stop"}));
        assert_eq!(response(&r, &SessionAction::Release { ask_id: 1 }).unwrap(), json!({}));
        assert!(claude_request(&json!({"hook_event_name":"PermissionRequest","tool_name":"Write","tool_input":{"content":"x".repeat(MAX_REQUEST_BYTES)}})).is_none());
    }
}
