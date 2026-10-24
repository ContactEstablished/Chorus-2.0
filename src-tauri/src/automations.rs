//! Automations (P8): a Bot run on a schedule, only while Chorus runs. Each firing creates an
//! ordinary session through the normal launch path and records a row, including firings that
//! started nothing and why: missed while Chorus was closed, skipped because a session an earlier
//! run started is still busy (D85), or failed (decision 3). Once a run finished and nobody took
//! its session over, Chorus closes its agent unless the automation says not to (D84).
//!
//! The first prompt is persisted (scrubbed, source `automation`) when the session is created and
//! passed as Claude Code's positional prompt at launch, because no structured "ready" signal
//! exists before a first turn and D78 forbids inferring one. The run's own hook report of that
//! prompt is matched to the row through the shared delivery receipts, so it is recorded once.

use std::sync::Arc;
use std::time::Duration;

use chorus_core::activity::Activity;
use chorus_core::agent::AgentKind;
use chorus_core::automation::{self, RunSession};
use chorus_core::schedule::{self as sched, Due, Schedule};
use chorus_core::scrub::{builtin_patterns, scrub_all};
use chorus_core::session::{RunEnd, clean_name};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::sessions::{CreateSession, SessionManager, SessionStatus};
use crate::store::sessions::SessionRow;
use crate::store::{Store, now_ms, projects as project_rows};
use crate::vault;
use crate::vault::commands::secs;

type Manager<'a> = State<'a, Arc<SessionManager>>;

