//! The run documents agents write and the orchestrator reads (P15): `roadmap.md`'s phase blocks
//! and `tasks.md`'s task blocks. Both are Markdown that a person can read and edit: a `## ` heading
//! naming the item, then `- Key: value` lines. Anything else in the file is kept as it is.
//!
//! ```markdown
//! ## P01 · Order history
//! - Goal: customers can page through their orders
//! - Risk: high
//! - Models: builder=sol
//!
//! ## T2 · Order history endpoint
//! - Depends on: T1
//! - Claims: src/Api/Orders/**, tests/Api.Tests/Orders/**
//! - Accept: GET /orders?cursor= returns 20 items, newest first
//! - Status: pending
//! ```

use serde::{Deserialize, Serialize};
use specta::Type;

/// One mistake in a run document, with the 1-based line it is on (0 for the file as a whole).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub line: u32,
    pub message: String,
}

impl Problem {
    fn at(line: u32, message: impl Into<String>) -> Self {
        Self { line, message: message.into() }
    }
}

/// Every problem, one per line, for a prompt that sends a document back to the agent that wrote it.
pub fn describe(file: &str, problems: &[Problem]) -> String {
    problems
        .iter()
        .map(|p| if p.line == 0 { format!("{file}: {}", p.message) } else { format!("{file} line {}: {}", p.line, p.message) })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Risk {
    /// A task a fallback model may take over when the default's allowance runs out.
    Low,
    Normal,
    /// A phase that always ends a milestone and gets Counsel's review (schema, auth, payments…).
    High,
}

impl Risk {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "low" => Some(Self::Low),
            "normal" | "" => Some(Self::Normal),
            "high" => Some(Self::High),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum TaskStatus {
    Pending,
    Running,
    Done,
    Failed,
    /// Waiting on a blocking question.
    Blocked,
    Stopped,
}

impl TaskStatus {
    pub const ALL: [Self; 6] = [Self::Pending, Self::Running, Self::Done, Self::Failed, Self::Blocked, Self::Stopped];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Blocked => "blocked",
            Self::Stopped => "stopped",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|t| t.as_str() == s)
    }
}

/// A phase as `roadmap.md` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PhaseEntry {
    pub number: u32,
    pub title: String,
    pub goal: String,
    pub risk: Risk,
    /// Role overrides for this phase, `role=model alias` pairs, in the order written.
    pub models: Vec<(String, String)>,
}

impl PhaseEntry {
    /// `P03`.
    pub fn code(&self) -> String {
        format!("P{:02}", self.number)
    }

    /// The phase's folder under `phases/`: `03-order-history`.
    pub fn folder(&self) -> String {
        format!("{:02}-{}", self.number, slug(&self.title))
    }

    /// The tag a passing phase gets: `phase-03`.
    pub fn tag(&self) -> String {
        format!("phase-{:02}", self.number)
    }
}

/// A task as `tasks.md` lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskEntry {
    /// `T2`.
    pub id: String,
    pub title: String,
    pub depends_on: Vec<String>,
    /// Path globs the task may change, relative to the repository root.
    pub claims: Vec<String>,
    /// Acceptance criteria, each a statement a test can prove.
    pub accept: Vec<String>,
    /// A model alias that overrides the builder role for this task.
    pub model: Option<String>,
    pub risk: Risk,
    pub status: TaskStatus,
    /// The 1-based line of the task's heading.
    pub line: u32,
}

/// A lower-case, hyphenated name for folders and branches: letters and digits only, up to 40
/// characters, never empty.
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let mut out: String = out.trim_end_matches('-').chars().take(40).collect();
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() { "phase".into() } else { out }
}

