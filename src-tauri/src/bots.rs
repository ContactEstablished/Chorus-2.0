//! Bots (P8, decision 1): the launch profiles, grown. A Bot is an agent with its options, provider
//! and key-as-variable (as a profile was), plus a description, instructions, tags, skill folders
//! and a preset. Starting one creates an ordinary session, which copies the Bot's instructions and
//! skills at create, so editing the Bot never changes it.

use std::path::Path;
use std::sync::Arc;

use chorus_core::agent::{AgentKind, LaunchOptions};
use chorus_core::instructions::MAX_SKILLS;
use chorus_core::launch::{Preset, clean_description};
use chorus_core::scrub::{builtin_patterns, scrub_all};
use chorus_core::session::clean_name;
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::sessions::{CredentialUse, SessionManager};
use crate::store::{Store, now_ms, providers as provider_rows};
use crate::vault;
use crate::vault::commands::secs;

type Manager<'a> = State<'a, Arc<SessionManager>>;

pub const MAX_INSTRUCTIONS_BYTES: usize = chorus_core::instructions::MAX_BOT_INSTRUCTION_BYTES;
const MAX_SKILL_PATH_CHARS: usize = 1024;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Bot {
    pub id: u32,
    pub name: String,
    pub agent: AgentKind,
    pub options: LaunchOptions,
    pub provider_id: Option<u32>,
    pub provider_name: Option<String>,
    pub credential: Option<CredentialUse>,
    pub description: Option<String>,
    pub instructions: String,
    pub tags: Vec<String>,
    pub skills: Vec<String>,
    pub preset: Option<Preset>,
    pub archived: bool,
    /// Sessions started from it that aren't closed, and all of them.
    pub open_sessions: u32,
    pub sessions: u32,
    /// Automations that run it.
    pub automations: u32,
    pub updated_at_secs: u32,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BotSave {
    /// None adds one.
    pub id: Option<u32>,
    pub name: String,
    pub agent: AgentKind,
    pub options: LaunchOptions,
    pub provider_id: Option<u32>,
    pub credential: Option<CredentialUse>,
    pub description: Option<String>,
    pub instructions: String,
    pub tags: Vec<String>,
    pub skills: Vec<String>,
    pub preset: Option<Preset>,
}

/// What a session copies from its Bot at create (`sessions.bot_snapshot`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Snapshot {
    pub name: String,
    pub instructions: String,
    pub skills: Vec<String>,
}

const COLUMNS: &str = "b.id, b.name, b.agent, b.options, b.provider_id, b.credential_id, b.credential_env, b.description,
    b.instructions, b.tags, b.skills, b.preset, b.archived_at, b.updated_at,
    (SELECT COUNT(*) FROM sessions s WHERE s.bot_id = b.id AND s.state != 'closed'),
    (SELECT COUNT(*) FROM sessions s WHERE s.bot_id = b.id),
    (SELECT COUNT(*) FROM automations a WHERE a.bot_id = b.id)";

fn preset_name(p: Preset) -> &'static str {
    match p {
        Preset::Solo => "solo",
        Preset::Pair => "pair",
        Preset::Workbench => "workbench",
        Preset::Swarm => "swarm",
    }
}

fn bot_from(r: &Row<'_>) -> rusqlite::Result<(Bot, Option<u32>)> {
    let bad = |i: usize, v: String| rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, v.into());
    let agent: String = r.get(2)?;
    let options: String = r.get(3)?;
    let tags: String = r.get(9)?;
    let skills: String = r.get(10)?;
    let preset: Option<String> = r.get(11)?;
    let credential_id: Option<u32> = r.get(5)?;
    let credential_env: Option<String> = r.get(6)?;
    let archived_at: Option<i64> = r.get(12)?;
    Ok((
        Bot {
            id: r.get(0)?,
            name: r.get(1)?,
            agent: AgentKind::parse(&agent).ok_or_else(|| bad(2, format!("unknown agent {agent:?}")))?,
            options: serde_json::from_str(&options).map_err(|_| bad(3, "unreadable options".into()))?,
            provider_id: r.get(4)?,
            provider_name: None,
            credential: match (credential_id, credential_env) {
                (Some(id), Some(env_var)) => Some(CredentialUse { id, env_var }),
                _ => None,
            },
            description: r.get(7)?,
            instructions: r.get(8)?,
            tags: serde_json::from_str(&tags).unwrap_or_default(),
            skills: serde_json::from_str(&skills).unwrap_or_default(),
            preset: preset.as_deref().and_then(|p| serde_json::from_value(serde_json::Value::String(p.to_owned())).ok()),
            archived: archived_at.is_some(),
            updated_at_secs: secs(r.get(13)?),
            open_sessions: r.get(14)?,
            sessions: r.get(15)?,
            automations: r.get(16)?,
        },
        r.get(4)?,
    ))
}

