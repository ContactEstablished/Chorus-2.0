//! Projects (PLAN P4): add, edit, reorder, hide, archive and delete, and the one-time, read-only
//! import of Chorus 1's projects (D33). Inputs are checked here; rows live in `store::projects`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chorus_core::projects::{self as pj, ProjectState};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::sessions::SessionManager;
use crate::store::projects::{self as rows, NewProject, ProjectRow};
use crate::store::{Store, sessions as session_rows};

type Manager<'a> = State<'a, Arc<SessionManager>>;

const MAX_PROJECTS: usize = 200;
const MAX_FOLDER_CHARS: usize = 1024;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: u32,
    pub name: String,
    pub folder: Option<String>,
    pub colour: String,
    pub state: ProjectState,
    /// `chorus1` when it came from the import.
    pub imported_from: Option<String>,
}

impl From<ProjectRow> for Project {
    fn from(r: ProjectRow) -> Self {
        Self { id: r.id, name: r.name, folder: r.folder, colour: r.colour, state: r.state, imported_from: r.imported_from }
    }
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInput {
    pub name: String,
    #[serde(default)]
    pub folder: Option<String>,
    /// Left out, the next colour in the palette.
    #[serde(default)]
    pub colour: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub added: u32,
    /// Already here, from an earlier import or with the same folder.
    pub skipped: u32,
    pub projects: Vec<Project>,
}

fn all(store: &Store) -> Result<Vec<Project>, String> {
    Ok(rows::list(store)?.into_iter().map(Project::from).collect())
}

fn clean_folder(folder: Option<&str>) -> Result<Option<String>, String> {
    let Some(f) = folder.map(str::trim).filter(|f| !f.is_empty()) else { return Ok(None) };
    if f.chars().count() > MAX_FOLDER_CHARS || f.chars().any(char::is_control) {
        return Err("That folder path is too long or has control characters.".into());
    }
    let p = Path::new(f);
    if !p.is_absolute() {
        return Err(format!("{f} isn't a full path."));
    }
    if !p.is_dir() {
        return Err(format!("{f} isn't a folder."));
    }
    Ok(Some(f.trim_end_matches(['\\', '/']).to_owned()))
}

fn existing(store: &Store, id: u32) -> Result<ProjectRow, String> {
    rows::get(store, id)?.ok_or_else(|| format!("no project {id}"))
}

#[tauri::command]
#[specta::specta]
pub fn projects_list(state: Manager<'_>) -> Result<Vec<Project>, String> {
    all(state.store())
}

/// The colours offered for projects, in the order new projects get them.
#[tauri::command]
#[specta::specta]
pub fn project_palette() -> Vec<String> {
    pj::PALETTE.iter().map(|c| (*c).to_owned()).collect()
}

/// Adds a project, or with `id` edits one. Returns every project.
#[tauri::command]
#[specta::specta]
pub fn project_save(state: Manager<'_>, id: Option<u32>, project: ProjectInput) -> Result<Vec<Project>, String> {
    let store = state.store();
    let name = pj::clean_name(&project.name)?;
    let folder = clean_folder(project.folder.as_deref())?;
    match id {
        Some(id) => {
            let current = existing(store, id)?;
            let colour = match project.colour.as_deref() {
                Some(c) => pj::clean_colour(c)?,
                None => current.colour,
            };
            rows::update(store, id, &name, folder.as_deref(), &colour)?;
        }
        None => {
            let count = rows::count(store)?;
            if count >= MAX_PROJECTS {
                return Err(format!("Chorus keeps up to {MAX_PROJECTS} projects; archive or delete one first."));
            }
            let colour = match project.colour.as_deref() {
                Some(c) => pj::clean_colour(c)?,
                None => pj::next_colour(count).to_owned(),
            };
            let new = NewProject {
                name: &name,
                folder: folder.as_deref(),
                colour: &colour,
                state: ProjectState::Active,
                imported_from: None,
                imported_id: None,
            };
            rows::insert(store, &new)?;
        }
    }
    all(store)
}

#[tauri::command]
#[specta::specta]
pub fn project_set_state(state: Manager<'_>, id: u32, project_state: ProjectState) -> Result<Vec<Project>, String> {
    existing(state.store(), id)?;
    rows::set_state(state.store(), id, project_state)?;
    all(state.store())
}

/// Puts projects in this order (the rail's, top first).
#[tauri::command]
#[specta::specta]
pub fn projects_reorder(state: Manager<'_>, ids: Vec<u32>) -> Result<Vec<Project>, String> {
    let store = state.store();
    let known: Vec<u32> = rows::list(store)?.iter().map(|p| p.id).collect();
    if ids.len() > known.len() || ids.iter().any(|id| !known.contains(id)) {
        return Err("That order names a project that isn't here.".into());
    }
    let mut seen = std::collections::HashSet::new();
    if !ids.iter().all(|id| seen.insert(*id)) {
        return Err("That order names a project twice.".into());
    }
    rows::reorder(store, &ids)?;
    all(store)
}

/// Deletes a project. Its sessions keep running, in no project.
#[tauri::command]
#[specta::specta]
pub fn project_delete(state: Manager<'_>, id: u32) -> Result<Vec<Project>, String> {
    let store = state.store();
    existing(store, id)?;
    let had_sessions = session_rows::list_open(store)?.iter().any(|s| s.project_id == Some(id));
    rows::delete(store, id)?;
    if had_sessions {
        state.notify_all();
    }
    all(store)
}

#[tauri::command]
#[specta::specta]
pub fn session_set_project(state: Manager<'_>, session_id: u32, project_id: Option<u32>) -> Result<crate::sessions::SessionSummary, String> {
    state.set_project(session_id, project_id)
}

/// Where Chorus 1 keeps its database (its `userData`), if `APPDATA` is set.
fn chorus1_db() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("chorus-app").join("chorus.db"))
}

