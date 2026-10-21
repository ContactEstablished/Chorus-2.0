//! Launch presets (SCOPE: Solo, Pair, Workbench, Swarm): how many sessions one Start makes and
//! which agent runs in each. Ported from Chorus 1's `launchPresets.ts` (D186 there). A preset is
//! the shape of the launch; a saved launch profile is the agent's settings. The two stay separate.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::agent::AgentKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Preset {
    /// One agent.
    Solo,
    /// One agent builds, a second agent reviews the same folder.
    Pair,
    /// An agent plus a shell in the same folder.
    Workbench,
    /// Several agents of one kind, each in its own worktree (D53).
    Swarm,
}

/// What a session in a launch is for. A label, not a limit: a reviewer can write files too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Builder,
    Reviewer,
    Shell,
}

pub const PRESETS: [Preset; 4] = [Preset::Solo, Preset::Pair, Preset::Workbench, Preset::Swarm];

/// How many agents a Swarm can start (Chorus 1 offered up to six).
pub const SWARM_MIN: u8 = 2;
pub const SWARM_MAX: u8 = 6;

impl Preset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Solo => "Solo",
            Self::Pair => "Pair",
            Self::Workbench => "Workbench",
            Self::Swarm => "Swarm",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Self::Solo => "One agent in one terminal",
            Self::Pair => "One agent builds, another reviews the same folder",
            Self::Workbench => "An agent plus a shell in the same folder",
            Self::Swarm => "Agents fan out on parallel work, each in its own worktree",
        }
    }

    /// The sentence shown when the preset is chosen: what it doesn't do that a user might assume.
    pub fn note(self) -> Option<&'static str> {
        match self {
            Self::Pair => Some("Both agents can write the same files: the reviewer is a role, not a sandbox."),
            Self::Workbench => Some("The shell is a plain terminal: Chorus never hands it a key."),
            // New session says which branch each worktree starts from.
            Self::Solo | Self::Swarm => None,
        }
    }
}

/// Who reviews, in order of preference: each agent's [`crate::agent::Spec::partner_rank`]. A list
/// rather than "any other agent", so the shell (which declares no rank) never becomes a reviewer by
/// accident, and adding an agent is one field rather than an edit here.
pub fn partner_order() -> Vec<AgentKind> {
    let mut ranked: Vec<(u8, AgentKind)> =
        AgentKind::ALL.iter().filter_map(|k| k.spec().partner_rank.map(|r| (r, *k))).collect();
    ranked.sort();
    ranked.into_iter().map(|(_, k)| k).collect()
}

/// The first agent in [`partner_order`] that isn't the builder and was found on this machine.
pub fn partner_for(builder: AgentKind, installed: &[AgentKind]) -> Option<AgentKind> {
    partner_order().into_iter().find(|k| *k != builder && installed.contains(k))
}

/// One session a launch makes. The first slot is the session the user set up; the others are
/// derived from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub agent: AgentKind,
    pub role: Option<Role>,
}

/// The sessions `preset` makes for `agent`, or why it can't. `count` is a Swarm's size.
pub fn plan(preset: Preset, agent: AgentKind, installed: &[AgentKind], count: u8) -> Result<Vec<Slot>, String> {
    let builder = Slot { agent, role: Some(Role::Builder) };
    match preset {
        Preset::Solo => Ok(vec![Slot { agent, role: None }]),
        _ if agent == AgentKind::Shell => Err(format!("{} needs an agent: choose one other than Shell.", preset.label())),
        Preset::Pair => match partner_for(agent, installed) {
            Some(partner) => Ok(vec![builder, Slot { agent: partner, role: Some(Role::Reviewer) }]),
            None => Err(format!("Pair needs a second agent CLI, and only {} was found.", agent.display_name())),
        },
        Preset::Workbench => Ok(vec![builder, Slot { agent: AgentKind::Shell, role: Some(Role::Shell) }]),
        Preset::Swarm if (SWARM_MIN..=SWARM_MAX).contains(&count) => Ok(vec![Slot { agent, role: None }; count as usize]),
        Preset::Swarm => Err(format!("A Swarm is {SWARM_MIN} to {SWARM_MAX} agents.")),
    }
}

/// A derived session's name: `<name> · review`.
pub fn slot_name(base: &str, role: Option<Role>) -> String {
    match role {
        Some(Role::Reviewer) => format!("{base} · review"),
        Some(Role::Shell) => format!("{base} · shell"),
        Some(Role::Builder) | None => base.to_owned(),
    }
}

