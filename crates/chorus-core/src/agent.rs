//! The agents Chorus launches, the options each one declares, and the arguments that carry them
//! (PLAN P2, ARCHITECTURE §5). Ported from Chorus 1's adapters and `argLevels.ts`, and checked
//! against each CLI's own `--help` at the version in [`Spec::verified`].
//!
//! The declaration and the arguments live in one table per agent, so they can't disagree. An
//! option an agent doesn't declare is refused with a reason, never dropped (D19). Whether an
//! option really takes effect is checked against the running agent (P2's exit), not here.

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Shell,
    Claude,
    Codex,
    Opencode,
    Grok,
}

impl AgentKind {
    pub const ALL: [AgentKind; 5] = [Self::Shell, Self::Claude, Self::Codex, Self::Opencode, Self::Grok];

    /// The stored and serialised name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Opencode => "opencode",
            Self::Grok => "grok",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    /// How the agent is named in the UI and in default session names.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Shell => "Shell",
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Opencode => "opencode",
            Self::Grok => "Grok",
        }
    }

    pub fn spec(self) -> &'static Spec {
        match self {
            Self::Shell => &SHELL,
            Self::Claude => &CLAUDE,
            Self::Codex => &CODEX,
            Self::Opencode => &OPENCODE,
            Self::Grok => &GROK,
        }
    }
}

/// One effort scale for every agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    Fast,
    Balanced,
    Deep,
    Max,
}

/// How much the agent may do without asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Permission {
    /// Asks before acting.
    Manual,
    /// The agent's own judgement of what is safe to do without asking.
    Auto,
    /// Edits files without asking.
    AcceptEdits,
    /// Plans only.
    Plan,
    /// No sandbox and no questions.
    FullAccess,
}

/// What a session asks for. An empty field means the agent's declared default, or for the model,
/// the CLI's own choice.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LaunchOptions {
    pub effort: Option<Effort>,
    pub permission: Option<Permission>,
    /// A model name in the form the CLI takes (`opus`, `gpt-5.5`).
    pub model: Option<String>,
}

/// How a session's next run picks up the agent's conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum ResumeKind {
    /// Nothing to resume.
    None,
    /// Chorus chooses the conversation id at launch and passes it back to resume.
    Assigned,
    /// The agent chooses the id; Chorus finds it in the agent's files after launch.
    Discovered,
    /// The conversation lives in a server Chorus runs for the session.
    Server,
}