const MAX_AUTOMATIONS: u32 = 100;
const MAX_PROMPT_BYTES: usize = 16 * 1024;
const BACKSTOP: Duration = Duration::from_secs(30);
const WEEK_MS: i64 = 7 * 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Trigger {
    Schedule,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// A session was started.
    Started,
    /// Nothing started, and the note says why.
    Failed,
    /// It was due while Chorus was closed.
    Missed,
    /// A session an earlier run started was still busy (D85).
    Skipped,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AutomationRun {
    pub id: u32,
    pub automation_id: u32,
    pub session_id: Option<u32>,
    pub trigger: Trigger,
    pub due_at_secs: u32,
    pub started_at_secs: Option<u32>,
    pub outcome: Outcome,
    pub note: Option<String>,
    /// For a started run: how its session is now, from its state and how its run ended.
    pub session_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Automation {
    pub id: u32,
    pub name: String,
    pub bot_id: Option<u32>,
    pub bot_name: Option<String>,
    pub prompt: String,
    pub schedule: Schedule,
    pub schedule_text: String,
    pub cwd: String,
    pub project_id: Option<u32>,
    pub enabled: bool,
    /// D84: close a run's agent once the run finished and nobody took it over.
    pub close_on_success: bool,
    pub next_run_at_secs: Option<u32>,
    pub last_run: Option<AutomationRun>,
    pub runs_this_week: u32,
    /// Why it can't run now (its Bot or folder is gone), in words to show as is.
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AutomationSave {
    pub id: Option<u32>,
    pub name: String,
    pub bot_id: u32,
    pub prompt: String,
    pub schedule: Schedule,
    pub cwd: String,
    pub project_id: Option<u32>,
    pub enabled: bool,
    pub close_on_success: bool,
}

struct AutoRow {
    id: u32,
    name: String,
    bot_id: Option<u32>,
    prompt: String,
    schedule: Schedule,
    cwd: String,
    project_id: Option<u32>,
    enabled: bool,
    next_run_at: Option<i64>,
    close_on_success: bool,
}

const COLUMNS: &str = "id, name, bot_id, prompt, schedule, cwd, project_id, enabled, next_run_at, close_on_success";

fn row_from(r: &Row<'_>) -> rusqlite::Result<AutoRow> {
    let schedule: String = r.get(4)?;
    Ok(AutoRow {
        id: r.get(0)?,
        name: r.get(1)?,
        bot_id: r.get(2)?,
        prompt: r.get(3)?,
        schedule: serde_json::from_str(&schedule)
            .map_err(|_| rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, "unreadable schedule".into()))?,
        cwd: r.get(5)?,
        project_id: r.get(6)?,
        enabled: r.get(7)?,
        next_run_at: r.get(8)?,
        close_on_success: r.get(9)?,
    })
}

fn rows(store: &Store) -> Result<Vec<AutoRow>, String> {
    store.with(|c| {
        let mut stmt = c.prepare(&format!("SELECT {COLUMNS} FROM automations ORDER BY name"))?;
        stmt.query_map([], row_from)?.collect()
    })
}

fn row(store: &Store, id: u32) -> Result<Option<AutoRow>, String> {
    store.with(|c| c.query_row(&format!("SELECT {COLUMNS} FROM automations WHERE id = ?1"), [id], row_from).optional())
}

/// The local offset, in minutes east of UTC, at a UTC instant, from SQLite's `localtime`, which
/// follows the system's time zone and its daylight-saving rules.
fn offset_at(store: &Store, utc_ms: i64) -> i32 {
    store
        .with(|c| {
            c.query_row(
                "SELECT CAST(ROUND((julianday(?1 / 1000.0, 'unixepoch', 'localtime') - julianday(?1 / 1000.0, 'unixepoch')) * 1440) AS INTEGER)",
                [utc_ms],
                |r| r.get(0),
            )
        })
        .unwrap_or(0)
}

fn next_after(store: &Store, schedule: &Schedule, after: i64, anchor: i64) -> i64 {
    sched::next_run(schedule, after, anchor, &|t| offset_at(store, t))
}

fn session_status(m: &SessionManager, session_id: u32) -> Option<String> {
    let s = m.summary(session_id).ok()?;
    Some(match (s.status, s.live) {
        (SessionStatus::Closed, _) => "closed".into(),
        (_, true) => match s.activity {
            Activity::Working => "working".into(),
            Activity::Ready => "finished its turn".into(),
            Activity::AwaitingApproval => "waiting for an approval".into(),
            Activity::NeedsAnswer => "waiting for an answer".into(),
            Activity::Failed => "its turn failed".into(),
            _ => "running".into(),
        },
        // Only `finish` ends a run as closed and leaves its session open (D84).
        _ if crate::store::sessions::last_run(m.store(), session_id).ok().flatten().is_some_and(|r| r.end_reason == Some(RunEnd::Closed)) => {
            "finished, and Chorus closed its agent".into()
        }
        _ => match s.last_exit_code {
            Some(0) => "exited".into(),
            Some(code) => format!("exited with code {code}"),
            None => s.note.unwrap_or_else(|| "not running".into()),
        },
    })
}

fn run_from(r: &Row<'_>) -> rusqlite::Result<AutomationRun> {
    let trigger: String = r.get(3)?;
    let outcome: String = r.get(6)?;
    Ok(AutomationRun {
        id: r.get(0)?,
        automation_id: r.get(1)?,
        session_id: r.get(2)?,
        trigger: if trigger == "manual" { Trigger::Manual } else { Trigger::Schedule },
        due_at_secs: secs(r.get(4)?),
        started_at_secs: r.get::<_, Option<i64>>(5)?.map(secs),
        outcome: match outcome.as_str() {
            "started" => Outcome::Started,
            "missed" => Outcome::Missed,
            "skipped" => Outcome::Skipped,
            _ => Outcome::Failed,
        },
        note: r.get(7)?,
        session_status: None,
    })
}

const RUN_COLUMNS: &str = "id, automation_id, session_id, trigger, due_at, started_at, outcome, note";

fn runs_of(m: &SessionManager, automation: u32, limit: u32) -> Result<Vec<AutomationRun>, String> {
    let mut out: Vec<AutomationRun> = m.store().with(|c| {
        let mut stmt = c.prepare(&format!("SELECT {RUN_COLUMNS} FROM automation_runs WHERE automation_id = ?1 ORDER BY id DESC LIMIT ?2"))?;
        stmt.query_map(params![automation, limit], run_from)?.collect()
    })?;
    for r in &mut out {
        r.session_status = r.session_id.and_then(|s| session_status(m, s));
    }
    Ok(out)
}

fn record_run(store: &Store, automation: u32, trigger: Trigger, due_at: i64, outcome: Outcome, session: Option<u32>, note: Option<&str>) -> Result<u32, String> {
    let trigger = if trigger == Trigger::Manual { "manual" } else { "schedule" };
    let outcome = match outcome {
        Outcome::Started => "started",
        Outcome::Failed => "failed",
        Outcome::Missed => "missed",
        Outcome::Skipped => "skipped",
    };
    let started = (session.is_some()).then(now_ms);
    store.with(|c| {
        c.execute(
            "INSERT INTO automation_runs (automation_id, session_id, trigger, due_at, started_at, outcome, note) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![automation, session, trigger, due_at, started, outcome, note],
        )?;
        Ok(c.last_insert_rowid() as u32)
    })
}

/// Why an automation can't start a session now, if it can't.
fn problem(store: &Store, a: &AutoRow) -> Option<String> {
    let Some(bot_id) = a.bot_id else { return Some("Its Bot was deleted. Choose another.".into()) };
    let bot = match crate::bots::get(store, bot_id) {
        Ok(Some(b)) => b,
        Ok(None) => return Some("Its Bot was deleted. Choose another.".into()),
        Err(e) => return Some(e),
    };
    if bot.archived {
        return Some(format!("Its Bot {} is archived.", bot.name));
    }
    if let Some(reason) = unsupported_agent(bot.agent) {
        return Some(reason);
    }
    if !std::path::Path::new(&a.cwd).is_dir() {
        return Some(format!("The folder {} no longer exists.", a.cwd));
    }
    None
}

/// Agents whose first prompt Chorus can pass at launch, verified. Others are refused visibly.
fn unsupported_agent(agent: AgentKind) -> Option<String> {
    (agent != AgentKind::Claude).then(|| {
        format!(
            "Automations start Claude Code Bots for now: passing a first prompt to {} at launch isn't verified.",
            agent.display_name()
        )
    })
}

fn view(m: &SessionManager, a: &AutoRow) -> Automation {
    let store = m.store();
    let last_run = runs_of(m, a.id, 1).ok().and_then(|mut v| v.pop());
    let runs_this_week = store
        .with(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM automation_runs WHERE automation_id = ?1 AND outcome = 'started' AND due_at > ?2",
                params![a.id, now_ms() - WEEK_MS],
                |r| r.get(0),
            )
        })
        .unwrap_or(0);
    Automation {
        id: a.id,
        name: a.name.clone(),
        bot_id: a.bot_id,
        bot_name: a.bot_id.and_then(|id| crate::bots::get(store, id).ok().flatten()).map(|b| b.name),
        prompt: a.prompt.clone(),
        schedule_text: sched::describe(&a.schedule),
        schedule: a.schedule.clone(),
        cwd: a.cwd.clone(),
        project_id: a.project_id,
        enabled: a.enabled,
        close_on_success: a.close_on_success,
        next_run_at_secs: a.next_run_at.filter(|_| a.enabled).map(secs),
        last_run,
        runs_this_week,
        problem: problem(store, a),
    }
}

