//! The Telegram relay (P15 slice 4, built from HANDOFF-P13's design): one bot, long polling only (no
//! port is opened), heard only from a paired phone's private chat. It carries the loop's questions
//! (with option buttons and reply-to answers), stops, milestones and finished phases, and takes
//! `/status`, `/pause`, `/resume`, `/approve` and `/stop`. The token is a DPAPI-sealed credential,
//! decrypted into memory only, used only in request URLs and never shown, logged or put in an error.

mod client;
pub mod db;
mod inbound;
pub mod outbound;
pub mod sessions;
#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex};

use chorus_core::relay::{self, Pairing};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::sessions::SessionManager;
use crate::store::{credentials, now_ms};
use crate::vault;
use crate::vault::commands::secs;

type Manager<'a> = State<'a, Arc<SessionManager>>;

const TOKEN_LABEL: &str = "Telegram bot";

/// How the relay is doing, for Settings → Phone.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Conn {
    Off,
    NoToken,
    Connecting,
    Connected,
    /// Another program is reading this bot's updates (HTTP 409).
    Conflict,
    Rejected(String),
    Offline(String),
}

struct Relay {
    token: Option<String>,
    pairing: Option<Pairing>,
    conn: Conn,
}

static STATE: Mutex<Relay> = Mutex::new(Relay { token: None, pairing: None, conn: Conn::Off });

fn state() -> std::sync::MutexGuard<'static, Relay> {
    STATE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn set_conn(conn: Conn) {
    state().conn = conn;
}

