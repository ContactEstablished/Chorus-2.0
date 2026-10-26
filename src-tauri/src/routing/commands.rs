//! Routing commands for New session (D57) and Settings → Routing. Each tier command takes an
//! OpenRouter provider and the model's OpenRouter id, checked here (an existing provider, a model
//! slug); the effort is the launch's, which decides whether endpoints must support reasoning.

use std::sync::Arc;

use chorus_core::agent::{AgentKind, Effort, LaunchOptions};
use chorus_core::routing::types::RoutingSettings;
use tauri::State;

use super::{RefreshOutcome, Routing, RoutingSettingsView, TiersView};
use crate::sessions::SessionManager;

type Manager<'a> = State<'a, Arc<SessionManager>>;
type RoutingState<'a> = State<'a, Arc<Routing>>;

/// opencode's variant for `effort`, as a launch would send it.
pub fn effort_variant(effort: Option<Effort>) -> Option<String> {
    chorus_core::opencode::wanted_variant(AgentKind::Opencode.spec(), &LaunchOptions { effort, ..LaunchOptions::default() })
}

/// The tiers as stored for `model` with `provider_id`'s account checks: free, no request sent.
#[tauri::command]
#[specta::specta]
pub fn routing_tiers(state: Manager<'_>, routing: RoutingState<'_>, provider_id: u32, model: String, effort: Option<Effort>) -> Result<TiersView, String> {
    super::view(&routing, state.store(), provider_id, model.trim(), effort_variant(effort).as_deref())
}

/// Refresh: fetch, check the account, rank, and test prompt caching within $0.05.
#[tauri::command]
#[specta::specta]
pub async fn routing_refresh(
    state: Manager<'_>,
    routing: RoutingState<'_>,
    provider_id: u32,
    model: String,
    effort: Option<Effort>,
) -> Result<RefreshOutcome, String> {
    let (m, r) = (state.inner().clone(), routing.inner().clone());
    tauri::async_runtime::spawn_blocking(move || super::refresh(&r, m.store(), provider_id, model.trim(), effort_variant(effort).as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

/// Which provider's key keeps refreshed models' speeds fresh every 30 minutes; `None` stops it.
#[tauri::command]
#[specta::specta]
pub fn routing_watch(state: Manager<'_>, provider_id: Option<u32>) -> Result<(), String> {
    super::set_watched_provider(state.store(), provider_id)
}

/// The saved ranking settings over the defaults, the defaults, and whether a row is stored.
#[tauri::command]
#[specta::specta]
pub fn routing_settings(state: Manager<'_>) -> Result<RoutingSettingsView, String> {
    super::settings_view(state.store())
}

/// Saves the whole object. serde has already refused unknown fields; `validate` refuses the rest.
#[tauri::command]
#[specta::specta]
pub fn routing_settings_save(state: Manager<'_>, settings: RoutingSettings) -> Result<RoutingSettingsView, String> {
    super::save_settings(state.store(), &settings)
}

/// Restore defaults: deletes the saved row.
#[tauri::command]
#[specta::specta]
pub fn routing_settings_reset(state: Manager<'_>) -> Result<RoutingSettingsView, String> {
    super::reset_settings(state.store())
}
