//! The orchestrator (P15): one worker thread per running run. Each pass reads the run's state from
//! the database, looks at the step in progress, feeds what happened to `chorus_core::run_loop` and
//! does what it says: start a planner or builder session, run the gate, commit, tag, or stop. It
//! never infers anything from a terminal's text: a step's turn ended when the agent's own events
//! say so (D78), and the model that ran it is the one its transcript records (L61).

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use chorus_core::activity::Activity;
use chorus_core::agent::{AgentKind, LaunchOptions};
use chorus_core::run_loop::files::{self, PhaseEntry, Problem, Risk, TaskStatus};
use chorus_core::run_loop::prompts::{self, CheckLine, RUN_DIR};
use chorus_core::run_loop::questions::{self, AnsweredBy};
use chorus_core::run_loop::roles::{self, BotFacts, LoopRole};
use chorus_core::run_loop::script::{self, ScriptStep, StepName};
use chorus_core::run_loop::tools::{self as core_tools, AskClass};
use chorus_core::run_loop::{self as engine, Action, CheckOutcome, Condition, Decision, Event, Stage, Stop, report};
use chorus_core::session::SessionState;

use super::db::{self, NewQuestion, NewStep, PhaseRow, QuestionRow, RunRow, RunState, StepKind, StepRow, StepState};
use super::{gate, tools};
use crate::git;
use crate::sessions::{CreateSession, LoopLaunch, SessionManager};
use crate::store::now_ms;

const PASS: Duration = Duration::from_millis(1500);
const FIRST_PASS: Duration = Duration::from_secs(3);
const FINISHED_NOTE: &str = "Its loop step finished, so Chorus closed the agent. Restart to follow up.";
const INTERRUPTED: &str = "Chorus restarted during this loop step, so the loop runs the step again.";
const WAITING_NOTE: &str = "Its loop step asked a question, so Chorus closed the agent until the answer comes; then it carries on in this conversation.";

/// Runs with a worker thread, so a second Start doesn't make a twin.
static ACTIVE: Mutex<Vec<u32>> = Mutex::new(Vec::new());
/// Tells the windows a run changed.
static NOTIFY: OnceLock<Box<dyn Fn(u32) + Send + Sync>> = OnceLock::new();

pub fn set_notify(f: Box<dyn Fn(u32) + Send + Sync>) {
    let _ = NOTIFY.set(f);
}

pub fn notify(run_id: u32) {
    if let Some(f) = NOTIFY.get() {
        f(run_id);
    }
}

/// Before sessions are relaunched at start: a step that was running when Chorus stopped is run
/// again from the start, so its session isn't relaunched as an idle agent and the step is ended. A
/// step waiting on a question keeps waiting: its answer resumes its conversation.
pub fn prepare(m: &SessionManager) {
    let store = m.store();
    for step in db::all_running_steps(store).unwrap_or_default() {
        let waiting = db::pending_for_step(store, step.id).is_ok_and(|q| !q.is_empty());
        if let Some(sid) = step.session_id {
            let note = if waiting { WAITING_NOTE } else { INTERRUPTED };
            let _ = crate::store::sessions::set_state(store, sid, SessionState::Exited, Some(note));
        }
        if !waiting {
            let _ = db::end_step(store, step.id, StepState::Failed, Some("Chorus restarted during this step; it runs again."), None);
        }
    }
}

/// Starts a worker for every running run (after the relaunch has begun).
pub fn resume_all(m: &Arc<SessionManager>) {
    for run in db::runs(m.store()).unwrap_or_default() {
        if run.state == RunState::Running {
            ensure_worker(m, run.id);
        }
    }
}

pub fn ensure_worker(m: &Arc<SessionManager>, run_id: u32) {
    {
        let mut active = ACTIVE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if active.contains(&run_id) {
            return;
        }
        active.push(run_id);
    }
    let m = m.clone();
    let spawned = std::thread::Builder::new().name(format!("loop-{run_id}")).spawn(move || {
        std::thread::sleep(FIRST_PASS);
        work(&m, run_id);
        ACTIVE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).retain(|r| *r != run_id);
    });
    if spawned.is_err() {
        ACTIVE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).retain(|r| *r != run_id);
    }
}

/// The worker: a pass every 1.5 s while the run runs, or while it is paused with a step still
/// going (which finishes, but nothing new starts).
fn work(m: &Arc<SessionManager>, run_id: u32) {
    while !m.is_quitting() {
        let Ok(Some(run)) = db::run(m.store(), run_id) else { return };
        let busy = || current_phase(m, run_id).and_then(|p| db::running_step(m.store(), p.id).ok().flatten()).is_some();
        match run.state {
            RunState::Running => {}
            RunState::Paused if busy() => {}
            _ => return,
        }
        if let Err(e) = pass(m, &run) {
            let _ = db::set_state(m.store(), run_id, RunState::Stopped, Some(&format!("The loop stopped on an error: {e}")));
            notify(run_id);
            return;
        }
        std::thread::sleep(PASS);
    }
}

