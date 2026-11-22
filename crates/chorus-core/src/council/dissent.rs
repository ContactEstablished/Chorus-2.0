//! Dissent, enforced in code. The list is built here from the transcript before the arbiter
//! writes anything, and the findings document appends it whatever the arbiter wrote. The arbiter
//! can comment on an entry; it cannot remove one.

use super::verdict::{MemberVerdict, QuestionDisagreement};
use super::{Outcome, Phase, Turn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DissentPath {
    /// Computed from the members' own verdict tokens.
    Structural,
    /// Parsed from a labelled section of a critique.
    Critique,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dissent {
    pub path: DissentPath,
    /// `Q2` or `R1`.
    pub source: String,
    pub member_ids: Vec<u32>,
    pub text: String,
}

/// A label body that only says there is nothing to object to. A closed list, matched against the
/// whole body: "None of the three addressed back-pressure" is kept.
pub fn states_no_objection(section: &str) -> bool {
    const NOTHING: [&str; 16] = [
        "none", "no", "n/a", "na", "nil", "nothing", "none at all", "no objection", "no objections", "no disagreement",
        "no disagreements", "no concerns", "nothing to add", "nothing further", "i agree", "agreed",
    ];
    let body = section.trim().to_lowercase();
    let body = body.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`' | '(' | '['));
    let body = body.trim_end_matches(|c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ')' | ']' | '.' | '!' | ';' | ':' | ',' | '—' | '–' | '-')
    });
    NOTHING.contains(&body.trim())
}

/// Strip a leading label (`DISAGREE:`), returning its body, if the line starts with one of `labels`.
fn labelled<'a>(line: &'a str, labels: &[&str]) -> Option<&'a str> {
    let upper = line.to_ascii_uppercase();
    labels.iter().find_map(|l| {
        if !upper.starts_with(l) {
            return None;
        }
        let rest = &line[l.len()..];
        // A whole word: "DISAGREEMENT" is not the label.
        if rest.starts_with(|c: char| c.is_ascii_alphanumeric()) {
            return None;
        }
        let rest = rest.trim_start();
        Some(rest.strip_prefix(':').unwrap_or(rest).trim())
    })
}

/// The bodies of a critique's `DISAGREE:`, `OBJECTION:` and `MISSED:` sections. A string matcher
/// on the format the critique prompt asks for: a member that ignores the format yields nothing,
/// a known limit (structural dissents are computed and always kept).
pub fn critique_sections(content: &str) -> Vec<String> {
    let mut sections = Vec::new();
    let mut current: Option<Vec<String>> = None;
    let close = |current: &mut Option<Vec<String>>, sections: &mut Vec<String>| {
        if let Some(parts) = current.take() {
            let body = parts.join(" ").trim().to_owned();
            if !body.is_empty() && !states_no_objection(&body) {
                sections.push(body);
            }
        }
    };
    for raw in content.lines() {
        let line = raw
            .trim()
            .trim_start_matches(|c: char| matches!(c, '>' | '-' | '*' | '+') || c.is_whitespace())
            .replace("**", "");
        if let Some(body) = labelled(&line, &["DISAGREE", "OBJECTION", "MISSED"]) {
            close(&mut current, &mut sections);
            current = Some(if body.is_empty() { vec![] } else { vec![body.to_owned()] });
            continue;
        }
        if labelled(&line, &["AGREE", "CHANGED", "POSITION", "SUMMARY"]).is_some() {
            close(&mut current, &mut sections);
            continue;
        }
        if let Some(parts) = current.as_mut()
            && !line.trim().is_empty()
        {
            parts.push(line.trim().to_owned());
        }
    }
    close(&mut current, &mut sections);
    sections
}

pub fn extract(disagreement: &[QuestionDisagreement], transcript: &[Turn], label_for: &dyn Fn(u32) -> String) -> Vec<Dissent> {
    let mut out = Vec::new();
    for q in disagreement.iter().filter(|q| q.disagrees) {
        let mut by_verdict: Vec<(MemberVerdict, Vec<u32>)> = Vec::new();
        for (id, v) in &q.verdicts {
            match by_verdict.iter_mut().find(|(bv, _)| bv == v) {
                Some((_, ids)) => ids.push(*id),
                None => by_verdict.push((*v, vec![*id])),
            }
        }
        let breakdown = by_verdict
            .iter()
            .map(|(v, ids)| format!("{}: {}", v.token(), ids.iter().map(|i| label_for(*i)).collect::<Vec<_>>().join(", ")))
            .collect::<Vec<_>>()
            .join(" · ");
        out.push(Dissent {
            path: DissentPath::Structural,
            source: format!("Q{}", q.index + 1),
            member_ids: q.verdicts.iter().map(|(id, _)| *id).collect(),
            text: format!("{} — {breakdown}", q.question),
        });
    }
    for t in transcript.iter().filter(|t| t.phase == Phase::Critique && t.outcome == Outcome::Answered) {
        let Some(id) = t.member_id else { continue };
        for section in critique_sections(&t.content) {
            out.push(Dissent {
                path: DissentPath::Critique,
                source: format!("R{}", Phase::Critique.round()),
                member_ids: vec![id],
                text: format!("{}: {section}", label_for(id)),
            });
        }
    }
    out
}

pub fn render_lines(entries: &[Dissent]) -> String {
    if entries.is_empty() {
        return "_None — the council was observed to agree on every enumerated question, and no objection was raised in the critique round._".into();
    }
    entries
        .iter()
        .map(|e| {
            let path = if e.path == DissentPath::Structural { "Structural" } else { "Critique" };
            format!("- [{path} — {}] {}", e.source, e.text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether the arbiter's synthesis opened its own `## Dissents preserved` heading (a heading,
/// not a mention in prose; `###` doesn't count).
pub fn synthesis_has_heading(synthesis: &str) -> bool {
    synthesis.lines().any(|l| {
        let Some(rest) = l.strip_prefix("##") else { return false };
        if !rest.starts_with([' ', '\t']) {
            return false;
        }
        let rest = rest.trim_start().to_lowercase();
        rest.strip_prefix("dissents preserved").is_some_and(|after| !after.starts_with(|c: char| c.is_alphanumeric()))
    })
}

/// The document's section: always present. When the synthesis already has a heading by that
/// name, this one becomes a subsection that says whose record it is.
pub fn render_for_document(entries: &[Dissent], label_for: &dyn Fn(u32) -> String, synthesis_has_heading: bool) -> String {
    let heading = if synthesis_has_heading {
        "### Dissents preserved — the orchestrator's record\n\n_The synthesis above opened its own `## Dissents preserved`. \
         This is the orchestrator's, generated from the transcript and appended whatever the arbiter chose to write._"
            .to_owned()
    } else {
        "## Dissents preserved".to_owned()
    };
    format!("{heading}\n\n{}{}", attribution(entries, label_for), render_lines(entries))
}

/// Per-member counts above the list, so one talkative member reads as depth, not breadth.
fn attribution(entries: &[Dissent], label_for: &dyn Fn(u32) -> String) -> String {
    if entries.is_empty() {
        return String::new();
    }
    let structural = entries.iter().filter(|e| e.path == DissentPath::Structural).count();
    let critique: Vec<&Dissent> = entries.iter().filter(|e| e.path == DissentPath::Critique).collect();
    let mut by_member: Vec<(String, usize)> = Vec::new();
    for e in &critique {
        let Some(id) = e.member_ids.first() else { continue };
        let label = label_for(*id);
        match by_member.iter_mut().find(|(l, _)| *l == label) {
            Some((_, n)) => *n += 1,
            None => by_member.push((label, 1)),
        }
    }
    by_member.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let shape = if critique.is_empty() {
        String::new()
    } else {
        let split = by_member.iter().map(|(l, n)| format!("{l} {n}")).collect::<Vec<_>>().join(" · ");
        format!(", from {} member{} — {split}", by_member.len(), if by_member.len() == 1 { "" } else { "s" })
    };
    format!(
        "_{} preserved: {structural} structural (computed from the members' own verdict tokens) · {} from critique prose{shape}._\n\n\
         _Read the per-member split before reading breadth into the total: several objections from one member is one member \
         disagreeing repeatedly, not several members disagreeing. Nothing is dropped to make the total smaller._\n\n",
        entries.len(),
        critique.len()
    )
}

/// Structural dissents the synthesis never refers to by tag. Recorded in the document, never a
/// reason to throw away a finished run.
pub fn elided(synthesis: &str, entries: &[Dissent]) -> Vec<String> {
    let hay = synthesis.to_lowercase();
    entries
        .iter()
        .filter(|e| e.path == DissentPath::Structural && !hay.contains(&e.source.to_lowercase()))
        .map(|e| e.source.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::council::verdict;

    #[test]
    fn nothing_bodies_are_dropped_whole_body_only() {
        assert!(states_no_objection("None."));
        assert!(states_no_objection("  \"No objections!\" "));
        assert!(!states_no_objection("None of the three addressed back-pressure"));
        assert!(!states_no_objection("nothing substantive"));
    }

    #[test]
    fn critique_sections_end_at_other_labels() {
        let c = "AGREE: the shape\n**DISAGREE:** the cache is unbounded\nand grows forever\nAGREE: the rest\nMISSED: None\n\
                 OBJECTION:\nno test for CRLF\nCHANGED: no\nDISAGREEMENT is a word";
        assert_eq!(critique_sections(c), vec!["the cache is unbounded and grows forever", "no test for CRLF"]);
    }

    #[test]
    fn extract_and_render() {
        let qs = vec!["Use DPAPI?".to_owned()];
        let d = verdict::disagreement(&qs, &[(1, "Q1: AGREE"), (2, "Q1: DISAGREE"), (3, "Q1: AGREE")]);
        let transcript = vec![Turn {
            member_id: Some(2),
            phase: Phase::Critique,
            content: "DISAGREE: Position A ignores roaming profiles".into(),
            outcome: Outcome::Answered,
        }];
        let label = |id: u32| ["", "Alpha", "Beta", "Gamma"][id as usize].to_owned();
        let e = extract(&d, &transcript, &label);
        assert_eq!(render_lines(&e), "- [Structural — Q1] Use DPAPI? — AGREE: Alpha, Gamma · DISAGREE: Beta\n- [Critique — R1] Beta: Position A ignores roaming profiles");
        assert_eq!(elided("We discuss Q1 at length.", &e), Vec::<String>::new());
        assert_eq!(elided("Nothing here.", &e), vec!["Q1"]);
        let doc = render_for_document(&e, &label, false);
        assert!(doc.starts_with("## Dissents preserved\n\n_2 preserved: 1 structural"));
        assert!(doc.contains("from 1 member — Beta 1"));
        assert!(render_for_document(&e, &label, true).starts_with("### Dissents preserved — the orchestrator's record"));
    }

    #[test]
    fn heading_detection_is_anchored() {
        assert!(synthesis_has_heading("x\n## Dissents Preserved (and why)\n"));
        assert!(!synthesis_has_heading("the dissents preserved section shows"));
        assert!(!synthesis_has_heading("### Dissents preserved"));
        assert!(!synthesis_has_heading("## Dissents preservedness"));
    }
}
