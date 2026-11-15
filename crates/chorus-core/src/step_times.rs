//! Step times for the Progress panel (P16, D115, D120, D122, D123).
//!
//! Turns stored plan snapshots, runs and tool calls into timed rows: which steps are done, current
//! and next, how long each was in progress, which skill drove it, and what the current step's tool
//! calls were. Every time comes from Chorus's clock as it stored each change; nothing here reads a
//! clock. A step that went straight to completed was never seen in progress, so it is "not timed".
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::progress::PlanStep;

/// One stored plan, at the time Chorus received it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSnapshot { pub at_ms: i64, pub run_id: Option<u32>, pub steps: Vec<PlanStep> }

/// One run of the agent: from launch (or resume) until it exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunSpan { pub id: u32, pub started_ms: i64, pub ended_ms: Option<i64> }

/// One stored tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub id: u32, pub turn_id: Option<u32>, pub run_id: Option<u32>, pub turn_ended_ms: Option<i64>,
    pub tool: String, pub target: Option<String>, pub started_ms: i64, pub ended_ms: Option<i64>, pub failed: bool,
}

/// The current step's last interval; `to_ms` is `None` while it still runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window { pub from_ms: i64, pub to_ms: Option<i64> }

/// How long a skill call drove the work, as an estimate: from the call until the next step started,
/// the next skill was called or its turn ended. `ended_ms` is `None` while it still runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillEstimate { pub call_id: u32, pub turn_id: Option<u32>, pub name: Option<String>, pub started_ms: i64, pub ended_ms: Option<i64> }

pub const SKILL_TOOL: &str = "Skill";
pub const MAX_SNAPSHOTS: u32 = 2000;
pub const MAX_SKILL_CALLS: u32 = 200;
pub const MAX_WINDOW_CALLS: u32 = 500;
pub const MAX_SKILL_ROWS: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum StepStatus { Pending, InProgress, Completed }

