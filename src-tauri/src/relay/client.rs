//! The Bot API calls the relay makes (Bot API 10.3). The token is only ever in a request's URL, and
//! a `reqwest` error names that URL, so every error is formatted without it (`without_url`) and
//! scrubbed with the token again.

use std::sync::OnceLock;
use std::time::Duration;

use chorus_core::relay::{self, ApiError};
use serde_json::Value;

/// Long polls ask Telegram to wait 25 s; the client allows a little more.
pub const POLL_SECS: u64 = 25;
const TIMEOUT: Duration = Duration::from_secs(40);

fn client() -> Result<&'static reqwest::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| reqwest::Client::builder().timeout(TIMEOUT).build().map_err(|e| format!("making the HTTP client: {e}")))
        .as_ref()
        .map_err(Clone::clone)
}

/// Why a call failed: Telegram refused it, or it never got an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    Api(ApiError),
    Network(String),
}

impl Failure {
    pub fn text(&self) -> String {
        match self {
            Self::Api(e) => format!("Telegram said {}: {}", e.code, e.description),
            Self::Network(e) => format!("couldn't reach Telegram: {e}"),
        }
    }
}

/// One method call with a JSON body.
pub fn call(token: &str, method: &str, body: &Value) -> Result<Value, Failure> {
    let url = format!("https://api.telegram.org/bot{token}/{method}");
    let body = body.to_string();
    let client = client().map_err(Failure::Network)?;
    let sent = tauri::async_runtime::block_on(async move {
        let response = client.post(&url).header(reqwest::header::CONTENT_TYPE, "application/json").body(body).send().await?;
        response.text().await
    });
    let text = sent.map_err(|e| Failure::Network(e.without_url().to_string().replace(token, "[token]")))?;
    relay::result(&text).map_err(Failure::Api)
}

pub fn get_me(token: &str) -> Result<String, Failure> {
    let me = call(token, "getMe", &serde_json::json!({}))?;
    Ok(me.get("username").and_then(Value::as_str).unwrap_or("bot").to_owned())
}

/// The next updates after `offset`, waiting up to [`POLL_SECS`] for one.
pub fn get_updates(token: &str, offset: i64) -> Result<Vec<(i64, relay::Incoming)>, Failure> {
    let body = serde_json::json!({"offset": offset, "timeout": POLL_SECS, "allowed_updates": relay::ALLOWED_UPDATES});
    let updates = call(token, "getUpdates", &body)?;
    relay::parse_updates(&serde_json::json!({"ok": true, "result": updates}).to_string()).map_err(Failure::Api)
}

/// The inline keyboard for buttons already given tokens: one button a row.
pub fn keyboard(buttons: &[(String, String)]) -> Value {
    let rows: Vec<Value> = buttons.iter().map(|(label, token)| serde_json::json!([{"text": label, "callback_data": relay::callback_data(token)}])).collect();
    serde_json::json!({"inline_keyboard": rows})
}

/// Sends HTML to a chat; returns the message's id.
pub fn send(token: &str, chat: i64, html: &str, silent: bool, buttons: &[(String, String)], reply_to: Option<i64>) -> Result<i64, Failure> {
    let mut body = serde_json::json!({
        "chat_id": chat,
        "text": html,
        "parse_mode": "HTML",
        "disable_notification": silent,
        "link_preview_options": {"is_disabled": true},
    });
    if !buttons.is_empty() {
        body["reply_markup"] = keyboard(buttons);
    }
    if let Some(id) = reply_to {
        body["reply_parameters"] = serde_json::json!({"message_id": id, "allow_sending_without_reply": true});
    }
    let sent = call(token, "sendMessage", &body)?;
    sent.get("message_id").and_then(Value::as_i64).ok_or_else(|| Failure::Network("Telegram's answer had no message id".into()))
}

/// Replaces a message's text, which also takes its buttons away.
pub fn edit(token: &str, chat: i64, message: i64, html: &str) -> Result<(), Failure> {
    let body = serde_json::json!({"chat_id": chat, "message_id": message, "text": html, "parse_mode": "HTML", "link_preview_options": {"is_disabled": true}});
    match call(token, "editMessageText", &body) {
        // Editing to the same text is refused; it is already as wanted.
        Err(Failure::Api(e)) if e.description.contains("message is not modified") => Ok(()),
        other => other.map(|_| ()),
    }
}

/// Stops the button's spinner, with a short note.
pub fn answer_callback(token: &str, id: &str, text: &str) -> Result<(), Failure> {
    call(token, "answerCallbackQuery", &serde_json::json!({"callback_query_id": id, "text": text})).map(|_| ())
}

pub fn set_commands(token: &str) -> Result<(), Failure> {
    let commands: Vec<Value> = relay::MENU.iter().map(|(c, d)| serde_json::json!({"command": c, "description": d})).collect();
    call(token, "setMyCommands", &serde_json::json!({"commands": commands})).map(|_| ())
}
