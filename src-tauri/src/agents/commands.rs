//! What the launch form needs about each agent: the options it declares, with the exact
//! arguments each one adds, and what Chorus found when it looked for the CLI.

use std::sync::Arc;

use chorus_core::agent::{AgentKind, Carrier, Effort, LaunchOptions, Permission, ResumeKind, Spec};
use serde::Serialize;
use specta::Type;
use tauri::State;

use super::detect::Detection;
use crate::sessions::SessionManager;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct EffortChoice {
    pub effort: Effort,
    /// The arguments it adds, as typed on a command line, or for an agent with
    /// `optionsInConfig`, the configuration key and value.
    pub args: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PermissionChoice {
    pub permission: Permission,
    pub args: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub kind: AgentKind,
    pub display_name: String,
    /// Empty when the agent has no setting for it; the form then leaves the control out and says
    /// why.
    pub efforts: Vec<EffortChoice>,
    pub default_effort: Option<Effort>,
    pub permissions: Vec<PermissionChoice>,
    pub default_permission: Option<Permission>,
    pub takes_model: bool,
    /// Options go into the configuration of a server Chorus starts (opencode), not argv.
    pub options_in_config: bool,
    /// The agent reports turns to Chorus. Without it, a running session is only "Running" (D47).
    pub has_event_channel: bool,
    /// An effort applies only with a model named beside it (opencode, LESSONS L5).
    pub effort_needs_model: bool,
    pub resume: ResumeKind,
    /// The CLI version the options were checked against.
    pub verified_version: Option<String>,
    /// None until the first detection finishes.
    pub detection: Option<Detection>,
}

fn info(spec: &Spec, detection: Option<Detection>) -> AgentInfo {
    AgentInfo {
        kind: spec.kind,
        display_name: spec.kind.display_name().to_owned(),
        efforts: spec.efforts.iter().map(|(e, a)| EffortChoice { effort: *e, args: a.join(" ") }).collect(),
        default_effort: spec.default_effort,
        permissions: spec.permissions.iter().map(|(p, a)| PermissionChoice { permission: *p, args: a.join(" ") }).collect(),
        default_permission: spec.default_permission,
        takes_model: spec.model.is_some(),
        options_in_config: spec.carrier == Carrier::ServerConfig,
        has_event_channel: spec.has_event_channel,
        effort_needs_model: spec.carrier == Carrier::ServerConfig,
        resume: spec.resume.kind(),
        verified_version: spec.verified.map(str::to_owned),
        detection,
    }
}

/// Every agent and what it declares. With `refresh`, looks for each CLI again first (a second or
/// two); otherwise returns what the last detection found.
#[tauri::command]
#[specta::specta]
pub async fn agents_list(state: State<'_, Arc<SessionManager>>, refresh: bool) -> Result<Vec<AgentInfo>, String> {
    let m = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        if refresh {
            m.refresh_agents();
        }
        AgentKind::ALL.iter().map(|k| info(k.spec(), m.agents().get(*k))).collect()
    })
    .await
    .map_err(|e| e.to_string())
}

/// The arguments a launch of `agent` with `options` adds, from the same functions the launch uses:
/// New session's summary shows them. For an agent with `optionsInConfig`, the server configuration
/// entries instead, as `key=value`. A provider adds its own on top; `on_provider` says the model is
/// that provider's own id.
#[tauri::command]
#[specta::specta]
pub fn launch_args(agent: AgentKind, options: LaunchOptions, on_provider: bool) -> Result<Vec<String>, String> {
    let spec = agent.spec();
    let resolved = spec.resolve_on(&options, on_provider)?;
    Ok(match spec.carrier {
        Carrier::ServerConfig => chorus_core::opencode::summary_entries(spec, &resolved),
        Carrier::Argv => spec.option_args(&resolved),
    })
}