/// A derived session's note, saying what it is for.
pub fn slot_description(base: &str, role: Option<Role>) -> Option<String> {
    match role {
        Some(Role::Reviewer) => Some(format!("Reviews the same folder as {base}. It can write files too.")),
        Some(Role::Shell) => Some(format!("A shell in the same folder as {base}.")),
        _ => None,
    }
}

pub const MAX_DESCRIPTION_CHARS: usize = 500;

/// A session's note: trimmed, at most [`MAX_DESCRIPTION_CHARS`], no control characters but line
/// breaks; empty means none.
pub fn clean_description(text: Option<&str>) -> Result<Option<String>, String> {
    let Some(t) = text.map(|t| t.trim().replace("\r\n", "\n")).filter(|t| !t.is_empty()) else { return Ok(None) };
    if t.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(format!("A note is up to {MAX_DESCRIPTION_CHARS} characters."));
    }
    if t.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return Err("A note can't hold control characters.".into());
    }
    Ok(Some(t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use AgentKind::*;

    #[test]
    fn solo_is_one_session() {
        assert_eq!(plan(Preset::Solo, Codex, &[], 1).unwrap(), [Slot { agent: Codex, role: None }]);
        assert_eq!(plan(Preset::Solo, Shell, &[], 1).unwrap().len(), 1);
    }

    #[test]
    fn partners_follow_their_declared_rank() {
        assert_eq!(partner_order().first(), Some(&Claude));
        assert!(!partner_order().contains(&Shell), "the shell is never a reviewer");
        assert_eq!(partner_for(Claude, &[Shell, Claude]), None);
        assert_eq!(partner_for(Codex, &[Codex, Shell]), None);
        // An agent without a channel comes before Grok, which has none either.
        let order = partner_order();
        let at = |k| order.iter().position(|x| *x == k).unwrap();
        assert!(at(Gemini) < at(Aider) && at(Aider) < at(Grok));
        assert_eq!(partner_for(Claude, &[Shell, Claude, Gemini]), Some(Gemini));
        assert_eq!(partner_for(Claude, &[Shell, Claude, Aider]), Some(Aider));
    }

    #[test]
    fn pair_finds_a_reviewer_in_order() {
        let all = [Shell, Claude, Codex, Opencode, Grok];
        let reviewer = |builder| plan(Preset::Pair, builder, &all, 1).unwrap()[1].agent;
        assert_eq!((reviewer(Codex), reviewer(Claude), reviewer(Opencode)), (Claude, Codex, Claude));
        // Codex not installed: Claude's reviewer is the next in order.
        assert_eq!(partner_for(Claude, &[Shell, Claude, Opencode]), Some(Opencode));
        assert!(plan(Preset::Pair, Claude, &[Shell, Claude], 1).unwrap_err().contains("only Claude Code was found"));
        assert!(plan(Preset::Pair, Shell, &all, 1).unwrap_err().contains("other than Shell"));
    }

    #[test]
    fn workbench_adds_a_shell_and_swarm_repeats_the_agent() {
        assert_eq!(
            plan(Preset::Workbench, Claude, &[], 1).unwrap(),
            [Slot { agent: Claude, role: Some(Role::Builder) }, Slot { agent: Shell, role: Some(Role::Shell) }]
        );
        assert_eq!(plan(Preset::Swarm, Codex, &[Codex], 3).unwrap(), vec![Slot { agent: Codex, role: None }; 3]);
        assert!(plan(Preset::Swarm, Codex, &[Codex], 1).is_err() && plan(Preset::Swarm, Codex, &[Codex], 7).is_err());
        assert!(plan(Preset::Swarm, Shell, &[], 2).unwrap_err().contains("other than Shell"));
        assert!(Preset::Pair.note().unwrap().contains("not a sandbox"));
    }

    #[test]
    fn derived_sessions_are_named_and_described() {
        assert_eq!(slot_name("Fix login", Some(Role::Reviewer)), "Fix login · review");
        assert_eq!(slot_name("Fix login", None), "Fix login");
        assert!(slot_description("Fix login", Some(Role::Shell)).unwrap().contains("Fix login"));
        assert_eq!(slot_description("x", Some(Role::Builder)), None);
    }

    #[test]
    fn notes_are_checked() {
        assert_eq!(clean_description(Some("  ")).unwrap(), None);
        assert_eq!(clean_description(Some(" a\r\nb ")).unwrap().as_deref(), Some("a\nb"));
        assert!(clean_description(Some(&"x".repeat(501))).is_err());
        assert!(clean_description(Some("a\u{1b}[31m")).is_err());
    }
}
