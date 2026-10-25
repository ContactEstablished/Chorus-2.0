//! The Telegram relay (P13's design, built inside P15 for the loop, D97): everything about it that
//! can be decided from inputs. Updates from `getUpdates` are parsed here, checked against the
//! paired users, and turned into commands, answers or nothing; pairing codes, callback tokens and the
//! send rate are small state machines with time passed in. Bot API 10.3 (2026-08-24).
//!
//! Only a paired user in their own private chat with the bot is heard. A bot, a group, an edited
//! message or a forwarded one is never acted on (P13 §7).

pub mod render;
pub mod sessions;

use serde_json::Value;

/// Telegram ids can pass 32 bits.
pub type TgId = i64;

/// One update, reduced to what the relay acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    Message(Message),
    Callback(Callback),
    /// An edit, a channel post, a reaction…: confirmed and ignored.
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub message_id: TgId,
    pub chat_id: TgId,
    pub private: bool,
    pub from_id: TgId,
    pub from_bot: bool,
    pub from_name: String,
    /// Unix seconds.
    pub date: i64,
    pub text: String,
    /// The bot's message this one replies to.
    pub reply_to: Option<TgId>,
    pub forwarded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Callback {
    pub id: String,
    pub from_id: TgId,
    pub chat_id: Option<TgId>,
    pub message_id: Option<TgId>,
    pub data: String,
}

/// The update types the relay asks for (`allowed_updates`).
pub const ALLOWED_UPDATES: &[&str] = &["message", "edited_message", "callback_query"];

/// `getUpdates`' answer: each update's id and what it is. `Err` when Telegram said no, with its
/// description and `retry_after` when it gave one.
pub fn parse_updates(json: &str) -> Result<Vec<(TgId, Incoming)>, ApiError> {
    let v: Value = serde_json::from_str(json).map_err(|_| ApiError { code: 0, description: "Telegram's answer wasn't JSON.".into(), retry_after: None })?;
    if v.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(api_error(&v));
    }
    let updates = v.get("result").and_then(Value::as_array).cloned().unwrap_or_default();
    Ok(updates
        .iter()
        .filter_map(|u| {
            let id = u.get("update_id").and_then(Value::as_i64)?;
            Some((id, incoming(u)))
        })
        .collect())
}

/// A failed Bot API call, as Telegram described it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub code: i64,
    pub description: String,
    pub retry_after: Option<u64>,
}

pub fn api_error(v: &Value) -> ApiError {
    ApiError {
        code: v.get("error_code").and_then(Value::as_i64).unwrap_or(0),
        description: v.get("description").and_then(Value::as_str).unwrap_or("no reason given").chars().take(300).collect(),
        retry_after: v.pointer("/parameters/retry_after").and_then(Value::as_u64),
    }
}

/// A Bot API answer's `result`, or its error.
pub fn result(json: &str) -> Result<Value, ApiError> {
    let v: Value = serde_json::from_str(json).map_err(|_| ApiError { code: 0, description: "Telegram's answer wasn't JSON.".into(), retry_after: None })?;
    if v.get("ok").and_then(Value::as_bool) == Some(true) { Ok(v.get("result").cloned().unwrap_or(Value::Null)) } else { Err(api_error(&v)) }
}

fn incoming(u: &Value) -> Incoming {
    if let Some(m) = u.get("message") {
        return message(m).map_or(Incoming::Ignored, Incoming::Message);
    }
    if let Some(q) = u.get("callback_query") {
        let get = |p: &str| q.pointer(p).and_then(Value::as_i64);
        return match (q.get("id").and_then(Value::as_str), get("/from/id")) {
            (Some(id), Some(from_id)) => Incoming::Callback(Callback {
                id: id.to_owned(),
                from_id,
                chat_id: get("/message/chat/id"),
                message_id: get("/message/message_id"),
                data: q.get("data").and_then(Value::as_str).unwrap_or("").to_owned(),
            }),
            _ => Incoming::Ignored,
        };
    }
    Incoming::Ignored
}

