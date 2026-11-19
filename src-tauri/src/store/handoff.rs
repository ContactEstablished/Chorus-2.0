//! P18's reads for the hand-off note: a session's last reply and prompt, and the session in a
//! project that worked most recently.

use rusqlite::{OptionalExtension, params};

use super::Store;

/// The newest final message of a session, and when its turn ended. Never a prompt: a turn with no
/// final message is skipped (`history::latest` falls back to the prompt, which a note must not).
pub fn latest_reply(store: &Store, session_id: u32) -> Result<Option<(String, i64)>, String> {
    store.with(|c| {
        c.query_row(
            "SELECT final_message, COALESCE(ended_at, started_at) FROM turns
             WHERE session_id = ?1 AND final_message IS NOT NULL AND final_message != ''
             ORDER BY id DESC LIMIT 1",
            [session_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
    })
}

/// The newest prompt of a session.
pub fn latest_prompt(store: &Store, session_id: u32) -> Result<Option<String>, String> {
    store.with(|c| {
        c.query_row("SELECT text FROM prompts WHERE session_id = ?1 ORDER BY id DESC LIMIT 1", [session_id], |r| r.get(0))
            .optional()
    })
}

/// The project's session with the newest turn, else its newest session.
pub fn newest_session(store: &Store, project_id: u32) -> Result<Option<u32>, String> {
    store.with(|c| {
        c.query_row(
            "SELECT s.id FROM sessions s LEFT JOIN turns t ON t.session_id = s.id
             WHERE s.project_id = ?1
             GROUP BY s.id
             ORDER BY MAX(t.id) IS NULL, MAX(t.id) DESC, s.id DESC
             LIMIT 1",
            params![project_id],
            |r| r.get(0),
        )
        .optional()
    })
}

/// One hand-off to OpenMausBot (migration 0028).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffRow {
    pub id: u32,
    pub project_id: u32,
    pub state: String,
    pub task: String,
    pub bot_id: Option<String>,
    pub bot_name: Option<String>,
    pub thread_id: Option<String>,
    pub start_problem: Option<String>,
    pub branch: String,
    pub worktree: String,
    pub base_branch: Option<String>,
    pub base_sha: String,
    pub started_at: i64,
    pub returned_at: Option<i64>,
    pub returned_by: Option<String>,
    /// The returned entries, a JSON array of scrubbed Markdown.
    pub returned_entries: Option<String>,
    pub closed_at: Option<i64>,
}

const HANDOFF_COLUMNS: &str = "id, project_id, state, task, bot_id, bot_name, thread_id, start_problem, branch, worktree, \
     base_branch, base_sha, started_at, returned_at, returned_by, returned_entries, closed_at";

fn handoff_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<HandoffRow> {
    Ok(HandoffRow {
        id: r.get(0)?,
        project_id: r.get(1)?,
        state: r.get(2)?,
        task: r.get(3)?,
        bot_id: r.get(4)?,
        bot_name: r.get(5)?,
        thread_id: r.get(6)?,
        start_problem: r.get(7)?,
        branch: r.get(8)?,
        worktree: r.get(9)?,
        base_branch: r.get(10)?,
        base_sha: r.get(11)?,
        started_at: r.get(12)?,
        returned_at: r.get(13)?,
        returned_by: r.get(14)?,
        returned_entries: r.get(15)?,
        closed_at: r.get(16)?,
    })
}

pub struct NewHandoff<'a> {
    pub project_id: u32,
    pub task: &'a str,
    pub bot_id: Option<&'a str>,
    pub branch: &'a str,
    pub worktree: &'a str,
    pub base_branch: Option<&'a str>,
    pub base_sha: &'a str,
    pub started_at: i64,
}

