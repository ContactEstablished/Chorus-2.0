//! P18 (18-2, D130): handing a project to OpenMausBot on its own branch, watching for the baton to
//! come back, and the return.
//!
//! Hand to MausBot makes a `maus/<date>-<slug>` worktree from the project's checked-out commit,
//! writes the hand-off entry to the worktree's own copy of the note (K8) and then to the project's,
//! and starts the conversation in MausBot when Chorus is paired, or leaves Matthew the message to
//! start it by hand. The watch reads the worktree's copy every 5 s; `baton: chorus` there (or Take
//! back) returns it. The return claims the hand-off first, so it happens once, keeps the entries it
//! brought back (MausBot's, or Chorus's own from git, K12) on the hand-off's row, then adds them to
//! the project's note.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chorus_core::handoff::{self, Baton, Entry, MAX_ENTRIES};
use chorus_core::maus::{self, Commit, FileChange, HandoffFacts};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::EntryView;
use super::connection;
use super::maus_client::{self as client, Failure};
use super::note;
use super::refresh::{self, Scrubber};
use crate::git;
use crate::sessions::SessionManager;
use crate::store::{self, handoff as rows, handoff::HandoffRow, handoff::NewHandoff};

const COMMIT_LIMIT: usize = 20;
const FILE_LIMIT: usize = 200;
const TASK_CHARS: usize = 200;

/// Why a hand-off's worktree note can't be read, or its return failed, by hand-off id.
static NOTE_PROBLEMS: Mutex<Option<HashMap<u32, String>>> = Mutex::new(None);
/// What MausBot last said about each hand-off's conversation, by hand-off id.
static ACTIVITY: Mutex<Option<HashMap<u32, ActivityView>>> = Mutex::new(None);

fn note_problem(id: u32) -> Option<String> {
    NOTE_PROBLEMS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().and_then(|m| m.get(&id).cloned())
}

/// Records a problem (or clears it). Returns whether it changed.
fn set_note_problem(id: u32, problem: Option<String>) -> bool {
    let mut guard = NOTE_PROBLEMS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let map = guard.get_or_insert_with(HashMap::new);
    let before = map.get(&id).cloned();
    match &problem {
        Some(p) => {
            map.insert(id, p.clone());
        }
        None => {
            map.remove(&id);
        }
    }
    before != problem
}

fn activity_of(id: u32) -> Option<ActivityView> {
    ACTIVITY.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref().and_then(|m| m.get(&id).cloned())
}

/// What MausBot says about the hand-off's conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActivityView {
    pub found: bool,
    pub busy: bool,
    pub updated_at_ms: Option<f64>,
    pub problem: Option<String>,
}