fn message(m: &Value) -> Option<Message> {
    let get = |p: &str| m.pointer(p).and_then(Value::as_i64);
    let name = [m.pointer("/from/first_name"), m.pointer("/from/last_name")]
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(" ");
    Some(Message {
        message_id: get("/message_id")?,
        chat_id: get("/chat/id")?,
        private: m.pointer("/chat/type").and_then(Value::as_str) == Some("private"),
        from_id: get("/from/id")?,
        from_bot: m.pointer("/from/is_bot").and_then(Value::as_bool).unwrap_or(false),
        from_name: name.chars().take(64).collect(),
        date: get("/date").unwrap_or(0),
        text: m.get("text").and_then(Value::as_str).unwrap_or("").to_owned(),
        reply_to: get("/reply_to_message/message_id"),
        forwarded: m.get("forward_origin").is_some() || m.get("forward_date").is_some(),
    })
}

/// Whether a message may be acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admit {
    Accept,
    NotPaired,
    NotPrivate,
    FromBot,
    Forwarded,
}

/// Only a paired user, in their own private chat with the bot (whose id is theirs), is heard.
pub fn admit(m: &Message, paired: &[TgId]) -> Admit {
    if m.from_bot {
        Admit::FromBot
    } else if !m.private || m.chat_id != m.from_id {
        Admit::NotPrivate
    } else if m.forwarded {
        Admit::Forwarded
    } else if paired.contains(&m.from_id) {
        Admit::Accept
    } else {
        Admit::NotPaired
    }
}

/// A button press is heard only from a paired user, on a message in their own chat.
pub fn admit_callback(c: &Callback, paired: &[TgId]) -> bool {
    paired.contains(&c.from_id) && c.chat_id == Some(c.from_id)
}

pub const PAIR_MINUTES: i64 = 10;
pub const PAIR_TRIES: u32 = 5;

/// A pairing code shown in Chorus and sent from the phone as `/pair 123456`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pairing {
    pub code: String,
    pub expires_ms: i64,
    pub wrong: u32,
}

