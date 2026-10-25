//! The autonomous phase loop (P15, D97, D98). A run takes a repository's roadmap
//! (`.chorus/run/roadmap.md`) and works it phase after phase on its own branch, in its own worktree:
//! a planner writes each phase's spec and tasks, builders build and fix them, the gatekeeper runs
//! Matthew's checks, and a passing phase is tagged. It stops at milestones for review, and on the
//! spec's stop conditions with the reason. The decisions are `chorus_core::run_loop`'s; this module
//! does the IO: sessions, git, the checks and the database.
//!
//! Slice 2 builds one phase at a time, task after task; later slices add questions, Telegram,
//! the plan grill, parallel builders, review, Counsel and allowances.

pub mod db;
pub mod gate;
pub mod tools;
#[cfg(test)]
mod tests;
pub mod worker;

use std::path::Path;
use std::sync::Arc;

use chorus_core::agent::AgentKind;
use chorus_core::run_loop::files::{self, PhaseEntry};
use chorus_core::run_loop::report::CheckRun;
use chorus_core::run_loop::roles::LoopRole;
use chorus_core::run_loop::script;
use chorus_core::run_loop::{Stop, TaskProgress};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use db::{RunConfig, RunRow, RunState, StepKind, StepState};

use crate::git;
use crate::sessions::SessionManager;
use crate::vault::commands::secs;

type Manager<'a> = State<'a, Arc<SessionManager>>;

/// A run changed: the Loop screen reads it again.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct LoopUpdated {
    pub run_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StepView {
    pub id: u32,
    pub kind: StepKind,
    pub task: Option<String>,
    pub attempt: u32,
    pub role: Option<String>,
    pub bot_id: Option<u32>,
    pub bot_name: Option<String>,
    pub session_id: Option<u32>,
    /// The model the step asked for, and the one its transcript says answered (L61).
    pub model_asked: Option<String>,
    pub model_observed: Option<String>,
    pub state: StepState,
    pub note: Option<String>,
    /// What the agent said it did, with Chorus's task_done tool.
    pub summary: Option<String>,
    pub started_at_secs: u32,
    pub ended_at_secs: Option<u32>,
    /// A gate step's checks.
    pub checks: Vec<CheckRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PhaseView {
    pub number: u32,
    pub title: String,
    /// planning | building | checking | tagging | passed | stopped
    pub state: String,
    pub stop: Option<Stop>,
    pub tag: Option<String>,
    pub started_at_secs: u32,
    pub ended_at_secs: Option<u32>,
    pub tasks: Vec<TaskProgress>,
    pub steps: Vec<StepView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub id: u32,
    pub name: String,
    pub repo_root: String,
    pub base_branch: String,
    pub run_branch: String,
    pub worktree_path: Option<String>,
    pub config: RunConfig,
    pub state: RunState,
    pub note: Option<String>,
    pub phases: Vec<PhaseView>,
    /// Every question its steps asked, newest first.
    pub questions: Vec<QuestionView>,
    /// Open questions: the run waits on Matthew for these.
    pub open_questions: u32,
    pub created_at_secs: u32,
    pub updated_at_secs: u32,
}

/// A question a step asked, as the Loop screen shows it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct QuestionView {
    pub id: u32,
    pub step_id: u32,
    pub phase: u32,
    pub task: Option<String>,
    /// blocking | timed
    pub class: String,
    pub question: String,
    pub options: Vec<String>,
    pub default_answer: Option<String>,
    /// open | answered | defaulted | cancelled
    pub state: String,
    pub answer: Option<String>,
    pub made_rule: bool,
    pub asked_at_secs: u32,
    /// A timed question's: when its default is taken.
    pub deadline_at_secs: Option<u32>,
}

