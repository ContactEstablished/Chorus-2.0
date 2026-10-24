//! Notes (P8, D17): project-tagged or global context added to agents' instructions at each run
//! start (decision 2). Inputs are checked here; composing them into a run's text is
//! `chorus_core::instructions`.

use std::sync::Arc;

use chorus_core::instructions::{NoteInput, check_note};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::sessions::SessionManager;
use crate::store::{Store, now_ms, projects as project_rows};
use crate::vault::commands::secs;

type Manager<'a> = State<'a, Arc<SessionManager>>;

/// More than anyone keeps by hand; the cap on a run's text leaves most out anyway.
const MAX_NOTES: u32 = 500;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: u32,
    /// None: the note applies to every project.
    pub project_id: Option<u32>,
    pub title: String,
    pub body: String,
    pub pinned: bool,
    pub created_at_secs: u32,
    pub updated_at_secs: u32,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NoteSave {
    /// None adds one.
    pub id: Option<u32>,
    pub project_id: Option<u32>,
    pub title: String,
    pub body: String,
    pub pinned: bool,
}

const COLUMNS: &str = "id, project_id, title, body, pinned, created_at, updated_at";

fn note_from(r: &Row<'_>) -> rusqlite::Result<(Note, i64)> {
    let updated: i64 = r.get(6)?;
    Ok((
        Note {
            id: r.get(0)?,
            project_id: r.get(1)?,
            title: r.get(2)?,
            body: r.get(3)?,
            pinned: r.get(4)?,
            created_at_secs: secs(r.get(5)?),
            updated_at_secs: secs(updated),
        },
        updated,
    ))
}

pub fn list(store: &Store) -> Result<Vec<Note>, String> {
    store.with(|c| {
        let mut stmt = c.prepare(&format!("SELECT {COLUMNS} FROM notes ORDER BY pinned DESC, updated_at DESC, id DESC"))?;
        stmt.query_map([], |r| note_from(r).map(|(n, _)| n))?.collect()
    })
}

fn get(store: &Store, id: u32) -> Result<Option<Note>, String> {
    store.with(|c| c.query_row(&format!("SELECT {COLUMNS} FROM notes WHERE id = ?1"), [id], |r| note_from(r).map(|(n, _)| n)).optional())
}

/// The notes a run in `project` receives: every untagged note, and that project's.
pub fn for_project(store: &Store, project: Option<u32>) -> Result<Vec<NoteInput>, String> {
    store.with(|c| {
        let mut stmt = c.prepare(&format!("SELECT {COLUMNS} FROM notes WHERE project_id IS NULL OR project_id = ?1"))?;
        stmt.query_map([project], |r| {
            note_from(r).map(|(n, updated_at)| NoteInput {
                id: n.id,
                project: n.project_id.is_some(),
                title: n.title,
                body: n.body,
                pinned: n.pinned,
                updated_at,
            })
        })?
        .collect()
    })
}

fn save(store: &Store, input: &NoteSave) -> Result<Note, String> {
    let (title, body) = check_note(&input.title, &input.body)?;
    crate::bots::refuse_secrets(store, "the note", &format!("{title}\n{body}"))?;
    if let Some(p) = input.project_id {
        project_rows::get(store, p)?.ok_or("That project no longer exists.")?;
    }
    let now = now_ms();
    let id = match input.id {
        Some(id) => {
            get(store, id)?.ok_or("That note no longer exists.")?;
            store.with(|c| {
                c.execute(
                    "UPDATE notes SET project_id = ?2, title = ?3, body = ?4, pinned = ?5, updated_at = ?6 WHERE id = ?1",
                    params![id, input.project_id, title, body, input.pinned, now],
                )
            })?;
            id
        }
        None => {
            let count: u32 = store.with(|c| c.query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0)))?;
            if count >= MAX_NOTES {
                return Err(format!("Chorus keeps up to {MAX_NOTES} notes. Delete some first."));
            }
            store.with(|c| {
                c.execute(
                    "INSERT INTO notes (project_id, title, body, pinned, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                    params![input.project_id, title, body, input.pinned, now],
                )?;
                Ok(c.last_insert_rowid() as u32)
            })?
        }
    };
    get(store, id)?.ok_or_else(|| "That note no longer exists.".into())
}

#[tauri::command]
#[specta::specta]
pub fn notes_list(state: Manager<'_>) -> Result<Vec<Note>, String> {
    list(state.store())
}

#[tauri::command]
#[specta::specta]
pub fn note_save(state: Manager<'_>, input: NoteSave) -> Result<Note, String> {
    save(state.store(), &input)
}

#[tauri::command]
#[specta::specta]
pub fn note_delete(state: Manager<'_>, id: u32) -> Result<bool, String> {
    state.store().with(|c| c.execute("DELETE FROM notes WHERE id = ?1", [id]).map(|n| n > 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(project_id: Option<u32>, title: &str, pinned: bool) -> NoteSave {
        NoteSave { id: None, project_id, title: title.into(), body: format!("{title} body"), pinned }
    }

    #[test]
    fn a_project_note_reaches_only_its_project_and_untagged_notes_reach_all() {
        let s = Store::in_memory();
        let a = project_rows::insert(&s, &project_rows::NewProject { name: "A", folder: Some(r"C:\projects\A"), colour: "#111111" }).unwrap();
        let b = project_rows::insert(&s, &project_rows::NewProject { name: "B", folder: Some(r"C:\projects\B"), colour: "#222222" }).unwrap();
        save(&s, &input(None, "Everywhere", false)).unwrap();
        save(&s, &input(Some(a), "Only A", true)).unwrap();
        let titles = |p| for_project(&s, p).unwrap().into_iter().map(|n| n.title).collect::<Vec<_>>();
        let mut in_a = titles(Some(a));
        in_a.sort();
        assert_eq!(in_a, ["Everywhere", "Only A"]);
        assert_eq!(titles(Some(b)), ["Everywhere"]);
        assert_eq!(titles(None), ["Everywhere"]);
        // Pinned first in the list.
        assert_eq!(list(&s).unwrap()[0].title, "Only A");
    }

    #[test]
    fn saving_checks_the_note_and_its_project() {
        let s = Store::in_memory();
        assert!(save(&s, &input(Some(99), "x", false)).unwrap_err().contains("project"));
        assert!(save(&s, &NoteSave { title: " ".into(), ..input(None, "x", false) }).is_err());
        let n = save(&s, &input(None, "x", false)).unwrap();
        let edited = save(&s, &NoteSave { id: Some(n.id), title: "y".into(), ..input(None, "x", true) }).unwrap();
        assert_eq!((edited.id, edited.title.as_str(), edited.pinned), (n.id, "y", true));
        assert!(save(&s, &NoteSave { id: Some(999), ..input(None, "x", false) }).is_err());
    }
}
