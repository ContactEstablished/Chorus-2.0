//! Error signatures (P15): what a failing check run reduces to, so the orchestrator can tell "the
//! same failure again" (a fix loop that isn't converging) from progress. A signature is the check's
//! name, the failing test names it can find and the first error line, with run-to-run noise
//! (durations, line and column numbers, addresses, colour codes) taken out.

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Signature {
    pub check: String,
    /// Failing test names, sorted, at most [`MAX_TESTS`].
    pub tests: Vec<String>,
    pub first_error: Option<String>,
}

pub const MAX_TESTS: usize = 10;
const MAX_LINE: usize = 200;

impl Signature {
    /// One string that is equal exactly when two signatures are.
    pub fn key(&self) -> String {
        format!("{}|{}|{}", self.check, self.tests.join(","), self.first_error.as_deref().unwrap_or(""))
    }

    /// A short line for a STOPPED message or a gate report.
    pub fn summary(&self) -> String {
        match (self.tests.as_slice(), &self.first_error) {
            ([], Some(e)) => format!("{}: {e}", self.check),
            ([], None) => format!("{}: failed with no error line", self.check),
            ([one], _) => format!("{}: {one} failed", self.check),
            (many, _) => format!("{}: {} and {} more failed", self.check, many[0], many.len() - 1),
        }
    }
}

/// The signature of one failing check from its output.
pub fn of(check: &str, output: &str) -> Signature {
    let text = strip_escapes(output);
    let mut tests: Vec<String> = text.lines().filter_map(failing_test).map(|t| normalise(&t)).collect();
    tests.sort();
    tests.dedup();
    tests.truncate(MAX_TESTS);
    let first_error = text.lines().find(|l| is_error_line(l)).map(normalise).filter(|l| !l.is_empty());
    Signature { check: check.to_owned(), tests, first_error }
}

