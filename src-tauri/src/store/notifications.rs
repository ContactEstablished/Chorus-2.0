//! The notification centre's history (P7). Bodies are scrubbed before they get here.

use chorus_core::inbox::Notice;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::Store;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: u32,
    /// The session it's about; none for a project's notice (P18: branches and the baton).
    pub session_id: Option<u32>,
    /// P18: the project a project's notice is about.
    pub project_id: Option<u32>,
    /// `approval`, `input` or `failed`; P18 adds `branch` and `handoff`.
    pub kind: String,
    pub title: String,
    pub body: String,
    /// Unix milliseconds.
    pub created_at: f64,
    pub read_at: Option<f64>,
}

const COLUMNS: &str = "id,session_id,project_id,kind,title,body,created_at,read_at";

fn from_row(r: &rusqlite::Row) -> rusqlite::Result<Notification> {
    Ok(Notification {
        id: r.get(0)?,
        session_id: r.get(1)?,
        project_id: r.get(2)?,
        kind: r.get(3)?,
        title: r.get(4)?,
        body: r.get(5)?,
        created_at: r.get::<_, i64>(6)? as f64,
        read_at: r.get::<_, Option<i64>>(7)?.map(|t| t as f64),
    })
}

/// Stores `notice` unless its episode is already stored; returns the new row, or none.
pub fn insert(store: &Store, session: u32, notice: &Notice, now: i64) -> Result<Option<Notification>, String> {
    store.with(|c| {
        let added = c.execute(
            "INSERT OR IGNORE INTO notifications(session_id,kind,title,body,created_at,dedupe_key) VALUES(?1,?2,?3,?4,?5,?6)",
            params![session, notice.kind, notice.title, notice.body, now, notice.dedupe_key],
        )?;
        if added == 0 {
            return Ok(None);
        }
        c.query_row(&format!("SELECT {COLUMNS} FROM notifications WHERE id=?1"), [c.last_insert_rowid()], from_row)
            .optional()
    })
}

/// P18: a project's notice (a branch unused for 30 days, the baton passing). Stored once per
/// dedupe key, like a session's.
pub fn insert_for_project(store: &Store, project: u32, notice: &Notice, now: i64) -> Result<Option<Notification>, String> {
    store.with(|c| {
        let added = c.execute(
            "INSERT OR IGNORE INTO notifications(project_id,kind,title,body,created_at,dedupe_key) VALUES(?1,?2,?3,?4,?5,?6)",
            params![project, notice.kind, notice.title, notice.body, now, notice.dedupe_key],
        )?;
        if added == 0 {
            return Ok(None);
        }
        c.query_row(&format!("SELECT {COLUMNS} FROM notifications WHERE id=?1"), [c.last_insert_rowid()], from_row)
            .optional()
    })
}

/// P18: marks read every unread notice whose dedupe key starts with `prefix` (one branch's).
pub fn mark_read_by_key_prefix(store: &Store, prefix: &str, now: i64) -> Result<(), String> {
    // A plain prefix comparison: no LIKE, so `_` and `%` in a branch name are only themselves.
    store.with(|c| {
        c.execute(
            "UPDATE notifications SET read_at=?2 WHERE read_at IS NULL AND substr(dedupe_key, 1, length(?1)) = ?1",
            params![prefix, now],
        )
        .map(|_| ())
    })
}

/// Newest first, at most `limit`.
pub fn list(store: &Store, limit: u32) -> Result<Vec<Notification>, String> {
    store.with(|c| {
        let mut q = c.prepare(&format!("SELECT {COLUMNS} FROM notifications ORDER BY created_at DESC, id DESC LIMIT ?1"))?;
        q.query_map([limit], from_row)?.collect()
    })
}

pub fn unread(store: &Store) -> Result<u32, String> {
    store.with(|c| c.query_row("SELECT count(*) FROM notifications WHERE read_at IS NULL", [], |r| r.get(0)))
}

/// Marks one read; false if there is no such notification.
pub fn mark_read(store: &Store, id: u32, now: i64) -> Result<bool, String> {
    store.with(|c| {
        let exists: bool = c.query_row("SELECT count(*) FROM notifications WHERE id=?1", [id], |r| r.get::<_, i64>(0))? > 0;
        c.execute("UPDATE notifications SET read_at=?2 WHERE id=?1 AND read_at IS NULL", params![id, now])?;
        Ok(exists)
    })
}

