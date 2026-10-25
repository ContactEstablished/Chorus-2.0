//! Where each agent CLI lives and how to start it. The options each agent declares, and their
//! arguments, are in `chorus_core::agent`; finding the CLI and its version is [`detect`].
//!
//! Each CLI is launched through its real install, never through `cmd.exe /c`: `claude` and `grok`
//! are native executables; `codex` and `opencode` are npm `.cmd` shims, resolved to their target
//! (LESSONS L14). Auto-update is switched off for every agent that has a switch (LESSONS L7).

pub mod codex;
pub mod codex_socket;
pub mod commands;
pub mod detect;
pub mod gemini;
pub mod opencode;

use std::path::{Path, PathBuf};

use chorus_core::shim::{ShimKind, parse_npm_shim};

pub use chorus_core::agent::AgentKind;

/// A resolved launch: what to run and what the agent needs beyond the baseline environment.
#[derive(Debug, Clone)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Imposed, non-secret values (auto-update switches, isolated state folders).
    pub additions: Vec<(&'static str, String)>,
}

/// Resolves `kind` against the parent's `PATH`. `scratch` is a per-session folder for agents whose
/// state must be isolated (opencode, LESSONS L5).
pub fn resolve(kind: AgentKind, parent_env: &[(String, String)], scratch: &Path) -> Result<Launch, String> {
    let path = env_value(parent_env, "PATH").unwrap_or_default();
    match kind {
        AgentKind::Shell => {
            let root = env_value(parent_env, "SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
            // UTF-8 code page, or cmd shows UTF-8 output as mojibake (S1 finding 5).
            Ok(Launch {
                program: Path::new(&root).join("System32\\cmd.exe"),
                args: vec!["/K".into(), "chcp 65001>nul".into()],
                additions: vec![],
            })
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
            let (program, mut args) = match find_on_path(&path, "codex.exe") {
                Some(program) => (program, vec![]),
                None => through_shim(&path, "codex.cmd")?,
            };
            // Codex has no environment switch for its update check (LESSONS L7).
            args.extend(["-c".into(), "check_for_update_on_startup=false".into()]);
            Ok(Launch { program, args, additions: vec![] })
        }
        AgentKind::Gemini => {
            let (program, args) = through_shim(&path, "gemini.cmd")?;
            // Folder trust is checked at launch, once the session's actual folder is known.
            // 0.64.0 has no auto-update switch on the command line or in the environment; its
            // `general.enableAutoUpdate` is a setting in the user's own settings file.
            Ok(Launch { program, args, additions: vec![] })
        }
        AgentKind::Aider => Ok(Launch {
            // `uv` installs a launcher executable, not an npm shim (LESSONS: aider on Windows).
            program: find_on_path(&path, "aider.exe").ok_or("aider.exe not found on PATH")?,
            args: vec![
                // 0.86.2 checks for a new version on every launch unless told not to (L7).
                "--no-check-update".into(),
                // Aider has no conversation id. Each Chorus session owns its history, so a new
                // session starts empty and Restart reloads only that session's chat (Q30).
                "--chat-history-file".into(),
                scratch.join("aider.chat.history.md").display().to_string(),
                "--input-history-file".into(),
                scratch.join("aider.input.history").display().to_string(),
                "--restore-chat-history".into(),
            ],
            additions: vec![],
        }),
        AgentKind::Opencode => {
            // The session runs `serve` and `--server` with these (ARCHITECTURE §5, `opencode`). Its
            // conversations live in the session's own data folder, which is how they resume.
            let (program, args) = through_shim(&path, "opencode.cmd")?;
            let state = opencode::state_home(scratch);
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

pub fn env_value(env: &[(String, String)], name: &str) -> Option<String> {
    env.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone())
}

/// `chorus-hook.exe`, Codex's hook command (D37), beside Chorus's own executable. The dev command
/// builds it there before starting the app. A path with spaces is given in its short (8.3) form:
/// Codex 0.162.1 runs hooks under PowerShell here, where a quoted path followed by an argument
/// doesn't parse (LESSONS L37).
pub fn hook_relay() -> Option<PathBuf> {
    let relay = std::env::current_exe().ok()?.parent()?.join("chorus-hook.exe");
    if !relay.is_file() {
        return None;
    }
    if relay.to_string_lossy().contains(' ') {
        return short_path(&relay).filter(|p| !p.to_string_lossy().contains(' ')).or(Some(relay));
    }
    Some(relay)
}

pub(crate) fn short_path(path: &Path) -> Option<PathBuf> {
    use windows::Win32::Storage::FileSystem::GetShortPathNameW;
    let long = windows::core::HSTRING::from(path.as_os_str());
    let mut buf = vec![0u16; 1024];
    // SAFETY: `long` is a valid wide string and `buf` a writable buffer whose length is passed.
    let n = unsafe { GetShortPathNameW(&long, Some(&mut buf)) } as usize;
    (n > 0 && n < buf.len()).then(|| PathBuf::from(String::from_utf16_lossy(&buf[..n])))
}

/// Claude Code's config, which has grown past a megabyte on a busy machine.
const MAX_CLAUDE_CONFIG_BYTES: u64 = 64 * 1024 * 1024;

/// Claude Code's config: `$CLAUDE_CONFIG_DIR/.claude.json`, else `~/.claude.json`.
fn claude_config_path(parent_env: &[(String, String)]) -> Option<PathBuf> {
    Some(
        env_value(parent_env, "CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .or_else(|| env_value(parent_env, "USERPROFILE").map(PathBuf::from))?
            .join(".claude.json"),
    )
}

fn read_claude_config(path: &Path) -> Option<String> {
    if std::fs::metadata(path).ok()?.len() > MAX_CLAUDE_CONFIG_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

/// Whether Claude Code trusts `cwd` or a folder above it, from its own config, read only. `None`
/// when unreadable.
pub fn claude_folder_trusted(parent_env: &[(String, String)], cwd: &str) -> Option<bool> {
    chorus_core::claude::folder_trusted(&read_claude_config(&claude_config_path(parent_env)?)?, cwd)
}

/// P15: whether Claude Code trusts sessions in the repository whose main working tree is
/// `main_root`, and so in its worktrees (`chorus_core::claude::repo_trusted`). None when its config
/// can't be read.
pub fn claude_repo_trusted(parent_env: &[(String, String)], main_root: &str) -> Option<bool> {
    chorus_core::claude::repo_trusted(&read_claude_config(&claude_config_path(parent_env)?)?, main_root)
}

/// Records in Claude Code's config that it trusts `folder` (D55): the only write Chorus makes
/// there. The config is read again just before, written to a file beside it and renamed over it,
/// so Claude Code never reads half a file, then read back. A config that isn't there yet is left
/// alone (Claude Code makes it on first run). Returns whether anything changed.
pub fn claude_trust_folder(parent_env: &[(String, String)], folder: &str) -> Result<bool, String> {
    let path = claude_config_path(parent_env).ok_or("no home folder for Claude Code's config")?;
    if !path.is_file() {
        return Ok(false);
    }
    let config = read_claude_config(&path).ok_or("Claude Code's config couldn't be read")?;
    if chorus_core::claude::folder_trusted(&config, folder) == Some(true) {
        return Ok(false);
    }
    let updated = chorus_core::claude::with_folder_trusted(&config, folder).ok_or("Claude Code's config isn't in the shape Chorus knows")?;
    let temp = path.with_file_name(format!(".claude.json.chorus2-{}.tmp", std::process::id()));
    std::fs::write(&temp, updated).map_err(|e| format!("writing {}: {e}", temp.display()))?;
    // Claude Code may hold the file open for a moment.
    let mut renamed = std::fs::rename(&temp, &path);
    for _ in 0..4 {
        if renamed.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        renamed = std::fs::rename(&temp, &path);
    }
    if let Err(e) = renamed {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("replacing Claude Code's config: {e}"));
    }
    match read_claude_config(&path).and_then(|c| chorus_core::claude::folder_trusted(&c, folder)) {
        Some(true) => Ok(true),
        _ => Err("Claude Code rewrote its config at the same moment, so the trust didn't stick".into()),
    }
}

/// Whether the agent has recorded the conversation `id`, so the next run resumes it instead of
/// starting it. Claude Code writes `<config>/projects/<folder>/<id>.jsonl` once the conversation has
/// a message; Grok keeps `~/.grok/sessions/<folder>/<id>/` (its bundled docs, unverified while Grok
/// isn't signed in); Gemini CLI names its chats `session-<ms>-<id's first 8>.jsonl`. The folder part
/// is derived from the working directory, so every folder is searched rather than re-deriving its
/// name. Codex's ids are only known once it has written them.
pub fn conversation_exists(kind: AgentKind, id: &str, parent_env: &[(String, String)]) -> bool {
    let home = || env_value(parent_env, "USERPROFILE").map(PathBuf::from);
    if kind == AgentKind::Gemini {
        return gemini_chat_state(id, parent_env) == GeminiChat::Resumable;
    }
    let (root, file) = match kind {
        AgentKind::Claude => {
            let config = env_value(parent_env, "CLAUDE_CONFIG_DIR").map(PathBuf::from).or_else(|| home().map(|h| h.join(".claude")));
            (config.map(|c| c.join("projects")), format!("{id}.jsonl"))
        }
        AgentKind::Grok => (home().map(|h| h.join(".grok").join("sessions")), id.to_owned()),
        AgentKind::Codex => return true,
        // Gemini is handled above. Aider has no conversation id to look for.
        AgentKind::Aider | AgentKind::Gemini | AgentKind::Shell | AgentKind::Opencode => return false,
    };
    let Some(Ok(folders)) = root.map(std::fs::read_dir) else {
        return false;
    };
    folders.flatten().any(|f| f.path().join(&file).exists())
}

/// What Gemini CLI would find in the chat file for `id`. Its own rule decides resumability
/// (`chorus_core::gemini`), and the two are not the same question: a file with no turn in it can't
/// be resumed and can't be created again either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeminiChat {
    /// No chat file: `--session-id <id>` makes one.
    Missing,
    /// A chat file with no turn: `--resume` refuses it, `--session-id` finds the id taken.
    Empty,
    /// At least one turn: `--resume <id>` continues it.
    Resumable,
}

/// Gemini CLI 0.64.0 keeps `<home>\.gemini\tmp\<project>\chats\session-<ms>-<id's first 8>.jsonl`.
/// The project folder follows the working directory, so every one is searched; the name only holds
/// the id's first 8 characters, so the record's `sessionId` is checked too.
pub fn gemini_chat_state(id: &str, parent_env: &[(String, String)]) -> GeminiChat {
    let Some(short) = id.get(..8) else { return GeminiChat::Missing };
    let Some(tmp) = env_value(parent_env, "USERPROFILE").map(|h| PathBuf::from(h).join(".gemini").join("tmp")) else {
        return GeminiChat::Missing;
    };
    let suffix = format!("-{short}.jsonl");
    let mut found = GeminiChat::Missing;
    for project in std::fs::read_dir(tmp).into_iter().flatten().flatten() {
        for file in std::fs::read_dir(project.path().join("chats")).into_iter().flatten().flatten() {
            if !file.file_name().to_string_lossy().ends_with(&suffix) {
                continue;
            }
            let record = std::fs::read_to_string(file.path()).unwrap_or_default();
            if !record.contains(id) {
                continue;
            }
            if chorus_core::gemini::chat_state(&record) == chorus_core::gemini::ChatState::Resumable {
                return GeminiChat::Resumable;
            }
            found = GeminiChat::Empty;
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_native_install_resolves_without_an_npm_shim() {
        let dir = std::env::temp_dir().join(format!("chorus-codex-native-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("codex.exe"), []).unwrap();
        let env = vec![("PATH".into(), dir.display().to_string())];
        let launch = resolve(AgentKind::Codex, &env, &dir).unwrap();
        assert_eq!(launch.program, dir.join("codex.exe"));
        assert_eq!(launch.args, ["-c", "check_for_update_on_startup=false"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn aider_sessions_have_separate_history_paths_and_restart_reuses_them() {
        let dir = std::env::temp_dir().join(format!("chorus-aider-history-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Resolution needs a program on PATH; this test never runs the stub.
        std::fs::write(dir.join("aider.exe"), []).unwrap();
        let env = vec![("PATH".into(), dir.display().to_string())];
        let first = resolve(AgentKind::Aider, &env, &dir.join("session1")).unwrap();
        let second = resolve(AgentKind::Aider, &env, &dir.join("session2")).unwrap();
        let again = resolve(AgentKind::Aider, &env, &dir.join("session1")).unwrap();
        for flag in ["--chat-history-file", "--input-history-file"] {
            let at = first.args.iter().position(|v| v == flag).unwrap() + 1;
            assert!(Path::new(&first.args[at]).starts_with(dir.join("session1")));
            assert!(Path::new(&second.args[at]).starts_with(dir.join("session2")));
            assert_ne!(first.args[at], second.args[at]);
        }
        assert_eq!(first.args, again.args);
        assert!(first.args.iter().any(|a| a == "--restore-chat-history"));
        assert!(!dir.join("session1").exists(), "resolution never writes history");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn trusting_a_folder_rewrites_claude_codes_config_in_place() {
        let dir = std::env::temp_dir().join(format!("chorus-claude-trust-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let env = vec![("CLAUDE_CONFIG_DIR".to_owned(), dir.display().to_string())];
        let home = dir.join(".chorus2").join("ledger").display().to_string();
        assert_eq!(claude_trust_folder(&env, &home), Ok(false), "no config yet: Claude Code makes it, not Chorus");
        assert!(!dir.join(".claude.json").exists());

        std::fs::write(dir.join(".claude.json"), r#"{"numStartups":3,"projects":{}}"#).unwrap();
        std::fs::create_dir_all(dir.join(".chorus2")).unwrap();
        assert_eq!(claude_trust_folder(&env, &home), Ok(true));
        let worktree = dir.join(".chorus2").join("ledger").join("1a2b3c4d").join("web");
        assert_eq!(claude_folder_trusted(&env, &worktree.display().to_string()), Some(true));
        assert_eq!(claude_trust_folder(&env, &home), Ok(false), "already trusted: nothing written");
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(left.len(), 2, "the config and the worktree folder only, no temp file: {left:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