fn base_label(h: &HandoffRow) -> String {
    format!("{} @ {}", h.base_branch.as_deref().unwrap_or("detached"), short(&h.base_sha))
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

fn offset(m: &SessionManager, at: i64) -> i32 {
    crate::automations::offset_at(m.store(), at)
}

/// What starting MausBot itself came to: the conversation, if Chorus made one, and why Matthew has
/// to finish the start by hand, if he does.
fn start_mausbot(m: &SessionManager, scrubber: &Scrubber, id: u32, bot_id: Option<&str>, worktree: &str, title: &str, message: &str) -> (Option<String>, Option<String>) {
    let token = match (connection::token(m), bot_id) {
        (_, None) => return (None, Some("No bot chosen, so start the conversation in MausBot yourself.".into())),
        (Ok(None), Some(_)) => return (None, Some("Chorus isn't paired with MausBot (Settings → MausBot), so start the conversation yourself.".into())),
        (Err(e), Some(_)) => return (None, Some(scrubber.clean(&format!("Chorus can't read its MausBot key ({e}), so start the conversation yourself.")))),
        (Ok(Some(t)), Some(_)) => t,
    };
    let bot = bot_id.unwrap_or_default();
    let thread = match client::create_conversation(&token, bot, worktree, title) {
        Ok(thread) => thread,
        Err(Failure::Timeout) => {
            return (None, Some("MausBot didn't answer in time, so it may have made the conversation anyway. Check MausBot before you start one yourself.".into()))
        }
        Err(f) => return (None, Some(scrubber.clean(&format!("{} Start the conversation yourself.", f.text())))),
    };
    let nonce: String = crate::vault::random_bytes(8).iter().map(|b| format!("{b:02x}")).collect();
    match client::send(&token, bot, &thread, message, &maus::send_id(id, &nonce)) {
        Ok(()) => (Some(thread), None),
        Err(Failure::Timeout) => (
            Some(thread),
            Some("MausBot didn't answer in time, so the message may already have arrived. Check the conversation before you paste it.".into()),
        ),
        Err(f) => (Some(thread), Some(scrubber.clean(&format!("Chorus made the conversation but couldn't send the message: {} Paste it yourself.", f.text())))),
    }
}

/// Hands the project to MausBot. Everything up to the project's note is all or nothing; starting
/// MausBot itself can fail, and then the hand-off stays, with the reason and the message to paste.
pub fn start(m: &SessionManager, project_id: u32, bot_id: Option<String>, task: &str) -> Result<u32, String> {
    let task = task.trim();
    if task.chars().count() > TASK_CHARS || task.contains(['\n', '\r']) {
        return Err(format!("Keep the task to one line of at most {TASK_CHARS} characters."));
    }
    if bot_id.as_deref().is_some_and(|b| !maus::valid_id(b)) {
        return Err("That isn't a MausBot bot.".into());
    }
    let store = m.store();
    let scrubber = Scrubber::new(m)?;
    // The task goes into the note, the database and MausBot: scrubbed before any of them.
    let task = scrubber.clean(task);
    let (project, folder) = refresh::project_folder(m, project_id)?;
    let project = scrubber.clean(&project);
    let folder = folder.ok_or("Set a folder for this project first.")?;
    let facts = git::facts(&folder)?
        .ok_or("This project's folder isn't a git repository, so Chorus can't give MausBot a branch of its own.")?;
    if rows::active(store, project_id)?.is_some() {
        return Err("This project is already with MausBot. Take it back first.".into());
    }
    if let Some(text) = note::read(&note::note_path(&folder))? {
        handoff::parse(&text).map_err(|e| format!("Fix the hand-off note first: {e}"))?;
    }
    let base_sha = git::rev(&folder, "HEAD")?.ok_or("The project has no commits yet, so there's nothing for MausBot to start from.")?;
    let base_branch = facts.branch.clone();
    let main_root = PathBuf::from(&facts.main_root);
    let now = store::now_ms();
    let offset = offset(m, now);
    let plan = maus::plan(&facts.main_root, &handoff::day(now, offset), &task, &|branch, path| {
        git::rev(&main_root, &format!("refs/heads/{branch}")).ok().flatten().is_some() || Path::new(path).exists()
    })?;
    git::add_worktree(&main_root, &plan.path, &plan.branch, Some(&base_sha))?;
    let undo = |e: String| -> String {
        let _ = git::remove_worktree(&main_root, &plan.path);
        let _ = git::delete_merged_branch(&main_root, &plan.branch);
        e
    };
    let id = rows::insert(store, &NewHandoff {
        project_id,
        task: &task,
        bot_id: bot_id.as_deref(),
        branch: &plan.branch,
        worktree: &plan.path,
        base_branch: base_branch.as_deref(),
        base_sha: &base_sha,
        started_at: now,
    })
    .map_err(undo)?;

    let bots = bot_id.as_ref().and_then(|_| client::bots().ok());
    let bot_name = bot_id
        .as_ref()
        .and_then(|b| bots.as_ref()?.iter().find(|x| &x.id == b))
        .map(|b| scrubber.clean(&b.name));
    let base = format!("{} @ {}", base_branch.as_deref().unwrap_or("detached"), short(&base_sha));
    // The worktree's copy first, then the project's note, both under the note lock: a failure
    // leaves the project's note as it was.
    let written = refresh::update_note(&folder, |current| {
        let latest = current.as_ref().and_then(|n| n.entries.first()).map(Entry::fields);
        let hf = HandoffFacts {
            at_ms: now,
            offset_minutes: offset,
            project: &project,
            branch: &plan.branch,
            worktree: &plan.path,
            base: &base,
            task: &task,
            bot: bot_name.as_deref(),
            latest: latest.as_ref(),
        };
        let mut n = handoff::prepend(current.unwrap_or_else(|| handoff::new_note(&project)), vec![scrubber.clean(&maus::handoff_entry(&hf))]);
        n.meta.project = project.clone();
        n.meta.baton = Baton::Mausbot;
        n.meta.branch = Some(plan.branch.clone());
        n.meta.worktree = Some(plan.path.clone());
        n.meta.base = Some(base.clone());
        n.meta.updated = Some(chorus_core::providers::iso_utc_secs(now));
        let (copy, _) = handoff::bound(n.clone());
        note::write_atomic(&note::note_path(Path::new(&plan.path)), &handoff::render(&copy))?;
        Ok((n, ()))
    });
    if let Err(e) = written {
        let _ = rows::delete(store, id);
        return Err(undo(e));
    }
    // The returns before this one are superseded: their branches stay for 18-3 to clean up.
    let _ = rows::close_returned(store, project_id, now);
    let hf = HandoffFacts {
        at_ms: now,
        offset_minutes: offset,
        project: &project,
        branch: &plan.branch,
        worktree: &plan.path,
        base: &base,
        task: &task,
        bot: bot_name.as_deref(),
        latest: None,
    };
    let message = maus::message(&hf);
    let (thread, problem) = start_mausbot(m, &scrubber, id, bot_id.as_deref(), &plan.path, &maus::task_title(&project, &task), &message);
    rows::set_started(store, id, bot_id.as_deref(), bot_name.as_deref(), thread.as_deref(), problem.as_deref())?;
    Ok(id)
}

/// One pass of the watch: each active hand-off's worktree note, and, when `with_activity`,
/// MausBot's view of its conversation. Returns the projects whose view changed. A return that
/// fails says why on the banner and is tried again next pass.
pub fn watch(m: &SessionManager, with_activity: bool) -> Vec<u32> {
    let Ok(active) = rows::all_active(m.store()) else { return Vec::new() };
    let mut changed = Vec::new();
    for h in &active {
        let path = note::note_path(Path::new(&h.worktree));
        let problem = match note::read(&path) {
            Ok(Some(text)) => match handoff::parse(&text) {
                Ok(n) if maus::handed_back(&n) => match return_(m, h, "mausbot") {
                    Ok(_) => {
                        changed.push(h.project_id);
                        continue;
                    }
                    Err(e) => Some(format!("MausBot handed back, but Chorus couldn't take it: {e}")),
                },
                Ok(_) => None,
                Err(e) => Some(format!("Chorus can't read the worktree's note: {e}")),
            },
            Ok(None) => Some("The worktree's note is missing, so Chorus can't see MausBot hand back. Use Take back.".into()),
            Err(e) => Some(e),
        };
        if set_note_problem(h.id, problem) {
            changed.push(h.project_id);
        }
    }
    if with_activity && !active.is_empty() {
        let bots = client::bots();
        let mut guard = ACTIVITY.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let map = guard.get_or_insert_with(HashMap::new);
        for h in &active {
            let seen = match &bots {
                Ok(list) => match maus::conversation(list, h.thread_id.as_deref(), &h.worktree) {
                    Some((_, c)) => ActivityView { found: true, busy: c.busy, updated_at_ms: c.updated_at_ms.map(|v| v as f64), problem: None },
                    None => ActivityView { found: false, busy: false, updated_at_ms: None, problem: None },
                },
                Err(f) => ActivityView { found: false, busy: false, updated_at_ms: None, problem: Some(f.text()) },
            };
            if map.get(&h.id) != Some(&seen) {
                map.insert(h.id, seen);
                changed.push(h.project_id);
            }
        }
    }
    changed.sort_unstable();
    changed.dedup();
    changed
}

/// The entries the return brings back: what MausBot wrote above the hand-off entry in its copy of
/// the note (at most `MAX_ENTRIES`), scrubbed, with Chorus's from-git entry on top when none of
/// them is the bot's. When git can't read the branch, the entry says so.
fn returned_entries(m: &SessionManager, h: &HandoffRow, scrubber: &Scrubber) -> Vec<String> {
    let heading = maus::handoff_heading(h.started_at, offset(m, h.started_at));
    let written: Vec<Entry> = note::read(&note::note_path(Path::new(&h.worktree)))
        .ok()
        .flatten()
        .and_then(|t| handoff::parse(&t).ok())
        .map(|n| maus::new_entries(&n, &heading))
        .unwrap_or_default();
    let written: Vec<Entry> = written.into_iter().take(MAX_ENTRIES).collect();
    let mut raws: Vec<String> = written.iter().map(|e| scrubber.clean(&e.raw)).collect();
    if !maus::has_bot_entry(&written) {
        let dir = git_dir(m, h);
        let now = store::now_ms();
        let entry = match (git::log_range(&dir, &h.base_sha, &h.branch, COMMIT_LIMIT), git::numstat(&dir, &h.base_sha, &h.branch)) {
            (Ok((commits, more_commits)), Ok(files)) => {
                let commits: Vec<Commit> = commits.into_iter().map(|(sha, subject)| Commit { sha, subject }).collect();
                let more_files = files.len().saturating_sub(FILE_LIMIT) as u32;
                let files: Vec<FileChange> = files.into_iter().take(FILE_LIMIT).map(|(path, added, removed)| FileChange { path, added, removed }).collect();
                maus::from_git_entry(now, offset(m, now), &h.branch, &base_label(h), &commits, more_commits, &files, more_files)
            }
            (Err(e), _) | (_, Err(e)) => maus::from_git_failure_entry(now, offset(m, now), &h.branch, &e),
        };
        raws.insert(0, scrubber.clean(&entry));
    }
    raws
}

/// Where git commands for a hand-off run: the project's folder, else the worktree.
fn git_dir(m: &SessionManager, h: &HandoffRow) -> PathBuf {
    refresh::project_folder(m, h.project_id).ok().and_then(|(_, f)| f).unwrap_or_else(|| PathBuf::from(&h.worktree))
}

/// The baton comes back. The hand-off is claimed first, so the watch and Take back can't both
/// return it; `false` when someone else already had. The entries are kept on the row, then added
/// to the project's note; when the note can't take them, the card still shows them and the note
/// card says why.
fn return_(m: &SessionManager, h: &HandoffRow, by: &str) -> Result<bool, String> {
    let scrubber = Scrubber::new(m)?;
    let now = store::now_ms();
    if !rows::set_returned(m.store(), h.id, by, now)? {
        return Ok(false);
    }
    set_note_problem(h.id, None);
    let raws = returned_entries(m, h, &scrubber);
    rows::set_returned_entries(m.store(), h.id, &serde_json::to_string(&raws).map_err(|e| e.to_string())?)?;
    let problem = match refresh::project_folder(m, h.project_id)? {
        (_, None) => Some("The project's folder is gone, so MausBot's entries are only on the return card.".to_owned()),
        (project, Some(folder)) => refresh::update_note(&folder, |current| {
            let mut n = handoff::prepend(current.unwrap_or_else(|| handoff::new_note(&project)), raws.clone());
            n.meta.baton = Baton::Chorus;
            n.meta.updated = Some(chorus_core::providers::iso_utc_secs(now));
            Ok((n, ()))
        })
        .err()
        .map(|e| format!("MausBot's entries are on the return card, but Chorus couldn't add them to the note: {e}")),
    };
    super::set_problem(h.project_id, problem);
    Ok(true)
}

/// Matthew takes the project back, whatever the worktree's note says.
pub fn take_back(m: &SessionManager, project_id: u32) -> Result<(), String> {
    let h = rows::active(m.store(), project_id)?.ok_or("This project isn't with MausBot.")?;
    if return_(m, &h, "chorus")? { Ok(()) } else { Err("MausBot handed it back already.".into()) }
}

/// Puts the return card away. The note's front matter forgets the branch first; the branch itself
/// stays until Matthew deletes it (18-3). A note that can't be updated keeps the card, with why.
pub fn dismiss(m: &SessionManager, project_id: u32) -> Result<(), String> {
    let h = rows::returned(m.store(), project_id)?.ok_or("There's no return to put away.")?;
    if let (_, Some(folder)) = refresh::project_folder(m, project_id)?
        && note::note_path(&folder).is_file()
    {
        refresh::update_note(&folder, |current| {
            let mut n = current.ok_or("The note went missing.")?;
            if n.meta.branch.as_deref() == Some(h.branch.as_str()) {
                n.meta.branch = None;
                n.meta.worktree = None;
                n.meta.base = None;
            }
            Ok((n, ()))
        })?;
    }
    rows::close(m.store(), h.id, store::now_ms())
}

/// The entries a return brought back, from its row.
fn kept_entries(h: &HandoffRow) -> Vec<Entry> {
    h.returned_entries
        .as_deref()
        .and_then(|j| serde_json::from_str::<Vec<String>>(j).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|raw| Entry { raw })
        .collect()
}

/// Whether the branch is merged into its base, and how many commits it has the base doesn't.
fn merged_and_ahead(m: &SessionManager, h: &HandoffRow) -> Result<(bool, u32), String> {
    let dir = git_dir(m, h);
    let ahead = git::count_range(&dir, &h.base_sha, &h.branch)?;
    let merged = match h.base_branch.as_deref() {
        Some(b) => git::is_ancestor(&dir, &h.branch, b)?,
        None => false,
    };
    Ok((merged, ahead))
}

/// What Continue from here puts in a session's composer.
pub fn continue_text(m: &SessionManager, project_id: u32) -> Result<String, String> {
    let h = rows::returned(m.store(), project_id)?.ok_or("There's no return to continue from.")?;
    let scrubber = Scrubber::new(m)?;
    let (project, _) = refresh::project_folder(m, project_id)?;
    let fields: Vec<handoff::Fields> = kept_entries(&h).iter().map(Entry::fields).collect();
    let base = h.base_branch.clone().unwrap_or_else(|| short(&h.base_sha).to_owned());
    let text = match merged_and_ahead(m, &h) {
        Ok((merged, ahead)) => maus::continue_text(&project, &h.branch, &h.worktree, &base, ahead, merged, &fields),
        Err(e) => format!(
            "{}\n\nChorus couldn't compare {} with {base}: {e}",
            maus::continue_text(&project, &h.branch, &h.worktree, &base, 0, false, &fields),
            h.branch
        ),
    };
    Ok(scrubber.clean(&text))
}

/// An active hand-off, as the banner shows it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ActiveView {
    pub id: u32,
    pub branch: String,
    pub worktree: String,
    pub base: String,
    pub task: String,
    pub bot_name: Option<String>,
    pub started_at_ms: f64,
    pub started_by_chorus: bool,
    pub start_problem: Option<String>,
    /// The message to paste, when Chorus didn't send it.
    pub message: Option<String>,
    pub note_problem: Option<String>,
    pub activity: Option<ActivityView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CommitView {
    pub sha: String,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    pub path: String,
    pub added: Option<u32>,
    pub removed: Option<u32>,
}

/// The newest return, as its card shows it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ReturnView {
    pub id: u32,
    pub branch: String,
    pub worktree: String,
    pub worktree_exists: bool,
    pub base: String,
    pub returned_by: String,
    pub returned_at_ms: f64,
    pub entries: Vec<EntryView>,
    pub from_git: bool,
    pub commits: Vec<CommitView>,
    pub more_commits: u32,
    pub files: Vec<FileView>,
    pub more_files: u32,
    pub ahead: u32,
    pub merged: bool,
    pub problem: Option<String>,
}

