//! P5: Git owns worktree membership; SQLite owns session history and Chorus's worktree records.
use std::collections::HashMap;
use std::path::Path;

use chorus_core::{folders, git_status::Tree, worktree};
use serde::Serialize;
use specta::Type;

use super::{SessionManager, check_cwd};
use crate::git::{self, Snapshot};
use crate::store::worktrees::{self as rows, WorktreeRow};

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeEntry {
    pub id: u32,
    pub repo_root: String,
    pub path: String,
    pub branch: String,
    pub removed: bool,
    pub open_sessions: u32,
    /// None means it can be selected; checked again at launch.
    pub problem: Option<String>,
    pub status: Option<Snapshot>,
}

pub(super) struct Reconciled {
    row: WorktreeRow,
    problem: Option<String>,
    branch: String,
}

fn member<'a>(w: &WorktreeRow, trees: &'a [Tree]) -> Option<&'a Tree> {
    trees.iter().find(|t| folders::same(&t.path, &w.path))
}

/// Canonical paths prevent a junction, nested repository or a replacement ordinary directory from
/// masquerading as the original worktree. No stored cwd is rewritten during recovery.
fn verify(w: &WorktreeRow, trees: &[Tree]) -> Result<(), String> {
    if w.removed || member(w, trees).is_none() {
        return Err("This worktree was removed or is no longer registered with Git. Session history is kept.".into());
    }
    let path = check_cwd(&w.path)?;
    let root = check_cwd(&w.repo_root)?;
    let facts = git::facts(Path::new(&path))?
        .ok_or("The worktree folder is no longer a Git working tree.")?;
    if !folders::same(&facts.top_level, &path)
        || !folders::same(&facts.main_root, &root)
        || !folders::same(&path, &w.path)
    {
        return Err(
            "The worktree folder now points somewhere else or belongs to a different repository."
                .into(),
        );
    }
    Ok(())
}

impl SessionManager {
    /// Serialize reconciliation, adoption and removal, so Close cannot remove a tree while a new
    /// session is being recorded in it. Git errors preserve the recorded state.
    pub(super) fn reconcile_worktrees(&self) -> Result<Vec<Reconciled>, String> {
        let _guard = self.worktree_lock.lock().expect("worktree lock");
        self.reconcile_worktrees_locked()
    }

