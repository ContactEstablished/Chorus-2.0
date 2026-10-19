//! Where each agent CLI lives and how to start it (spike S1; the full adapters are P2).
//!
//! Each CLI is launched through its real install, never through `cmd.exe /c`: `claude` and `grok`
//! are native executables; `codex` and `opencode` are npm `.cmd` shims, resolved to their target
//! (LESSONS L14). Auto-update is switched off for every agent that has a switch (LESSONS L7).

use std::path::{Path, PathBuf};

use chorus_core::shim::{ShimKind, parse_npm_shim};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Shell,
    Claude,
    Codex,
    Opencode,
    Grok,
}

/// A resolved launch: what to run and what the agent needs beyond the baseline environment.
#[derive(Debug, Clone)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Imposed, non-secret values (auto-update switches, isolated state folders).
    pub additions: Vec<(&'static str, String)>,
}

/// Run opencode's TUI against a server Chorus started (`opencode attach`), as ARCHITECTURE §5 plans.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeAttach {
    pub port: u16,
    pub session_id: String,
}

/// Resolves `kind` against the parent's `PATH`. `scratch` is a per-session folder for agents whose
/// state must be isolated (opencode, LESSONS L5).
pub fn resolve(
    kind: AgentKind,
    parent_env: &[(String, String)],
    scratch: &Path,
    attach: Option<&OpencodeAttach>,
) -> Result<Launch, String> {
    let path = env_value(parent_env, "PATH").unwrap_or_default();
    match kind {
        AgentKind::Shell => {
            let root = env_value(parent_env, "SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
            Ok(Launch { program: Path::new(&root).join("System32\\cmd.exe"), args: vec![], additions: vec![] })
        }
        AgentKind::Claude => Ok(Launch {
            program: find_on_path(&path, "claude.exe").ok_or("claude.exe not found on PATH")?,
            args: vec![],
            additions: vec![("DISABLE_AUTOUPDATER", "1".into())],
        }),
        AgentKind::Grok => Ok(Launch {
            program: find_on_path(&path, "grok.exe").ok_or("grok.exe not found on PATH")?,
            // Both switches are documented in grok 1.0.62. Without them the first S1 launch updated
            // Grok from 1.0.5 to 1.0.62 (LESSONS L26).
            args: vec!["--no-auto-update".into()],
            additions: vec![("GROK_DISABLE_AUTOUPDATER", "1".into())],
        }),
        AgentKind::Codex => {
            let (program, mut args) = through_shim(&path, "codex.cmd")?;
            // Codex has no environment switch for its update check (LESSONS L7).
            args.extend(["-c".into(), "check_for_update_on_startup=false".into()]);
            Ok(Launch { program, args, additions: vec![] })
        }
        AgentKind::Opencode => {
            let (program, mut args) = through_shim(&path, "opencode.cmd")?;
            if let Some(a) = attach {
                if a.session_id.is_empty()
                    || a.session_id.len() > 64
                    || !a.session_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    return Err("invalid opencode session id".into());
                }
                args.extend(["attach".into(), format!("http://127.0.0.1:{}", a.port), "--session".into(), a.session_id.clone()]);
            }
            let state = scratch.join("xdg-state");
            let data = scratch.join("xdg-data");
            Ok(Launch {
                program,
                args,
                additions: vec![
                    ("OPENCODE_DISABLE_AUTOUPDATE", "true".into()),
                    ("XDG_STATE_HOME", state.display().to_string()),
                    ("XDG_DATA_HOME", data.display().to_string()),
                ],
            })
        }
    }
}

/// The program and leading arguments an npm shim really runs.
fn through_shim(path: &str, shim_name: &str) -> Result<(PathBuf, Vec<String>), String> {
    let shim = find_on_path(path, shim_name).ok_or_else(|| format!("{shim_name} not found on PATH"))?;
    let text = std::fs::read_to_string(&shim).map_err(|e| format!("reading {}: {e}", shim.display()))?;
    let target = parse_npm_shim(&text, &shim.display().to_string())
        .ok_or_else(|| format!("{} is not an npm shim shape Chorus recognises", shim.display()))?;
    match target.kind {
        ShimKind::Executable => Ok((PathBuf::from(target.file), vec![])),
        ShimKind::NodeScript => {
            // npm's shim prefers a node.exe beside itself, then PATH; do the same.
            let beside = shim.parent().map(|d| d.join("node.exe")).filter(|p| p.is_file());
            let node = beside.or_else(|| find_on_path(path, "node.exe")).ok_or("node.exe not found on PATH")?;
            Ok((node, vec![target.file]))
        }
    }
}

fn find_on_path(path: &str, name: &str) -> Option<PathBuf> {
    path.split(';').filter(|d| !d.is_empty()).map(|d| Path::new(d).join(name)).find(|p| p.is_file())
}

fn env_value(env: &[(String, String)], name: &str) -> Option<String> {
    env.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone())
}
