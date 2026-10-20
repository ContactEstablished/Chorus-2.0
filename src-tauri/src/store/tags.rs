//! Session tags (migration 0009). Tags arrive cleaned (`chorus_core::tags`).

use rusqlite::params;

use super::Store;

/// Replaces a session's tags, keeping their order.
pub fn set(store: &Store, session_id: u32, tags: &[String]) -> Result<(), String> {
    store.with(|c| {
        let tx = c.unchecked_transaction()?;
        tx.execute("DELETE FROM session_tags WHERE session_id = ?1", [session_id])?;
        for (i, t) in tags.iter().enumerate() {
            tx.execute("INSERT INTO session_tags (session_id, tag, position) VALUES (?1, ?2, ?3)", params![session_id, t, i as i64])?;
        }
        tx.commit()
    })
}

pub fn get(store: &Store, session_id: u32) -> Result<Vec<String>, String> {
    store.with(|c| {
        let mut stmt = c.prepare("SELECT tag FROM session_tags WHERE session_id = ?1 ORDER BY position")?;
        stmt.query_map([session_id], |r| r.get(0))?.collect()
    })
}

/// Every tag used, with how many sessions have it: the most used first, then the newest.
pub fn known(store: &Store) -> Result<Vec<(String, u32)>, String> {
    store.with(|c| {
        let mut stmt = c.prepare(
            "SELECT tag, COUNT(*), MAX(session_id) FROM session_tags GROUP BY tag ORDER BY COUNT(*) DESC, MAX(session_id) DESC, tag",
        )?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
    })
}
