//! The relay's rows (migration 0023): its one config row, paired users, the outbox, what each sent
//! message was about, button tokens and the audit log.

use chorus_core::relay::render::Act;
use rusqlite::{OptionalExtension, params};

use crate::store::{Store, now_ms};

#[derive(Debug, Clone)]
pub struct Config {
    pub credential_id: Option<u32>,
    pub enabled: bool,
    pub bot_name: Option<String>,
    pub poll_offset: i64,
}

pub fn config(store: &Store) -> Result<Config, String> {
    store.with(|c| {
        c.query_row("SELECT credential_id, enabled, bot_name, poll_offset FROM relay_config WHERE id = 1", [], |r| {
            Ok(Config { credential_id: r.get(0)?, enabled: r.get(1)?, bot_name: r.get(2)?, poll_offset: r.get(3)? })
        })
    })
}

pub fn set_token(store: &Store, credential_id: Option<u32>, bot_name: Option<&str>) -> Result<(), String> {
    store
        .with(|c| c.execute("UPDATE relay_config SET credential_id = ?1, bot_name = ?2, poll_offset = 0 WHERE id = 1", params![credential_id, bot_name]))
        .map(|_| ())
}

pub fn set_enabled(store: &Store, enabled: bool) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE relay_config SET enabled = ?1 WHERE id = 1", [enabled])).map(|_| ())
}

pub fn set_offset(store: &Store, offset: i64) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE relay_config SET poll_offset = ?1 WHERE id = 1", [offset])).map(|_| ())
}

#[derive(Debug, Clone)]
pub struct User {
    pub telegram_id: i64,
    pub name: String,
    pub paired_at: i64,
}

pub fn users(store: &Store) -> Result<Vec<User>, String> {
    store.with(|c| {
        let mut stmt = c.prepare("SELECT telegram_id, name, paired_at FROM relay_users ORDER BY paired_at")?;
        stmt.query_map([], |r| Ok(User { telegram_id: r.get(0)?, name: r.get(1)?, paired_at: r.get(2)? }))?.collect()
    })
}

pub fn add_user(store: &Store, id: i64, name: &str) -> Result<(), String> {
    store
        .with(|c| c.execute("INSERT OR REPLACE INTO relay_users (telegram_id, name, paired_at) VALUES (?1, ?2, ?3)", params![id, name, now_ms()]))
        .map(|_| ())
}

pub fn remove_user(store: &Store, id: i64) -> Result<(), String> {
    store.with(|c| c.execute("DELETE FROM relay_users WHERE telegram_id = ?1", [id])).map(|_| ())
}

/// A message to send. `chat_id` none means every paired phone.
pub struct NewOut<'a> {
    pub kind: &'a str,
    pub run_id: Option<u32>,
    pub question_id: Option<u32>,
    pub chat_id: Option<i64>,
    pub reply_to: Option<i64>,
    pub html: &'a str,
    pub silent: bool,
    /// Labels and their tokens.
    pub buttons: &'a [(String, String)],
}

pub fn enqueue(store: &Store, o: &NewOut<'_>) -> Result<u32, String> {
    let buttons = serde_json::to_string(o.buttons).map_err(|e| e.to_string())?;
    store.with(|c| {
        c.execute(
            "INSERT INTO relay_outbox (kind, run_id, question_id, chat_id, reply_to, html, silent, buttons, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![o.kind, o.run_id, o.question_id, o.chat_id, o.reply_to, o.html, o.silent, buttons, now_ms()],
        )?;
        Ok(c.last_insert_rowid() as u32)
    })
}

#[derive(Debug, Clone)]
pub struct Out {
    pub id: u32,
    pub kind: String,
    pub run_id: Option<u32>,
    pub question_id: Option<u32>,
    pub chat_id: Option<i64>,
    pub reply_to: Option<i64>,
    pub html: String,
    pub silent: bool,
    pub buttons: Vec<(String, String)>,
    pub created_at: i64,
    pub attempts: u32,
}

