//! The loop's rows (migration 0023): runs, their phases, each step and each check a gate ran.

use chorus_core::run_loop::report::CheckRun;
use chorus_core::run_loop::roles::RoleSetting;
use chorus_core::run_loop::{Limits, PhaseRun, Stage, Stop};
use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use specta::Type;

use super::gate::Check;
use crate::store::{Store, now_ms};

/// A run's settings, which only Matthew edits, in Chorus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunConfig {
    pub roles: Vec<RoleSetting>,
    pub checks: Vec<Check>,
    pub limits: Limits,
    /// Phases between milestone reviews (the spec's `milestone_every`).
    pub milestone_every: u32,
    /// Debug builds only: steps are played from this script file instead of being run by agents.
    #[serde(default)]
    pub script: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    /// Made, not started.
    Draft,
    Running,
    /// Paused by Matthew: the step that is running finishes, and nothing new starts.
    Paused,
    /// A phase passed at a milestone, and the run waits for Matthew's review.
    Waiting,
    Stopped,
    /// Every phase in the roadmap passed.
    Done,
}

impl RunState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Waiting => "waiting",
            Self::Stopped => "stopped",
            Self::Done => "done",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "paused" => Self::Paused,
            "waiting" => Self::Waiting,
            "stopped" => Self::Stopped,
            "done" => Self::Done,
            _ => Self::Draft,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunRow {
    pub id: u32,
    pub name: String,
    pub repo_root: String,
    pub base_branch: String,
    pub run_branch: String,
    pub worktree_path: Option<String>,
    pub config: RunConfig,
    pub state: RunState,
    pub note: Option<String>,
    pub reviewed_phase: u32,
    pub created_at: i64,
    pub updated_at: i64,
}

fn run_from(r: &Row<'_>) -> rusqlite::Result<RunRow> {
    let config: String = r.get(6)?;
    let config = serde_json::from_str(&config)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e)))?;
    Ok(RunRow {
        id: r.get(0)?,
        name: r.get(1)?,
        repo_root: r.get(2)?,
        base_branch: r.get(3)?,
        run_branch: r.get(4)?,
        worktree_path: r.get(5)?,
        config,
        state: RunState::parse(&r.get::<_, String>(7)?),
        note: r.get(8)?,
        reviewed_phase: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
    })
}

const RUN_COLUMNS: &str = "id, name, repo_root, base_branch, run_branch, worktree_path, config, state, note, reviewed_phase, created_at, updated_at";

pub fn runs(store: &Store) -> Result<Vec<RunRow>, String> {
    store.with(|c| {
        let mut stmt = c.prepare(&format!("SELECT {RUN_COLUMNS} FROM loop_runs ORDER BY id DESC"))?;
        stmt.query_map([], run_from)?.collect()
    })
}

pub fn run(store: &Store, id: u32) -> Result<Option<RunRow>, String> {
    store.with(|c| c.query_row(&format!("SELECT {RUN_COLUMNS} FROM loop_runs WHERE id = ?1"), [id], run_from).optional())
}

pub fn insert_run(store: &Store, name: &str, repo_root: &str, base: &str, branch: &str, config: &RunConfig) -> Result<u32, String> {
    let config = serde_json::to_string(config).map_err(|e| e.to_string())?;
    let now = now_ms();
    store
        .with(|c| {
            c.execute(
                "INSERT INTO loop_runs (name, repo_root, base_branch, run_branch, config, state, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'draft', ?6, ?6)",
                params![name, repo_root, base, branch, config, now],
            )?;
            Ok(c.last_insert_rowid() as u32)
        })
        .map_err(|e| if e.contains("UNIQUE") { format!("A run named {name:?} already exists.") } else { e })
}

pub fn set_state(store: &Store, id: u32, state: RunState, note: Option<&str>) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE loop_runs SET state = ?2, note = ?3, updated_at = ?4 WHERE id = ?1", params![id, state.as_str(), note, now_ms()])).map(|_| ())
}

pub fn set_config(store: &Store, id: u32, config: &RunConfig) -> Result<(), String> {
    let config = serde_json::to_string(config).map_err(|e| e.to_string())?;
    store.with(|c| c.execute("UPDATE loop_runs SET config = ?2, updated_at = ?3 WHERE id = ?1", params![id, config, now_ms()])).map(|_| ())
}