impl StepStatus {
    /// The plan's status text (`progress::status` already normalised it). Unknown reads pending.
    pub fn from_plan(status: &str) -> Self {
        match status { "in-progress" | "in_progress" | "inProgress" => Self::InProgress, "completed" => Self::Completed, _ => Self::Pending }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum RowKind { Step, Skill }

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionProgress {
    pub steps: Vec<TimedStep>,
    pub current: Option<u32>,
    pub next: Option<u32>,
    /// Step rows only, not skill rows.
    pub done: u32,
    pub total: u32,
    pub current_tools: Vec<ToolGroup>,
    pub plan_at_secs: Option<u32>,
    /// A turn reached the 500-row cap, so later plan updates in it weren't stored.
    pub capped: bool,
    /// Where the newest run's time went (16-2). `None` when the session never ran.
    pub split: Option<crate::time_split::TimeSplit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TimedStep {
    pub kind: RowKind,
    pub text: String,
    pub status: StepStatus,
    /// When it was first seen in progress in this plan.
    pub started_at_secs: Option<u32>,
    /// Closed time. While it runs in the live run, add `now - running_since_secs`.
    pub secs: u32,
    pub running_since_secs: Option<u32>,
    /// False when it was completed without ever being seen in progress.
    pub timed: bool,
    pub skills: Vec<SkillUse>,
}

/// A skill call shown as a chip. Its time is always an estimate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SkillUse { pub call_id: u32, pub name: Option<String>, pub at_secs: u32, pub secs: u32, pub running_since_secs: Option<u32> }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum ToolKind { Read, Edit, Run, Search, Web, Agent, Other }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ToolGroup {
    pub kind: ToolKind,
    pub tool: String,
    pub count: u32,
    /// Set only when every call in the group shares it.
    pub target: Option<String>,
    pub secs: u32,
    pub running_since_secs: Option<u32>,
    pub failed: u32,
}

/// Instants: whole seconds, saturating.
fn at_secs(ms: i64) -> u32 { u32::try_from(ms.max(0) / 1000).unwrap_or(u32::MAX) }
/// Durations: rounded to the nearest second, saturating.
fn dur_secs(ms: i64) -> u32 { u32::try_from((ms.max(0) + 500) / 1000).unwrap_or(u32::MAX) }

/// When a run stopped counting: its exit, else the next run's start. `None`: unknown.
fn run_end(runs: &[RunSpan], id: Option<u32>) -> Option<i64> {
    let id = id?;
    let i = runs.iter().position(|r| r.id == id)?;
    runs[i].ended_ms.or_else(|| runs.get(i + 1).map(|r| r.started_ms))
}

/// A run still counts as running only when it is the live one and hasn't ended.
fn is_live(runs: &[RunSpan], live_run: Option<u32>, id: Option<u32>) -> bool {
    id.is_some() && id == live_run && runs.iter().find(|r| Some(r.id) == id).is_none_or(|r| r.ended_ms.is_none())
}

/// The k-th step with text T in a snapshot is the k-th step with text T in the one before (K1).
fn keys(steps: &[PlanStep]) -> Vec<(String, u32)> {
    let mut seen: HashMap<String, u32> = HashMap::new();
    steps.iter().map(|s| {
        let text = s.text.trim().to_owned();
        let n = seen.entry(text.clone()).or_default();
        *n += 1;
        (text, *n)
    }).collect()
}

#[derive(Debug, Default, Clone)]
struct Track {
    status: Option<StepStatus>,
    /// Closed intervals.
    closed: Vec<(i64, i64)>,
    /// The open interval: its start and its run.
    open: Option<(i64, Option<u32>)>,
    first: Option<i64>,
    seen_in_progress: bool,
}

impl Track {
    fn close(&mut self, at: i64, runs: &[RunSpan]) {
        if let Some((start, run)) = self.open.take() {
            let end = run_end(runs, run).map_or(at, |e| e.min(at)).max(start);
            self.closed.push((start, end));
        }
    }
    fn reset(&mut self) { *self = Self { status: self.status, ..Self::default() }; }
}

/// The walk over every snapshot: the newest plan's tracks, and every interval opening in any plan.
struct Walk {
    /// The newest plan's rows, in the newest snapshot's order, with their tracks.
    rows: Vec<(PlanStep, (String, u32))>,
    tracks: HashMap<(String, u32), Track>,
    /// Every time any step (in any plan, removed or not) opened an interval.
    openings: Vec<i64>,
    /// The last snapshot before the newest plan began. `None`: the newest plan is the first.
    previous_plan_end: Option<i64>,
    newest_at: Option<i64>,
    /// The latest snapshot time seen in each run, to close an interval whose run end is unknown.
    last_in_run: HashMap<Option<u32>, i64>,
}

fn walk(snapshots: &[PlanSnapshot], runs: &[RunSpan]) -> Walk {
    let mut order: Vec<&PlanSnapshot> = snapshots.iter().filter(|s| !s.steps.is_empty()).collect();
    order.sort_by_key(|s| s.at_ms); // stable: ties keep input order
    let mut w = Walk { rows: Vec::new(), tracks: HashMap::new(), openings: Vec::new(), previous_plan_end: None, newest_at: None, last_in_run: HashMap::new() };
    let mut previous: Option<(&PlanSnapshot, Vec<(String, u32)>)> = None;
    for snap in order {
        let ks = keys(&snap.steps);
        let entry = w.last_in_run.entry(snap.run_id).or_insert(snap.at_ms);
        *entry = (*entry).max(snap.at_ms);
        if let Some((prev, prev_keys)) = &previous {
            let shares = ks.iter().any(|k| prev_keys.iter().any(|p| p.0 == k.0));
            if !shares {
                // A new plan (K2): the old plan's open intervals end here, and its times are dropped.
                w.previous_plan_end = Some(prev.at_ms);
                w.tracks.clear();
            }
        }
        // Steps dropped from the plan close their intervals.
        let present: std::collections::HashSet<&(String, u32)> = ks.iter().collect();
        for (k, t) in w.tracks.iter_mut() {
            if !present.contains(k) { t.close(snap.at_ms, runs); }
        }
        for (step, k) in snap.steps.iter().zip(&ks) {
            let status = StepStatus::from_plan(&step.status);
            let t = w.tracks.entry(k.clone()).or_default();
            // The previous interval belonged to a run that has ended: it closes at that run's end.
            if let Some((_, run)) = t.open && run != snap.run_id && run_end(runs, run).is_some_and(|e| e <= snap.at_ms) {
                t.close(snap.at_ms, runs);
            }
            match status {
                StepStatus::InProgress => {
                    if t.open.is_none() {
                        t.open = Some((snap.at_ms, snap.run_id));
                        t.first.get_or_insert(snap.at_ms);
                        t.seen_in_progress = true;
                        w.openings.push(snap.at_ms);
                    }
                }
                other => {
                    t.close(snap.at_ms, runs);
                    if other == StepStatus::Pending && t.status == Some(StepStatus::Completed) { t.reset(); }
                }
            }
            t.status = Some(status);
        }
        w.newest_at = Some(snap.at_ms);
        w.rows = snap.steps.iter().cloned().zip(ks.iter().cloned()).collect();
        previous = Some((snap, ks));
    }
    w.openings.sort_unstable();
    w
}

/// Where an interval that is still open stops counting, or `None` when it runs in the live run.
fn open_end(w: &Walk, runs: &[RunSpan], live_run: Option<u32>, start: i64, run: Option<u32>) -> Option<i64> {
    if is_live(runs, live_run, run) { return None; }
    Some(run_end(runs, run).or_else(|| w.last_in_run.get(&run).copied()).unwrap_or(start).max(start))
}

/// Each skill call's estimate (K3). `compute` uses it and 16-4 reuses it, so there is one rule.
pub fn skill_estimates(snapshots: &[PlanSnapshot], runs: &[RunSpan], live_run: Option<u32>, skill_calls: &[Call]) -> Vec<SkillEstimate> {
    let w = walk(snapshots, runs);
    estimates(&w, runs, live_run, skill_calls)
}

fn estimates(w: &Walk, runs: &[RunSpan], live_run: Option<u32>, skill_calls: &[Call]) -> Vec<SkillEstimate> {
    let mut calls: Vec<&Call> = skill_calls.iter().filter(|c| c.tool == SKILL_TOOL).collect();
    calls.sort_by_key(|c| (c.started_ms, c.id));
    calls.iter().enumerate().map(|(i, c)| {
        let opening = w.openings.iter().copied().find(|&o| o > c.started_ms);
        let next_skill = calls.get(i + 1).map(|n| n.started_ms);
        let mut end = [opening, next_skill, c.turn_ended_ms].into_iter().flatten().min();
        if let Some(stop) = run_end(runs, c.run_id) { end = Some(end.map_or(stop, |e| e.min(stop))); }
        let end = match end {
            Some(e) => Some(e.max(c.started_ms)),
            None if is_live(runs, live_run, c.run_id) => None,
            None => Some(c.started_ms),
        };
        SkillEstimate { call_id: c.id, turn_id: c.turn_id, name: c.target.clone(), started_ms: c.started_ms, ended_ms: end }
    }).collect()
}

fn skill_use(e: &SkillEstimate) -> SkillUse {
    SkillUse {
        call_id: e.call_id, name: e.name.clone(), at_secs: at_secs(e.started_ms),
        secs: e.ended_ms.map_or(0, |end| dur_secs(end - e.started_ms)),
        running_since_secs: e.ended_ms.is_none().then(|| at_secs(e.started_ms)),
    }
}

/// The session's plan with step times, skill chips and the current step's window.
pub fn compute(snapshots: &[PlanSnapshot], runs: &[RunSpan], live_run: Option<u32>, skill_calls: &[Call]) -> (SessionProgress, Option<Window>) {
    let w = walk(snapshots, runs);
    let mut steps = Vec::new();
    // Each row's intervals (start, end; None while running), for skill attachment and the window.
    let mut spans: Vec<Vec<(i64, Option<i64>)>> = Vec::new();
    for (step, k) in &w.rows {
        let t = w.tracks.get(k).cloned().unwrap_or_default();
        let status = StepStatus::from_plan(&step.status);
        let mut intervals: Vec<(i64, Option<i64>)> = t.closed.iter().map(|&(s, e)| (s, Some(e))).collect();
        let mut secs: u32 = t.closed.iter().map(|&(s, e)| dur_secs(e - s)).fold(0, u32::saturating_add);
        let mut running = None;
        if let Some((start, run)) = t.open {
            match open_end(&w, runs, live_run, start, run) {
                None => { running = Some(at_secs(start)); intervals.push((start, None)); }
                Some(end) => { secs = secs.saturating_add(dur_secs(end - start)); intervals.push((start, Some(end))); }
            }
        }
        let timed = !(status == StepStatus::Completed && !t.seen_in_progress);
        steps.push(TimedStep {
            kind: RowKind::Step, text: step.text.trim().to_owned(), status,
            started_at_secs: t.first.map(at_secs), secs: if timed { secs } else { 0 },
            running_since_secs: running, timed, skills: Vec::new(),
        });
        spans.push(intervals);
    }

    // Skills (rule 8): calls made after the previous plan's last snapshot; with no plan, the newest.
    let mut calls: Vec<Call> = skill_calls.iter().filter(|c| c.tool == SKILL_TOOL).cloned().collect();
    calls.sort_by_key(|c| (c.started_ms, c.id));
    if steps.is_empty() {
        let skip = calls.len().saturating_sub(MAX_SKILL_ROWS);
        calls.drain(..skip);
    } else if let Some(after) = w.previous_plan_end {
        calls.retain(|c| c.started_ms > after);
    }
    let estimates = estimates(&w, runs, live_run, &calls);
    let mut skill_rows: Vec<TimedStep> = Vec::new();
    for e in &estimates {
        let within = spans.iter().position(|iv| iv.iter().any(|&(s, end)| s <= e.started_ms && end.is_none_or(|end| e.started_ms < end)));
        let next = spans.iter().enumerate()
            .filter_map(|(i, iv)| iv.iter().map(|&(s, _)| s).filter(|&s| s > e.started_ms).min().map(|s| (s, i)))
            .min().map(|(_, i)| i);
        match within.or(next) {
            Some(i) => steps[i].skills.push(skill_use(e)),
            None if skill_rows.len() < MAX_SKILL_ROWS => {
                let u = skill_use(e);
                skill_rows.push(TimedStep {
                    kind: RowKind::Skill, text: e.name.clone().unwrap_or_else(|| "skill".into()),
                    status: if u.running_since_secs.is_some() { StepStatus::InProgress } else { StepStatus::Completed },
                    started_at_secs: Some(u.at_secs), secs: u.secs, running_since_secs: u.running_since_secs, timed: true, skills: Vec::new(),
                });
                spans.push(vec![(e.started_ms, e.ended_ms)]);
            }
            None => {}
        }
    }
    let plan_rows = steps.len();
    steps.extend(skill_rows);

    let current = steps.iter().position(|s| s.status == StepStatus::InProgress);
    let next = match current {
        Some(c) => steps.iter().skip(c + 1).position(|s| s.status == StepStatus::Pending).map(|i| i + c + 1),
        None => steps.iter().position(|s| s.status == StepStatus::Pending),
    };
    let window = current.and_then(|c| spans.get(c)?.last().map(|&(from_ms, to_ms)| Window { from_ms, to_ms }));
    let progress = SessionProgress {
        done: u32::try_from(steps[..plan_rows].iter().filter(|s| s.status == StepStatus::Completed).count()).unwrap_or(u32::MAX),
        total: u32::try_from(plan_rows).unwrap_or(u32::MAX),
        current: current.and_then(|c| u32::try_from(c).ok()),
        next: next.and_then(|n| u32::try_from(n).ok()),
        current_tools: Vec::new(),
        plan_at_secs: w.newest_at.map(at_secs),
        capped: false,
        split: None,
        steps,
    };
    (progress, window)
}

/// The latest plan at a glance, for an Overview card's bar (16-3, K7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProgressGlance {
    pub done: u32,
    pub total: u32,
    /// Each step's status in plan order (plans hold at most 100 steps).
    pub statuses: Vec<StepStatus>,
    /// The first in-progress step's text.
    pub current: Option<String>,
    /// When it started, while it runs in the live run; `None` when stopped or only seeded.
    pub current_since_secs: Option<u32>,
    /// The first pending step after the current one, or the first pending one.
    pub next: Option<String>,
}

/// The first in-progress step, by index.
fn first_in_progress(steps: &[PlanStep]) -> Option<usize> {
    steps.iter().position(|s| StepStatus::from_plan(&s.status) == StepStatus::InProgress)
}

/// `None` for an empty plan (D122: no bar without a plan).
pub fn glance(steps: &[PlanStep], current_since_ms: Option<i64>) -> Option<ProgressGlance> {
    if steps.is_empty() { return None; }
    let statuses: Vec<StepStatus> = steps.iter().map(|s| StepStatus::from_plan(&s.status)).collect();
    let current = first_in_progress(steps);
    let pending_from = |from: usize| statuses.iter().skip(from).position(|&s| s == StepStatus::Pending).map(|i| i + from);
    let next = current.map_or_else(|| pending_from(0), |c| pending_from(c + 1));
    Some(ProgressGlance {
        done: u32::try_from(statuses.iter().filter(|&&s| s == StepStatus::Completed).count()).unwrap_or(u32::MAX),
        total: u32::try_from(steps.len()).unwrap_or(u32::MAX),
        current: current.map(|c| steps[c].text.trim().to_owned()),
        current_since_secs: current.and(current_since_ms).map(at_secs),
        next: next.map(|n| steps[n].text.trim().to_owned()),
        statuses,
    })
}

/// The current step's text and since when, after a new snapshot at `at_ms`. The same step (the same
/// trimmed text, as K1 matches) keeps its start; another step starts at `at_ms`; none in progress
/// gives `None`.
pub fn advance_current(prev: Option<&(String, i64)>, steps: &[PlanStep], at_ms: i64) -> Option<(String, i64)> {
    let text = steps[first_in_progress(steps)?].text.trim().to_owned();
    match prev {
        Some((p, since)) if *p == text => Some((text, *since)),
        _ => Some((text, at_ms)),
    }
}

/// Tools that are bookkeeping, not work, and never appear in a summary.
const NOT_WORK: &[&str] = &["Skill", "TodoWrite", "TaskCreate", "TaskUpdate", "TaskList", "TaskGet", "ToolSearch"];

fn kind_of(tool: &str) -> ToolKind {
    match tool {
        "Read" => ToolKind::Read,
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" | "fileChange" => ToolKind::Edit,
        "Bash" | "PowerShell" | "commandExecution" => ToolKind::Run,
        "Grep" | "Glob" => ToolKind::Search,
        "WebFetch" | "WebSearch" => ToolKind::Web,
        "Agent" | "Task" => ToolKind::Agent,
        _ => ToolKind::Other,
    }
}

/// The current step's tool calls, grouped by kind (other tools by name), in first-seen order (D120).
pub fn tool_groups(calls: &[Call], window_open: bool) -> Vec<ToolGroup> {
    let mut sorted: Vec<&Call> = calls.iter().filter(|c| !NOT_WORK.contains(&c.tool.as_str())).collect();
    sorted.sort_by_key(|c| (c.started_ms, c.id));
    let mut groups: Vec<ToolGroup> = Vec::new();
    let mut targets: Vec<bool> = Vec::new(); // whether every call so far shared the group's target
    for c in sorted {
        let kind = kind_of(&c.tool);
        let i = groups.iter().position(|g| g.kind == kind && (kind != ToolKind::Other || g.tool == c.tool)).unwrap_or_else(|| {
            groups.push(ToolGroup { kind, tool: c.tool.clone(), count: 0, target: c.target.clone(), secs: 0, running_since_secs: None, failed: 0 });
            targets.push(true);
            groups.len() - 1
        });
        let g = &mut groups[i];
        g.count += 1;
        if g.target != c.target { targets[i] = false; }
        if c.failed { g.failed += 1; }
        match c.ended_ms {
            Some(end) => g.secs = g.secs.saturating_add(dur_secs(end - c.started_ms)),
            None if window_open => { let s = at_secs(c.started_ms); g.running_since_secs = Some(g.running_since_secs.map_or(s, |r| r.min(s))); }
            None => {}
        }
    }
    for (g, same) in groups.iter_mut().zip(targets) { if !same { g.target = None; } }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(text: &str, status: &str) -> PlanStep { PlanStep { id: text.into(), text: text.into(), status: status.into() } }
    fn snap(at_s: i64, steps: &[(&str, &str)]) -> PlanSnapshot {
        PlanSnapshot { at_ms: at_s * 1000, run_id: Some(1), steps: steps.iter().map(|(t, s)| step(t, s)).collect() }
    }
    fn live() -> Vec<RunSpan> { vec![RunSpan { id: 1, started_ms: 0, ended_ms: None }] }
    fn skill(id: u32, at_s: i64, name: Option<&str>) -> Call {
        Call { id, turn_id: Some(1), run_id: Some(1), turn_ended_ms: None, tool: SKILL_TOOL.into(), target: name.map(Into::into), started_ms: at_s * 1000, ended_ms: Some(at_s * 1000 + 50), failed: false }
    }
    fn tool(id: u32, name: &str, target: Option<&str>, from_s: i64, to_s: Option<i64>) -> Call {
        Call { id, turn_id: Some(1), run_id: Some(1), turn_ended_ms: None, tool: name.into(), target: target.map(Into::into), started_ms: from_s * 1000, ended_ms: to_s.map(|s| s * 1000), failed: false }
    }
    const P: &str = "pending"; const I: &str = "in-progress"; const C: &str = "completed";

    #[test]
    fn a_step_is_timed_from_in_progress_to_completed() {
        let (p, _) = compute(&[snap(10, &[("A", I), ("B", P)]), snap(70, &[("A", C), ("B", I)])], &live(), Some(1), &[]);
        assert_eq!(p.steps[0].secs, 60);
        assert_eq!(p.steps[0].started_at_secs, Some(10));
        assert!(p.steps[0].timed);
        assert_eq!((p.done, p.total, p.current, p.next), (1, 2, Some(1), None));
    }

    #[test]
    fn a_step_completed_without_being_seen_in_progress_is_not_timed() {
        let (p, _) = compute(&[snap(10, &[("A", P)]), snap(20, &[("A", C)])], &live(), Some(1), &[]);
        assert!(!p.steps[0].timed);
        assert_eq!(p.steps[0].secs, 0);
    }

    #[test]
    fn several_steps_marked_done_in_one_update_are_not_timed() {
        let (p, _) = compute(&[snap(10, &[("A", I), ("B", P), ("C", P)]), snap(40, &[("A", C), ("B", C), ("C", C)])], &live(), Some(1), &[]);
        assert!(p.steps[0].timed);
        assert!(!p.steps[1].timed && !p.steps[2].timed);
        assert_eq!(p.done, 3);
    }

    #[test]
    fn the_current_step_runs_while_its_run_lives() {
        let (p, w) = compute(&[snap(10, &[("A", I)])], &live(), Some(1), &[]);
        assert_eq!(p.steps[0].running_since_secs, Some(10));
        assert_eq!(p.steps[0].secs, 0);
        assert_eq!(w, Some(Window { from_ms: 10_000, to_ms: None }));
    }

    #[test]
    fn an_interval_ends_when_its_run_ends() {
        let runs = [RunSpan { id: 1, started_ms: 0, ended_ms: Some(40_000) }, RunSpan { id: 2, started_ms: 100_000, ended_ms: None }];
        let (p, w) = compute(&[snap(10, &[("A", I)])], &runs, Some(2), &[]);
        assert_eq!(p.steps[0].running_since_secs, None);
        assert_eq!(p.steps[0].secs, 30);
        assert_eq!(w, Some(Window { from_ms: 10_000, to_ms: Some(40_000) }));
    }

    #[test]
    fn a_reopened_step_adds_its_second_interval() {
        let (p, _) = compute(&[snap(0, &[("A", I)]), snap(10, &[("A", C)]), snap(20, &[("A", I)]), snap(25, &[("A", C)])], &live(), Some(1), &[]);
        assert_eq!(p.steps[0].secs, 15);
        assert_eq!(p.steps[0].started_at_secs, Some(0));
    }

    #[test]
    fn a_step_back_to_pending_after_done_starts_over() {
        let (p, _) = compute(&[snap(0, &[("A", I)]), snap(10, &[("A", C)]), snap(20, &[("A", P)])], &live(), Some(1), &[]);
        assert_eq!(p.steps[0].secs, 0);
        assert_eq!(p.steps[0].started_at_secs, None);
        assert_eq!(p.steps[0].status, StepStatus::Pending);
    }

    #[test]
    fn a_plan_replaced_wholesale_starts_a_new_epoch() {
        let (p, _) = compute(&[snap(0, &[("A", I)]), snap(30, &[("X", I), ("Y", P)])], &live(), Some(1), &[]);
        assert_eq!(p.steps.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), ["X", "Y"]);
        assert_eq!(p.steps[0].started_at_secs, Some(30));
    }

