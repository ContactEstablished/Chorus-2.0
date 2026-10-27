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
    /// Something worth saying about how it was found that doesn't stop a launch.
    pub note: Option<String>,
}

impl Launch {
    /// A shim found outside PATH: its folder joins the child's PATH, so the agent's own tools find
    /// what the shim found, and the user is told.
    fn found_in(mut self, outside: Option<PathBuf>, path: &str) -> Self {
        if let Some(dir) = outside {
            self.additions.push(("PATH", format!("{path};{}", dir.display())));
            self.note = Some(format!("Found in npm's global folder ({}), which isn't on your PATH. Chorus adds it for this agent.", dir.display()));
        }
        self
    }
}

/// Resolves `kind` against the parent's `PATH`. `scratch` is a per-session folder for agents whose
/// state must be isolated (opencode, LESSONS L5).
pub fn resolve(kind: AgentKind, parent_env: &[(String, String)], scratch: &Path) -> Result<Launch, String> {
    let path = env_value(parent_env, "PATH").unwrap_or_default();
    let npm = npm_global_dirs(parent_env);
    match kind {
        AgentKind::Shell => {
            let root = env_value(parent_env, "SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
            // UTF-8 code page, or cmd shows UTF-8 output as mojibake (S1 finding 5).
            Ok(Launch {
                program: Path::new(&root).join("System32\\cmd.exe"),
                args: vec!["/K".into(), "chcp 65001>nul".into()],
                additions: vec![],
                note: None,
            })
        }
        AgentKind::Claude => Ok(Launch {
            program: find_on_path(&path, "claude.exe").ok_or("claude.exe not found on PATH")?,
            args: vec![],
            additions: vec![("DISABLE_AUTOUPDATER", "1".into())],
            note: None,
        }),
        AgentKind::Grok => Ok(Launch {
            program: find_on_path(&path, "grok.exe").ok_or("grok.exe not found on PATH")?,
            // Both switches are documented in grok 1.0.62. Without them the first S1 launch updated
            // Grok from 1.0.5 to 1.0.62 (LESSONS L26).
            args: vec!["--no-auto-update".into()],
            additions: vec![("GROK_DISABLE_AUTOUPDATER", "1".into())],
            note: None,
        }),
        AgentKind::Codex => {
            let (program, mut args, outside) = match find_on_path(&path, "codex.exe") {
                Some(program) => (program, vec![], None),
                None => through_shim(&path, &npm, "codex.cmd")?,
            };
            // Codex has no environment switch for its update check (LESSONS L7).
            args.extend(["-c".into(), "check_for_update_on_startup=false".into()]);
            Ok(Launch { program, args, additions: vec![], note: None }.found_in(outside, &path))
        }
        AgentKind::Gemini => {
            let (program, args, outside) = through_shim(&path, &npm, "gemini.cmd")?;
            // Folder trust is checked at launch, once the session's actual folder is known.
            // 0.64.0 has no auto-update switch on the command line or in the environment; its
            // `general.enableAutoUpdate` is a setting in the user's own settings file.
            Ok(Launch { program, args, additions: vec![], note: None }.found_in(outside, &path))
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
            note: None,
        }),
        AgentKind::Opencode => {
            // The session runs `serve` and `--server` with these (ARCHITECTURE §5, `opencode`). Its
            // conversations live in the session's own data folder, which is how they resume.
            let (program, args, outside) = through_shim(&path, &npm, "opencode.cmd")?;
            if let Some(unfinished) = chorus_core::opencode::unfinished_install(std::fs::metadata(&program).ok().map(|m| m.len()), None) {
                return Err(unfinished);
            }
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
                note: None,
            }
            .found_in(outside, &path))
        }
    }
}

/// npm's global folder, where `npm install -g` puts its shims: `%APPDATA%\npm` unless the user
/// set another prefix. npm doesn't always put it on PATH (LESSONS L68).
fn npm_global_dirs(parent_env: &[(String, String)]) -> Vec<PathBuf> {
    env_value(parent_env, "APPDATA").map(|a| PathBuf::from(a).join("npm")).into_iter().collect()
}

