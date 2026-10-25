//! The autonomous phase loop (P15, D97): a phase's progress as a pure state machine. The
//! orchestrator in `src-tauri/src/run_loop/` starts steps, reads what they produced and feeds the
//! result back here as an [`Event`]; [`advance`] says what happens next. Nothing here knows about
//! sessions, git or the clock: time comes in as `now_ms`.
//!
//! A phase is planned, then each task is built and checked, with fix attempts while checks fail,
//! and the phase is tagged once every task passes. The stop conditions are the spec's: the same
//! check still failing after the last fix attempt stops the phase, the same error signature on
//! three fix attempts in a row stops the task, and so does a phase running past its wall-clock cap.

pub mod files;
pub mod prompts;
pub mod report;
pub mod roles;
pub mod script;
pub mod signature;

use serde::{Deserialize, Serialize};
use specta::Type;

use files::{Problem, TaskEntry, TaskStatus};
use signature::Signature;

/// The limits a run is configured with (the spec's `limits:` block).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    /// Fix attempts after a task's first build; the last follows Counsel's diagnosis.
    pub fix_attempts: u32,
    /// Fix attempts in a row ending with the same error signature that stop the task.
    pub same_signature: u32,
    /// Times an invalid `tasks.md` goes back to the planner before the phase stops.
    pub plan_retries: u32,
    /// Times a step whose agent failed is run again before the phase stops.
    pub step_retries: u32,
    /// Minutes a phase may run; `None` for no cap.
    pub phase_minutes: Option<u32>,
}

impl Default for Limits {
    fn default() -> Self {
        Self { fix_attempts: 3, same_signature: 3, plan_retries: 1, step_retries: 1, phase_minutes: Some(240) }
    }
}

impl Limits {
    pub fn check(&self) -> Result<Self, String> {
        if !(1..=10).contains(&self.fix_attempts) {
            return Err("Fix attempts are 1 to 10.".into());
        }
        if !(2..=10).contains(&self.same_signature) {
            return Err("The same error signature stops a task after 2 to 10 fix attempts.".into());
        }
        if self.plan_retries > 3 || self.step_retries > 3 {
            return Err("Retries are 0 to 3.".into());
        }
        if self.phase_minutes.is_some_and(|m| !(5..=48 * 60).contains(&m)) {
            return Err("A phase's time cap is 5 minutes to 48 hours.".into());
        }
        Ok(*self)
    }
}

/// Where a phase is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Stage {
    /// The planner is writing `spec.md` and `tasks.md`. `problems` is what was wrong with the last
    /// `tasks.md`, sent back with a retry.
    Planning { attempt: u32, problems: Option<String> },
    /// A builder is working on a task: attempt 0 builds it, later attempts fix failed checks.
    Building { task: String, attempt: u32 },
    /// The gatekeeper is running the checks after that attempt.
    Checking { task: String, attempt: u32 },
    /// Every task passed; the phase is being committed and tagged.
    Tagging,
    Passed,
    Stopped,
}

/// One failing check in one gate run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub check: String,
    pub key: String,
    pub summary: String,
}

/// The checks after one attempt at a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GateRun {
    pub attempt: u32,
    /// Empty when every check passed.
    pub failed: Vec<Failure>,
}

impl GateRun {
    /// The run's error signature: every failing check's, in check order.
    fn key(&self) -> String {
        self.failed.iter().map(|f| f.key.as_str()).collect::<Vec<_>>().join("\n")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskProgress {
    pub id: String,
    pub title: String,
    pub depends_on: Vec<String>,
    pub status: TaskStatus,
    pub gates: Vec<GateRun>,
}

/// Everything [`advance`] decides from, persisted between steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PhaseRun {
    pub stage: Stage,
    pub tasks: Vec<TaskProgress>,
    /// Consecutive steps whose agent failed, reset by any step that finishes.
    pub step_failures: u32,
    pub started_ms: i64,
}

/// The outcome of one check, as the gatekeeper reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CheckOutcome {
    pub name: String,
    pub passed: bool,
    /// For a failure.
    pub signature: Option<Signature>,
}

