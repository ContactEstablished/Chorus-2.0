//! Verdict tokens, and the disagreement measured from them.
//!
//! Two vocabularies that are never merged: what a *member* said before anyone ruled
//! (`AGREE | DISAGREE | QUALIFY`), and what the *arbiter* decided after reading them all (five
//! states). The strip shows both, so "the members split and the arbiter approved anyway" stays
//! visible. Prose is never read as a ruling: a missing token is reported as missing.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
pub enum MemberVerdict {
    Agree,
    Disagree,
    Qualify,
}

impl MemberVerdict {
    pub fn token(self) -> &'static str {
        match self {
            Self::Agree => "AGREE",
            Self::Disagree => "DISAGREE",
            Self::Qualify => "QUALIFY",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        [Self::Agree, Self::Disagree, Self::Qualify].into_iter().find(|v| v.token() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
pub enum Ruling {
    Approved,
    ApprovedWithRevisions,
    Revise,
    Rejected,
    InsufficientInformation,
}

impl Ruling {
    pub const ALL: [Self; 5] =
        [Self::Approved, Self::ApprovedWithRevisions, Self::Revise, Self::Rejected, Self::InsufficientInformation];

    pub fn token(self) -> &'static str {
        match self {
            Self::Approved => "APPROVED",
            Self::ApprovedWithRevisions => "APPROVED-WITH-REVISIONS",
            Self::Revise => "REVISE",
            Self::Rejected => "REJECTED",
            Self::InsufficientInformation => "INSUFFICIENT-INFORMATION",
        }
    }

    /// For a digest line: "approved", "approved with revisions"…
    pub fn words(self) -> String {
        self.token().to_lowercase().replace('-', " ")
    }
}

/// A line with list markers, quotes and bold removed.
fn clean(line: &str) -> String {
    line.trim()
        .trim_start_matches(|c: char| matches!(c, '>' | '-' | '*' | '+') || c.is_whitespace())
        .replace("**", "")
        .trim_end()
        .to_owned()
}

/// `Q<n>` then `:`, `.` or `-`, returning the 0-based index and the rest.
fn q_line(line: &str) -> Option<(usize, &str)> {
    let rest = line.strip_prefix('Q').or_else(|| line.strip_prefix('q'))?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if !(1..=2).contains(&digits) {
        return None;
    }
    let n: usize = rest[..digits].parse().ok()?;
    let rest = rest[digits..].trim_start();
    let rest = rest.strip_prefix([':', '.', '-'])?.trim_start();
    (n >= 1).then_some((n - 1, rest))
}

/// One token per question from a member's position. The first token for a question wins, so a
/// later restatement can't change what the measurement used.
pub fn parse_member(content: &str) -> BTreeMap<usize, MemberVerdict> {
    let mut out = BTreeMap::new();
    for raw in content.lines() {
        let line = clean(raw);
        let Some((index, rest)) = q_line(&line) else { continue };
        let word: String = rest.chars().take_while(char::is_ascii_alphabetic).collect();
        if let Some(v) = MemberVerdict::parse(&word.to_ascii_uppercase()) {
            out.entry(index).or_insert(v);
        }
    }
    out
}

/// The arbiter's ruling block.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ArbiterRuling {
    /// False when no `## Verdict` heading and no ruling line was found: the arbiter didn't give
    /// one. True with a question missing means it gave a block and skipped that question.
    pub block_present: bool,
    pub rulings: BTreeMap<usize, Ruling>,
}

pub fn parse_arbiter(content: &str) -> ArbiterRuling {
    let mut out = ArbiterRuling::default();
    for raw in content.lines() {
        let line = clean(raw);
        let bare = line.trim_start_matches('#').trim().to_ascii_uppercase();
        if line.len() - line.trim_start_matches('#').len() <= 4 && (bare == "VERDICT" || bare == "VERDICTS") {
            out.block_present = true;
            continue;
        }
        let Some((index, rest)) = q_line(&line) else { continue };
        let word: String = rest.chars().take_while(|c| c.is_ascii_alphabetic() || *c == '-' || *c == ' ').collect();
        let token = word.split([' ', '-']).filter(|w| !w.is_empty()).collect::<Vec<_>>().join("-").to_ascii_uppercase();
        if let Some(r) = Ruling::ALL.into_iter().find(|r| r.token() == token) {
            out.rulings.entry(index).or_insert(r);
            out.block_present = true;
        }
    }
    out
}

/// How a question's disagreement was established. Carried into the output: a counted
/// disagreement and a model's impression of one are different facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum DetectionPath {
    Structural,
    ModelJudged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionDisagreement {
    pub index: usize,
    pub question: String,
    pub detection: DetectionPath,
    /// True only when structural and the verdicts differ.
    pub disagrees: bool,
    /// Member id → verdict, in the order the positions were given.
    pub verdicts: Vec<(u32, MemberVerdict)>,
    /// Members whose position had no token for this question.
    pub silent: Vec<u32>,
}

/// The measurement: structural when at least two members gave a token, model-judged otherwise.
pub fn disagreement(questions: &[String], positions: &[(u32, &str)]) -> Vec<QuestionDisagreement> {
    let parsed: Vec<(u32, BTreeMap<usize, MemberVerdict>)> = positions.iter().map(|(id, c)| (*id, parse_member(c))).collect();
    questions
        .iter()
        .enumerate()
        .map(|(index, question)| {
            let mut verdicts = Vec::new();
            let mut silent = Vec::new();
            for (id, v) in &parsed {
                match v.get(&index) {
                    Some(v) => verdicts.push((*id, *v)),
                    None => silent.push(*id),
                }
            }
            let structural = verdicts.len() >= 2;
            let disagrees = structural && verdicts.iter().any(|(_, v)| *v != verdicts[0].1);
            QuestionDisagreement {
                index,
                question: question.clone(),
                detection: if structural { DetectionPath::Structural } else { DetectionPath::ModelJudged },
                disagrees,
                verdicts,
                silent,
            }
        })
        .collect()
}

/// A question's outcome in one word. It says whether the members converged, never whether they
/// were right. A unanimous DISAGREE is `Disagreed`, not a green light.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum QuestionState {
    Agreed,
    Qualified,
    Split,
    Disagreed,
    NotMeasured,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Vote {
    pub label: String,
    pub verdict: MemberVerdict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct QuestionSummary {
    pub index: u32,
    /// Clipped to 200 characters: the strip is a glance, not a re-read.
    pub question: String,
    pub detection: DetectionPath,
    pub state: QuestionState,
    pub votes: Vec<Vote>,
    pub silent: Vec<String>,
}

const SUMMARY_QUESTION_CAP: usize = 200;

pub fn clip(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_owned();
    }
    let head: String = s.chars().take(max_chars - 1).collect();
    format!("{}…", head.trim_end())
}

fn state_of(q: &QuestionDisagreement) -> QuestionState {
    if q.detection != DetectionPath::Structural {
        return QuestionState::NotMeasured;
    }
    let first = q.verdicts[0].1;
    if q.verdicts.iter().all(|(_, v)| *v == first) {
        return match first {
            MemberVerdict::Agree => QuestionState::Agreed,
            MemberVerdict::Qualify => QuestionState::Qualified,
            MemberVerdict::Disagree => QuestionState::Disagreed,
        };
    }
    if q.verdicts.iter().any(|(_, v)| *v == MemberVerdict::Disagree) { QuestionState::Split } else { QuestionState::Qualified }
}

pub fn summarise(disagreement: &[QuestionDisagreement], label_for: &dyn Fn(u32) -> String) -> Vec<QuestionSummary> {
    disagreement
        .iter()
        .map(|q| QuestionSummary {
            index: q.index as u32,
            question: clip(&q.question, SUMMARY_QUESTION_CAP),
            detection: q.detection,
            state: state_of(q),
            votes: q.verdicts.iter().map(|(id, v)| Vote { label: label_for(*id), verdict: *v }).collect(),
            silent: q.silent.iter().map(|id| label_for(*id)).collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_tokens_first_wins_and_markup_is_tolerated() {
        let v = parse_member("- **Q1: AGREE** because\n> Q2 - disagree\nQ3. MAYBE\nQ1: DISAGREE in summary\nQ12: qualify");
        assert_eq!(v.get(&0), Some(&MemberVerdict::Agree));
        assert_eq!(v.get(&1), Some(&MemberVerdict::Disagree));
        assert_eq!(v.get(&2), None, "outside the vocabulary");
        assert_eq!(v.get(&11), Some(&MemberVerdict::Qualify));
    }

    #[test]
    fn arbiter_block_states() {
        let r = parse_arbiter("## Verdict\nQ1: APPROVED WITH REVISIONS\nQ2: approved\nQ3: MAYBE\n\nQ1: REJECTED later");
        assert!(r.block_present);
        assert_eq!(r.rulings.get(&0), Some(&Ruling::ApprovedWithRevisions));
        assert_eq!(r.rulings.get(&1), Some(&Ruling::Approved));
        assert_eq!(r.rulings.get(&2), None);
        assert!(!parse_arbiter("I think it is approved overall.").block_present, "prose is never a ruling");
        assert!(parse_arbiter("### VERDICTS\n").block_present);
        assert!(parse_arbiter("Q1: insufficient information").rulings.get(&0) == Some(&Ruling::InsufficientInformation));
    }

    #[test]
    fn measurement_and_states() {
        let qs: Vec<String> = ["one question", "two question", "three question", "four question"].map(String::from).to_vec();
        let a = "Q1: AGREE\nQ2: AGREE\nQ3: QUALIFY\nQ4: DISAGREE";
        let b = "Q1: AGREE\nQ2: DISAGREE\nQ3: AGREE";
        let d = disagreement(&qs, &[(1, a), (2, b)]);
        assert!(!d[0].disagrees && d[0].detection == DetectionPath::Structural);
        assert!(d[1].disagrees);
        assert_eq!(d[3].detection, DetectionPath::ModelJudged);
        assert_eq!(d[3].silent, vec![2]);
        let s = summarise(&d, &|id| format!("M{id}"));
        let states: Vec<_> = s.iter().map(|q| q.state).collect();
        assert_eq!(states, vec![QuestionState::Agreed, QuestionState::Split, QuestionState::Qualified, QuestionState::NotMeasured]);
        assert_eq!(s[3].silent, vec!["M2"]);
        let both_no = disagreement(&qs[..1], &[(1, "Q1: DISAGREE"), (2, "Q1: DISAGREE")]);
        assert_eq!(summarise(&both_no, &|_| String::new())[0].state, QuestionState::Disagreed);
    }

    #[test]
    fn clip_keeps_short_and_marks_long() {
        assert_eq!(clip("short", 10), "short");
        assert_eq!(clip("abcdefghijk", 5), "abcd…");
    }
}
