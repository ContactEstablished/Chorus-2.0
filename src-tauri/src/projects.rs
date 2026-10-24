//! Projects (PLAN P4): add, edit, reorder, hide, archive and delete. Inputs are checked here; rows
//! live in `store::projects`.

use std::path::Path;
use std::sync::Arc;

use chorus_core::projects::{self as pj, ProjectState};
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
}

impl From<ProjectRow> for Project {
    fn from(r: ProjectRow) -> Self {
        Self { id: r.id, name: r.name, folder: r.folder, colour: r.colour, state: r.state }
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

fn all(store: &Store) -> Result<Vec<Project>, String> {
    Ok(rows::list(store)?.into_iter().map(Project::from).collect())
}

fn clean_folder(folder: Option<&str>) -> Result<Option<String>, String> {
    let Some(f) = folder.map(str::trim).filter(|f| !f.is_empty()) else {
        return Err("Choose a folder for this project.".into());
    };
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
    // Resolve dot segments, junctions and short Windows names before storing the folder.
    // Keep drive roots intact and use the normal spelling for session folder matching.
    let canonical = p.canonicalize().map_err(|e| format!("Couldn't resolve that folder: {e}"))?;
    let canonical = canonical.to_string_lossy();
    let display = if let Some(unc) = canonical.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        canonical.strip_prefix(r"\\?\").unwrap_or(&canonical).to_owned()
    };
    Ok(Some(display))
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
            rows::insert(store, &NewProject { name: &name, folder: folder.as_deref(), colour: &colour })?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_folder_requires_an_existing_absolute_directory() {
        for folder in [None, Some(""), Some("  "), Some("relative"), Some("C:\\bad\npath")] {
            assert!(clean_folder(folder).is_err());
        }
        let file = std::env::current_exe().unwrap();
        assert!(clean_folder(file.to_str()).is_err());
        assert!(clean_folder(file.join("missing-folder").to_str()).is_err());
        let temp = std::env::temp_dir();
        assert_eq!(clean_folder(temp.to_str()).unwrap(), clean_folder(temp.join(".").to_str()).unwrap());
        let folder = clean_folder(temp.to_str()).unwrap().unwrap();
        assert!(Path::new(&folder).is_absolute() && Path::new(&folder).is_dir());
        let root = temp.ancestors().last().unwrap();
        let root = clean_folder(root.to_str()).unwrap().unwrap();
        assert!(Path::new(&root).is_absolute() && Path::new(&root).is_dir(), "drive root must keep its slash");
    }
}