pub fn set_worktree(store: &Store, id: u32, worktree_id: u32, path: &str) -> Result<(), String> {
    store
        .with(|c| c.execute("UPDATE loop_runs SET worktree_id = ?2, worktree_path = ?3, updated_at = ?4 WHERE id = ?1", params![id, worktree_id, path, now_ms()]))
        .map(|_| ())
}

pub fn set_reviewed(store: &Store, id: u32, phase: u32) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE loop_runs SET reviewed_phase = ?2, updated_at = ?3 WHERE id = ?1", params![id, phase, now_ms()])).map(|_| ())
}

pub fn delete_run(store: &Store, id: u32) -> Result<(), String> {
    store.with(|c| c.execute("DELETE FROM loop_runs WHERE id = ?1", [id])).map(|_| ())
}

#[derive(Debug, Clone)]
pub struct PhaseRow {
    pub id: u32,
    pub number: u32,
    pub title: String,
    pub progress: PhaseRun,
    pub stop: Option<Stop>,
    pub tag: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

fn json_col<T: serde::de::DeserializeOwned>(r: &Row<'_>, i: usize) -> rusqlite::Result<T> {
    let text: String = r.get(i)?;
    serde_json::from_str(&text).map_err(|e| rusqlite::Error::FromSqlConversionFailure(i, rusqlite::types::Type::Text, Box::new(e)))
}

fn phase_from(r: &Row<'_>) -> rusqlite::Result<PhaseRow> {
    let stop: Option<String> = r.get(4)?;
    Ok(PhaseRow {
        id: r.get(0)?,
        number: r.get(1)?,
        title: r.get(2)?,
        progress: json_col(r, 3)?,
        stop: stop.and_then(|s| serde_json::from_str(&s).ok()),
        tag: r.get(5)?,
        started_at: r.get(6)?,
        ended_at: r.get(7)?,
    })
}

const PHASE_COLUMNS: &str = "id, number, title, progress, stop, tag, started_at, ended_at";

pub fn phases(store: &Store, run_id: u32) -> Result<Vec<PhaseRow>, String> {
    store.with(|c| {
        let mut stmt = c.prepare(&format!("SELECT {PHASE_COLUMNS} FROM loop_phases WHERE run_id = ?1 ORDER BY number"))?;
        stmt.query_map([run_id], phase_from)?.collect()
    })
}

pub fn insert_phase(store: &Store, run_id: u32, number: u32, title: &str, folder: &str, progress: &PhaseRun) -> Result<u32, String> {
    let json = serde_json::to_string(progress).map_err(|e| e.to_string())?;
    store.with(|c| {
        c.execute(
            "INSERT INTO loop_phases (run_id, number, title, folder, progress, state, started_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![run_id, number, title, folder, json, stage_name(&progress.stage), progress.started_ms],
        )?;
        Ok(c.last_insert_rowid() as u32)
    })
}

pub fn stage_name(stage: &Stage) -> &'static str {
    match stage {
        Stage::Planning { .. } => "planning",
        Stage::Building { .. } => "building",
        Stage::Checking { .. } => "checking",
        Stage::Tagging => "tagging",
        Stage::Passed => "passed",
        Stage::Stopped => "stopped",
    }
}