fn with_provider(store: &Store, (mut bot, provider): (Bot, Option<u32>)) -> Bot {
    bot.provider_name = provider.map(|id| {
        provider_rows::get(store, id).ok().flatten().map_or_else(|| "(deleted provider)".to_owned(), |p| p.name)
    });
    bot
}

pub fn list(store: &Store) -> Result<Vec<Bot>, String> {
    let rows = store.with(|c| {
        let mut stmt = c.prepare(&format!("SELECT {COLUMNS} FROM bots b ORDER BY b.archived_at IS NOT NULL, b.name"))?;
        stmt.query_map([], bot_from)?.collect::<rusqlite::Result<Vec<_>>>()
    })?;
    Ok(rows.into_iter().map(|r| with_provider(store, r)).collect())
}

pub fn get(store: &Store, id: u32) -> Result<Option<Bot>, String> {
    let row = store.with(|c| c.query_row(&format!("SELECT {COLUMNS} FROM bots b WHERE b.id = ?1"), [id], bot_from).optional())?;
    Ok(row.map(|r| with_provider(store, r)))
}

/// Text a user typed that Chorus will keep and pass to agents: refused if it holds a stored key or
/// something shaped like one, so no key reaches the database, a file or argv this way.
pub fn refuse_secrets(store: &Store, what: &str, text: &str) -> Result<(), String> {
    let (keys, _) = vault::all_for_scrubbing(store);
    if scrub_all(text.as_bytes(), &keys, &builtin_patterns()) != text.as_bytes() {
        return Err(format!("That looks like a key in {what}. Chorus doesn't keep keys there: remove it, or store it under Settings → Keys."));
    }
    Ok(())
}

fn check_skills(skills: &[String]) -> Result<Vec<String>, String> {
    if skills.len() > MAX_SKILLS {
        return Err(format!("A Bot takes up to {MAX_SKILLS} skill folders."));
    }
    let mut out: Vec<String> = Vec::new();
    for s in skills.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        if s.chars().count() > MAX_SKILL_PATH_CHARS || s.chars().any(char::is_control) {
            return Err("A skill folder's path is too long or has control characters.".into());
        }
        let p = Path::new(s);
        if !p.is_absolute() || !p.is_dir() {
            return Err(format!("{s} isn't a folder. A skill is a folder holding SKILL.md."));
        }
        if !p.join("SKILL.md").is_file() {
            return Err(format!("{s} has no SKILL.md."));
        }
        let s = chorus_core::folders::trim_trailing_separators(s);
        if !out.contains(&s) {
            out.push(s);
        }
    }
    Ok(out)
}

pub fn save(m: &SessionManager, input: &BotSave) -> Result<Bot, String> {
    if input.name.trim().is_empty() {
        return Err("A Bot needs a name.".into());
    }
    let name = clean_name(Some(&input.name), "")?;
    // Stored as chosen, not with defaults filled in, so a later default change still reaches it.
    m.check_settings(input.agent, &input.options, input.provider_id, input.credential.as_ref())?;
    let description = clean_description(input.description.as_deref())?;
    let instructions = input.instructions.trim().replace("\r\n", "\n");
    if instructions.len() > MAX_INSTRUCTIONS_BYTES {
        return Err(format!("A Bot's instructions are up to {} KiB.", MAX_INSTRUCTIONS_BYTES / 1024));
    }
    if instructions.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return Err("Instructions can't hold control characters.".into());
    }
    refuse_secrets(m.store(), "the instructions", &instructions)?;
    if let Some(d) = &description {
        refuse_secrets(m.store(), "the description", d)?;
    }
    let tags = chorus_core::tags::clean_tags(&input.tags)?;
    let skills = check_skills(&input.skills)?;
    let options = serde_json::to_string(&input.options).map_err(|e| e.to_string())?;
    let tags = serde_json::to_string(&tags).map_err(|e| e.to_string())?;
    let skills = serde_json::to_string(&skills).map_err(|e| e.to_string())?;
    let preset = input.preset.map(preset_name);
    let now = now_ms();
    let unique = |e: String| if e.contains("UNIQUE") { format!("a Bot named {name:?} already exists") } else { e };
    let credential_id = input.credential.as_ref().map(|c| c.id);
    let credential_env = input.credential.as_ref().map(|c| c.env_var.as_str());
    let id = match input.id {
        Some(id) => {
            get(m.store(), id)?.ok_or("That Bot no longer exists.")?;
            m.store()
                .with(|c| {
                    c.execute(
                        "UPDATE bots SET name = ?2, agent = ?3, options = ?4, provider_id = ?5, credential_id = ?6, credential_env = ?7,
                                description = ?8, instructions = ?9, tags = ?10, skills = ?11, preset = ?12, updated_at = ?13
                         WHERE id = ?1",
                        params![id, name, input.agent.as_str(), options, input.provider_id, credential_id, credential_env,
                            description, instructions, tags, skills, preset, now],
                    )
                })
                .map_err(unique)?;
            id
        }
        None => m
            .store()
            .with(|c| {
                c.execute(
                    "INSERT INTO bots (name, agent, options, provider_id, credential_id, credential_env, description, instructions,
                                       tags, skills, preset, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
                    params![name, input.agent.as_str(), options, input.provider_id, credential_id, credential_env,
                        description, instructions, tags, skills, preset, now],
                )?;
                Ok(c.last_insert_rowid() as u32)
            })
            .map_err(unique)?,
    };
    get(m.store(), id)?.ok_or_else(|| "That Bot no longer exists.".into())
}

