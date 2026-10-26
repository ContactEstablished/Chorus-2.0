//! The gatekeeper (P15): runs a run's check commands in its worktree, one at a time, each hidden,
//! in a job of its own and with a time limit, and reports pass or fail with no opinions. Commands
//! come only from the run's configuration, which only Matthew edits; their output is scrubbed
//! before anything keeps it.

use std::io::Read;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use chorus_core::env::{ChildEnvInput, compose_child_env};
use chorus_core::run_loop::report::{self, CheckRun};
use chorus_core::run_loop::signature;
use chorus_core::scrub::{builtin_patterns, scrub_all};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::sessions::job::Job;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Output kept per check, from its end; enough for a signature and a report's tail.
const KEEP_BYTES: usize = 1024 * 1024;

/// A check as the run's configuration holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: String,
    pub command: String,
    pub timeout_secs: u32,
}

/// Toolchains look for these beyond the agents' baseline (LESSONS L13): npm's cache and config,
/// .NET's and the usual SDK homes. Each is copied only when the parent has it.
const REQUIRED: &[&str] = &[
    "APPDATA",
    "LOCALAPPDATA",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "CommonProgramFiles",
    "ComSpec",
    "windir",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "OS",
    "USERNAME",
    "DOTNET_ROOT",
    "NUGET_PACKAGES",
    "JAVA_HOME",
    "GOPATH",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "PNPM_HOME",
    "NVM_HOME",
    "NVM_SYMLINK",
];

/// Non-interactive, colourless runs: test runners don't watch, and output is plain text.
const ADDITIONS: &[(&str, &str)] = &[
    ("CI", "1"),
    ("NO_COLOR", "1"),
    ("FORCE_COLOR", "0"),
    ("DOTNET_CLI_TELEMETRY_OPTOUT", "1"),
    ("DOTNET_NOLOGO", "1"),
];

/// The environment checks run with.
pub fn environment(parent: &[(String, String)]) -> Vec<(String, String)> {
    compose_child_env(&ChildEnvInput { parent, required: REQUIRED, additions: ADDITIONS, secrets: &[] }).into_iter().collect()
}

/// Runs one check in `dir` through `cmd /C`, with stderr merged into stdout so the output keeps
/// its order. Never panics: a check that can't start fails with the reason as its output.
pub fn run(dir: &Path, check: &Check, env: &[(String, String)], redact: &[Vec<u8>]) -> CheckRun {
    let t0 = Instant::now();
    let (exit_code, timed_out, output) = match spawn_and_wait(dir, check, env) {
        Ok(r) => r,
        Err(e) => (None, false, format!("Chorus couldn't run this check: {e}")),
    };
    let scrubbed = String::from_utf8_lossy(&scrub_all(output.as_bytes(), redact, &builtin_patterns())).into_owned();
    let passed = exit_code == Some(0) && !timed_out;
    let signature = (!passed).then(|| signature::of(&check.name, &scrubbed));
    CheckRun {
        name: check.name.clone(),
        command: check.command.clone(),
        passed,
        exit_code,
        timed_out,
        duration_ms: u32::try_from(t0.elapsed().as_millis()).unwrap_or(u32::MAX),
        output_tail: report::tail(&signature::strip_escapes(&scrubbed)),
        signature,
    }
}

fn spawn_and_wait(dir: &Path, check: &Check, env: &[(String, String)]) -> Result<(Option<i32>, bool, String), String> {
    let shell = env.iter().find(|(k, _)| k.eq_ignore_ascii_case("ComSpec")).map_or("cmd.exe", |(_, v)| v.as_str()).to_owned();
    let mut child = Command::new(shell)
        .args(["/D", "/S", "/C"])
        // With /S, cmd drops the outer quotes and runs the rest exactly as written; the brackets
        // send every part of a compound command's errors to the one pipe.
        .raw_arg(format!("\"({}) 2>&1\"", check.command))
        .current_dir(dir)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| e.to_string())?;
    // Dropping the job ends the whole tree the check started, a test server included.
    let job = Job::new().map_err(|e| format!("creating a job: {e}"))?;
    let _ = job.assign(windows::Win32::Foundation::HANDLE(child.as_raw_handle()));
    let pipe = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut kept: Vec<u8> = Vec::new();
        if let Some(mut p) = pipe {
            let mut buf = [0u8; 16 * 1024];
            while let Ok(n) = p.read(&mut buf) {
                if n == 0 {
                    break;
                }
                kept.extend_from_slice(&buf[..n]);
                if kept.len() > 2 * KEEP_BYTES {
                    kept.drain(..kept.len() - KEEP_BYTES);
                }
            }
        }
        kept
    });
    let limit = Duration::from_secs(u64::from(check.timeout_secs));
    let started = Instant::now();
    let (code, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.code(), false),
            Ok(None) if started.elapsed() > limit => {
                let _ = job.terminate(1);
                let _ = child.kill();
                let _ = child.wait();
                break (None, true);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    };
    drop(job);
    let mut out = reader.join().unwrap_or_default();
    if out.len() > KEEP_BYTES {
        out.drain(..out.len() - KEEP_BYTES);
    }
    let mut text = String::from_utf8_lossy(&out).into_owned();
    if timed_out {
        text.push_str(&format!("\n(Chorus stopped this check after {} s.)", check.timeout_secs));
    }
    Ok((code, timed_out, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(command: &str, timeout_secs: u32) -> Check {
        Check { name: "t".into(), command: command.into(), timeout_secs }
    }

    fn env() -> Vec<(String, String)> {
        environment(&std::env::vars().collect::<Vec<_>>())
    }

    #[test]
    fn a_passing_and_a_failing_command() {
        let dir = std::env::temp_dir();
        let ok = run(&dir, &check("echo hello", 30), &env(), &[]);
        assert!(ok.passed && ok.output_tail.contains("hello") && ok.signature.is_none(), "{ok:?}");
        let bad = run(&dir, &check("echo error TS2322: nope 1>&2 & exit /b 3", 30), &env(), &[]);
        assert_eq!((bad.passed, bad.exit_code), (false, Some(3)));
        assert!(bad.output_tail.contains("error TS2322: nope"), "stderr is kept: {bad:?}");
        assert_eq!(bad.signature.unwrap().first_error.as_deref(), Some("error TS2322: nope"));
    }

    #[test]
    fn output_is_scrubbed() {
        let dir = std::env::temp_dir();
        let r = run(&dir, &check("echo token sk-fake-1234567890abcdef", 30), &env(), &[b"sk-fake-1234567890abcdef".to_vec()]);
        assert!(!r.output_tail.contains("sk-fake-1234567890abcdef"), "{r:?}");
    }

    #[test]
    fn a_check_past_its_limit_is_stopped() {
        let dir = std::env::temp_dir();
        let r = run(&dir, &check("ping -n 30 127.0.0.1 >nul", 1), &env(), &[]);
        assert!(r.timed_out && !r.passed && r.duration_ms < 15_000, "{r:?}");
        assert!(r.output_tail.contains("stopped this check after 1 s"));
    }

    #[test]
    fn checks_run_non_interactively_without_inherited_secrets() {
        let parent = vec![("PATH".to_owned(), "C:\\x".to_owned()), ("GITHUB_TOKEN".to_owned(), "secret".to_owned()), ("APPDATA".to_owned(), "C:\\a".to_owned())];
        let e = environment(&parent);
        assert!(e.iter().any(|(k, v)| k == "CI" && v == "1"));
        assert!(e.iter().any(|(k, _)| k == "APPDATA"));
        assert!(!e.iter().any(|(k, _)| k == "GITHUB_TOKEN"));
    }
}
