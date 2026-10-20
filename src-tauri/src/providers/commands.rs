//! Providers (Settings → Providers): add one with keys from the vault, test it, refresh its models
//! and pick a shortlist. Inputs are checked here.

use std::sync::Arc;

use chorus_core::agent::{AgentKind, valid_model};
use chorus_core::providers::{self as p, CatalogModel, ProviderKind};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::sessions::SessionManager;
use crate::store::providers::{self as rows, ProviderInput, ProviderRow};
use crate::store::{Store, credentials};
use crate::vault;
use crate::vault::commands::secs;

type Manager<'a> = State<'a, Arc<SessionManager>>;

const MAX_SHORTLIST: usize = 200;

/// Whether an agent can run on the provider; `reason` says why not.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentRoute {
    pub agent: AgentKind,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    pub id: u32,
    pub kind: ProviderKind,
    pub name: String,
    /// A gateway's address, or the fixed one OpenRouter and Jev use.
    pub base_url: String,
    pub credential_id: Option<u32>,
    pub credential_label: Option<String>,
    pub management_credential_id: Option<u32>,
    pub management_label: Option<String>,
    /// Each run's minted key stops at this; `None` means the default.
    pub cap_usd: Option<f64>,
    pub default_cap_usd: f64,
    pub tested_at_secs: Option<u32>,
    pub test_ok: Option<bool>,
    pub test_note: Option<String>,
    pub catalog_refreshed_at_secs: Option<u32>,
    pub catalog_count: u32,
    pub shortlist: Vec<String>,
    pub agents: Vec<AgentRoute>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSave {
    /// `None` adds a provider.
    pub id: Option<u32>,
    pub kind: ProviderKind,
    pub name: String,
    pub base_url: Option<String>,
    pub credential_id: Option<u32>,
    pub management_credential_id: Option<u32>,
    pub cap_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub model: CatalogModel,
    /// The last refresh no longer listed it.
    pub missing: bool,
    pub shortlisted: bool,
}

fn label(store: &Store, id: Option<u32>) -> Option<String> {
    id.and_then(|id| credentials::get(store, id).ok().flatten()).map(|c| c.label)
}

pub fn summary(store: &Store, row: &ProviderRow) -> ProviderSummary {
    ProviderSummary {
        id: row.id,
        kind: row.kind,
        name: row.name.clone(),
        base_url: row.base_url.clone().or_else(|| row.kind.fixed_url().map(str::to_owned)).unwrap_or_default(),
        credential_id: row.credential_id,
        credential_label: label(store, row.credential_id),
        management_credential_id: row.management_credential_id,
        management_label: label(store, row.management_credential_id),
        cap_usd: row.cap_usd,
        default_cap_usd: p::DEFAULT_CAP_USD,
        tested_at_secs: row.tested_at.map(secs),
        test_ok: row.test_ok,
        test_note: row.test_note.clone(),
        catalog_refreshed_at_secs: row.catalog_refreshed_at.map(secs),
        catalog_count: rows::catalog(store, row.id).map(|c| c.iter().filter(|r| r.missing_since.is_none()).count() as u32).unwrap_or(0),
        shortlist: rows::shortlist(store, row.id).unwrap_or_default(),
        agents: AgentKind::ALL
            .iter()
            .map(|a| AgentRoute { agent: *a, reason: p::route_support(*a, row.kind).err() })
            .collect(),
    }
}

fn row(store: &Store, id: u32) -> Result<ProviderRow, String> {
    rows::get(store, id)?.ok_or_else(|| "That provider no longer exists.".into())
}

#[tauri::command]
#[specta::specta]
pub fn providers_list(state: Manager<'_>) -> Result<Vec<ProviderSummary>, String> {
    let store = state.store();
    Ok(rows::list(store)?.iter().map(|r| summary(store, r)).collect())
}

