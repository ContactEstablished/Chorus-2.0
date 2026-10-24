//! The installed app-server's typed request semantics, separate from transport and persistence.
use serde_json::{Value, json};
use crate::{actions::{ApprovalDecision, SessionAction, clean_prompt}, replies::{HookRequest, Question, QuestionOption, ReplyDetails, MAX_REQUEST_BYTES}};

pub fn request(method: &str, params: &Value) -> Option<HookRequest> {
    if params.to_string().len() > MAX_REQUEST_BYTES { return None; }
    let details = match method {
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => ReplyDetails::Approval {
            tool: if method.contains("commandExecution") { "Command" } else { "File changes" }.into(),
            input: serde_json::to_string_pretty(params).ok()?,
        },
        "item/tool/requestUserInput" => {
            let values = params.get("questions")?.as_array()?;
            if values.is_empty() || values.len() > 4 { return None; }
            let mut ids = std::collections::HashSet::new();
            let questions = values.iter().map(|q| {
                if q.get("isSecret").and_then(Value::as_bool) == Some(true) { return None; }
                let id = q.get("id")?.as_str()?;
                if id.is_empty() || !ids.insert(id) { return None; }
                let question = q.get("question")?.as_str()?.to_owned();
                if question.is_empty() || question.len() > 4096 { return None; }
                let options = q.get("options").and_then(Value::as_array).into_iter().flatten().map(|o| Some(QuestionOption {
                    label: o.get("label")?.as_str()?.into(), description: o.get("description").and_then(Value::as_str).unwrap_or("").into(),
                })).collect::<Option<Vec<_>>>()?;
                if options.len() > 8 { return None; }
                Some(Question { question, options, multi_select: false })
            }).collect::<Option<Vec<_>>>()?;
            ReplyDetails::Question { questions }
        },
        _ => return None,
    };
    Some(HookRequest { details, input: params.clone(), protocol: Some(method.into()) })
}

pub fn response(request: &HookRequest, action: &SessionAction) -> Result<Value, String> {
    match (request.protocol.as_deref(), action) {
        (_, SessionAction::Release { .. }) => Ok(json!({})),
        (Some("item/tool/requestUserInput"), SessionAction::Answer { answers, .. }) => {
            let questions = request.input["questions"].as_array().ok_or("Invalid question request.")?;
            if answers.len() != questions.len() { return Err("Answer every question in this request.".into()); }
            let mut mapped = serde_json::Map::new();
            for (q,a) in questions.iter().zip(answers) {
                mapped.insert(q["id"].as_str().ok_or("Missing question identity.")?.into(), json!({"answers":[clean_prompt(a)?]}));
            }
            Ok(json!({"answers":mapped}))
        },
        (Some("item/commandExecution/requestApproval" | "item/fileChange/requestApproval"), SessionAction::Approval { decision, message, .. }) => {
            if message.as_deref().is_some_and(|s| !s.trim().is_empty()) { return Err("Codex accepts allow or deny without a message. Clear the reason, then send the decision.".into()); }
            let decision = if *decision == ApprovalDecision::Allow { "accept" } else if request.input["availableDecisions"].as_array().is_some_and(|a| !a.iter().any(|v|v=="decline") && a.iter().any(|v|v=="cancel")) { "cancel" } else { "decline" };
            if let Some(allowed) = request.input["availableDecisions"].as_array()
                && !allowed.iter().any(|v|v==decision) { return Err("This Codex decision is unavailable. Respond in Raw.".into()); }
            Ok(json!({"decision":decision}))
        },
        _ => Err("That action does not match this Codex request.".into()),
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn answers_use_request_ids_not_display_labels() {
        let r=request("item/tool/requestUserInput",&json!({"questions":[{"id":"color","question":"Color?","options":null}]})).unwrap();
        assert_eq!(response(&r,&SessionAction::Answer{ask_id:1,answers:vec!["Blue".into()]}).unwrap(),json!({"answers":{"color":{"answers":["Blue"]}}}));
        assert!(request("item/tool/requestUserInput",&json!({"questions":[{"id":"secret","question":"Key?","isSecret":true}]})).is_none());
    }
    #[test] fn approval_is_once_only_and_unsupported_permissions_stay_native() {
        let r=request("item/fileChange/requestApproval",&json!({"itemId":"one"})).unwrap();
        assert_eq!(response(&r,&SessionAction::Approval{ask_id:1,decision:ApprovalDecision::Allow,message:None}).unwrap(),json!({"decision":"accept"}));
        assert_eq!(response(&r,&SessionAction::Release{ask_id:1}).unwrap(),json!({}));
        assert!(request("item/permissions/requestApproval",&json!({})).is_none());
        let r=request("item/commandExecution/requestApproval",&json!({"availableDecisions":["accept","cancel"]})).unwrap();
        assert_eq!(response(&r,&SessionAction::Approval{ask_id:1,decision:ApprovalDecision::Deny,message:None}).unwrap(),json!({"decision":"cancel"}));
    }
}
