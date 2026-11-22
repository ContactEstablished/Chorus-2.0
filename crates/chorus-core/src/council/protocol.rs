//! The state machine: what to do next, recomputed from the transcript every time. Nothing is
//! carried between calls, so every branch is testable by handing it a state.

use super::verdict::{self, QuestionSummary};
use super::{Action, MIN_MEMBERS, Phase, State, brief, dissent, findings, prompts};

pub fn next_action(state: &State) -> Vec<Action> {
    if state.cancelled {
        return vec![Action::Abort { reason: "The run was cancelled.".into() }];
    }
    let questions = brief::parse_questions(&state.run.brief_text);
    let mandate = prompts::code_mandate(&state.project_name);
    let brief = &state.run.brief_text;

    if !state.has_turn(Phase::Positions) {
        let prompt = prompts::positions(brief, &questions, &mandate);
        return state
            .run
            .members
            .iter()
            .map(|m| Action::Ask { member_id: m.id, phase: Phase::Positions, prompt: prompt.clone() })
            .collect();
    }

    let positions = state.answered_in(Phase::Positions);
    if positions.len() < MIN_MEMBERS {
        return vec![Action::Abort {
            reason: format!(
                "Only {} of {} members answered; a council needs at least two.",
                positions.len(),
                state.run.members.len()
            ),
        }];
    }
    let disagreement = verdict::disagreement(&questions, &positions);

    if !state.has_turn(Phase::Critique) {
        return positions
            .iter()
            .map(|(id, _)| Action::Ask {
                member_id: *id,
                phase: Phase::Critique,
                prompt: prompts::critique(brief, &questions, *id, &positions, &mandate),
            })
            .collect();
    }

    let label_for = |id| state.run.label_for(id);
    let dissents = dissent::extract(&disagreement, &state.transcript, &label_for);

    if !state.has_turn(Phase::Arbitration) {
        return vec![Action::Ask {
            member_id: state.run.arbiter.id,
            phase: Phase::Arbitration,
            prompt: prompts::arbitration(state, &disagreement, &mandate),
        }];
    }
    if state.answered_in(Phase::Arbitration).is_empty() {
        return vec![Action::Abort { reason: "The arbiter refused at arbitration; the run cannot produce findings.".into() }];
    }

    if !state.has_turn(Phase::Synthesis) {
        return vec![Action::Ask {
            member_id: state.run.arbiter.id,
            phase: Phase::Synthesis,
            prompt: prompts::synthesis(state, &dissents, &mandate),
        }];
    }
    let Some((_, synthesis)) = state.answered_in(Phase::Synthesis).first().copied() else {
        return vec![Action::Abort { reason: "The arbiter refused at synthesis; the run cannot produce findings.".into() }];
    };

    vec![Action::Complete {
        summary: verdict::summarise(&disagreement, &label_for),
        findings: findings::assemble(state, synthesis, &disagreement, &dissents),
    }]
}

/// The glance strip, from the same measurement the findings use. Empty below the floor.
pub fn summary(state: &State) -> Vec<QuestionSummary> {
    let positions = state.answered_in(Phase::Positions);
    if positions.len() < MIN_MEMBERS {
        return vec![];
    }
    let questions = brief::parse_questions(&state.run.brief_text);
    verdict::summarise(&verdict::disagreement(&questions, &positions), &|id| state.run.label_for(id))
}

