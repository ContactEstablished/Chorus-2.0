//! The first prompts the orchestrator gives each step's agent (P15). They point at files rather than
//! carry content, because the agent reads the repository itself: the run documents under
//! [`RUN_DIR`], the phase's spec and tasks, and the gate report after a failed check.

use super::files::PhaseEntry;

/// Where the run documents live in the repository, relative to its root.
pub const RUN_DIR: &str = ".chorus/run";

/// A check the gatekeeper runs, as a builder is told about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckLine {
    pub name: String,
    pub command: String,
}

/// `.chorus/run/phases/03-order-history`.
pub fn phase_dir(phase: &PhaseEntry) -> String {
    format!("{RUN_DIR}/phases/{}", phase.folder())
}

const TASK_FORMAT: &str = "## T1 · <short title>\n- Depends on: <task ids, or none>\n- Claims: <comma-separated paths or globs the task may change>\n- Accept: <one statement a test can prove>\n- Accept: <another, as many as needed>\n- Status: pending";

/// The phase planner's prompt. `problems` is what was wrong with its last `tasks.md`.
pub fn plan(phase: &PhaseEntry, problems: Option<&str>) -> String {
    let dir = phase_dir(phase);
    let mut p = format!(
        "You are the phase planner for phase {code} · {title} of this project's roadmap. Read {RUN_DIR}/roadmap.md, \
         {RUN_DIR}/decisions.md if it exists, and the code as it is now. Then write two files:\n\
         1. {dir}/spec.md: what this phase delivers, and acceptance criteria written as statements a test can prove.\n\
         2. {dir}/tasks.md: the tasks that build it, each in exactly this format:\n{TASK_FORMAT}\n\
         Each task claims every file it will change. Keep tasks small enough for one builder session each.\n\
         Don't write any code in this step and don't change any other file. End your turn when both files are written.",
        code = phase.code(),
        title = phase.title,
    );
    if let Some(problems) = problems {
        p.push_str(&format!("\nYour last tasks.md had these problems; fix them:\n{problems}"));
    }
    p
}

/// A builder's prompt for `task`: its first build (`attempt` 0) or a fix after failed checks.
pub fn build(phase: &PhaseEntry, task: &str, title: &str, attempt: u32, fix_attempts: u32, checks: &[CheckLine]) -> String {
    let dir = phase_dir(phase);
    let checks = checks.iter().map(|c| format!("- {}: {}", c.name, c.command)).collect::<Vec<_>>().join("\n");
    let rules = format!(
        "Rules: change only the files {task} claims in tasks.md. Never delete, skip or weaken a test to get the checks to \
         pass. Don't run git: Chorus commits your work. Log any choice you make on your own as one line in \
         {RUN_DIR}/assumptions.md. If a tool or command you need is refused, don't work around it: say what you needed \
         in your final message and end your turn.\n\
         These checks must pass, and you may run them yourself:\n{checks}"
    );
    if attempt == 0 {
        format!(
            "You are the builder for task {task} · {title} in phase {code} · {phase_title}. Read {RUN_DIR}/decisions.md if it \
             exists, {dir}/spec.md and your task in {dir}/tasks.md. Build the task so that each of its acceptance \
             criteria is proved by a test.\n{rules}\nEnd your turn when the task is done and the checks pass.",
            code = phase.code(),
            phase_title = phase.title,
        )
    } else {
        format!(
            "The checks failed after the last attempt at task {task} · {title} in phase {code}. This is fix attempt \
             {attempt} of {fix_attempts}. Read {dir}/gate-report.md for what failed, then fix the cause in the code.\n\
             {rules}\nEnd your turn when the checks pass.",
            code = phase.code(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_loop::files::Risk;

    fn phase() -> PhaseEntry {
        PhaseEntry { number: 3, title: "Order history".into(), goal: String::new(), risk: Risk::Normal, models: Vec::new() }
    }

    #[test]
    fn the_planner_is_pointed_at_its_files() {
        let p = plan(&phase(), None);
        assert!(p.contains("phase P03 · Order history"));
        assert!(p.contains(".chorus/run/phases/03-order-history/spec.md"));
        assert!(p.contains("- Claims: <comma-separated"));
        assert!(!p.contains("problems"));
        assert!(plan(&phase(), Some("tasks.md line 3: T1 claims no files")).ends_with("fix them:\ntasks.md line 3: T1 claims no files"));
    }

    #[test]
    fn builders_get_the_checks_and_the_rules() {
        let checks = [CheckLine { name: "tests".into(), command: "npm test".into() }];
        let b = build(&phase(), "T2", "Endpoint", 0, 3, &checks);
        assert!(b.starts_with("You are the builder for task T2 · Endpoint in phase P03 · Order history."));
        assert!(b.contains("- tests: npm test"));
        assert!(b.contains("Never delete, skip or weaken a test"));
        let f = build(&phase(), "T2", "Endpoint", 2, 3, &checks);
        assert!(f.contains("fix attempt 2 of 3") && f.contains("03-order-history/gate-report.md"), "{f}");
        // Every prompt is a sentence the launch check accepts.
        for p in [b, f, plan(&phase(), None)] {
            assert!(crate::actions::clean_prompt(&p).is_ok());
        }
    }
}
