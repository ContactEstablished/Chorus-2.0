//! D135: the log of Chorus's messages to OpenMausBot (migration 0031). A row is written, scrubbed,
//! before its message goes out, and finished with what happened.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{Store, now_ms};

/// Rows kept; older ones go as new ones come.
const KEEP: i64 = 500;

/// One message, as Settings → MausBot lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRow {
    pub id: u32,
    pub at_ms: f64,
    pub session_id: Option<u32>,
    pub run_id: Option<u32>,
    /// `ad-hoc`, `done`, `failed`, `needs-you` or `test`.
    pub kind: String,
    pub text: String,
    /// `sending`, `sent` or `skipped`.
    pub outcome: String,
    pub reason: Option<String>,
}

/// Writes a message about to be sent. Returns its id.
pub fn begin(store: &Store, session_id: Option<u32>, run_id: Option<u32>, kind: &str, text: &str) -> Result<i64, String> {
    store.with(|c| {
        c.execute(
            "INSERT INTO maus_updates (at, session_id, run_id, kind, text, outcome) VALUES (?1, ?2, ?3, ?4, ?5, 'sending')",
            params![now_ms(), session_id, run_id, kind, text],
        )?;
        let id = c.last_insert_rowid();
        c.execute("DELETE FROM maus_updates WHERE id <= ?1", [id - KEEP])?;
        Ok(id)
    })
}

/// Records how it went: `reason` is `None` when it was sent.
pub fn finish(store: &Store, id: i64, reason: Option<&str>) -> Result<(), String> {
    let outcome = if reason.is_none() { "sent" } else { "skipped" };
    store.with(|c| c.execute("UPDATE maus_updates SET outcome = ?2, reason = ?3 WHERE id = ?1", params![id, outcome, reason])).map(|_| ())
}

/// A message skipped before it was written (switched off, rate-limited, nothing to scrub with).
pub fn skipped(store: &Store, session_id: Option<u32>, run_id: Option<u32>, kind: &str, text: &str, reason: &str) -> Result<(), String> {
    let id = begin(store, session_id, run_id, kind, text)?;
    finish(store, id, Some(reason))
}

/// The newest `limit` rows, newest first.
pub fn recent(store: &Store, limit: u32) -> Result<Vec<UpdateRow>, String> {
    store.with(|c| {
        let mut q = c.prepare("SELECT id, at, session_id, run_id, kind, text, outcome, reason FROM maus_updates ORDER BY id DESC LIMIT ?1")?;
        let rows = q.query_map([limit], |r| {
            Ok(UpdateRow {
                id: r.get(0)?,
                at_ms: r.get::<_, i64>(1)? as f64,
                session_id: r.get(2)?,
                run_id: r.get(3)?,
                kind: r.get(4)?,
                text: r.get(5)?,
                outcome: r.get(6)?,
                reason: r.get(7)?,
            })
        })?;
        rows.collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_is_logged_before_it_is_sent_and_finished_after() {
        let store = Store::in_memory();
        let id = begin(&store, None, Some(3), "done", "[Chorus] run · done").unwrap();
        assert_eq!(recent(&store, 5).unwrap()[0].outcome, "sending");
        finish(&store, id, None).unwrap();
        skipped(&store, None, None, "test", "x", "OpenMausBot isn't running").unwrap();
        let rows = recent(&store, 5).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].outcome.as_str(), rows[0].reason.as_deref()), ("skipped", Some("OpenMausBot isn't running")));
        assert_eq!((rows[1].outcome.as_str(), rows[1].run_id, rows[1].reason.as_deref()), ("sent", Some(3), None));
    }

    #[test]
    fn old_rows_go() {
        let store = Store::in_memory();
        for i in 0..(KEEP + 3) {
            begin(&store, None, None, "test", &i.to_string()).unwrap();
        }
        let n: i64 = store.with(|c| c.query_row("SELECT COUNT(*) FROM maus_updates", [], |r| r.get(0))).unwrap();
        assert_eq!(n, KEEP);
    }
}
