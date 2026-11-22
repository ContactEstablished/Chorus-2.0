//! A brief's numbered questions, and the checks a brief passes before any model reads it.
//!
//! Questions come from the first heading whose subject contains "questions", down to the next
//! heading at the same level or shallower. A brief without such a heading is scanned whole; one
//! whose questions section is empty is refused rather than scanned whole. Chorus 1 learned both
//! from real briefs: scanning everything turned rubric lines into "questions" (D68(1) there).

use crate::scrub::{self, Pattern};

use super::MAX_BRIEF_BYTES;

/// Lines in either ending. A CRLF brief once made every heading invisible in Chorus 1.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l))
}

/// `(level, text)` for a Markdown ATX heading.
fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.bytes().take_while(|b| *b == b'#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &line[level..];
    let text = rest.strip_prefix(|c: char| c.is_whitespace())?;
    Some((level, text.trim_start()))
}

/// Whether a heading's *subject* names the questions. The section number, emphasis, and anything
/// after a dash or colon are not the subject: "Binding prior rulings — not open questions" is not
/// the questions section.
fn is_questions_heading(text: &str) -> bool {
    let mut s = text.trim_start();
    // "8. " or "3.1 " or "8) ".
    let digits_and_dots = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    if digits_and_dots > 0 && s[..digits_and_dots].starts_with(|c: char| c.is_ascii_digit()) {
        let mut rest = &s[digits_and_dots..];
        rest = rest.strip_prefix(')').unwrap_or(rest);
        if rest.starts_with(char::is_whitespace) {
            s = rest.trim_start();
        }
    }
    let cleaned: String = s.chars().filter(|c| !matches!(c, '*' | '_' | '`')).collect();
    let subject = cleaned.split(['—', '–', ':']).next().unwrap_or("").to_lowercase();
    subject
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w == "questions")
}

/// The text under the first questions heading, or `None` when there is none.
pub fn questions_section(text: &str) -> Option<String> {
    let all: Vec<&str> = lines(text).collect();
    let (start, level) = all.iter().enumerate().find_map(|(i, l)| {
        heading(l).filter(|(_, t)| is_questions_heading(t)).map(|(level, _)| (i + 1, level))
    })?;
    let end = all[start..]
        .iter()
        .position(|l| heading(l).is_some_and(|(lv, _)| lv <= level))
        .map_or(all.len(), |p| start + p);
    Some(all[start..end].join("\n"))
}

/// The numbered questions: lines starting `1.` or `1)` (after any list marker or quote), at least
/// eight characters of text, in the order they appear. The brief's own numbers are not kept; the
/// prompts number them Q1..Qn.
pub fn parse_questions(text: &str) -> Vec<String> {
    let section = questions_section(text);
    let scope = section.as_deref().unwrap_or(text);
    lines(scope)
        .filter_map(|raw| {
            let line = raw.trim().trim_start_matches(|c: char| matches!(c, '>' | '-' | '*' | '+') || c.is_whitespace());
            let digits = line.bytes().take_while(u8::is_ascii_digit).count();
            if !(1..=2).contains(&digits) {
                return None;
            }
            let rest = &line[digits..];
            let rest = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')'))?;
            if !rest.starts_with(char::is_whitespace) {
                return None;
            }
            let question = rest.replace("**", "").trim().to_owned();
            (question.chars().count() >= 8).then_some(question)
        })
        .collect()
}

/// Everything wrong with a brief before a run may start, as one sentence.
pub fn check(text: &str, patterns: &[Pattern]) -> Result<Vec<String>, String> {
    if text.len() > MAX_BRIEF_BYTES {
        return Err(format!(
            "That brief is {} KB; the limit is {} KB. Every council member reads all of it in every round.",
            text.len().div_ceil(1000),
            MAX_BRIEF_BYTES / 1000
        ));
    }
    if text.trim().is_empty() {
        return Err("A council needs a brief to deliberate on.".into());
    }
    for (n, line) in lines(text).enumerate() {
        if let Some(name) = scrub::first_pattern(line.as_bytes(), patterns) {
            return Err(format!(
                "This brief looks like it contains a credential, so the run was refused before any model read it: line {} ({name}). Remove it from the brief and run again.",
                n + 1
            ));
        }
    }
    if questions_section(text).is_some_and(|s| parse_questions(&s).is_empty()) {
        return Err("This brief's questions section has no numbered questions. List them as \"1. …\", \"2. …\" under that heading.".into());
    }
    let questions = parse_questions(text);
    if questions.is_empty() {
        return Err("This brief has no numbered questions. A council compares the members' answers question by question, so the brief needs a numbered list (\"1. …\", \"2. …\") for them to answer.".into());
    }
    Ok(questions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn questions_come_from_the_questions_section_only() {
        let brief = "# Brief\n\n## 6. Constraints\n\n1. Windows-only v1 for now.\n2. No Electron in the build.\n\n\
                     ## 8. Questions for the council\n\n1. Should the vault use DPAPI blobs?\n2. **Is SQLite the right store?**\n\n\
                     ### 8.1 Detail\n\n3. Does the migration plan hold up?\n\n## 9. Output\n\n1. Write findings please.\n";
        assert_eq!(
            parse_questions(brief),
            vec!["Should the vault use DPAPI blobs?", "Is SQLite the right store?", "Does the migration plan hold up?"]
        );
    }

    #[test]
    fn a_qualifier_mentioning_questions_is_not_the_section() {
        let brief = "## 4. Binding prior rulings — constraints on your answer, not open questions\n\n1. Rule one is binding.\n\n\
                     ## 5. Questions\n\n1. Is the design sound overall?\n";
        assert_eq!(parse_questions(brief), vec!["Is the design sound overall?"]);
    }

    #[test]
    fn crlf_briefs_find_their_headings() {
        let brief = "## Context\r\n\r\n1. Not a question at all.\r\n\r\n## Questions\r\n\r\n1. Is this the real question?\r\n";
        assert_eq!(parse_questions(brief), vec!["Is this the real question?"]);
    }

    #[test]
    fn without_a_heading_the_whole_brief_is_scanned() {
        assert_eq!(parse_questions("Intro.\n\n- 1) First question here?\n> 2. Second question here?\n3.1 not one\n4. Short\n"), vec![
            "First question here?",
            "Second question here?"
        ]);
    }

    #[test]
    fn an_empty_questions_section_refuses_instead_of_scanning_everything() {
        let brief = "## Rubric\n\n1. A rubric line, not a question.\n\n## Questions\n\nNone yet.\n";
        assert!(check(brief, &[]).unwrap_err().contains("questions section has no numbered questions"));
    }

    #[test]
    fn a_brief_with_a_credential_is_refused_by_line_and_pattern_never_value() {
        let patterns = scrub::builtin_patterns();
        let key = format!("sk-or-v1-{}", "a".repeat(64));
        let brief = format!("## Questions\n\n1. Is this design sound?\n\nkey: {key}\n");
        let err = check(&brief, &patterns).unwrap_err();
        assert!(err.contains("line 5"), "{err}");
        assert!(!err.contains(&key));
    }

    #[test]
    fn size_and_blank_refusals() {
        assert!(check("   ", &[]).unwrap_err().contains("needs a brief"));
        assert!(check(&"x".repeat(MAX_BRIEF_BYTES + 1), &[]).unwrap_err().contains("limit is 200 KB"));
        assert!(check("No list here.", &[]).unwrap_err().contains("no numbered questions"));
    }
}
