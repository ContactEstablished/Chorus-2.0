//! The loop against a real git repository, with scripted steps and a real check command: a phase
//! that passes after a fix, a fix loop that doesn't converge, a plan that stays invalid and a step
//! that keeps failing. Each run's state is driven pass by pass, without the worker thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chorus_core::agent::{AgentKind, LaunchOptions};
use chorus_core::run_loop::Limits;
use chorus_core::run_loop::roles::{LoopRole, RoleSetting};

use super::db::{self, RunConfig, RunState, StepKind};
use super::gate::Check;
use super::{NewRun, worker};
use crate::bots::BotSave;
use crate::sessions::SessionManager;
use crate::store::Store;

struct Fixture {
    dir: PathBuf,
    repo: PathBuf,
    manager: Arc<SessionManager>,
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

const ROADMAP: &str = "# Roadmap\n\n## P01 · Sum\n- Goal: a sum that adds\n\n## P02 · Product\n- Goal: a product\n";
const SPEC: &str = "# P01 · Sum\n\nsum adds two numbers.\n";
const TASKS: &str = "## T1 · Sum\n- Claims: src/**\n- Accept: sum(1, 2) is 3\n";
/// Passes when the file adds, and says what's wrong when it doesn't.
const CHECK: &str = "findstr /C:\"return a + b\" src\\sum.txt >nul || (echo error: sum subtracts & exit /b 1)";

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("chorus-loop-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = dir.join("repo");
        std::fs::create_dir_all(repo.join(".chorus/run")).unwrap();
        std::fs::write(repo.join(".chorus/run/roadmap.md"), ROADMAP).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "Chorus test"]);
        git(&repo, &["config", "user.email", "test@example.invalid"]);
        git(&repo, &["add", "--all"]);
        git(&repo, &["commit", "-q", "-m", "Roadmap"]);
        let store = Store::open(&dir.join("chorus.db")).unwrap();
        let manager = SessionManager::new(store, dir.join("data"), Box::new(|_| {}));
        Self { dir, repo, manager }
    }

    fn bot(&self, name: &str) -> u32 {
        crate::bots::save(
            &self.manager,
            &BotSave {
                id: None,
                name: name.into(),
                agent: AgentKind::Claude,
                options: LaunchOptions { effort: None, permission: None, model: Some("haiku".into()) },
                provider_id: None,
                credential: None,
                description: None,
                instructions: String::new(),
                tags: Vec::new(),
                skills: Vec::new(),
                preset: None,
            },
        )
        .unwrap()
        .id
    }

    /// A run whose steps are played from `steps` (JSON array), started and driven until it waits,
    /// stops or finishes.
    fn run(&self, steps: &str) -> u32 {
        let script = self.dir.join("script.json");
        std::fs::write(&script, format!("{{\"steps\": {steps}}}")).unwrap();
        let (planner, builder) = (self.bot("Planner"), self.bot("Builder"));
        let config = RunConfig {
            roles: vec![
                RoleSetting { role: LoopRole::Planner, bot: planner, fallback: None },
                RoleSetting { role: LoopRole::Builder, bot: builder, fallback: None },
            ],
            checks: vec![Check { name: "sum".into(), command: CHECK.into(), timeout_secs: 30 }],
            limits: Limits::default(),
            milestone_every: 3,
            script: Some(script.display().to_string()),
        };
        let input = NewRun { name: "Sums".into(), folder: self.repo.display().to_string(), base_branch: None, config };
        let created = super::create(&self.manager, &input).unwrap();
        assert!(super::ready_to_run(&self.manager, created.id).unwrap());
        for _ in 0..20 {
            let run = db::run(self.manager.store(), created.id).unwrap().unwrap();
            if run.state != RunState::Running {
                break;
            }
            worker::pass(&self.manager, &run).unwrap();
        }
        created.id
    }

    fn worktree(&self, id: u32) -> PathBuf {
        PathBuf::from(db::run(self.manager.store(), id).unwrap().unwrap().worktree_path.unwrap())
    }

    fn kinds(&self, id: u32) -> Vec<(StepKind, u32)> {
        db::steps(self.manager.store(), id).unwrap().iter().map(|s| (s.kind, s.attempt)).collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn plan_step() -> String {
    format!(
        r#"{{"phase": 1, "step": "plan", "write": {{".chorus/run/phases/01-sum/spec.md": {}, ".chorus/run/phases/01-sum/tasks.md": {}}}}}"#,
        serde_json::to_string(SPEC).unwrap(),
        serde_json::to_string(TASKS).unwrap()
    )
}

fn build_step(attempt: u32, body: &str) -> String {
    format!(r#"{{"phase": 1, "step": "build", "task": "T1", "attempt": {attempt}, "write": {{"src/sum.txt": "{body}"}}}}"#)
}

#[test]
fn a_phase_passes_after_a_fix_and_waits_at_its_milestone() {
    let f = Fixture::new("pass");
    let id = f.run(&format!("[{}, {}, {}]", plan_step(), build_step(0, "return a - b"), build_step(1, "return a + b")));
    let run = db::run(f.manager.store(), id).unwrap().unwrap();
    assert_eq!(run.state, RunState::Waiting, "{:?}", run.note);
    assert!(run.note.as_deref().unwrap().contains("P01 · Sum passed and is tagged phase-01"), "{:?}", run.note);
    assert_eq!(
        f.kinds(id),
        [(StepKind::Plan, 0), (StepKind::Build, 0), (StepKind::Check, 0), (StepKind::Build, 1), (StepKind::Check, 1), (StepKind::Tag, 0)]
    );
    let wt = f.worktree(id);
    // The tag is on the run branch, which main never saw.
    assert_eq!(git(&wt, &["rev-parse", "phase-01^{commit}"]).trim(), git(&wt, &["rev-parse", "HEAD"]).trim());
    assert_eq!(git(&f.repo, &["rev-parse", "--abbrev-ref", "HEAD"]).trim(), "main");
    assert!(!f.repo.join("src").exists(), "the main checkout is untouched");
    let log = git(&wt, &["log", "--format=%s"]);
    assert!(log.contains("P01 · Sum: plan") && log.contains("P01 T1: Sum"), "{log}");
    // run-state.json is ignored, so the tag leaves nothing changed behind it.
    assert_eq!(git(&wt, &["status", "--porcelain"]).trim(), "", "a clean worktree at the tag");
    assert!(std::fs::read_to_string(wt.join(".chorus/run/.gitignore")).unwrap().contains("run-state.json"));
    let report = std::fs::read_to_string(wt.join(".chorus/run/phases/01-sum/gate-report.md")).unwrap();
    assert!(report.find("fix attempt 1: every check passed").unwrap() < report.find("first build: 1 of 1 checks failed").unwrap(), "{report}");
    assert!(report.contains("Failure: sum: error: sum subtracts"), "{report}");
    let tasks = std::fs::read_to_string(wt.join(".chorus/run/phases/01-sum/tasks.md")).unwrap();
    assert!(tasks.contains("- Status: done"), "{tasks}");
    let state: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(wt.join(".chorus/run/run-state.json")).unwrap()).unwrap();
    assert_eq!(state["phases"][0]["steps"][1]["modelObserved"], "scripted");
    assert_eq!(state["phases"][0]["steps"][1]["modelAsked"], "haiku");

    // Approving goes on to phase 2, whose plan the script doesn't have: that step fails twice and
    // the run stops, with phase 1's tag intact.
    super::approve(&f.manager, id).unwrap();
    for _ in 0..10 {
        let run = db::run(f.manager.store(), id).unwrap().unwrap();
        if run.state != RunState::Running {
            break;
        }
        worker::pass(&f.manager, &run).unwrap();
    }
    let run = db::run(f.manager.store(), id).unwrap().unwrap();
    assert_eq!(run.state, RunState::Stopped);
    let note = run.note.unwrap();
    assert!(note.contains("P02 · Product stopped because a step's agent failed again") && note.contains("last good state is phase-01"), "{note}");
}

#[test]
fn a_fix_loop_that_fails_the_same_way_stops() {
    let f = Fixture::new("stuck");
    let builds: Vec<String> = (0..4).map(|a| build_step(a, "return a - b")).collect();
    let id = f.run(&format!("[{}, {}]", plan_step(), builds.join(", ")));
    let run = db::run(f.manager.store(), id).unwrap().unwrap();
    assert_eq!(run.state, RunState::Stopped);
    let note = run.note.unwrap();
    assert!(note.contains("STOPPED: P01 · Sum stopped because a task stopped and nothing else can run"), "{note}");
    assert!(note.contains("3 fix attempts in a row ended with the same failure: sum: error: sum subtracts"), "{note}");
    assert_eq!(f.kinds(id).iter().filter(|(k, _)| *k == StepKind::Check).count(), 4);
    assert!(git(&f.worktree(id), &["tag"]).trim().is_empty(), "nothing was tagged");
}

#[test]
fn an_invalid_plan_goes_back_once_then_stops() {
    let f = Fixture::new("plan");
    let bad = r###"{"phase": 1, "step": "plan", "attempt": ATTEMPT, "write": {".chorus/run/phases/01-sum/tasks.md": "## T1 · Sum\n- Accept: x\n"}}"###;
    let id = f.run(&format!("[{}, {}]", bad.replace("ATTEMPT", "0"), bad.replace("ATTEMPT", "1")));
    let run = db::run(f.manager.store(), id).unwrap().unwrap();
    assert_eq!(run.state, RunState::Stopped);
    let note = run.note.unwrap();
    assert!(note.contains("the plan's tasks.md stayed invalid") && note.contains("spec.md wasn't written") && note.contains("T1 claims no files"), "{note}");
    assert_eq!(f.kinds(id), [(StepKind::Plan, 0), (StepKind::Plan, 1)]);
}

#[test]
fn checks_and_roles_are_required() {
    let f = Fixture::new("config");
    let bot = f.bot("Only");
    let config = RunConfig {
        roles: vec![RoleSetting { role: LoopRole::Planner, bot, fallback: None }],
        checks: Vec::new(),
        limits: Limits::default(),
        milestone_every: 3,
        script: None,
    };
    let input = NewRun { name: "x".into(), folder: f.repo.display().to_string(), base_branch: None, config: config.clone() };
    assert!(super::create(&f.manager, &input).unwrap_err().contains("Builder role"));
    let both = RunConfig {
        roles: vec![RoleSetting { role: LoopRole::Planner, bot, fallback: None }, RoleSetting { role: LoopRole::Builder, bot, fallback: None }],
        ..config
    };
    let input = NewRun { config: both.clone(), ..input };
    assert!(super::create(&f.manager, &input).unwrap_err().contains("at least one check"));
    let key = format!("sk-or-v1-{}", "a1".repeat(32));
    let secret = RunConfig { checks: vec![Check { name: "t".into(), command: format!("echo {key}"), timeout_secs: 30 }], ..both };
    assert!(super::create(&f.manager, &NewRun { config: secret, ..input }).unwrap_err().contains("looks like a key"));
}