/// `run-state.json`: what the run is doing and which model ran each step, for people and agents.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateFile<'a> {
    pub note: &'static str,
    pub run: &'a str,
    pub branch: &'a str,
    pub state: RunState,
    pub phases: Vec<StatePhase<'a>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatePhase<'a> {
    pub phase: String,
    pub title: &'a str,
    pub state: &'a str,
    pub tag: Option<&'a str>,
    pub tasks: Vec<(&'a str, &'a str)>,
    pub steps: Vec<StateStep<'a>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateStep<'a> {
    pub kind: StepKind,
    pub task: Option<&'a str>,
    pub attempt: u32,
    pub role: Option<&'a str>,
    pub model_asked: Option<&'a str>,
    pub model_observed: Option<&'a str>,
    pub state: StepState,
}

impl RunView {
    pub fn state_file(&self) -> StateFile<'_> {
        StateFile {
            note: "Written by Chorus after every step. Read it; don't edit it.",
            run: &self.name,
            branch: &self.run_branch,
            state: self.state,
            phases: self
                .phases
                .iter()
                .map(|p| StatePhase {
                    phase: format!("P{:02}", p.number),
                    title: &p.title,
                    state: &p.state,
                    tag: p.tag.as_deref(),
                    tasks: p.tasks.iter().map(|t| (t.id.as_str(), t.status.as_str())).collect(),
                    steps: p
                        .steps
                        .iter()
                        .map(|s| StateStep {
                            kind: s.kind,
                            task: s.task.as_deref(),
                            attempt: s.attempt,
                            role: s.role.as_deref(),
                            model_asked: s.model_asked.as_deref(),
                            model_observed: s.model_observed.as_deref(),
                            state: s.state,
                        })
                        .collect(),
                })
                .collect(),
        }
    }
}

pub fn view(m: &SessionManager, run: &RunRow) -> Result<RunView, String> {
    let store = m.store();
    let bots = crate::bots::list(store).unwrap_or_default();
    let steps = db::steps(store, run.id)?;
    let phases = db::phases(store, run.id)?
        .into_iter()
        .map(|p| {
            let steps = steps
                .iter()
                .filter(|s| s.phase_id == p.id)
                .map(|s| StepView {
                    id: s.id,
                    kind: s.kind,
                    task: s.task.clone(),
                    attempt: s.attempt,
                    role: s.role.clone(),
                    bot_id: s.bot_id,
                    bot_name: s.bot_id.and_then(|id| bots.iter().find(|b| b.id == id)).map(|b| b.name.clone()),
                    session_id: s.session_id,
                    model_asked: s.model_asked.clone(),
                    model_observed: s.model_observed.clone(),
                    state: s.state,
                    note: s.note.clone(),
                    summary: s.summary.clone(),
                    started_at_secs: secs(s.started_at),
                    ended_at_secs: s.ended_at.map(secs),
                    checks: if s.kind == StepKind::Check { db::checks(store, s.id).unwrap_or_default() } else { Vec::new() },
                })
                .collect();
            PhaseView {
                number: p.number,
                title: p.title.clone(),
                state: db::stage_name(&p.progress.stage).to_owned(),
                stop: p.stop.clone(),
                tag: p.tag.clone(),
                started_at_secs: secs(p.started_at),
                ended_at_secs: p.ended_at.map(secs),
                tasks: p.progress.tasks.clone(),
                steps,
            }
        })
        .collect();
    let questions: Vec<QuestionView> = db::questions(store, run.id)?
        .into_iter()
        .map(|q| QuestionView {
            id: q.id,
            step_id: q.step_id,
            phase: q.phase,
            task: q.task,
            class: q.class,
            question: q.question,
            options: q.options,
            default_answer: q.default_answer,
            state: q.state,
            answer: q.answer,
            made_rule: q.made_rule,
            asked_at_secs: secs(q.asked_at),
            deadline_at_secs: q.deadline_at.map(secs),
        })
        .collect();
    let open_questions = questions.iter().filter(|q| q.state == "open").count() as u32;
    Ok(RunView {
        id: run.id,
        name: run.name.clone(),
        repo_root: run.repo_root.clone(),
        base_branch: run.base_branch.clone(),
        run_branch: run.run_branch.clone(),
        worktree_path: run.worktree_path.clone(),
        config: run.config.clone(),
        state: run.state,
        note: run.note.clone(),
        phases,
        questions,
        open_questions,
        created_at_secs: secs(run.created_at),
        updated_at_secs: secs(run.updated_at),
    })
}

/// Before sessions relaunch at start: steps Chorus was running when it stopped are ended.
pub fn prepare(m: &SessionManager, notify: Box<dyn Fn(u32) + Send + Sync>) {
    worker::set_notify(notify);
    worker::prepare(m);
}

/// After the relaunch has begun: running runs carry on.
pub fn start(m: &Arc<SessionManager>) {
    worker::resume_all(m);
}

const MAX_CHECKS: usize = 20;

