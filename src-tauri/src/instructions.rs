//! A run's instructions at launch (P8): composed from the session's copy of its Bot, the notes
//! that apply now (read fresh at each run start, decision 2) and the Bot's skill folders, then
//! scrubbed and written to a per-run file in the session's state folder. Claude Code reads it with
//! `--append-system-prompt-file`; agents without a verified mechanism get nothing, and the session
//! says why. The file is deleted when the run ends, and at start-up for runs that ended with an
//! earlier Chorus. Nothing is written to the repository or the user's agent configuration.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chorus_core::agent::AgentKind;
use chorus_core::instructions::{Carrier, RunInstructions, Skill, carrier, compose, parse_skill, text_hash};
use chorus_core::scrub::{builtin_patterns, scrub_all};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::sessions::SessionManager;
use crate::store::Store;
use crate::store::sessions::SessionRow;

type Manager<'a> = State<'a, Arc<SessionManager>>;

/// Instruction files are `instructions-<n>.md`, one per run, so a slow-exiting old run can't
/// delete the file a Restart has just written for the new one.
const FILE_PREFIX: &str = "instructions";
const MAX_SKILL_FILE_BYTES: u64 = 64 * 1024;

/// Each live run's instruction file, by run id, deleted when that run ends.
static RUN_FILES: std::sync::Mutex<std::collections::BTreeMap<u32, PathBuf>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

/// What `start_run` adds for the run's instructions.
pub struct Prepared {
    pub args: Vec<String>,
    pub facts: RunInstructions,
    /// For the session's note, when something went wrong or wasn't given.
    pub note: Option<String>,
    /// An automation's first prompt, already persisted, passed last on the command line.
    pub first_prompt: Option<crate::automations::Staged>,
    /// The scrubbed text for the run's Codex bridge to inject.
    pub codex: Option<String>,
    /// The file this run's instructions were written to, if any.
    pub file: Option<PathBuf>,
}

/// A new, unique instruction file name in the session's state folder.
fn new_file_in(state_dir: &Path) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    state_dir.join(format!("{FILE_PREFIX}-{}-{n}.md", crate::store::now_ms()))
}

fn is_instruction_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "md")
        && path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(FILE_PREFIX))
}

fn read_skill(folder: &str) -> Result<Skill, String> {
    use std::io::Read;
    let path = Path::new(folder).join("SKILL.md");
    let mut text = String::new();
    std::fs::File::open(&path)
        .and_then(|f| f.take(MAX_SKILL_FILE_BYTES).read_to_string(&mut text))
        .map_err(|e| format!("{folder}: {e}"))?;
    let (name, description) = parse_skill(&text).unwrap_or((None, None));
    let fallback = Path::new(folder).file_name().map_or_else(|| folder.to_owned(), |n| n.to_string_lossy().into_owned());
    Ok(Skill { name: name.unwrap_or(fallback), description: description.unwrap_or_else(|| "(no description)".into()), path: folder.to_owned() })
}

/// The hash of the text given to the run that started the session's current conversation.
fn conversation_hash(store: &Store, session_id: u32) -> Option<String> {
    let json: Option<String> = store
        .with(|c| {
            c.query_row(
                "SELECT instructions FROM runs WHERE session_id = ?1 AND resumed = 0 ORDER BY id DESC LIMIT 1",
                [session_id],
                |r| r.get(0),
            )
            .optional()
        })
        .ok()
        .flatten()
        .flatten();
    json.and_then(|j| serde_json::from_str::<RunInstructions>(&j).ok()).and_then(|f| f.hash)
}

