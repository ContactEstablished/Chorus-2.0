//! Documents (P8): the plans, specs and reports sessions wrote, linked to the session that wrote
//! each and inheriting its project. A pass over new `tool_calls` rows (no recorder change) finds
//! files a session wrote inside its own folder that `chorus_core::documents` accepts, and reads
//! only each file's first heading. Opening goes through `tauri-plugin-opener` from Rust, after
//! checking the path is still inside the session's folder or its project's.

use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chorus_core::documents::{DocKind, TITLE_SCAN_BYTES, classify, title};
use chorus_core::folders;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::sessions::SessionManager;
use crate::store::{Store, now_ms, settings};
use crate::vault::commands::secs;

type Manager<'a> = State<'a, Arc<SessionManager>>;

const CURSOR_KEY: &str = "documents.scan_cursor";
/// Tools whose target is a file they wrote: Claude Code's.
const WRITING_TOOLS: &[&str] = &["Write", "Edit", "MultiEdit", "NotebookEdit"];
const PASS_EVERY: Duration = Duration::from_secs(5);
const BATCH: u32 = 500;
/// A write still under way stops the pass, so it is seen once finished, unless it is this old.
const UNFINISHED_GRACE_MS: i64 = 5 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub id: u32,
    pub path: String,
    /// The file's name, for a document without a heading.
    pub file_name: String,
    pub title: Option<String>,
    pub kind: DocKind,
    pub session_id: Option<u32>,
    pub session_name: Option<String>,
    pub project_id: Option<u32>,
    pub first_seen_at_secs: u32,
    /// The file's own modified time, while it exists.
    pub edited_at_secs: Option<u32>,
    pub missing: bool,
}

fn first_heading(path: &Path) -> Option<String> {
    let mut buf = Vec::new();
    std::fs::File::open(path).ok()?.take(TITLE_SCAN_BYTES as u64).read_to_end(&mut buf).ok()?;
    title(&String::from_utf8_lossy(&buf))
}

struct Written {
    id: i64,
    session_id: u32,
    target: Option<String>,
    started_at: i64,
    ended: bool,
    cwd: String,
    project_id: Option<u32>,
}

/// One pass over tool calls recorded since the last. Returns how many documents it saw.
pub fn scan(store: &Store) -> Result<u32, String> {
    let cursor: i64 = settings::get(store, CURSOR_KEY)?.and_then(|v| v.parse().ok()).unwrap_or(0);
    let tools = WRITING_TOOLS.iter().map(|t| format!("'{t}'")).collect::<Vec<_>>().join(",");
    let rows: Vec<Written> = store.with(|c| {
        let mut stmt = c.prepare(&format!(
            "SELECT t.id, t.session_id, t.target, t.started_at, t.ended_at IS NOT NULL, s.cwd, s.project_id
             FROM tool_calls t JOIN sessions s ON s.id = t.session_id
             WHERE t.id > ?1 AND t.tool IN ({tools}) AND t.failed = 0 ORDER BY t.id LIMIT ?2"
        ))?;
        stmt.query_map(params![cursor, BATCH], |r| {
            Ok(Written { id: r.get(0)?, session_id: r.get(1)?, target: r.get(2)?, started_at: r.get(3)?, ended: r.get(4)?, cwd: r.get(5)?, project_id: r.get(6)? })
        })?
        .collect()
    })?;
    let now = now_ms();
    let mut last = cursor;
    let mut seen = 0;
    for w in rows {
        if !w.ended && now - w.started_at < UNFINISHED_GRACE_MS {
            break;
        }
        last = w.id;
        let Some(target) = w.target.as_deref().map(str::trim).filter(|t| !t.is_empty()) else { continue };
        let path = if Path::new(target).is_absolute() { target.to_owned() } else { Path::new(&w.cwd).join(target).display().to_string() };
        let Some(relative) = folders::relative(&path, &w.cwd) else { continue };
        let Some(kind) = classify(&relative) else { continue };
        if !Path::new(&path).is_file() {
            continue;
        }
        let heading = first_heading(Path::new(&path));
        store.with(|c| {
            c.execute(
                "INSERT INTO documents (session_id, project_id, path, kind, title, first_seen_at, last_seen_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
                 ON CONFLICT (path) DO UPDATE SET kind = excluded.kind, title = excluded.title,
                   last_seen_at = excluded.last_seen_at, missing_at = NULL",
                params![w.session_id, w.project_id, path, kind.as_str(), heading, now],
            )
        })?;
        seen += 1;
    }
    if last != cursor {
        settings::set(store, CURSOR_KEY, &last.to_string())?;
    }
    Ok(seen)
}