/// Saves a phase's progress, with its stop or tag when it has one.
pub fn save_phase(store: &Store, id: u32, progress: &PhaseRun, stop: Option<&Stop>, tag: Option<&str>) -> Result<(), String> {
    let json = serde_json::to_string(progress).map_err(|e| e.to_string())?;
    let stop = stop.map(serde_json::to_string).transpose().map_err(|e| e.to_string())?;
    let ended = matches!(progress.stage, Stage::Passed | Stage::Stopped).then(now_ms);
    store
        .with(|c| {
            c.execute(
                "UPDATE loop_phases SET progress = ?2, state = ?3, stop = COALESCE(?4, stop), tag = COALESCE(?5, tag), ended_at = COALESCE(ended_at, ?6) WHERE id = ?1",
                params![id, json, stage_name(&progress.stage), stop, tag, ended],
            )
        })
        .map(|_| ())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum StepKind {
    Plan,
    Build,
    Check,
    Tag,
}

impl StepKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Build => "build",
            Self::Check => "check",
            Self::Tag => "tag",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "plan" => Self::Plan,
            "check" => Self::Check,
            "tag" => Self::Tag,
            _ => Self::Build,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum StepState {
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone)]
pub struct StepRow {
    pub id: u32,
    pub phase_id: u32,
    pub kind: StepKind,
    pub task: Option<String>,
    pub attempt: u32,
    pub role: Option<String>,
    pub bot_id: Option<u32>,
    pub session_id: Option<u32>,
    pub session_run_id: Option<u32>,
    pub model_asked: Option<String>,
    pub model_observed: Option<String>,
    pub state: StepState,
    pub note: Option<String>,
    pub summary: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

fn step_from(r: &Row<'_>) -> rusqlite::Result<StepRow> {
    Ok(StepRow {
        id: r.get(0)?,
        phase_id: r.get(1)?,
        kind: StepKind::parse(&r.get::<_, String>(2)?),
        task: r.get(3)?,
        attempt: r.get(4)?,
        role: r.get(5)?,
        bot_id: r.get(6)?,
        session_id: r.get(7)?,
        session_run_id: r.get(8)?,
        model_asked: r.get(9)?,
        model_observed: r.get(10)?,
        state: match r.get::<_, String>(11)?.as_str() {
            "running" => StepState::Running,
            "done" => StepState::Done,
            _ => StepState::Failed,
        },
        note: r.get(12)?,
        summary: r.get(13)?,
        started_at: r.get(14)?,
        ended_at: r.get(15)?,
    })
}

const STEP_COLUMNS: &str =
    "id, phase_id, kind, task, attempt, role, bot_id, session_id, session_run_id, model_asked, model_observed, state, note, summary, started_at, ended_at";

pub fn steps(store: &Store, run_id: u32) -> Result<Vec<StepRow>, String> {
    store.with(|c| {
        let mut stmt = c.prepare(&format!("SELECT {STEP_COLUMNS} FROM loop_steps WHERE run_id = ?1 ORDER BY id"))?;
        stmt.query_map([run_id], step_from)?.collect()
    })
}

/// The step of `phase` that is still running, if any.
pub fn running_step(store: &Store, phase_id: u32) -> Result<Option<StepRow>, String> {
    store.with(|c| {
        c.query_row(&format!("SELECT {STEP_COLUMNS} FROM loop_steps WHERE phase_id = ?1 AND state = 'running' ORDER BY id DESC LIMIT 1"), [phase_id], step_from)
            .optional()
    })
}

/// Every running step of every run, for the check at start.
pub fn all_running_steps(store: &Store) -> Result<Vec<StepRow>, String> {
    store.with(|c| {
        let mut stmt = c.prepare(&format!("SELECT {STEP_COLUMNS} FROM loop_steps WHERE state = 'running'"))?;
        stmt.query_map([], step_from)?.collect()
    })
}

pub struct NewStep<'a> {
    pub run_id: u32,
    pub phase_id: u32,
    pub kind: StepKind,
    pub task: Option<&'a str>,
    pub attempt: u32,
    pub role: Option<&'a str>,
    pub bot_id: Option<u32>,
    pub model_asked: Option<&'a str>,
}

pub fn insert_step(store: &Store, s: &NewStep<'_>) -> Result<u32, String> {
    store.with(|c| {
        c.execute(
            "INSERT INTO loop_steps (run_id, phase_id, kind, task, attempt, role, bot_id, model_asked, state, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'running', ?9)",
            params![s.run_id, s.phase_id, s.kind.as_str(), s.task, s.attempt, s.role, s.bot_id, s.model_asked, now_ms()],
        )?;
        Ok(c.last_insert_rowid() as u32)
    })
}

pub fn set_step_session(store: &Store, id: u32, session_id: u32, session_run_id: Option<u32>) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE loop_steps SET session_id = ?2, session_run_id = ?3 WHERE id = ?1", params![id, session_id, session_run_id])).map(|_| ())
}

