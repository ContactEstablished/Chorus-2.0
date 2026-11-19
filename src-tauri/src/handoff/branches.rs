//! P18 (18-3, D131): branch hygiene. A daily sweep flags each local branch with no commit for 30
//! days, once per stale spell, in the notification centre; the project page's Branches card offers
//! Keep (30 more days) and Delete. Chorus never deletes a branch on its own.
//!
//! Never flagged and never deleted: the default branch, the branch checked out in the repository's
//! main checkout or in any project's folder, and branches checked out in worktrees Chorus didn't
//! make. Left alone while in use: a branch or worktree that an open session, an active hand-off (any
//! project's) or a loop run still going is using.
//!
//! Delete acts only on what the card lists (stale branches and MausBot's), and only if the branch
//! still points at the commit the user was shown. It refuses a branch whose commits it couldn't
//! count, asks again before unmerged commits go, and removes the branch's worktree first, only
//! when the worktree holds nothing that would be lost (ignored files such as `.env` included).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use chorus_core::branch_hygiene::{self as hygiene, BranchFacts, Stale};
use chorus_core::folders::inside;
use chorus_core::inbox::Notice;
use chorus_core::projects::same_folder;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::Manager as _;

use super::refresh::{self, Scrubber};
use crate::git;
use crate::sessions::SessionManager;
use crate::store::{self, handoff as rows, sessions, worktrees};

/// Paths named when a worktree holds something that would be lost.
const NAMED_LEFT: usize = 3;

/// The sweep, Keep and Delete run one at a time, so a sweep never flags a branch a Delete is
/// removing.
static BRANCHES: Mutex<()> = Mutex::new(());

fn one_at_a_time() -> MutexGuard<'static, ()> {
    BRANCHES.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One branch on the project's Branches card.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BranchView {
    pub name: String,
    /// Its commit when Chorus looked. Delete acts only while it's still there.
    pub tip: String,
    pub last_commit_ms: f64,
    pub days: u32,
    pub merged: bool,
    /// Commits the default branch doesn't have; none when git couldn't count them.
    pub ahead: Option<u32>,
    /// Unused for 30 days and not kept.
    pub stale: bool,
    pub snoozed_until_ms: Option<f64>,
    /// The worktree it's checked out in, if any.
    pub worktree: Option<String>,
    pub maus: bool,
    /// Its name looked like a secret and is hidden, so the card can't act on it.
    pub redacted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BranchReport {
    pub problem: Option<String>,
    /// The branch the others are compared with.
    pub base: Option<String>,
    /// Stale branches (oldest first), then MausBot's branches.
    pub branches: Vec<BranchView>,
}

/// What git and Chorus know about a project's branches.
struct Facts {
    root: PathBuf,
    base: Option<String>,
    /// The branches checked out in the main checkout and in any project's folder.
    own: HashSet<String>,
    /// The main checkout and the worktrees that hold a project's folder: never removed.
    own_paths: Vec<String>,
    branches: Vec<BranchFacts>,
    /// Branch → the worktree it's checked out in (not the main checkout).
    worktree_of: HashMap<String, String>,
    /// Branch → its full commit.
    sha_of: HashMap<String, String>,
}

/// Whether a worktree is one Chorus made (sessions', loop runs' and MausBot's live in `.chorus2`).
fn chorus_made(path: &str) -> bool {
    path.to_ascii_lowercase().replace('/', "\\").contains("\\.chorus2\\")
}

/// The branches and folders something running is using, in any project: open sessions (their
/// folders, and their worktrees' branches), active hand-offs, and loop runs that aren't done or
/// stopped (one waiting weeks for a milestone review is still using its branch).
fn using(m: &SessionManager) -> Result<(HashSet<String>, Vec<String>), String> {
    let store = m.store();
    let mut branches = HashSet::new();
    let mut paths: Vec<String> = sessions::list_open(store)?.into_iter().map(|s| s.cwd).collect();
    for w in worktrees::list(store)?.into_iter().filter(|w| !w.removed) {
        if worktrees::other_open_sessions(store, w.id, 0)? > 0 {
            branches.insert(w.branch);
            paths.push(w.path);
        }
    }
    for h in rows::all_active(store)? {
        branches.insert(h.branch);
        paths.push(h.worktree);
    }
    for (branch, path) in rows::live_loop_runs(store)? {
        branches.insert(branch);
        paths.extend(path);
    }
    Ok((branches, paths))
}