/// The pass runs every few seconds on its own thread, so turns don't wait for it.
pub fn start(manager: &Arc<SessionManager>) -> Result<(), String> {
    let m = manager.clone();
    std::thread::Builder::new()
        .name("documents".into())
        .spawn(move || {
            while !m.is_quitting() {
                if let Err(e) = scan(m.store()) {
                    eprintln!("chorus: documents: {e}");
                }
                std::thread::sleep(PASS_EVERY);
            }
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn modified_secs(path: &Path) -> Option<u32> {
    let t = std::fs::metadata(path).ok()?.modified().ok()?;
    let ms = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis();
    Some(secs(i64::try_from(ms).ok()?))
}

pub fn list(store: &Store) -> Result<Vec<Document>, String> {
    type Raw = (u32, String, String, Option<String>, Option<u32>, Option<String>, Option<u32>, i64, Option<i64>);
    let rows: Vec<Raw> = store.with(|c| {
        let mut stmt = c.prepare(
            "SELECT d.id, d.path, d.kind, d.title, d.session_id, s.name, d.project_id, d.first_seen_at, d.missing_at
             FROM documents d LEFT JOIN sessions s ON s.id = d.session_id ORDER BY d.last_seen_at DESC, d.id DESC",
        )?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)))?.collect()
    })?;
    let now = now_ms();
    let mut out = Vec::new();
    for (id, path, kind, title, session_id, session_name, project_id, first_seen, missing_at) in rows {
        let p = Path::new(&path);
        let edited = modified_secs(p);
        let missing = !p.is_file();
        if missing != missing_at.is_some() {
            let at = missing.then_some(now);
            let _ = store.with(|c| c.execute("UPDATE documents SET missing_at = ?2 WHERE id = ?1", params![id, at]));
        }
        out.push(Document {
            id,
            file_name: p.file_name().map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned()),
            path,
            title,
            kind: DocKind::parse(&kind).unwrap_or(DocKind::Plan),
            session_id,
            session_name,
            project_id,
            first_seen_at_secs: secs(first_seen),
            edited_at_secs: edited.filter(|_| !missing),
            missing,
        });
    }
    Ok(out)
}

/// The path to open, checked: the document exists, and it is still inside its session's folder
/// or its project's.
fn openable(store: &Store, id: u32) -> Result<String, String> {
    let (path, session, project): (String, Option<u32>, Option<u32>) = store
        .with(|c| c.query_row("SELECT path, session_id, project_id FROM documents WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional())?
        .ok_or("That document is no longer listed.")?;
    let cwd = session.and_then(|s| crate::store::sessions::get(store, s).ok().flatten()).map(|s| s.cwd);
    let folder = project.and_then(|p| crate::store::projects::get(store, p).ok().flatten()).and_then(|p| p.folder);
    let inside = [cwd, folder].into_iter().flatten().any(|f| folders::inside(&path, &f));
    if !inside {
        return Err("That document is outside its session's folder and project, so Chorus won't open it.".into());
    }
    if !Path::new(&path).is_file() {
        return Err("That document is missing: it was moved or deleted.".into());
    }
    Ok(path)
}

#[tauri::command]
#[specta::specta]
pub fn documents_list(state: Manager<'_>) -> Result<Vec<Document>, String> {
    list(state.store())
}

/// Opens a document in the application Windows associates with it.
#[tauri::command]
#[specta::specta]
pub fn document_open(app: tauri::AppHandle, state: Manager<'_>, id: u32) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = openable(state.store(), id)?;
    app.opener().open_path(path, None::<&str>).map_err(|e| format!("opening the document: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_a_session_wrote_is_listed_once_and_marked_missing_when_gone() {
        let dir = std::env::temp_dir().join(format!("chorus-docs-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("_docs")).unwrap();
        let plan = dir.join("_docs").join("PLAN-test.md");
        std::fs::write(&plan, "# Test plan\n\nBody").unwrap();
        std::fs::write(dir.join("main.rs"), "fn main() {}").unwrap();
        let s = Store::in_memory();
        let cwd = dir.display().to_string();
        s.with(|c| {
            c.execute("INSERT INTO sessions (id,name,agent,cwd,state,cols,rows,created_at) VALUES (1,'Writer','claude',?1,'running',80,24,1)", [&cwd])?;
            for (id, tool, target) in [(1, "Write", plan.display().to_string()), (2, "Write", dir.join("main.rs").display().to_string()), (3, "Read", plan.display().to_string()), (4, "Edit", plan.display().to_string())] {
                c.execute("INSERT INTO tool_calls (id,session_id,tool,target,started_at,ended_at) VALUES (?1,1,?2,?3,1,2)", params![id, tool, target])?;
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(scan(&s).unwrap(), 2);
        assert_eq!(scan(&s).unwrap(), 0);
        let docs = list(&s).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!((docs[0].title.as_deref(), docs[0].kind, docs[0].session_name.as_deref()), (Some("Test plan"), DocKind::Plan, Some("Writer")));
        assert_eq!(openable(&s, docs[0].id).unwrap(), plan.display().to_string());
        std::fs::remove_file(&plan).unwrap();
        assert!(list(&s).unwrap()[0].missing);
        assert!(openable(&s, docs[0].id).unwrap_err().contains("missing"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
