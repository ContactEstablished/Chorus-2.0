//! Projects (migration 0007). Names and colours arrive checked (`chorus_core::projects`).

use chorus_core::projects::ProjectState;
use rusqlite::{OptionalExtension, Row, params};

use super::{Store, now_ms};

#[derive(Debug, Clone)]
pub struct ProjectRow {
    pub id: u32,
    pub name: String,
    pub folder: Option<String>,
    pub colour: String,
    pub state: ProjectState,
    pub imported_from: Option<String>,
}

const COLUMNS: &str = "id, name, folder, colour, state, imported_from";

fn project_from(r: &Row<'_>) -> rusqlite::Result<ProjectRow> {
    let state: String = r.get(4)?;
    Ok(ProjectRow {
        id: r.get(0)?,
        name: r.get(1)?,
        folder: r.get(2)?,
        colour: r.get(3)?,
        state: ProjectState::parse(&state).unwrap_or(ProjectState::Active),
        imported_from: r.get(5)?,
    })
}

pub struct NewProject<'a> {
    pub name: &'a str,
    pub folder: Option<&'a str>,
    pub colour: &'a str,
    pub state: ProjectState,
    pub imported_from: Option<&'a str>,
    pub imported_id: Option<&'a str>,
}

/// Adds a project at the end of the rail.
pub fn insert(store: &Store, p: &NewProject<'_>) -> Result<u32, String> {
    store.with(|c| {
        let next: i64 = c.query_row("SELECT COALESCE(MAX(position), -1) + 1 FROM projects", [], |r| r.get(0))?;
        c.execute(
            "INSERT INTO projects (name, folder, colour, position, state, imported_from, imported_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![p.name, p.folder, p.colour, next, p.state.as_str(), p.imported_from, p.imported_id, now_ms()],
        )?;
        Ok(c.last_insert_rowid() as u32)
    })
}

pub fn update(store: &Store, id: u32, name: &str, folder: Option<&str>, colour: &str) -> Result<(), String> {
    store.with(|c| {
        c.execute("UPDATE projects SET name = ?2, folder = ?3, colour = ?4 WHERE id = ?1", params![id, name, folder, colour]).map(|_| ())
    })
}

pub fn set_state(store: &Store, id: u32, state: ProjectState) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE projects SET state = ?2 WHERE id = ?1", params![id, state.as_str()]).map(|_| ()))
}

/// Puts projects in the given order; ids not listed keep their order after them.
pub fn reorder(store: &Store, ids: &[u32]) -> Result<(), String> {
    store.with(|c| {
        let tx = c.unchecked_transaction()?;
        let listed = serde_json::to_string(ids).unwrap_or_else(|_| "[]".into());
        tx.execute(
            "UPDATE projects SET position = position + ?1 WHERE id NOT IN (SELECT value FROM json_each(?2))",
            params![ids.len() as i64, listed],
        )?;
        for (i, id) in ids.iter().enumerate() {
            tx.execute("UPDATE projects SET position = ?2 WHERE id = ?1", params![id, i as i64])?;
        }
        tx.commit()
    })
}

pub fn delete(store: &Store, id: u32) -> Result<bool, String> {
    store.with(|c| c.execute("DELETE FROM projects WHERE id = ?1", [id]).map(|n| n > 0))
}

pub fn get(store: &Store, id: u32) -> Result<Option<ProjectRow>, String> {
    store.with(|c| c.query_row(&format!("SELECT {COLUMNS} FROM projects WHERE id = ?1"), [id], project_from).optional())
}

pub fn list(store: &Store) -> Result<Vec<ProjectRow>, String> {
    store.with(|c| {
        let mut stmt = c.prepare(&format!("SELECT {COLUMNS} FROM projects ORDER BY position, id"))?;
        stmt.query_map([], project_from)?.collect()
    })
}

/// Whether a project imported from `source` with that id is already here.
pub fn imported_exists(store: &Store, source: &str, imported_id: &str) -> Result<bool, String> {
    store.with(|c| {
        c.query_row("SELECT 1 FROM projects WHERE imported_from = ?1 AND imported_id = ?2", params![source, imported_id], |_| Ok(()))
            .optional()
            .map(|o| o.is_some())
    })
}

pub fn count(store: &Store) -> Result<usize, String> {
    store.with(|c| c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get::<_, i64>(0))).map(|n| n as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(s: &Store, name: &str) -> u32 {
        insert(s, &NewProject { name, folder: None, colour: "#f472b6", state: ProjectState::Active, imported_from: None, imported_id: None }).unwrap()
    }

    #[test]
    fn projects_are_added_reordered_and_removed() {
        let s = Store::in_memory();
        let (a, b, c) = (add(&s, "A"), add(&s, "B"), add(&s, "C"));
        assert_eq!(list(&s).unwrap().iter().map(|p| p.id).collect::<Vec<_>>(), [a, b, c]);
        reorder(&s, &[c, a]).unwrap();
        assert_eq!(list(&s).unwrap().iter().map(|p| p.id).collect::<Vec<_>>(), [c, a, b]);
        set_state(&s, b, ProjectState::Archived).unwrap();
        update(&s, a, "A2", Some("C:\\a"), "#a5b4fc").unwrap();
        let got = get(&s, a).unwrap().unwrap();
        assert_eq!((got.name.as_str(), got.folder.as_deref(), got.colour.as_str()), ("A2", Some("C:\\a"), "#a5b4fc"));
        assert_eq!(get(&s, b).unwrap().unwrap().state, ProjectState::Archived);
        assert!(delete(&s, c).unwrap());
        assert_eq!(count(&s).unwrap(), 2);
        assert!(!imported_exists(&s, "chorus1", "x").unwrap());
    }
}