/// What happened since the last decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The planner's turn ended, and this is `tasks.md` as the orchestrator read it.
    Planned(Result<Vec<TaskEntry>, Vec<Problem>>),
    /// A build or fix step's turn ended.
    Built,
    /// The checks after the current attempt finished.
    Checked(Vec<CheckOutcome>),
    /// The phase was committed and tagged.
    Tagged,
    /// The current step's agent failed, exited or couldn't start.
    StepFailed { why: String },
}

/// Which stop condition was hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Condition {
    /// A check still failed after the last fix attempt.
    FixAttempts,
    /// Fix attempts kept ending with the same error signature.
    SameSignature,
    WallClock,
    /// `tasks.md` was still invalid after the planner's retries.
    PlanInvalid,
    /// A step's agent failed again after its retries.
    StepFailed,
    /// A task stopped and nothing else in the phase can run.
    TaskStopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Stop {
    pub condition: Condition,
    /// The task it stopped, or `None` for the phase.
    pub task: Option<String>,
    /// What to tell Matthew: the failing check and its last error, or why the step failed.
    pub detail: String,
}

impl Stop {
    fn phase(condition: Condition, detail: impl Into<String>) -> Self {
        Self { condition, task: None, detail: detail.into() }
    }
}

/// What the orchestrator does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Start the planner, with the problems in the last `tasks.md` if this is a retry.
    Plan { attempt: u32, problems: Option<String> },
    /// Start a builder on `task`. Attempt 0 builds; later attempts fix the last gate report.
    /// `counsel` marks the attempt that should follow Counsel's diagnosis (built in slice 8).
    Build { task: String, attempt: u32, counsel: bool },
    Check { task: String, attempt: u32 },
    Tag,
    /// The phase passed and is tagged.
    Pass,
    Stop(Stop),
    /// Nothing to do: the event didn't fit the stage (a late or repeated report).
    Wait,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub phase: PhaseRun,
    pub action: Action,
    /// Tasks stopped by this decision while the rest of the phase goes on.
    pub task_stops: Vec<Stop>,
}

/// A new phase and its first action.
pub fn start(now_ms: i64) -> Decision {
    Decision {
        phase: PhaseRun { stage: Stage::Planning { attempt: 0, problems: None }, tasks: Vec::new(), step_failures: 0, started_ms: now_ms },
        action: Action::Plan { attempt: 0, problems: None },
        task_stops: Vec::new(),
    }
}

/// The action that starts (or restarts) whatever `stage` is doing, for resuming after Chorus
/// restarted mid-step: every step can be re-run safely.
pub fn resume(phase: &PhaseRun, limits: &Limits) -> Action {
    match &phase.stage {
        Stage::Planning { attempt, problems } => Action::Plan { attempt: *attempt, problems: problems.clone() },
        Stage::Building { task, attempt } => {
            Action::Build { task: task.clone(), attempt: *attempt, counsel: counsel_due(phase, task, *attempt, limits) }
        }
        Stage::Checking { task, attempt } => Action::Check { task: task.clone(), attempt: *attempt },
        Stage::Tagging => Action::Tag,
        Stage::Passed => Action::Pass,
        Stage::Stopped => Action::Wait,
    }
}

/// A stop if the phase has run past its wall-clock cap.
pub fn overdue(phase: &PhaseRun, limits: &Limits, now_ms: i64) -> Option<Stop> {
    let minutes = limits.phase_minutes?;
    let running = !matches!(phase.stage, Stage::Passed | Stage::Stopped);
    (running && now_ms - phase.started_ms > i64::from(minutes) * 60_000)
        .then(|| Stop::phase(Condition::WallClock, format!("The phase ran past its {} cap.", duration(minutes))))
}

