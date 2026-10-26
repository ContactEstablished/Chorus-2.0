//! The relay's rows (migrations 0023 and 0024): its one config row, paired users, the outbox, what
//! each sent message was about, button tokens, the audit log, and which projects send full text.

use std::collections::HashSet;

use chorus_core::relay::render::Act;
use rusqlite::{OptionalExtension, params};

use crate::store::{Store, now_ms};

#[derive(Debug, Clone)]
pub struct Config {
    pub credential_id: Option<u32>,
    pub enabled: bool,
    pub bot_name: Option<String>,
    pub poll_offset: i64,
    /// Where plain messages from the phone go (`/use`).
    pub target_session: Option<u32>,
    /// The daily digest's local time, "HH:MM".
    pub digest_at: Option<String>,
}

pub fn config(store: &Store) -> Result<Config, String> {
    store.with(|c| {
        c.query_row("SELECT credential_id, enabled, bot_name, poll_offset, target_session, digest_at FROM relay_config WHERE id = 1", [], |r| {
            Ok(Config {
                credential_id: r.get(0)?,
                enabled: r.get(1)?,
                bot_name: r.get(2)?,
                poll_offset: r.get(3)?,
                target_session: r.get(4)?,
                digest_at: r.get(5)?,
            })
        })
    })
}

pub fn set_token(store: &Store, credential_id: Option<u32>, bot_name: Option<&str>) -> Result<(), String> {
    store
        .with(|c| {
            c.execute("UPDATE relay_config SET credential_id = ?1, bot_name = ?2, poll_offset = 0, target_session = NULL WHERE id = 1", params![credential_id, bot_name])
        })
        .map(|_| ())
}

pub fn set_enabled(store: &Store, enabled: bool) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE relay_config SET enabled = ?1 WHERE id = 1", [enabled])).map(|_| ())
}

pub fn set_offset(store: &Store, offset: i64) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE relay_config SET poll_offset = ?1 WHERE id = 1", [offset])).map(|_| ())
}

pub fn set_target(store: &Store, session: Option<u32>) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE relay_config SET target_session = ?1 WHERE id = 1", [session])).map(|_| ())
}

pub fn set_digest_at(store: &Store, at: Option<&str>) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE relay_config SET digest_at = ?1 WHERE id = 1", [at])).map(|_| ())
}

/// Projects that send the phone full text.
pub fn full_text_projects(store: &Store) -> Result<HashSet<u32>, String> {
    store.with(|c| {
        let mut stmt = c.prepare("SELECT project_id FROM relay_projects WHERE full_text = 1")?;
        stmt.query_map([], |r| r.get(0))?.collect()
    })
}

pub fn set_full_text(store: &Store, project: u32, full: bool) -> Result<(), String> {
    store
        .with(|c| {
            c.execute(
                "INSERT INTO relay_projects (project_id, full_text) VALUES (?1, ?2) ON CONFLICT (project_id) DO UPDATE SET full_text = ?2",
                params![project, full],
            )
        })
        .map(|_| ())
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

/// What a message is about: a loop run (and its question), or a session (and its ask).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct About {
    pub run_id: Option<u32>,
    pub question_id: Option<u32>,
    pub session_id: Option<u32>,
    pub ask_id: Option<u32>,
}

impl About {
    pub fn run(run: u32) -> Self {
        Self { run_id: Some(run), ..Self::default() }
    }

    pub fn session(session: u32) -> Self {
        Self { session_id: Some(session), ..Self::default() }
    }
}

/// A message to send. `chat_id` none means every paired phone.
pub struct NewOut<'a> {
    pub kind: &'a str,
    pub about: About,
    pub chat_id: Option<i64>,
    /// The message this replies to; for an `edit`, the message whose text it replaces.
    pub reply_to: Option<i64>,
    pub html: &'a str,
    pub silent: bool,
    /// Labels and their tokens.
    pub buttons: &'a [(String, String)],
}

pub fn enqueue(store: &Store, o: &NewOut<'_>) -> Result<u32, String> {
    let buttons = serde_json::to_string(o.buttons).map_err(|e| e.to_string())?;
    let a = o.about;
    store.with(|c| {
        c.execute(
            "INSERT INTO relay_outbox (kind, run_id, question_id, session_id, ask_id, chat_id, reply_to, html, silent, buttons, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![o.kind, a.run_id, a.question_id, a.session_id, a.ask_id, o.chat_id, o.reply_to, o.html, o.silent, buttons, now_ms()],
        )?;
        Ok(c.last_insert_rowid() as u32)
    })
}