/// Unsent messages, oldest first.
pub fn pending(store: &Store, limit: u32) -> Result<Vec<Out>, String> {
    store.with(|c| {
        let mut stmt = c.prepare(
            "SELECT id, kind, run_id, question_id, chat_id, reply_to, html, silent, buttons, created_at, attempts FROM relay_outbox WHERE sent_at IS NULL ORDER BY id LIMIT ?1",
        )?;
        stmt.query_map([limit], |r| {
            let buttons: String = r.get(8)?;
            Ok(Out {
                id: r.get(0)?,
                kind: r.get(1)?,
                run_id: r.get(2)?,
                question_id: r.get(3)?,
                chat_id: r.get(4)?,
                reply_to: r.get(5)?,
                html: r.get(6)?,
                silent: r.get(7)?,
                buttons: serde_json::from_str(&buttons).unwrap_or_default(),
                created_at: r.get(9)?,
                attempts: r.get(10)?,
            })
        })?
        .collect()
    })
}

pub fn pending_count(store: &Store) -> u32 {
    store.with(|c| c.query_row("SELECT COUNT(*) FROM relay_outbox WHERE sent_at IS NULL", [], |r| r.get(0))).unwrap_or(0)
}

/// Marks a message sent, or given up on with the reason.
pub fn finish_out(store: &Store, id: u32, error: Option<&str>) -> Result<(), String> {
    store
        .with(|c| c.execute("UPDATE relay_outbox SET sent_at = ?2, error = ?3 WHERE id = ?1", params![id, now_ms(), error]))
        .map(|_| ())
}

pub fn retry_out(store: &Store, id: u32, error: &str) -> Result<(), String> {
    store
        .with(|c| c.execute("UPDATE relay_outbox SET attempts = attempts + 1, error = ?2 WHERE id = ?1", params![id, error]))
        .map(|_| ())
}

pub fn record_message(store: &Store, chat: i64, message: i64, kind: &str, run_id: Option<u32>, question_id: Option<u32>) -> Result<(), String> {
    store
        .with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO relay_messages (chat_id, message_id, kind, run_id, question_id, sent_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![chat, message, kind, run_id, question_id, now_ms()],
            )
        })
        .map(|_| ())
}

/// The question a sent message asked, if it was one.
pub fn question_of_message(store: &Store, chat: i64, message: i64) -> Result<Option<u32>, String> {
    store
        .with(|c| {
            c.query_row("SELECT question_id FROM relay_messages WHERE chat_id = ?1 AND message_id = ?2", params![chat, message], |r| r.get::<_, Option<u32>>(0))
                .optional()
        })
        .map(Option::flatten)
}

/// Where a question was sent, to edit those messages once it is settled.
pub fn messages_of_question(store: &Store, question_id: u32) -> Result<Vec<(i64, i64)>, String> {
    store.with(|c| {
        let mut stmt = c.prepare("SELECT chat_id, message_id FROM relay_messages WHERE question_id = ?1 AND kind = 'question'")?;
        stmt.query_map([question_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
    })
}

pub fn add_button(store: &Store, token: &str, act: &Act, run_id: Option<u32>, question_id: Option<u32>) -> Result<(), String> {
    let act = serde_json::to_string(act).map_err(|e| e.to_string())?;
    store
        .with(|c| {
            c.execute(
                "INSERT INTO relay_buttons (token, act, run_id, question_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![token, act, run_id, question_id, now_ms()],
            )
        })
        .map(|_| ())
}

/// What a pressed button does, and what it is about.
pub struct Button {
    pub act: Act,
    pub run_id: Option<u32>,
    pub question_id: Option<u32>,
}

/// A button's action, used once and within a week; it is marked used here.
pub fn take_button(store: &Store, token: &str) -> Result<Option<Button>, String> {
    let week = now_ms() - 7 * 24 * 60 * 60 * 1000;
    store.with(|c| {
        let found = c
            .query_row(
                "SELECT act, run_id, question_id FROM relay_buttons WHERE token = ?1 AND used_at IS NULL AND created_at > ?2",
                params![token, week],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<u32>>(1)?, r.get::<_, Option<u32>>(2)?)),
            )
            .optional()?;
        let Some((act, run, question)) = found else { return Ok(None) };
        c.execute("UPDATE relay_buttons SET used_at = ?2 WHERE token = ?1", params![token, now_ms()])?;
        Ok(serde_json::from_str(&act).ok().map(|act| Button { act, run_id: run, question_id: question }))
    })
}

pub fn audit(store: &Store, kind: &str, telegram_id: Option<i64>, note: &str) {
    let note: String = note.chars().take(200).collect();
    let _ = store.with(|c| c.execute("INSERT INTO relay_audit (at, kind, telegram_id, note) VALUES (?1, ?2, ?3, ?4)", params![now_ms(), kind, telegram_id, note]));
}
