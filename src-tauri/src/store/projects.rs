//! Projects (migrations 0007 and 0008). Names and colours arrive checked (`chorus_core::projects`).

use chorus_core::projects::{ProjectState, same_folder};
use rusqlite::{OptionalExtension, Row, params};

use super::{Store, now_ms};

#[derive(Debug, Clone)]
pub struct ProjectRow {
    pub id: u32,
    pub name: String,
    pub folder: Option<String>,
    pub colour: String,
    pub state: ProjectState,
}

const COLUMNS: &str = "id, name, folder, colour, state";

fn project_from(r: &Row<'_>) -> rusqlite::Result<ProjectRow> {
    let state: String = r.get(4)?;
    Ok(ProjectRow {
        id: r.get(0)?,
        name: r.get(1)?,
        folder: r.get(2)?,
        colour: r.get(3)?,
        state: ProjectState::parse(&state).unwrap_or(ProjectState::Active),
    })
}

pub struct NewProject<'a> {
    pub name: &'a str,
    pub folder: Option<&'a str>,
    pub colour: &'a str,
}

/// Check under the same store lock as the write, including hidden and archived projects.
/// Resolve older stored paths too: they may predate canonical folder storage.
fn folder_error(c: &rusqlite::Connection, folder: Option<&str>, own_id: Option<u32>) -> rusqlite::Result<Option<String>> {
    let Some(folder) = folder.filter(|f| !f.trim().is_empty()) else {
        return Ok(Some("Choose a folder for this project.".into()));
    };
    let resolved = |path: &str| std::fs::canonicalize(path).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| path.to_owned());
    let candidate = resolved(folder);
    let mut stmt = c.prepare("SELECT id, name, folder FROM projects WHERE folder IS NOT NULL")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let id: u32 = row.get(0)?;
        let existing: String = row.get(2)?;
        if Some(id) != own_id && (same_folder(folder, &existing) || same_folder(&candidate, &resolved(&existing))) {
            let name: String = row.get(1)?;
            return Ok(Some(format!("That folder is already tracked by project \"{name}\". Choose a different folder or edit that project.")));
        }
    }
    Ok(None)
}

/// Adds an active project at the end of the rail.
pub fn insert(store: &Store, p: &NewProject<'_>) -> Result<u32, String> {
    store.with(|c| {
        if let Some(error) = folder_error(c, p.folder, None)? {
            return Ok(Err(error));
        }
        let next: i64 = c.query_row("SELECT COALESCE(MAX(position), -1) + 1 FROM projects", [], |r| r.get(0))?;
        c.execute(
            "INSERT INTO projects (name, folder, colour, position, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![p.name, p.folder, p.colour, next, now_ms()],
        )?;
        Ok(Ok(c.last_insert_rowid() as u32))
    })?
}

pub fn update(store: &Store, id: u32, name: &str, folder: Option<&str>, colour: &str) -> Result<(), String> {
    store.with(|c| {
        if let Some(error) = folder_error(c, folder, Some(id))? {
            return Ok(Err(error));
        }
        c.execute("UPDATE projects SET name = ?2, folder = ?3, colour = ?4 WHERE id = ?1", params![id, name, folder, colour])?;
        Ok(Ok(()))
    })?
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

pub fn count(store: &Store) -> Result<usize, String> {
    store.with(|c| c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get::<_, i64>(0))).map(|n| n as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_are_required_and_unique_for_creates_and_edits_in_every_state() {
        let s = Store::in_memory();
        for folder in [None, Some(""), Some("  ")] {
            assert!(insert(&s, &NewProject { name: "Empty", folder, colour: "#f472b6" }).unwrap_err().contains("Choose a folder"));
        }
        let a = add(&s, "A");
        let b = add(&s, "B");
        for state in [ProjectState::Active, ProjectState::Hidden, ProjectState::Archived] {
            set_state(&s, a, state).unwrap();
            let duplicate = NewProject { name: "Duplicate", folder: Some("c:/PROJECTS/a/"), colour: "#f472b6" };
            assert!(insert(&s, &duplicate).unwrap_err().contains("project \"A\""));
            assert!(update(&s, b, "B", Some(r"\\?\C:\projects\A"), "#f472b6").is_err());
        }
        assert!(update(&s, b, "B", None, "#f472b6").is_err());
        assert_eq!(get(&s, b).unwrap().unwrap().folder.as_deref(), Some(r"C:\projects\B"));
        update(&s, a, "Renamed", Some(r"C:\projects\A"), "#a5b4fc").unwrap();
        insert(&s, &NewProject { name: "Child", folder: Some(r"C:\projects\A\child"), colour: "#f472b6" }).unwrap();
        delete(&s, a).unwrap();
        insert(&s, &NewProject { name: "Replacement", folder: Some(r"C:\projects\A"), colour: "#f472b6" }).unwrap();
    }

    #[test]
    fn concurrent_project_creates_cannot_claim_the_same_folder() {
        let store = std::sync::Arc::new(Store::in_memory());
        let threads: Vec<_> = (0..8).map(|_| {
            let store = store.clone();
            std::thread::spawn(move || insert(&store, &NewProject { name: "Project", folder: Some(r"C:\projects\shared"), colour: "#f472b6" }))
        }).collect();
        assert_eq!(threads.into_iter().filter_map(|t| t.join().unwrap().ok()).count(), 1);
        assert_eq!(count(&store).unwrap(), 1);
    }

    #[test]
    fn legacy_folder_aliases_are_resolved_before_comparison() {
        let s = Store::in_memory();
        let folder = std::env::temp_dir();
        let alias = folder.join(".").display().to_string();
        // Simulate a row written before canonical folder storage.
        s.with(|c| c.execute("INSERT INTO projects (name, folder, colour, position, created_at) VALUES ('Legacy', ?1, '#f472b6', 0, 0)", [&alias])).unwrap();
        let canonical = folder.canonicalize().unwrap().display().to_string();
        assert!(insert(&s, &NewProject { name: "Duplicate", folder: Some(&canonical), colour: "#f472b6" }).unwrap_err().contains("Legacy"));
    }

    fn add(s: &Store, name: &str) -> u32 {
        insert(s, &NewProject { name, folder: Some(&format!(r"C:\projects\{name}")), colour: "#f472b6" }).unwrap()
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
        assert_eq!(get(&s, a).unwrap().unwrap().state, ProjectState::Active, "a new project starts in the rail");
    }
}