#[derive(Debug, Clone)]
pub struct Out {
    pub id: u32,
    pub kind: String,
    pub about: About,
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
            "SELECT id, kind, run_id, question_id, session_id, ask_id, chat_id, reply_to, html, silent, buttons, created_at, attempts
             FROM relay_outbox WHERE sent_at IS NULL ORDER BY id LIMIT ?1",
        )?;
        stmt.query_map([limit], |r| {
            let buttons: String = r.get(10)?;
            Ok(Out {
                id: r.get(0)?,
                kind: r.get(1)?,
                about: About { run_id: r.get(2)?, question_id: r.get(3)?, session_id: r.get(4)?, ask_id: r.get(5)? },
                chat_id: r.get(6)?,
                reply_to: r.get(7)?,
                html: r.get(8)?,
                silent: r.get(9)?,
                buttons: serde_json::from_str(&buttons).unwrap_or_default(),
                created_at: r.get(11)?,
                attempts: r.get(12)?,
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

/// A loop run's messages or a session's, for settling the last one of a kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    Run(u32),
    Session(u32),
}

impl Subject {
    fn column(self) -> &'static str {
        match self {
            Self::Run(_) => "run_id",
            Self::Session(_) => "session_id",
        }
    }

    fn id(self) -> u32 {
        match self {
            Self::Run(id) | Self::Session(id) => id,
        }
    }
}

/// The newest message of a kind queued for a run or session: its id, text, when it was queued and
/// whether it went.
pub fn last_out(store: &Store, subject: Subject, kind: &str) -> Result<Option<(u32, String, i64, bool)>, String> {
    let sql = format!("SELECT id, html, created_at, sent_at IS NOT NULL FROM relay_outbox WHERE {} = ?1 AND kind = ?2 ORDER BY id DESC LIMIT 1", subject.column());
    store.with(|c| c.query_row(&sql, params![subject.id(), kind], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional())
}

/// Replaces a message's text before it goes and takes its buttons away; it is settled, so it isn't
/// settled again.
pub fn rewrite_unsent(store: &Store, id: u32, html: &str) -> Result<(), String> {
    store
        .with(|c| c.execute("UPDATE relay_outbox SET html = ?2, buttons = '[]', kind = kind || '-settled' WHERE id = ?1 AND sent_at IS NULL", params![id, html]))
        .map(|_| ())
}

/// Where a run's or session's messages of a kind went, since a time.
pub fn sent_messages(store: &Store, subject: Subject, kind: &str, since: i64) -> Result<Vec<(i64, i64)>, String> {
    let sql = format!("SELECT chat_id, message_id FROM relay_messages WHERE {} = ?1 AND kind = ?2 AND sent_at >= ?3", subject.column());
    store.with(|c| {
        let mut stmt = c.prepare(&sql)?;
        stmt.query_map(params![subject.id(), kind, since], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
    })
}

/// Marks a run's or session's messages of a kind as settled (`<kind>-settled`), so they aren't
/// edited again.
pub fn settle_messages(store: &Store, subject: Subject, kind: &str) -> Result<(), String> {
    let sql = format!("UPDATE relay_messages SET kind = ?2 || '-settled' WHERE {} = ?1 AND kind = ?2", subject.column());
    store.with(|c| c.execute(&sql, params![subject.id(), kind])).map(|_| ())
}

pub fn record_message(store: &Store, chat: i64, message: i64, kind: &str, about: About) -> Result<(), String> {
    store
        .with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO relay_messages (chat_id, message_id, kind, run_id, question_id, session_id, ask_id, sent_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![chat, message, kind, about.run_id, about.question_id, about.session_id, about.ask_id, now_ms()],
            )
        })
        .map(|_| ())
}

/// What a sent message was about, and its kind.
pub fn message_about(store: &Store, chat: i64, message: i64) -> Result<Option<(String, About)>, String> {
    store.with(|c| {
        c.query_row(
            "SELECT kind, run_id, question_id, session_id, ask_id FROM relay_messages WHERE chat_id = ?1 AND message_id = ?2",
            params![chat, message],
            |r| Ok((r.get(0)?, About { run_id: r.get(1)?, question_id: r.get(2)?, session_id: r.get(3)?, ask_id: r.get(4)? })),
        )
        .optional()
    })
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
