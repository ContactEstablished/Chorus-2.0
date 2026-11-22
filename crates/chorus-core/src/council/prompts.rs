//! The four prompts. A prompt is the protocol, so they are built here and nowhere else.
//!
//! Chorus 1's wording is kept; 2.0 adds the code mandate (D137) to every prompt, and gives the
//! critique round the brief and the questions (Chorus 1 sent only the other positions).

use super::dissent::{self, Dissent};
use super::verdict::{DetectionPath, QuestionDisagreement};
use super::{TurnOutcome, Phase, State};

/// What every participant is told about the project's code.
pub fn code_mandate(project_name: &str) -> String {
    format!(
        "## Your access to the code\n\n\
         You can read the source code of the project this brief is about (\"{project_name}\"), read-only, with three tools: \
         `list_files` (the files under a folder), `read_file` (numbered lines of one file) and `search_code` (lines containing \
         some text). Paths are relative to the project's root folder. You cannot change anything, run anything, or see files \
         that hold secrets.\n\n\
         Use them. Check the brief's claims and its rubric against the code itself instead of taking the brief's description \
         of the code on trust. When something you say rests on code you read, cite it as `path:line`. When you could not \
         verify a claim, say so plainly rather than guessing. Your tool use is limited, so read what matters for the \
         questions; you will be told when the limit is reached, and must then answer with what you have."
    )
}

fn numbered(questions: &[String]) -> String {
    questions.iter().enumerate().map(|(i, q)| format!("Q{}. {q}", i + 1)).collect::<Vec<_>>().join("\n")
}

pub fn positions(brief: &str, questions: &[String], mandate: &str) -> String {
    format!(
        "{brief}\n\n---\n\n{mandate}\n\n---\n\n\
         You are one member of an independent review council. You are answering BLIND: you cannot see the other members' \
         answers, and they cannot see yours.\n\n\
         Answer each question below. For EACH one, begin a line with the question number and one verdict token, exactly in \
         this form:\n\nQ1: AGREE\nQ2: DISAGREE\nQ3: QUALIFY\n\n\
         Use AGREE if you support the proposal in the question, DISAGREE if you oppose it, and QUALIFY if you support it only \
         under a condition you must then state. After each verdict line, give your reasoning in prose, citing the code you \
         checked.\n\n\
         The questions:\n{}\n\n\
         If you believe the questions miss something important, add a final section headed \"UNPROMPTED OBSERVATION\" and say \
         what.",
        numbered(questions)
    )
}

/// The other members' positions, relabelled A, B… after the asker is removed. Only the prompt is
/// anonymous: the transcript keeps who said what.
pub fn critique(brief: &str, questions: &[String], self_id: u32, positions: &[(u32, &str)], mandate: &str) -> String {
    let others = positions
        .iter()
        .filter(|(id, _)| *id != self_id)
        .enumerate()
        .map(|(i, (_, content))| format!("### Position {}\n\n{content}", char::from(b'A' + i as u8)))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!(
        "{brief}\n\n---\n\n{mandate}\n\n---\n\n\
         You are one member of an independent review council. You have already answered these questions:\n\n{}\n\n\
         Below are the other members' positions on the same brief, with their identities removed. Judge them on their \
         content. Where a position makes a claim about the code, check it against the code before you accept or reject it.\n\n\
         {others}\n\n---\n\n\
         For EACH position, respond using these exact section labels, one per line:\n\n\
         AGREE: <what you agree with>\nDISAGREE: <what you disagree with, and why>\nMISSED: <anything the position did not \
         consider>\n\nThen finish with:\n\nCHANGED: <has your own position changed, and why or why not>\n\n\
         Use the DISAGREE and MISSED labels literally. An objection written without its label may not reach the findings \
         document.",
        numbered(questions)
    )
}