    #[test]
    fn steps_removed_from_the_plan_are_not_shown() {
        let (p, _) = compute(&[snap(0, &[("A", C), ("B", P)]), snap(10, &[("A", C)])], &live(), Some(1), &[]);
        assert_eq!(p.total, 1);
        assert_eq!(p.steps.len(), 1);
    }

    #[test]
    fn steps_match_by_text_when_ids_are_positions() {
        // TodoWrite: ids are positions, so inserting a step shifts them; text still matches.
        let first = PlanSnapshot { at_ms: 0, run_id: Some(1), steps: vec![PlanStep { id: "0".into(), text: "Build".into(), status: I.into() }] };
        let second = PlanSnapshot { at_ms: 20_000, run_id: Some(1), steps: vec![
            PlanStep { id: "0".into(), text: "Plan".into(), status: C.into() },
            PlanStep { id: "1".into(), text: "Build".into(), status: C.into() },
        ] };
        let (p, _) = compute(&[first, second], &live(), Some(1), &[]);
        assert_eq!(p.steps[1].secs, 20);
        assert!(!p.steps[0].timed);
    }

    #[test]
    fn duplicate_texts_match_in_order() {
        let (p, _) = compute(&[snap(0, &[("Test", C), ("Test", I)]), snap(5, &[("Test", C), ("Test", C)])], &live(), Some(1), &[]);
        assert!(!p.steps[0].timed);
        assert_eq!(p.steps[1].secs, 5);
    }

