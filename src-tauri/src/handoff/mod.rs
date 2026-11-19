//! P18: working alongside OpenMausBot ([HANDOFF-P18-mausbot.md](../../../_docs/HANDOFF-P18-mausbot.md)).
//!
//! The hand-off note (18-1, D128): `.chorus/handoff.md` in each project folder says what was done
//! last, where things stand and what's next. This module's own subscription to session summaries
//! (`observe`) only queues; the `handoff` thread waits 3 s for a project to settle (K3), then
//! rewrites its note's "latest work" entry.

pub mod note;
pub mod refresh;

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use chorus_core::activity::Activity;
use chorus_core::handoff::{self, Author, Baton};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use self::note::IgnoreState;
use self::refresh::Scrubber;
use crate::sessions::{SessionManager, SessionStatus, SessionSummary};

type Manager<'a> = State<'a, Arc<SessionManager>>;

/// How long a project waits after its last trigger before its note is rewritten (K3).
const SETTLE: Duration = Duration::from_secs(3);

static QUEUE: OnceLock<Mutex<Sender<(u32, u32)>>> = OnceLock::new();
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
/// Each session's last seen (working, closed), to see a turn end or a close.
static LAST: Mutex<Option<HashMap<u32, (bool, bool)>>> = Mutex::new(None);
/// Why a project's last refresh failed, until one succeeds.
static PROBLEMS: Mutex<Option<HashMap<u32, String>>> = Mutex::new(None);

/// A project's note changed (or couldn't be written): its card reads the view again.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct HandoffChanged {
    pub project_id: u32,
}

fn emit(project_id: u32) {
    if let Some(app) = APP.get() {
        let _ = HandoffChanged { project_id }.emit(app);
    }
}

fn set_problem(project_id: u32, problem: Option<String>) {
    let mut guard = PROBLEMS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let map = guard.get_or_insert_with(HashMap::new);
    match problem {
        Some(p) => {
            map.insert(project_id, p);
        }
        None => {
            map.remove(&project_id);
        }
    }
}

fn problem_of(project_id: u32) -> Option<String> {
    PROBLEMS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().and_then(|m| m.get(&project_id).cloned())
}

/// Every session summary, from `lib.rs`'s notify closure. Never blocks and does no IO. Queues the
/// session's project when a turn ends (activity leaves Working) or the session closes. Loop steps
/// never do: the loop keeps its own documents.
pub fn observe(s: &SessionSummary) {
    let Some(project) = s.project_id else { return };
    if s.tags.iter().any(|t| t == "loop") {
        return;
    }
    let now = (s.activity == Activity::Working, matches!(s.status, SessionStatus::Closed));
    let before = {
        let mut guard = LAST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.get_or_insert_with(HashMap::new).insert(s.id, now)
    };
    if triggers(before, now)
        && let Some(q) = QUEUE.get()
    {
        let _ = q.lock().unwrap_or_else(std::sync::PoisonError::into_inner).send((project, s.id));
    }
}

/// Whether a session's change from `before` to `now` (each `(working, closed)`) should refresh the
/// note: a turn ended, or the session closed. A session seen for the first time never does, so
/// the summaries published at start-up don't rewrite every note.
fn triggers(before: Option<(bool, bool)>, now: (bool, bool)) -> bool {
    let Some((was_working, was_closed)) = before else { return false };
    (was_working && !now.0) || (now.1 && !was_closed)
}

pub fn start(m: &Arc<SessionManager>, app: tauri::AppHandle) -> Result<(), String> {
    let _ = APP.set(app);
    let (tx, rx) = channel();
    let _ = QUEUE.set(Mutex::new(tx));
    let worker = m.clone();
    std::thread::Builder::new().name("handoff".into()).spawn(move || run(&worker, &rx)).map_err(|e| e.to_string())?;
    Ok(())
}