impl Pairing {
    /// Six digits from four random bytes, valid for [`PAIR_MINUTES`].
    pub fn new(random: [u8; 4], now_ms: i64) -> Self {
        Self { code: format!("{:06}", u32::from_le_bytes(random) % 1_000_000), expires_ms: now_ms + PAIR_MINUTES * 60_000, wrong: 0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairOutcome {
    Paired,
    Wrong { left: u32 },
    Expired,
    /// Too many wrong codes: this code is dead; make a new one in Chorus.
    LockedOut,
    /// No code is waiting.
    NoPairing,
}

/// Checks a code against the waiting pairing, which ends when it is used, expires or locks out.
pub fn try_pair(p: &mut Option<Pairing>, given: &str, now_ms: i64) -> PairOutcome {
    let Some(pairing) = p.as_mut() else { return PairOutcome::NoPairing };
    if now_ms > pairing.expires_ms {
        *p = None;
        return PairOutcome::Expired;
    }
    if given.trim() == pairing.code {
        *p = None;
        return PairOutcome::Paired;
    }
    pairing.wrong += 1;
    if pairing.wrong >= PAIR_TRIES {
        *p = None;
        return PairOutcome::LockedOut;
    }
    PairOutcome::Wrong { left: PAIR_TRIES - pairing.wrong }
}

/// What a message starting with `/` asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Pair(String),
    Start,
    Help,
    Status,
    Pause,
    Resume,
    Approve,
    /// `/stop`, or `/stop P03` for a phase.
    Stop(Option<u32>),
    /// Open sessions, numbered.
    Sessions,
    /// `/use 2`, `/use api` or `/use off`: where plain messages go; bare, which one it is.
    Use(String),
    Digest,
    Unknown(String),
}

/// The bot's command menu (`setMyCommands`).
pub const MENU: &[(&str, &str)] = &[
    ("status", "What the loop is doing, and open questions"),
    ("pause", "Pause the run at the next safe point"),
    ("resume", "Resume a paused run"),
    ("approve", "Approve the milestone the run waits on"),
    ("stop", "Stop the run (or /stop P03)"),
    ("sessions", "Open sessions, numbered"),
    ("use", "Send plain messages to a session: /use 2"),
    ("digest", "What needs you, and what finished or failed today"),
    ("help", "What this bot does"),
];

pub fn parse_command(text: &str) -> Option<Command> {
    let t = text.trim();
    let rest = t.strip_prefix('/')?;
    let (head, arg) = rest.split_once(char::is_whitespace).map_or((rest, ""), |(h, a)| (h, a.trim()));
    // "/status@ChorusBot" in a client that adds the bot's name.
    let name = head.split('@').next().unwrap_or(head).to_ascii_lowercase();
    Some(match name.as_str() {
        "pair" => Command::Pair(arg.to_owned()),
        "start" => Command::Start,
        "help" => Command::Help,
        "status" => Command::Status,
        "pause" => Command::Pause,
        "resume" => Command::Resume,
        "approve" => Command::Approve,
        "sessions" => Command::Sessions,
        "use" => Command::Use(arg.chars().take(80).collect()),
        "digest" => Command::Digest,
        "stop" => {
            let n = arg.trim_start_matches(['P', 'p']).parse().ok();
            Command::Stop(n)
        }
        other => Command::Unknown(other.chars().take(32).collect()),
    })
}

pub const MAX_INBOUND: usize = 4000;

/// Text from the phone, made safe to keep and pass on: control characters and terminal escape
/// sequences out, at most [`MAX_INBOUND`] characters.
pub fn sanitize(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => {
                // ESC [ … final, ESC ] … BEL/ST, or ESC and one character.
                match chars.next() {
                    Some('[') => {
                        for d in chars.by_ref() {
                            if ('@'..='~').contains(&d) {
                                break;
                            }
                        }
                    }
                    Some(']') => {
                        while let Some(d) = chars.next() {
                            if d == '\u{7}' || (d == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
            '\n' | '\t' => out.push(c),
            '\r' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out.trim().chars().take(MAX_INBOUND).collect()
}

/// `callback_data` is `v1:<token>`; the token maps, in Chorus, to what the button does. Never text.
pub fn callback_data(token: &str) -> String {
    format!("v1:{token}")
}

pub fn callback_token(data: &str) -> Option<&str> {
    data.strip_prefix("v1:").filter(|t| !t.is_empty() && t.len() <= 32 && t.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// A token from random bytes: lower-case letters and digits, 12 of them.
pub fn token_from(random: &[u8]) -> String {
    const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
    random.iter().take(12).map(|b| ALPHABET[usize::from(*b) % ALPHABET.len()] as char).collect()
}

/// About one message a second to a chat, with a small burst (the Bot API's guidance).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bucket {
    pub tokens: f64,
    pub last_ms: i64,
}

impl Bucket {
    const RATE_PER_MS: f64 = 1.0 / 1000.0;
    const BURST: f64 = 3.0;

    pub fn new(now_ms: i64) -> Self {
        Self { tokens: Self::BURST, last_ms: now_ms }
    }

    /// Takes a send, or says how long to wait for one.
    pub fn take(&mut self, now_ms: i64) -> Result<(), u64> {
        let elapsed = (now_ms - self.last_ms).max(0) as f64;
        self.tokens = (self.tokens + elapsed * Self::RATE_PER_MS).min(Self::BURST);
        self.last_ms = now_ms;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(())
        } else {
            Err(((1.0 - self.tokens) / Self::RATE_PER_MS).ceil() as u64)
        }
    }
}

/// A bot token's shape, `<digits>:<35 characters>`, for checking what was pasted (never shown).
pub fn token_shape_ok(token: &str) -> bool {
    let Some((id, secret)) = token.trim().split_once(':') else { return false };
    (6..=12).contains(&id.len())
        && id.chars().all(|c| c.is_ascii_digit())
        && (30..=50).contains(&secret.len())
        && secret.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(text: &str, from: i64, chat: i64, kind: &str) -> String {
        format!(
            r#"{{"ok":true,"result":[{{"update_id":7,"message":{{"message_id":42,"date":1792900000,"chat":{{"id":{chat},"type":"{kind}"}},"from":{{"id":{from},"is_bot":false,"first_name":"Mat"}},"text":{}}}}}]}}"#,
            serde_json::to_string(text).unwrap()
        )
    }

    fn msg(json: &str) -> Message {
        match parse_updates(json).unwrap().pop().unwrap().1 {
            Incoming::Message(m) => m,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn messages_are_parsed_and_only_paired_dms_are_heard() {
        let m = msg(&update("hello", 5_000_000_001, 5_000_000_001, "private"));
        assert_eq!((m.message_id, m.from_id, m.text.as_str(), m.from_name.as_str(), m.private), (42, 5_000_000_001, "hello", "Mat", true));
        assert_eq!(admit(&m, &[5_000_000_001]), Admit::Accept);
        assert_eq!(admit(&m, &[1]), Admit::NotPaired);
        // A forged chat id, a group, a bot, a forward.
        assert_eq!(admit(&msg(&update("x", 5, 6, "private")), &[5]), Admit::NotPrivate);
        assert_eq!(admit(&msg(&update("x", 5, -100, "group")), &[5]), Admit::NotPrivate);
        let mut bot = m.clone();
        bot.from_bot = true;
        assert_eq!(admit(&bot, &[5_000_000_001]), Admit::FromBot);
        let mut fwd = m;
        fwd.forwarded = true;
        assert_eq!(admit(&fwd, &[5_000_000_001]), Admit::Forwarded);
    }

    #[test]
    fn replies_callbacks_and_edits() {
        let reply = r#"{"ok":true,"result":[{"update_id":8,"message":{"message_id":43,"date":1,"chat":{"id":5,"type":"private"},"from":{"id":5,"is_bot":false,"first_name":"M"},"text":"cursor","reply_to_message":{"message_id":40,"date":1,"chat":{"id":5,"type":"private"}}}}]}"#;
        assert_eq!(msg(reply).reply_to, Some(40));
        let cb = r#"{"ok":true,"result":[{"update_id":9,"callback_query":{"id":"abc","from":{"id":5,"is_bot":false,"first_name":"M"},"chat_instance":"x","data":"v1:k3y","message":{"message_id":40,"date":1,"chat":{"id":5,"type":"private"}}}}]}"#;
        let Incoming::Callback(c) = parse_updates(cb).unwrap().pop().unwrap().1 else { panic!() };
        assert_eq!((c.id.as_str(), c.message_id, callback_token(&c.data)), ("abc", Some(40), Some("k3y")));
        assert!(admit_callback(&c, &[5]) && !admit_callback(&c, &[6]));
        let edit = r#"{"ok":true,"result":[{"update_id":10,"edited_message":{"message_id":1}}]}"#;
        assert_eq!(parse_updates(edit).unwrap(), vec![(10, Incoming::Ignored)]);
    }

    #[test]
    fn errors_carry_retry_after() {
        let e = parse_updates(r#"{"ok":false,"error_code":429,"description":"Too Many Requests: retry after 7","parameters":{"retry_after":7}}"#).unwrap_err();
        assert_eq!((e.code, e.retry_after), (429, Some(7)));
        let e = parse_updates(r#"{"ok":false,"error_code":409,"description":"Conflict: terminated by other getUpdates request"}"#).unwrap_err();
        assert_eq!(e.code, 409);
        assert!(parse_updates("<html>").is_err());
    }

    #[test]
    fn pairing_expires_and_locks_out() {
        let mut p = Some(Pairing::new([1, 2, 3, 4], 0));
        let code = p.as_ref().unwrap().code.clone();
        assert_eq!(code.len(), 6);
        assert_eq!(try_pair(&mut p, "000000", 1000), PairOutcome::Wrong { left: 4 });
        assert_eq!(try_pair(&mut p, &code, 2000), PairOutcome::Paired);
        assert_eq!(try_pair(&mut p, &code, 3000), PairOutcome::NoPairing, "single use");
        let mut p = Some(Pairing::new([9, 9, 9, 9], 0));
        assert_eq!(try_pair(&mut p, "x", PAIR_MINUTES * 60_000 + 1), PairOutcome::Expired);
        let mut p = Some(Pairing::new([9, 9, 9, 9], 0));
        for _ in 0..4 {
            assert!(matches!(try_pair(&mut p, "nope", 1), PairOutcome::Wrong { .. }));
        }
        assert_eq!(try_pair(&mut p, "nope", 1), PairOutcome::LockedOut);
        assert!(p.is_none());
    }

    #[test]
    fn commands() {
        assert_eq!(parse_command("/pair 123456"), Some(Command::Pair("123456".into())));
        assert_eq!(parse_command("/status@ChorusDevBot"), Some(Command::Status));
        assert_eq!(parse_command("/stop P03"), Some(Command::Stop(Some(3))));
        assert_eq!(parse_command("/stop"), Some(Command::Stop(None)));
        assert_eq!(parse_command("/frob"), Some(Command::Unknown("frob".into())));
        assert_eq!(parse_command("cursor please"), None);
        assert_eq!(parse_command("/use 2"), Some(Command::Use("2".into())));
        assert_eq!(parse_command("/use"), Some(Command::Use(String::new())));
        assert_eq!(parse_command("/sessions"), Some(Command::Sessions));
        assert_eq!(parse_command("/digest"), Some(Command::Digest));
    }

    #[test]
    fn inbound_text_is_sanitized_and_capped() {
        assert_eq!(sanitize("a\u{1b}[31mred\u{1b}[0m\r\nb\u{7}c \u{1b}]0;t\u{7}d"), "ared\nbc d");
        assert_eq!(sanitize(&"x".repeat(4001)).chars().count(), MAX_INBOUND);
    }

    #[test]
    fn callback_tokens() {
        let t = token_from(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 255, 99]);
        assert_eq!(t.len(), 12);
        assert_eq!(callback_token(&callback_data(&t)), Some(t.as_str()));
        assert!(callback_data(&t).len() <= 64);
        assert_eq!(callback_token("v1:"), None);
        assert_eq!(callback_token("v1:has space"), None);
        assert_eq!(callback_token("v2:abc"), None);
    }

    #[test]
    fn the_bucket_paces_sends() {
        let mut b = Bucket::new(0);
        assert!(b.take(0).is_ok() && b.take(0).is_ok() && b.take(0).is_ok());
        let wait = b.take(0).unwrap_err();
        assert!((900..=1000).contains(&wait), "{wait}");
        assert!(b.take(1000).is_ok());
    }

    #[test]
    fn token_shapes() {
        let fake = format!("123456789:{}", "A".repeat(35));
        assert!(token_shape_ok(&fake));
        assert!(!token_shape_ok("123:short"));
        assert!(!token_shape_ok("not a token"));
    }
}

#[cfg(test)]
mod token_pattern {
    use crate::scrub::{PLACEHOLDER, builtin_patterns, scrub_all};

    #[test]
    fn a_bot_token_is_scrubbed_by_shape() {
        // Built at run time, so the repository's own secret scan never sees a token-shaped string.
        let token = format!("{}:{}{}", "1234567890", "AA", "x9_-".repeat(9).chars().take(33).collect::<String>());
        let out = String::from_utf8(scrub_all(format!("token={token} end").as_bytes(), &[], &builtin_patterns())).unwrap();
        assert!(!out.contains(&token[3..]), "{out}");
        assert!(out.contains(std::str::from_utf8(PLACEHOLDER).unwrap()));
        // Ordinary text with a colon survives.
        let plain = "at 12:30 the build 20261025:done";
        assert_eq!(String::from_utf8(scrub_all(plain.as_bytes(), &[], &builtin_patterns())).unwrap(), plain);
    }
}