/// A `## ` heading of the form `<prefix><digits> <separator> <title>`, for `prefix` `P` or `T`.
/// The separator may be `·`, `-`, `–`, `—` or `:`.
fn heading(line: &str, prefix: char) -> Option<(String, String)> {
    let rest = line.strip_prefix("## ")?.trim();
    let mut chars = rest.chars();
    if !chars.next()?.eq_ignore_ascii_case(&prefix) {
        return None;
    }
    let after = chars.as_str();
    let digits: String = after.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let title = after[digits.len()..].trim_start();
    let title = title.strip_prefix(['·', '-', '–', '—', ':']).unwrap_or(title).trim();
    Some((format!("{prefix}{digits}"), title.to_owned()))
}

/// `- Key: value` (or `* Key: value`), key compared case-insensitively.
fn bullet(line: &str) -> Option<(String, String)> {
    let rest = line.trim_start().strip_prefix("- ").or_else(|| line.trim_start().strip_prefix("* "))?;
    let (key, value) = rest.split_once(':')?;
    let key = key.trim().to_ascii_lowercase();
    if key.is_empty() || key.len() > 24 {
        return None;
    }
    Some((key, value.trim().to_owned()))
}

fn list(value: &str) -> Vec<String> {
    value.split(',').map(str::trim).filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("none")).map(str::to_owned).collect()
}

/// Splits a document into its `## ` blocks: the heading line's index and the lines up to the next
/// `#` heading of any level.
fn blocks(text: &str) -> Vec<(usize, Vec<(usize, &str)>)> {
    let mut out: Vec<(usize, Vec<(usize, &str)>)> = Vec::new();
    let mut open = false;
    for (i, line) in text.lines().enumerate() {
        if line.starts_with("## ") {
            out.push((i, Vec::new()));
            open = true;
        } else if line.starts_with('#') {
            open = false;
        } else if open && let Some(last) = out.last_mut() {
            last.1.push((i, line));
        }
    }
    out
}

const MAX_ITEMS: usize = 60;

/// The phases in `roadmap.md`, in order, or every problem found.
pub fn parse_roadmap(text: &str) -> Result<Vec<PhaseEntry>, Vec<Problem>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut phases = Vec::new();
    let mut problems = Vec::new();
    for (at, body) in blocks(text) {
        let Some((code, title)) = heading(lines[at], 'P') else { continue };
        let line = at as u32 + 1;
        let number: u32 = code[1..].parse().unwrap_or(0);
        if number == 0 || number > 99 {
            problems.push(Problem::at(line, format!("{code}: phases are numbered P01 to P99.")));
            continue;
        }
        if title.is_empty() {
            problems.push(Problem::at(line, format!("{code} has no title.")));
        }
        let mut phase = PhaseEntry { number, title, goal: String::new(), risk: Risk::Normal, models: Vec::new() };
        for (i, l) in body {
            let Some((key, value)) = bullet(l) else { continue };
            match key.as_str() {
                "goal" => phase.goal = value,
                "risk" => match Risk::parse(&value) {
                    Some(Risk::Low) | None => problems.push(Problem::at(i as u32 + 1, format!("{code}: Risk is normal or high."))),
                    Some(r) => phase.risk = r,
                },
                "models" => match models(&value) {
                    Ok(m) => phase.models = m,
                    Err(e) => problems.push(Problem::at(i as u32 + 1, format!("{code}: {e}"))),
                },
                _ => {}
            }
        }
        if phases.iter().any(|p: &PhaseEntry| p.number == number) {
            problems.push(Problem::at(line, format!("{code} is listed twice.")));
        }
        phases.push(phase);
    }
    if phases.is_empty() && problems.is_empty() {
        problems.push(Problem::at(0, "no phases: each starts with a heading like \"## P01 · Project skeleton\"."));
    }
    if phases.len() > MAX_ITEMS {
        problems.push(Problem::at(0, format!("a roadmap holds up to {MAX_ITEMS} phases.")));
    }
    if problems.is_empty() { Ok(phases) } else { Err(problems) }
}