fn current_phase(m: &SessionManager, run_id: u32) -> Option<PhaseRow> {
    db::phases(m.store(), run_id).ok()?.pop()
}

/// The phases `roadmap.md` lists in the run's worktree.
pub fn roadmap(dir: &Path) -> Result<Vec<PhaseEntry>, String> {
    let path = dir.join(RUN_DIR).join("roadmap.md");
    let text = std::fs::read_to_string(&path).map_err(|_| format!("There is no {RUN_DIR}/roadmap.md on the run branch."))?;
    files::parse_roadmap(&text).map_err(|p| files::describe("roadmap.md", &p))
}

fn worktree(run: &RunRow) -> Result<&Path, String> {
    run.worktree_path.as_deref().map(Path::new).filter(|p| p.is_dir()).ok_or_else(|| "The run's worktree is missing.".to_owned())
}

/// One pass over a run.
pub(super) fn pass(m: &Arc<SessionManager>, run: &RunRow) -> Result<(), String> {
    let store = m.store();
    let dir = worktree(run)?;
    let plan = roadmap(dir)?;
    let phases = db::phases(store, run.id)?;
    let Some(phase) = phases.last().cloned() else {
        let first = plan.first().cloned().ok_or("The roadmap has no phases.")?;
        return begin_phase(m, run, &first);
    };
    let entry = plan.iter().find(|p| p.number == phase.number).cloned().unwrap_or_else(|| PhaseEntry {
        number: phase.number,
        title: phase.title.clone(),
        goal: String::new(),
        risk: Risk::Normal,
        models: Vec::new(),
    });
    match phase.progress.stage {
        Stage::Passed => return after_pass(m, run, &phases, &plan),
        Stage::Stopped => return stop_run(m, run, &phase, phase.stop.as_ref()),
        _ => {}
    }
    // Time spent waiting on a blocking question doesn't count: it waits for Matthew with no timeout.
    let blocked = db::running_step(store, phase.id)?
        .is_some_and(|st| db::pending_for_step(store, st.id).is_ok_and(|q| q.iter().any(|q| q.state == "open" && q.class == "blocking")));
    if let Some(stop) = engine::overdue(&phase.progress, &run.config.limits, now_ms()).filter(|_| !blocked) {
        if let Some(step) = db::running_step(store, phase.id)? {
            end_agent(m, &step, "The phase ran past its time cap.");
        }
        let mut progress = phase.progress.clone();
        progress.stage = Stage::Stopped;
        return apply(m, run, &phase, &entry, Decision { phase: progress, action: Action::Stop(stop), task_stops: Vec::new() });
    }
    match db::running_step(store, phase.id)? {
        Some(step) => match poll(m, run, &phase, &entry, &step)? {
            Some(event) => {
                let d = engine::advance(&phase.progress, event, &run.config.limits);
                apply(m, run, &phase, &entry, d)
            }
            None => Ok(()),
        },
        None if run.state == RunState::Running => {
            let action = engine::resume(&phase.progress, &run.config.limits);
            execute(m, run, &phase, &entry, action)
        }
        None => Ok(()),
    }
}

fn begin_phase(m: &Arc<SessionManager>, run: &RunRow, entry: &PhaseEntry) -> Result<(), String> {
    let start = engine::start(now_ms());
    let id = db::insert_phase(m.store(), run.id, entry.number, &entry.title, &entry.folder(), &start.phase)?;
    let phase = db::phases(m.store(), run.id)?.into_iter().find(|p| p.id == id).ok_or("The new phase wasn't saved.")?;
    notify(run.id);
    execute(m, run, &phase, entry, start.action)
}

/// Saves a decision and acts on it.
fn apply(m: &Arc<SessionManager>, run: &RunRow, phase: &PhaseRow, entry: &PhaseEntry, d: Decision) -> Result<(), String> {
    let stop = match &d.action {
        Action::Stop(s) => Some(s),
        _ => None,
    };
    db::save_phase(m.store(), phase.id, &d.phase, stop, None)?;
    for s in &d.task_stops {
        eprintln!("chorus: loop {}: {} stopped: {}", run.name, s.task.as_deref().unwrap_or("?"), s.detail);
    }
    write_state(m, run);
    notify(run.id);
    let phase = PhaseRow { progress: d.phase.clone(), ..phase.clone() };
    // A paused run finishes what it was doing, and starts nothing new.
    let starts = matches!(d.action, Action::Plan { .. } | Action::Build { .. } | Action::Check { .. } | Action::Tag);
    let state = db::run(m.store(), run.id)?.map_or(run.state, |r| r.state);
    if starts && state != RunState::Running {
        return Ok(());
    }
    execute(m, run, &phase, entry, d.action)
}