    #[test]
    fn a_skill_is_a_chip_on_the_step_in_progress() {
        let (p, _) = compute(&[snap(0, &[("A", I), ("B", P)]), snap(100, &[("A", C), ("B", I)])], &live(), Some(1), &[skill(7, 30, Some("grilling"))]);
        assert_eq!(p.steps[0].skills.len(), 1);
        assert_eq!(p.steps[0].skills[0].name.as_deref(), Some("grilling"));
        assert_eq!(p.steps[0].skills[0].secs, 70); // until B opened at 100
    }

    #[test]
    fn a_skill_between_steps_goes_to_the_next_step() {
        let s = [snap(0, &[("A", I), ("B", P)]), snap(10, &[("A", C), ("B", P)]), snap(50, &[("A", C), ("B", I)])];
        let (p, _) = compute(&s, &live(), Some(1), &[skill(1, 20, Some("architect"))]);
        assert!(p.steps[0].skills.is_empty());
        assert_eq!(p.steps[1].skills[0].secs, 30);
    }

    #[test]
    fn a_skill_called_before_the_plan_goes_to_its_first_step() {
        let (p, _) = compute(&[snap(40, &[("A", I), ("B", P)])], &live(), Some(1), &[skill(1, 5, Some("phase-kickoff"))]);
        assert_eq!(p.steps[0].skills.len(), 1);
        assert_eq!(p.steps[0].skills[0].secs, 35);
    }

