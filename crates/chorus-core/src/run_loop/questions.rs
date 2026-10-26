//! Questions a loop step asks (P15 slice 3, the spec's "Escalation"): how an answer goes back to
//! the agent, what lands in `assumptions.md`, and how "make it a rule" grows `decisions.md` so the
//! same question isn't asked again.

/// `[P03 T2]`: the tag every line a step writes carries, so parallel tasks never blur together.
pub fn tag(phase: u32, task: Option<&str>) -> String {
    match task {
        Some(t) => format!("[P{phase:02} {t}]"),
        None => format!("[P{phase:02} plan]"),
    }
}

/// One line for `assumptions.md`, on one line whatever the agent wrote, and tagged once even when
/// the agent copied the tag into its text.
pub fn assumption_line(tag: &str, text: &str) -> String {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let one = one.strip_prefix(tag).map_or(one.as_str(), str::trim_start);
    format!("- {tag} {one}")
}

const ASSUMPTIONS_TITLE: &str = "# Assumptions\n\nChoices agents made on their own, newest last. Skim them; anything wrong becomes a rule in decisions.md.\n\n";

/// `existing` (the file, or empty) with `line` appended.
pub fn append_assumption(existing: &str, line: &str) -> String {
    let base = if existing.trim().is_empty() { ASSUMPTIONS_TITLE.to_owned() } else { existing.to_owned() };
    let sep = if base.ends_with('\n') { "" } else { "\n" };
    format!("{base}{sep}{line}\n")
}

pub const RULES_HEADING: &str = "## Rules from your answers";

/// `decisions.md` with a rule from one of Matthew's answers, under its own heading at the end, so
/// the agents' next read finds it and the question isn't asked again. The rule comes first; the
/// question it answered follows, shortened, for context.
pub fn append_rule(decisions: &str, question: &str, answer: &str, tag: &str) -> String {
    let one = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let asked = one(question);
    let asked = if asked.chars().count() > 200 { format!("{}…", asked.chars().take(200).collect::<String>().trim_end()) } else { asked };
    let line = format!("- {}\n  (Asked {tag}: {asked})", one(answer));
    let mut out = if decisions.trim().is_empty() { "# Decisions\n".to_owned() } else { decisions.to_owned() };
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.lines().any(|l| l.trim() == RULES_HEADING) {
        out.push_str(&format!("\n{RULES_HEADING}\nMatthew's answers to questions agents asked. Follow them like the rest of this file.\n\n"));
    }
    out.push_str(&line);
    out.push('\n');
    out
}

/// How an answer reached the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsweredBy {
    Matthew,
    /// Nobody answered a timed question in time.
    Default,
}

/// The prompt that gives an answer back to the agent that asked.
pub fn answer_prompt(question: &str, answer: &str, by: AnsweredBy, rule: bool, timed_minutes: u32) -> String {
    let mut p = match by {
        AnsweredBy::Matthew => format!("Matthew answered your question \"{question}\": {answer}"),
        AnsweredBy::Default => format!(
            "Nobody answered your question \"{question}\" within {timed_minutes} minutes, so go ahead with the default: {answer}. It is logged in assumptions.md."
        ),
    };
    if rule {
        p.push_str(" He made it a rule in decisions.md, so follow it from now on.");
    }
    p.push_str(" Carry on with the task, and end your turn when it is done.");
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_and_lines() {
        assert_eq!(tag(3, Some("T2")), "[P03 T2]");
        assert_eq!(tag(1, None), "[P01 plan]");
        assert_eq!(assumption_line("[P01 T1]", "used\n Number()  for parsing"), "- [P01 T1] used Number() for parsing");
        assert_eq!(assumption_line("[P01 T1]", "[P01 T1] sum is a plain function"), "- [P01 T1] sum is a plain function");
    }

    #[test]
    fn assumptions_start_a_file_or_append() {
        let first = append_assumption("", "- [P01 T1] a");
        assert!(first.starts_with("# Assumptions") && first.ends_with("- [P01 T1] a\n"));
        let next = append_assumption("old line", "- [P01 T1] b");
        assert_eq!(next, "old line\n- [P01 T1] b\n");
    }

    #[test]
    fn rules_get_their_own_section_once() {
        let d = "# Decisions: f1\n\n## Stack\n- Node\n";
        let one = append_rule(d, "Error wording?", "Use \"<command>: <problem>\"", "[P01 T2]");
        assert!(one.starts_with(d));
        assert!(one.contains("\n## Rules from your answers\n"));
        assert!(one.ends_with("- Use \"<command>: <problem>\"\n  (Asked [P01 T2]: Error wording?)\n"), "{one}");
        let two = append_rule(&one, "Exit codes", "1 on error", "[P02 T1]");
        assert_eq!(two.matches(RULES_HEADING).count(), 1);
        assert!(two.ends_with("- 1 on error\n  (Asked [P02 T1]: Exit codes)\n"));
        assert!(append_rule("", "q", "a", "[P01 plan]").starts_with("# Decisions\n"));
        let long = append_rule("", &format!("What wording?\n{}", "x ".repeat(200)), "Short.", "[P01 plan]");
        assert!(long.contains("(Asked [P01 plan]: What wording? x x") && long.contains("…)"), "{long}");
    }

    #[test]
    fn answers_go_back_in_words() {
        let p = answer_prompt("Offset or cursor?", "cursor", AnsweredBy::Matthew, true, 15);
        assert!(p.starts_with("Matthew answered your question \"Offset or cursor?\": cursor"));
        assert!(p.contains("made it a rule"));
        let d = answer_prompt("Offset or cursor?", "cursor", AnsweredBy::Default, false, 15);
        assert!(d.contains("within 15 minutes, so go ahead with the default: cursor") && !d.contains("rule"));
        assert!(crate::actions::clean_prompt(&p).is_ok());
    }
}