/// Composes and writes the run's instructions. Never fails the launch: a problem launches without
/// instructions and says so (D19).
pub fn prepare(store: &Store, row: &SessionRow, state_dir: &Path, redact: &[Vec<u8>], parent: &[(String, String)]) -> Prepared {
    let file = new_file_in(state_dir);
    let mut written = None;
    let mut facts = RunInstructions::default();
    let bot = crate::bots::of_session(store, row.id).unwrap_or(None);
    let notes = match crate::notes::for_project(store, row.project_id) {
        Ok(n) => n,
        Err(e) => {
            facts.reason = Some(format!("Notes couldn't be read ({e}), so none were given."));
            Vec::new()
        }
    };
    let mut skills = Vec::new();
    for folder in bot.as_ref().map(|(_, s)| s.skills.as_slice()).unwrap_or_default() {
        match read_skill(folder) {
            Ok(s) => skills.push(s),
            Err(e) => facts.skill_problems.push(e),
        }
    }
    facts.bot = bot.as_ref().filter(|(_, s)| !s.instructions.trim().is_empty()).map(|(_, s)| s.name.clone());
    facts.skills = skills.iter().map(|s| s.name.clone()).collect();
    let composed = compose(bot.as_ref().map(|(_, s)| s.instructions.as_str()), &notes, &skills);
    let mut args = Vec::new();
    let mut note = None;
    let mut given_text: Option<String> = None;
    let mut codex: Option<String> = None;
    if let Some(c) = composed {
        facts.notes = c.notes.clone();
        facts.left_out = c.left_out;
        match carrier(row.agent) {
            Carrier::Unsupported(reason) => {
                facts.reason = Some(reason.to_owned());
                // Nobody expects a plain shell to take instructions, so it gets no warning on every
                // launch; its facts still say why.
                if row.agent != AgentKind::Shell {
                    note = Some(reason.to_owned());
                }
            }
            Carrier::CodexDeveloperInstructions => {
                // Given by the run's Codex bridge once its thread appears (`record`, `codex_injected`).
                let text = String::from_utf8_lossy(&scrub_all(c.text.as_bytes(), redact, &builtin_patterns())).into_owned();
                facts.bytes = u32::try_from(text.len()).unwrap_or(u32::MAX);
                facts.hash = Some(text_hash(&text));
                facts.reason = Some(CODEX_WAITING.into());
                codex = Some(text);
            }
            Carrier::AppendFile(flag) => {
                let text = String::from_utf8_lossy(&scrub_all(c.text.as_bytes(), redact, &builtin_patterns())).into_owned();
                match std::fs::write(&file, &text) {
                    Ok(()) => {
                        args = vec![flag.to_owned(), file.display().to_string()];
                        facts.given = true;
                        facts.bytes = u32::try_from(text.len()).unwrap_or(u32::MAX);
                        facts.hash = Some(text_hash(&text));
                        given_text = Some(text);
                        written = Some(file.clone());
                    }
                    Err(e) => {
                        let why = format!("Chorus couldn't write this run's instructions ({e}), so it started without them.");
                        facts.reason = Some(why.clone());
                        note = Some(why);
                    }
                }
            }
        }
    }
    // Claude Code keeps the system prompt its conversation started with, instructions included,
    // unless told not to (2.1.308 `--system-prompt-snapshot`). A resumed conversation that started
    // with different instructions gets this run's.
    if row.agent == AgentKind::Claude
        && let Some(id) = row.agent_session_id.as_deref()
        && crate::agents::conversation_exists(AgentKind::Claude, id, parent)
        && conversation_hash(store, row.id) != given_text.as_deref().map(text_hash)
    {
        args.extend(["--system-prompt-snapshot".to_owned(), "off".to_owned()]);
        facts.fresh_prompt = true;
    }
    // An automation's first prompt goes last: it is the CLI's positional prompt, after `--` so a
    // prompt starting with "-" (a Markdown list, say) isn't read as an option (checked on 2.1.308).
    let first_prompt = crate::automations::staged(store, row);
    if let Some(p) = &first_prompt {
        args.extend(["--".to_owned(), p.text.clone()]);
    }
    Prepared { args, facts, note, first_prompt, codex, file: written }
}

const CODEX_WAITING: &str = "Given when Codex's conversation starts.";

fn save_facts(store: &Store, run_id: u32, facts: &RunInstructions) {
    if let Ok(json) = serde_json::to_string(facts) {
        let _ = store.with(|c| c.execute("UPDATE runs SET instructions = ?2 WHERE id = ?1", params![run_id, json]));
    }
}

fn facts_of(store: &Store, run_id: u32) -> Option<RunInstructions> {
    let json: Option<String> = store.with(|c| c.query_row("SELECT instructions FROM runs WHERE id = ?1", [run_id], |r| r.get(0)).optional()).ok()??;
    serde_json::from_str(&json?).ok()
}