fn vector(disagreement: &[QuestionDisagreement], label_for: &dyn Fn(u32) -> String) -> String {
    disagreement
        .iter()
        .map(|q| {
            if q.detection == DetectionPath::ModelJudged {
                return format!(
                    "Q{}: NOT MEASURED (too few members answered in the required form) — judge it from the prose.",
                    q.index + 1
                );
            }
            let votes = q.verdicts.iter().map(|(id, v)| format!("{}={}", label_for(*id), v.token())).collect::<Vec<_>>().join(", ");
            format!("Q{}: {} — {votes}", q.index + 1, if q.disagrees { "MEMBERS DISAGREE" } else { "members agree" })
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn attributed(state: &State, phase: Phase, suffix: &str) -> String {
    state
        .transcript
        .iter()
        .filter(|t| t.phase == phase && t.outcome == TurnOutcome::Answered)
        .filter_map(|t| t.member_id.map(|id| format!("### {}{suffix}\n\n{}", state.run.label_for(id), t.content)))
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn arbitration(state: &State, disagreement: &[QuestionDisagreement], mandate: &str) -> String {
    let label_for = |id| state.run.label_for(id);
    let closing = if disagreement.iter().any(|q| q.disagrees) {
        "Where the members disagreed, say which position is better supported and why — and if the minority position is \
         right, say so plainly. Where they disagree about what the code does, read the code yourself before ruling."
    } else {
        "The members AGREED on every measured question. That is not the same as being right. They all read the same brief, \
         so they share its blind spots. Say whether this agreement is warranted, and what the council collectively missed — \
         check the code for it."
    };
    format!(
        "{}\n\n---\n\n{mandate}\n\n---\n\n\
         You are the ARBITER of this review council. Unlike the members, you see who said what.\n\n\
         ## The members' positions\n\n{}\n\n\
         ## The members' critiques of each other\n\n{}\n\n\
         ## What the orchestrator MEASURED (not an opinion — a count of their verdict tokens)\n\n{}\n\n---\n\n\
         Rule on each question in turn, with your reasoning.\n\n\
         Begin your answer with a section headed exactly \"## Verdict\", containing one line per question and nothing else, \
         in this form:\n\nQ1: APPROVED\nQ2: APPROVED-WITH-REVISIONS\nQ3: REVISE\nQ4: REJECTED\nQ5: INSUFFICIENT-INFORMATION\n\n\
         Use APPROVED if the proposal in the question should proceed as written; APPROVED-WITH-REVISIONS if it should \
         proceed once specific changes you name are made; REVISE if the proposal is not yet sound enough to act on; REJECTED \
         if it should not proceed; and INSUFFICIENT-INFORMATION if the council was not given enough to rule on it. Use one \
         of those five tokens exactly — do not invent a sixth, and do not omit a question. Then, below that section, give \
         your full reasoning in prose.\n\n{closing}",
        state.run.brief_text,
        attributed(state, Phase::Positions, " — position"),
        attributed(state, Phase::Critique, " — critique"),
        vector(disagreement, &label_for),
    )
}

pub fn synthesis(state: &State, dissents: &[Dissent], mandate: &str) -> String {
    let ruling = state
        .transcript
        .iter()
        .find(|t| t.phase == Phase::Arbitration && t.outcome == TurnOutcome::Answered)
        .map_or("(none)", |t| t.content.as_str());
    format!(
        "You are the ARBITER of this review council, writing its findings.\n\n\
         ## The brief\n\n{}\n\n{mandate}\n\n\
         ## The members' positions\n\n{}\n\n\
         ## Your own ruling\n\n{ruling}\n\n\
         ## Disagreements the orchestrator recorded\n\n## Dissents preserved\n\n{}\n\n---\n\n\
         Write the findings document. Include per-member positions, a synthesis with your ruling per question, risks with \
         mitigations, and action items that are each checkable. Keep the `path:line` citations that support them.\n\n\
         You MAY add narrative context to any disagreement above — say whether you think it is well-founded. You MUST NOT \
         drop one. Refer to each by its source tag (Q1, Q2, …) so a reader can match your commentary to the record.",
        state.run.brief_text,
        attributed(state, Phase::Positions, ""),
        dissent::render_lines(dissents),
    )
}