/// A new active hand-off. A project already with MausBot is refused (the partial unique index).
pub fn insert(store: &Store, h: &NewHandoff<'_>) -> Result<u32, String> {
    let inserted = store.with(|c| {
        c.execute(
            "INSERT INTO handoffs (project_id, state, task, bot_id, branch, worktree, base_branch, base_sha, started_at)
             VALUES (?1, 'active', ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![h.project_id, h.task, h.bot_id, h.branch, h.worktree, h.base_branch, h.base_sha, h.started_at],
        )
        .map(|_| c.last_insert_rowid() as u32)
    });
    inserted.map_err(|e| if e.contains("UNIQUE") { "This project is already with MausBot. Take it back first.".into() } else { e })
}

fn one(store: &Store, sql: &str, p: impl rusqlite::Params) -> Result<Option<HandoffRow>, String> {
    store.with(|c| c.query_row(&format!("SELECT {HANDOFF_COLUMNS} FROM handoffs {sql}"), p, handoff_from).optional())
}

pub fn active(store: &Store, project_id: u32) -> Result<Option<HandoffRow>, String> {
    one(store, "WHERE project_id = ?1 AND state = 'active'", [project_id])
}

/// The newest hand-off that came back and hasn't been dismissed.
pub fn returned(store: &Store, project_id: u32) -> Result<Option<HandoffRow>, String> {
    one(store, "WHERE project_id = ?1 AND state = 'returned' ORDER BY id DESC LIMIT 1", [project_id])
}

/// Every active hand-off, for the watch.
pub fn all_active(store: &Store) -> Result<Vec<HandoffRow>, String> {
    store.with(|c| {
        c.prepare(&format!("SELECT {HANDOFF_COLUMNS} FROM handoffs WHERE state = 'active' ORDER BY id"))?
            .query_map([], handoff_from)?
            .collect()
    })
}

/// The bot used last for a project, to offer it first.
pub fn last_bot(store: &Store, project_id: u32) -> Result<Option<String>, String> {
    store.with(|c| {
        c.query_row("SELECT bot_id FROM handoffs WHERE project_id = ?1 AND bot_id IS NOT NULL ORDER BY id DESC LIMIT 1", [project_id], |r| r.get(0))
            .optional()
    })
}

/// How MausBot was started: by Chorus (a conversation) or not (the reason).
pub fn set_started(store: &Store, id: u32, bot_id: Option<&str>, bot_name: Option<&str>, thread_id: Option<&str>, problem: Option<&str>) -> Result<(), String> {
    store.with(|c| {
        c.execute(
            "UPDATE handoffs SET bot_id = ?2, bot_name = ?3, thread_id = ?4, start_problem = ?5 WHERE id = ?1",
            params![id, bot_id, bot_name, thread_id, problem],
        )
        .map(|_| ())
    })
}

/// Claims the return: true only for the one caller that moved the hand-off from active, so the
/// watch and Take back never both bring the entries back.
pub fn set_returned(store: &Store, id: u32, by: &str, at: i64) -> Result<bool, String> {
    store.with(|c| {
        c.execute("UPDATE handoffs SET state = 'returned', returned_by = ?2, returned_at = ?3 WHERE id = ?1 AND state = 'active'", params![id, by, at])
            .map(|n| n == 1)
    })
}

pub fn set_returned_entries(store: &Store, id: u32, entries_json: &str) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE handoffs SET returned_entries = ?2 WHERE id = ?1", params![id, entries_json]).map(|_| ()))
}

/// Puts away every return card of a project, when a new hand-off starts.
pub fn close_returned(store: &Store, project_id: u32, at: i64) -> Result<(), String> {
    store.with(|c| {
        c.execute("UPDATE handoffs SET state = 'closed', closed_at = ?2 WHERE project_id = ?1 AND state = 'returned'", params![project_id, at])
            .map(|_| ())
    })
}

pub fn close(store: &Store, id: u32, at: i64) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE handoffs SET state = 'closed', closed_at = ?2 WHERE id = ?1", params![id, at]).map(|_| ()))
}