/// The hash of the newest text an earlier run of the session was given.
fn last_given_hash(store: &Store, session_id: u32, before_run: u32) -> Option<String> {
    let rows: Vec<String> = store
        .with(|c| {
            let mut stmt = c.prepare("SELECT instructions FROM runs WHERE session_id = ?1 AND id < ?2 AND instructions IS NOT NULL ORDER BY id DESC")?;
            stmt.query_map(params![session_id, before_run], |r| r.get(0))?.collect()
        })
        .unwrap_or_default();
    rows.iter().filter_map(|j| serde_json::from_str::<RunInstructions>(j).ok()).find(|f| f.given).and_then(|f| f.hash)
}

/// After the run's row exists: keeps what its instructions held, hands Codex's text to the run's
/// bridge, and links an automation's first prompt to the run so the agent's own report of it is
/// matched, not recorded again. Returns a note for the session when something wasn't given.
pub fn record(store: &Store, session_id: u32, run_id: u32, prepared: &Prepared, bridge: Option<&Arc<crate::sessions::codex_bridge::Bridge>>) -> Option<String> {
    if let Some(file) = &prepared.file {
        RUN_FILES.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(run_id, file.clone());
    }
    let mut facts = prepared.facts.clone();
    let mut note = None;
    if let Some(text) = &prepared.codex {
        match bridge {
            None => {
                facts.reason = Some(chorus_core::instructions::CODEX_UNSUPPORTED.into());
                note = facts.reason.clone();
            }
            // A resumed conversation keeps the developer message an earlier run injected.
            Some(b) if b.resume.is_some() && last_given_hash(store, session_id, run_id) == facts.hash => {
                facts.given = true;
                facts.reason = None;
            }
            Some(b) => {
                let replaces = b.resume.is_some() && last_given_hash(store, session_id, run_id).is_some();
                let text = if replaces { format!("{}{text}", chorus_core::instructions::CODEX_REPLACES) } else { text.clone() };
                b.set_instructions(text, run_id);
            }
        }
    }
    save_facts(store, run_id, &facts);
    if let Some(p) = &prepared.first_prompt {
        crate::automations::launched(store, session_id, run_id, p);
    }
    note
}

/// The Codex bridge injected the run's instructions, or couldn't.
pub fn codex_injected(store: &Store, session_id: u32, run_id: u32, result: Result<(), String>) {
    let Some(mut facts) = facts_of(store, run_id) else { return };
    match result {
        Ok(()) => {
            facts.given = true;
            facts.reason = None;
        }
        Err(e) => {
            let why = format!("Codex didn't take this run's instructions ({e}), so it runs without them.");
            facts.given = false;
            facts.reason = Some(why.clone());
            let _ = crate::store::sessions::set_note(store, session_id, Some(&why));
        }
    }
    save_facts(store, run_id, &facts);
}

/// The run ended: its own instructions file goes, never a newer run's.
pub fn run_ended(run_id: u32) {
    if let Some(file) = RUN_FILES.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(&run_id) {
        let _ = std::fs::remove_file(file);
    }
}