/// Whether a Chorus 1 database is there to import from.
#[tauri::command]
#[specta::specta]
pub fn chorus1_import_available() -> bool {
    chorus1_db().is_some_and(|p| p.is_file())
}

/// Copies Chorus 1's projects (name, folder, colour, order, state) in, once each. Chorus 1's
/// database is opened read only and never written (D33).
#[tauri::command]
#[specta::specta]
pub fn chorus1_import(state: Manager<'_>) -> Result<ImportReport, String> {
    let path = chorus1_db().filter(|p| p.is_file()).ok_or("Chorus 1's database isn't on this computer.")?;
    let conn = open_read_only(&path).map_err(|e| format!("Couldn't open Chorus 1's database read only: {e}"))?;
    let found = read_chorus1(&conn).map_err(|e| format!("Couldn't read Chorus 1's projects: {e}"))?;
    drop(conn);
    import(state.store(), &found)
}

/// Opens without writing anything: through the write-ahead log when Chorus 1 is running (its
/// `-wal` is there), else as an unchanging file, so no `-wal` or `-shm` is ever created.
fn open_read_only(path: &Path) -> rusqlite::Result<Connection> {
    let wal = path.with_file_name(format!("{}-wal", path.file_name().and_then(|n| n.to_str()).unwrap_or("chorus.db")));
    let url = path.to_string_lossy().replace('%', "%25").replace('\\', "/").replace('?', "%3f").replace('#', "%23");
    let uri = if wal.is_file() { format!("file:/{url}?mode=ro") } else { format!("file:/{url}?mode=ro&immutable=1") };
    let conn = Connection::open_with_flags(uri, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    conn.pragma_update(None, "query_only", "ON")?;
    Ok(conn)
}

#[derive(Debug, Clone, PartialEq)]
struct Chorus1Project {
    id: String,
    name: String,
    folder: String,
    colour: Option<String>,
    status: String,
}

/// Chorus 1's projects in its own order.
fn read_chorus1(conn: &Connection) -> rusqlite::Result<Vec<Chorus1Project>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, root_path, color, COALESCE(status, 'active') FROM projects
         ORDER BY COALESCE(sort_order, 1e18), created_at, name",
    )?;
    stmt.query_map([], |r| {
        Ok(Chorus1Project { id: r.get(0)?, name: r.get(1)?, folder: r.get(2)?, colour: r.get(3)?, status: r.get(4)? })
    })?
    .collect()
}