pub fn end_step(store: &Store, id: u32, state: StepState, note: Option<&str>, model_observed: Option<&str>) -> Result<(), String> {
    let state = match state {
        StepState::Running => "running",
        StepState::Done => "done",
        StepState::Failed => "failed",
    };
    store
        .with(|c| {
            c.execute(
                "UPDATE loop_steps SET state = ?2, note = COALESCE(?3, note), model_observed = COALESCE(?4, model_observed), ended_at = ?5 WHERE id = ?1",
                params![id, state, note, model_observed, now_ms()],
            )
        })
        .map(|_| ())
}

pub fn step(store: &Store, id: u32) -> Result<Option<StepRow>, String> {
    store.with(|c| c.query_row(&format!("SELECT {STEP_COLUMNS} FROM loop_steps WHERE id = ?1"), [id], step_from).optional())
}

/// The run and phase number a step belongs to.
pub fn step_place(store: &Store, step_id: u32) -> Result<Option<(u32, u32)>, String> {
    store.with(|c| {
        c.query_row(
            "SELECT s.run_id, p.number FROM loop_steps s JOIN loop_phases p ON p.id = s.phase_id WHERE s.id = ?1",
            [step_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
    })
}

pub fn set_step_summary(store: &Store, id: u32, summary: &str) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE loop_steps SET summary = ?2 WHERE id = ?1", params![id, summary])).map(|_| ())
}

/// A question a step asked (slice 3).
#[derive(Debug, Clone)]
pub struct QuestionRow {
    pub id: u32,
    pub step_id: u32,
    pub phase: u32,
    pub task: Option<String>,
    /// blocking | timed
    pub class: String,
    pub question: String,
    pub options: Vec<String>,
    pub default_answer: Option<String>,
    pub asked_at: i64,
    pub deadline_at: Option<i64>,
    /// open | answered | defaulted | cancelled
    pub state: String,
    pub answer: Option<String>,
    pub made_rule: bool,
}

fn question_from(r: &Row<'_>) -> rusqlite::Result<QuestionRow> {
    Ok(QuestionRow {
        id: r.get(0)?,
        step_id: r.get(1)?,
        phase: r.get(2)?,
        task: r.get(3)?,
        class: r.get(4)?,
        question: r.get(5)?,
        options: json_col(r, 6)?,
        default_answer: r.get(7)?,
        asked_at: r.get(8)?,
        deadline_at: r.get(9)?,
        state: r.get(10)?,
        answer: r.get(11)?,
        made_rule: r.get(12)?,
    })
}

const QUESTION_COLUMNS: &str =
    "id, step_id, phase, task, class, question, options, default_answer, asked_at, deadline_at, state, answer, made_rule";

pub struct NewQuestion<'a> {
    pub run_id: u32,
    pub step_id: u32,
    pub phase: u32,
    pub task: Option<&'a str>,
    pub class: &'a str,
    pub question: &'a str,
    pub options: &'a [String],
    pub default_answer: Option<&'a str>,
    pub deadline_at: Option<i64>,
}

pub fn insert_question(store: &Store, q: &NewQuestion<'_>) -> Result<u32, String> {
    let options = serde_json::to_string(q.options).map_err(|e| e.to_string())?;
    store.with(|c| {
        c.execute(
            "INSERT INTO loop_questions (run_id, step_id, phase, task, class, question, options, default_answer, asked_at, deadline_at, state)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'open')",
            params![q.run_id, q.step_id, q.phase, q.task, q.class, q.question, options, q.default_answer, now_ms(), q.deadline_at],
        )?;
        Ok(c.last_insert_rowid() as u32)
    })
}

/// A run's questions, newest first.
pub fn questions(store: &Store, run_id: u32) -> Result<Vec<QuestionRow>, String> {
    store.with(|c| {
        let mut stmt = c.prepare(&format!("SELECT {QUESTION_COLUMNS} FROM loop_questions WHERE run_id = ?1 ORDER BY id DESC"))?;
        stmt.query_map([run_id], question_from)?.collect()
    })
}

