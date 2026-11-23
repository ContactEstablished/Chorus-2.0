//! Council (P10, D136–D138): a panel of models reviews a brief's numbered questions. Members
//! answer blind, critique each other anonymously, and an arbiter rules and writes the findings.
//! Every participant can read the project's code through read-only tools (D137).
//!
//! Ported from Chorus 1's `councilCore.ts`, keeping its protocol, its verdict vocabularies and its
//! dissent guarantee. Pure: the transcript is the only memory, time and ids arrive as data.

pub mod brief;
pub mod dissent;
pub mod files;
pub mod findings;
pub mod local;
pub mod prompts;
pub mod protocol;
pub mod tools;
pub mod verdict;

use serde::{Deserialize, Serialize};
use specta::Type;

/// The four phases, and `Done`. Each phase is asked once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    Positions,
    Critique,
    Arbitration,
    Synthesis,
    Done,
}

impl Phase {
    pub fn key(self) -> &'static str {
        match self {
            Self::Positions => "positions",
            Self::Critique => "critique",
            Self::Arbitration => "arbitration",
            Self::Synthesis => "synthesis",
            Self::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Positions, Self::Critique, Self::Arbitration, Self::Synthesis, Self::Done].into_iter().find(|p| p.key() == s)
    }

    /// The round number Chorus 1 labelled each phase with.
    pub fn round(self) -> u32 {
        match self {
            Self::Positions => 0,
            Self::Critique => 1,
            Self::Arbitration => 2,
            Self::Synthesis => 3,
            Self::Done => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum CouncilRole {
    Member,
    Arbiter,
}

impl CouncilRole {
    pub fn key(self) -> &'static str {
        match self {
            Self::Member => "member",
            Self::Arbiter => "arbiter",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "member" => Some(Self::Member),
            "arbiter" => Some(Self::Arbiter),
            _ => None,
        }
    }
}

/// How a turn ended. A refusal is a result, not an absence: it is stored like any other turn so a
/// partial council reads as partial. A cancelled turn is neither an answer nor the model's refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum TurnOutcome {
    Answered,
    Refused,
    Cancelled,
}

impl TurnOutcome {
    pub fn key(self) -> &'static str {
        match self {
            Self::Answered => "answered",
            Self::Refused => "refused",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "answered" => Some(Self::Answered),
            "refused" => Some(Self::Refused),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

/// A participant as the run planned it: its label and model are copied in at run start, so a
/// member renamed or deleted since still reads correctly in the run's own record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Participant {
    pub id: u32,
    pub label: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedRun {
    /// Role `member`, at least two.
    pub members: Vec<Participant>,
    pub arbiter: Participant,
    pub brief_text: String,
}

impl PlannedRun {
    pub fn label_for(&self, id: u32) -> String {
        self.members
            .iter()
            .chain(std::iter::once(&self.arbiter))
            .find(|m| m.id == id)
            .map_or_else(|| format!("member {id}"), |m| m.label.clone())
    }

    pub fn participants(&self) -> impl Iterator<Item = &Participant> {
        self.members.iter().chain(std::iter::once(&self.arbiter))
    }
}

/// One stored turn as the protocol sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    /// `None` for an orchestrator note (a run-level refusal).
    pub member_id: Option<u32>,
    pub phase: Phase,
    pub content: String,
    pub outcome: TurnOutcome,
}

#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub run: PlannedRun,
    pub transcript: Vec<Turn>,
    pub cancelled: bool,
    pub run_id: u32,
    /// The project whose code the participants read.
    pub project_name: String,
    /// ISO-8601, from the service's clock.
    pub started_at: String,
}

impl State {
    pub fn answered_in(&self, phase: Phase) -> Vec<(u32, &str)> {
        self.transcript
            .iter()
            .filter(|t| t.phase == phase && t.outcome == TurnOutcome::Answered)
            .filter_map(|t| t.member_id.map(|id| (id, t.content.as_str())))
            .collect()
    }

    pub fn has_turn(&self, phase: Phase) -> bool {
        self.transcript.iter().any(|t| t.phase == phase)
    }
}

/// What the service does next.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Ask { member_id: u32, phase: Phase, prompt: String },
    Complete { summary: Vec<verdict::QuestionSummary>, findings: String },
    Abort { reason: String },
}

/// Fewer than two members and an arbiter is a review, not a council.
pub const MIN_MEMBERS: usize = 2;
/// Participants per run, the arbiter included (Chorus 1's limit).
pub const MAX_PARTICIPANTS: usize = 12;
/// A brief is read by every participant in every round.
pub const MAX_BRIEF_BYTES: usize = 200_000;
