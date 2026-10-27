//! Finding each agent CLI and reading its version (PLAN P2). Detection runs in the background at
//! start-up and again when the launch form asks; a launch records the last result on its run.
//!
//! `--version` runs exactly as a launch would: the real program behind any npm shim, the minimal
//! environment, the update switches and opencode's isolated state folders.

use std::collections::HashMap;
use std::io::Read;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chorus_core::agent::AgentKind;
use chorus_core::cli::{VersionFit, fit, parse_version};
use chorus_core::env::{ChildEnvInput, compose_child_env};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::sessions::job::Job;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const VERSION_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_VERSION_BYTES: u64 = 4096;

/// What Chorus found for one agent.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    /// Whether a launch can use it: found, and not a version Chorus can't drive at all.
    pub found: bool,
    /// The program a launch runs (behind any npm shim).
    pub program: Option<String>,
    /// The version as `x.y.z`, when it could be read.
    pub version: Option<String>,
    pub fit: Option<VersionFit>,
    /// Why the agent can't be launched, or why its version couldn't be read.
    pub problem: Option<String>,
}

#[derive(Default)]
pub struct Detector {
    results: Mutex<HashMap<AgentKind, Detection>>,
}

impl Detector {
    pub fn get(&self, kind: AgentKind) -> Option<Detection> {
        self.results.lock().expect("detector lock").get(&kind).cloned()
    }

    /// Detects every agent at once (each in its own thread: a CLI can take seconds to start).
    pub fn refresh(&self, parent_env: &[(String, String)], scratch: &Path) -> Vec<(AgentKind, Detection)> {
        let found: Vec<(AgentKind, Detection)> = std::thread::scope(|s| {
            let handles: Vec<_> =
                AgentKind::ALL.iter().map(|&k| (k, s.spawn(move || detect(k, parent_env, &scratch.join(k.as_str()))))).collect();
            handles
                .into_iter()
                .map(|(k, h)| {
                    let d = h.join().unwrap_or_else(|_| Detection {
                        found: false,
                        program: None,
                        version: None,
                        fit: None,
                        problem: Some("detection failed".into()),
                    });
                    (k, d)
                })
                .collect()
        });
        let mut results = self.results.lock().expect("detector lock");
        for (k, d) in &found {
            results.insert(*k, d.clone());
        }
        found
    }
}

pub fn detect(kind: AgentKind, parent_env: &[(String, String)], scratch: &Path) -> Detection {
    let launch = match super::resolve(kind, parent_env, scratch) {
        Ok(l) => l,
        Err(e) => return Detection { found: false, program: None, version: None, fit: None, problem: Some(e) },
    };
    let program = Some(launch.program.display().to_string());
    let note = launch.note.clone();
    if kind == AgentKind::Shell {
        return Detection { found: true, program, version: None, fit: None, problem: None };
    }
    let additions: Vec<(&str, &str)> = launch.additions.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let env = compose_child_env(&ChildEnvInput { parent: parent_env, required: &[], additions: &additions, secrets: &[] });
    let verified = kind.spec().verified.and_then(parse_version);
    match run_version(&launch.program, &launch.args, &env, scratch) {
        Ok(out) => {
            let version = parse_version(&out);
            let fit = Some(fit(version, verified));
            let shown = version.map(|v| v.to_string());
            if kind == AgentKind::Opencode
                && let Some(refusal) = chorus_core::opencode::unfinished_install(None, Some(&out))
                    .or_else(|| chorus_core::opencode::too_old(shown.as_deref(), kind.spec().verified))
            {
                return Detection { found: false, program, version: shown, fit, problem: Some(refusal) };
            }
            let problem = version.is_none().then(|| format!("couldn't read a version from {:?}", first_line(&out))).or(note);
            Detection { found: true, program, version: shown, fit, problem }
        }
        Err(e) => Detection {
            found: true,
            program,
            version: None,
            fit: Some(VersionFit::Unreadable),
            problem: Some(note.map_or_else(|| e.clone(), |n| format!("{e}. {n}"))),
        },
    }
}

fn first_line(s: &str) -> &str {
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("")
}

/// Runs `program args --version` hidden, and returns what it printed. It runs in a job of its own,
/// so a shim's child process can't outlive a timeout ([`VERSION_TIMEOUT`]).
fn run_version(
    program: &PathBuf,
    args: &[String],
    env: &std::collections::BTreeMap<String, String>,
    scratch: &Path,
) -> Result<String, String> {
    std::fs::create_dir_all(scratch).map_err(|e| format!("creating {}: {e}", scratch.display()))?;
    let mut child = Command::new(program)
        .args(args)
        .arg("--version")
        .env_clear()
        .envs(env)
        .current_dir(scratch)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("running {} --version: {e}", program.display()))?;
    // Dropping the job ends anything still in it.
    let job = Job::new().map_err(|e| format!("creating a job: {e}"))?;
    let _ = job.assign(windows::Win32::Foundation::HANDLE(child.as_raw_handle()));
    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        let _ = (&mut stdout).take(MAX_VERSION_BYTES).read_to_string(&mut out);
        out
    });
    let t0 = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if t0.elapsed() < VERSION_TIMEOUT => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{} --version didn't finish in {} s", program.display(), VERSION_TIMEOUT.as_secs()));
            }
        }
    }
    reader.join().map_err(|_| "reading the version failed".to_owned())
}
