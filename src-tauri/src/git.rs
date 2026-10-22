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
                p.by_ref().take(MAX_OUTPUT_BYTES + 1).read_to_end(&mut out).map_err(|e| format!("Reading git output: {e}"))?;
                // Keep draining so Git can't hang on a full pipe. Never call truncated output clean.
                std::io::copy(&mut p, &mut std::io::sink()).map_err(|e| format!("Reading git output: {e}"))?;
            }
            if out.len() as u64 > MAX_OUTPUT_BYTES { return Err("Git output exceeded the 256 KiB limit; status is unavailable.".to_owned()); }
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