#[derive(Debug)]
pub enum Resume {
    None,
    /// `create <id>` starts a conversation with that id; `resume <id>` continues it.
    Assigned { create: &'static str, resume: &'static str },
    /// `subcommand <id>` continues a conversation the agent named.
    Discovered { subcommand: &'static str },
    Server,
}

impl Resume {
    pub fn kind(&self) -> ResumeKind {
        match self {
            Self::None => ResumeKind::None,
            Self::Assigned { .. } => ResumeKind::Assigned,
            Self::Discovered { .. } => ResumeKind::Discovered,
            Self::Server => ResumeKind::Server,
        }
    }
}

/// How the model name is passed.
#[derive(Debug)]
pub enum ModelArg {
    /// `flag <name>`.
    Flag(&'static str),
    /// `-c <key>="<name>"`, Codex's config override.
    CodexConfig(&'static str),
}

/// What an agent declares, and the arguments behind each declared option.
#[derive(Debug)]
pub struct Spec {
    pub kind: AgentKind,
    /// The CLI version these arguments were checked against.
    pub verified: Option<&'static str>,
    pub efforts: &'static [(Effort, &'static [&'static str])],
    pub default_effort: Option<Effort>,
    pub permissions: &'static [(Permission, &'static [&'static str])],
    pub default_permission: Option<Permission>,
    pub model: Option<ModelArg>,
    pub resume: Resume,
}

const SHELL: Spec = Spec {
    kind: AgentKind::Shell,
    verified: None,
    efforts: &[],
    default_effort: None,
    permissions: &[],
    default_permission: None,
    model: None,
    resume: Resume::None,
};

/// Claude Code 2.1.300: `--effort` (low, medium, high, xhigh, max) and `--permission-mode`. Chorus 1's
/// mapping, which leaves out `low`, and leaves out `bypassPermissions` and `dontAsk` on purpose.
const CLAUDE: Spec = Spec {
    kind: AgentKind::Claude,
    verified: Some("2.1.300"),
    efforts: &[
        (Effort::Fast, &["--effort", "medium"]),
        (Effort::Balanced, &["--effort", "high"]),
        (Effort::Deep, &["--effort", "xhigh"]),
        (Effort::Max, &["--effort", "max"]),
    ],
    default_effort: Some(Effort::Deep),
    permissions: &[
        (Permission::Manual, &["--permission-mode", "manual"]),
        (Permission::Auto, &["--permission-mode", "auto"]),
        (Permission::AcceptEdits, &["--permission-mode", "acceptEdits"]),
        (Permission::Plan, &["--permission-mode", "plan"]),
    ],
    default_permission: Some(Permission::Auto),
    model: Some(ModelArg::Flag("--model")),
    resume: Resume::Assigned { create: "--session-id", resume: "--resume" },
};

/// Codex 0.162.1. Options travel as `-c` overrides: Chorus 1 measured that only `-c` survives the
/// `resume` subcommand. Its binary accepts `minimal` to `max` for the effort. Both permission knobs
/// are always set together.
const CODEX: Spec = Spec {
    kind: AgentKind::Codex,
    verified: Some("0.162.1"),
    efforts: &[
        (Effort::Fast, &["-c", "model_reasoning_effort=\"low\""]),
        (Effort::Balanced, &["-c", "model_reasoning_effort=\"medium\""]),
        (Effort::Deep, &["-c", "model_reasoning_effort=\"high\""]),
        (Effort::Max, &["-c", "model_reasoning_effort=\"max\""]),
    ],
    default_effort: None,
    permissions: &[
        (Permission::Manual, &["-c", "sandbox_mode=\"read-only\"", "-c", "approval_policy=\"on-request\""]),
        (Permission::Auto, &["-c", "sandbox_mode=\"workspace-write\"", "-c", "approval_policy=\"on-request\""]),
        (Permission::AcceptEdits, &["-c", "sandbox_mode=\"workspace-write\"", "-c", "approval_policy=\"never\""]),
        (Permission::FullAccess, &["-c", "sandbox_mode=\"danger-full-access\"", "-c", "approval_policy=\"never\""]),
    ],
    // Chorus 1's default.
    default_permission: Some(Permission::FullAccess),
    model: Some(ModelArg::CodexConfig("model")),
    resume: Resume::Discovered { subcommand: "resume" },
};

/// opencode runs as a server plus an attached TUI; its options are set through the server.
const OPENCODE: Spec = Spec {
    kind: AgentKind::Opencode,
    verified: Some("1.19.3"),
    efforts: &[],
    default_effort: None,
    permissions: &[],
    default_permission: None,
    model: None,
    resume: Resume::Server,
};

/// Grok 1.0.62: `--reasoning-effort` and `--permission-mode`. A permission mode is always passed,
/// because a user's config can set `always-approve` (LESSONS L26). Chorus 1 found that `xhigh`
/// makes `grok-4.5` exit at launch, so Max may fail for some models.
const GROK: Spec = Spec {
    kind: AgentKind::Grok,
    verified: Some("1.0.62"),
    efforts: &[
        (Effort::Fast, &["--reasoning-effort", "low"]),
        (Effort::Balanced, &["--reasoning-effort", "medium"]),
        (Effort::Deep, &["--reasoning-effort", "high"]),
        (Effort::Max, &["--reasoning-effort", "xhigh"]),
    ],
    default_effort: Some(Effort::Deep),
    permissions: &[
        (Permission::Manual, &["--permission-mode", "default"]),
        (Permission::Auto, &["--permission-mode", "auto"]),
        (Permission::AcceptEdits, &["--permission-mode", "acceptEdits"]),
        (Permission::Plan, &["--permission-mode", "plan"]),
    ],
    default_permission: Some(Permission::Auto),
    model: Some(ModelArg::Flag("-m")),
    resume: Resume::Assigned { create: "--session-id", resume: "--resume" },
};

pub const MAX_MODEL_CHARS: usize = 200;

/// A model name safe to pass as an argument and inside a TOML string: letters, digits and
/// `. _ : / @ ~ - [ ]` (Chorus 1's catalog rule, plus brackets for names like `opus[1m]`).
pub fn valid_model(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_MODEL_CHARS
        && !name.starts_with('-')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "._:/@~-[]".contains(c))
}

fn effort_name(e: Effort) -> &'static str {
    match e {
        Effort::Fast => "Fast",
        Effort::Balanced => "Balanced",
        Effort::Deep => "Deep",
        Effort::Max => "Max",
    }
}

fn permission_name(p: Permission) -> &'static str {
    match p {
        Permission::Manual => "Manual",
        Permission::Auto => "Auto",
        Permission::AcceptEdits => "Accept edits",
        Permission::Plan => "Plan",
        Permission::FullAccess => "Full access",
    }
}

impl Spec {
    pub fn effort_args(&self, e: Effort) -> Option<&'static [&'static str]> {
        self.efforts.iter().find(|(x, _)| *x == e).map(|(_, a)| *a)
    }

