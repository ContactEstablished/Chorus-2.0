//! Key-value settings. Values are plain text; nothing secret goes here except the agent lock's
//! PIN, which is stored only as a slow salted hash (`chorus_core::lock`).

use rusqlite::{OptionalExtension, params};

use super::Store;

pub fn get(store: &Store, key: &str) -> Result<Option<String>, String> {
    store.with(|c| c.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional())
}

pub fn set(store: &Store, key: &str, value: &str) -> Result<(), String> {
    store.with(|c| {
        c.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .map(|_| ())
    })
}

pub fn delete(store: &Store, key: &str) -> Result<(), String> {
    store.with(|c| c.execute("DELETE FROM settings WHERE key = ?1", [key]).map(|_| ()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_overwrite_delete() {
        let store = Store::in_memory();
        assert_eq!(get(&store, "a").unwrap(), None);
        set(&store, "a", "1").unwrap();
        set(&store, "a", "2").unwrap();
        assert_eq!(get(&store, "a").unwrap().as_deref(), Some("2"));
        delete(&store, "a").unwrap();
        assert_eq!(get(&store, "a").unwrap(), None);
    }
}