pub(super) fn active_view(m: &SessionManager, project_id: u32, s: &Scrubber) -> Result<Option<ActiveView>, String> {
    let Some(h) = rows::active(m.store(), project_id)? else { return Ok(None) };
    let (project, _) = refresh::project_folder(m, project_id)?;
    let base = base_label(&h);
    let message = h.start_problem.as_ref().map(|_| {
        s.clean(&maus::message(&HandoffFacts {
            at_ms: h.started_at,
            offset_minutes: 0,
            project: &project,
            branch: &h.branch,
            worktree: &h.worktree,
            base: &base,
            task: &h.task,
            bot: h.bot_name.as_deref(),
            latest: None,
        }))
    });
    Ok(Some(ActiveView {
        id: h.id,
        branch: h.branch.clone(),
        worktree: h.worktree.clone(),
        base,
        task: s.clean(&h.task),
        bot_name: h.bot_name.as_deref().map(|b| s.clean(b)),
        started_at_ms: h.started_at as f64,
        started_by_chorus: h.thread_id.is_some() && h.start_problem.is_none(),
        start_problem: h.start_problem.as_deref().map(|p| s.clean(p)),
        message,
        note_problem: note_problem(h.id).map(|p| s.clean(&p)),
        activity: activity_of(h.id).map(|mut a| {
            a.problem = a.problem.map(|p| s.clean(&p));
            a
        }),
    }))
}

