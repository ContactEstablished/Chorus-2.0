//! Routing records (migration 0010): JSON in `chorus_core::routing`'s shapes, one row per model and
//! kind, and the account checks per model and provider. The `routing` service reads and writes them.

use rusqlite::{OptionalExtension, params};

use super::{Store, now_ms};

/// What a `routing_data` row holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Snapshot,
    Observations,
    Cache,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Snapshot => "snapshot",
            Self::Observations => "observations",
            Self::Cache => "cache",
        }
    }
}

pub fn get(store: &Store, model: &str, kind: Kind) -> Result<Option<String>, String> {
    store.with(|c| {
        c.query_row("SELECT data FROM routing_data WHERE model = ?1 AND kind = ?2", params![model, kind.as_str()], |r| r.get(0))
            .optional()
    })
}

pub fn set(store: &Store, model: &str, kind: Kind, data: &str) -> Result<(), String> {
    store.with(|c| {
        c.execute(
            "INSERT INTO routing_data (model, kind, data, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (model, kind) DO UPDATE SET data = excluded.data, updated_at = excluded.updated_at",
            params![model, kind.as_str(), data, now_ms()],
        )
        .map(|_| ())
    })
}

/// Every model with an endpoint list stored, in slug order: what the observer keeps up to date.
pub fn models_with_snapshot(store: &Store) -> Result<Vec<String>, String> {
    store.with(|c| {
        let mut stmt = c.prepare("SELECT model FROM routing_data WHERE kind = 'snapshot' ORDER BY model")?;
        stmt.query_map([], |r| r.get(0))?.collect()
    })
}

pub fn get_account(store: &Store, model: &str, provider_id: u32) -> Result<Option<String>, String> {
    store.with(|c| {
        c.query_row("SELECT data FROM routing_accounts WHERE model = ?1 AND provider_id = ?2", params![model, provider_id], |r| r.get(0))
            .optional()
    })
}

pub fn set_account(store: &Store, model: &str, provider_id: u32, data: &str) -> Result<(), String> {
    store.with(|c| {
        c.execute(
            "INSERT INTO routing_accounts (model, provider_id, data) VALUES (?1, ?2, ?3)
             ON CONFLICT (model, provider_id) DO UPDATE SET data = excluded.data",
            params![model, provider_id, data],
        )
        .map(|_| ())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_are_kept_per_model_and_kind() {
        let s = Store::in_memory();
        assert_eq!(get(&s, "a/b", Kind::Snapshot).unwrap(), None);
        set(&s, "a/b", Kind::Snapshot, "{\"x\":1}").unwrap();
        set(&s, "a/b", Kind::Snapshot, "{\"x\":2}").unwrap();
        set(&s, "a/b", Kind::Cache, "{}").unwrap();
        set(&s, "c/d", Kind::Observations, "[]").unwrap();
        assert_eq!(get(&s, "a/b", Kind::Snapshot).unwrap().as_deref(), Some("{\"x\":2}"));
        assert_eq!(models_with_snapshot(&s).unwrap(), ["a/b"]);
    }
}