/// At start-up, before any session relaunches: files left by runs that ended with an earlier
/// Chorus go. Each relaunch writes its own again.
pub fn start(manager: &SessionManager) {
    let Ok(dirs) = std::fs::read_dir(manager.data_dir().join("state")) else { return };
    for dir in dirs.flatten() {
        let Ok(files) = std::fs::read_dir(dir.path()) else { continue };
        for f in files.flatten().map(|f| f.path()).filter(|p| is_instruction_file(p)) {
            let _ = std::fs::remove_file(f);
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionBot {
    /// None once the Bot is deleted; the session keeps its copy.
    pub id: Option<u32>,
    pub name: String,
    pub has_instructions: bool,
    pub skills: u32,
}

/// What session facts show about instructions.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionInstructions {
    pub bot: Option<SessionBot>,
    /// The newest run's; None before the first run with P8, or for a run before P8.
    pub run: Option<RunInstructions>,
    /// Why this agent gets no instructions, whatever the session holds.
    pub unsupported: Option<String>,
}

pub fn of_session(store: &Store, session_id: u32, agent: AgentKind) -> Result<SessionInstructions, String> {
    let bot = crate::bots::of_session(store, session_id)?.map(|(id, s)| SessionBot {
        id,
        has_instructions: !s.instructions.trim().is_empty(),
        skills: u32::try_from(s.skills.len()).unwrap_or(0),
        name: s.name,
    });
    let json: Option<String> = store
        .with(|c| c.query_row("SELECT instructions FROM runs WHERE session_id = ?1 ORDER BY id DESC LIMIT 1", [session_id], |r| r.get(0)).optional())?
        .flatten();
    let unsupported = match carrier(agent) {
        Carrier::Unsupported(r) => Some(r.to_owned()),
        // Codex depends on the run (its private server), so the run's facts say.
        Carrier::CodexDeveloperInstructions => None,
        Carrier::AppendFile(_) => None,
    };
    Ok(SessionInstructions { bot, run: json.and_then(|j| serde_json::from_str(&j).ok()), unsupported })
}

#[tauri::command]
#[specta::specta]
pub fn session_instructions(state: Manager<'_>, session_id: u32) -> Result<SessionInstructions, String> {
    let agent = crate::store::sessions::get(state.store(), session_id)?.ok_or_else(|| format!("no session {session_id}"))?.agent;
    of_session(state.store(), session_id, agent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::projects::{NewProject, insert as add_project};

    fn session(s: &Store, id: u32, agent: &str, project: Option<u32>) -> SessionRow {
        s.with(|c| {
            c.execute(
                "INSERT INTO sessions (id,name,agent,cwd,state,cols,rows,created_at,project_id) VALUES (?1,'s',?2,'C:\\w','running',80,24,1,?3)",
                params![id, agent, project],
            )
        })
        .unwrap();
        crate::store::sessions::get(s, id).unwrap().unwrap()
    }

    #[test]
    fn a_project_note_reaches_its_projects_claude_run_through_a_file_and_no_other() {
        let s = Store::in_memory();
        let a = add_project(&s, &NewProject { name: "A", folder: None, colour: "#111111" }).unwrap();
        let b = add_project(&s, &NewProject { name: "B", folder: None, colour: "#222222" }).unwrap();
        s.with(|c| {
            c.execute("INSERT INTO notes (project_id,title,body,pinned,created_at,updated_at) VALUES (?1,'Only A','alpha',0,1,1)", [a])?;
            c.execute("INSERT INTO notes (project_id,title,body,pinned,created_at,updated_at) VALUES (NULL,'Everywhere','omega',1,1,1)", [])
        })
        .unwrap();
        let dir = std::env::temp_dir().join(format!("chorus-instr-{}", std::process::id()));
        let (in_a, in_b) = (dir.join("1"), dir.join("2"));
        std::fs::create_dir_all(&in_a).unwrap();
        std::fs::create_dir_all(&in_b).unwrap();
        let secret = format!("sk-ant-{}", "q".repeat(30));
        let row_a = session(&s, 1, "claude", Some(a));
        let pa = prepare(&s, &row_a, &in_a, &[secret.clone().into_bytes()], &[]);
        assert_eq!(pa.args[0], "--append-system-prompt-file");
        let text = std::fs::read_to_string(&pa.args[1]).unwrap();
        assert!(text.contains("alpha") && text.contains("omega"));
        assert!(pa.facts.given && pa.facts.notes.len() == 2 && pa.facts.notes[0].title == "Everywhere");
        let p = prepare(&s, &session(&s, 2, "claude", Some(b)), &in_b, &[], &[]);
        let text = std::fs::read_to_string(&p.args[1]).unwrap();
        assert!(!text.contains("alpha") && text.contains("omega"));
        // Grok gets nothing, and says why.
        let p = prepare(&s, &session(&s, 3, "grok", Some(a)), &in_b, &[], &[]);
        assert!(p.args.is_empty() && !p.facts.given && p.note.is_some_and(|n| n.contains("Grok")) && p.file.is_none());
        // A run's end removes its own file only: a Restart's newer file stays.
        let (old, newer) = (pa.file.clone().unwrap(), prepare(&s, &row_a, &in_a, &[], &[]).file.unwrap());
        assert_ne!(old, newer);
        RUN_FILES.lock().unwrap().extend([(901, old.clone()), (902, newer.clone())]);
        run_ended(901);
        assert!(!old.exists() && newer.exists());
        run_ended(902);
        assert!(!newer.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_automation_prompt_starting_with_a_dash_follows_the_end_of_options() {
        let s = Store::in_memory();
        let row = session(&s, 1, "claude", None);
        s.with(|c| c.execute("INSERT INTO prompts (session_id,run_id,source,text,created_at) VALUES (1,NULL,'automation','- check the build',1)", []))
            .unwrap();
        let dir = std::env::temp_dir().join(format!("chorus-instr-dash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = prepare(&s, &row, &dir, &[], &[]);
        assert_eq!(p.args[p.args.len() - 2..], ["--", "- check the build"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stored_key_in_a_note_is_scrubbed_from_the_file() {
        let s = Store::in_memory();
        // Saving refuses a key-shaped note; one already stored (or a vault key in plain words) is
        // still scrubbed when the file is written.
        let key = "plainvaultsecretvalue1234";
        s.with(|c| c.execute("INSERT INTO notes (project_id,title,body,pinned,created_at,updated_at) VALUES (NULL,'K',?1,0,1,1)", [format!("use {key}")])).unwrap();
        let dir = std::env::temp_dir().join(format!("chorus-instr-key-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = prepare(&s, &session(&s, 1, "claude", None), &dir, &[key.as_bytes().to_vec()], &[]);
        let text = std::fs::read_to_string(&p.args[1]).unwrap();
        assert!(!text.contains(key));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod codex_tests {
    use super::*;

    #[test]
    fn codex_without_its_private_server_gets_nothing_and_says_why() {
        let s = Store::in_memory();
        s.with(|c| {
            c.execute("INSERT INTO sessions (id,name,agent,cwd,state,cols,rows,created_at) VALUES (1,'s','codex','C:/w','running',80,24,1)", [])?;
            c.execute("INSERT INTO runs (id,session_id,program,args,started_at) VALUES (7,1,'codex','[]',1)", [])?;
            c.execute("INSERT INTO notes (project_id,title,body,pinned,created_at,updated_at) VALUES (NULL,'N','body',0,1,1)", [])
        })
        .unwrap();
        let dir = std::env::temp_dir().join(format!("chorus-instr-codex-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let row = crate::store::sessions::get(&s, 1).unwrap().unwrap();
        let p = prepare(&s, &row, &dir, &[], &[]);
        // Nothing in argv or on disk: the text waits for the bridge.
        assert!(p.args.is_empty() && p.file.is_none());
        assert!(p.codex.as_deref().is_some_and(|t| t.contains("body")));
        let note = record(&s, 1, 7, &p, None);
        assert!(note.is_some_and(|n| n.contains("0.163.0")));
        let facts = facts_of(&s, 7).unwrap();
        assert!(!facts.given && facts.reason.is_some_and(|r| r.contains("0.163.0")));
        codex_injected(&s, 1, 7, Ok(()));
        assert!(facts_of(&s, 7).unwrap().given);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// How each agent takes instructions, for Settings → Agents (D19: an unsupported one says why).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InstructionSupport {
    pub agent: AgentKind,
    pub supported: bool,
    pub text: String,
}

pub fn support() -> Vec<InstructionSupport> {
    AgentKind::ALL
        .into_iter()
        .map(|agent| {
            let (supported, text) = match carrier(agent) {
                Carrier::AppendFile(flag) => (true, format!("Gets Bot instructions and notes from a file in Chorus's data folder ({flag}).")),
                Carrier::CodexDeveloperInstructions => (
                    true,
                    "Gets Bot instructions and notes as a developer message through Chorus's private Codex server (Codex 0.163.0); other versions get none.".to_owned(),
                ),
                Carrier::Unsupported(reason) => (false, reason.to_owned()),
            };
            InstructionSupport { agent, supported, text }
        })
        .collect()
}

#[tauri::command]
#[specta::specta]
pub fn instructions_support() -> Vec<InstructionSupport> {
    support()
}
