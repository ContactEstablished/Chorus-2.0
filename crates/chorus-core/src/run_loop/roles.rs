//! Which Bot fills each loop role for a step (P15, the spec's "Assigning models"): a task's override
//! in `tasks.md`, then the phase's in `roadmap.md`, then the run's role default. Overrides name a
//! Bot, compared without case, spaces or hyphens. Whatever the overrides say, a reviewer is never
//! from the same vendor as the model that built the task.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::agent::AgentKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum LoopRole {
    Planner,
    PlanGriller,
    Builder,
    Mechanic,
    Reviewer,
    Reporter,
    Arbitrator,
}

impl LoopRole {
    pub const ALL: [Self; 7] =
        [Self::Planner, Self::PlanGriller, Self::Builder, Self::Mechanic, Self::Reviewer, Self::Reporter, Self::Arbitrator];

    /// The name `roadmap.md` overrides use: `builder`, `plan_griller`…
    pub fn key(self) -> &'static str {
        match self {
            Self::Planner => "planner",
            Self::PlanGriller => "plan_griller",
            Self::Builder => "builder",
            Self::Mechanic => "mechanic",
            Self::Reviewer => "reviewer",
            Self::Reporter => "reporter",
            Self::Arbitrator => "arbitrator",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Planner => "Phase planner",
            Self::PlanGriller => "Plan griller",
            Self::Builder => "Builder",
            Self::Mechanic => "Builder, mechanical tasks",
            Self::Reviewer => "Reviewer",
            Self::Reporter => "Reporter",
            Self::Arbitrator => "Counsel arbitrator",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        Self::ALL.into_iter().find(|r| r.key() == s)
    }
}

/// A role's default Bot and its fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RoleSetting {
    pub role: LoopRole,
    pub bot: u32,
    pub fallback: Option<u32>,
}

/// What a Bot is, for routing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotFacts {
    pub id: u32,
    pub name: String,
    pub agent: AgentKind,
    /// The model it launches with, in the CLI's own form (`opus`, `deepseek/deepseek-v4.1-flash`).
    pub model: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    Task,
    Phase,
    Default,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolved {
    pub bot: u32,
    pub source: Source,
}

fn same_name(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.chars().filter(|c| !matches!(c, ' ' | '-' | '_')).flat_map(char::to_lowercase).collect::<String>();
    norm(a) == norm(b)
}

fn named(bots: &[BotFacts], name: &str, whose: &str) -> Result<u32, String> {
    bots.iter().find(|b| same_name(&b.name, name)).map(|b| b.id).ok_or_else(|| format!("{whose} names a Bot {name:?}, and there is none by that name."))
}

/// The Bot for `role` in a step. `task_model` is the task's `- Model:` line, which overrides the
/// builder roles only; `phase_models` are the phase's `role=bot` pairs.
pub fn resolve(
    role: LoopRole,
    task_model: Option<&str>,
    phase_models: &[(String, String)],
    defaults: &[RoleSetting],
    bots: &[BotFacts],
) -> Result<Resolved, String> {
    if matches!(role, LoopRole::Builder | LoopRole::Mechanic)
        && let Some(name) = task_model
    {
        return named(bots, name, "The task's Model line").map(|bot| Resolved { bot, source: Source::Task });
    }
    if let Some((_, name)) = phase_models.iter().find(|(r, _)| LoopRole::parse(r) == Some(role)) {
        return named(bots, name, "The phase's Models line").map(|bot| Resolved { bot, source: Source::Phase });
    }
    defaults
        .iter()
        .find(|d| d.role == role)
        .map(|d| Resolved { bot: d.bot, source: Source::Default })
        .ok_or_else(|| format!("No Bot is set for the {} role.", role.label()))
}

/// Roles a phase override names that aren't roles, for the roadmap check.
pub fn unknown_roles(phase_models: &[(String, String)]) -> Vec<String> {
    phase_models.iter().filter(|(r, _)| LoopRole::parse(r).is_none()).map(|(r, _)| r.clone()).collect()
}

/// The company whose model a Bot runs: the `vendor/` prefix of a routed model id, else the agent's
/// own. `None` when it can't be told (a shell, or an opencode Bot with no model).
pub fn vendor(bot: &BotFacts) -> Option<String> {
    if let Some((v, _)) = bot.model.as_deref().and_then(|m| m.split_once('/')) {
        let v = v.trim().to_ascii_lowercase();
        return Some(match v.as_str() {
            "anthropic" => "anthropic".into(),
            "openai" => "openai".into(),
            "google" => "google".into(),
            "x-ai" | "xai" => "xai".into(),
            _ => v,
        });
    }
    match bot.agent {
        AgentKind::Claude => Some("anthropic".into()),
        AgentKind::Codex => Some("openai".into()),
        AgentKind::Gemini => Some("google".into()),
        AgentKind::Grok => Some("xai".into()),
        AgentKind::Shell | AgentKind::Opencode | AgentKind::Aider => None,
    }
}