/// A prompt Chorus will keep and pass on the command line: one line or more, no control
/// characters, and not something the CLI would read as a subcommand. It goes after `--`, so one
/// starting with "-" (a Markdown list) is still a prompt, not an option.
fn check_prompt(store: &Store, prompt: &str) -> Result<String, String> {
    let p = chorus_core::actions::clean_prompt(prompt)?;
    let p = p.trim().to_owned();
    if p.len() > MAX_PROMPT_BYTES {
        return Err("A prompt is up to 16 KiB.".into());
    }
    if !p.contains(char::is_whitespace) {
        return Err("Write the prompt as a sentence: a single word could be read as one of the agent's commands.".into());
    }
    crate::bots::refuse_secrets(store, "the prompt", &p)?;
    Ok(p)
}

fn save(m: &SessionManager, input: &AutomationSave) -> Result<Automation, String> {
    let store = m.store();
    if input.name.trim().is_empty() {
        return Err("An automation needs a name.".into());
    }
    let name = clean_name(Some(&input.name), "")?;
    let bot = crate::bots::get(store, input.bot_id)?.ok_or("That Bot no longer exists.")?;
    if bot.archived {
        return Err(format!("{} is archived.", bot.name));
    }
    if let Some(reason) = unsupported_agent(bot.agent) {
        return Err(reason);
    }
    let prompt = check_prompt(store, &input.prompt)?;
    let schedule = sched::check(&input.schedule)?;
    let cwd = chorus_core::folders::trim_trailing_separators(input.cwd.trim().trim_matches('"').trim());
    let path = std::path::Path::new(&cwd);
    if cwd.is_empty() || cwd.len() > 1024 || !path.is_absolute() || !path.is_dir() {
        return Err(format!("{cwd} is not a folder that exists."));
    }
    if let Some(p) = input.project_id {
        project_rows::get(store, p)?.ok_or("That project no longer exists.")?;
    }
    let now = now_ms();
    let schedule_json = serde_json::to_string(&schedule).map_err(|e| e.to_string())?;
    let next = input.enabled.then(|| next_after(store, &schedule, now, now));
    let unique = |e: String| if e.contains("UNIQUE") { format!("an automation named {name:?} already exists") } else { e };
    let id = match input.id {
        Some(id) => {
            row(store, id)?.ok_or("That automation no longer exists.")?;
            store
                .with(|c| {
                    c.execute(
                        "UPDATE automations SET name = ?2, bot_id = ?3, prompt = ?4, schedule = ?5, cwd = ?6, project_id = ?7,
                                enabled = ?8, next_run_at = ?9, updated_at = ?10, close_on_success = ?11 WHERE id = ?1",
                        params![id, name, input.bot_id, prompt, schedule_json, cwd, input.project_id, input.enabled, next, now, input.close_on_success],
                    )
                })
                .map_err(unique)?;
            id
        }
        None => {
            let count: u32 = store.with(|c| c.query_row("SELECT COUNT(*) FROM automations", [], |r| r.get(0)))?;
            if count >= MAX_AUTOMATIONS {
                return Err(format!("Chorus keeps up to {MAX_AUTOMATIONS} automations."));
            }
            store
                .with(|c| {
                    c.execute(
                        "INSERT INTO automations (name, bot_id, prompt, schedule, cwd, project_id, enabled, next_run_at, created_at, updated_at, close_on_success)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10)",
                        params![name, input.bot_id, prompt, schedule_json, cwd, input.project_id, input.enabled, next, now, input.close_on_success],
                    )?;
                    Ok(c.last_insert_rowid() as u32)
                })
                .map_err(unique)?
        }
    };
    Ok(view(m, &row(store, id)?.ok_or("That automation no longer exists.")?))
}