/// Removes a hand-off whose worktree couldn't be finished, as if it never started.
pub fn delete(store: &Store, id: u32) -> Result<(), String> {
    store.with(|c| c.execute("DELETE FROM handoffs WHERE id = ?1", [id]).map(|_| ()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::projects::{self, NewProject};
    use crate::store::sessions::{self as rows, NewSession};
    use chorus_core::agent::{AgentKind, LaunchOptions};

    fn session(s: &Store, project: Option<u32>) -> u32 {
        rows::insert(s, &NewSession {
            name: "t", agent: AgentKind::Claude, cwd: "C:\\w", credential_id: None, credential_env: None, cols: 80, rows: 24,
            options: &LaunchOptions::default(), agent_session_id: None, provider_id: None, description: None,
            project_id: project, worktree_id: None, routing: None,
        })
        .unwrap()
    }

    fn turn(s: &Store, session: u32, prompt: Option<&str>, reply: Option<&str>, at: i64) {
        s.with(|c| {
            let prompt_id: Option<i64> = match prompt {
                Some(p) => {
                    c.execute("INSERT INTO prompts (session_id, source, text, created_at) VALUES (?1, 'hook', ?2, ?3)", params![session, p, at])?;
                    Some(c.last_insert_rowid())
                }
                None => None,
            };
            c.execute(
                "INSERT INTO turns (session_id, prompt_id, started_at, ended_at, final_message) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![session, prompt_id, at, at + 10, reply],
            )
        })
        .unwrap();
    }

    #[test]
    fn the_latest_reply_skips_turns_without_one_and_never_returns_a_prompt() {
        let s = Store::in_memory();
        let id = session(&s, None);
        assert_eq!(latest_reply(&s, id).unwrap(), None);
        turn(&s, id, Some("first"), Some("first reply"), 100);
        turn(&s, id, Some("second"), None, 200);
        turn(&s, id, Some("third"), Some(""), 300);
        assert_eq!(latest_reply(&s, id).unwrap(), Some(("first reply".into(), 110)));
        assert_eq!(latest_prompt(&s, id).unwrap().as_deref(), Some("third"));
    }

    #[test]
    fn the_newest_session_is_the_one_that_worked_last() {
        let s = Store::in_memory();
        let p = projects::insert(&s, &NewProject { name: "p", folder: Some("C:\\p"), colour: "#000000" }).unwrap();
        assert_eq!(newest_session(&s, p).unwrap(), None);
        let a = session(&s, Some(p));
        let b = session(&s, Some(p));
        let _elsewhere = session(&s, None);
        assert_eq!(newest_session(&s, p).unwrap(), Some(b), "no turns yet: the newest session");
        turn(&s, a, Some("x"), Some("y"), 100);
        assert_eq!(newest_session(&s, p).unwrap(), Some(a));
        turn(&s, b, Some("x"), Some("y"), 200);
        assert_eq!(newest_session(&s, p).unwrap(), Some(b));
    }

    #[test]
    fn a_project_has_at_most_one_active_handoff() {
        let s = Store::in_memory();
        let p = projects::insert(&s, &NewProject { name: "p", folder: Some("C:\\p"), colour: "#000000" }).unwrap();
        let new = |branch: &'static str| NewHandoff {
            project_id: p, task: "", bot_id: None, branch, worktree: "D:\\w", base_branch: Some("main"), base_sha: "abc", started_at: 1,
        };
        let a = insert(&s, &new("maus/a")).unwrap();
        assert!(insert(&s, &new("maus/b")).unwrap_err().contains("already with MausBot"));
        assert_eq!(active(&s, p).unwrap().map(|h| h.id), Some(a));
        set_started(&s, a, Some("bot-1"), Some("Atlas Dev"), Some("t-1"), None).unwrap();
        assert_eq!(last_bot(&s, p).unwrap().as_deref(), Some("bot-1"));
        assert!(set_returned(&s, a, "mausbot", 5).unwrap(), "the first return claims it");
        assert!(!set_returned(&s, a, "chorus", 6).unwrap(), "a second return finds nothing to claim");
        set_returned_entries(&s, a, "[\"## x\"]").unwrap();
        assert_eq!(active(&s, p).unwrap(), None);
        let back = returned(&s, p).unwrap().unwrap();
        assert_eq!((back.returned_by.as_deref(), back.returned_at, back.thread_id.as_deref()), (Some("mausbot"), Some(5), Some("t-1")));
        assert_eq!(back.returned_entries.as_deref(), Some("[\"## x\"]"));
        let b = insert(&s, &new("maus/b")).unwrap();
        assert_eq!(all_active(&s).unwrap().len(), 1);
        close_returned(&s, p, 6).unwrap();
        assert_eq!(returned(&s, p).unwrap(), None, "a new hand-off puts the old return away");
        close(&s, a, 6).unwrap();
        delete(&s, b).unwrap();
        assert_eq!(active(&s, p).unwrap(), None);
    }
}