fn gather(m: &SessionManager, project_id: u32, now: i64) -> Result<Option<Facts>, String> {
    let (_, folder) = refresh::project_folder(m, project_id)?;
    let Some(folder) = folder else { return Ok(None) };
    let Some(facts) = git::facts(&folder)? else { return Ok(None) };
    let root = PathBuf::from(&facts.main_root);
    let list = git::branches(&root)?;
    let trees = git::worktrees(&root)?;
    let names: HashSet<&str> = list.iter().map(|(n, _, _)| n.as_str()).collect();
    let origin = git::origin_head(&root)?;
    // git lists the main checkout first.
    let main = trees.first();
    let base = hygiene::default_branch(origin.as_deref(), &|b| names.contains(b), main.and_then(|t| t.branch.as_deref()));
    // Compared with the local branch, or with origin's when there's no local one.
    let base_ref = base.as_ref().map(|b| if names.contains(b.as_str()) { b.clone() } else { format!("origin/{b}") });
    let merged: HashSet<String> = match &base_ref {
        Some(b) => git::merged_into(&root, b)?.into_iter().collect(),
        None => HashSet::new(),
    };
    let mut own: HashSet<String> = main.and_then(|t| t.branch.clone()).into_iter().chain(facts.branch.clone()).collect();
    let mut own_paths: Vec<String> = main.map(|t| t.path.clone()).into_iter().chain([facts.top_level.clone()]).collect();
    // Every project's folder, archived ones too, is somebody's home: a worktree holding one is
    // never removed, and its branch never flagged.
    let homes: Vec<String> = store::projects::list(m.store())?.into_iter().filter_map(|p| p.folder).collect();
    let mut worktree_of = HashMap::new();
    let mut foreign: HashSet<String> = HashSet::new();
    for t in trees.iter().skip(1) {
        if homes.iter().any(|h| inside(h, &t.path)) {
            own_paths.push(t.path.clone());
            own.extend(t.branch.clone());
        }
        if let Some(b) = &t.branch {
            worktree_of.insert(b.clone(), t.path.clone());
            if !chorus_made(&t.path) {
                foreign.insert(b.clone());
            }
        }
    }
    let (busy_branches, busy_paths) = using(m)?;
    let mut sha_of = HashMap::new();
    let branches = list
        .into_iter()
        .map(|(name, at_secs, sha)| {
            sha_of.insert(name.clone(), sha);
            let last_commit_ms = at_secs * 1000;
            let is_merged = merged.contains(&name);
            // Counting is a git call per branch: only for the ones the card can show.
            let shown = name.starts_with("maus/") || now - last_commit_ms >= hygiene::STALE_AFTER_MS;
            let ahead = match &base_ref {
                Some(_) if is_merged => Some(0),
                Some(b) if shown => git::count_range(&root, b, &name).ok(),
                _ => None,
            };
            let in_use = busy_branches.contains(&name)
                || worktree_of.get(&name).is_some_and(|p| busy_paths.iter().any(|b| inside(b, p)));
            BranchFacts { checked_out: own.contains(&name) || foreign.contains(&name), in_use, merged: is_merged, ahead, last_commit_ms, name }
        })
        .collect();
    Ok(Some(Facts { root, base, own, own_paths, branches, worktree_of, sha_of }))
}

/// The stale branches, oldest first; none without a default branch to compare with.
fn stale_of(f: &Facts, now: i64, snoozed: &[(String, i64)]) -> Vec<Stale> {
    if f.base.is_some() { hygiene::stale(&f.branches, f.base.as_deref(), now, snoozed) } else { Vec::new() }
}

fn view_of(b: &BranchFacts, f: &Facts, now: i64, stale: bool, snoozed: Option<i64>, s: &Scrubber) -> BranchView {
    let name = s.clean(&b.name);
    BranchView {
        redacted: name != b.name,
        name,
        tip: f.sha_of.get(&b.name).cloned().unwrap_or_default(),
        last_commit_ms: b.last_commit_ms as f64,
        days: u32::try_from((now - b.last_commit_ms).max(0) / hygiene::DAY_MS).unwrap_or(u32::MAX),
        merged: b.merged,
        ahead: b.ahead,
        stale,
        snoozed_until_ms: snoozed.map(|t| t as f64),
        worktree: f.worktree_of.get(&b.name).map(|p| s.clean(p)),
        maus: b.name.starts_with("maus/"),
    }
}

const NO_BASE: &str = "Chorus can't tell which branch is this repository's main one (no origin/HEAD, main or master), so it doesn't flag branches here.";

