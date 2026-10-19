//! Saved launch profiles (migration 0004): a named agent, options, provider and key-as-variable to
//! start sessions from. They grow into Bots in P8.

use chorus_core::agent::{AgentKind, LaunchOptions};
use rusqlite::{OptionalExtension, Row, params};

use super::{Store, now_ms};

#[derive(Debug, Clone)]
pub struct ProfileRow {
    pub id: u32,
    pub name: String,
    pub agent: AgentKind,
    pub options: LaunchOptions,
    pub provider_id: Option<u32>,
    pub credential_id: Option<u32>,
    pub credential_env: Option<String>,
    pub updated_at: i64,
}

pub struct ProfileInput<'a> {
    pub name: &'a str,
    pub agent: AgentKind,
    pub options: &'a LaunchOptions,
    pub provider_id: Option<u32>,
    pub credential_id: Option<u32>,
    pub credential_env: Option<&'a str>,
}

const COLUMNS: &str = "id, name, agent, options, provider_id, credential_id, credential_env, updated_at";

fn profile_from(r: &Row<'_>) -> rusqlite::Result<ProfileRow> {
    let bad = |i: usize, v: String| rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, v.into());
    let agent: String = r.get(2)?;
    let options: String = r.get(3)?;
    Ok(ProfileRow {
        id: r.get(0)?,
        name: r.get(1)?,
        agent: AgentKind::parse(&agent).ok_or_else(|| bad(2, format!("unknown agent {agent:?}")))?,
        options: serde_json::from_str(&options).map_err(|_| bad(3, format!("unreadable options {options:?}")))?,
        provider_id: r.get(4)?,
        credential_id: r.get(5)?,
        credential_env: r.get(6)?,
        updated_at: r.get(7)?,
    })
}

fn unique(e: String, name: &str) -> String {
    if e.contains("UNIQUE") { format!("a profile named {name:?} already exists") } else { e }
}

pub fn insert(store: &Store, p: &ProfileInput<'_>) -> Result<u32, String> {
    let options = serde_json::to_string(p.options).map_err(|e| e.to_string())?;
    let now = now_ms();
    store
        .with(|c| {
            c.execute(
                "INSERT INTO launch_profiles (name, agent, options, provider_id, credential_id, credential_env, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![p.name, p.agent.as_str(), options, p.provider_id, p.credential_id, p.credential_env, now],
            )?;
            Ok(c.last_insert_rowid() as u32)
        })
        .map_err(|e| unique(e, p.name))
}

pub fn update(store: &Store, id: u32, p: &ProfileInput<'_>) -> Result<(), String> {
    let options = serde_json::to_string(p.options).map_err(|e| e.to_string())?;
    store
        .with(|c| {
            c.execute(
                "UPDATE launch_profiles SET name = ?2, agent = ?3, options = ?4, provider_id = ?5, credential_id = ?6,
                        credential_env = ?7, updated_at = ?8
                 WHERE id = ?1",
                params![id, p.name, p.agent.as_str(), options, p.provider_id, p.credential_id, p.credential_env, now_ms()],
            )
            .map(|_| ())
        })
        .map_err(|e| unique(e, p.name))
}

pub fn get(store: &Store, id: u32) -> Result<Option<ProfileRow>, String> {
    store.with(|c| c.query_row(&format!("SELECT {COLUMNS} FROM launch_profiles WHERE id = ?1"), [id], profile_from).optional())
}

pub fn list(store: &Store) -> Result<Vec<ProfileRow>, String> {
    store.with(|c| {
        let mut stmt = c.prepare(&format!("SELECT {COLUMNS} FROM launch_profiles ORDER BY name"))?;
        stmt.query_map([], profile_from)?.collect()
    })
}

pub fn delete(store: &Store, id: u32) -> Result<bool, String> {
    store.with(|c| c.execute("DELETE FROM launch_profiles WHERE id = ?1", [id]).map(|n| n > 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chorus_core::agent::Effort;

    #[test]
    fn profiles_are_saved_listed_and_deleted() {
        let s = Store::in_memory();
        let options = LaunchOptions { effort: Some(Effort::Deep), model: Some("opus".into()), ..Default::default() };
        let input = ProfileInput {
            name: "Claude deep",
            agent: AgentKind::Claude,
            options: &options,
            provider_id: None,
            credential_id: None,
            credential_env: None,
        };
        let id = insert(&s, &input).unwrap();
        assert!(insert(&s, &input).unwrap_err().contains("already exists"));
        let row = get(&s, id).unwrap().unwrap();
        assert_eq!((row.agent, row.options.effort, row.options.model.as_deref()), (AgentKind::Claude, Some(Effort::Deep), Some("opus")));
        update(&s, id, &ProfileInput { name: "Claude max", ..input }).unwrap();
        assert_eq!(list(&s).unwrap().iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Claude max"]);
        assert!(delete(&s, id).unwrap() && list(&s).unwrap().is_empty());
    }
}