/// Removes CSI (`ESC [ … final`) and OSC (`ESC ] … BEL` or `ESC \`) sequences.
pub fn strip_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for d in chars.by_ref() {
                    if ('@'..='~').contains(&d) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(d) = chars.next() {
                    if d == '\u{7}' || (d == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// A failing test's name from one output line, for the test runners the loop is likely to meet:
/// Vitest and Jest, `dotnet test`, `cargo test`, pytest, Playwright and Go.
fn failing_test(line: &str) -> Option<String> {
    let l = line.trim();
    let after = |prefix: &str| l.strip_prefix(prefix).map(str::trim).filter(|s| !s.is_empty());
    // Vitest: "FAIL  tests/a.test.ts > suite > name", "× suite > name 12ms"; Jest: "● suite › name".
    if let Some(rest) = after("FAIL ").or_else(|| after("×")).or_else(|| after("✗")).or_else(|| after("✕")) {
        return (rest.contains('>') || rest.contains('›') || rest.contains("::")).then(|| rest.to_owned());
    }
    if let Some(rest) = after("●") {
        return rest.contains('›').then(|| rest.to_owned());
    }
    // dotnet test: "Failed Namespace.Class.Method [12 ms]".
    if let Some(rest) = after("Failed ") {
        let name = rest.split(" [").next().unwrap_or(rest).trim();
        return (!name.contains(' ') && name.contains('.')).then(|| name.to_owned());
    }
    // cargo test: "test module::name ... FAILED".
    if let Some(rest) = after("test ")
        && let Some(name) = rest.strip_suffix("... FAILED").or_else(|| rest.strip_suffix("FAILED"))
    {
        return Some(name.trim().to_owned());
    }
    // pytest: "FAILED tests/test_x.py::test_name - AssertionError".
    if let Some(rest) = after("FAILED ") {
        return Some(rest.split(" - ").next().unwrap_or(rest).trim().to_owned());
    }
    // Go: "--- FAIL: TestName (0.00s)".
    if let Some(rest) = after("--- FAIL:") {
        return Some(rest.split(" (").next().unwrap_or(rest).trim().to_owned());
    }
    // Playwright: "1) [chromium] › tests/smoke.spec.ts:3:1 › home page loads".
    if let Some((n, rest)) = l.split_once(") [")
        && n.chars().all(|c| c.is_ascii_digit())
        && rest.contains('›')
    {
        return Some(format!("[{rest}"));
    }
    None
}

fn is_error_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    let mentions = lower.contains("error") || lower.contains("panicked at") || lower.contains("exception");
    let all_clear = ["0 error", "errors: 0", "no error", "0 failed", "error(s): 0"].iter().any(|ok| lower.contains(ok));
    mentions && !all_clear
}

/// One line with the noise that changes between identical failures taken out.
fn normalise(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line.trim();
    while let Some(c) = rest.chars().next() {
        // Durations and counts in brackets or parentheses: "(12ms)", "[12 ms]", "(0.00s)".
        if (c == '(' || c == '[')
            && let Some(close) = rest.find([')', ']'])
            && is_duration(&rest[1..close])
        {
            rest = &rest[close + 1..];
            continue;
        }
        // Hex addresses.
        if rest.starts_with("0x") && rest[2..].chars().next().is_some_and(|d| d.is_ascii_hexdigit()) {
            out.push_str("0x…");
            rest = rest[2..].trim_start_matches(|d: char| d.is_ascii_hexdigit());
            continue;
        }
        // Line and column numbers: ":12:5", ":12", "(12,5)".
        if (c == ':' || c == ',' || c == '(') && rest[1..].chars().next().is_some_and(|d| d.is_ascii_digit()) {
            let digits = rest[1..].chars().take_while(char::is_ascii_digit).count();
            let next = rest[1 + digits..].chars().next();
            if matches!(next, None | Some(':' | ',' | ')' | ' ')) {
                out.push(c);
                out.push('N');
                rest = &rest[1 + digits..];
                continue;
            }
        }
        // A trailing bare duration: "12ms", "1.2s".
        if c.is_ascii_digit() && out.ends_with(' ') && is_duration(rest.trim_end()) {
            break;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    out.chars().take(MAX_LINE).collect()
}

fn is_duration(s: &str) -> bool {
    let s = s.trim();
    let num = s.trim_end_matches(|c: char| c.is_ascii_alphabetic()).trim_end();
    let unit = s[num.len()..].trim();
    !num.is_empty() && num.chars().all(|c| c.is_ascii_digit() || c == '.') && ["ms", "s", "m", "µs", "us", "ns", "sec", "secs"].contains(&unit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vitest_failures_and_the_first_error() {
        let out = "\u{1b}[31m FAIL \u{1b}[39m tests/sum.test.ts > sum > adds two numbers\nAssertionError: expected 3 to be 4 // Object.is equality\n ❯ tests/sum.test.ts:5:17\n\n × sum > adds two numbers 12ms\n Test Files  1 failed (1)\n";
        let s = of("tests", out);
        assert_eq!(s.tests, ["sum > adds two numbers", "tests/sum.test.ts > sum > adds two numbers"]);
        assert_eq!(s.first_error.as_deref(), Some("AssertionError: expected 3 to be 4 // Object.is equality"));
        assert_eq!(s.summary(), "tests: sum > adds two numbers and 1 more failed");
    }

    #[test]
    fn the_same_failure_twice_has_the_same_key() {
        let a = "src/app.ts(12,5): error TS2322: Type 'string' is not assignable to type 'number'.\nFound 1 error in 812ms\n";
        let b = "src/app.ts(40,9): error TS2322: Type 'string' is not assignable to type 'number'.\nFound 1 error in 1.4s\n";
        assert_eq!(of("build", a).key(), of("build", b).key());
        let c = "src/app.ts(40,9): error TS2345: Argument of type 'x' is not assignable.\n";
        assert_ne!(of("build", a).key(), of("build", c).key());
    }

    #[test]
    fn other_runners() {
        let dotnet = "  Failed Shop.Tests.OrdersTests.Returns404 [23 ms]\n  Error Message:\n   Assert.Equal() Failure\n";
        assert_eq!(of("dotnet test", dotnet).tests, ["Shop.Tests.OrdersTests.Returns404"]);
        let cargo = "test parse::tests::rejects_empty ... FAILED\nthread 'parse::tests::rejects_empty' panicked at src/parse.rs:40:9:\n";
        let s = of("cargo test", cargo);
        assert_eq!(s.tests, ["parse::tests::rejects_empty"]);
        assert_eq!(s.first_error.as_deref(), Some("thread 'parse::tests::rejects_empty' panicked at src/parse.rs:N:N:"));
        assert_eq!(of("pytest", "FAILED tests/test_a.py::test_b - AssertionError: x\n").tests, ["tests/test_a.py::test_b"]);
        assert_eq!(of("go", "--- FAIL: TestOrders (0.00s)\n").tests, ["TestOrders"]);
        assert_eq!(of("e2e", "  1) [chromium] › tests/smoke.spec.ts:3:1 › home page loads ──\n").tests, ["[chromium] › tests/smoke.spec.ts:N:N › home page loads ──"]);
        assert!(of("jest", "  ● orders › pages by cursor\n").tests.len() == 1);
    }

    #[test]
    fn all_clear_lines_are_not_errors() {
        let s = of("lint", "0 errors, 2 warnings\nBuild succeeded. Errors: 0\nsomething else broke\n");
        assert_eq!(s.first_error, None);
        assert_eq!(s.summary(), "lint: failed with no error line");
    }

    #[test]
    fn noise_is_taken_out() {
        assert_eq!(normalise("at Object.<anonymous> (src/a.ts:12:5) [12 ms]"), "at Object.<anonymous> (src/a.ts:N:N)");
        assert_eq!(normalise("segfault at 0x7ffd5e8a reading"), "segfault at 0x… reading");
        assert_eq!(normalise("done in 1.2s"), "done in");
        assert_eq!(normalise("error TS2322: bad"), "error TS2322: bad");
        assert_eq!(normalise(&"x".repeat(500)).len(), MAX_LINE);
    }

    #[test]
    fn escapes_are_stripped() {
        assert_eq!(strip_escapes("\u{1b}[1;31mred\u{1b}[0m \u{1b}]0;title\u{7}ok \u{1b}]8;;http://x\u{1b}\\link"), "red ok link");
    }
}