/// Checks a run's settings: the roles it needs, its checks, limits and, in debug builds, a script.
fn check_config(m: &SessionManager, config: &RunConfig) -> Result<RunConfig, String> {
    let store = m.store();
    let mut config = config.clone();
    config.limits = config.limits.check()?;
    if !(1..=10).contains(&config.milestone_every) {
        return Err("A milestone comes every 1 to 10 phases.".into());
    }
    if let Some(path) = config.script.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        if !cfg!(debug_assertions) {
            return Err("Scripted steps are for testing, in development builds only.".into());
        }
        let text = std::fs::read_to_string(path).map_err(|e| format!("Couldn't read the script {path}: {e}"))?;
        script::parse(&text)?;
        config.script = Some(path.to_owned());
    } else {
        config.script = None;
    }
    for role in [LoopRole::Planner, LoopRole::Builder] {
        let setting = config.roles.iter().find(|r| r.role == role).ok_or_else(|| format!("Choose a Bot for the {} role.", role.label()))?;
        for id in std::iter::once(setting.bot).chain(setting.fallback) {
            let bot = crate::bots::get(store, id)?.ok_or_else(|| format!("The {} role's Bot no longer exists.", role.label()))?;
            if bot.archived {
                return Err(format!("{} is archived.", bot.name));
            }
            if let (Some(pid), Some(model)) = (bot.provider_id, bot.options.model.as_deref())
                && crate::store::providers::get(store, pid)?.is_some_and(|p| p.kind == chorus_core::providers::ProviderKind::Openrouter)
                && chorus_core::run_loop::roles::gpt_or_claude(model)
            {
                return Err(format!(
                    "{} runs {model} through OpenRouter. GPT and Claude models run only on your subscriptions, through Codex and Claude Code (D99): choose another model for it, or another Bot.",
                    bot.name
                ));
            }
            if config.script.is_none() && bot.agent != AgentKind::Claude {
                return Err(format!(
                    "{} runs {}: loop steps run Claude Code Bots for now, because passing a first prompt to other agents at launch isn't verified yet.",
                    bot.name,
                    bot.agent.display_name()
                ));
            }
        }
    }
    if config.checks.is_empty() {
        return Err("Add at least one check: the gatekeeper passes a task only when every check passes.".into());
    }
    if config.checks.len() > MAX_CHECKS {
        return Err(format!("A run has up to {MAX_CHECKS} checks."));
    }
    let mut names: Vec<String> = Vec::new();
    for c in &mut config.checks {
        c.name = c.name.trim().to_owned();
        c.command = c.command.trim().to_owned();
        if c.name.is_empty() || c.name.chars().count() > 40 {
            return Err("Give each check a name of up to 40 characters.".into());
        }
        if names.contains(&c.name.to_lowercase()) {
            return Err(format!("Two checks are called {:?}.", c.name));
        }
        names.push(c.name.to_lowercase());
        if c.command.is_empty() || c.command.len() > 500 || c.command.chars().any(char::is_control) {
            return Err(format!("{}: write the command on one line, up to 500 characters.", c.name));
        }
        if !(10..=7200).contains(&c.timeout_secs) {
            return Err(format!("{}: its time limit is 10 seconds to 2 hours.", c.name));
        }
        crate::bots::refuse_secrets(store, "a check's command", &c.command)?;
    }
    Ok(config)
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewRun {
    pub name: String,
    /// A folder in the repository.
    pub folder: String,
    /// The branch the run starts from; left out, the folder's checked-out branch.
    pub base_branch: Option<String>,
    pub config: RunConfig,
}

/// What a folder offers a new run: its repository, branch and roadmap.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LoopFolder {
    pub repo_root: Option<String>,
    pub branch: Option<String>,
    pub phases: Vec<PhaseEntry>,
    /// Why a run can't start here, if it can't.
    pub problem: Option<String>,
}

fn folder_check(folder: &str, base: Option<&str>) -> LoopFolder {
    let mut out = LoopFolder { repo_root: None, branch: None, phases: Vec::new(), problem: None };
    let facts = match git::facts(Path::new(folder.trim())) {
        Ok(Some(f)) => f,
        Ok(None) => {
            out.problem = Some(format!("{folder} isn't in a git repository."));
            return out;
        }
        Err(e) => {
            out.problem = Some(e);
            return out;
        }
    };
    out.repo_root = Some(facts.main_root.clone());
    let branch = base.map(str::to_owned).or(facts.branch);
    out.branch = branch.clone();
    let Some(branch) = branch else {
        out.problem = Some("The repository is on a detached HEAD: choose the branch to start from.".into());
        return out;
    };
    let root = Path::new(&facts.main_root);
    match git::show(root, &branch, &format!("{}/roadmap.md", chorus_core::run_loop::prompts::RUN_DIR)) {
        Ok(Some(text)) => match files::parse_roadmap(&text) {
            Ok(p) => out.phases = p,
            Err(p) => out.problem = Some(files::describe("roadmap.md", &p)),
        },
        Ok(None) => out.problem = Some(format!("{branch} has no .chorus/run/roadmap.md yet: commit the roadmap there first.")),
        Err(e) => out.problem = Some(e),
    }
    out
}