/// `builder=sol, reviewer=claude` as pairs.
fn models(value: &str) -> Result<Vec<(String, String)>, String> {
    let value = value.trim().trim_start_matches('{').trim_end_matches('}');
    list(value)
        .into_iter()
        .map(|pair| {
            let (role, model) = pair.split_once(['=', ':']).ok_or_else(|| format!("write Models as role=model pairs, not {pair:?}."))?;
            let (role, model) = (role.trim().to_ascii_lowercase(), model.trim().to_owned());
            if role.is_empty() || model.is_empty() {
                return Err(format!("write Models as role=model pairs, not {pair:?}."));
            }
            Ok((role, model))
        })
        .collect()
}

/// The tasks in `tasks.md`, in order, or every problem found: no tasks, a repeated id, a missing
/// claim or acceptance criterion, an unknown dependency or a cycle.
pub fn parse_tasks(text: &str) -> Result<Vec<TaskEntry>, Vec<Problem>> {
    let lines: Vec<&str> = text.lines().collect();
    let mut tasks: Vec<TaskEntry> = Vec::new();
    let mut problems = Vec::new();
    for (at, body) in blocks(text) {
        let Some((id, title)) = heading(lines[at], 'T') else { continue };
        let line = at as u32 + 1;
        if title.is_empty() {
            problems.push(Problem::at(line, format!("{id} has no title.")));
        }
        let mut task = TaskEntry {
            id: id.clone(),
            title,
            depends_on: Vec::new(),
            claims: Vec::new(),
            accept: Vec::new(),
            model: None,
            risk: Risk::Normal,
            status: TaskStatus::Pending,
            line,
        };
        for (i, l) in body {
            let Some((key, value)) = bullet(l) else { continue };
            let here = i as u32 + 1;
            match key.as_str() {
                "depends on" | "depends" => task.depends_on = list(&value).into_iter().map(|d| d.to_ascii_uppercase()).collect(),
                "claims" => task.claims.extend(list(&value)),
                "accept" if !value.is_empty() => task.accept.push(value),
                "model" if !value.is_empty() => task.model = Some(value),
                "risk" => match Risk::parse(&value) {
                    Some(Risk::High) | None => problems.push(Problem::at(here, format!("{id}: Risk is low or normal."))),
                    Some(r) => task.risk = r,
                },
                "status" => match TaskStatus::parse(&value) {
                    Some(s) => task.status = s,
                    None => problems.push(Problem::at(here, format!("{id}: Status is one of pending, running, done, failed, blocked or stopped."))),
                },
                _ => {}
            }
        }
        if task.claims.is_empty() {
            problems.push(Problem::at(line, format!("{id} claims no files: add \"- Claims: <paths or globs>\".")));
        }
        if task.accept.is_empty() {
            problems.push(Problem::at(line, format!("{id} has no acceptance criterion: add \"- Accept: <a statement a test can prove>\".")));
        }
        if tasks.iter().any(|t| t.id == task.id) {
            problems.push(Problem::at(line, format!("{id} is listed twice.")));
        }
        tasks.push(task);
    }
    if tasks.is_empty() && problems.is_empty() {
        problems.push(Problem::at(0, "no tasks: each starts with a heading like \"## T1 · Order history endpoint\"."));
    }
    if tasks.len() > MAX_ITEMS {
        problems.push(Problem::at(0, format!("a phase holds up to {MAX_ITEMS} tasks.")));
    }
    for t in &tasks {
        for d in &t.depends_on {
            if *d == t.id {
                problems.push(Problem::at(t.line, format!("{} depends on itself.", t.id)));
            } else if !tasks.iter().any(|o| o.id == *d) {
                problems.push(Problem::at(t.line, format!("{} depends on {d}, which isn't in this file.", t.id)));
            }
        }
    }
    if problems.is_empty()
        && let Some(id) = cycle(&tasks)
    {
        problems.push(Problem::at(0, format!("the dependencies of {id} go round in a circle.")));
    }
    if problems.is_empty() { Ok(tasks) } else { Err(problems) }
}