    #[test]
    fn a_skill_without_a_plan_is_its_own_row() {
        let mut first = skill(1, 0, Some("grilling"));
        first.turn_ended_ms = Some(300_000);
        let (p, w) = compute(&[], &live(), Some(1), &[first, skill(2, 600, Some("architect"))]);
        assert_eq!(p.steps.len(), 2);
        assert_eq!(p.steps[0].kind, RowKind::Skill);
        assert_eq!((p.steps[0].text.as_str(), p.steps[0].secs, p.steps[0].status), ("grilling", 300, StepStatus::Completed));
        assert_eq!(p.steps[1].status, StepStatus::InProgress);
        assert_eq!(p.steps[1].running_since_secs, Some(600));
        assert_eq!((p.total, p.current), (0, Some(1)));
        assert_eq!(w, Some(Window { from_ms: 600_000, to_ms: None }));
    }

    #[test]
    fn a_skill_estimate_ends_at_the_first_boundary() {
        let mut a = skill(1, 0, Some("a"));
        a.turn_ended_ms = Some(50_000);
        let b = skill(2, 20, Some("b"));
        let e = skill_estimates(&[snap(10, &[("X", I)])], &live(), Some(1), &[a, b]);
        assert_eq!(e[0].ended_ms, Some(10_000)); // the step opened first
        // b: no later opening or skill; its turn hasn't ended and the run is live, so it runs.
        assert_eq!(e[1].ended_ms, None);
        let ended = [RunSpan { id: 1, started_ms: 0, ended_ms: Some(90_000) }];
        assert_eq!(skill_estimates(&[], &ended, None, &[skill(3, 30, None)])[0].ended_ms, Some(90_000));
    }