/// The phase a run is in, from its transcript (for the screen's phase track).
pub fn current_phase(state: &State) -> Phase {
    [Phase::Positions, Phase::Critique, Phase::Arbitration, Phase::Synthesis]
        .into_iter()
        .rev()
        .find(|p| state.has_turn(*p))
        .unwrap_or(Phase::Positions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::council::{Outcome, Participant, PlannedRun, Turn};

    fn p(id: u32, label: &str) -> Participant {
        Participant { id, label: label.into(), model: format!("vendor/model-{id}") }
    }

    fn state() -> State {
        State {
            run: PlannedRun {
                members: vec![p(1, "Alpha"), p(2, "Beta"), p(3, "Gamma")],
                arbiter: p(4, "Arbiter"),
                brief_text: "# Brief\n\n## Questions\n\n1. Should we adopt the vault design?\n2. Is the migration plan sound?\n".into(),
            },
            transcript: vec![],
            cancelled: false,
            run_id: 7,
            project_name: "Demo".into(),
            started_at: "2026-11-21T10:00:00Z".into(),
        }
    }

    fn turn(id: Option<u32>, phase: Phase, content: &str, outcome: Outcome) -> Turn {
        Turn { member_id: id, phase, content: content.into(), outcome }
    }

    fn asks(actions: &[Action]) -> Vec<(u32, Phase, String)> {
        actions
            .iter()
            .map(|a| match a {
                Action::Ask { member_id, phase, prompt } => (*member_id, *phase, prompt.clone()),
                other => panic!("expected ask, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn positions_are_blind_and_carry_the_mandate() {
        let a = asks(&next_action(&state()));
        assert_eq!(a.iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 2, 3], "arbiter is not in the blind round");
        assert!(a.iter().all(|x| x.1 == Phase::Positions));
        assert!(a[0].2.contains("Q1. Should we adopt the vault design?\nQ2. Is the migration plan sound?"));
        assert!(a[0].2.contains("`read_file`") && a[0].2.contains("\"Demo\""));
    }

    #[test]
    fn critique_is_anonymised_skips_refusals_and_carries_the_brief() {
        let mut s = state();
        s.transcript = vec![
            turn(Some(1), Phase::Positions, "Q1: AGREE\nAlpha text", Outcome::Answered),
            turn(Some(2), Phase::Positions, "Q1: DISAGREE\nBeta text", Outcome::Answered),
            turn(Some(3), Phase::Positions, "The provider returned an error.", Outcome::Refused),
        ];
        let a = asks(&next_action(&s));
        assert_eq!(a.iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 2]);
        let alpha = &a[0].2;
        assert!(alpha.contains("### Position A\n\nQ1: DISAGREE\nBeta text"));
        assert!(!alpha.contains("Alpha text") && !alpha.contains("Beta\n") && !alpha.contains("vendor/model-"));
        assert!(alpha.contains("## Questions") && alpha.contains("Q2. Is the migration plan sound?"));
    }

    #[test]
    fn floors_and_aborts() {
        let mut s = state();
        s.transcript = vec![
            turn(Some(1), Phase::Positions, "Q1: AGREE", Outcome::Answered),
            turn(Some(2), Phase::Positions, "x", Outcome::Refused),
            turn(Some(3), Phase::Positions, "x", Outcome::Refused),
        ];
        assert_eq!(next_action(&s), vec![Action::Abort { reason: "Only 1 of 3 members answered; a council needs at least two.".into() }]);
        let mut c = state();
        c.cancelled = true;
        assert!(matches!(&next_action(&c)[0], Action::Abort { reason } if reason.contains("cancelled")));
    }

    #[test]
    fn a_full_run_completes_with_dissents_kept_whatever_the_arbiter_wrote() {
        let mut s = state();
        s.transcript = vec![
            turn(Some(1), Phase::Positions, "Q1: AGREE\nQ2: AGREE", Outcome::Answered),
            turn(Some(2), Phase::Positions, "Q1: DISAGREE\nQ2: AGREE", Outcome::Answered),
            turn(Some(3), Phase::Positions, "Q1: AGREE\nQ2: AGREE", Outcome::Answered),
        ];
        assert!(matches!(&next_action(&s)[0], Action::Ask { phase: Phase::Critique, .. }));
        for id in 1..=3 {
            s.transcript.push(turn(Some(id), Phase::Critique, "DISAGREE: store.rs:10 leaks the handle", Outcome::Answered));
        }
        let arb = asks(&next_action(&s));
        assert_eq!((arb[0].0, arb[0].1), (4, Phase::Arbitration));
        assert!(arb[0].2.contains("### Alpha — position") && arb[0].2.contains("Q1: MEMBERS DISAGREE — Alpha=AGREE, Beta=DISAGREE, Gamma=AGREE"));
        s.transcript.push(turn(Some(4), Phase::Arbitration, "## Verdict\nQ1: REVISE\nQ2: APPROVED", Outcome::Answered));
        let syn = asks(&next_action(&s));
        assert!(syn[0].2.contains("- [Structural — Q1]"));
        s.transcript.push(turn(Some(4), Phase::Synthesis, "All fine, nothing to see.", Outcome::Answered));
        let Action::Complete { summary, findings } = &next_action(&s)[0] else { panic!() };
        assert_eq!(summary.len(), 2);
        assert!(findings.starts_with("> Council of 3 members plus an arbiter. All members completed."));
        assert!(findings.contains(findings::STANDING_CAVEAT));
        assert!(findings.contains("did not refer to Q1"));
        assert!(findings.contains("- [Structural — Q1] Should we adopt the vault design? — AGREE: Alpha, Gamma · DISAGREE: Beta"));
        assert!(findings.contains("- [Critique — R1] Beta: store.rs:10 leaks the handle"));
        assert!(findings.contains("| Arbiter | arbiter | `vendor/model-4` | answered 2 turns |"));
        assert_eq!(current_phase(&s), Phase::Synthesis);
    }

    #[test]
    fn arbiter_refusals_abort_without_a_fallback() {
        let mut s = state();
        for id in 1..=3 {
            s.transcript.push(turn(Some(id), Phase::Positions, "Q1: AGREE", Outcome::Answered));
            s.transcript.push(turn(Some(id), Phase::Critique, "AGREE: all", Outcome::Answered));
        }
        s.transcript.push(turn(Some(4), Phase::Arbitration, "error", Outcome::Refused));
        assert!(matches!(&next_action(&s)[0], Action::Abort { reason } if reason.contains("at arbitration")));
        s.transcript.pop();
        s.transcript.push(turn(Some(4), Phase::Arbitration, "## Verdict\nQ1: APPROVED", Outcome::Answered));
        let syn = asks(&next_action(&s));
        assert!(syn[0].2.contains("_None — the council was observed to agree"));
        s.transcript.push(turn(Some(4), Phase::Synthesis, "error", Outcome::Refused));
        assert!(matches!(&next_action(&s)[0], Action::Abort { reason } if reason.contains("at synthesis")));
    }

    #[test]
    fn a_partial_run_says_so_first() {
        let mut s = state();
        s.transcript = vec![
            turn(Some(1), Phase::Positions, "Q1: AGREE", Outcome::Answered),
            turn(Some(2), Phase::Positions, "Q1: AGREE", Outcome::Answered),
            turn(Some(3), Phase::Positions, "Rate limited by the provider.", Outcome::Refused),
            turn(Some(1), Phase::Critique, "AGREE: yes", Outcome::Answered),
            turn(Some(2), Phase::Critique, "AGREE: yes", Outcome::Answered),
            turn(Some(4), Phase::Arbitration, "## Verdict\nQ1: APPROVED", Outcome::Answered),
            turn(Some(4), Phase::Synthesis, "Fine. Q1 approved.", Outcome::Answered),
        ];
        let Action::Complete { findings, .. } = &next_action(&s)[0] else { panic!() };
        assert!(findings.starts_with("> **PARTIAL RUN — 2 of 3 members completed.**"));
        assert!(findings.contains("> - Gamma refused at **positions** (round 0): Rate limited by the provider."));
    }
}
