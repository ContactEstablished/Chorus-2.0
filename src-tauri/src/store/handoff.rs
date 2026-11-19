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
}