fn execute(m: &Arc<SessionManager>, run: &RunRow, phase: &PhaseRow, entry: &PhaseEntry, action: Action) -> Result<(), String> {
    match action {
        Action::Plan { attempt, problems } => {
            let prompt = prompts::plan(entry, problems.as_deref());
            start_step(m, run, phase, entry, StepKind::Plan, None, attempt, &prompt)
        }
        Action::Build { task, attempt, counsel: _ } => {
            let title = phase.progress.tasks.iter().find(|t| t.id == task).map(|t| t.title.clone()).unwrap_or_default();
            let checks: Vec<CheckLine> = run.config.checks.iter().map(|c| CheckLine { name: c.name.clone(), command: c.command.clone() }).collect();
            let prompt = prompts::build(entry, &task, &title, attempt, run.config.limits.fix_attempts, &checks);
            start_step(m, run, phase, entry, StepKind::Build, Some(&task), attempt, &prompt)
        }
        Action::Check { task, attempt } => {
            let event = run_gate(m, run, phase, entry, &task, attempt)?;
            let d = engine::advance(&phase.progress, event, &run.config.limits);
            apply(m, run, phase, entry, d)
        }
        Action::Tag => {
            let event = tag_phase(m, run, phase, entry)?;
            let d = engine::advance(&phase.progress, event, &run.config.limits);
            apply(m, run, phase, entry, d)
        }
        Action::Pass => Ok(()),
        Action::Stop(stop) => stop_run(m, run, phase, Some(&stop)),
        Action::Wait => Ok(()),
    }
}

fn bot_facts(m: &SessionManager) -> Vec<BotFacts> {
    crate::bots::list(m.store())
        .unwrap_or_default()
        .into_iter()
        .filter(|b| !b.archived)
        .map(|b| BotFacts { id: b.id, name: b.name, agent: b.agent, model: b.options.model })
        .collect()
}

/// The task's `- Model:` line in the phase's tasks.md, for the builder override.
fn task_model(dir: &Path, entry: &PhaseEntry, task: &str) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(prompts::phase_dir(entry)).join("tasks.md")).ok()?;
    files::parse_tasks(&text).ok()?.into_iter().find(|t| t.id == task)?.model
}

#[allow(clippy::too_many_arguments)]
fn start_step(m: &Arc<SessionManager>, run: &RunRow, phase: &PhaseRow, entry: &PhaseEntry, kind: StepKind, task: Option<&str>, attempt: u32, prompt: &str) -> Result<(), String> {
    let store = m.store();
    let dir = worktree(run)?;
    let role = if kind == StepKind::Plan { LoopRole::Planner } else { LoopRole::Builder };
    let model_line = task.and_then(|t| task_model(dir, entry, t));
    let resolved = roles::resolve(role, model_line.as_deref(), &entry.models, &run.config.roles, &bot_facts(m));
    let bot = match resolved.and_then(|r| crate::bots::get(store, r.bot)?.ok_or_else(|| "That Bot was deleted.".to_owned())) {
        Ok(b) => b,
        Err(e) => return fail_step(m, run, phase, entry, None, &e),
    };
    let asked = bot.options.model.clone().unwrap_or_else(|| "default".into());
    let step_id = db::insert_step(
        store,
        &NewStep { run_id: run.id, phase_id: phase.id, kind, task, attempt, role: Some(role.key()), bot_id: Some(bot.id), model_asked: Some(&asked) },
    )?;
    notify(run.id);
    if let Some(path) = &run.config.script {
        let event = match play(m, run, entry, step_id, path, kind, task, attempt) {
            Played::Done(e) => e,
            // The step waits, with no session, for its answer.
            Played::Asked => return Ok(()),
        };
        let (state, note) = match &event {
            Event::StepFailed { why } => (StepState::Failed, Some(why.as_str())),
            _ => (StepState::Done, Some("Played from the run's script.")),
        };
        db::end_step(store, step_id, state, note, Some("scripted"))?;
        if matches!(event, Event::Planned(Ok(_))) {
            git::commit_all(dir, &format!("{} · {}: plan", entry.code(), entry.title))?;
        }
        let d = engine::advance(&phase.progress, event, &run.config.limits);
        return apply(m, run, phase, entry, d);
    }
    if bot.agent != AgentKind::Claude {
        let why = format!("Loop steps run Claude Code Bots for now: passing a first prompt to {} isn't verified.", bot.agent.display_name());
        db::end_step(store, step_id, StepState::Failed, Some(&why), None)?;
        let d = engine::advance(&phase.progress, Event::StepFailed { why }, &run.config.limits);
        return apply(m, run, phase, entry, d);
    }
    let what = match (kind, task) {
        (StepKind::Plan, _) => "plan".to_owned(),
        (_, Some(t)) if attempt == 0 => format!("{t} build"),
        (_, Some(t)) => format!("{t} fix {attempt}"),
        _ => "step".to_owned(),
    };
    let mut tags = bot.tags.clone();
    if !tags.iter().any(|t| t == "loop") {
        tags.push("loop".into());
    }
    let request = CreateSession {
        agent: bot.agent,
        cwd: dir.display().to_string(),
        name: Some(step_name(&run.name, &entry.code(), &what)),
        credential: bot.credential.clone(),
        cols: 120,
        rows: 32,
        options: LaunchOptions { permission: None, ..bot.options.clone() },
        provider_id: bot.provider_id,
        description: Some(format!("Loop {}: {} · {}, {what}", run.name, entry.code(), entry.title)),
        project_id: None,
        tags,
        workspace: Default::default(),
        routing: None,
        bot_id: Some(bot.id),
        first_prompt: Some(prompt.to_owned()),
        loop_step: Some(step_launch(m, step_id, kind, &run.config.checks)?),
    };
    match m.create(request) {
        Ok(s) if s.live => {
            db::set_step_session(store, step_id, s.id, m.run_input(s.id).map(|(r, _)| r))?;
            notify(run.id);
            Ok(())
        }
        Ok(s) => {
            crate::automations::unstage(store, s.id);
            let why = s.note.unwrap_or_else(|| "The step's session didn't start.".into());
            db::set_step_session(store, step_id, s.id, None)?;
            db::end_step(store, step_id, StepState::Failed, Some(&why), None)?;
            let d = engine::advance(&phase.progress, Event::StepFailed { why }, &run.config.limits);
            apply(m, run, phase, entry, d)
        }
        Err(e) => {
            db::end_step(store, step_id, StepState::Failed, Some(&e), None)?;
            let d = engine::advance(&phase.progress, Event::StepFailed { why: e }, &run.config.limits);
            apply(m, run, phase, entry, d)
        }
    }
}