/// Checks a save at the boundary: the name, a gateway's address, keys that exist, a management key
/// only for OpenRouter (and never as the key agents run with), and a cap in range.
fn check(store: &Store, s: &ProviderSave) -> Result<(String, Option<String>), String> {
    let name = s.name.trim().to_owned();
    if !p::valid_provider_name(&name) {
        return Err("A provider's name is up to 60 letters, digits, spaces and - _ . ( ).".into());
    }
    let base_url = match s.kind {
        ProviderKind::Gateway => Some(p::clean_base_url(s.base_url.as_deref().unwrap_or(""))?),
        _ => None,
    };
    for id in [s.credential_id, s.management_credential_id].into_iter().flatten() {
        credentials::get(store, id)?.ok_or("One of the chosen keys no longer exists.")?;
    }
    if s.management_credential_id.is_some() && s.kind != ProviderKind::Openrouter {
        return Err("Only OpenRouter takes a management key.".into());
    }
    if s.management_credential_id.is_some() && s.management_credential_id == s.credential_id {
        return Err("A management key can't run models: choose your normal OpenRouter key as the key.".into());
    }
    if let Some(cap) = s.cap_usd {
        if s.kind != ProviderKind::Openrouter {
            return Err("Only OpenRouter's minted keys have a cap.".into());
        }
        if !(cap.is_finite() && cap > 0.0 && cap <= p::MAX_CAP_USD) {
            return Err(format!("The cap must be more than $0 and at most ${}.", p::MAX_CAP_USD));
        }
    }
    Ok((name, base_url))
}

#[tauri::command]
#[specta::specta]
pub fn provider_save(state: Manager<'_>, input: ProviderSave) -> Result<ProviderSummary, String> {
    let store = state.store();
    let (name, base_url) = check(store, &input)?;
    let fields = ProviderInput {
        kind: input.kind,
        name: &name,
        base_url: base_url.as_deref(),
        credential_id: input.credential_id,
        management_credential_id: input.management_credential_id,
        cap_usd: input.cap_usd,
    };
    let id = match input.id {
        Some(id) => {
            let old = row(store, id)?;
            if old.kind != input.kind {
                return Err("A provider's kind can't change; add a new one instead.".into());
            }
            rows::update(store, id, &fields)?;
            id
        }
        None => rows::insert(store, &fields)?,
    };
    Ok(summary(store, &row(store, id)?))
}

/// Sessions on it keep their runs; their next launch says the provider is gone.
#[tauri::command]
#[specta::specta]
pub fn provider_delete(state: Manager<'_>, id: u32) -> Result<bool, String> {
    rows::delete(state.store(), id)
}

fn key(store: &Store, id: Option<u32>) -> Result<Option<String>, String> {
    id.map(|id| vault::decrypt(store, id)).transpose()
}

/// Tries the provider's keys (for Jev, one tiny paid question, D40) and records what it found.
#[tauri::command]
#[specta::specta]
pub async fn provider_test(state: Manager<'_>, id: u32) -> Result<ProviderSummary, String> {
    let m = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store = m.store();
        let r = row(store, id)?;
        let (ok, note) = super::test(
            r.kind,
            r.base_url.as_deref(),
            key(store, r.credential_id)?.as_deref(),
            key(store, r.management_credential_id)?.as_deref(),
        );
        rows::set_test(store, id, ok, &note)?;
        Ok(summary(store, &row(store, id)?))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Fetches the provider's model list (free for OpenRouter).
#[tauri::command]
#[specta::specta]
pub async fn provider_refresh_catalog(state: Manager<'_>, id: u32) -> Result<ProviderSummary, String> {
    let m = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let store = m.store();
        let r = row(store, id)?;
        let models = super::fetch_catalog(r.kind, r.base_url.as_deref(), key(store, r.credential_id)?.as_deref())?;
        rows::replace_catalog(store, id, &models)?;
        Ok(summary(store, &row(store, id)?))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
pub fn provider_catalog(state: Manager<'_>, id: u32) -> Result<Vec<CatalogEntry>, String> {
    let store = state.store();
    row(store, id)?;
    let shortlist = rows::shortlist(store, id)?;
    Ok(rows::catalog(store, id)?
        .into_iter()
        .map(|r| CatalogEntry { shortlisted: shortlist.contains(&r.model.id), missing: r.missing_since.is_some(), model: r.model })
        .collect())
}

#[tauri::command]
#[specta::specta]
pub fn provider_set_shortlist(state: Manager<'_>, id: u32, models: Vec<String>) -> Result<ProviderSummary, String> {
    let store = state.store();
    row(store, id)?;
    if models.len() > MAX_SHORTLIST {
        return Err(format!("A shortlist holds up to {MAX_SHORTLIST} models."));
    }
    if let Some(bad) = models.iter().find(|m| !valid_model(m)) {
        return Err(format!("{bad:?} is not a model name Chorus can pass on."));
    }
    let mut unique: Vec<String> = Vec::new();
    for m in models {
        if !unique.contains(&m) {
            unique.push(m);
        }
    }
    rows::set_shortlist(store, id, &unique)?;
    Ok(summary(store, &row(store, id)?))
}