pub fn question(store: &Store, id: u32) -> Result<Option<(u32, QuestionRow)>, String> {
    store.with(|c| {
        c.query_row(&format!("SELECT run_id, {QUESTION_COLUMNS} FROM loop_questions WHERE id = ?1"), [id], |r| {
            let run: u32 = r.get(0)?;
            let q = QuestionRow {
                id: r.get(1)?,
                step_id: r.get(2)?,
                phase: r.get(3)?,
                task: r.get(4)?,
                class: r.get(5)?,
                question: r.get(6)?,
                options: json_col(r, 7)?,
                default_answer: r.get(8)?,
                asked_at: r.get(9)?,
                deadline_at: r.get(10)?,
                state: r.get(11)?,
                answer: r.get(12)?,
                made_rule: r.get(13)?,
            };
            Ok((run, q))
        })
        .optional()
    })
}

/// A step's questions that are open, or answered but not yet given back to its agent.
pub fn pending_for_step(store: &Store, step_id: u32) -> Result<Vec<QuestionRow>, String> {
    store.with(|c| {
        let mut stmt = c.prepare(&format!(
            "SELECT {QUESTION_COLUMNS} FROM loop_questions WHERE step_id = ?1 AND delivered_at IS NULL AND state != 'cancelled' ORDER BY id"
        ))?;
        stmt.query_map([step_id], question_from)?.collect()
    })
}

/// Records an answer, Matthew's (`answered`) or the default (`defaulted`), if the question is open.
pub fn answer_question(store: &Store, id: u32, state: &str, answer: &str, made_rule: bool) -> Result<bool, String> {
    store
        .with(|c| {
            c.execute(
                "UPDATE loop_questions SET state = ?2, answer = ?3, made_rule = ?4, answered_at = ?5 WHERE id = ?1 AND state = 'open'",
                params![id, state, answer, made_rule, now_ms()],
            )
        })
        .map(|n| n == 1)
}

pub fn set_delivered(store: &Store, ids: &[u32]) -> Result<(), String> {
    let now = now_ms();
    store.with(|c| {
        for id in ids {
            c.execute("UPDATE loop_questions SET delivered_at = ?2 WHERE id = ?1", params![id, now])?;
        }
        Ok(())
    })
}

/// Cancels a step's open questions (the step failed or the run stopped), or a whole run's.
pub fn cancel_questions(store: &Store, run_id: u32, step_id: Option<u32>) -> Result<(), String> {
    store
        .with(|c| {
            c.execute(
                "UPDATE loop_questions SET state = 'cancelled' WHERE run_id = ?1 AND state = 'open' AND (?2 IS NULL OR step_id = ?2)",
                params![run_id, step_id],
            )
        })
        .map(|_| ())
}

pub fn insert_check(store: &Store, step_id: u32, c: &CheckRun) -> Result<(), String> {
    let signature = c.signature.as_ref().map(serde_json::to_string).transpose().map_err(|e| e.to_string())?;
    store
        .with(|db| {
            db.execute(
                "INSERT INTO loop_checks (step_id, name, command, passed, exit_code, timed_out, duration_ms, output_tail, signature)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![step_id, c.name, c.command, c.passed, c.exit_code, c.timed_out, c.duration_ms, c.output_tail, signature],
            )
        })
        .map(|_| ())
}

pub fn checks(store: &Store, step_id: u32) -> Result<Vec<CheckRun>, String> {
    store.with(|c| {
        let mut stmt = c.prepare(
            "SELECT name, command, passed, exit_code, timed_out, duration_ms, output_tail, signature FROM loop_checks WHERE step_id = ?1 ORDER BY id",
        )?;
        stmt.query_map([step_id], |r| {
            let signature: Option<String> = r.get(7)?;
            Ok(CheckRun {
                name: r.get(0)?,
                command: r.get(1)?,
                passed: r.get(2)?,
                exit_code: r.get(3)?,
                timed_out: r.get(4)?,
                duration_ms: u32::try_from(r.get::<_, i64>(5)?).unwrap_or(0),
                output_tail: r.get(6)?,
                signature: signature.and_then(|s| serde_json::from_str(&s).ok()),
            })
        })?
        .collect()
    })
}