    #[test]
    fn an_unnamed_skill_call_is_kept_without_a_name() {
        let (p, _) = compute(&[], &live(), Some(1), &[skill(1, 0, None)]);
        assert_eq!(p.steps[0].text, "skill");
        let e = skill_estimates(&[], &live(), Some(1), &[skill(1, 0, None)]);
        assert_eq!(e[0].name, None);
    }

    #[test]
    fn tool_calls_group_by_kind_in_first_seen_order() {
        let calls = [
            tool(1, "Read", Some("a.rs"), 0, Some(1)), tool(2, "Bash", Some("cargo test"), 2, Some(32)),
            tool(3, "Read", Some("b.rs"), 33, Some(34)), tool(4, "Write", Some("c.rs"), 35, Some(36)),
            tool(5, "mcp__x", None, 37, None),
        ];
        let g = tool_groups(&calls, true);
        assert_eq!(g.iter().map(|g| (g.kind, g.count)).collect::<Vec<_>>(), [(ToolKind::Read, 2), (ToolKind::Run, 1), (ToolKind::Edit, 1), (ToolKind::Other, 1)]);
        assert_eq!(g[0].target, None);
        assert_eq!(g[1].target.as_deref(), Some("cargo test"));
        assert_eq!(g[1].secs, 30);
        assert_eq!(g[3].running_since_secs, Some(37));
        assert_eq!(tool_groups(&calls, false)[3].running_since_secs, None);
    }