/// The project's Branches card: stale branches, then MausBot's that nothing is using.
pub fn report(m: &SessionManager, project_id: u32) -> Result<BranchReport, String> {
    let now = store::now_ms();
    let s = Scrubber::new(m)?;
    let facts = match gather(m, project_id, now) {
        Ok(Some(f)) => f,
        Ok(None) => return Ok(BranchReport { problem: None, base: None, branches: Vec::new() }),
        Err(e) => return Ok(BranchReport { problem: Some(s.clean(&e)), base: None, branches: Vec::new() }),
    };
    let snoozed = rows::snoozes(m.store(), project_id)?;
    let stale = stale_of(&facts, now, &snoozed);
    let stale_names: HashSet<&str> = stale.iter().map(|x| x.name.as_str()).collect();
    let mut out: Vec<BranchView> = stale
        .iter()
        .filter_map(|x| facts.branches.iter().find(|b| b.name == x.name))
        .map(|b| view_of(b, &facts, now, true, None, &s))
        .collect();
    // A branch an active hand-off is on shows in the baton's banner instead.
    let maus = facts.branches.iter().filter(|b| b.name.starts_with("maus/") && !b.in_use && !b.checked_out && !stale_names.contains(b.name.as_str()));
    for b in maus {
        let until = snoozed.iter().find(|(n, t)| n == &b.name && *t > now).map(|(_, t)| *t);
        out.push(view_of(b, &facts, now, false, until, &s));
    }
    Ok(BranchReport {
        problem: facts.base.is_none().then(|| NO_BASE.to_owned()),
        base: facts.base.as_deref().map(|b| s.clean(b)),
        branches: out,
    })
}

/// Keep: the branch isn't flagged again for 30 days, and its notices are marked read.
pub fn keep(m: &SessionManager, project_id: u32, branch: &str) -> Result<(), String> {
    if !hygiene::valid_branch(branch) {
        return Err("That isn't a branch name.".into());
    }
    let _one = one_at_a_time();
    let now = store::now_ms();
    rows::snooze(m.store(), project_id, branch, now + hygiene::KEEP_MS)?;
    store::notifications::mark_read_by_key_prefix(m.store(), &format!("branch:{project_id}:{branch}:"), now)
}

/// Deletes a branch the card lists, and its worktree first (see the module's rules). `tip` is the
/// commit the user was shown; without one (the return card's Delete branch) only a merged branch
/// goes. Its reasons are scrubbed: they can name files and folders.
pub fn delete(m: &SessionManager, project_id: u32, branch: &str, tip: Option<&str>, confirmed: bool) -> Result<(), String> {
    let s = Scrubber::new(m)?;
    delete_checked(m, project_id, branch, tip, confirmed).map_err(|e| s.clean(&e))
}

fn delete_checked(m: &SessionManager, project_id: u32, branch: &str, tip: Option<&str>, confirmed: bool) -> Result<(), String> {
    if !hygiene::valid_branch(branch) {
        return Err("That isn't a branch name.".into());
    }
    let _one = one_at_a_time();
    let now = store::now_ms();
    let facts = gather(m, project_id, now)?.ok_or("This project's folder isn't a git repository.")?;
    let b = facts.branches.iter().find(|b| b.name == branch).ok_or_else(|| format!("There's no branch called {branch}."))?;
    if facts.base.as_deref() == Some(branch) {
        return Err(format!("{branch} is the default branch, so Chorus won't delete it."));
    }
    if facts.own.contains(branch) {
        return Err(format!("{branch} is checked out in a project's folder or the repository's main checkout, so Chorus won't delete it."));
    }
    if b.checked_out {
        return Err(format!("{branch} is checked out in a worktree Chorus didn't make, so delete it with git."));
    }
    if b.in_use {
        return Err(format!("A session, a hand-off or a loop run is using {branch}."));
    }
    let snoozed = rows::snoozes(m.store(), project_id)?;
    let stale = stale_of(&facts, now, &snoozed).iter().any(|s| s.name == branch);
    if !hygiene::deletable(branch, stale) {
        return Err("Chorus only deletes the branches it lists: ones unused for 30 days, and MausBot's.".into());
    }
    let current = facts.sha_of.get(branch).ok_or_else(|| format!("Chorus couldn't read {branch}'s commit."))?;
    match tip {
        Some(t) if t != current => {
            return Err(format!("{branch} has changed since Chorus showed it to you, so it was left alone. Look at it again, then decide."));
        }
        None if !b.merged => return Err(format!("{branch} has commits that aren't merged. Delete it from the Branches card, which shows them.")),
        _ => {}
    }
    hygiene::delete_mode(branch, b.merged, b.ahead, confirmed)?;
    if let Some(path) = facts.worktree_of.get(branch) {
        if facts.own_paths.iter().any(|own| same_folder(own, path)) {
            return Err(format!("{branch}'s worktree holds a project's folder, so Chorus won't remove it."));
        }
        if Path::new(path).is_dir() {
            let lost = hygiene::would_lose(git::status_paths(Path::new(path))?);
            if !lost.is_empty() {
                return Err(left_behind(branch, path, &lost));
            }
        }
        // A worktree whose folder is already gone just has git's record of it cleared.
        git::remove_worktree(&facts.root, path)?;
        for w in worktrees::list(m.store())?.into_iter().filter(|w| !w.removed && same_folder(&w.path, path)) {
            worktrees::set_removed(m.store(), w.id)?;
        }
    }
    git::delete_branch_at(&facts.root, branch, current)?;
    let store = m.store();
    rows::unsnooze(store, project_id, branch)?;
    rows::clear_flagged(store, project_id, branch)?;
    if let Some(h) = rows::open_on_branch(store, project_id, branch)? {
        rows::close(store, h.id, now)?;
    }
    store::notifications::mark_read_by_key_prefix(store, &format!("branch:{project_id}:{branch}:"), now)
}

