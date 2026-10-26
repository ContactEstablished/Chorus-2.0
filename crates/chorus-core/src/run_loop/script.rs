//! The scripted worker's script (P15 testing): a test double that stands in for a model step by
//! writing planned files, so the orchestrator, gatekeeper, git work and stop conditions can be run
//! deterministically and for free. Debug builds only; a run names its script file when it is made.
//!
//! ```json
//! {"steps": [
//!   {"phase": 1, "step": "plan", "write": {".chorus/run/phases/01-sum/tasks.md": "## T1 · Sum\n…"}},
//!   {"phase": 1, "step": "build", "task": "T1", "attempt": 0, "write": {"src/sum.ts": "…"}},
//!   {"phase": 1, "step": "build", "task": "T1", "attempt": 1, "fail": "the agent exited"}
//! ]}
//! ```

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Script {
    pub steps: Vec<ScriptStep>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepName {
    Plan,
    Build,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptStep {
    pub phase: u32,
    pub step: StepName,
    #[serde(default)]
    pub task: Option<String>,
    /// The plan retry or build attempt this answers; 0 when left out.
    #[serde(default)]
    pub attempt: u32,
    /// Files to write, relative to the repository root.
    #[serde(default)]
    pub write: BTreeMap<String, String>,
    /// Files to delete, relative to the repository root.
    #[serde(default)]
    pub delete: Vec<String>,
    /// Make the step fail as if its agent had, with this reason.
    #[serde(default)]
    pub fail: Option<String>,
    /// Ask a question, as the agent's `ask` tool would, after writing `write`.
    #[serde(default)]
    pub ask: Option<serde_json::Value>,
    /// Files to write once the question is answered; `{answer}` in a text is the answer.
    #[serde(default)]
    pub after_answer: BTreeMap<String, String>,
}

const MAX_STEPS: usize = 500;

/// Reads and checks a script: every path relative, inside the repository and outside `.git`.
pub fn parse(text: &str) -> Result<Script, String> {
    let script: Script = serde_json::from_str(text).map_err(|e| format!("The script isn't valid: {e}."))?;
    if script.steps.len() > MAX_STEPS {
        return Err(format!("A script has up to {MAX_STEPS} steps."));
    }
    for s in &script.steps {
        for p in s.write.keys().chain(s.delete.iter()).chain(s.after_answer.keys()) {
            safe_path(p)?;
        }
        if let Some(a) = &s.ask {
            crate::run_loop::tools::parse_ask(a)?;
        }
        if s.step == StepName::Build && s.task.is_none() {
            return Err(format!("Phase {}'s build step needs a task.", s.phase));
        }
    }
    Ok(script)
}

/// A repository-relative path a script may touch: no drive, no leading slash, no `..`, not in `.git`.
pub fn safe_path(p: &str) -> Result<(), String> {
    let parts: Vec<&str> = p.split(['/', '\\']).collect();
    let bad = p.is_empty()
        || p.starts_with(['/', '\\'])
        || p.contains(':')
        || parts.iter().any(|x| *x == ".." || x.is_empty())
        || parts.first().is_some_and(|x| x.eq_ignore_ascii_case(".git"));
    if bad { Err(format!("The script path {p:?} must be relative, inside the repository and outside .git.")) } else { Ok(()) }
}

impl Script {
    /// The step that answers a plan (`task` `None`) or a task's build attempt.
    pub fn find(&self, phase: u32, step: StepName, task: Option<&str>, attempt: u32) -> Option<&ScriptStep> {
        self.steps.iter().find(|s| {
            s.phase == phase && s.step == step && s.attempt == attempt && s.task.as_deref().map(str::to_ascii_uppercase) == task.map(str::to_ascii_uppercase)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_are_found_by_phase_step_task_and_attempt() {
        let s = parse(r#"{"steps":[{"phase":1,"step":"plan","write":{"a.md":"x"}},{"phase":1,"step":"build","task":"T1","attempt":1,"fail":"gone"}]}"#).unwrap();
        assert_eq!(s.find(1, StepName::Plan, None, 0).unwrap().write["a.md"], "x");
        assert_eq!(s.find(1, StepName::Build, Some("t1"), 1).unwrap().fail.as_deref(), Some("gone"));
        assert!(s.find(1, StepName::Build, Some("T1"), 0).is_none());
        assert!(s.find(2, StepName::Plan, None, 0).is_none());
    }

    #[test]
    fn unsafe_paths_and_bad_scripts_are_refused() {
        for p in ["../x", "C:/x", "/etc/x", ".git/config", "a//b", "a\\..\\b", ""] {
            assert!(safe_path(p).is_err(), "{p}");
        }
        for p in ["src/a.ts", ".chorus/run/phases/01-x/tasks.md", ".gitignore"] {
            assert!(safe_path(p).is_ok(), "{p}");
        }
        assert!(parse(r#"{"steps":[{"phase":1,"step":"build"}]}"#).unwrap_err().contains("needs a task"));
        assert!(parse(r#"{"steps":[{"phase":1,"step":"plan","write":{"../a":"x"}}]}"#).is_err());
        assert!(parse("nope").unwrap_err().starts_with("The script isn't valid"));
    }
}