    pub fn permission_args(&self, p: Permission) -> Option<&'static [&'static str]> {
        self.permissions.iter().find(|(x, _)| *x == p).map(|(_, a)| *a)
    }

    /// Fills in the declared defaults and refuses anything this agent doesn't declare.
    pub fn resolve(&self, asked: &LaunchOptions) -> Result<LaunchOptions, String> {
        let name = self.kind.display_name();
        let effort = asked.effort.or(self.default_effort);
        if let Some(e) = effort
            && self.effort_args(e).is_none()
        {
            return Err(format!("{name} has no {} effort setting", effort_name(e)));
        }
        let permission = asked.permission.or(self.default_permission);
        if let Some(p) = permission
            && self.permission_args(p).is_none()
        {
            return Err(format!("{name} has no {} permission mode", permission_name(p)));
        }
        let model = match asked.model.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
            None => None,
            Some(_) if self.model.is_none() => return Err(format!("{name} doesn't take a model at launch")),
            Some(m) if !valid_model(m) => return Err(format!("{m:?} is not a model name Chorus can pass on")),
            Some(m) => Some(m.to_owned()),
        };
        Ok(LaunchOptions { effort, permission, model })
    }

    /// The arguments for options already passed through [`Spec::resolve`]: effort, permission,
    /// then model.
    pub fn option_args(&self, resolved: &LaunchOptions) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if let Some(a) = resolved.effort.and_then(|e| self.effort_args(e)) {
            out.extend(a.iter().map(|s| (*s).to_owned()));
        }
        if let Some(a) = resolved.permission.and_then(|p| self.permission_args(p)) {
            out.extend(a.iter().map(|s| (*s).to_owned()));
        }
        match (&self.model, resolved.model.as_deref().filter(|m| valid_model(m))) {
            (Some(ModelArg::Flag(flag)), Some(m)) => out.extend([(*flag).to_owned(), m.to_owned()]),
            (Some(ModelArg::CodexConfig(key)), Some(m)) => out.extend(["-c".to_owned(), format!("{key}=\"{m}\"")]),
            _ => {}
        }
        out
    }

    /// The arguments that start or continue the agent's conversation. `id` is the conversation id
    /// Chorus holds (chosen at launch for [`Resume::Assigned`], found later for
    /// [`Resume::Discovered`]); `exists` says the agent has recorded that conversation. Ids that
    /// aren't UUIDs are never passed: an empty `--resume` opens a picker or picks the latest.
    pub fn conversation_args(&self, id: Option<&str>, exists: bool) -> Vec<String> {
        let Some(id) = id.filter(|i| is_uuid(i)) else {
            return Vec::new();
        };
        match self.resume {
            Resume::Assigned { create, resume } => vec![(if exists { resume } else { create }).to_owned(), id.to_owned()],
            Resume::Discovered { subcommand } if exists => vec![subcommand.to_owned(), id.to_owned()],
            _ => Vec::new(),
        }
    }
}

