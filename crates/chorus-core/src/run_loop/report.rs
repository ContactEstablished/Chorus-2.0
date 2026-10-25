//! What the gatekeeper writes (P15): a phase's `gate-report.md`, newest attempt first, with each
//! failing check's command, exit and the tail of its output. The output is scrubbed before it gets
//! here; this module only shapes it.

use serde::{Deserialize, Serialize};
use specta::Type;

use super::signature::Signature;

/// One check as the gatekeeper ran it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CheckRun {
    pub name: String,
    pub command: String,
    pub passed: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u64,
    /// The end of its output, scrubbed.
    pub output_tail: String,
    pub signature: Option<Signature>,
}

/// Lines of output kept for a failing check in the report.
pub const TAIL_LINES: usize = 60;
/// Bytes of output kept for a failing check in the report.
pub const TAIL_BYTES: usize = 6 * 1024;

/// The last [`TAIL_LINES`] lines and at most [`TAIL_BYTES`] bytes of `output`, cut on a character
/// boundary.
pub fn tail(output: &str) -> String {
    let lines: Vec<&str> = output.trim_end().lines().collect();
    let kept = lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n");
    if kept.len() <= TAIL_BYTES {
        return kept;
    }
    let mut start = kept.len() - TAIL_BYTES;
    while !kept.is_char_boundary(start) {
        start += 1;
    }
    kept[start..].to_owned()
}

fn seconds(ms: u64) -> String {
    format!("{:.1} s", ms as f64 / 1000.0)
}

/// A fence longer than any run of backticks in `text`, so output can't close it early.
fn fence(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    "`".repeat((longest + 1).max(3))
}

/// One attempt's section of the report.
pub fn attempt_section(phase_code: &str, task: &str, attempt: u32, checks: &[CheckRun]) -> String {
    let failed = checks.iter().filter(|c| !c.passed).count();
    let what = if attempt == 0 { "first build".to_owned() } else { format!("fix attempt {attempt}") };
    let verdict = if failed == 0 { "every check passed".to_owned() } else { format!("{failed} of {} checks failed", checks.len()) };
    let mut s = format!("## {phase_code} {task}, {what}: {verdict}\n\n| Check | Result | Time |\n|---|---|---|\n");
    for c in checks {
        let result = match (c.passed, c.timed_out, c.exit_code) {
            (true, _, _) => "passed".to_owned(),
            (false, true, _) => "timed out".to_owned(),
            (false, false, Some(code)) => format!("failed (exit {code})"),
            (false, false, None) => "failed".to_owned(),
        };
        s.push_str(&format!("| {} | {result} | {} |\n", c.name.replace('|', "\\|"), seconds(c.duration_ms)));
    }
    for c in checks.iter().filter(|c| !c.passed) {
        s.push_str(&format!("\n### {}\n\nCommand: `{}`\n", c.name, c.command.replace('`', "'")));
        if let Some(sig) = &c.signature {
            s.push_str(&format!("\nFailure: {}\n", sig.summary()));
        }
        let body = if c.output_tail.trim().is_empty() { "(no output)".to_owned() } else { c.output_tail.clone() };
        let f = fence(&body);
        s.push_str(&format!("\nThe end of its output:\n\n{f}text\n{body}\n{f}\n"));
    }
    s
}

const TITLE: &str = "# Gate report\n\nThe gatekeeper's check results for this phase, newest attempt first.\n";

/// `existing` (a report, or empty) with `section` added at the top.
pub fn prepend(existing: &str, section: &str) -> String {
    let rest = existing.strip_prefix(TITLE).unwrap_or(existing).trim_start_matches('\n');
    if rest.trim().is_empty() { format!("{TITLE}\n{section}") } else { format!("{TITLE}\n{section}\n{rest}") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_loop::signature;

    fn check(name: &str, passed: bool, out: &str) -> CheckRun {
        CheckRun {
            name: name.into(),
            command: format!("npm run {name}"),
            passed,
            exit_code: Some(if passed { 0 } else { 1 }),
            timed_out: false,
            duration_ms: 1234,
            output_tail: out.into(),
            signature: (!passed).then(|| signature::of(name, out)),
        }
    }

    #[test]
    fn a_section_lists_every_check_and_details_failures() {
        let s = attempt_section("P01", "T1", 2, &[check("build", true, "ok"), check("test", false, "× sum > adds\n```\nAssertionError: no")]);
        assert!(s.starts_with("## P01 T1, fix attempt 2: 1 of 2 checks failed\n"), "{s}");
        assert!(s.contains("| build | passed | 1.2 s |"));
        assert!(s.contains("| test | failed (exit 1) | 1.2 s |"));
        assert!(s.contains("Failure: test: sum > adds failed"));
        assert!(s.contains("````text\n× sum > adds\n```\nAssertionError: no\n````"), "{s}");
        assert!(!s.contains("### build"));
    }

    #[test]
    fn newest_first() {
        let one = attempt_section("P01", "T1", 0, &[check("test", false, "x")]);
        let two = attempt_section("P01", "T1", 1, &[check("test", true, "")]);
        let r = prepend(&prepend("", &one), &two);
        assert!(r.starts_with(TITLE));
        assert!(r.find("fix attempt 1").unwrap() < r.find("first build").unwrap());
        assert_eq!(r.matches("# Gate report").count(), 1);
    }

    #[test]
    fn tails_are_bounded() {
        let long: String = (0..200).map(|i| format!("line {i}\n")).collect();
        let t = tail(&long);
        assert!(t.starts_with("line 140") && t.ends_with("line 199"));
        let wide = "é".repeat(5000);
        assert!(tail(&wide).len() <= TAIL_BYTES);
    }
}
