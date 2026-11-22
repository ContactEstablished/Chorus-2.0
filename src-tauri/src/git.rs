//! Running git (D53): asking about a folder, and making, checking and removing the worktrees New
//! session starts sessions in. git runs hidden, in a job of its own, with a time limit, and never
//! prompts: a credential prompt nobody can see would hang it.

use std::io::Read;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use chorus_core::worktree::{self as wt, Facts};
use chorus_core::git_status::{self, Changes, Diff, Tree};
use serde::Serialize;
use specta::Type;

use crate::sessions::job::Job;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MAX_OUTPUT_BYTES: u64 = 256 * 1024;
/// Queries whose time doesn't grow with the repository (rev-parse, status): past this, git is stuck.
const QUICK: Duration = Duration::from_secs(15);
/// Making or removing a worktree writes or deletes every tracked file. Chorus 1 measured a 7,000-file
/// repository with Defender scanning run well past 15 s cold, and a kill mid-checkout leaves a
/// half-made folder and a stray branch; this is a backstop against a stuck git, not a budget.
const SLOW: Duration = Duration::from_secs(600);

struct Ran {
    ok: bool,
    stdout: String,
    stderr: String,
}

/// git's first line of complaint, for a message.
fn complaint(stderr: &str) -> String {
    let line = stderr.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("no reason given");
    line.trim_start_matches("fatal: ").trim_start_matches("error: ").to_owned()
}

fn run(dir: &Path, args: &[&str], limit: Duration) -> Result<Ran, String> {
    run_capped(dir, args, limit, MAX_OUTPUT_BYTES)
}

fn run_capped(dir: &Path, args: &[&str], limit: Duration, max_bytes: u64) -> Result<Ran, String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        // Status shouldn't take the index lock an agent's own git may want.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => "git isn't installed, or isn't on PATH.".to_owned(),
            _ => format!("running git: {e}"),
        })?;
    // Dropping the job ends anything git started (a hook) that's still running.
    let job = Job::new().map_err(|e| format!("creating a job: {e}"))?;
    let _ = job.assign(windows::Win32::Foundation::HANDLE(child.as_raw_handle()));
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut out = Vec::new();
            if let Some(mut p) = pipe {
                p.by_ref().take(max_bytes + 1).read_to_end(&mut out).map_err(|e| format!("Reading git output: {e}"))?;
                // Keep draining so Git can't hang on a full pipe. Never call truncated output clean.
                std::io::copy(&mut p, &mut std::io::sink()).map_err(|e| format!("Reading git output: {e}"))?;
            }
            if out.len() as u64 > max_bytes { return Err(format!("Git output exceeded its {} KiB limit; status is unavailable.", max_bytes / 1024)); }
            String::from_utf8(out).map_err(|_| "Git returned text that isn't UTF-8.".to_owned())
        })
    };
    let stdout = read(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let stderr = read(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let t0 = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if t0.elapsed() < limit => std::thread::sleep(Duration::from_millis(15)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("git {} didn't finish in {} s.", args.first().unwrap_or(&""), limit.as_secs()));
            }
        }
    };
    drop(job);
    Ok(Ran {
        ok: status.success(),
        stdout: stdout.join().map_err(|_| "Reading git output failed.")??,
        stderr: stderr.join().map_err(|_| "Reading git errors failed.")??,
    })
}

/// The files under `dir` git tracks or would track (untracked but not ignored), relative to `dir`
/// with forward slashes, sorted; none when `dir` isn't in a repository. For Council's code tools
/// (D137).
pub fn ls_files(dir: &Path) -> Result<Option<Vec<String>>, String> {
    let args = ["ls-files", "-z", "--cached", "--others", "--exclude-standard"];
    let ran = run_capped(dir, &args, Duration::from_secs(60), 64 * 1024 * 1024)?;
    if !ran.ok {
        return if ran.stderr.contains("not a git repository") { Ok(None) } else { Err(complaint(&ran.stderr)) };
    }
    let mut files: Vec<String> = ran.stdout.split('\0').filter(|f| !f.is_empty()).map(str::to_owned).collect();
    files.sort();
    files.dedup();
    Ok(Some(files))
}

/// What git says about `dir`, or none when it isn't in a repository.
pub fn facts(dir: &Path) -> Result<Option<Facts>, String> {
    let mut args = wt::FACTS_ARGS.to_vec();
    args.push(wt::FACTS_LAST_ARG);
    let r = run(dir, &args, QUICK)?;
    if !r.ok {
        return if r.stderr.contains("not a git repository") { Ok(None) } else { Err(format!("git: {}", complaint(&r.stderr))) };
    }
    wt::parse_facts(&r.stdout).map(Some).ok_or_else(|| "git's answer about this folder couldn't be read.".to_owned())
}