fn run(m: &Arc<SessionManager>, rx: &Receiver<(u32, u32)>) {
    // Project → (the session that last ended a turn there, when to refresh).
    let mut pending: HashMap<u32, (u32, Instant)> = HashMap::new();
    while !m.is_quitting() {
        let soonest = pending.values().map(|(_, due)| due.saturating_duration_since(Instant::now())).min();
        match rx.recv_timeout(soonest.unwrap_or(Duration::from_secs(1)).min(Duration::from_secs(1))) {
            Ok((project, session)) => {
                pending.insert(project, (session, Instant::now() + SETTLE));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        let now = Instant::now();
        let due: Vec<(u32, u32)> = pending.iter().filter(|(_, (_, at))| *at <= now).map(|(p, (s, _))| (*p, *s)).collect();
        for (project, session) in due {
            pending.remove(&project);
            let outcome = refresh::refresh(m, project, Some(session));
            set_problem(project, outcome.err());
            emit(project);
        }
    }
}

/// One entry, scrubbed, for the project card.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EntryView {
    pub heading: String,
    pub author: Author,
    pub latest_work: bool,
    pub done: Option<String>,
    pub state: Option<String>,
    pub open: Option<String>,
    pub next: Option<String>,
    pub other: Vec<String>,
}

impl EntryView {
    fn of(e: &handoff::Entry, s: &Scrubber) -> Self {
        let f = e.fields();
        let clean = |v: Option<String>| v.map(|t| s.clean(&t));
        Self {
            heading: s.clean(e.heading()),
            author: e.author(),
            latest_work: e.is_latest_work(),
            done: clean(f.done),
            state: clean(f.state),
            open: clean(f.open),
            next: clean(f.next),
            other: f.other.iter().map(|o| s.clean(o)).collect(),
        }
    }
}

/// A project's hand-off, as its card shows it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HandoffView {
    pub project_id: u32,
    pub has_folder: bool,
    /// The note's full path, for a tooltip only.
    pub note_path: Option<String>,
    pub exists: bool,
    pub problem: Option<String>,
    pub ignored: Option<IgnoreState>,
    pub baton: Baton,
    pub updated: Option<String>,
    pub entries: Vec<EntryView>,
}

/// Reads the note fresh, so a hand edit shows at once. A note that won't parse is a problem on the
/// card, never an error: the card still shows.
pub(crate) fn view(m: &SessionManager, project_id: u32) -> Result<HandoffView, String> {
    let (_, folder) = refresh::project_folder(m, project_id)?;
    let mut v = HandoffView {
        project_id,
        has_folder: folder.is_some(),
        note_path: None,
        exists: false,
        problem: problem_of(project_id),
        ignored: None,
        baton: Baton::Nobody,
        updated: None,
        entries: Vec::new(),
    };
    let Some(folder) = folder else { return Ok(v) };
    let path = note::note_path(&folder);
    v.note_path = Some(path.display().to_string());
    v.ignored = note::ignore_state(&folder).ok();
    match note::read(&path) {
        Ok(None) => {}
        Ok(Some(text)) => {
            v.exists = true;
            match handoff::parse(&text) {
                Ok(n) => {
                    let s = Scrubber::new(m)?;
                    v.baton = n.meta.baton;
                    v.updated = n.meta.updated.clone();
                    v.entries = n.entries.iter().take(handoff::MAX_ENTRIES).map(|e| EntryView::of(e, &s)).collect();
                }
                Err(e) => v.problem = Some(format!("Chorus can't read the note, so it leaves it as it is: {e}")),
            }
        }
        Err(e) => {
            v.exists = Path::new(&path).exists();
            v.problem = Some(e);
        }
    }
    Ok(v)
}

async fn blocking<T: Send + 'static>(m: &Manager<'_>, f: impl FnOnce(Arc<SessionManager>) -> Result<T, String> + Send + 'static) -> Result<T, String> {
    let m = m.inner().clone();
    tauri::async_runtime::spawn_blocking(move || f(m)).await.map_err(|e| e.to_string())?
}

/// The project's note, as the card shows it.
#[tauri::command]
#[specta::specta]
pub async fn handoff_view(state: Manager<'_>, project_id: u32) -> Result<HandoffView, String> {
    blocking(&state, move |m| view(&m, project_id)).await
}

/// Rewrites the note's "latest work" entry now, from the project's session that worked last.
#[tauri::command]
#[specta::specta]
pub async fn handoff_refresh(state: Manager<'_>, project_id: u32) -> Result<HandoffView, String> {
    blocking(&state, move |m| {
        let outcome = refresh::refresh(&m, project_id, None);
        set_problem(project_id, outcome.as_ref().err().cloned());
        emit(project_id);
        outcome?;
        view(&m, project_id)
    })
    .await
}

/// Opens the note in its default app.
#[tauri::command]
#[specta::specta]
pub async fn handoff_open_note(app: tauri::AppHandle, state: Manager<'_>, project_id: u32) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let path = blocking(&state, move |m| {
        let (_, folder) = refresh::project_folder(&m, project_id)?;
        let path = note::note_path(&folder.ok_or("This project has no folder.")?);
        if path.is_file() { Ok(path) } else { Err("There's no note yet: it's written when a session in the project ends a turn.".into()) }
    })
    .await?;
    app.opener().open_path(path.display().to_string(), None::<&str>).map_err(|e| format!("opening the note: {e}"))
}

#[cfg(test)]
mod tests;