    #[test]
    fn plan_bookkeeping_and_skills_are_not_tool_groups() {
        let calls = [tool(1, "TodoWrite", None, 0, Some(1)), tool(2, "Skill", Some("x"), 1, Some(2)), tool(3, "TaskUpdate", None, 2, Some(3)), tool(4, "ToolSearch", None, 3, Some(4))];
        assert!(tool_groups(&calls, true).is_empty());
    }

    #[test]
    fn snapshots_are_sorted_by_time() {
        let (p, _) = compute(&[snap(70, &[("A", C)]), snap(10, &[("A", I)])], &live(), Some(1), &[]);
        assert_eq!(p.steps[0].secs, 60);
        assert_eq!(p.plan_at_secs, Some(70));
    }

    #[test]
    fn no_input_gives_empty_progress() {
        let (p, w) = compute(&[], &[], None, &[]);
        assert_eq!(p, SessionProgress::default());
        assert_eq!(w, None);
    }

    #[test]
    fn statuses_read_as_the_plan_wrote_them() {
        assert_eq!(StepStatus::from_plan("in-progress"), StepStatus::InProgress);
        assert_eq!(StepStatus::from_plan("completed"), StepStatus::Completed);
        assert_eq!(StepStatus::from_plan("odd"), StepStatus::Pending);
    }

