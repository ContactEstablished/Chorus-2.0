//! Where a new session works (D52, D53): checking the folder New session is given (git, other
//! sessions already there), making a worktree for it, the recent folders, a suggested name and the
//! session's tags; and, on Close, whether its worktree can go.

use std::path::Path;

use chorus_core::agent::AgentKind;
use chorus_core::{folders, names, tags as tag_rules, worktree as wt};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::{SessionManager, SessionSummary, check_cwd};
use crate::git;
use crate::store::worktrees::{self as worktree_rows, NewWorktree};
use crate::store::{sessions as rows, settings, tags as tag_rows};
use crate::vault;

const RECENT_KEY: &str = "recent_folders";

/// Where a new session works.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Workspace {
    /// In the folder as chosen.
    #[default]
    Here,
    /// In a new worktree of the folder's repository, on a branch of its own.
    NewWorktree,
    // Reuse a recorded worktree of this repository; checked again at launch.
    ExistingWorktree(u32),
}

/// A session's worktree, as the UI shows it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfo {
    pub id: u32,
    pub branch: String,
    pub path: String,
    /// The branch it was made from; none if that was a detached HEAD.
    pub base_branch: Option<String>,
    /// Removed by Chorus or found missing during reconciliation; history is kept.
    pub removed: bool,
}