/// The sessions this automation's runs started that are still running, newest first.
fn running_sessions(store: &Store, automation: u32) -> Vec<u32> {
    store
        .with(|c| {
            let mut stmt = c.prepare(
                "SELECT DISTINCT r.session_id FROM automation_runs r JOIN sessions s ON s.id = r.session_id
                 WHERE r.automation_id = ?1 AND s.state = 'running' ORDER BY r.session_id DESC",
            )?;
            stmt.query_map([automation], |r| r.get(0))?.collect()
        })
        .unwrap_or_default()
}

/// What Chorus knows about a live session an automation run started: its name, its current run
/// and the facts D84 and D85 decide on.
fn run_session(m: &SessionManager, session: u32) -> Option<(String, u32, RunSession)> {
    let s = m.summary(session).ok().filter(|s| s.live)?;
    let (run_id, typed) = m.run_input(session)?;
    let store = m.store();
    // The run's first prompt not yet seen: it is starting. A relaunched run has none to see.
    let starting = crate::store::delivery::unobserved(store, run_id).unwrap_or(true);
    let (runs, other_prompts): (u32, u32) = store
        .with(|c| {
            c.query_row(
                "SELECT (SELECT COUNT(*) FROM runs WHERE session_id = ?1),
                        (SELECT COUNT(*) FROM prompts WHERE session_id = ?1 AND source != 'automation')",
                [session],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
        })
        .ok()?;
    let drafted = crate::store::composer::draft(store, session).is_ok_and(|d| !d.text.trim().is_empty() || !d.images.is_empty())
        || crate::store::composer::oldest(store, session).ok().flatten().is_some();
    let facts = RunSession { activity: s.activity, starting, first_run: runs == 1, other_prompts: other_prompts > 0, typed, drafted, locked: s.locked };
    Some((s.name, run_id, facts))
}

/// A busy session from any of this automation's runs, not only the latest (D85).
fn busy_session(m: &SessionManager, automation: u32) -> Option<String> {
    running_sessions(m.store(), automation).into_iter().find_map(|id| {
        let (name, _, facts) = run_session(m, id)?;
        automation::busy(&facts)
            .then(|| format!("Skipped: {name}, from an earlier run, is still {}.", session_status(m, id).unwrap_or_else(|| "running".into())))
    })
}

const FINISHED_NOTE: &str = "Its automation run finished, so Chorus closed the agent. Restart to follow up.";

/// D84: for each automation that asks for it, closes the agent of every run that finished with
/// nobody taking it over. The session stays, exited, for review and follow-up.
fn close_finished(m: &SessionManager) {
    for a in rows(m.store()).unwrap_or_default().into_iter().filter(|a| a.close_on_success) {
        for id in running_sessions(m.store(), a.id) {
            let Some((_, run_id, facts)) = run_session(m, id) else { continue };
            if automation::finished(&facts)
                && let Err(e) = m.finish(id, run_id, FINISHED_NOTE)
            {
                eprintln!("chorus: automation {}: closing session {id}: {e}", a.id);
            }
        }
    }
}

/// Starts one run of an automation now and records it. Never panics; every outcome has a row.
fn fire(m: &Arc<SessionManager>, a: &AutoRow, trigger: Trigger, due_at: i64) -> Result<u32, String> {
    // Run now and the scheduler can fire the same automation together; one at a time, so the
    // second sees the first's session as busy and is skipped rather than starting a twin.
    static FIRING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = FIRING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let store = m.store();
    if let Some(p) = problem(store, a) {
        return record_run(store, a.id, trigger, due_at, Outcome::Failed, None, Some(&p));
    }
    if let Some(reason) = busy_session(m, a.id) {
        return record_run(store, a.id, trigger, due_at, Outcome::Skipped, None, Some(&reason));
    }
    let bot = crate::bots::get(store, a.bot_id.unwrap_or(0))?.ok_or("Its Bot was deleted.")?;
    let number: u32 = store.with(|c| c.query_row("SELECT COUNT(*) + 1 FROM automation_runs WHERE automation_id = ?1 AND outcome = 'started'", [a.id], |r| r.get(0)))?;
    let request = CreateSession {
        agent: bot.agent,
        cwd: a.cwd.clone(),
        name: Some(run_name(&a.name, number)),
        credential: bot.credential.clone(),
        cols: 120,
        rows: 30,
        options: bot.options.clone(),
        provider_id: bot.provider_id,
        description: Some(format!("Automation: {}", a.name)),
        project_id: a.project_id,
        tags: bot.tags.clone(),
        workspace: Default::default(),
        routing: None,
        bot_id: Some(bot.id),
        first_prompt: Some(a.prompt.clone()),
    };
    match m.create(request) {
        Ok(s) if s.live => record_run(store, a.id, trigger, due_at, Outcome::Started, Some(s.id), None),
        Ok(s) => {
            // Made, but its run didn't start: the prompt was never passed on, so it isn't kept.
            unstage(store, s.id);
            let note = s.note.unwrap_or_else(|| "The session didn't start.".into());
            record_run(store, a.id, trigger, due_at, Outcome::Failed, Some(s.id), Some(&note))
        }
        Err(e) => record_run(store, a.id, trigger, due_at, Outcome::Failed, None, Some(&format!("Couldn't start: {e}"))),
    }
}

/// A run's session name: the automation's, shortened if need be so " #N" still fits the session
/// name limit (an 80-character automation name would otherwise fail every firing).
fn run_name(name: &str, number: u32) -> String {
    let suffix = format!(" #{number}");
    let room = chorus_core::session::MAX_NAME_CHARS.saturating_sub(suffix.chars().count());
    let base: String = name.chars().take(room).collect();
    format!("{}{suffix}", base.trim_end())
}

/// One pass of the scheduler: everything due fires or is recorded as missed, and gets its next
/// time. Returns the soonest next time.
fn tick(m: &Arc<SessionManager>, app_started: i64) -> Option<i64> {
    let now = now_ms();
    let mut soonest: Option<i64> = None;
    for listed in rows(m.store()).unwrap_or_default() {
        // Read again just before deciding: an edit, pause or delete since the list was read wins.
        let Some(a) = row(m.store(), listed.id).ok().flatten().filter(|a| a.enabled) else { continue };
        let Some(next) = a.next_run_at else { continue };
        let fired = match sched::due(next, now, app_started) {
            Due::Wait => None,
            Due::Missed => Some(record_run(m.store(), a.id, Trigger::Schedule, next, Outcome::Missed, None, Some("Missed: Chorus was closed."))),
            Due::Fire => Some(fire(m, &a, Trigger::Schedule, next)),
        };
        let next = if fired.is_some() {
            if let Some(Err(e)) = &fired {
                eprintln!("chorus: automation {}: {e}", a.id);
            }
            let n = next_after(m.store(), &a.schedule, now, next);
            // Only if nobody saved or paused it meanwhile: their next time (or pause) stands.
            let _ = m.store().with(|c| {
                c.execute("UPDATE automations SET next_run_at = ?2 WHERE id = ?1 AND next_run_at = ?3 AND enabled = 1", params![a.id, n, next])
            });
            n
        } else {
            next
        };
        soonest = Some(soonest.map_or(next, |s| s.min(next)));
    }
    soonest
}

/// The scheduler thread: wakes at the next due time, and every 30 s as a backstop (a saved or
/// enabled automation, or a clock change, is picked up then). Each pass first closes the agents of
/// runs that finished (D84).
pub fn start(manager: &Arc<SessionManager>) -> Result<(), String> {
    let app_started = now_ms();
    let m = manager.clone();
    std::thread::Builder::new()
        .name("automations".into())
        .spawn(move || {
            while !m.is_quitting() {
                close_finished(&m);
                let soonest = tick(&m, app_started);
                let wait = soonest.map_or(BACKSTOP, |t| Duration::from_millis(u64::try_from((t - now_ms()).max(1000)).unwrap_or(1000)));
                std::thread::sleep(wait.min(BACKSTOP));
            }
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Persists an automation's first prompt for a session just made: scrubbed, source `automation`,
/// with no run yet. The run that passes it links it (`launched`).
pub fn stage_prompt(store: &Store, session_id: u32, prompt: Option<&str>) -> Result<(), String> {
    let Some(p) = prompt else { return Ok(()) };
    let (keys, _) = vault::all_for_scrubbing(store);
    let text = String::from_utf8_lossy(&scrub_all(p.as_bytes(), &keys, &builtin_patterns())).into_owned();
    crate::store::history::insert_prompt(store, session_id, None, "automation", &text).map(|_| ())
}

/// A staged first prompt, as the launch passes it.
#[derive(Debug, Clone)]
pub struct Staged {
    pub prompt_id: u32,
    pub text: String,
}

/// The session's staged first prompt, for a Claude Code run that hasn't passed it yet.
pub fn staged(store: &Store, row: &SessionRow) -> Option<Staged> {
    if row.agent != AgentKind::Claude {
        return None;
    }
    store
        .with(|c| {
            c.query_row(
                "SELECT p.id, p.text FROM prompts p WHERE p.session_id = ?1 AND p.run_id IS NULL AND p.source = 'automation'
                   AND NOT EXISTS (SELECT 1 FROM prompt_deliveries d WHERE d.prompt_id = p.id)
                 ORDER BY p.id LIMIT 1",
                [row.id],
                |r| Ok(Staged { prompt_id: r.get(0)?, text: r.get(1)? }),
            )
            .optional()
        })
        .ok()
        .flatten()
}

/// The run passed the prompt: it belongs to the run, and its receipt says written, so the hook's
/// echo of it is matched once (D75) rather than recorded again.
pub fn launched(store: &Store, session_id: u32, run_id: u32, p: &Staged) {
    let request = chorus_core::agent::uuid_v4(vault::random_bytes(16).try_into().unwrap_or([0; 16]));
    let link = || {
        store.with(|c| {
            let tx = c.unchecked_transaction()?;
            tx.execute("UPDATE prompts SET run_id = ?2 WHERE id = ?1", params![p.prompt_id, run_id])?;
            tx.execute(
                "INSERT INTO prompt_deliveries (prompt_id, session_id, run_id, request_id, status, attempted_at) VALUES (?1, ?2, ?3, ?4, 'written', ?5)",
                params![p.prompt_id, session_id, run_id, request, now_ms()],
            )?;
            tx.commit()
        })
    };
    if (0..3).any(|_| link().is_ok()) {
        return;
    }
    // The prompt was passed on, but couldn't be tied to its run. A staged prompt is passed again at
    // the next start, which would run the task twice, so it goes; the hook's echo still records it.
    eprintln!("chorus: automation prompt {} couldn't be linked to run {run_id}; unstaged", p.prompt_id);
    unstage(store, session_id);
}

/// A staged prompt that no run passed on goes.
fn unstage(store: &Store, session_id: u32) {
    let _ = store.with(|c| {
        c.execute(
            "DELETE FROM prompts WHERE session_id = ?1 AND run_id IS NULL AND source = 'automation'
               AND NOT EXISTS (SELECT 1 FROM prompt_deliveries d WHERE d.prompt_id = prompts.id)",
            [session_id],
        )
    });
}

async fn blocking<T: Send + 'static>(m: &Manager<'_>, f: impl FnOnce(Arc<SessionManager>) -> Result<T, String> + Send + 'static) -> Result<T, String> {
    let m = m.inner().clone();
    tauri::async_runtime::spawn_blocking(move || f(m)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
pub fn automations_list(state: Manager<'_>) -> Result<Vec<Automation>, String> {
    Ok(rows(state.store())?.iter().map(|a| view(&state, a)).collect())
}

#[tauri::command]
#[specta::specta]
pub fn automation_save(state: Manager<'_>, input: AutomationSave) -> Result<Automation, String> {
    save(&state, &input)
}

#[tauri::command]
#[specta::specta]
pub fn automation_set_enabled(state: Manager<'_>, id: u32, enabled: bool) -> Result<Automation, String> {
    let a = row(state.store(), id)?.ok_or("That automation no longer exists.")?;
    let now = now_ms();
    let next = enabled.then(|| next_after(state.store(), &a.schedule, now, now));
    state.store().with(|c| c.execute("UPDATE automations SET enabled = ?2, next_run_at = ?3, updated_at = ?4 WHERE id = ?1", params![id, enabled, next, now]))?;
    Ok(view(&state, &row(state.store(), id)?.ok_or("That automation no longer exists.")?))
}

#[tauri::command]
#[specta::specta]
pub fn automation_delete(state: Manager<'_>, id: u32) -> Result<bool, String> {
    state.store().with(|c| c.execute("DELETE FROM automations WHERE id = ?1", [id]).map(|n| n > 0))
}

/// Runs an automation now, whether it is paused or not, and returns the run's row.
#[tauri::command]
#[specta::specta]
pub async fn automation_run_now(state: Manager<'_>, id: u32) -> Result<AutomationRun, String> {
    blocking(&state, move |m| {
        let a = row(m.store(), id)?.ok_or("That automation no longer exists.")?;
        let run = fire(&m, &a, Trigger::Manual, now_ms())?;
        runs_of(&m, id, 50)?.into_iter().find(|r| r.id == run).ok_or_else(|| "The run wasn't recorded.".into())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub fn automation_runs(state: Manager<'_>, id: u32) -> Result<Vec<AutomationRun>, String> {
    runs_of(&state, id, 50)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_local_offset_is_minutes_east_of_utc() {
        let s = Store::in_memory();
        let o = offset_at(&s, 1_792_713_600_000);
        assert!((-14 * 60..=14 * 60).contains(&o));
    }

    #[test]
    fn a_long_name_still_leaves_room_for_the_run_number() {
        assert_eq!(run_name("Nightly", 3), "Nightly #3");
        let long = "x".repeat(80);
        let name = run_name(&long, 12);
        assert_eq!(name.chars().count(), chorus_core::session::MAX_NAME_CHARS);
        assert!(name.ends_with(" #12"));
        assert!(chorus_core::session::clean_name(Some(&name), "s").is_ok());
    }

    #[test]
    fn a_prompt_is_checked_before_it_is_kept() {
        let s = Store::in_memory();
        assert!(check_prompt(&s, "--help").is_err());
        // A Markdown list is a prompt: it goes after `--` on the command line.
        assert_eq!(check_prompt(&s, "- check the build").unwrap(), "- check the build");
        assert!(check_prompt(&s, "update").is_err());
        assert!(check_prompt(&s, &format!("use sk-or-v1-{} now", "z9".repeat(20))).unwrap_err().contains("key"));
        assert_eq!(check_prompt(&s, "  Summarise the open issues  ").unwrap(), "Summarise the open issues");
    }
}
