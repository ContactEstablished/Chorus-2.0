//! P18: OpenMausBot's local API (`_docs/spikes/P18.md` S1). Reads need no token; writes carry the
//! token in the `Authorization` header only, never a URL. Calls block on the async runtime, so they
//! run on the handoff thread or a command's blocking thread, never on the runtime itself.

use std::sync::OnceLock;
use std::time::Duration;

use chorus_core::maus::{self, API, Bot, PAIRING_LABEL};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const TIMEOUT: Duration = Duration::from_secs(10);

fn client() -> Result<&'static reqwest::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| format!("making the HTTP client: {e}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Why a call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// Nothing answered on 127.0.0.1:8799.
    NotRunning,
    /// It didn't answer in time. A write may have happened anyway.
    Timeout,
    /// MausBot answered with an error; its own words.
    Refused(String),
    Other(String),
}

impl Failure {
    pub fn text(&self) -> String {
        match self {
            Self::NotRunning => "OpenMausBot isn't running (Chorus looks for it on 127.0.0.1:8799).".into(),
            Self::Timeout => "OpenMausBot didn't answer in time; it may have done it anyway.".into(),
            Self::Refused(e) => format!("OpenMausBot said: {e}"),
            Self::Other(e) => format!("Chorus couldn't talk to OpenMausBot: {e}"),
        }
    }
}

/// A network failure in words: its innermost cause, never the URL (which holds nothing secret, but
/// is noise).
fn failure(e: &reqwest::Error) -> Failure {
    if e.is_timeout() {
        return Failure::Timeout;
    }
    if e.is_connect() {
        return Failure::NotRunning;
    }
    let mut cause: &dyn std::error::Error = e;
    while let Some(next) = cause.source() {
        cause = next;
    }
    Failure::Other(cause.to_string())
}

fn call(method: Method, path: &str, token: Option<&str>, body: Option<&Value>) -> Result<Value, Failure> {
    let client = client().map_err(Failure::Other)?;
    let url = format!("{API}{path}");
    let sent = tauri::async_runtime::block_on(async {
        let mut request = client.request(method, &url);
        if let Some(t) = token {
            request = request.bearer_auth(t);
        }
        if let Some(b) = body {
            request = request.header(reqwest::header::CONTENT_TYPE, "application/json").body(b.to_string());
        }
        let response = request.send().await?;
        let status = response.status();
        Ok::<_, reqwest::Error>((status, response.text().await?))
    });
    let (status, text) = sent.map_err(|e| failure(&e.without_url()))?;
    let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(value);
    }
    let said = value
        .get("error")
        .and_then(Value::as_str)
        .map(|s| s.chars().take(200).collect::<String>())
        .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
    Err(Failure::Refused(said))
}

/// The running OpenMausBot's version.
pub fn health() -> Result<String, Failure> {
    let v = call(Method::GET, "/api/health", None, None)?;
    if v.get("app").and_then(Value::as_str) != Some("openmausbot") {
        return Err(Failure::Other("something other than OpenMausBot answers on 127.0.0.1:8799".into()));
    }
    Ok(v.get("version").and_then(Value::as_str).unwrap_or("unknown").to_owned())
}

/// Its bots and their conversations. No token: loopback reads have the owner's access.
pub fn bots() -> Result<Vec<Bot>, Failure> {
    let v = call(Method::GET, "/api/bots", None, None)?;
    maus::parse_bots(&v).map_err(Failure::Other)
}

/// What a token can do, as MausBot reports it. Stored as plain settings (no secret).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingFacts {
    pub session_id: Option<String>,
    pub scopes: Vec<String>,
    pub expires_at_ms: Option<i64>,
}

fn facts_of(v: &Value) -> PairingFacts {
    let s = v.get("session").unwrap_or(v);
    PairingFacts {
        session_id: s.get("id").and_then(Value::as_str).map(str::to_owned),
        scopes: s.get("scopes").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default(),
        expires_at_ms: s.get("expiresAt").and_then(Value::as_i64),
    }
}

/// Before a code or a token is sent anywhere: the listener on 8799 must say it's OpenMausBot.
fn ensure_mausbot() -> Result<(), Failure> {
    health().map(drop)
}

/// Redeems a pairing code Matthew made in MausBot (Settings → Phone & devices) for a token.
pub fn pair(code: &str) -> Result<(String, PairingFacts), Failure> {
    ensure_mausbot()?;
    let v = call(Method::POST, "/api/auth/pair", None, Some(&json!({ "code": code.trim(), "label": PAIRING_LABEL })))?;
    let token = v.get("token").and_then(Value::as_str).filter(|t| maus::is_token(t)).ok_or_else(|| Failure::Other("MausBot paired but sent no key Chorus can use".into()))?;
    Ok((token.to_owned(), facts_of(&v)))
}

/// Whether a token still works, and what it may do.
pub fn check(token: &str) -> Result<PairingFacts, Failure> {
    ensure_mausbot()?;
    call(Method::GET, "/api/auth/session", Some(token), None).map(|v| facts_of(&v))
}

/// A new conversation for `bot_id`, not selected in MausBot's own view, pinned to `cwd` (needs the
/// admin scope); without one, MausBot pins the bot's default folder at its first turn. Returns its
/// thread id.
pub fn create_conversation(token: &str, bot_id: &str, cwd: Option<&str>, title: &str) -> Result<String, Failure> {
    if !maus::valid_id(bot_id) {
        return Err(Failure::Other("that isn't a MausBot bot id".into()));
    }
    ensure_mausbot()?;
    let mut body = json!({ "title": title, "select": false });
    if let Some(cwd) = cwd {
        body["cwd"] = json!(cwd);
    }
    let v = call(Method::POST, &format!("/api/bots/{bot_id}/tasks"), Some(token), Some(&body))?;
    v.pointer("/task/threadId")
        .and_then(Value::as_str)
        .filter(|t| maus::valid_id(t))
        .map(str::to_owned)
        .ok_or_else(|| Failure::Other("MausBot made the conversation but didn't say which".into()))
}

/// Sends `text` to one conversation. The same `send_id` never sends twice.
pub fn send(token: &str, bot_id: &str, thread_id: &str, text: &str, send_id: &str) -> Result<(), Failure> {
    if !maus::valid_id(bot_id) || !maus::valid_id(thread_id) {
        return Err(Failure::Other("that isn't a MausBot conversation".into()));
    }
    ensure_mausbot()?;
    call(Method::POST, &format!("/api/bots/{bot_id}/messages"), Some(token), Some(&json!({ "text": text, "threadId": thread_id, "sendId": send_id })))
        .map(|_| ())
}