/// A step that couldn't even be set up (no Bot for its role).
fn fail_step(m: &Arc<SessionManager>, run: &RunRow, phase: &PhaseRow, entry: &PhaseEntry, step: Option<u32>, why: &str) -> Result<(), String> {
    if let Some(id) = step {
        db::end_step(m.store(), id, StepState::Failed, Some(why), None)?;
    }
    let d = engine::advance(&phase.progress, Event::StepFailed { why: why.to_owned() }, &run.config.limits);
    apply(m, run, phase, entry, d)
}

/// `chorus-p15 P01 T1 fix 2`, within the session name limit.
fn step_name(run: &str, code: &str, what: &str) -> String {
    let suffix = format!(" {code} {what}");
    let room = chorus_core::session::MAX_NAME_CHARS.saturating_sub(suffix.chars().count());
    format!("{}{suffix}", run.chars().take(room).collect::<String>().trim_end())
}

/// Claude Code's arguments for an unattended step (P15 spike S1, checked on 2.1.308 and 2.1.310):
/// only the named tools, user and project settings ignored, file tools kept to the worktree,
/// anything not allowed refused without asking, and no MCP servers but Chorus's own. The planner
/// may write only the run documents; a builder may edit the worktree and run the checks. Both may
/// call Chorus's tools.
fn claude_args(kind: StepKind, checks: &[gate::Check], mcp: &str) -> Vec<String> {
    let mut args: Vec<String> = vec!["--restricted".into(), "--tools".into()];
    let mut allowed: Vec<String> = Vec::new();
    if kind == StepKind::Plan {
        args.push("Read,Edit,Write,Glob,Grep".into());
        allowed.push(format!("Edit(./{RUN_DIR}/**)"));
    } else {
        args.push("Read,Edit,Write,Glob,Grep,Bash".into());
        allowed.push("Edit(./**)".into());
        for c in checks {
            allowed.push(format!("Bash({})", c.command.trim()));
            // Each part of a compound command must be allowed on its own.
            if c.command.contains("&&") {
                allowed.extend(c.command.split("&&").map(|p| format!("Bash({})", p.trim())).filter(|p| p != "Bash()"));
            }
        }
    }
    allowed.extend(core_tools::CLAUDE_TOOL_NAMES.iter().map(|t| (*t).to_owned()));
    args.extend(["--permission-mode".into(), "dontAsk".into(), "--allowedTools".into()]);
    args.extend(allowed);
    args.extend(["--strict-mcp-config".into(), "--mcp-config".into(), mcp.to_owned()]);
    args
}

/// A step's launch: its flags, and a fresh token for Chorus's tools in its environment.
fn step_launch(m: &SessionManager, step_id: u32, kind: StepKind, checks: &[gate::Check]) -> Result<LoopLaunch, String> {
    let mcp = tools::config_file(m, step_id)?;
    let token = tools::issue(step_id);
    Ok(LoopLaunch { args: claude_args(kind, checks, &mcp), secrets: vec![(tools::TOKEN_VAR.to_owned(), token)] })
}

enum Played {
    Done(Event),
    /// The script's step asked a blocking or timed question; it waits for the answer.
    Asked,
}