#[tauri::command]
#[specta::specta]
pub fn loop_folder_check(folder: String, base_branch: Option<String>) -> LoopFolder {
    folder_check(&folder, base_branch.as_deref().map(str::trim).filter(|b| !b.is_empty()))
}

#[tauri::command]
#[specta::specta]
pub fn loop_runs_list(state: Manager<'_>) -> Result<Vec<RunView>, String> {
    db::runs(state.store())?.iter().map(|r| view(&state, r)).collect()
}

fn get(m: &SessionManager, id: u32) -> Result<RunRow, String> {
    db::run(m.store(), id)?.ok_or_else(|| "That run no longer exists.".into())
}

#[tauri::command]
#[specta::specta]
pub fn loop_run_create(state: Manager<'_>, input: NewRun) -> Result<RunView, String> {
    create(&state, &input)
}

fn create(m: &SessionManager, input: &NewRun) -> Result<RunView, String> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err("Give the run a name of up to 40 characters.".into());
    }
    let config = check_config(m, &input.config)?;
    let check = folder_check(&input.folder, input.base_branch.as_deref().map(str::trim).filter(|b| !b.is_empty()));
    if let Some(p) = check.problem {
        return Err(p);
    }
    let (Some(root), Some(base)) = (check.repo_root, check.branch) else { return Err("That folder isn't in a repository.".into()) };
    let branch = format!("run/{}", files::slug(name));
    let id = db::insert_run(m.store(), name, &root, &base, &branch, &config)?;
    view(m, &get(m, id)?)
}

#[tauri::command]
#[specta::specta]
pub fn loop_run_update(state: Manager<'_>, id: u32, config: RunConfig) -> Result<RunView, String> {
    let run = get(&state, id)?;
    if run.state == RunState::Running {
        return Err("Pause the run first: its settings change at a step boundary.".into());
    }
    let config = check_config(&state, &config)?;
    db::set_config(state.store(), id, &config)?;
    worker::notify(id);
    view(&state, &get(&state, id)?)
}

#[tauri::command]
#[specta::specta]
pub async fn loop_run_start(state: Manager<'_>, id: u32) -> Result<RunView, String> {
    let m = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || start_run(&m, id)).await.map_err(|e| e.to_string())?
}

fn start_run(m: &Arc<SessionManager>, id: u32) -> Result<RunView, String> {
    if ready_to_run(m, id)? {
        worker::ensure_worker(m, id);
        worker::notify(id);
    }
    view(m, &get(m, id)?)
}

/// Makes the run's worktree if it has none, checks its roadmap and marks it running. False when
/// it was running already.
fn ready_to_run(m: &SessionManager, id: u32) -> Result<bool, String> {
    let run = get(m, id)?;
    match run.state {
        RunState::Draft | RunState::Paused => {}
        RunState::Running => return Ok(false),
        RunState::Waiting => return Err("The run is waiting for your review: Approve it to go on.".into()),
        RunState::Stopped | RunState::Done => return Err("This run has ended. Make a new run to go again.".into()),
    }
    check_config(m, &run.config)?;
    // A step in a repository Claude Code doesn't trust would stop at its trust question, with
    // nobody there to answer. Chorus never trusts one for Matthew (D55); it says so instead.
    let env: Vec<(String, String)> = std::env::vars().collect();
    if run.config.script.is_none() && crate::agents::claude_repo_trusted(&env, &run.repo_root) == Some(false) {
        return Err(format!(
            "Claude Code doesn't trust {} yet, so the run's steps would stop at its trust question. Start a Claude Code session in that folder once, choose Yes, then start the run.",
            run.repo_root
        ));
    }
    if run.worktree_path.as_deref().is_none_or(|p| !Path::new(p).is_dir()) {
        let (wt_id, path) = m.add_run_worktree(&run.repo_root, &files::slug(&run.name), &run.run_branch, &run.base_branch)?;
        db::set_worktree(m.store(), id, wt_id, &path)?;
    }
    let run = get(m, id)?;
    worker::roadmap(Path::new(run.worktree_path.as_deref().unwrap_or_default()))?;
    db::set_state(m.store(), id, RunState::Running, None)?;
    Ok(true)
}