/// Makes a worktree at `path` on a new branch, from `base` (else the current commit).
pub fn add_worktree(main_root: &Path, path: &str, branch: &str, base: Option<&str>) -> Result<(), String> {
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    let r = run(main_root, &["worktree", "add", "-b", branch, path, base.unwrap_or("HEAD")], SLOW)?;
    if r.ok { Ok(()) } else { Err(format!("git couldn't make the worktree: {}", complaint(&r.stderr))) }
}

/// Whether a working tree has nothing uncommitted: no changes and no untracked files.
pub fn is_clean(dir: &Path) -> Result<bool, String> {
    let r = run(dir, &["status", "--porcelain"], QUICK)?;
    if r.ok { Ok(wt::is_clean(&r.stdout)) } else { Err(format!("git status: {}", complaint(&r.stderr))) }
}

/// Deletes a branch only if it's merged (`-d`, never `-D`): for a worktree that was never used.
pub fn delete_merged_branch(main_root: &Path, branch: &str) -> Result<(), String> {
    let r = run(main_root, &["branch", "-d", branch], QUICK)?;
    if r.ok { Ok(()) } else { Err(complaint(&r.stderr)) }
}

/// Removes a worktree. Without `--force`, so git itself refuses if anything uncommitted is there.
/// Its branch stays. A worktree whose folder is already gone just has git's record of it cleared
/// (git 2.50).
pub fn remove_worktree(main_root: &Path, path: &str) -> Result<(), String> {
    let r = run(main_root, &["worktree", "remove", path], SLOW)?;
    if r.ok { Ok(()) } else { Err(complaint(&r.stderr)) }
}

/// P15: makes a worktree at `path` on `branch`, which already exists (a run that resumes).
pub fn add_worktree_on(main_root: &Path, path: &str, branch: &str) -> Result<(), String> {
    if let Some(parent) = Path::new(path).parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    let r = run(main_root, &["worktree", "add", path, branch], SLOW)?;
    if r.ok { Ok(()) } else { Err(format!("git couldn't make the worktree: {}", complaint(&r.stderr))) }
}

/// P15: the commit `rev` names, or none when it names nothing.
pub fn rev(dir: &Path, rev: &str) -> Result<Option<String>, String> {
    let spec = format!("{rev}^{{commit}}");
    let r = run(dir, &["rev-parse", "--verify", "--quiet", &spec], QUICK)?;
    Ok(r.ok.then(|| r.stdout.trim().to_owned()).filter(|s| !s.is_empty()))
}

/// P18: the repository's `info/exclude` file for `dir`, or none outside a repository. It is the
/// common git directory's, so every worktree of the repository shares it.
pub fn exclude_file(dir: &Path) -> Result<Option<std::path::PathBuf>, String> {
    let r = run(dir, &["rev-parse", "--git-path", "info/exclude"], QUICK)?;
    if !r.ok {
        return if r.stderr.contains("not a git repository") { Ok(None) } else { Err(format!("git: {}", complaint(&r.stderr))) };
    }
    let answer = r.stdout.trim();
    if answer.is_empty() {
        return Err("git didn't say where its exclude file is.".into());
    }
    let path = Path::new(answer);
    Ok(Some(if path.is_absolute() { path.to_owned() } else { dir.join(path) }))
}

/// P18: the commits in `from..to`, newest first, as (short sha, subject): the first `limit`, and
/// how many more there are.
pub fn log_range(dir: &Path, from: &str, to: &str, limit: usize) -> Result<(Vec<(String, String)>, u32), String> {
    let range = format!("{from}..{to}");
    let r = run(dir, &["log", "--format=%h%x00%s", &range, "--"], QUICK)?;
    if !r.ok {
        return Err(format!("git log: {}", complaint(&r.stderr)));
    }
    let all: Vec<(String, String)> = r
        .stdout
        .lines()
        .filter_map(|l| l.split_once('\0').map(|(sha, subject)| (sha.to_owned(), subject.to_owned())))
        .collect();
    let more = all.len().saturating_sub(limit) as u32;
    Ok((all.into_iter().take(limit).collect(), more))
}

/// A changed file: its path, and lines added and removed (`None` for a binary file).
pub type FileStat = (String, Option<u32>, Option<u32>);