/// A UUID in its 8-4-4-4-12 hex form.
pub fn is_uuid(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    groups.len() == 5
        && groups.iter().zip([8, 4, 4, 4, 12]).all(|(g, n)| g.len() == n && g.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// A version 4 UUID made from 16 random bytes.
pub fn uuid_v4(mut bytes: [u8; 16]) -> String {
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32])
}

#[cfg(test)]
mod tests {
    use super::*;

    const EFFORTS: [Effort; 4] = [Effort::Fast, Effort::Balanced, Effort::Deep, Effort::Max];
    const PERMISSIONS: [Permission; 5] =
        [Permission::Manual, Permission::Auto, Permission::AcceptEdits, Permission::Plan, Permission::FullAccess];

    #[test]
    fn names_round_trip() {
        for k in AgentKind::ALL {
            assert_eq!(AgentKind::parse(k.as_str()), Some(k));
            assert_eq!(k.spec().kind, k);
        }
        assert_eq!(AgentKind::parse("kimi"), None);
    }

    #[test]
    fn declarations_match_the_arguments() {
        for k in AgentKind::ALL {
            let s = k.spec();
            // Defaults are declared options.
            assert!(s.default_effort.is_none_or(|e| s.effort_args(e).is_some()), "{k:?} default effort");
            assert!(s.default_permission.is_none_or(|p| s.permission_args(p).is_some()), "{k:?} default permission");
            // Each declared level gives arguments of its own, declared once.
            let mut seen = Vec::new();
            for e in EFFORTS {
                if let Some(a) = s.effort_args(e) {
                    assert!(!a.is_empty() && !seen.contains(&a), "{k:?} {e:?}");
                    assert_eq!(s.efforts.iter().filter(|(x, _)| *x == e).count(), 1);
                    seen.push(a);
                }
            }
            let mut seen = Vec::new();
            for p in PERMISSIONS {
                if let Some(a) = s.permission_args(p) {
                    assert!(!a.is_empty() && !seen.contains(&a), "{k:?} {p:?}");
                    assert_eq!(s.permissions.iter().filter(|(x, _)| *x == p).count(), 1);
                    seen.push(a);
                }
                let asked = LaunchOptions { permission: Some(p), ..Default::default() };
                assert_eq!(s.resolve(&asked).is_ok(), s.permission_args(p).is_some(), "{k:?} {p:?}");
            }
            // Nothing chosen: only the declared defaults.
            let defaults = s.resolve(&LaunchOptions::default()).unwrap();
            assert_eq!((defaults.effort, defaults.permission), (s.default_effort, s.default_permission));
            let mut expected: Vec<String> = Vec::new();
            for a in [defaults.effort.and_then(|e| s.effort_args(e)), defaults.permission.and_then(|p| s.permission_args(p))]
                .into_iter()
                .flatten()
            {
                expected.extend(a.iter().map(|x| (*x).to_owned()));
            }
            assert_eq!(s.option_args(&defaults), expected, "{k:?}");
        }
    }