/// The program and leading arguments an npm shim really runs, looked for on PATH and then in
/// `fallback`; the last value is the folder it was found in when that isn't on PATH.
fn through_shim(path: &str, fallback: &[PathBuf], shim_name: &str) -> Result<(PathBuf, Vec<String>, Option<PathBuf>), String> {
    let (shim, outside) = match find_on_path(path, shim_name) {
        Some(shim) => (shim, None),
        None => fallback
            .iter()
            .map(|d| (d.join(shim_name), Some(d.clone())))
            .find(|(f, _)| f.is_file())
            .ok_or_else(|| match fallback.first() {
                Some(dir) => format!("{shim_name} not found on PATH or in npm's global folder ({})", dir.display()),
                None => format!("{shim_name} not found on PATH"),
            })?,
    };
    let text = std::fs::read_to_string(&shim).map_err(|e| format!("reading {}: {e}", shim.display()))?;
    let target = parse_npm_shim(&text, &shim.display().to_string())
        .ok_or_else(|| format!("{} is not an npm shim shape Chorus recognises", shim.display()))?;
    match target.kind {
        ShimKind::Executable => Ok((PathBuf::from(target.file), vec![], outside)),
        ShimKind::NodeScript => {
            // npm's shim prefers a node.exe beside itself, then PATH; do the same.
            let beside = shim.parent().map(|d| d.join("node.exe")).filter(|p| p.is_file());
            let node = beside.or_else(|| find_on_path(path, "node.exe")).ok_or("node.exe not found on PATH")?;
            Ok((node, vec![target.file], outside))
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

/// Whether Claude Code will start in `cwd` without asking to trust it, from its own config, read
/// only: through the main working tree's own entry when `cwd` is in a git repository (worktrees
/// included), else through `cwd` or a folder above it (D100, L66). `None` when either can't be read.
pub fn claude_folder_trusted(parent_env: &[(String, String)], cwd: &str) -> Option<bool> {
    let config = read_claude_config(&claude_config_path(parent_env)?)?;
    let facts = crate::git::facts(Path::new(cwd)).ok()?;
    chorus_core::claude::trusted_at(&config, cwd, facts.as_ref().map(|f| f.main_root.as_str()))
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

    /// An `@opencode/cli` shim in `<appdata>\npm`, pointing at a target of `bytes` bytes.
    fn npm_opencode(appdata: &Path, bytes: usize) -> PathBuf {
        let npm = appdata.join("npm");
        let bin = npm.join(r"node_modules\@opencode\cli\bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("opencode.exe"), vec![0u8; bytes]).unwrap();
        let shim = "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\"%dp0%\\node_modules\\@opencode\\cli\\bin\\opencode.exe\"   %*\r\n";
        std::fs::write(npm.join("opencode.cmd"), shim).unwrap();
        npm
    }

    #[test]
    fn an_npm_shim_off_path_is_found_in_npms_global_folder_and_said_so() {
        let dir = std::env::temp_dir().join(format!("chorus-npm-global-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let npm = npm_opencode(&dir, 2 * 1024 * 1024);
        let env = vec![("PATH".into(), r"C:\Windows".into()), ("APPDATA".into(), dir.display().to_string())];
        let launch = resolve(AgentKind::Opencode, &env, &dir.join("scratch")).unwrap();
        assert!(launch.program.ends_with(r"@opencode\cli\bin\opencode.exe"), "{:?}", launch.program);
        let path = launch.additions.iter().find(|(k, _)| *k == "PATH").map(|(_, v)| v.clone());
        assert_eq!(path, Some(format!(r"C:\Windows;{}", npm.display())), "the agent's own tools see the folder too");
        assert!(launch.note.unwrap().contains("isn't on your PATH"));

        // On PATH, nothing is added or said.
        let on_path = vec![("PATH".into(), npm.display().to_string()), ("APPDATA".into(), dir.display().to_string())];
        let launch = resolve(AgentKind::Opencode, &on_path, &dir.join("scratch")).unwrap();
        assert!(launch.additions.iter().all(|(k, _)| *k != "PATH"));
        assert_eq!(launch.note, None);

        // Neither: the error names both places.
        let nowhere = vec![("PATH".into(), r"C:\Windows".into()), ("APPDATA".into(), dir.join("other").display().to_string())];
        let e = resolve(AgentKind::Opencode, &nowhere, &dir).unwrap_err();
        assert!(e.starts_with("opencode.cmd not found on PATH or in npm's global folder"), "{e}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn npms_placeholder_opencode_is_refused_with_the_command_that_finishes_it() {
        let dir = std::env::temp_dir().join(format!("chorus-npm-placeholder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        npm_opencode(&dir, 229);
        let env = vec![("PATH".into(), String::new()), ("APPDATA".into(), dir.display().to_string())];
        let e = resolve(AgentKind::Opencode, &env, &dir.join("scratch")).unwrap_err();
        assert!(e.contains("--allow-scripts=@opencode/cli"), "{e}");
        std::fs::remove_dir_all(dir).unwrap();
    }

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
    fn claude_codes_trust_follows_the_repository() {
        let dir = std::env::temp_dir().join(format!("chorus-claude-trust-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        let plain = dir.join("plain");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(&plain).unwrap();
        let git = |args: &[&str]| assert!(std::process::Command::new("git").arg("-C").arg(&repo).args(args).status().unwrap().success());
        git(&["init", "-q"]);
        git(&["-c", "user.name=t", "-c", "user.email=t@example.invalid", "commit", "-q", "--allow-empty", "-m", "first"]);
        let env = vec![("CLAUDE_CONFIG_DIR".to_owned(), dir.display().to_string())];
        let at = |p: &Path| claude_folder_trusted(&env, &p.display().to_string());
        assert_eq!(at(&repo), None, "no config yet: unknown");
        let key = |p: &Path| p.display().to_string().replace('\\', "/");
        // The folder above both is trusted: the plain folder is, the repository isn't.
        std::fs::write(dir.join(".claude.json"), format!(r#"{{"projects":{{"{}":{{"hasTrustDialogAccepted":true}}}}}}"#, key(&dir))).unwrap();
        assert_eq!(at(&plain), Some(true));
        assert_eq!(at(&repo.join("src")), Some(false));
        // The repository's own entry trusts it and everything in it.
        std::fs::write(dir.join(".claude.json"), format!(r#"{{"projects":{{"{}":{{"hasTrustDialogAccepted":true}}}}}}"#, key(&repo))).unwrap();
        assert_eq!(at(&repo.join("src")), Some(true));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