/// P18: the files `to` changed since it left `from` (`from...to`).
pub fn numstat(dir: &Path, from: &str, to: &str) -> Result<Vec<FileStat>, String> {
    let range = format!("{from}...{to}");
    let r = run(dir, &["diff", "--numstat", &range, "--"], QUICK)?;
    if !r.ok {
        return Err(format!("git diff: {}", complaint(&r.stderr)));
    }
    Ok(r
        .stdout
        .lines()
        .filter_map(|l| {
            let mut parts = l.splitn(3, '\t');
            let (added, removed, path) = (parts.next()?, parts.next()?, parts.next()?);
            Some((path.to_owned(), added.parse().ok(), removed.parse().ok()))
        })
        .collect())
}

/// P18: whether commit `a` is in `b`'s history (merged into it).
pub fn is_ancestor(dir: &Path, a: &str, b: &str) -> Result<bool, String> {
    let r = run(dir, &["merge-base", "--is-ancestor", a, b], QUICK)?;
    if r.ok {
        return Ok(true);
    }
    if r.stderr.trim().is_empty() { Ok(false) } else { Err(format!("git merge-base: {}", complaint(&r.stderr))) }
}

/// P18: how many commits `to` has that `from` doesn't.
pub fn count_range(dir: &Path, from: &str, to: &str) -> Result<u32, String> {
    let range = format!("{from}..{to}");
    let r = run(dir, &["rev-list", "--count", &range, "--"], QUICK)?;
    if !r.ok {
        return Err(format!("git rev-list: {}", complaint(&r.stderr)));
    }
    r.stdout.trim().parse().map_err(|_| "git's commit count couldn't be read.".to_owned())
}

/// P18: every local branch, with its last commit's time (Unix seconds) and full commit.
pub fn branches(dir: &Path) -> Result<Vec<(String, i64, String)>, String> {
    let r = run(dir, &["for-each-ref", "--format=%(refname:short)%00%(committerdate:unix)%00%(objectname)", "refs/heads"], QUICK)?;
    if !r.ok {
        return Err(format!("git for-each-ref: {}", complaint(&r.stderr)));
    }
    Ok(r
        .stdout
        .lines()
        .filter_map(|l| {
            let mut parts = l.split('\0');
            let (name, at, sha) = (parts.next()?, parts.next()?, parts.next()?);
            Some((name.to_owned(), at.trim().parse().ok()?, sha.to_owned()))
        })
        .collect())
}