    #[test]
    fn options_an_agent_lacks_are_refused_with_a_reason() {
        let asked = LaunchOptions { effort: Some(Effort::Max), ..Default::default() };
        assert_eq!(AgentKind::Shell.spec().resolve(&asked).unwrap_err(), "Shell has no Max effort setting");
        let asked = LaunchOptions { permission: Some(Permission::FullAccess), ..Default::default() };
        assert!(AgentKind::Claude.spec().resolve(&asked).unwrap_err().contains("Full access"));
        let asked = LaunchOptions { model: Some("gpt-5.5".into()), ..Default::default() };
        assert!(AgentKind::Shell.spec().resolve(&asked).unwrap_err().contains("doesn't take a model"));
        let asked = LaunchOptions { model: Some("--dangerously-skip-permissions".into()), ..Default::default() };
        assert!(AgentKind::Claude.spec().resolve(&asked).is_err());
        // A blank model is no model.
        let asked = LaunchOptions { model: Some("  ".into()), ..Default::default() };
        assert_eq!(AgentKind::Claude.spec().resolve(&asked).unwrap().model, None);
    }

    #[test]
    fn each_agent_gets_its_own_flags() {
        let opts = |e, p, m: &str| LaunchOptions { effort: Some(e), permission: Some(p), model: Some(m.into()) };
        let claude = AgentKind::Claude.spec();
        assert_eq!(
            claude.option_args(&claude.resolve(&opts(Effort::Deep, Permission::Plan, "opus")).unwrap()),
            ["--effort", "xhigh", "--permission-mode", "plan", "--model", "opus"]
        );
        let codex = AgentKind::Codex.spec();
        assert_eq!(
            codex.option_args(&codex.resolve(&opts(Effort::Fast, Permission::Auto, "gpt-5.5")).unwrap()),
            [
                "-c",
                "model_reasoning_effort=\"low\"",
                "-c",
                "sandbox_mode=\"workspace-write\"",
                "-c",
                "approval_policy=\"on-request\"",
                "-c",
                "model=\"gpt-5.5\""
            ]
        );
        let grok = AgentKind::Grok.spec();
        assert_eq!(
            grok.option_args(&grok.resolve(&opts(Effort::Max, Permission::Manual, "grok-4.5")).unwrap()),
            ["--reasoning-effort", "xhigh", "--permission-mode", "default", "-m", "grok-4.5"]
        );
        // Grok always gets a permission mode, even when none is chosen.
        assert!(grok.option_args(&grok.resolve(&LaunchOptions::default()).unwrap()).contains(&"--permission-mode".to_owned()));
    }

    #[test]
    fn conversations_start_with_an_id_and_resume_it() {
        let id = "0f8fad5b-d9cb-469f-a165-70867728950e";
        let claude = AgentKind::Claude.spec();
        assert_eq!(claude.conversation_args(Some(id), false), ["--session-id", id]);
        assert_eq!(claude.conversation_args(Some(id), true), ["--resume", id]);
        let codex = AgentKind::Codex.spec();
        assert!(codex.conversation_args(None, false).is_empty());
        assert!(codex.conversation_args(Some(id), false).is_empty());
        assert_eq!(codex.conversation_args(Some(id), true), ["resume", id]);
        // Never an empty or malformed id: `--resume` alone opens a picker.
        assert!(claude.conversation_args(Some(""), true).is_empty());
        assert!(claude.conversation_args(Some("--last"), true).is_empty());
        assert!(AgentKind::Shell.spec().conversation_args(Some(id), true).is_empty());
    }

    #[test]
    fn uuids_are_version_4_and_recognised() {
        let id = uuid_v4([0xff; 16]);
        assert_eq!(id, "ffffffff-ffff-4fff-bfff-ffffffffffff");
        assert!(is_uuid(&id) && is_uuid(&uuid_v4([0; 16])));
        assert!(!is_uuid("ffffffff-ffff-4fff-bfff-fffffffffff") && !is_uuid("not-a-uuid") && !is_uuid(""));
    }

    #[test]
    fn model_names_are_limited_to_safe_characters() {
        for ok in ["opus", "opus[1m]", "gpt-5.5", "openrouter/x-ai/grok-4:free", "claude-opus-5-5@20261001"] {
            assert!(valid_model(ok), "{ok}");
        }
        for bad in ["", "-m", "a b", "a\"b", "a\\b", "a;b", &"x".repeat(201)] {
            assert!(!valid_model(bad), "{bad}");
        }
    }
}