fn script_step(script_path: &str, phase: u32, kind: StepKind, task: Option<&str>, attempt: u32) -> Result<ScriptStep, String> {
    let text = std::fs::read_to_string(script_path).map_err(|e| format!("Couldn't read the script: {e}"))?;
    let name = if kind == StepKind::Plan { StepName::Plan } else { StepName::Build };
    script::parse(&text)?
        .find(phase, name, task, attempt)
        .cloned()
        .ok_or_else(|| format!("The script has no {name:?} step for phase {phase}, {task:?}, attempt {attempt}."))
}

fn write_files(dir: &Path, files: &std::collections::BTreeMap<String, String>, answer: Option<&str>) -> Result<(), String> {
    for (path, text) in files {
        let full = dir.join(path);
        if let Some(parent) = full.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let text = answer.map_or_else(|| text.clone(), |a| text.replace("{answer}", a));
        std::fs::write(&full, text).map_err(|e| format!("The script couldn't write {path}: {e}"))?;
    }
    Ok(())
}

/// Plays a scripted step: writes and deletes its files and asks its question, then reports what an
/// agent would have.
#[allow(clippy::too_many_arguments)]
fn play(m: &SessionManager, run: &RunRow, entry: &PhaseEntry, step_id: u32, script_path: &str, kind: StepKind, task: Option<&str>, attempt: u32) -> Played {
    let Ok(dir) = worktree(run) else { return Played::Done(Event::StepFailed { why: "The run's worktree is missing.".into() }) };
    let step = match script_step(script_path, entry.number, kind, task, attempt) {
        Ok(s) => s,
        Err(why) => return Played::Done(Event::StepFailed { why }),
    };
    if let Some(why) = step.fail {
        return Played::Done(Event::StepFailed { why });
    }
    if let Err(why) = write_files(dir, &step.write, None) {
        return Played::Done(Event::StepFailed { why });
    }
    for path in &step.delete {
        let _ = std::fs::remove_file(dir.join(path));
    }
    if let Some(args) = &step.ask {
        match core_tools::parse_ask(args) {
            Ok(a) if a.class == AskClass::LogOnly => {
                let tag = questions::tag(entry.number, task);
                let choice = a.default.clone().unwrap_or_default();
                let _ = tools::log_assumption(dir, &questions::assumption_line(&tag, &format!("{} Chose: {choice}", a.question)));
            }
            Ok(a) => {
                let deadline = (a.class == AskClass::Timed).then(|| now_ms() + i64::from(run.config.limits.timed_minutes) * 60_000);
                let asked = db::insert_question(
                    m.store(),
                    &NewQuestion {
                        run_id: run.id,
                        step_id,
                        phase: entry.number,
                        task,
                        class: a.class.as_str(),
                        question: &a.question,
                        options: &a.options,
                        default_answer: a.default.as_deref(),
                        deadline_at: deadline,
                    },
                );
                return match asked {
                    Ok(_) => Played::Asked,
                    Err(why) => Played::Done(Event::StepFailed { why }),
                };
            }
            Err(why) => return Played::Done(Event::StepFailed { why }),
        }
    }
    Played::Done(if kind == StepKind::Plan { Event::Planned(read_plan(dir, entry)) } else { Event::Built })
}

/// The planner's tasks.md, with what's missing or wrong.
fn read_plan(dir: &Path, entry: &PhaseEntry) -> Result<Vec<files::TaskEntry>, Vec<Problem>> {
    let folder = dir.join(prompts::phase_dir(entry));
    let mut problems = Vec::new();
    if !folder.join("spec.md").is_file() {
        problems.push(Problem { line: 0, message: format!("spec.md wasn't written at {}/spec.md.", prompts::phase_dir(entry)) });
    }
    let tasks = match std::fs::read_to_string(folder.join("tasks.md")) {
        Ok(text) => files::parse_tasks(&text),
        Err(_) => Err(vec![Problem { line: 0, message: format!("tasks.md wasn't written at {}/tasks.md.", prompts::phase_dir(entry)) }]),
    };
    match tasks {
        Ok(t) if problems.is_empty() => Ok(t),
        Ok(_) => Err(problems),
        Err(mut p) => {
            problems.append(&mut p);
            Err(problems)
        }
    }
}

/// Timed questions past their deadline take their default, logged in assumptions.md.
fn take_defaults(m: &SessionManager, run: &RunRow, step: &StepRow) -> Result<(), String> {
    let now = now_ms();
    for q in db::pending_for_step(m.store(), step.id)? {
        if q.state != "open" || q.class != "timed" || q.deadline_at.is_none_or(|d| now < d) {
            continue;
        }
        let default = q.default_answer.clone().unwrap_or_default();
        if db::answer_question(m.store(), q.id, "defaulted", &default, false)? {
            let line = format!("Nobody answered \"{}\" within {} minutes; took the default: {default}", q.question, run.config.limits.timed_minutes);
            tools::log_assumption(worktree(run)?, &questions::assumption_line(&questions::tag(q.phase, q.task.as_deref()), &line))?;
            notify(run.id);
        }
    }
    Ok(())
}