/// Whether a model id names an OpenAI or Anthropic model. D99: those run only on Matthew's
/// subscriptions, through Codex and Claude Code, never through OpenRouter.
pub fn gpt_or_claude(model: &str) -> bool {
    let m = model.trim().trim_start_matches('~').to_ascii_lowercase();
    let name = m.rsplit('/').next().unwrap_or(&m);
    m.starts_with("openai/") || m.starts_with("anthropic/") || name.starts_with("gpt") || name.starts_with("claude")
}

/// The reviewer for a task built by `builder`: the reviewer role's Bot, or its fallback when the
/// first shares the builder's vendor. An unknown vendor never counts as a match.
pub fn reviewer_for(builder: &BotFacts, reviewer: &BotFacts, fallback: Option<&BotFacts>) -> Result<u32, String> {
    let clash = |r: &BotFacts| vendor(r).is_some() && vendor(r) == vendor(builder);
    if !clash(reviewer) {
        return Ok(reviewer.id);
    }
    match fallback {
        Some(f) if !clash(f) => Ok(f.id),
        _ => Err(format!(
            "{} built this task, and the reviewer ({}) and its fallback are from the same vendor. Choose a reviewer from another vendor.",
            builder.name, reviewer.name
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bot(id: u32, name: &str, agent: AgentKind, model: Option<&str>) -> BotFacts {
        BotFacts { id, name: name.into(), agent, model: model.map(str::to_owned) }
    }

    fn bots() -> Vec<BotFacts> {
        vec![
            bot(1, "Claude Opus", AgentKind::Claude, Some("opus")),
            bot(2, "Sol", AgentKind::Codex, Some("gpt-6.1-sol")),
            bot(3, "DeepSeek Flash", AgentKind::Opencode, Some("deepseek/deepseek-v4.1-flash")),
        ]
    }

    const DEFAULTS: [RoleSetting; 3] = [
        RoleSetting { role: LoopRole::Planner, bot: 1, fallback: Some(2) },
        RoleSetting { role: LoopRole::Builder, bot: 1, fallback: Some(3) },
        RoleSetting { role: LoopRole::Reviewer, bot: 2, fallback: Some(3) },
    ];

    #[test]
    fn the_most_specific_setting_wins() {
        let phase = vec![("builder".to_owned(), "sol".to_owned())];
        let b = bots();
        assert_eq!(resolve(LoopRole::Builder, Some("deepseek-flash"), &phase, &DEFAULTS, &b), Ok(Resolved { bot: 3, source: Source::Task }));
        assert_eq!(resolve(LoopRole::Builder, None, &phase, &DEFAULTS, &b), Ok(Resolved { bot: 2, source: Source::Phase }));
        assert_eq!(resolve(LoopRole::Planner, Some("sol"), &phase, &DEFAULTS, &b), Ok(Resolved { bot: 1, source: Source::Default }));
        assert!(resolve(LoopRole::Builder, Some("gemini"), &phase, &DEFAULTS, &b).unwrap_err().contains("\"gemini\""));
        assert!(resolve(LoopRole::Reporter, None, &[], &DEFAULTS, &b).unwrap_err().contains("Reporter"));
    }

    #[test]
    fn phase_roles_are_read_loosely() {
        let phase = vec![("Plan-Griller".to_owned(), "Claude Opus".to_owned()), ("chef".to_owned(), "x".to_owned())];
        let d = [RoleSetting { role: LoopRole::PlanGriller, bot: 2, fallback: None }];
        assert_eq!(resolve(LoopRole::PlanGriller, None, &phase, &d, &bots()).unwrap().bot, 1);
        assert_eq!(unknown_roles(&phase), ["chef"]);
    }

    #[test]
    fn vendors() {
        let b = bots();
        assert_eq!(vendor(&b[0]).as_deref(), Some("anthropic"));
        assert_eq!(vendor(&b[1]).as_deref(), Some("openai"));
        assert_eq!(vendor(&b[2]).as_deref(), Some("deepseek"));
        assert_eq!(vendor(&bot(4, "x", AgentKind::Opencode, Some("openai/gpt-6.1-sol"))).as_deref(), Some("openai"));
        assert_eq!(vendor(&bot(5, "x", AgentKind::Opencode, None)), None);
    }

    #[test]
    fn gpt_and_claude_are_spotted() {
        for m in ["openai/gpt-6.1-sol", "~anthropic/claude-sonnet-latest", "anthropic/claude-opus-5-5", "gpt-6.1-sol", "x/claude-fork"] {
            assert!(gpt_or_claude(m), "{m}");
        }
        for m in ["~z-ai/glm-flash-latest", "~deepseek/deepseek-v4-flash-latest", "~moonshotai/kimi-latest", "deepseek/deepseek-v4.1-flash"] {
            assert!(!gpt_or_claude(m), "{m}");
        }
    }

    #[test]
    fn the_reviewer_is_never_the_builders_vendor() {
        let b = bots();
        // Claude built it: Sol reviews.
        assert_eq!(reviewer_for(&b[0], &b[1], Some(&b[2])), Ok(2));
        // Sol built it as a fallback: the fallback reviewer takes over.
        assert_eq!(reviewer_for(&b[1], &b[1], Some(&b[2])), Ok(3));
        // Nothing from another vendor: refused, with the reason.
        assert!(reviewer_for(&b[1], &b[1], None).unwrap_err().contains("same vendor"));
    }
}
