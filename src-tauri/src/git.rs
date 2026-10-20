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

use crate::sessions::job::Job;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MAX_OUTPUT_BYTES: u64 = 256 * 1024;
const QUICK: Duration = Duration::from_secs(10);
/// Checking out a large repository into a new worktree can take a while.
const SLOW: Duration = Duration::from_secs(180);

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
            if let Some(p) = pipe {
                let _ = p.take(MAX_OUTPUT_BYTES).read_to_end(&mut out);
            }
            String::from_utf8_lossy(&out).into_owned()
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
    Ok(Ran {
        ok: status.success(),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
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
/// Its branch stays.
pub fn remove_worktree(main_root: &Path, path: &str) -> Result<(), String> {
    let r = run(main_root, &["worktree", "remove", path], SLOW)?;
    if r.ok { Ok(()) } else { Err(complaint(&r.stderr)) }
}