/// The prompt that gives a step's answered questions back to its agent.
fn answers_prompt(run: &RunRow, answered: &[QuestionRow]) -> String {
    answered
        .iter()
        .map(|q| {
            let by = if q.state == "defaulted" { AnsweredBy::Default } else { AnsweredBy::Matthew };
            questions::answer_prompt(&q.question, q.answer.as_deref().unwrap_or(""), by, q.made_rule, run.config.limits.timed_minutes)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Resumes the step's conversation with the answers as its first prompt: the reliable way to give a
/// run its first instruction (L64, D96), with the step's flags and a new token. `Some(why)` when it
/// couldn't.
fn deliver(m: &Arc<SessionManager>, run: &RunRow, step: &StepRow, answered: &[QuestionRow]) -> Result<Option<String>, String> {
    let store = m.store();
    let Some(sid) = step.session_id else { return Ok(Some("The step has no session to answer.".into())) };
    let prompt_id = crate::automations::stage(store, sid, "loop", &answers_prompt(run, answered))?;
    m.set_loop_launch(sid, step_launch(m, step.id, step.kind, &run.config.checks)?);
    let restarted = m.restart(sid);
    let passed = crate::automations::passed(store, prompt_id);
    match restarted {
        Ok(s) if s.live && passed => {
            db::set_step_session(store, step.id, sid, m.run_input(sid).map(|(r, _)| r))?;
            db::set_delivered(store, &answered.iter().map(|q| q.id).collect::<Vec<_>>())?;
            notify(run.id);
            Ok(None)
        }
        Ok(s) => {
            crate::automations::unstage(store, sid);
            Ok(Some(s.note.unwrap_or_else(|| "The step's agent didn't start again with the answer.".into())))
        }
        Err(e) => {
            crate::automations::unstage(store, sid);
            Ok(Some(format!("Couldn't resume the step with the answer: {e}")))
        }
    }
}

/// What happened to a running planner or builder step since the last pass, if anything. A step
/// that asked a question waits for it, with its agent closed; the answer resumes its conversation.
fn poll(m: &Arc<SessionManager>, run: &RunRow, _phase: &PhaseRow, entry: &PhaseEntry, step: &StepRow) -> Result<Option<Event>, String> {
    let store = m.store();
    let failed = |why: &str| -> Result<Option<Event>, String> {
        db::end_step(store, step.id, StepState::Failed, Some(why), None)?;
        db::cancel_questions(store, run.id, Some(step.id))?;
        Ok(Some(Event::StepFailed { why: why.to_owned() }))
    };
    take_defaults(m, run, step)?;
    let pending = db::pending_for_step(store, step.id)?;
    let open = pending.iter().any(|q| q.state == "open");
    let answered: Vec<QuestionRow> = pending.iter().filter(|q| q.state != "open").cloned().collect();

    // A scripted step that asked: no session, so its answer goes into the script's files.
    let Some(sid) = step.session_id else {
        let Some(path) = run.config.script.as_deref() else { return failed("The step has no session.") };
        if open || answered.is_empty() {
            return if open { Ok(None) } else { failed("The step has no session.") };
        }
        let dir = worktree(run)?;
        let script = match script_step(path, entry.number, step.kind, step.task.as_deref(), step.attempt) {
            Ok(st) => st,
            Err(e) => return failed(&e),
        };
        if let Err(e) = write_files(dir, &script.after_answer, answered.last().and_then(|q| q.answer.as_deref())) {
            return failed(&e);
        }
        db::set_delivered(store, &answered.iter().map(|q| q.id).collect::<Vec<_>>())?;
        db::end_step(store, step.id, StepState::Done, Some("Played from the run's script, with the answer."), Some("scripted"))?;
        return Ok(Some(if step.kind == StepKind::Plan { Event::Planned(read_plan(dir, entry)) } else { Event::Built }));
    };

    let Ok(s) = m.summary(sid) else { return failed("The step's session was deleted.") };
    let run_now = m.run_input(sid).map(|(r, _)| r);
    let live = s.live && run_now.is_some() && run_now == step.session_run_id;
    if !live {
        if open {
            return Ok(None);
        }
        if !answered.is_empty() {
            return match deliver(m, run, step, &answered)? {
                None => Ok(None),
                Some(why) => failed(&why),
            };
        }
        return failed(s.note.as_deref().unwrap_or("The agent exited before its turn ended."));
    }
    let run_id = run_now.unwrap_or(0);
    let starting = crate::store::delivery::unobserved(store, run_id).unwrap_or(true);
    if starting {
        return Ok(None);
    }
    match s.activity {
        Activity::Ready | Activity::NeedsAnswer if open || !answered.is_empty() => {
            // It asked and ended its turn: its agent closes until the answer comes, and someone
            // who typed into it keeps it. An answer already given goes back at once.
            let _ = m.finish(sid, run_id, WAITING_NOTE);
            notify(run.id);
            if !open {
                return match deliver(m, run, step, &answered)? {
                    None => Ok(None),
                    Some(why) => failed(&why),
                };
            }
            Ok(None)
        }
        Activity::Ready | Activity::NeedsAnswer => {
            let observed = s.observed.as_ref().map(|o| o.model.clone());
            // Someone who typed into the session took it over; it stays open for them.
            let _ = m.finish(sid, run_id, FINISHED_NOTE);
            db::end_step(store, step.id, StepState::Done, None, observed.as_deref())?;
            if step.kind != StepKind::Plan {
                return Ok(Some(Event::Built));
            }
            let dir = worktree(run)?;
            let plan = read_plan(dir, entry);
            if plan.is_ok() {
                git::commit_all(dir, &format!("{} · {}: plan", entry.code(), entry.title))?;
            }
            Ok(Some(Event::Planned(plan)))
        }
        Activity::Failed => {
            let why = format!("The agent's turn failed: {}", s.latest.as_deref().unwrap_or("no message"));
            let _ = m.finish(sid, run_id, FINISHED_NOTE);
            failed(&why)
        }
        _ => Ok(None),
    }
}

/// Runs every check in the worktree, writes the gate report, and commits a task that passed.
fn run_gate(m: &Arc<SessionManager>, run: &RunRow, phase: &PhaseRow, entry: &PhaseEntry, task: &str, attempt: u32) -> Result<Event, String> {
    let store = m.store();
    let dir = worktree(run)?;
    let step_id = db::insert_step(
        store,
        &NewStep { run_id: run.id, phase_id: phase.id, kind: StepKind::Check, task: Some(task), attempt, role: None, bot_id: None, model_asked: None },
    )?;
    notify(run.id);
    let parent: Vec<(String, String)> = std::env::vars().collect();
    let env = gate::environment(&parent);
    let (redact, _) = crate::vault::all_for_scrubbing(store);
    let mut ran = Vec::new();
    for check in &run.config.checks {
        let r = gate::run(dir, check, &env, &redact);
        db::insert_check(store, step_id, &r)?;
        ran.push(r);
        notify(run.id);
    }
    let report_path = dir.join(prompts::phase_dir(entry)).join("gate-report.md");
    let existing = std::fs::read_to_string(&report_path).unwrap_or_default();
    let section = report::attempt_section(&entry.code(), task, attempt, &ran);
    if let Some(parent) = report_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&report_path, report::prepend(&existing, &section)).map_err(|e| format!("writing the gate report: {e}"))?;
    let failed = ran.iter().filter(|c| !c.passed).count();
    let note = if failed == 0 { "Every check passed.".to_owned() } else { format!("{failed} of {} checks failed.", ran.len()) };
    db::end_step(store, step_id, StepState::Done, Some(&note), None)?;
    if failed == 0 {
        let tasks_path = dir.join(prompts::phase_dir(entry)).join("tasks.md");
        if let Ok(text) = std::fs::read_to_string(&tasks_path)
            && let Some(done) = files::set_task_status(&text, task, TaskStatus::Done)
        {
            let _ = std::fs::write(&tasks_path, done);
        }
        let title = phase.progress.tasks.iter().find(|t| t.id == task).map(|t| t.title.clone()).unwrap_or_default();
        git::commit_all(dir, &format!("{} {task}: {title}", entry.code()))?;
    }
    Ok(Event::Checked(ran.into_iter().map(|c| CheckOutcome { name: c.name, passed: c.passed, signature: c.signature }).collect()))
}

/// Commits what's left and tags the phase.
fn tag_phase(m: &Arc<SessionManager>, run: &RunRow, phase: &PhaseRow, entry: &PhaseEntry) -> Result<Event, String> {
    let store = m.store();
    let dir = worktree(run)?;
    let step_id = db::insert_step(
        store,
        &NewStep { run_id: run.id, phase_id: phase.id, kind: StepKind::Tag, task: None, attempt: 0, role: None, bot_id: None, model_asked: None },
    )?;
    write_state(m, run);
    let done = git::commit_all(dir, &format!("{} · {}: passed its checks", entry.code(), entry.title))
        .and_then(|_| git::tag(dir, &entry.tag(), &format!("{} · {} passed its checks", entry.code(), entry.title)));
    match done {
        Ok(()) => {
            db::end_step(store, step_id, StepState::Done, Some(&format!("Tagged {}.", entry.tag())), None)?;
            db::save_phase(store, phase.id, &phase.progress, None, Some(&entry.tag()))?;
            Ok(Event::Tagged)
        }
        Err(e) => {
            db::end_step(store, step_id, StepState::Failed, Some(&e), None)?;
            Ok(Event::StepFailed { why: e })
        }
    }
}

/// A phase passed: on to the next, or wait for Matthew at a milestone, or finish.
fn after_pass(m: &Arc<SessionManager>, run: &RunRow, phases: &[PhaseRow], plan: &[PhaseEntry]) -> Result<(), String> {
    let Some(last) = phases.last() else { return Ok(()) };
    let entry = plan.iter().find(|p| p.number == last.number);
    let next = plan.iter().find(|p| p.number > last.number).cloned();
    let tag = last.tag.clone().unwrap_or_else(|| format!("phase-{:02}", last.number));
    if last.number > run.reviewed_phase {
        let since = phases.iter().filter(|p| p.number > run.reviewed_phase && p.number < last.number && p.progress.stage == Stage::Passed).count() as u32;
        let high = entry.is_some_and(|e| e.risk == Risk::High);
        if engine::ends_milestone(last.number, high, since, run.config.milestone_every, false) {
            let then = if next.is_some() { "then Approve to start the next phase" } else { "it was the last phase in the roadmap" };
            let note = format!("P{:02} · {} passed and is tagged {tag}. Review the {} branch; {then}.", last.number, last.title, run.run_branch);
            db::set_state(m.store(), run.id, RunState::Waiting, Some(&note))?;
            notify(run.id);
            return Ok(());
        }
    }
    match next {
        Some(n) => begin_phase(m, run, &n),
        None => {
            db::set_state(m.store(), run.id, RunState::Done, Some(&format!("Every phase passed; the last is tagged {tag}. Review and merge {} yourself.", run.run_branch)))?;
            notify(run.id);
            Ok(())
        }
    }
}

fn condition_text(c: Condition) -> &'static str {
    match c {
        Condition::FixAttempts => "a check still failed after the last fix attempt",
        Condition::SameSignature => "fix attempts kept failing the same way",
        Condition::WallClock => "the phase ran past its time cap",
        Condition::PlanInvalid => "the plan's tasks.md stayed invalid",
        Condition::StepFailed => "a step's agent failed again",
        Condition::TaskStopped => "a task stopped and nothing else can run",
    }
}

/// The run stops, saying which condition stopped it, the last error, and where to look.
fn stop_run(m: &Arc<SessionManager>, run: &RunRow, phase: &PhaseRow, stop: Option<&Stop>) -> Result<(), String> {
    let last_good = db::phases(m.store(), run.id)?.iter().rev().find_map(|p| p.tag.clone()).unwrap_or_else(|| run.base_branch.clone());
    let why = stop.map_or_else(|| "it was stopped.".to_owned(), |s| {
        let detail = s.detail.trim_end();
        let end = if detail.ends_with(['.', '!', '?']) { "" } else { "." };
        format!("{}. {detail}{end}", condition_text(s.condition))
    });
    let note = format!(
        "STOPPED: P{:02} · {} stopped because {why} The worktree at {} holds the last attempt, uncommitted; the last good state is {last_good}.",
        phase.number,
        phase.title,
        run.worktree_path.as_deref().unwrap_or("?")
    );
    db::set_state(m.store(), run.id, RunState::Stopped, Some(&note))?;
    db::cancel_questions(m.store(), run.id, None)?;
    notify(run.id);
    Ok(())
}

/// Ends a running step's agent (Stop, or the time cap), leaving its session for review.
pub fn end_agent(m: &SessionManager, step: &StepRow, why: &str) {
    if let Some(sid) = step.session_id
        && let Err(e) = m.end_agent(sid, why)
    {
        eprintln!("chorus: loop step {}: ending its agent: {e}", step.id);
    }
    let _ = db::end_step(m.store(), step.id, StepState::Failed, Some(why), None);
}

/// The read-only `run-state.json` beside the run documents, for people and agents. It is an
/// export of the database, rewritten after every step, so git ignores it: a phase's tag never
/// leaves the worktree changed.
fn write_state(m: &SessionManager, run: &RunRow) {
    let Some(dir) = run.worktree_path.as_deref() else { return };
    let Ok(view) = super::view(m, run) else { return };
    let folder = Path::new(dir).join(RUN_DIR);
    let _ = std::fs::create_dir_all(&folder);
    let ignore = folder.join(".gitignore");
    let listed = std::fs::read_to_string(&ignore).unwrap_or_default();
    if !listed.lines().any(|l| l.trim() == "run-state.json") {
        let sep = if listed.is_empty() || listed.ends_with('\n') { "" } else { "\n" };
        let _ = std::fs::write(&ignore, format!("{listed}{sep}run-state.json\n"));
    }
    if let Ok(json) = serde_json::to_string_pretty(&view.state_file()) {
        let _ = std::fs::write(folder.join("run-state.json"), json + "\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_names_fit() {
        assert_eq!(step_name("shop", "P01", "T1 fix 2"), "shop P01 T1 fix 2");
        let long = "x".repeat(200);
        assert!(step_name(&long, "P01", "plan").chars().count() <= chorus_core::session::MAX_NAME_CHARS);
    }
}