pub(super) fn return_view(m: &SessionManager, project_id: u32, s: &Scrubber) -> Result<Option<ReturnView>, String> {
    let Some(h) = rows::returned(m.store(), project_id)? else { return Ok(None) };
    let dir = git_dir(m, &h);
    let mut problems: Vec<String> = Vec::new();
    let (commits, more_commits) = git::log_range(&dir, &h.base_sha, &h.branch, COMMIT_LIMIT).unwrap_or_else(|e| {
        problems.push(e);
        (Vec::new(), 0)
    });
    let files = git::numstat(&dir, &h.base_sha, &h.branch).unwrap_or_else(|e| {
        problems.push(e);
        Vec::new()
    });
    let more_files = files.len().saturating_sub(FILE_LIMIT) as u32;
    let (merged, ahead) = merged_and_ahead(m, &h).unwrap_or_else(|e| {
        problems.push(e);
        (false, 0)
    });
    let entries = kept_entries(&h);
    problems.dedup();
    Ok(Some(ReturnView {
        id: h.id,
        branch: h.branch.clone(),
        worktree: h.worktree.clone(),
        worktree_exists: Path::new(&h.worktree).is_dir(),
        base: base_label(&h),
        returned_by: h.returned_by.clone().unwrap_or_else(|| "chorus".into()),
        returned_at_ms: h.returned_at.unwrap_or(h.started_at) as f64,
        from_git: entries.first().is_some_and(|e| e.heading().contains("(from git)")),
        entries: entries.iter().map(|e| EntryView::of(e, s)).collect(),
        commits: commits.into_iter().map(|(sha, subject)| CommitView { sha, subject: s.clean(&subject) }).collect(),
        more_commits,
        files: files.into_iter().take(FILE_LIMIT).map(|(path, added, removed)| FileView { path: s.clean(&path), added, removed }).collect(),
        more_files,
        ahead,
        merged,
        problem: (!problems.is_empty()).then(|| s.clean(&format!("Chorus couldn't read {} fully: {}", h.branch, problems.join("; ")))),
    }))
}