/// A task that is part of a dependency cycle, if there is one.
fn cycle(tasks: &[TaskEntry]) -> Option<String> {
    // Repeatedly drop tasks whose dependencies are all dropped; whatever is left is in a cycle.
    let mut left: Vec<&TaskEntry> = tasks.iter().collect();
    loop {
        let before = left.len();
        let ids: Vec<String> = left.iter().map(|t| t.id.clone()).collect();
        left.retain(|t| t.depends_on.iter().any(|d| ids.contains(d)));
        if left.is_empty() {
            return None;
        }
        if left.len() == before {
            return left.first().map(|t| t.id.clone());
        }
    }
}

/// `text` with task `id`'s Status set to `status`, everything else kept byte for byte. A task with
/// no Status line gets one after its last `- ` line. `None` if the task isn't there.
pub fn set_task_status(text: &str, id: &str, status: TaskStatus) -> Option<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let plain: Vec<&str> = lines.iter().map(|l| l.trim_end_matches(['\n', '\r'])).collect();
    let (at, body) = blocks(text).into_iter().find(|(at, _)| heading(plain[*at], 'T').is_some_and(|(t, _)| t.eq_ignore_ascii_case(id)))?;
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let new_line = format!("- Status: {}", status.as_str());
    let mut out: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
    if let Some((i, _)) = body.iter().find(|(_, l)| bullet(l).is_some_and(|(k, _)| k == "status")) {
        let end = if lines[*i].ends_with('\n') { eol } else { "" };
        out[*i] = format!("{new_line}{end}");
    } else {
        let after = body.iter().rev().find(|(_, l)| bullet(l).is_some()).map_or(at, |(i, _)| *i);
        if !out[after].ends_with('\n') {
            out[after].push_str(eol);
        }
        out.insert(after + 1, format!("{new_line}{eol}"));
    }
    Some(out.concat())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TASKS: &str = "# Tasks: order history\n\nIntro text kept.\n\n## T1 · Orders table\n- Claims: src/Data/**\n- Accept: the migration adds an Orders table\n\n## T2 · Order history endpoint\n- Depends on: T1\n- Claims: src/Api/Orders/**, tests/Api.Tests/Orders/**\n- Accept: GET /orders?cursor= returns 20 items, newest first\n- Accept: an unknown customer returns 404 ProblemDetails\n- Model: deepseek-flash\n- Risk: low\n- Status: running\n";

    #[test]
    fn tasks_parse_with_their_fields() {
        let t = parse_tasks(TASKS).unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!((t[0].id.as_str(), t[0].title.as_str(), t[0].status, t[0].line), ("T1", "Orders table", TaskStatus::Pending, 5));
        assert_eq!(t[1].depends_on, ["T1"]);
        assert_eq!(t[1].claims, ["src/Api/Orders/**", "tests/Api.Tests/Orders/**"]);
        assert_eq!(t[1].accept.len(), 2);
        assert_eq!(t[1].model.as_deref(), Some("deepseek-flash"));
        assert_eq!((t[1].risk, t[1].status), (Risk::Low, TaskStatus::Running));
    }

    #[test]
    fn headings_accept_several_separators() {
        for h in ["## T3 · Title", "## T3 - Title", "## T3: Title", "## T3 — Title", "## t3 Title"] {
            assert_eq!(heading(h, 'T'), Some(("T3".into(), "Title".into())), "{h}");
        }
        assert_eq!(heading("## Tasks", 'T'), None);
        assert_eq!(heading("### T3 · Title", 'T'), None);
    }

    #[test]
    fn task_problems_name_the_line() {
        let bad = "## T1 · A\n- Accept: x\n\n## T1 · B\n- Claims: a\n- Accept: y\n- Depends on: T9\n- Status: maybe\n";
        let p = parse_tasks(bad).unwrap_err();
        let text = describe("tasks.md", &p);
        assert!(text.contains("tasks.md line 1: T1 claims no files"), "{text}");
        assert!(text.contains("line 8: T1: Status is one of"), "{text}");
        assert!(text.contains("line 4: T1 is listed twice"), "{text}");
        assert!(text.contains("depends on T9, which isn't in this file"), "{text}");
    }

    #[test]
    fn cycles_and_empty_files_are_problems() {
        let c = "## T1 · A\n- Claims: a\n- Accept: x\n- Depends on: T2\n## T2 · B\n- Claims: b\n- Accept: y\n- Depends on: T1\n";
        assert!(describe("tasks.md", &parse_tasks(c).unwrap_err()).contains("go round in a circle"));
        assert!(describe("tasks.md", &parse_tasks("# Nothing here\n").unwrap_err()).contains("no tasks"));
        let self_dep = "## T1 · A\n- Claims: a\n- Accept: x\n- Depends on: t1\n";
        assert!(describe("tasks.md", &parse_tasks(self_dep).unwrap_err()).contains("T1 depends on itself"));
    }

    #[test]
    fn a_later_heading_of_another_level_ends_a_block() {
        let t = "## T1 · A\n- Claims: a\n- Accept: x\n# Notes\n- Status: done\n";
        assert_eq!(parse_tasks(t).unwrap()[0].status, TaskStatus::Pending);
    }

    #[test]
    fn status_is_replaced_or_added_and_nothing_else_changes() {
        let done = set_task_status(TASKS, "t2", TaskStatus::Done).unwrap();
        assert_eq!(done, TASKS.replace("- Status: running", "- Status: done"));
        let added = set_task_status(TASKS, "T1", TaskStatus::Running).unwrap();
        assert!(added.contains("- Accept: the migration adds an Orders table\n- Status: running\n\n## T2"), "{added}");
        assert_eq!(parse_tasks(&added).unwrap()[0].status, TaskStatus::Running);
        assert_eq!(set_task_status(TASKS, "T7", TaskStatus::Done), None);
        let crlf = TASKS.replace('\n', "\r\n");
        let out = set_task_status(&crlf, "T1", TaskStatus::Done).unwrap();
        assert!(!out.replace("\r\n", "").contains('\n'), "kept CRLF");
    }

    #[test]
    fn a_status_at_the_very_end_without_a_newline() {
        let t = "## T1 · A\n- Claims: a\n- Accept: x";
        assert_eq!(set_task_status(t, "T1", TaskStatus::Done).unwrap(), "## T1 · A\n- Claims: a\n- Accept: x\n- Status: done\n");
    }

    #[test]
    fn roadmap_phases_parse() {
        let r = "# Roadmap\n\n## P01 · Project skeleton\n- Goal: a CLI that prints its version\n\n## P02 · Order history!\n- Goal: paging\n- Risk: high\n- Models: { builder: sol, reviewer=claude }\n";
        let p = parse_roadmap(r).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].code(), p[0].folder(), p[0].tag()), ("P01".into(), "01-project-skeleton".into(), "phase-01".into()));
        assert_eq!(p[1].risk, Risk::High);
        assert_eq!(p[1].models, [("builder".into(), "sol".into()), ("reviewer".into(), "claude".into())]);
        assert_eq!(p[1].folder(), "02-order-history");
    }

    #[test]
    fn roadmap_problems() {
        let p = parse_roadmap("## P00 · Zero\n## P01 · A\n- Risk: low\n- Models: builder\n## P01 · B\n").unwrap_err();
        let text = describe("roadmap.md", &p);
        assert!(text.contains("numbered P01 to P99"), "{text}");
        assert!(text.contains("line 3: P01: Risk is normal or high"), "{text}");
        assert!(text.contains("role=model pairs"), "{text}");
        assert!(text.contains("P01 is listed twice"), "{text}");
        assert!(describe("roadmap.md", &parse_roadmap("text only").unwrap_err()).contains("no phases"));
    }

    #[test]
    fn slugs_are_short_and_never_empty() {
        assert_eq!(slug("Order history: v2 (API)"), "order-history-v2-api");
        assert_eq!(slug("!!!"), "phase");
        assert!(slug(&"long ".repeat(20)).len() <= 40);
        assert!(!slug(&"ab ".repeat(30)).ends_with('-'));
    }
}