    fn steps(list: &[(&str, &str)]) -> Vec<PlanStep> { list.iter().map(|(t, s)| step(t, s)).collect() }

    #[test]
    fn glance_counts_done_and_names_the_current_step() {
        let g = glance(&steps(&[("A", C), ("B", P), ("C", I), ("D", P), ("E", I)]), Some(42_400)).unwrap();
        assert_eq!((g.done, g.total), (1, 5));
        assert_eq!(g.statuses, vec![StepStatus::Completed, StepStatus::Pending, StepStatus::InProgress, StepStatus::Pending, StepStatus::InProgress]);
        assert_eq!(g.current.as_deref(), Some("C"));
        assert_eq!(g.current_since_secs, Some(42));
        // The first pending step after the current one, not the one before it.
        assert_eq!(g.next.as_deref(), Some("D"));
        // No current step: the first pending one is next, and there is no time.
        let g = glance(&steps(&[("A", C), ("B", P)]), Some(5_000)).unwrap();
        assert_eq!((g.current, g.current_since_secs, g.next.as_deref()), (None, None, Some("B")));
        let g = glance(&steps(&[("A", C), ("B", C)]), None).unwrap();
        assert_eq!((g.done, g.current, g.next), (2, None, None));
    }

    #[test]
    fn no_steps_give_no_glance() {
        assert_eq!(glance(&[], Some(1_000)), None);
    }

    #[test]
    fn advance_current_keeps_the_start_while_the_same_step_runs() {
        let prev = ("B".to_owned(), 10_000);
        assert_eq!(advance_current(Some(&prev), &steps(&[("A", C), (" B ", I), ("C", P)]), 50_000), Some(("B".into(), 10_000)));
    }

    #[test]
    fn advance_current_restarts_when_another_step_starts() {
        let prev = ("B".to_owned(), 10_000);
        assert_eq!(advance_current(Some(&prev), &steps(&[("B", C), ("C", I)]), 50_000), Some(("C".into(), 50_000)));
        assert_eq!(advance_current(None, &steps(&[("A", I)]), 7_000), Some(("A".into(), 7_000)));
    }

    #[test]
    fn advance_current_clears_when_nothing_runs() {
        let prev = ("B".to_owned(), 10_000);
        assert_eq!(advance_current(Some(&prev), &steps(&[("B", C), ("C", P)]), 50_000), None);
        assert_eq!(advance_current(Some(&prev), &[], 50_000), None);
    }
}