/// The token, if one is stored and the relay is on.
fn active_token(m: &SessionManager) -> Option<String> {
    let cfg = db::config(m.store()).ok()?;
    if !cfg.enabled {
        return None;
    }
    state().token.clone()
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RelayUser {
    /// Telegram's user id, as text: it can pass 32 bits.
    pub id: String,
    pub name: String,
    pub paired_at_secs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PairingCode {
    pub code: String,
    pub expires_at_secs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RelayView {
    pub has_token: bool,
    pub enabled: bool,
    pub bot_name: Option<String>,
    /// off | no-token | connecting | connected | conflict | rejected | offline
    pub state: String,
    /// Why, when it isn't connected.
    pub note: Option<String>,
    pub users: Vec<RelayUser>,
    pub pairing: Option<PairingCode>,
    /// Messages waiting to go.
    pub queued: u32,
    /// The daily digest's local time, "HH:MM", if there is one.
    pub digest_at: Option<String>,
    /// The session plain messages from the phone go to (`/use`).
    pub target: Option<String>,
    /// Each project not archived, and whether it sends the phone full text.
    pub projects: Vec<RelayProject>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RelayProject {
    pub id: u32,
    pub name: String,
    pub full_text: bool,
}

fn view(m: &SessionManager) -> Result<RelayView, String> {
    let cfg = db::config(m.store())?;
    let full = db::full_text_projects(m.store())?;
    let projects = crate::store::projects::list(m.store())?
        .into_iter()
        .filter(|p| p.state != chorus_core::projects::ProjectState::Archived)
        .map(|p| RelayProject { full_text: full.contains(&p.id), id: p.id, name: p.name })
        .collect();
    let target = sessions::target(m).and_then(|id| m.summary(id).ok()).map(|s| s.name);
    let s = state();
    let (name, note) = match &s.conn {
        Conn::Off => ("off", Some("The relay is off.".to_owned())),
        Conn::NoToken => ("no-token", Some("No bot token yet.".to_owned())),
        Conn::Connecting => ("connecting", None),
        Conn::Connected => ("connected", None),
        Conn::Conflict => ("conflict", Some("Another program is reading this bot's messages (Telegram's 409). Close it, or give Chorus its own bot.".to_owned())),
        Conn::Rejected(why) => ("rejected", Some(format!("Telegram rejected the token: {why}"))),
        Conn::Offline(why) => ("offline", Some(format!("Offline, retrying: {why}"))),
    };
    Ok(RelayView {
        has_token: cfg.credential_id.is_some(),
        enabled: cfg.enabled,
        bot_name: cfg.bot_name,
        state: name.into(),
        note,
        users: db::users(m.store())?
            .into_iter()
            .map(|u| RelayUser { id: u.telegram_id.to_string(), name: u.name, paired_at_secs: secs(u.paired_at) })
            .collect(),
        pairing: s.pairing.as_ref().filter(|p| p.expires_ms > now_ms()).map(|p| PairingCode { code: p.code.clone(), expires_at_secs: secs(p.expires_ms) }),
        queued: db::pending_count(m.store()),
        digest_at: cfg.digest_at,
        target,
        projects,
    })
}

/// Loads the stored token and starts the poller and the sender.
pub fn start(m: &Arc<SessionManager>) -> Result<(), String> {
    if let Some(id) = db::config(m.store())?.credential_id {
        match vault::decrypt(m.store(), id) {
            Ok(t) => state().token = Some(t),
            Err(e) => set_conn(Conn::Rejected(format!("the stored token couldn't be decrypted ({e})"))),
        }
    }
    let poller = m.clone();
    std::thread::Builder::new().name("relay-poll".into()).spawn(move || inbound::poll(&poller)).map_err(|e| e.to_string())?;
    let sender = m.clone();
    std::thread::Builder::new().name("relay-send".into()).spawn(move || outbound::send_loop(&sender)).map_err(|e| e.to_string())?;
    sessions::start(m)
}

#[tauri::command]
#[specta::specta]
pub fn relay_status(state: Manager<'_>) -> Result<RelayView, String> {
    view(&state)
}

/// Stores a bot token after Telegram accepts it (`getMe`). A new bot means pairing again.
#[tauri::command]
#[specta::specta]
pub async fn relay_set_token(state: Manager<'_>, token: String) -> Result<RelayView, String> {
    let m = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || set_token(&m, &token)).await.map_err(|e| e.to_string())?
}

fn set_token(m: &SessionManager, token: &str) -> Result<RelayView, String> {
    let token = token.trim();
    if !relay::token_shape_ok(token) {
        return Err("That doesn't look like a bot token: BotFather's is digits, a colon, then about 35 letters, digits, - and _.".into());
    }
    let name = client::get_me(token).map_err(|e| {
        match &e {
            client::Failure::Api(_) => format!("Telegram didn't accept it: {}", e.text()),
            client::Failure::Network(why) => format!("Chorus couldn't reach Telegram to check it ({why}). Nothing was saved: try Save again."),
        }
        .replace(token, "[token]")
    })?;
    let store = m.store();
    forget_token(m)?;
    let id = vault::add(store, TOKEN_LABEL, token)?;
    db::set_token(store, Some(id), Some(&name))?;
    for u in db::users(store)? {
        db::remove_user(store, u.telegram_id)?;
    }
    {
        let mut s = state();
        s.token = Some(token.to_owned());
        s.pairing = None;
        s.conn = Conn::Connecting;
    }
    let _ = client::set_commands(token);
    db::audit(store, "token-set", None, &format!("bot @{name}"));
    view(m)
}

/// Deletes the stored token and the paired phones.
fn forget_token(m: &SessionManager) -> Result<(), String> {
    let store = m.store();
    if let Some(id) = db::config(store)?.credential_id {
        credentials::delete(store, id)?;
    }
    if let Some(stale) = credentials::list(store)?.into_iter().find(|c| c.label == TOKEN_LABEL) {
        credentials::delete(store, stale.id)?;
    }
    db::set_token(store, None, None)?;
    let mut s = state();
    s.token = None;
    s.pairing = None;
    s.conn = Conn::NoToken;
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn relay_clear_token(state: Manager<'_>) -> Result<RelayView, String> {
    forget_token(&state)?;
    for u in db::users(state.store())? {
        db::remove_user(state.store(), u.telegram_id)?;
    }
    db::audit(state.store(), "token-cleared", None, "");
    view(&state)
}

#[tauri::command]
#[specta::specta]
pub fn relay_set_enabled(state: Manager<'_>, enabled: bool) -> Result<RelayView, String> {
    db::set_enabled(state.store(), enabled)?;
    set_conn(if enabled { Conn::Connecting } else { Conn::Off });
    view(&state)
}

/// A pairing code to send from the phone as `/pair 123456`, for ten minutes.
#[tauri::command]
#[specta::specta]
pub fn relay_pair_start(state: Manager<'_>) -> Result<RelayView, String> {
    if self::state().token.is_none() {
        return Err("Add the bot's token first.".into());
    }
    let random: [u8; 4] = vault::random_bytes(4).try_into().map_err(|_| "no randomness")?;
    self::state().pairing = Some(Pairing::new(random, now_ms()));
    view(&state)
}

#[tauri::command]
#[specta::specta]
pub fn relay_unpair(state: Manager<'_>, id: String) -> Result<RelayView, String> {
    let id: i64 = id.parse().map_err(|_| "That isn't a paired phone.")?;
    db::remove_user(state.store(), id)?;
    db::audit(state.store(), "unpaired", Some(id), "");
    view(&state)
}

/// Sends one test message to every paired phone.
#[tauri::command]
#[specta::specta]
pub fn relay_test(state: Manager<'_>) -> Result<RelayView, String> {
    if db::users(state.store())?.is_empty() {
        return Err("Pair a phone first.".into());
    }
    outbound::enqueue_text(
        &state,
        "test",
        db::About::default(),
        "Chorus is connected. Loop questions, stops and milestones, and sessions that need you, will come here. Send /help to see what you can send.",
        false,
    )?;
    view(&state)
}

/// The daily digest's local time ("HH:MM"), or none for no digest.
#[tauri::command]
#[specta::specta]
pub fn relay_set_digest(state: Manager<'_>, at: Option<String>) -> Result<RelayView, String> {
    let at = at.map(|a| a.trim().to_owned()).filter(|a| !a.is_empty());
    if let Some(a) = &at
        && chorus_core::relay::sessions::parse_time(a).is_none()
    {
        return Err("Give the time as HH:MM, on a 24-hour clock.".into());
    }
    db::set_digest_at(state.store(), at.as_deref())?;
    view(&state)
}

/// Whether a project's sessions send the phone full text, or status only (D101).
#[tauri::command]
#[specta::specta]
pub fn relay_set_full_text(state: Manager<'_>, project_id: u32, full: bool) -> Result<RelayView, String> {
    if crate::store::projects::get(state.store(), project_id)?.is_none() {
        return Err("That project no longer exists.".into());
    }
    db::set_full_text(state.store(), project_id, full)?;
    view(&state)
}
