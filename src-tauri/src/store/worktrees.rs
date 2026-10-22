//! Worktrees New session made (migration 0009).

use rusqlite::{OptionalExtension, Row, params};

use super::{Store, now_ms};

#[derive(Debug, Clone)]
pub struct WorktreeRow {
    pub id: u32,
    pub repo_root: String,
    pub path: String,
    pub branch: String,
    pub base_branch: Option<String>,
    pub removed: bool,
}

fn worktree_from(r: &Row<'_>) -> rusqlite::Result<WorktreeRow> {
    Ok(WorktreeRow {
        id: r.get(0)?,
        repo_root: r.get(1)?,
        path: r.get(2)?,
        branch: r.get(3)?,
        base_branch: r.get(4)?,
        removed: r.get::<_, Option<i64>>(5)?.is_some(),
    })
}

pub struct NewWorktree<'a> {
    pub repo_root: &'a str,
    pub path: &'a str,
    pub branch: &'a str,
    pub base_branch: Option<&'a str>,
}

pub fn insert(store: &Store, w: &NewWorktree<'_>) -> Result<u32, String> {
    store.with(|c| {
        c.execute(
            "INSERT INTO worktrees (repo_root, path, branch, base_branch, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![w.repo_root, w.path, w.branch, w.base_branch, now_ms()],
        )?;
        Ok(c.last_insert_rowid() as u32)
    })
}

pub fn get(store: &Store, id: u32) -> Result<Option<WorktreeRow>, String> {
    store.with(|c| {
        c.query_row("SELECT id, repo_root, path, branch, base_branch, removed_at FROM worktrees WHERE id = ?1", [id], worktree_from)
            .optional()
    })
}

pub fn list(store: &Store) -> Result<Vec<WorktreeRow>, String> {
    store.with(|c| {
        let mut q = c.prepare("SELECT id, repo_root, path, branch, base_branch, removed_at FROM worktrees ORDER BY id DESC")?;
        q.query_map([], worktree_from)?.collect()
    })
}

pub fn set_branch(store: &Store, id: u32, branch: &str) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE worktrees SET branch = ?2 WHERE id = ?1", params![id, branch]).map(|_| ()))
}

pub fn set_removed(store: &Store, id: u32) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE worktrees SET removed_at = ?2 WHERE id = ?1", params![id, now_ms()]).map(|_| ()))
}

/// Forgets a worktree that was never used: its session couldn't be recorded.
pub fn delete(store: &Store, id: u32) -> Result<(), String> {
    store.with(|c| c.execute("DELETE FROM worktrees WHERE id = ?1", [id]).map(|_| ()))
}

/// Open sessions working in a worktree, other than `except`.
pub fn other_open_sessions(store: &Store, id: u32, except: u32) -> Result<u32, String> {
    store.with(|c| {
        c.query_row(
            "SELECT COUNT(*) FROM sessions WHERE worktree_id = ?1 AND id != ?2 AND state != 'closed'",
            params![id, except],
            |r| r.get(0),
        )
    })
}
