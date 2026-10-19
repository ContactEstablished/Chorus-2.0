//! The vault's rows: DPAPI blobs, never plaintext (ARCHITECTURE §8). Sealing and unsealing is in
//! `crate::vault`.

use rusqlite::{OptionalExtension, params};

use super::{Store, now_ms};

#[derive(Debug, Clone)]
pub struct CredentialRow {
    pub id: u32,
    pub label: String,
    pub fingerprint: String,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

pub fn insert(store: &Store, label: &str, blob: &[u8], fingerprint: &str) -> Result<u32, String> {
    store
        .with(|c| {
            c.execute(
                "INSERT INTO credentials (label, blob, fingerprint, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![label, blob, fingerprint, now_ms()],
            )?;
            Ok(c.last_insert_rowid() as u32)
        })
        .map_err(|e| if e.contains("UNIQUE") { format!("a key labelled {label:?} already exists") } else { e })
}

pub fn list(store: &Store) -> Result<Vec<CredentialRow>, String> {
    store.with(|c| {
        let mut stmt =
            c.prepare("SELECT id, label, fingerprint, created_at, last_used_at FROM credentials ORDER BY label COLLATE NOCASE")?;
        stmt.query_map([], |r| {
            Ok(CredentialRow {
                id: r.get(0)?,
                label: r.get(1)?,
                fingerprint: r.get(2)?,
                created_at: r.get(3)?,
                last_used_at: r.get(4)?,
            })
        })?
        .collect()
    })
}

pub fn get(store: &Store, id: u32) -> Result<Option<CredentialRow>, String> {
    Ok(list(store)?.into_iter().find(|c| c.id == id))
}

pub fn blob(store: &Store, id: u32) -> Result<Option<Vec<u8>>, String> {
    store.with(|c| c.query_row("SELECT blob FROM credentials WHERE id = ?1", [id], |r| r.get(0)).optional())
}

/// Every sealed key, so each session's scrubber can redact all of them (ARCHITECTURE §8).
pub fn all_blobs(store: &Store) -> Result<Vec<(u32, Vec<u8>)>, String> {
    store.with(|c| {
        let mut stmt = c.prepare("SELECT id, blob FROM credentials ORDER BY id")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
    })
}

pub fn touch(store: &Store, id: u32) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE credentials SET last_used_at = ?1 WHERE id = ?2", params![now_ms(), id]).map(|_| ()))
}

/// Deletes a key. Sessions that used it keep running; their next launch reports the key missing.
pub fn delete(store: &Store, id: u32) -> Result<bool, String> {
    store.with(|c| c.execute("DELETE FROM credentials WHERE id = ?1", [id]).map(|n| n > 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_list_and_delete() {
        let store = Store::in_memory();
        let id = insert(&store, "OpenRouter", b"sealed", "abcd").unwrap();
        assert!(insert(&store, "OpenRouter", b"x", "y").unwrap_err().contains("already exists"));
        let rows = list(&store).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "OpenRouter");
        assert_eq!(blob(&store, id).unwrap().as_deref(), Some(b"sealed".as_slice()));
        touch(&store, id).unwrap();
        assert!(get(&store, id).unwrap().unwrap().last_used_at.is_some());
        assert!(delete(&store, id).unwrap());
        assert!(!delete(&store, id).unwrap());
        assert!(all_blobs(&store).unwrap().is_empty());
    }
}