/// P18: the branch origin's HEAD points at (`origin/main`), or none.
pub fn origin_head(dir: &Path) -> Result<Option<String>, String> {
    let r = run(dir, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"], QUICK)?;
    Ok(r.ok.then(|| r.stdout.trim().to_owned()).filter(|s| !s.is_empty()))
}

/// P18: the local branches merged into `base`.
pub fn merged_into(dir: &Path, base: &str) -> Result<Vec<String>, String> {
    let r = run(dir, &["branch", "--format=%(refname:short)", "--merged", base], QUICK)?;
    if !r.ok {
        return Err(format!("git branch --merged: {}", complaint(&r.stderr)));
    }
    Ok(r.stdout.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect())
}

/// P18: deletes a branch the user asked to, only if it still points at `expected`: `update-ref`
/// checks that and deletes in one step, so a commit added in between is never lost. Whether it's
/// merged, and that nothing has it checked out, are the caller's checks; so is the name.
pub fn delete_branch_at(dir: &Path, branch: &str, expected: &str) -> Result<(), String> {
    if branch.starts_with('-') || expected.starts_with('-') {
        return Err("That isn't a branch.".into());
    }
    let r = run(dir, &["update-ref", "-d", &format!("refs/heads/{branch}"), expected], QUICK)?;
    if !r.ok {
        return Err(format!("{branch} changed while Chorus was deleting it, so it was left alone ({}).", complaint(&r.stderr)));
    }
    // What `git branch -d` also removes: the branch's settings (its upstream). A branch without
    // any has no section, which is the usual case, so a failure here means nothing.
    let _ = run(dir, &["config", "--remove-section", &format!("branch.{branch}")], QUICK);
    Ok(())
}

/// P18: every path `git status` reports in a worktree: changed, untracked and *ignored* ones
/// (`.env`, build output), which `git worktree remove` would delete without a word.
pub fn status_paths(dir: &Path) -> Result<Vec<String>, String> {
    let r = run(dir, &["status", "--porcelain", "-z", "--ignored=matching", "--untracked-files=normal"], QUICK)?;
    if r.ok { Ok(chorus_core::branch_hygiene::status_paths(&r.stdout)) } else { Err(format!("git status: {}", complaint(&r.stderr))) }
}

/// P15: stages every change in the working tree and commits it, with the repository's own hooks
/// and identity. Returns the new commit, or none when there was nothing to commit.
pub fn commit_all(dir: &Path, message: &str) -> Result<Option<String>, String> {
    let add = run(dir, &["add", "--all"], SLOW)?;
    if !add.ok {
        return Err(format!("git add: {}", complaint(&add.stderr)));
    }
    let staged = run(dir, &["diff", "--cached", "--quiet"], QUICK)?;
    if staged.ok {
        return Ok(None);
    }
    let c = run(dir, &["commit", "--quiet", "-m", message], SLOW)?;
    if !c.ok {
        return Err(format!("git commit: {}", complaint(if c.stderr.trim().is_empty() { &c.stdout } else { &c.stderr })));
    }
    rev(dir, "HEAD")
}

/// P15: tags the current commit. A tag that already names it is left alone; one that names another
/// commit is refused, so a passing phase's tag is never moved.
pub fn tag(dir: &Path, name: &str, message: &str) -> Result<(), String> {
    let head = rev(dir, "HEAD")?.ok_or("The branch has no commit to tag.")?;
    if let Some(existing) = rev(dir, &format!("refs/tags/{name}"))? {
        return if existing == head { Ok(()) } else { Err(format!("The tag {name} already names another commit.")) };
    }
    let r = run(dir, &["tag", "-a", name, "-m", message], QUICK)?;
    if r.ok { Ok(()) } else { Err(format!("git tag: {}", complaint(&r.stderr))) }
}

/// P15: a file's text at `rev`, or none when it isn't there.
pub fn show(dir: &Path, rev: &str, path: &str) -> Result<Option<String>, String> {
    let spec = format!("{rev}:{}", path.replace('\\', "/"));
    let r = run(dir, &["show", &spec], QUICK)?;
    Ok(r.ok.then_some(r.stdout))
}

pub fn worktrees(root: &Path) -> Result<Vec<Tree>, String> {
    let r = run(root, &["worktree", "list", "--porcelain", "-z"], QUICK)?;
    if !r.ok { return Err(format!("git worktree list: {}", complaint(&r.stderr))); }
    git_status::worktrees(&r.stdout)
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub changes: Changes,
    /// Net tracked changes against HEAD (empty tree before the first commit).
    pub diff: Option<Diff>,
    pub diff_note: Option<String>,
}

/// Fresh working-tree facts. Never fetches, executes external diff drivers or reads file contents
/// into the app; numstat counts tracked changes, while status includes every untracked path.
pub fn snapshot(path: &Path) -> Result<Snapshot, String> {
    let s = run(path, &["status", "--porcelain=v2", "--branch", "--untracked-files=all", "--ignore-submodules=none", "-z"], QUICK)?;
    if !s.ok { return Err(format!("git status: {}", complaint(&s.stderr))); }
    let changes = git_status::changes(&s.stdout)?;
    let diff = (|| {
        let base = if s.stdout.contains("# branch.oid (initial)\0") {
            let empty = run(path, &["hash-object", "-t", "tree", "--stdin"], QUICK)?;
            if !empty.ok { return Err(complaint(&empty.stderr)); }
            empty.stdout.trim().to_owned()
        } else { "HEAD".into() };
        let d = run(path, &["diff", "--no-ext-diff", "--no-textconv", "--numstat", "-z", &base, "--"], QUICK)?;
        if !d.ok { return Err(format!("git diff: {}", complaint(&d.stderr))); }
        git_status::numstat(&d.stdout)
    })();
    let (diff, diff_note) = match diff { Ok(d) => (Some(d), None), Err(e) => (None, Some(e)) };
    Ok(Snapshot { changes, diff, diff_note })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_output_is_an_error_not_a_partial_answer() {
        let dir = std::env::temp_dir();
        let file = dir.join(format!("chorus-git-output-{}.config", std::process::id()));
        std::fs::write(&file, format!("[large]\nvalue={}\n", "a".repeat(MAX_OUTPUT_BYTES as usize + 1))).unwrap();
        let result = run(&dir, &["config", "--file", &file.display().to_string(), "--get", "large.value"], QUICK);
        std::fs::remove_file(file).unwrap();
        assert!(matches!(result, Err(e) if e.contains("exceeded")));
    }
}