    fn reconcile_worktrees_locked(&self) -> Result<Vec<Reconciled>, String> {
        let mut inventories: HashMap<String, Result<Vec<Tree>, String>> = HashMap::new();
        let mut out = Vec::new();
        for mut w in rows::list(&self.store)? {
            let mut branch_label = w.branch.clone();
            let problem = if w.removed {
                Some("Worktree removed or missing; session history is kept.".into())
            } else {
                let trees = inventories
                    .entry(w.repo_root.to_lowercase())
                    .or_insert_with(|| git::worktrees(Path::new(&w.repo_root)));
                match trees {
                    Err(e) => Some(e.clone()),
                    Ok(trees) => {
                        let missing_folder = std::fs::metadata(&w.path)
                            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound);
                        if member(&w, trees).is_none() || missing_folder {
                            rows::set_removed(&self.store, w.id)?;
                            w.removed = true;
                            Some("Worktree removed or missing; session history is kept.".into())
                        } else {
                            match verify(&w, trees) {
                                Err(e) => Some(e),
                                Ok(()) => {
                                    branch_label = member(&w, trees)
                                        .and_then(|t| t.branch.clone())
                                        .unwrap_or_else(|| "Detached HEAD".into());
                                    if let Some(branch) =
                                        member(&w, trees).and_then(|t| t.branch.as_ref())
                                    {
                                        rows::set_branch(&self.store, w.id, branch)?;
                                        w.branch = branch.clone();
                                    }
                                    None
                                }
                            }
                        }
                    }
                }
            };
            out.push(Reconciled {
                row: w,
                problem,
                branch: branch_label,
            });
        }
        Ok(out)
    }

    /// Folder-filtered records are the New session chooser. The panel also includes removed trees.
    pub fn worktrees_list(&self, folder: Option<&str>) -> Result<Vec<WorktreeEntry>, String> {
        let repo = folder
            .map(|p| {
                let p = check_cwd(p)?;
                git::facts(Path::new(&p))?
                    .map(|f| f.main_root)
                    .ok_or_else(|| "Choose a Git repository.".to_owned())
            })
            .transpose()?;
        let reconciled = self.reconcile_worktrees()?;
        let mut out = Vec::new();
        for Reconciled {
            row: w,
            mut problem,
            branch,
        } in reconciled
        {
            if let Some(repo) = &repo
                && (w.removed || !folders::same(repo, &w.repo_root))
            {
                continue;
            }
            // The chooser needs identity and availability; full status is loaded by the panel.
            let status = if problem.is_none() && repo.is_none() {
                match git::snapshot(Path::new(&w.path)) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        problem = Some(e);
                        None
                    }
                }
            } else {
                None
            };
            out.push(WorktreeEntry {
                open_sessions: rows::other_open_sessions(&self.store, w.id, 0)?,
                id: w.id,
                repo_root: w.repo_root,
                path: w.path,
                branch,
                removed: w.removed,
                problem,
                status,
            });
        }
        Ok(out)
    }

    pub(super) fn validate_worktree_cwd(&self, id: u32, cwd: &str) -> Result<(), String> {
        let w = rows::get(&self.store, id)?.ok_or("This worktree record no longer exists.")?;
        if w.removed {
            return Err("This worktree was removed or is missing. Session history is kept; start a new session in another worktree.".into());
        }
        let trees = git::worktrees(Path::new(&w.repo_root))?;
        verify(&w, &trees)?;
        let cwd = check_cwd(cwd)?;
        if !folders::inside(&cwd, &w.path) {
            return Err("The session folder is outside its worktree.".into());
        }
        let facts = git::facts(Path::new(&cwd))?
            .ok_or("The session folder is no longer in its worktree.")?;
        if !folders::same(&facts.top_level, &w.path) {
            return Err("The session folder belongs to a different working tree.".into());
        }
        Ok(())
    }

    /// Select a stored worktree of the requested repository, keeping the requested relative folder.
    /// A missing relative folder is an error, never a fallback to the root or the original repo.
    pub(super) fn reuse_worktree(&self, id: u32, asked: &str) -> Result<String, String> {
        let w = rows::get(&self.store, id)?.ok_or("This worktree record no longer exists.")?;
        let source =
            git::facts(Path::new(asked))?.ok_or("Choose a folder in the worktree's repository.")?;
        if !folders::same(&source.main_root, &w.repo_root) {
            return Err("That worktree belongs to a different repository.".into());
        }
        let cwd = worktree::cwd_in(&w.path, &source.top_level, asked);
        self.validate_worktree_cwd(id, &cwd)?;
        check_cwd(&cwd)
    }

    /// A direct 'This folder' launch inside a managed tree must count toward Close's protection too.
    pub(super) fn managed_worktree_at(&self, cwd: &str) -> Result<Option<u32>, String> {
        for w in rows::list(&self.store)? {
            if !w.removed && folders::inside(cwd, &w.path) {
                self.validate_worktree_cwd(w.id, cwd)?;
                return Ok(Some(w.id));
            }
        }
        Ok(None)
    }

    pub fn session_git_status(&self, id: u32) -> Result<Option<Snapshot>, String> {
        let row = self.row(id)?;
        if let Some(w) = row.worktree_id {
            self.validate_worktree_cwd(w, &row.cwd)?;
        }
        let cwd = check_cwd(&row.cwd)?;
        let Some(facts) = git::facts(Path::new(&cwd))? else {
            return Ok(None);
        };
        git::snapshot(Path::new(&facts.top_level)).map(Some)
    }
}
