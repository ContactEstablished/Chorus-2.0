//! Saved launch profiles: a named agent, options, provider and key-as-variable that fill the launch
//! form. Saving checks them the way a launch would, so a profile can't hold an option its agent
//! lacks. They grow into Bots in P8.

use chorus_core::agent::{AgentKind, LaunchOptions};
use chorus_core::session::clean_name;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::commands::Manager;
use super::{CredentialUse, SessionManager};
use crate::store::profiles::{self as rows, ProfileInput, ProfileRow};
use crate::store::providers as provider_rows;
use crate::vault::commands::secs;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    pub id: u32,
    pub name: String,
    pub agent: AgentKind,
    pub options: LaunchOptions,
    pub provider_id: Option<u32>,
    /// `(deleted provider)` when it no longer exists: using the profile then says so.
    pub provider_name: Option<String>,
    pub credential: Option<CredentialUse>,
    pub updated_at_secs: u32,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSave {
    /// `None` adds one.
    pub id: Option<u32>,
    pub name: String,
    pub agent: AgentKind,
    pub options: LaunchOptions,
    pub provider_id: Option<u32>,
    pub credential: Option<CredentialUse>,
}

fn summary(m: &SessionManager, r: &ProfileRow) -> ProfileSummary {
    ProfileSummary {
        id: r.id,
        name: r.name.clone(),
        agent: r.agent,
        options: r.options.clone(),
        provider_id: r.provider_id,
        provider_name: r.provider_id.map(|id| {
            provider_rows::get(m.store(), id).ok().flatten().map_or_else(|| "(deleted provider)".to_owned(), |p| p.name)
        }),
        credential: match (r.credential_id, &r.credential_env) {
            (Some(id), Some(env_var)) => Some(CredentialUse { id, env_var: env_var.clone() }),
            _ => None,
        },
        updated_at_secs: secs(r.updated_at),
    }
}

#[tauri::command]
#[specta::specta]
pub fn profiles_list(state: Manager<'_>) -> Result<Vec<ProfileSummary>, String> {
    Ok(rows::list(state.store())?.iter().map(|r| summary(&state, r)).collect())
}

#[tauri::command]
#[specta::specta]
pub fn profile_save(state: Manager<'_>, input: ProfileSave) -> Result<ProfileSummary, String> {
    if input.name.trim().is_empty() {
        return Err("A profile needs a name.".into());
    }
    let name = clean_name(Some(&input.name), "")?;
    // Stored as chosen, not with defaults filled in, so a later default change still reaches it.
    state.check_settings(input.agent, &input.options, input.provider_id, input.credential.as_ref())?;
    let fields = ProfileInput {
        name: &name,
        agent: input.agent,
        options: &input.options,
        provider_id: input.provider_id,
        credential_id: input.credential.as_ref().map(|c| c.id),
        credential_env: input.credential.as_ref().map(|c| c.env_var.as_str()),
    };
    let id = match input.id {
        Some(id) => {
            rows::get(state.store(), id)?.ok_or("That profile no longer exists.")?;
            rows::update(state.store(), id, &fields)?;
            id
        }
        None => rows::insert(state.store(), &fields)?,
    };
    let row = rows::get(state.store(), id)?.ok_or("That profile no longer exists.")?;
    Ok(summary(&state, &row))
}

#[tauri::command]
#[specta::specta]
pub fn profile_delete(state: Manager<'_>, id: u32) -> Result<bool, String> {
    rows::delete(state.store(), id)
}