/// What New session learns about a folder as it's typed or picked.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FolderCheck {
    /// Why a session can't start there, in words to show; none when it can.
    pub problem: Option<String>,
    pub git: Option<GitFolder>,
    /// Why git couldn't say anything (not installed, an unsafe repository…). The folder still works.
    pub git_note: Option<String>,
    /// Running sessions already working in the same tree, on the same branch.
    pub sharing: Vec<SharingSession>,
    /// The project a session here joins when none is chosen.
    pub project_id: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GitFolder {
    /// The working tree the folder is in.
    pub top_level: String,
    /// The repository's name (its main folder's).
    pub repo: String,
    /// The checked-out branch; none on a detached HEAD.
    pub branch: Option<String>,
    /// The folder is in one of the repository's worktrees, not its main tree.
    pub in_worktree: bool,
    /// What a new worktree's branch looks like (`chorus2/<repo>/…`).
    pub new_branch: Option<String>,
    /// Why a worktree can't be made here.
    pub worktree_problem: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SharingSession {
    pub id: u32,
    pub name: String,
    pub agent: AgentKind,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TagUse {
    pub tag: String,
    /// How many sessions have it, closed ones included.
    pub sessions: u32,
}

/// What closing a session would do to its worktree.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CloseCheck {
    pub worktree: Option<WorktreeClose>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeClose {
    pub branch: String,
    pub path: String,
    /// Nothing uncommitted; none when git couldn't tell (see `note`).
    pub clean: Option<bool>,
    /// Its folder was deleted outside Chorus: removing it only clears git's record of it.
    pub gone: bool,
    pub note: Option<String>,
    /// Other open sessions working in it (a Pair's reviewer, a Workbench's shell).
    pub others: u32,
}

/// A worktree made for a session about to be recorded.
pub(super) struct Made {
    pub id: u32,
    pub cwd: String,
    repo_root: String,
    path: String,
    branch: String,
}

impl SessionManager {
    pub fn check_folder(&self, input: &str) -> FolderCheck {
        let mut check = FolderCheck { problem: None, git: None, git_note: None, sharing: Vec::new(), project_id: None };
        let folder = match check_cwd(input) {
            Ok(f) => f,
            Err(e) => {
                check.problem = Some(e);
                return check;
            }
        };
        check.project_id = self.project_for(None, &folder).ok().flatten();
        let facts = match git::facts(Path::new(&folder)) {
            Ok(Some(f)) => f,
            Ok(None) => return check,
            Err(e) => {
                check.git_note = Some(e);
                return check;
            }
        };
        // Sessions in the same tree: their folders are inside it, and git agrees (a nested
        // repository is a different tree).
        for row in rows::list_open(&self.store).unwrap_or_default() {
            if !folders::inside(&row.cwd, &facts.top_level) || self.running(row.id).is_err() {
                continue;
            }
            if let Ok(Some(theirs)) = git::facts(Path::new(&row.cwd))
                && folders::same(&theirs.top_level, &facts.top_level)
            {
                check.sharing.push(SharingSession { id: row.id, name: row.name, agent: row.agent });
            }
        }
        let plan = wt::plan(&facts.main_root, "…");
        check.git = Some(GitFolder {
            repo: Path::new(&facts.main_root).file_name().map_or_else(|| facts.main_root.clone(), |n| n.to_string_lossy().into_owned()),
            in_worktree: !folders::same(&facts.top_level, &facts.main_root),
            new_branch: plan.as_ref().ok().map(|p| p.branch.clone()),
            worktree_problem: plan.err(),
            top_level: facts.top_level,
            branch: facts.branch,
        });
        check
    }

    /// Makes a worktree of `folder`'s repository, from its checked-out branch. The session works
    /// in the same subfolder there, or at the top if that subfolder isn't in the repository.
    pub(super) fn make_worktree(&self, folder: &str) -> Result<Made, String> {
        let facts = git::facts(Path::new(folder))?
            .ok_or_else(|| format!("{folder} isn't in a git repository, so it can't have a worktree."))?;
        for _ in 0..5 {
            let id: [u8; 4] = vault::random_bytes(4).try_into().expect("4 bytes");
            let plan = wt::plan(&facts.main_root, &wt::short_id(id))?;
            if Path::new(&plan.path).exists() {
                continue;
            }
            match git::add_worktree(Path::new(&facts.main_root), &plan.path, &plan.branch, facts.branch.as_deref()) {
                Ok(()) => {}
                // That branch or folder name is taken (Chorus 1 retries the same way).
                Err(e) if e.contains("already exists") => continue,
                Err(e) => return Err(e),
            }
            let new = NewWorktree { repo_root: &facts.main_root, path: &plan.path, branch: &plan.branch, base_branch: facts.branch.as_deref() };
            let made = Made { id: 0, cwd: String::new(), repo_root: facts.main_root.clone(), path: plan.path.clone(), branch: plan.branch.clone() };
            let row_id = match worktree_rows::insert(&self.store, &new) {
                Ok(id) => id,
                Err(e) => {
                    self.forget_worktree(&made);
                    return Err(e);
                }
            };
            let cwd = wt::cwd_in(&plan.path, &facts.top_level, folder);
            let cwd = if Path::new(&cwd).is_dir() { cwd } else { plan.path.clone() };
            return Ok(Made { id: row_id, cwd, ..made });
        }
        Err("Couldn't find a free name for a new worktree.".into())
    }

    /// Undoes a worktree no session ended up using: removes it and its branch, which has nothing
    /// on it yet.
    pub(super) fn forget_worktree(&self, made: &Made) {
        let root = Path::new(&made.repo_root);
        if git::remove_worktree(root, &made.path).is_ok() {
            let _ = git::delete_merged_branch(root, &made.branch);
        }
        if made.id != 0 {
            let _ = worktree_rows::delete(&self.store, made.id);
        }
    }

    pub(super) fn worktree_info(&self, id: u32) -> Option<WorktreeInfo> {
        worktree_rows::get(&self.store, id).ok().flatten().map(|w| WorktreeInfo {
            id: w.id,
            branch: w.branch,
            path: w.path,
            base_branch: w.base_branch,
            removed: w.removed,
        })
    }

    /// Recently used folders that still exist, newest first.
    pub fn recent_folders(&self) -> Vec<String> {
        let stored = settings::get(&self.store, RECENT_KEY).ok().flatten().unwrap_or_default();
        let list: Vec<String> = serde_json::from_str(&stored).unwrap_or_default();
        list.into_iter().filter(|f| Path::new(f).is_dir()).collect()
    }

    pub(super) fn remember_folder(&self, folder: &str) {
        let list = folders::remember(&self.recent_folders(), folder);
        if let Ok(json) = serde_json::to_string(&list) {
            let _ = settings::set(&self.store, RECENT_KEY, &json);
        }
    }

    /// A name from the pool that no open session has, nor any in `skip` (the ones just offered).
    pub fn suggest_name(&self, skip: &[String]) -> String {
        let mut taken: Vec<String> = rows::list_open(&self.store).unwrap_or_default().into_iter().map(|r| r.name).collect();
        taken.extend(skip.iter().cloned());
        let roll = u32::from_le_bytes(vault::random_bytes(4).try_into().expect("4 bytes"));
        names::suggest(&taken, roll).to_owned()
    }

    pub fn set_tags(&self, id: u32, tags: &[String]) -> Result<SessionSummary, String> {
        self.row(id)?;
        tag_rows::set(&self.store, id, &tag_rules::clean_tags(tags)?)?;
        self.publish(id)
    }

    pub fn known_tags(&self) -> Result<Vec<TagUse>, String> {
        Ok(tag_rows::known(&self.store)?.into_iter().map(|(tag, sessions)| TagUse { tag, sessions }).collect())
    }

    pub fn close_check(&self, id: u32) -> Result<CloseCheck, String> {
        let row = self.row(id)?;
        let Some(w) = row.worktree_id.and_then(|w| worktree_rows::get(&self.store, w).ok().flatten()).filter(|w| !w.removed) else {
            return Ok(CloseCheck { worktree: None });
        };
        let others = worktree_rows::other_open_sessions(&self.store, w.id, id)?;
        let gone = !Path::new(&w.path).is_dir();
        let (clean, note) = if gone {
            (None, Some("Its folder is already gone.".to_owned()))
        } else {
            match git::is_clean(Path::new(&w.path)) {
                Ok(c) => (Some(c), None),
                Err(e) => (None, Some(e)),
            }
        };
        Ok(CloseCheck { worktree: Some(WorktreeClose { branch: w.branch, path: w.path, clean, gone, note, others }) })
    }

    /// After Close: removes the session's worktree if nothing would be lost and no other session
    /// works in it. Returns what happened, to show.
    pub(super) fn retire_worktree(&self, session_id: u32, worktree_id: u32) -> Option<String> {
        let _guard = self.worktree_lock.lock().expect("worktree lock");
        let w = worktree_rows::get(&self.store, worktree_id).ok().flatten().filter(|w| !w.removed)?;
        let others = match worktree_rows::other_open_sessions(&self.store, w.id, session_id) {
            Ok(n) => n,
            Err(e) => return Some(format!("Kept the worktree: couldn't check other sessions ({e}).")),
        };
        if others > 0 {
            return Some("Kept the worktree: another session still works in it.".into());
        }
        if !Path::new(&w.path).is_dir() {
            return Some(match git::remove_worktree(Path::new(&w.repo_root), &w.path) {
                Ok(()) => {
                    let _ = worktree_rows::set_removed(&self.store, w.id);
                    format!("The worktree's folder was already gone, so git's record of it is cleared. Its branch {} is kept.", w.branch)
                }
                Err(e) => format!("The worktree's folder is gone, but git's record of it couldn't be cleared: {e}"),
            });
        }
        if let Err(e) = self.validate_worktree_cwd(w.id, &w.path) {
            return Some(format!("Kept the worktree: {e}"));
        }
        Some(match git::is_clean(Path::new(&w.path)) {
            Ok(true) => match git::remove_worktree(Path::new(&w.repo_root), &w.path) {
                Ok(()) => {
                    let _ = worktree_rows::set_removed(&self.store, w.id);
                    format!("Removed the worktree. Its branch {} is kept.", w.branch)
                }
                // git stops trying partway through deleting the files, but drops its record of
                // the worktree anyway (LESSONS L44).
                Err(e) if !still_a_worktree(&w.path) => {
                    let _ = worktree_rows::set_removed(&self.store, w.id);
                    format!(
                        "Removed the worktree from git, but couldn't delete all of its folder ({e}). What's left can be deleted. Its branch {} is kept.",
                        w.branch
                    )
                }
                Err(e) => format!("Couldn't remove the worktree ({e}), so it's kept."),
            },
            Ok(false) => "Kept the worktree: it has uncommitted changes.".into(),
            Err(e) => format!("Kept the worktree: {e}"),
        })
    }
}

/// Whether git still takes `path` for a worktree. When it can't say, the answer is yes, so nothing
/// is claimed gone.
fn still_a_worktree(path: &str) -> bool {
    match git::facts(Path::new(path)) {
        Ok(Some(f)) => folders::same(&f.top_level, path),
        Ok(None) => false,
        Err(_) => true,
    }
}
