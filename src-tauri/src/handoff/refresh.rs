//! P18: rewriting a project's hand-off note after a session's turn ends or the session closes
//! (spec 18-1 §3.3). Everything shown comes from Chorus's own records and git, scrubbed, and a
//! note Chorus can't read is never overwritten (K4).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chorus_core::handoff::{self, GitLine, WorkFacts};
use chorus_core::progress::PlanStep;
use chorus_core::scrub;

use super::note::{self, IgnoreState};

/// One writer at a time for every project's note: the refresh, the baton's start, its return and
/// Dismiss each read, change and write a note, and two at once could lose one's change.
static NOTE_LOCK: Mutex<()> = Mutex::new(());
use crate::git;
use crate::sessions::SessionManager;
use crate::store;
use crate::vault;

/// Scrubs text with every vault key and the built-in patterns. Refuses when a key can't be read,
/// because a secret could then pass unredacted.
pub(super) struct Scrubber {
    keys: Vec<Vec<u8>>,
    patterns: Vec<scrub::Pattern>,
}

impl Scrubber {
    pub(super) fn new(m: &SessionManager) -> Result<Self, String> {
        let (keys, failed) = vault::all_for_scrubbing(m.store());
        if failed > 0 {
            return Err("A key in the vault couldn't be read, so Chorus can't scrub the note. Check Settings → Keys and lock.".into());
        }
        Ok(Self { keys, patterns: scrub::builtin_patterns() })
    }

    pub(super) fn clean(&self, text: &str) -> String {
        String::from_utf8_lossy(&scrub::scrub_all(text.as_bytes(), &self.keys, &self.patterns)).into_owned()
    }
}

/// Where a folder's checkout stands: branch, short head and uncommitted files. `None` when git
/// can't read it (not a repository, or gone).
fn checkout(dir: &Path) -> Option<(Option<String>, Option<String>, u32)> {
    let facts = git::facts(dir).ok()??;
    let head = git::rev(dir, "HEAD").ok().flatten().map(|sha| sha.chars().take(7).collect::<String>());
    let dirty = git::snapshot(dir).map(|s| s.changes.files).unwrap_or(0);
    Some((facts.branch, head, dirty))
}

/// The project's folder, when it has one that exists.
pub(super) fn project_folder(m: &SessionManager, project_id: u32) -> Result<(String, Option<PathBuf>), String> {
    let project = store::projects::get(m.store(), project_id)?.ok_or("That project no longer exists.")?;
    let folder = project.folder.map(PathBuf::from).filter(|f| f.is_dir());
    Ok((project.name, folder))
}

/// Rewrites the project's note with a fresh "latest work" entry from `session_id` (else the
/// project's session that worked last). Returns whether git ignores the note.
pub fn refresh(m: &SessionManager, project_id: u32, session_id: Option<u32>) -> Result<Option<IgnoreState>, String> {
    let (project, folder) = project_folder(m, project_id)?;
    let Some(folder) = folder else { return Ok(None) };
    // A broken note is reported before any git work, and checked again under the lock.
    if let Some(text) = note::read(&note::note_path(&folder))? {
        handoff::parse(&text).map_err(|e| format!("Chorus left the note as it is: {e}"))?;
    }
    let store = m.store();
    let session = match session_id {
        Some(s) => s,
        None => store::handoff::newest_session(store, project_id)?.ok_or("No session in this project has finished a turn yet.")?,
    };
    let row = store::sessions::get(store, session)?.ok_or("That session no longer exists.")?;
    let scrubber = Scrubber::new(m)?;
    let plan: Vec<PlanStep> = store::progress::latest_plan(store, session)?
        .unwrap_or_default()
        .into_iter()
        .map(|s| PlanStep { text: scrubber.clean(&s.text), ..s })
        .collect();
    let prompt = store::handoff::latest_prompt(store, session)?.map(|p| scrubber.clean(&p));
    let reply = store::handoff::latest_reply(store, session)?.map(|(t, at)| (scrubber.clean(&t), at));
    let checkout = checkout(Path::new(&row.cwd));
    let name = scrubber.clean(&row.name);
    let now = store::now_ms();
    let facts = WorkFacts {
        at_ms: now,
        offset_minutes: crate::automations::offset_at(store, now),
        session: &name,
        agent: row.agent.display_name(),
        plan: &plan,
        last_prompt: prompt.as_deref(),
        last_reply: reply.as_ref().map(|(t, at)| (t.as_str(), *at)),
        git: checkout.as_ref().map(|(branch, head, dirty)| GitLine { branch: branch.as_deref(), head: head.as_deref(), dirty_files: *dirty }),
    };
    let entry = scrubber.clean(&handoff::latest_work_entry(&facts));
    let project = scrubber.clean(&project);
    update_note(&folder, |current| {
        let mut note = handoff::with_latest_work(current.unwrap_or_else(|| handoff::new_note(&project)), entry);
        note.meta.project = project.clone();
        note.meta.updated = Some(chorus_core::providers::iso_utc_secs(now));
        Ok((note, ()))
    })
    .map(|((), ignore)| Some(ignore))
}

/// Reads the project's note, lets `change` make the new one, and writes it, holding the note lock
/// throughout. A note that won't parse is never written (K4).
pub(super) fn update_note<T>(folder: &Path, change: impl FnOnce(Option<handoff::Note>) -> Result<(handoff::Note, T), String>) -> Result<(T, IgnoreState), String> {
    let _one_writer = NOTE_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let current = match note::read(&note::note_path(folder))? {
        None => None,
        Some(text) => Some(handoff::parse(&text).map_err(|e| format!("Chorus left the note as it is: {e}"))?),
    };
    let (note, out) = change(current)?;
    Ok((out, write_note(folder, note)?))
}

/// Writes a note: git learns to ignore it first, then entries past the limit move to the archive,
/// then the note itself. A failed note write can leave moved entries in both files, never in
/// neither.
fn write_note(folder: &Path, note: handoff::Note) -> Result<IgnoreState, String> {
    let ignore = note::ensure_excluded(folder)?;
    let (note, moved) = handoff::bound(note);
    if !moved.is_empty() {
        note::append_archive(folder, &moved)?;
    }
    note::write_atomic(&note::note_path(folder), &handoff::render(&note))?;
    Ok(ignore)
}