/// Why a worktree wasn't removed: what removing it would lose, the first few named.
fn left_behind(branch: &str, path: &str, lost: &[String]) -> String {
    let named = lost.iter().take(NAMED_LEFT).map(String::as_str).collect::<Vec<_>>().join(", ");
    let more = if lost.len() > NAMED_LEFT { format!(" and {} more", lost.len() - NAMED_LEFT) } else { String::new() };
    format!("The worktree at {path} has changes or files that removing it would lose ({named}{more}), so Chorus left {branch} alone.")
}

/// The notices one project's sweep would raise, each with its branch: one for each stale branch
/// not yet flagged this spell. A spell is flagged once even after its notice is pruned.
pub(super) fn due(m: &SessionManager, project_id: u32, project: &str, now: i64) -> Result<Vec<(String, Notice)>, String> {
    let Some(facts) = gather(m, project_id, now)? else { return Ok(Vec::new()) };
    let Some(base) = facts.base.as_deref() else { return Ok(Vec::new()) };
    let store = m.store();
    let snoozed = rows::snoozes(store, project_id)?;
    let mut out = Vec::new();
    for s in hygiene::stale(&facts.branches, Some(base), now, &snoozed) {
        let notice = hygiene::stale_notice(project_id, project, base, &s);
        if rows::flagged(store, project_id, &s.name)?.as_deref() != Some(notice.dedupe_key.as_str()) {
            out.push((s.name, notice));
        }
    }
    Ok(out)
}

/// One project's part of the sweep. A notice that can't be stored doesn't stop the others.
pub fn sweep_project(m: &SessionManager, app: &tauri::AppHandle, project_id: u32, project: &str) -> Result<(), String> {
    let _one = one_at_a_time();
    let attention = app.state::<Arc<crate::attention::Attention>>();
    for (branch, notice) in due(m, project_id, project, store::now_ms())? {
        let spell = notice.dedupe_key.clone();
        match attention.add_project_notice(m.store(), project_id, notice) {
            Ok(()) => rows::set_flagged(m.store(), project_id, &branch, &spell)?,
            Err(e) => eprintln!("branch sweep, {branch}: {e}"),
        }
    }
    Ok(())
}

/// Every usable project's sweep, one repository once (the first project on it).
pub fn sweep(m: &SessionManager, app: &tauri::AppHandle) {
    let Ok(projects) = store::projects::list(m.store()) else { return };
    let mut roots: HashSet<String> = HashSet::new();
    for p in projects.into_iter().filter(|p| p.state != chorus_core::projects::ProjectState::Archived) {
        let Some(folder) = p.folder.as_deref().map(PathBuf::from).filter(|f| f.is_dir()) else { continue };
        let Ok(Some(facts)) = git::facts(&folder) else { continue };
        if !roots.insert(facts.main_root.to_ascii_lowercase()) {
            continue;
        }
        if let Err(e) = sweep_project(m, app, p.id, &p.name) {
            eprintln!("branch sweep for {}: {e}", p.name);
        }
    }
}

/// The `branch-sweep` thread: 2 minutes after start, then once a day.
pub fn start(m: &Arc<SessionManager>, app: tauri::AppHandle) -> Result<(), String> {
    let worker = m.clone();
    std::thread::Builder::new()
        .name("branch-sweep".into())
        .spawn(move || {
            let mut wait_ms = hygiene::FIRST_SWEEP_MS;
            while !worker.is_quitting() {
                if wait_ms <= 0 {
                    sweep(&worker, &app);
                    wait_ms = hygiene::SWEEP_EVERY_MS;
                }
                std::thread::sleep(Duration::from_secs(1));
                wait_ms -= 1000;
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}
