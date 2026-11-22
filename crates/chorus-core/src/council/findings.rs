//! The findings document: run header, standing caveat, the arbiter's synthesis, how disagreement
//! was detected, any elision, the dissents (always), and provenance.

use super::dissent::{self, Dissent};
use super::verdict::{DetectionPath, QuestionDisagreement};
use super::{TurnOutcome, Phase, State};

/// Directly under the header, above the synthesis: a caveat read after the conclusions is too late.
pub const STANDING_CAVEAT: &str = "> **These findings are model deliberation, not verified fact.** The members could read the \
project's code, read-only, but nothing here was compiled, executed or tested, and a model can misread what it reads. Verify \
anything you are about to rely on.";

pub fn assemble(state: &State, synthesis: &str, disagreement: &[QuestionDisagreement], dissents: &[Dissent]) -> String {
    let run = &state.run;
    let label_for = |id: u32| run.label_for(id);
    let planned = run.members.len();
    let answered: std::collections::BTreeSet<u32> = state
        .transcript
        .iter()
        .filter(|t| t.phase == Phase::Positions && t.outcome == TurnOutcome::Answered)
        .filter_map(|t| t.member_id)
        .collect();

    let header = if answered.len() < planned {
        let refusals = state
            .transcript
            .iter()
            .filter(|t| t.outcome != TurnOutcome::Answered)
            .map(|t| {
                let who = t.member_id.map_or_else(|| "the orchestrator".to_owned(), label_for);
                format!("> - {who} refused at **{}** (round {}): {}", t.phase.key(), t.phase.round(), t.content)
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "> **PARTIAL RUN — {} of {planned} members completed.**\n>\n{refusals}\n>\n> These findings are the output of a \
             council that did not fully convene. Read them as such.",
            answered.len()
        )
    } else {
        format!("> Council of {planned} members plus an arbiter. All members completed.")
    };

    let measurement = disagreement
        .iter()
        .map(|q| {
            let how = match q.detection {
                DetectionPath::Structural => {
                    format!("`structural` · {}", if q.disagrees { "members disagreed" } else { "members agreed" })
                }
                DetectionPath::ModelJudged => "`model-judged` · not measured".to_owned(),
            };
            let silent = if q.silent.is_empty() {
                String::new()
            } else {
                format!(" · no verdict token from: {}", q.silent.iter().map(|i| label_for(*i)).collect::<Vec<_>>().join(", "))
            };
            format!("- **Q{}** — detection: {how}{silent}", q.index + 1)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let elided = dissent::elided(synthesis, dissents);
    let elided_note = if elided.is_empty() {
        String::new()
    } else {
        format!(
            "\n\n> **The arbiter's synthesis did not refer to {}.** The disagreement is preserved below regardless — the \
             section is generated from the transcript, not by the arbiter.",
            elided.join(", ")
        )
    };

    format!(
        "{header}\n\n{STANDING_CAVEAT}\n\n{}\n\n---\n\n## How disagreement was detected\n\n{measurement}\n\n\
         _`structural` means the orchestrator compared the members' own verdict tokens and counted the difference. \
         `model-judged` means too few members answered in the required form, so the arbiter judged it from prose — a weaker \
         signal, labelled rather than hidden._{elided_note}\n\n{}\n\n{}\n",
        synthesis.trim(),
        dissent::render_for_document(dissents, &label_for, dissent::synthesis_has_heading(synthesis)),
        provenance(state),
    )
}

fn provenance(state: &State) -> String {
    let outcome = |id: u32| {
        let turns: Vec<_> = state.transcript.iter().filter(|t| t.member_id == Some(id)).collect();
        if turns.is_empty() {
            return "never asked".to_owned();
        }
        let answered = turns.iter().filter(|t| t.outcome == TurnOutcome::Answered).count();
        let other = turns.len() - answered;
        let s = |n: usize| if n == 1 { "" } else { "s" };
        match (answered, other) {
            (a, 0) => format!("answered {a} turn{}", s(a)),
            (0, r) => format!("refused {r} turn{}", s(r)),
            (a, r) => format!("answered {a}, refused {r}"),
        }
    };
    let rows = state
        .run
        .members
        .iter()
        .map(|m| (m, "member"))
        .chain(std::iter::once((&state.run.arbiter, "arbiter")))
        .map(|(m, role)| format!("| {} | {role} | `{}` | {} |", m.label, m.model, outcome(m.id)))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "## Provenance\n\n- **Run:** {}\n- **Project:** {}\n- **Started:** {}\n\n| Member | Role | Model | Turns |\n|---|---|---|---|\n{rows}\n",
        state.run_id, state.project_name, state.started_at
    )
}