fn same_folder(a: &str, b: &str) -> bool {
    let n = |s: &str| s.replace('/', "\\").trim_end_matches('\\').to_lowercase();
    n(a) == n(b)
}

fn import(store: &Store, found: &[Chorus1Project]) -> Result<ImportReport, String> {
    let (mut added, mut skipped) = (0, 0);
    for p in found {
        let here = rows::list(store)?;
        let dup = rows::imported_exists(store, "chorus1", &p.id)?
            || here.iter().any(|h| h.folder.as_deref().is_some_and(|f| same_folder(f, &p.folder)));
        if dup || here.len() >= MAX_PROJECTS {
            skipped += 1;
            continue;
        }
        let name = pj::clean_name(&p.name).unwrap_or_else(|_| {
            Path::new(&p.folder).file_name().map_or_else(|| "Imported project".to_owned(), |n| n.to_string_lossy().chars().take(pj::MAX_NAME_CHARS).collect())
        });
        let colour = p.colour.as_deref().and_then(|c| pj::clean_colour(c).ok()).unwrap_or_else(|| pj::next_colour(here.len()).to_owned());
        let folder = p.folder.trim();
        rows::insert(
            store,
            &NewProject {
                name: &name,
                folder: (!folder.is_empty()).then_some(folder),
                colour: &colour,
                state: pj::chorus1_state(&p.status),
                imported_from: Some("chorus1"),
                imported_id: Some(&p.id),
            },
        )?;
        added += 1;
    }
    Ok(ImportReport { added, skipped, projects: all(store)? })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Chorus 1's `projects` table as its source creates it.
    fn chorus1_fixture() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL, root_path TEXT NOT NULL UNIQUE,
               created_at TEXT NOT NULL, color TEXT, description TEXT, status TEXT NOT NULL DEFAULT 'active',
               sort_order INTEGER, color_seed TEXT);
             INSERT INTO projects VALUES ('p2', 'Ledger', 'C:\\work\\ledger', '2026-01-02', '#E0A96D', NULL, 'active', 1, NULL);
             INSERT INTO projects VALUES ('p1', 'Auth', 'C:\\work\\auth', '2026-01-01', NULL, NULL, 'hidden', 0, NULL);
             INSERT INTO projects VALUES ('p3', '', 'C:\\work\\old-site', '2026-01-03', 'nope', NULL, 'archived', NULL, NULL);",
        )
        .unwrap();
        c
    }

    #[test]
    fn chorus_1_projects_come_across_once() {
        let found = read_chorus1(&chorus1_fixture()).unwrap();
        assert_eq!(found.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["p1", "p2", "p3"]);

        let store = Store::in_memory();
        let first = import(&store, &found).unwrap();
        assert_eq!((first.added, first.skipped), (3, 0));
        let p = &first.projects;
        assert_eq!((p[0].name.as_str(), p[0].state, p[0].colour.as_str()), ("Auth", ProjectState::Hidden, pj::PALETTE[0]));
        assert_eq!((p[1].name.as_str(), p[1].colour.as_str()), ("Ledger", "#e0a96d"));
        assert_eq!((p[2].name.as_str(), p[2].state), ("old-site", ProjectState::Archived), "no name: the folder's");
        assert_eq!(p[2].imported_from.as_deref(), Some("chorus1"));

        let again = import(&store, &found).unwrap();
        assert_eq!((again.added, again.skipped, again.projects.len()), (0, 3, 3));
    }

    #[test]
    fn a_project_with_the_same_folder_is_not_imported_again() {
        let store = Store::in_memory();
        let mine = NewProject { name: "Mine", folder: Some("c:/work/ledger/"), colour: "#a5b4fc", state: ProjectState::Active, imported_from: None, imported_id: None };
        rows::insert(&store, &mine).unwrap();
        let report = import(&store, &read_chorus1(&chorus1_fixture()).unwrap()).unwrap();
        assert_eq!((report.added, report.skipped), (2, 1));
    }
}