#[tauri::command]
#[specta::specta]
pub fn loop_run_pause(state: Manager<'_>, id: u32) -> Result<RunView, String> {
    let run = get(&state, id)?;
    if run.state == RunState::Running {
        db::set_state(state.store(), id, RunState::Paused, Some("Paused: the step that is running finishes, and nothing new starts."))?;
        worker::notify(id);
    }
    view(&state, &get(&state, id)?)
}

#[tauri::command]
#[specta::specta]
pub fn loop_run_stop(state: Manager<'_>, id: u32) -> Result<RunView, String> {
    stop(&state, id)?;
    view(&state, &get(&state, id)?)
}

/// Stops a run at once: its step's agent ends, its phase and open questions close.
fn stop(m: &SessionManager, id: u32) -> Result<(), String> {
    let run = get(m, id)?;
    if matches!(run.state, RunState::Stopped | RunState::Done) {
        return Ok(());
    }
    if let Some(phase) = db::phases(m.store(), id)?.pop() {
        if let Some(step) = db::running_step(m.store(), phase.id)? {
            worker::end_agent(m, &step, "The run was stopped.");
        }
        if !matches!(phase.progress.stage, chorus_core::run_loop::Stage::Passed) {
            let mut progress = phase.progress.clone();
            progress.stage = chorus_core::run_loop::Stage::Stopped;
            db::save_phase(m.store(), phase.id, &progress, None, None)?;
        }
    }
    db::set_state(m.store(), id, RunState::Stopped, Some("Stopped by you. The worktree keeps what the last step left."))?;
    db::cancel_questions(m.store(), id, None)?;
    worker::notify(id);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn loop_run_approve(state: Manager<'_>, id: u32) -> Result<RunView, String> {
    approve(&state, id)?;
    worker::ensure_worker(&state, id);
    worker::notify(id);
    view(&state, &get(&state, id)?)
}

/// Marks the last phase reviewed and lets the run go on.
fn approve(m: &SessionManager, id: u32) -> Result<(), String> {
    let run = get(m, id)?;
    if run.state != RunState::Waiting {
        return Err("Only a run waiting for your review can be approved.".into());
    }
    let last = db::phases(m.store(), id)?.pop().map_or(0, |p| p.number);
    db::set_reviewed(m.store(), id, last)?;
    db::set_state(m.store(), id, RunState::Running, None)
}

#[tauri::command]
#[specta::specta]
pub fn loop_question_answer(state: Manager<'_>, question_id: u32, answer: String, make_rule: bool) -> Result<RunView, String> {
    let run_id = answer_question(&state, question_id, &answer, make_rule)?;
    worker::notify(run_id);
    view(&state, &get(&state, run_id)?)
}

/// Records Matthew's answer to an open question; the run's worker gives it back to the step. "Make
/// it a rule" adds it to the run's decisions.md too, so no step asks it again.
fn answer_question(m: &SessionManager, id: u32, answer: &str, make_rule: bool) -> Result<u32, String> {
    let store = m.store();
    let (run_id, q) = db::question(store, id)?.ok_or("That question no longer exists.")?;
    if q.state != "open" {
        return Err("That question was already answered or closed.".into());
    }
    let answer = answer.trim();
    if answer.is_empty() || answer.chars().count() > 2000 {
        return Err("Write an answer of up to 2000 characters.".into());
    }
    crate::bots::refuse_secrets(store, "an answer", answer)?;
    if !db::answer_question(store, id, "answered", answer, make_rule)? {
        return Err("That question was already answered or closed.".into());
    }
    if make_rule {
        let run = get(m, run_id)?;
        let dir = run.worktree_path.ok_or("The run has no worktree for decisions.md.")?;
        let tag = chorus_core::run_loop::questions::tag(q.phase, q.task.as_deref());
        tools::add_rule(Path::new(&dir), &q.question, answer, &tag)?;
    }
    Ok(run_id)
}

#[tauri::command]
#[specta::specta]
pub fn loop_run_delete(state: Manager<'_>, id: u32) -> Result<bool, String> {
    let run = get(&state, id)?;
    if matches!(run.state, RunState::Running | RunState::Paused) {
        return Err("Stop the run before deleting it.".into());
    }
    db::delete_run(state.store(), id)?;
    worker::notify(id);
    Ok(true)
}