fn duration(minutes: u32) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}-minute"),
        (h, 0) => format!("{h}-hour"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// Applies `event` to `phase`.
pub fn advance(phase: &PhaseRun, event: Event, limits: &Limits) -> Decision {
    let mut next = phase.clone();
    let wait = |p: PhaseRun| Decision { phase: p, action: Action::Wait, task_stops: Vec::new() };
    match (&phase.stage, event) {
        (_, Event::StepFailed { why }) if !matches!(phase.stage, Stage::Passed | Stage::Stopped) => {
            next.step_failures += 1;
            if next.step_failures > limits.step_retries {
                return stopped(next, Stop::phase(Condition::StepFailed, why), Vec::new());
            }
            let action = resume(&next, limits);
            Decision { phase: next, action, task_stops: Vec::new() }
        }
        (Stage::Planning { attempt, .. }, Event::Planned(Err(problems))) => {
            next.step_failures = 0;
            let text = files::describe("tasks.md", &problems);
            if *attempt >= limits.plan_retries {
                return stopped(next, Stop::phase(Condition::PlanInvalid, format!("tasks.md is still invalid:\n{text}")), Vec::new());
            }
            next.stage = Stage::Planning { attempt: attempt + 1, problems: Some(text.clone()) };
            Decision { phase: next, action: Action::Plan { attempt: attempt + 1, problems: Some(text) }, task_stops: Vec::new() }
        }
        (Stage::Planning { .. }, Event::Planned(Ok(tasks))) => {
            next.step_failures = 0;
            next.tasks = tasks
                .into_iter()
                .map(|t| {
                    // A task the file says is done stays done; anything else starts again.
                    let status = if t.status == TaskStatus::Done { TaskStatus::Done } else { TaskStatus::Pending };
                    TaskProgress { id: t.id, title: t.title, depends_on: t.depends_on, status, gates: Vec::new() }
                })
                .collect();
            continue_tasks(next, Vec::new())
        }
        (Stage::Building { task, attempt }, Event::Built) => {
            next.step_failures = 0;
            next.stage = Stage::Checking { task: task.clone(), attempt: *attempt };
            Decision { phase: next, action: Action::Check { task: task.clone(), attempt: *attempt }, task_stops: Vec::new() }
        }
        (Stage::Checking { task, attempt }, Event::Checked(outcomes)) => checked(next, task, *attempt, &outcomes, limits),
        (Stage::Tagging, Event::Tagged) => {
            next.stage = Stage::Passed;
            Decision { phase: next, action: Action::Pass, task_stops: Vec::new() }
        }
        _ => wait(next),
    }
}

fn stopped(mut phase: PhaseRun, stop: Stop, task_stops: Vec<Stop>) -> Decision {
    phase.stage = Stage::Stopped;
    Decision { phase, action: Action::Stop(stop), task_stops }
}

fn checked(mut phase: PhaseRun, task: &str, attempt: u32, outcomes: &[CheckOutcome], limits: &Limits) -> Decision {
    phase.step_failures = 0;
    let failed: Vec<Failure> = outcomes
        .iter()
        .filter(|o| !o.passed)
        .map(|o| {
            let sig = o.signature.clone().unwrap_or_else(|| signature::of(&o.name, ""));
            Failure { check: o.name.clone(), key: sig.key(), summary: sig.summary() }
        })
        .collect();
    let Some(t) = phase.tasks.iter_mut().find(|t| t.id == task) else {
        return stopped(phase, Stop::phase(Condition::StepFailed, format!("{task} is no longer in tasks.md.")), Vec::new());
    };
    t.gates.push(GateRun { attempt, failed: failed.clone() });
    if failed.is_empty() {
        t.status = TaskStatus::Done;
        return continue_tasks(phase, Vec::new());
    }
    let summaries = failed.iter().map(|f| f.summary.as_str()).collect::<Vec<_>>().join("; ");
    if same_signature_streak(t) >= limits.same_signature {
        t.status = TaskStatus::Stopped;
        let stop = Stop {
            condition: Condition::SameSignature,
            task: Some(task.to_owned()),
            detail: format!("{} fix attempts in a row ended with the same failure: {summaries}", limits.same_signature),
        };
        return continue_tasks(phase, vec![stop]);
    }
    if attempt >= limits.fix_attempts {
        t.status = TaskStatus::Failed;
        let stop = Stop {
            condition: Condition::FixAttempts,
            task: Some(task.to_owned()),
            detail: format!("{task} still fails after {attempt} fix attempts: {summaries}"),
        };
        return stopped(phase, stop, Vec::new());
    }
    let counsel = counsel_due(&phase, task, attempt + 1, limits);
    phase.stage = Stage::Building { task: task.to_owned(), attempt: attempt + 1 };
    Decision { phase, action: Action::Build { task: task.to_owned(), attempt: attempt + 1, counsel }, task_stops: Vec::new() }
}

/// How many fix attempts in a row, ending with the latest, failed with the same signature. The
/// first build's failure isn't counted: nothing has been fixed yet.
fn same_signature_streak(t: &TaskProgress) -> u32 {
    let fixes: Vec<&GateRun> = t.gates.iter().filter(|g| g.attempt > 0).collect();
    let Some(last) = fixes.last().filter(|g| !g.failed.is_empty()) else { return 0 };
    let key = last.key();
    fixes.iter().rev().take_while(|g| !g.failed.is_empty() && g.key() == key).count() as u32
}

/// Counsel diagnoses before the last fix attempt when the one before it failed the same way as the
/// attempt before that (the spec's "stuck fix loop": a second fix fails with the same signature).
fn counsel_due(phase: &PhaseRun, task: &str, attempt: u32, limits: &Limits) -> bool {
    attempt == limits.fix_attempts
        && attempt >= 2
        && phase.tasks.iter().find(|t| t.id == task).is_some_and(|t| {
            let fixes: Vec<&GateRun> = t.gates.iter().filter(|g| g.attempt > 0 && !g.failed.is_empty()).collect();
            fixes.len() >= 2 && fixes[fixes.len() - 1].key() == fixes[fixes.len() - 2].key()
        })
}

/// Starts the next task that can run, tags the phase when all are done, or stops it when what is
/// left can't run.
fn continue_tasks(mut phase: PhaseRun, task_stops: Vec<Stop>) -> Decision {
    let done = |id: &str, tasks: &[TaskProgress]| tasks.iter().any(|t| t.id == id && t.status == TaskStatus::Done);
    let runnable = phase
        .tasks
        .iter()
        .find(|t| t.status == TaskStatus::Pending && t.depends_on.iter().all(|d| done(d, &phase.tasks)))
        .map(|t| t.id.clone());
    if let Some(task) = runnable {
        phase.stage = Stage::Building { task: task.clone(), attempt: 0 };
        return Decision { phase, action: Action::Build { task, attempt: 0, counsel: false }, task_stops };
    }
    if phase.tasks.iter().all(|t| t.status == TaskStatus::Done) {
        phase.stage = Stage::Tagging;
        return Decision { phase, action: Action::Tag, task_stops };
    }
    let blocked: Vec<&str> = phase.tasks.iter().filter(|t| t.status != TaskStatus::Done).map(|t| t.id.as_str()).collect();
    let cause = task_stops.first().map(|s| format!(" {}", s.detail)).unwrap_or_default();
    let stop = Stop::phase(Condition::TaskStopped, format!("Nothing left can run ({} not done).{cause}", blocked.join(", ")));
    stopped(phase, stop, task_stops)
}

/// Whether a passing phase ends a milestone (the spec's review cadence): phase 1, a `risk: high`
/// phase, the `every`th phase since the last review, or a run that had stopped.
pub fn ends_milestone(phase_number: u32, high_risk: bool, phases_since_review: u32, every: u32, resumed_after_stop: bool) -> bool {
    phase_number == 1 || high_risk || resumed_after_stop || phases_since_review + 1 >= every.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use files::Risk;

    fn task(id: &str, deps: &[&str]) -> TaskEntry {
        TaskEntry {
            id: id.into(),
            title: format!("Task {id}"),
            depends_on: deps.iter().map(|d| (*d).to_owned()).collect(),
            claims: vec!["src/**".into()],
            accept: vec!["it works".into()],
            model: None,
            risk: Risk::Normal,
            status: TaskStatus::Pending,
            line: 1,
        }
    }

    fn fail(check: &str, output: &str) -> CheckOutcome {
        CheckOutcome { name: check.into(), passed: false, signature: Some(signature::of(check, output)) }
    }

    fn pass(check: &str) -> CheckOutcome {
        CheckOutcome { name: check.into(), passed: true, signature: None }
    }

    fn planned(tasks: Vec<TaskEntry>) -> (PhaseRun, Action) {
        let d = advance(&start(0).phase, Event::Planned(Ok(tasks)), &Limits::default());
        (d.phase, d.action)
    }

    fn build_t1() -> Action {
        Action::Build { task: "T1".into(), attempt: 0, counsel: false }
    }

    #[test]
    fn the_happy_path_plans_builds_checks_and_tags() {
        let l = Limits::default();
        let s = start(0);
        assert_eq!(s.action, Action::Plan { attempt: 0, problems: None });
        let (p, a) = planned(vec![task("T1", &[]), task("T2", &["T1"])]);
        assert_eq!(a, build_t1());
        let d = advance(&p, Event::Built, &l);
        assert_eq!(d.action, Action::Check { task: "T1".into(), attempt: 0 });
        let d = advance(&d.phase, Event::Checked(vec![pass("build"), pass("tests")]), &l);
        assert_eq!(d.action, Action::Build { task: "T2".into(), attempt: 0, counsel: false });
        let d = advance(&advance(&d.phase, Event::Built, &l).phase, Event::Checked(vec![pass("build")]), &l);
        assert_eq!(d.action, Action::Tag);
        let d = advance(&d.phase, Event::Tagged, &l);
        assert_eq!((d.action, d.phase.stage), (Action::Pass, Stage::Passed));
    }

    #[test]
    fn an_invalid_plan_goes_back_once_then_stops() {
        let l = Limits::default();
        let problems = vec![Problem { line: 3, message: "T1 claims no files".into() }];
        let d = advance(&start(0).phase, Event::Planned(Err(problems.clone())), &l);
        assert_eq!(d.action, Action::Plan { attempt: 1, problems: Some("tasks.md line 3: T1 claims no files".into()) });
        let d = advance(&d.phase, Event::Planned(Err(problems)), &l);
        let Action::Stop(stop) = d.action else { panic!() };
        assert_eq!(stop.condition, Condition::PlanInvalid);
        assert!(stop.detail.contains("line 3"));
    }

    /// Feeds a run of failed gates to T1, starting from its first build.
    fn fail_times(outputs: &[&str]) -> Decision {
        let l = Limits::default();
        let (p, _) = planned(vec![task("T1", &[])]);
        let mut d = Decision { phase: p, action: build_t1(), task_stops: Vec::new() };
        for out in outputs {
            d = advance(&d.phase, Event::Built, &l);
            d = advance(&d.phase, Event::Checked(vec![pass("build"), fail("tests", out)]), &l);
        }
        d
    }

    #[test]
    fn a_failure_gets_fix_attempts_then_the_phase_stops() {
        let d = fail_times(&["× a > one"]);
        assert_eq!(d.action, Action::Build { task: "T1".into(), attempt: 1, counsel: false });
        // Different failures each time: no streak, so the phase stops when the attempts run out.
        let d = fail_times(&["× a > one", "× a > two", "× a > three", "× a > four"]);
        let Action::Stop(stop) = d.action else { panic!("{:?}", d.action) };
        assert_eq!((stop.condition, stop.task.as_deref()), (Condition::FixAttempts, Some("T1")));
        assert!(stop.detail.contains("after 3 fix attempts: tests: a > four failed"), "{}", stop.detail);
        assert_eq!(d.phase.stage, Stage::Stopped);
    }

    #[test]
    fn a_stuck_fix_loop_gets_counsel_and_then_stops_the_task() {
        // The first build and two fixes fail the same way: the third fix follows Counsel.
        let d = fail_times(&["× a > one", "× a > one", "× a > one"]);
        assert_eq!(d.action, Action::Build { task: "T1".into(), attempt: 3, counsel: true });
        // The third fix fails the same way too: three fixes in a row, the task stops, and with
        // nothing else to run the phase stops with it.
        let d = fail_times(&["× a > one", "× a > one", "× a > one", "× a > one"]);
        assert_eq!(d.task_stops.len(), 1);
        assert_eq!(d.task_stops[0].condition, Condition::SameSignature);
        let Action::Stop(stop) = d.action else { panic!() };
        assert_eq!(stop.condition, Condition::TaskStopped);
        assert!(stop.detail.contains("T1 not done") && stop.detail.contains("same failure"), "{}", stop.detail);
    }

    #[test]
    fn progress_breaks_the_streak() {
        let d = fail_times(&["× a > one", "× a > one", "× a > two"]);
        assert_eq!(d.action, Action::Build { task: "T1".into(), attempt: 3, counsel: false });
    }

    #[test]
    fn a_stopped_task_lets_independent_tasks_run() {
        let l = Limits { same_signature: 2, ..Limits::default() };
        let (p, _) = planned(vec![task("T1", &[]), task("T2", &[]), task("T3", &["T1"])]);
        let mut d = Decision { phase: p, action: build_t1(), task_stops: Vec::new() };
        for _ in 0..3 {
            d = advance(&advance(&d.phase, Event::Built, &l).phase, Event::Checked(vec![fail("tests", "× a > one")]), &l);
        }
        assert_eq!(d.task_stops[0].task.as_deref(), Some("T1"));
        assert_eq!(d.action, Action::Build { task: "T2".into(), attempt: 0, counsel: false });
        // T2 passes; T3 depends on the stopped T1, so the phase stops.
        let d = advance(&advance(&d.phase, Event::Built, &l).phase, Event::Checked(vec![pass("tests")]), &l);
        let Action::Stop(stop) = d.action else { panic!() };
        assert!(stop.detail.contains("T1, T3 not done"), "{}", stop.detail);
    }

    #[test]
    fn a_failed_step_runs_again_once() {
        let l = Limits::default();
        let (p, _) = planned(vec![task("T1", &[])]);
        let d = advance(&p, Event::StepFailed { why: "the agent exited".into() }, &l);
        assert_eq!(d.action, build_t1());
        let d = advance(&d.phase, Event::StepFailed { why: "the agent exited again".into() }, &l);
        assert_eq!(d.action, Action::Stop(Stop::phase(Condition::StepFailed, "the agent exited again")));
        // A step that finishes resets the count.
        let d = advance(&p, Event::StepFailed { why: "x".into() }, &l);
        let d = advance(&d.phase, Event::Built, &l);
        assert_eq!(d.phase.step_failures, 0);
    }

    #[test]
    fn done_tasks_stay_done_when_planning_is_rerun() {
        let mut t1 = task("T1", &[]);
        t1.status = TaskStatus::Done;
        let (_, a) = planned(vec![t1, task("T2", &["T1"])]);
        assert_eq!(a, Action::Build { task: "T2".into(), attempt: 0, counsel: false });
    }

    #[test]
    fn late_or_repeated_events_wait() {
        let l = Limits::default();
        let (p, _) = planned(vec![task("T1", &[])]);
        assert_eq!(advance(&p, Event::Tagged, &l).action, Action::Wait);
        assert_eq!(advance(&p, Event::Checked(vec![]), &l).action, Action::Wait);
        let passed = PhaseRun { stage: Stage::Passed, ..p };
        assert_eq!(advance(&passed, Event::StepFailed { why: "x".into() }, &l).action, Action::Wait);
    }

    #[test]
    fn resume_restarts_the_current_step() {
        let l = Limits::default();
        let (p, _) = planned(vec![task("T1", &[])]);
        assert_eq!(resume(&p, &l), build_t1());
        let checking = advance(&p, Event::Built, &l).phase;
        assert_eq!(resume(&checking, &l), Action::Check { task: "T1".into(), attempt: 0 });
    }

    #[test]
    fn the_wall_clock_cap() {
        let l = Limits { phase_minutes: Some(90), ..Limits::default() };
        let p = start(0).phase;
        assert_eq!(overdue(&p, &l, 90 * 60_000), None);
        let stop = overdue(&p, &l, 90 * 60_000 + 1).unwrap();
        assert_eq!((stop.condition, stop.detail.as_str()), (Condition::WallClock, "The phase ran past its 1 h 30 min cap."));
        assert_eq!(overdue(&p, &Limits { phase_minutes: None, ..l }, i64::MAX), None);
        assert_eq!(overdue(&PhaseRun { stage: Stage::Passed, ..p }, &l, i64::MAX), None);
    }

    #[test]
    fn limits_are_checked() {
        assert!(Limits::default().check().is_ok());
        assert!(Limits { fix_attempts: 0, ..Limits::default() }.check().is_err());
        assert!(Limits { phase_minutes: Some(1), ..Limits::default() }.check().is_err());
    }

    #[test]
    fn milestones() {
        assert!(ends_milestone(1, false, 0, 3, false));
        assert!(!ends_milestone(2, false, 0, 3, false));
        assert!(ends_milestone(4, false, 2, 3, false));
        assert!(ends_milestone(3, true, 0, 3, false));
        assert!(ends_milestone(5, false, 0, 3, true));
    }
}