/// The Bot a new session starts from, checked before the session is made: it exists and isn't
/// archived. Returns what the session copies.
/// A Bot belongs to its own agent: a session of another agent (a Pair's reviewer, or a form whose
/// agent was changed after choosing the Bot) doesn't carry it.
pub fn for_launch(store: &Store, bot_id: Option<u32>, agent: AgentKind) -> Result<Option<(u32, Snapshot)>, String> {
    let Some(id) = bot_id else { return Ok(None) };
    let bot = get(store, id)?.ok_or("That Bot no longer exists.")?;
    if bot.archived {
        return Err(format!("{} is archived. Restore it on the Bots screen to start it.", bot.name));
    }
    if bot.agent != agent {
        return Ok(None);
    }
    Ok(Some((id, Snapshot { name: bot.name, instructions: bot.instructions, skills: bot.skills })))
}

/// Records the Bot on a session just made.
pub fn attach(store: &Store, session_id: u32, bot: Option<&(u32, Snapshot)>) -> Result<(), String> {
    let Some((id, snapshot)) = bot else { return Ok(()) };
    let json = serde_json::to_string(snapshot).map_err(|e| e.to_string())?;
    store.with(|c| c.execute("UPDATE sessions SET bot_id = ?2, bot_snapshot = ?3 WHERE id = ?1", params![session_id, id, json]).map(|_| ()))
}

/// A session's Bot: its id (None once the Bot is deleted) and the copy it was started with.
pub fn of_session(store: &Store, session_id: u32) -> Result<Option<(Option<u32>, Snapshot)>, String> {
    let row: Option<(Option<u32>, Option<String>)> = store
        .with(|c| c.query_row("SELECT bot_id, bot_snapshot FROM sessions WHERE id = ?1", [session_id], |r| Ok((r.get(0)?, r.get(1)?))).optional())?;
    Ok(row.and_then(|(id, json)| json.and_then(|j| serde_json::from_str(&j).ok()).map(|s| (id, s))))
}

#[tauri::command]
#[specta::specta]
pub fn bots_list(state: Manager<'_>) -> Result<Vec<Bot>, String> {
    list(state.store())
}

#[tauri::command]
#[specta::specta]
pub fn bot_save(state: Manager<'_>, input: BotSave) -> Result<Bot, String> {
    save(&state, &input)
}

/// Archives a Bot (hidden from New session, can't be started) or restores it. Its sessions keep
/// running either way.
#[tauri::command]
#[specta::specta]
pub fn bot_set_archived(state: Manager<'_>, id: u32, archived: bool) -> Result<Bot, String> {
    get(state.store(), id)?.ok_or("That Bot no longer exists.")?;
    let at = archived.then(now_ms);
    state.store().with(|c| c.execute("UPDATE bots SET archived_at = ?2 WHERE id = ?1", params![id, at]))?;
    get(state.store(), id)?.ok_or_else(|| "That Bot no longer exists.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_round_trips_and_an_archived_bot_is_refused() {
        let s = Store::in_memory();
        s.with(|c| {
            c.execute(
                "INSERT INTO bots (id, name, agent, options, instructions, skills, created_at, updated_at)
                 VALUES (1, 'Reviewer', 'claude', '{}', 'Review only.', '[]', 1, 1)",
                [],
            )
        })
        .unwrap();
        let (id, snap) = for_launch(&s, Some(1), AgentKind::Claude).unwrap().unwrap();
        assert_eq!((id, snap.instructions.as_str()), (1, "Review only."));
        assert_eq!(for_launch(&s, None, AgentKind::Claude).unwrap(), None);
        // A Claude Bot doesn't come along to a Codex session.
        assert_eq!(for_launch(&s, Some(1), AgentKind::Codex).unwrap(), None);
        assert!(for_launch(&s, Some(9), AgentKind::Claude).is_err());
        s.with(|c| c.execute("UPDATE bots SET archived_at = 5 WHERE id = 1", [])).unwrap();
        assert!(for_launch(&s, Some(1), AgentKind::Claude).unwrap_err().contains("archived"));
        let bot = get(&s, 1).unwrap().unwrap();
        assert!(bot.archived && bot.tags.is_empty() && bot.preset.is_none());
    }

    #[test]
    fn a_planted_key_is_refused() {
        let s = Store::in_memory();
        let fake = format!("sk-or-v1-{}", "a1".repeat(32));
        assert!(refuse_secrets(&s, "the note", &format!("use {fake} here")).unwrap_err().contains("looks like a key"));
        assert!(refuse_secrets(&s, "the note", "no keys here").is_ok());
    }
}