pub fn mark_all_read(store: &Store, now: i64) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE notifications SET read_at=?1 WHERE read_at IS NULL", [now]).map(|_| ()))
}

/// Deletes notifications created before `before`; returns how many.
pub fn prune(store: &Store, before: i64) -> Result<usize, String> {
    store.with(|c| c.execute("DELETE FROM notifications WHERE created_at < ?1", [before]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chorus_core::attention::Attention;
    use chorus_core::inbox::{NoticeFacts, notice_for};

    fn store_with_session() -> Store {
        let store = Store::in_memory();
        store
            .with(|c| c.execute_batch("INSERT INTO sessions(id,name,agent,cwd,state,cols,rows,created_at) VALUES(1,'Ada','shell','.','running',80,24,1)"))
            .unwrap();
        store
    }

    fn notice(attention: Attention, since: i64) -> Notice {
        notice_for(&NoticeFacts { session: 1, name: "Ada", attention, since_ms: Some(since), fallback_at: 0, now_ms: 0, detail: None }).unwrap()
    }

    #[test]
    fn an_episode_is_stored_once() {
        let store = store_with_session();
        let first = insert(&store, 1, &notice(Attention::Approval, 10), 100).unwrap().unwrap();
        assert_eq!(first.kind, "approval");
        assert!(insert(&store, 1, &notice(Attention::Approval, 10), 200).unwrap().is_none());
        assert!(insert(&store, 1, &notice(Attention::Approval, 11), 300).unwrap().is_some());
        assert_eq!(list(&store, 10).unwrap().len(), 2);
        assert_eq!(unread(&store).unwrap(), 2);
    }

    #[test]
    fn read_marks_and_newest_first() {
        let store = store_with_session();
        let a = insert(&store, 1, &notice(Attention::Approval, 1), 100).unwrap().unwrap();
        insert(&store, 1, &notice(Attention::Failed, 2), 200).unwrap().unwrap();
        assert_eq!(list(&store, 10).unwrap()[0].kind, "failed");
        assert!(mark_read(&store, a.id, 300).unwrap());
        assert!(!mark_read(&store, 999, 300).unwrap());
        assert_eq!(unread(&store).unwrap(), 1);
        mark_all_read(&store, 400).unwrap();
        assert_eq!(unread(&store).unwrap(), 0);
        // Marking again keeps the first time it was read.
        assert_eq!(list(&store, 10).unwrap()[1].read_at, Some(300.0));
    }

    #[test]
    fn prune_drops_old_rows() {
        let store = store_with_session();
        insert(&store, 1, &notice(Attention::Approval, 1), 100).unwrap();
        insert(&store, 1, &notice(Attention::Approval, 2), 1000).unwrap();
        assert_eq!(prune(&store, 500).unwrap(), 1);
        assert_eq!(list(&store, 10).unwrap().len(), 1);
    }

    #[test]
    fn a_projects_notice_needs_no_session_and_is_marked_read_by_its_key() {
        let store = store_with_session();
        store.with(|c| c.execute("INSERT INTO projects(id,name,folder,colour,position,created_at) VALUES(4,'Atlas','D:/p','#336699',0,1)", [])).unwrap();
        let n = Notice { kind: "branch", title: "old unused for 45 days".into(), body: "Atlas: merged into main.".into(), dedupe_key: "branch:4:old_1:123".into() };
        let added = insert_for_project(&store, 4, &n, 100).unwrap().unwrap();
        assert_eq!((added.session_id, added.project_id, added.kind.as_str()), (None, Some(4), "branch"));
        assert!(insert_for_project(&store, 4, &n, 200).unwrap().is_none(), "once per spell");
        let other = Notice { dedupe_key: "branch:4:old_10:5".into(), ..n.clone() };
        insert_for_project(&store, 4, &other, 300).unwrap().unwrap();
        mark_read_by_key_prefix(&store, "branch:4:old_1:", 400).unwrap();
        let unread_keys: Vec<Option<f64>> = list(&store, 10).unwrap().iter().map(|x| x.read_at).collect();
        assert_eq!(unread_keys, [None, Some(400.0)], "old_10 isn't old_1, and _ is only itself");
        let session = insert(&store, 1, &notice(Attention::Approval, 1), 500).unwrap().unwrap();
        assert_eq!((session.session_id, session.project_id), (Some(1), None));
        assert!(store.with(|c| c.execute("INSERT INTO notifications(kind,title,body,created_at,dedupe_key) VALUES('x','t','b',1,'k')", [])).is_err(), "a notice belongs to something");
    }
}
