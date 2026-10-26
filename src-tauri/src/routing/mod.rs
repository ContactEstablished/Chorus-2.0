//! OpenRouter routing tiers for opencode (D57, brought forward from PLAN P12), run as Chorus 1 runs
//! them. A Refresh, with an OpenRouter provider's own key, fetches a model's endpoints, runs the two
//! zero-cost account checks, ranks, tests prompt caching on the endpoints that need it (at most
//! $0.05) and ranks again. The observer refetches only the endpoints of every refreshed model every
//! 30 minutes, with the provider the user chose for it, skipping the models the user stopped; it
//! keeps its last check in memory for Settings → Routing. Every Refresh that got as far as sending a
//! request is logged (the last 50, in the settings table). A launch ranks what is stored and keeps
//! the selection with the session, so Restart reuses it. Every decision is `chorus_core::routing`;
//! this module does the IO.

pub mod client;
pub mod commands;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chorus_core::providers::ProviderKind;
use chorus_core::routing::cache_probe::{
    CACHE_PROBE_CALLS, CACHE_PROBE_CAP_USD, CACHE_PROBE_CONCURRENCY, CACHE_PROBE_GAP_MS, CacheProbeOutcome, ProbePlanInput, ProbeSkip, ProbeSkipReason,
    evaluate_probe, plan_cache_probe, probe_call_spend_usd,
};
use chorus_core::routing::endpoints::extract_observations;
use chorus_core::routing::history::{RefreshLogEntry, SpeedHistoryRow, append_refresh_log, parse_refresh_log, sample_count, speed_history};
use chorus_core::agent::{AgentKind, LaunchOptions};
use chorus_core::routing::launch::{
    REFUSE_NITRO_MODEL, REFUSE_NO_MODEL, REFUSE_NOT_OPENCODE, RoutingLaunchChoice, RoutingLaunchSelection, RoutingLaunchTier, StoredSelection,
    parse_stored_selection, resolve_launch_selection, routing_refusal,
};
use chorus_core::routing::payload::NITRO_SUFFIX;
use chorus_core::routing::preflight::{PreflightStep, rows_per_tag};
use chorus_core::routing::registry::{RegistrySource, bundled_model_registry, entry_for};
use chorus_core::routing::store::{OBSERVATIONS_MAX_PER_TAG, merge_cache_verifications, merge_observations, parse_routing_settings_value};
use chorus_core::routing::tiers::compute_tiers;
use chorus_core::routing::types::{
    AccountEligibility, CacheRecord, CacheVerification, EndpointSnapshot, Quantization, RankInput, RankedTier, RawEndpoint, RoutingObservation,
    RoutingProfileId, RoutingSettings, TierResult, is_model_slug,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use specta::Type;

use crate::sessions::SessionManager;
use crate::store::providers::{self as provider_rows, ProviderRow};
use crate::store::routing::{self as rows, Kind};
use crate::store::{Store, now_ms, settings as setting_rows};
use crate::vault;
use client::Failure;

/// MR-D22: a model is refreshed at most once a minute.
const COOLDOWN_MS: i64 = 60_000;
const OBSERVER_FIRST: Duration = Duration::from_secs(120);
const OBSERVER_EVERY: Duration = Duration::from_secs(30 * 60);
/// The observer skips a model refreshed within this.
const OBSERVER_FRESH_MS: i64 = 25 * 60 * 1000;
const SETTINGS_KEY: &str = "routing_settings";
const WATCH_KEY: &str = "routing_watch_provider";
const CHOICES_KEY: &str = "routing_launch_choices";
/// A JSON array of the slugs the observer skips (Stop observing).
const STOPPED_KEY: &str = "routing_observer_stopped";
const STOPPED_MAX: usize = 500;
/// Shown when the stored list isn't a JSON array of at most `STOPPED_MAX` slugs; the observer then
/// fetches nothing until Stop observing or Observe again replaces it.
const STOPPED_UNREADABLE: &str = "The list of stopped models couldn't be read. Stop or observe a model to replace it.";
/// A JSON array of `RefreshLogEntry`, newest first, at most `REFRESH_LOG_MAX`.
const REFRESH_LOG_KEY: &str = "routing_refresh_log";
/// Shown when the stored log isn't a JSON array of entries; Refreshes aren't logged until Chorus can
/// read it again, and nothing rewrites it.
const REFRESH_LOG_UNREADABLE: &str = "The Refresh log couldn't be read.";
/// Held while the log is read and written back, so two Refreshes ending together both land in it.
static REFRESH_LOG_LOCK: Mutex<()> = Mutex::new(());

/// Refreshes under way, when each model's last one ended, and what the observer last did (in memory:
/// after a restart there is no last check).
#[derive(Default)]
pub struct Routing {
    busy: Mutex<HashSet<String>>,
    ended: Mutex<HashMap<String, i64>>,
    observer: Mutex<ObserverStatus>,
}

/// When the observer began, when its last tick ended, and the last check that recorded an outcome.
#[derive(Default)]
struct ObserverStatus {
    started_ms: Option<i64>,
    /// Every tick moves the schedule, including one that found no provider watched and so recorded
    /// no check.
    last_tick_ms: Option<i64>,
    last_check: Option<LastCheck>,
}

/// Holds a model's slot until dropped.
struct Slot<'a> {
    routing: &'a Routing,
    model: String,
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        self.routing.busy.lock().expect("routing lock").remove(&self.model);
    }
}

impl Routing {
    fn take(&self, model: &str) -> Option<Slot<'_>> {
        self.busy.lock().expect("routing lock").insert(model.to_owned()).then(|| Slot { routing: self, model: model.to_owned() })
    }

    /// Seconds before `model` may be refreshed again.
    fn wait_secs(&self, model: &str, now: i64) -> u32 {
        let ended = self.ended.lock().expect("routing lock").get(model).copied();
        match ended.map(|e| now - e) {
            Some(elapsed) if (0..COOLDOWN_MS).contains(&elapsed) => ((COOLDOWN_MS - elapsed + 999) / 1000) as u32,
            _ => 0,
        }
    }
}

/// The ranker's settings: the defaults, under anything stored (Settings → Routing edits them).
pub fn settings(store: &Store) -> RoutingSettings {
    parse_routing_settings_value(setting_rows::get(store, SETTINGS_KEY).ok().flatten().as_deref()).value
}

/// Shown when the stored row isn't JSON, has an unknown field or fails `validate`.
const SETTINGS_UNREADABLE: &str = "The saved ranking settings couldn't be read, so the defaults apply. Save or Restore defaults replaces them.";

/// Settings → Routing's ranking settings: what applies now, the defaults beside them, and whether
/// a row is stored.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RoutingSettingsView {
    pub settings: RoutingSettings,
    pub defaults: RoutingSettings,
    /// A `routing_settings` row exists, readable or not.
    pub stored: bool,
    /// Set when the stored row can't be read, so the defaults apply. Reading never rewrites it.
    pub problem: Option<String>,
}

/// A failed read of the row is an error, not "nothing stored": that would hide a database fault.
pub fn settings_view(store: &Store) -> Result<RoutingSettingsView, String> {
    let raw = setting_rows::get(store, SETTINGS_KEY)?;
    let parsed = parse_routing_settings_value(raw.as_deref());
    Ok(RoutingSettingsView {
        settings: parsed.value,
        defaults: RoutingSettings::default(),
        stored: raw.is_some(),
        problem: parsed.warning.map(|_| SETTINGS_UNREADABLE.to_owned()),
    })
}

/// Stores the whole object once `validate` accepts it; otherwise returns its message and leaves the
/// row as it was (an unreadable one included). Applies to the next ranking, never to a selection
/// already made.
pub fn save_settings(store: &Store, settings: &RoutingSettings) -> Result<RoutingSettingsView, String> {
    settings.validate()?;
    setting_rows::set(store, SETTINGS_KEY, &serde_json::to_string(settings).map_err(|e| e.to_string())?)?;
    settings_view(store)
}

/// Restore defaults: deletes the row, so Data collection goes back to its default too.
pub fn reset_settings(store: &Store) -> Result<RoutingSettingsView, String> {
    setting_rows::delete(store, SETTINGS_KEY)?;
    settings_view(store)
}

fn read<T: DeserializeOwned>(store: &Store, model: &str, kind: Kind) -> Option<T> {
    serde_json::from_str(&rows::get(store, model, kind).ok()??).ok()
}

fn write<T: Serialize>(store: &Store, model: &str, kind: Kind, value: &T) -> Result<(), String> {
    rows::set(store, model, kind, &serde_json::to_string(value).map_err(|e| e.to_string())?)
}

/// A provider routing can use for its requests: OpenRouter, with its own API key.
pub fn routing_provider(store: &Store, provider_id: u32) -> Result<ProviderRow, String> {
    let p = provider_rows::get(store, provider_id)?.ok_or("That provider no longer exists.")?;
    if p.kind != ProviderKind::Openrouter {
        return Err("Routing tiers are for OpenRouter.".into());
    }
    if p.credential_id.is_none() {
        return Err(format!("Routing needs {}'s own API key (a management key can't fetch endpoints).", p.name));
    }
    Ok(p)
}

fn key_for(store: &Store, p: &ProviderRow) -> Result<String, String> {
    vault::decrypt(store, p.credential_id.ok_or("no key")?)
}

/// Everything a ranking reads for `model`, as stored; `None` before its first Refresh.
fn rank_stored(store: &Store, model: &str, provider_id: Option<u32>, effort: Option<&str>, now: i64) -> Result<Option<(TierResult, RegistryInfo)>, String> {
    let Some(snapshot) = read::<EndpointSnapshot>(store, model, Kind::Snapshot) else { return Ok(None) };
    let account: AccountEligibility = provider_id
        .and_then(|p| rows::get_account(store, model, p).ok().flatten())
        .and_then(|a| serde_json::from_str(&a).ok())
        .unwrap_or_default();
    let history: Vec<RoutingObservation> = read(store, model, Kind::Observations).unwrap_or_default();
    let cache: CacheVerification = read(store, model, Kind::Cache).unwrap_or_default();
    rank(model, &snapshot, &history, &account, &cache, effort, &settings(store), now).map(Some)
}

#[allow(clippy::too_many_arguments, reason = "the ranker's inputs, one each")]
fn rank(
    model: &str,
    snapshot: &EndpointSnapshot,
    history: &[RoutingObservation],
    account: &AccountEligibility,
    cache: &CacheVerification,
    effort: Option<&str>,
    settings: &RoutingSettings,
    now: i64,
) -> Result<(TierResult, RegistryInfo), String> {
    let (entry, source) = entry_for(&bundled_model_registry()?, model, &snapshot.endpoints);
    let input = RankInput {
        model: &entry,
        snapshot,
        history,
        account,
        cache,
        profile: RoutingProfileId::Interactive,
        effort,
        settings,
        now_ms: now,
    };
    let result = compute_tiers(&input).map_err(|e| match e {
        chorus_core::routing::tiers::TierError::SnapshotAfterNow => "The stored snapshot is newer than the current time; refresh again.".to_owned(),
        other => other.to_string(),
    })?;
    Ok((result, RegistryInfo { source, native_precision: entry.native_precision, min_context: entry.min_context }))
}

/// What routing knows about the model beyond its endpoints.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RegistryInfo {
    /// Chorus's own facts, or ones derived from the endpoints (fp8 as a quality floor).
    pub source: RegistrySource,
    pub native_precision: Option<Quantization>,
    pub min_context: u32,
}

/// New session's routing section for one provider and model.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TiersView {
    pub model: String,
    /// `None` until the first Refresh stores this model's endpoints.
    pub result: Option<TierResult>,
    pub registry: Option<RegistryInfo>,
    /// What the last launch of this model used.
    pub remembered: Option<RoutingLaunchChoice>,
    /// What New session chooses: the remembered choice if it can launch, else Balanced, else
    /// OpenRouter default (Chorus 1's `defaultLaunchChoice`). Nitro only when remembered.
    pub preselect: RoutingLaunchChoice,
    /// Why the wanted choice isn't the preselected one ("Refresh to use Balanced.").
    pub preselect_hint: Option<String>,
    /// The observer keeps this model's speeds fresh with this provider's key.
    pub watching: bool,
    /// Why Refresh can't run with this provider; tiers already stored still show.
    pub problem: Option<String>,
    /// Seconds before Refresh may run again for this model.
    pub retry_in_secs: u32,
}

fn label(choice: RoutingLaunchChoice) -> &'static str {
    match choice {
        RoutingLaunchChoice::Budget => "Budget",
        RoutingLaunchChoice::Balanced => "Balanced",
        RoutingLaunchChoice::Fast => "Fast",
        RoutingLaunchChoice::Nitro => "Nitro",
        RoutingLaunchChoice::Default => "OpenRouter default",
    }
}

fn ranked(choice: RoutingLaunchChoice) -> Option<RankedTier> {
    match choice {
        RoutingLaunchChoice::Budget => Some(RankedTier::Budget),
        RoutingLaunchChoice::Balanced => Some(RankedTier::Balanced),
        RoutingLaunchChoice::Fast => Some(RankedTier::Fast),
        _ => None,
    }
}

fn tier_of(choice: RoutingLaunchChoice) -> Option<RoutingLaunchTier> {
    match choice {
        RoutingLaunchChoice::Budget => Some(RoutingLaunchTier::Budget),
        RoutingLaunchChoice::Balanced => Some(RoutingLaunchTier::Balanced),
        RoutingLaunchChoice::Fast => Some(RoutingLaunchTier::Fast),
        RoutingLaunchChoice::Nitro => Some(RoutingLaunchTier::Nitro),
        RoutingLaunchChoice::Default => None,
    }
}

fn preselect(remembered: Option<RoutingLaunchChoice>, result: Option<&TierResult>) -> (RoutingLaunchChoice, Option<String>) {
    let launchable = |c: RoutingLaunchChoice| match ranked(c) {
        None => true,
        Some(t) => result.is_some_and(|r| !r.stale && r.tiers.get(t).is_some()),
    };
    let wanted = remembered.unwrap_or(RoutingLaunchChoice::Balanced);
    if launchable(wanted) {
        return (wanted, None);
    }
    if launchable(RoutingLaunchChoice::Balanced) {
        return (RoutingLaunchChoice::Balanced, None);
    }
    (RoutingLaunchChoice::Default, Some(format!("Refresh to use {}.", label(wanted))))
}

pub fn view(routing: &Routing, store: &Store, provider_id: u32, model: &str, effort: Option<&str>) -> Result<TiersView, String> {
    if !is_model_slug(model) {
        return Err(format!("{model:?} isn't an OpenRouter model id."));
    }
    if let Some(reason) = routing_refusal(model) {
        return Err(reason.into());
    }
    let now = now_ms();
    let ranked = rank_stored(store, model, Some(provider_id), effort, now)?;
    let remembered = remembered(store, model);
    let (preselect, preselect_hint) = preselect(remembered, ranked.as_ref().map(|(r, _)| r));
    let (result, registry) = ranked.map_or((None, None), |(r, i)| (Some(r), Some(i)));
    Ok(TiersView {
        model: model.to_owned(),
        result,
        registry,
        remembered,
        preselect,
        preselect_hint,
        watching: watched_provider(store) == Some(provider_id),
        problem: routing_provider(store, provider_id).err(),
        retry_in_secs: routing.wait_secs(model, now),
    })
}

/// What one Refresh did.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RefreshOutcome {
    pub view: TiersView,
    /// The cache test's worst case, and what it cost by OpenRouter's own figures.
    pub estimate_usd: f64,
    pub spent_usd: f64,
    pub probed: Vec<String>,
    pub not_probed: Vec<ProbeSkip>,
    /// Endpoint rows OpenRouter sent that Chorus couldn't use.
    pub rejected_rows: u32,
    /// A check or test that failed, in words; the tiers still rank what was learned.
    pub notes: Vec<String>,
}

/// One user action (MR-D18): fetch the endpoints, check the account, rank, test caching within the
/// cap, rank again. Refused while another runs for the model, or within a minute of the last.
pub fn refresh(routing: &Routing, store: &Store, provider_id: u32, model: &str, effort: Option<&str>) -> Result<RefreshOutcome, String> {
    if !is_model_slug(model) {
        return Err(format!("{model:?} isn't an OpenRouter model id."));
    }
    // Before the provider check, so a refused model starts no cooldown.
    if let Some(reason) = routing_refusal(model) {
        return Err(reason.into());
    }
    let provider = routing_provider(store, provider_id)?;
    let wait = routing.wait_secs(model, now_ms());
    if wait > 0 {
        return Err(format!("This model was refreshed less than a minute ago. Try again in {wait} s."));
    }
    let _slot = routing.take(model).ok_or("A routing refresh or observation is already running for this model.")?;
    let key = key_for(store, &provider)?;
    let result = run_refresh(store, &key, provider_id, model, effort);
    let (estimate_usd, spent_usd, probed, not_probed, rejected_rows, notes) = end_refresh(routing, store, &provider, model, result)?;
    Ok(RefreshOutcome { view: view(routing, store, provider_id, model, effort)?, estimate_usd, spent_usd, probed, not_probed, rejected_rows, notes })
}

type RefreshParts = (f64, f64, Vec<String>, Vec<ProbeSkip>, u32, Vec<String>);

/// What every Refresh that sent a request does when it ends, worked or not: starts the model's
/// cooldown and puts it in the Refresh log. Returns `result` as it was.
fn end_refresh(routing: &Routing, store: &Store, provider: &ProviderRow, model: &str, result: Result<RefreshParts, String>) -> Result<RefreshParts, String> {
    let ended = now_ms();
    routing.ended.lock().expect("routing lock").insert(model.to_owned(), ended);
    record_refresh(store, refresh_entry(ended, model, &provider.name, &result));
    result
}

/// The Refresh log's entry for a Refresh that ended at `at_ms`. A failure carries its message and no
/// figures: every failure in `run_refresh` comes before the cache test spends anything, bar storing
/// its verdicts.
fn refresh_entry(at_ms: i64, model: &str, provider_name: &str, result: &Result<RefreshParts, String>) -> RefreshLogEntry {
    // A figure that isn't finite would be stored as null, and the whole log would stop reading.
    let money = |usd: f64| if usd.is_finite() { usd } else { 0.0 };
    let (error, endpoints_tested, spent_usd, estimate_usd) = match result {
        Ok((estimate, spent, probed, ..)) => (None, u32::try_from(probed.len()).unwrap_or(u32::MAX), money(*spent), money(*estimate)),
        Err(message) => (Some(message.clone()), 0, 0.0, 0.0),
    };
    RefreshLogEntry { at_ms, model: model.to_owned(), provider_name: provider_name.to_owned(), error, endpoints_tested, spent_usd, estimate_usd }
}

/// Puts `entry` first in the stored log, keeping the newest 50. A log that can't be read is left
/// alone and the Refresh goes unlogged (Settings → Routing says so); the log never fails a Refresh.
fn record_refresh(store: &Store, entry: RefreshLogEntry) {
    let _held = REFRESH_LOG_LOCK.lock().expect("routing lock");
    let Ok(raw) = setting_rows::get(store, REFRESH_LOG_KEY) else { return };
    let Ok(log) = parse_refresh_log(raw.as_deref()) else { return };
    if let Ok(json) = serde_json::to_string(&append_refresh_log(&log, entry)) {
        let _ = setting_rows::set(store, REFRESH_LOG_KEY, &json);
    }
}

/// Settings → Routing's Refresh log.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RefreshLogView {
    /// Newest first; empty when the log can't be read.
    pub entries: Vec<RefreshLogEntry>,
    /// Set when the stored log can't be read. Reading never rewrites it.
    pub problem: Option<String>,
}

/// A failed read of the row is an error, not an empty log: that would hide a database fault.
pub fn refresh_log(store: &Store) -> Result<RefreshLogView, String> {
    let raw = setting_rows::get(store, REFRESH_LOG_KEY)?;
    Ok(match parse_refresh_log(raw.as_deref()) {
        Ok(entries) => RefreshLogView { entries, problem: None },
        Err(_) => RefreshLogView { entries: Vec::new(), problem: Some(REFRESH_LOG_UNREADABLE.to_owned()) },
    })
}

/// The inspected model's speed history, one row per endpoint tag, from the stored observations no
/// older than the saved `observationMaxAgeDays`. Rows follow the stored ranking's candidate order
/// (ranked with no provider and no effort), or go by name when it can't rank.
pub fn model_history(store: &Store, model: &str) -> Result<Vec<SpeedHistoryRow>, String> {
    if !is_model_slug(model) {
        return Err(format!("{model:?} isn't an OpenRouter model id."));
    }
    if model.ends_with(NITRO_SUFFIX) {
        return Err(REFUSE_NITRO_MODEL.into());
    }
    if let Some(reason) = routing_refusal(model) {
        return Err(reason.into());
    }
    let now = now_ms();
    let observations: Vec<RoutingObservation> = rows::get(store, model, Kind::Observations)?.and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or_default();
    let order: Vec<String> = rank_stored(store, model, None, None, now)
        .ok()
        .flatten()
        .map(|(result, _)| result.candidates.into_iter().map(|c| c.tag).collect())
        .unwrap_or_default();
    Ok(speed_history(&observations, &order, settings(store).observation_max_age_days, now))
}

fn run_refresh(store: &Store, key: &str, provider_id: u32, model: &str, effort: Option<&str>) -> Result<RefreshParts, String> {
    let settings = settings(store);
    let mut notes = Vec::new();
    // Endpoints.
    let (endpoints, rejected_rows) = client::fetch_endpoints(key, model).map_err(|f| f.message().to_owned())?;
    let fetched_at = now_ms();
    let snapshot = EndpointSnapshot { fetched_at_ms: fetched_at, endpoints };
    write(store, model, Kind::Snapshot, &snapshot)?;
    let history = observe(store, model, &snapshot, &settings)?;

    // The account checks; the second isn't sent after a refused or rate-limited first.
    let rows = rows_per_tag(&snapshot.endpoints);
    let mut stop = false;
    let mut check = |step: PreflightStep, stop: &mut bool| -> Option<Vec<String>> {
        let name = match step {
            PreflightStep::Guardrails => "account guardrails",
            PreflightStep::DataPolicy => "data policy",
        };
        if *stop {
            return None;
        }
        match client::preflight(key, model, step, &rows) {
            Ok(Ok(removed)) => Some(removed),
            Ok(Err(issue)) => {
                notes.push(format!("The {name} check gave no answer Chorus could read ({issue:?})."));
                None
            }
            Err(f) => {
                *stop |= f.stops_further_calls();
                notes.push(format!("The {name} check failed: {}", f.message()));
                None
            }
        }
    };
    let guardrail_removed = check(PreflightStep::Guardrails, &mut stop);
    let data_policy_removed = check(PreflightStep::DataPolicy, &mut stop);
    let account = AccountEligibility { guardrail_removed, data_policy_removed, checked_at_ms: Some(now_ms()) };
    rows::set_account(store, model, provider_id, &serde_json::to_string(&account).map_err(|e| e.to_string())?)?;

    // Rank, then plan the cache test (its estimate is known before any request).
    let cache: CacheVerification = read(store, model, Kind::Cache).unwrap_or_default();
    let (first, _) = rank(model, &snapshot, &history, &account, &cache, effort, &settings, now_ms())?;
    let plan = plan_cache_probe(&ProbePlanInput { result: &first, cache: &cache, now_ms: now_ms(), cap_usd: CACHE_PROBE_CAP_USD, tag_limit: None })
        .map_err(|e| e.to_string())?;
    let mut not_probed = plan.not_probed.clone();
    let planned: Vec<(String, f64)> = plan.planned.iter().map(|p| (p.tag.clone(), p.estimate_usd)).collect();
    let (estimate, spent, probed, verdicts) = if stop {
        not_probed.extend(planned.iter().map(|(tag, _)| ProbeSkip { tag: tag.clone(), reason: ProbeSkipReason::Aborted }));
        (0.0, 0.0, Vec::new(), CacheVerification::new())
    } else {
        let run = run_probes(key, model, &planned);
        not_probed.extend(run.skips);
        match run.failure {
            Some(f) if f.stops_further_calls() => notes.push(format!("The cache test stopped: {}", f.message())),
            Some(f) => notes.push(format!("Some cache-test calls failed: {}", f.message())),
            None => {}
        }
        (plan.estimate_usd, run.spent, run.probed, run.verdicts)
    };
    if !verdicts.is_empty() {
        write(store, model, Kind::Cache, &merge_cache_verifications(&cache, &verdicts))?;
    }
    Ok((estimate, spent, probed, not_probed, rejected_rows, notes))
}

/// Stores the snapshot's speeds in the model's history (seven days, at most 400 per endpoint).
fn observe(store: &Store, model: &str, snapshot: &EndpointSnapshot, settings: &RoutingSettings) -> Result<Vec<RoutingObservation>, String> {
    let existing: Vec<RoutingObservation> = read(store, model, Kind::Observations).unwrap_or_default();
    let history = merge_observations(&existing, &extract_observations(snapshot), now_ms(), settings.observation_max_age_days, OBSERVATIONS_MAX_PER_TAG)
        .map_err(|e| format!("{e:?}"))?;
    write(store, model, Kind::Observations, &history)?;
    Ok(history)
}

struct ProbeRun {
    spent: f64,
    probed: Vec<String>,
    skips: Vec<ProbeSkip>,
    verdicts: CacheVerification,
    /// The first failed call's reason; one that refuses the key or rate-limits stops the test.
    failure: Option<Failure>,
}

/// K4, MR-G7: at most five endpoints at once, in plan order, three calls each 1.5 s apart. A tag
/// doesn't start once spend reaches the cap, or after a refused or rate-limited answer.
fn run_probes(key: &str, model: &str, planned: &[(String, f64)]) -> ProbeRun {
    let nonce: String = vault::random_bytes(8).iter().map(|b| format!("{b:02x}")).collect();
    let state = Mutex::new((0usize, ProbeRun { spent: 0.0, probed: Vec::new(), skips: Vec::new(), verdicts: BTreeMap::new(), failure: None }));
    let stopped = |run: &ProbeRun| run.failure.is_some_and(Failure::stops_further_calls);
    std::thread::scope(|scope| {
        for _ in 0..(CACHE_PROBE_CONCURRENCY as usize).min(planned.len()) {
            scope.spawn(|| loop {
                let (tag, estimate) = {
                    let mut guard = state.lock().expect("probe lock");
                    let (next, run) = &mut *guard;
                    if *next >= planned.len() {
                        return;
                    }
                    let reason = if stopped(run) {
                        Some(ProbeSkipReason::Aborted)
                    } else if run.spent >= CACHE_PROBE_CAP_USD {
                        Some(ProbeSkipReason::Cap)
                    } else {
                        None
                    };
                    if let Some(reason) = reason {
                        run.skips.extend(planned[*next..].iter().map(|(t, _)| ProbeSkip { tag: t.clone(), reason }));
                        *next = planned.len();
                        return;
                    }
                    let item = planned[*next].clone();
                    *next += 1;
                    run.probed.push(item.0.clone());
                    item
                };
                let per_call = estimate / f64::from(CACHE_PROBE_CALLS);
                let mut calls = Vec::new();
                for call in 0..CACHE_PROBE_CALLS {
                    if call > 0 {
                        if stopped(&state.lock().expect("probe lock").1) {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(u64::from(CACHE_PROBE_GAP_MS)));
                    }
                    let (answer, failure) = client::probe(key, model, &tag, &nonce);
                    let mut guard = state.lock().expect("probe lock");
                    guard.1.spent += probe_call_spend_usd(&answer, per_call);
                    calls.push(answer);
                    if let Some(f) = failure {
                        if f.stops_further_calls() {
                            guard.1.failure = Some(f);
                            break;
                        }
                        guard.1.failure.get_or_insert(f);
                    }
                }
                let outcome = evaluate_probe(&calls);
                if outcome != CacheProbeOutcome::Inconclusive {
                    let record = CacheRecord { verified: outcome == CacheProbeOutcome::Verified, checked_at_ms: now_ms() };
                    state.lock().expect("probe lock").1.verdicts.insert(tag, record);
                }
            });
        }
    });
    state.into_inner().expect("probe lock").1
}

/// The routing a launch of `model` on `provider_id` uses: what is stored, ranked now, for `tier`.
pub fn select_for_launch(store: &Store, provider_id: u32, model: &str, tier: RoutingLaunchTier, effort: Option<&str>) -> Result<RoutingLaunchSelection, String> {
    let now = now_ms();
    let ranked = if tier == RoutingLaunchTier::Nitro { None } else { rank_stored(store, model, Some(provider_id), effort, now)? };
    resolve_launch_selection(tier, model, ranked.as_ref().map(|(r, _)| r), &settings(store), now).map_err(|r| r.message)
}

/// The routing a launch asked for, checked and ranked now, as JSON to keep with the session; `None`
/// for none or OpenRouter default. A tier for anything but opencode on an OpenRouter provider with a
/// model is refused, never silently dropped (Chorus 1 K3).
pub fn for_launch(
    store: &Store,
    agent: AgentKind,
    provider_id: Option<u32>,
    choice: Option<RoutingLaunchChoice>,
    options: &LaunchOptions,
) -> Result<Option<String>, String> {
    let Some(tier) = choice.and_then(tier_of) else { return Ok(None) };
    if agent != AgentKind::Opencode {
        return Err(REFUSE_NOT_OPENCODE.into());
    }
    let provider = provider_id
        .and_then(|id| provider_rows::get(store, id).ok().flatten())
        .filter(|p| p.kind == ProviderKind::Openrouter)
        .ok_or("Routing tiers need an OpenRouter provider.")?;
    let model = options.model.as_deref().ok_or(REFUSE_NO_MODEL)?;
    if model.ends_with(NITRO_SUFFIX) {
        return Err(REFUSE_NITRO_MODEL.into());
    }
    if let Some(reason) = routing_refusal(model) {
        return Err(reason.into());
    }
    let effort = chorus_core::opencode::wanted_variant(agent.spec(), options);
    let selection = select_for_launch(store, provider.id, model, tier, effort.as_deref())?;
    serde_json::to_string(&selection).map(Some).map_err(|e| e.to_string())
}

/// A session's routing in words, for its facts: "Balanced · coreweave/fp8, then 2 more".
pub fn describe(stored: Option<&str>) -> Option<String> {
    match parse_stored_selection(stored) {
        StoredSelection::None => None,
        StoredSelection::Invalid => Some("its saved routing couldn't be read".into()),
        StoredSelection::Selection(s) if s.tier == RoutingLaunchTier::Nitro => Some("Nitro: OpenRouter picks the fastest endpoint".into()),
        StoredSelection::Selection(s) => {
            let more = match s.endpoints.len() {
                0 | 1 => String::new(),
                n => format!(", then {} more", n - 1),
            };
            Some(format!("{} · {}{more}", s.tier.label(), s.endpoints.first().map_or("", String::as_str)))
        }
    }
}

pub fn remembered(store: &Store, model: &str) -> Option<RoutingLaunchChoice> {
    let all: BTreeMap<String, RoutingLaunchChoice> = serde_json::from_str(&setting_rows::get(store, CHOICES_KEY).ok()??).ok()?;
    all.get(model).copied()
}

/// K8: what an eligible launch of `model` used, remembered after it started.
pub fn remember(store: &Store, model: &str, choice: RoutingLaunchChoice) {
    let stored = setting_rows::get(store, CHOICES_KEY).ok().flatten();
    // An unreadable record is left alone: rewriting it would forget every other model's choice.
    let Ok(mut all) = stored.map_or(Ok(BTreeMap::new()), |s| serde_json::from_str::<BTreeMap<String, RoutingLaunchChoice>>(&s)) else { return };
    if all.get(model) == Some(&choice) {
        return;
    }
    all.insert(model.to_owned(), choice);
    if let Ok(json) = serde_json::to_string(&all) {
        let _ = setting_rows::set(store, CHOICES_KEY, &json);
    }
}

/// The provider whose key the observer uses (MR-D19: choosing one is the consent).
pub fn watched_provider(store: &Store) -> Option<u32> {
    setting_rows::get(store, WATCH_KEY).ok().flatten()?.parse().ok()
}

pub fn set_watched_provider(store: &Store, provider_id: Option<u32>) -> Result<(), String> {
    match provider_id {
        Some(id) => {
            routing_provider(store, id)?;
            setting_rows::set(store, WATCH_KEY, &id.to_string())
        }
        None => setting_rows::delete(store, WATCH_KEY),
    }
}

/// How one observer check ended.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CheckOutcome {
    /// Models whose endpoints were refetched and stored.
    Updated { count: u32 },
    NothingDue,
    /// The watched provider can't be used; `routing_provider`'s reason.
    NoProvider { reason: String },
    /// OpenRouter refused the key.
    KeyRefused,
    RateLimited,
    Failed { message: String },
}

/// The last check that recorded an outcome, and when its tick ended.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LastCheck {
    #[specta(type = f64)]
    pub at_ms: i64,
    pub outcome: CheckOutcome,
}

/// Why one model got no update in a tick.
#[derive(Debug, Clone, PartialEq)]
enum Miss {
    Fetch(Failure),
    /// The endpoints came but couldn't be stored; the text is what the database said.
    Store(String),
}

/// A refused key or a rate limit wins, even after updates: either stops the tick. Otherwise any
/// update (or no miss at all) gives the count, and failures alone give the first one's message.
fn check_outcome(updated: u32, misses: &[Miss]) -> CheckOutcome {
    for miss in misses {
        match miss {
            Miss::Fetch(Failure::AuthFailed) => return CheckOutcome::KeyRefused,
            Miss::Fetch(Failure::RateLimited) => return CheckOutcome::RateLimited,
            _ => {}
        }
    }
    match misses.first() {
        Some(first) if updated == 0 => CheckOutcome::Failed {
            message: match first {
                Miss::Fetch(f) => f.message().to_owned(),
                Miss::Store(text) => text.clone(),
            },
        },
        _ => CheckOutcome::Updated { count: updated },
    }
}

/// When the next tick runs: the last tick's end plus 30 minutes, or Chorus's start plus 2 minutes
/// before the first.
pub fn next_check_at(started_ms: i64, last_check_ms: Option<i64>) -> i64 {
    match last_check_ms {
        Some(last) => last + OBSERVER_EVERY.as_millis() as i64,
        None => started_ms + OBSERVER_FIRST.as_millis() as i64,
    }
}

/// A model the observer refetches: not refused by routing, not stopped, and (when its snapshot can
/// be read) at least 25 minutes old. A snapshot that can't be read is replaced by the next update.
fn observer_due(model: &str, stopped: &[String], fetched_at_ms: Option<i64>, now: i64) -> bool {
    routing_refusal(model).is_none() && !stopped.iter().any(|s| s == model) && fetched_at_ms.is_none_or(|at| now - at >= OBSERVER_FRESH_MS)
}

/// The stopped list from its stored JSON; `None` when it isn't an array of at most `STOPPED_MAX`
/// slugs. No row is an empty list.
fn parse_stopped(raw: Option<&str>) -> Option<Vec<String>> {
    let Some(raw) = raw else { return Some(Vec::new()) };
    serde_json::from_str::<Vec<String>>(raw).ok().filter(|list| list.len() <= STOPPED_MAX)
}

/// One model the observer follows.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ObservedModel {
    pub model: String,
    /// When its endpoints were last stored; `None` when the snapshot can't be read.
    #[specta(type = Option<f64>)]
    pub fetched_at_ms: Option<i64>,
    /// How many updates its speed history holds (distinct update times).
    pub sample_count: u32,
    pub stopped: bool,
}

/// Settings → Routing's background observation.
#[derive(Debug, Clone, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ObserverView {
    /// The watched provider; `None` is off.
    pub provider_id: Option<u32>,
    /// `None` when off or when the provider no longer exists.
    pub provider_name: Option<String>,
    /// What `routing_provider` says about the watched provider now.
    pub problem: Option<String>,
    pub last_check: Option<LastCheck>,
    /// `None` only if the observer never started.
    #[specta(type = Option<f64>)]
    pub next_at_ms: Option<i64>,
    /// Set when the stopped list can't be read; the observer then fetches nothing.
    pub stopped_problem: Option<String>,
    /// Every model with a stored snapshot that routing doesn't refuse, in slug order.
    pub models: Vec<ObservedModel>,
}

pub fn observer_view(routing: &Routing, store: &Store) -> Result<ObserverView, String> {
    let watched = watched_provider(store);
    let provider_name = watched.and_then(|id| provider_rows::get(store, id).ok().flatten()).map(|p| p.name);
    let raw = setting_rows::get(store, STOPPED_KEY)?;
    let stopped = parse_stopped(raw.as_deref());
    let models = rows::models_with_snapshot(store)?
        .into_iter()
        .filter(|m| routing_refusal(m).is_none())
        .map(|model| {
            let observations: Vec<RoutingObservation> = read(store, &model, Kind::Observations).unwrap_or_default();
            ObservedModel {
                fetched_at_ms: read::<EndpointSnapshot>(store, &model, Kind::Snapshot).map(|s| s.fetched_at_ms),
                sample_count: sample_count(&observations),
                stopped: stopped.as_ref().is_some_and(|list| list.contains(&model)),
                model,
            }
        })
        .collect();
    let (last_check, next_at_ms) = {
        let status = routing.observer.lock().expect("routing lock");
        (status.last_check.clone(), status.started_ms.map(|started| next_check_at(started, status.last_tick_ms)))
    };
    Ok(ObserverView {
        provider_id: watched,
        provider_name,
        problem: watched.and_then(|id| routing_provider(store, id).err()),
        last_check,
        next_at_ms,
        stopped_problem: stopped.is_none().then(|| STOPPED_UNREADABLE.to_owned()),
        models,
    })
}

/// Stop observing (`observe` false) or Observe again: changes only the stopped list, never the
/// model's routing data. A list that can't be read is replaced.
pub fn observe_model(routing: &Routing, store: &Store, model: &str, observe: bool) -> Result<ObserverView, String> {
    if !is_model_slug(model) {
        return Err(format!("{model:?} isn't an OpenRouter model id."));
    }
    if model.ends_with(NITRO_SUFFIX) {
        return Err(REFUSE_NITRO_MODEL.into());
    }
    if let Some(reason) = routing_refusal(model) {
        return Err(reason.into());
    }
    let raw = setting_rows::get(store, STOPPED_KEY)?;
    let mut list = parse_stopped(raw.as_deref()).unwrap_or_default();
    if observe {
        list.retain(|m| m != model);
    } else if !list.iter().any(|m| m == model) {
        if list.len() >= STOPPED_MAX {
            return Err(format!("At most {STOPPED_MAX} models can be stopped. Observe one again first."));
        }
        list.push(model.to_owned());
    }
    setting_rows::set(store, STOPPED_KEY, &serde_json::to_string(&list).map_err(|e| e.to_string())?)?;
    observer_view(routing, store)
}

impl Routing {
    fn finish_tick(&self, at_ms: i64, outcome: Option<CheckOutcome>) {
        let mut status = self.observer.lock().expect("routing lock");
        status.last_tick_ms = Some(at_ms);
        if let Some(outcome) = outcome {
            status.last_check = Some(LastCheck { at_ms, outcome });
        }
    }
}

/// Every 30 minutes (the first two minutes after start): with the watched provider's key, the free
/// endpoints request for each refreshed model not refreshed in the last 25 minutes, and not stopped.
/// No account check, no cache test. An answer that refuses the key or rate-limits stops the tick.
pub fn start_observer(manager: Arc<SessionManager>, routing: Arc<Routing>) {
    routing.observer.lock().expect("routing lock").started_ms = Some(now_ms());
    let _ = std::thread::Builder::new().name("routing-observer".into()).spawn(move || {
        std::thread::sleep(OBSERVER_FIRST);
        loop {
            observe_tick(manager.store(), &routing);
            std::thread::sleep(OBSERVER_EVERY);
        }
    });
}

/// Fetches one model's endpoints with a key: `client::fetch_endpoints`, or a stand-in in tests.
type FetchEndpoints<'a> = &'a dyn Fn(&str, &str) -> Result<(Vec<RawEndpoint>, u32), Failure>;

fn observe_tick(store: &Store, routing: &Routing) {
    tick(store, routing, &client::fetch_endpoints);
}

/// One check. With no provider watched it records no check; otherwise it records how it ended, when
/// it ended.
fn tick(store: &Store, routing: &Routing, fetch: FetchEndpoints<'_>) {
    let outcome = watched_provider(store).map(|id| run_tick(store, routing, id, fetch));
    routing.finish_tick(now_ms(), outcome);
}

fn run_tick(store: &Store, routing: &Routing, provider_id: u32, fetch: FetchEndpoints<'_>) -> CheckOutcome {
    let failed = |message: String| CheckOutcome::Failed { message };
    let provider = match routing_provider(store, provider_id) {
        Ok(p) => p,
        Err(reason) => return CheckOutcome::NoProvider { reason },
    };
    let stopped = match setting_rows::get(store, STOPPED_KEY).map(|raw| parse_stopped(raw.as_deref())) {
        Ok(Some(list)) => list,
        Ok(None) => return failed(STOPPED_UNREADABLE.into()),
        Err(e) => return failed(e),
    };
    let models = match rows::models_with_snapshot(store) {
        Ok(models) => models,
        Err(e) => return failed(e),
    };
    let now = now_ms();
    let due: Vec<String> = models
        .into_iter()
        .filter(|m| observer_due(m, &stopped, read::<EndpointSnapshot>(store, m, Kind::Snapshot).map(|s| s.fetched_at_ms), now))
        .collect();
    if due.is_empty() {
        return CheckOutcome::NothingDue;
    }
    let key = match key_for(store, &provider) {
        Ok(key) => key,
        Err(message) => return failed(message),
    };
    let settings = settings(store);
    let mut updated = 0;
    let mut misses = Vec::new();
    for model in due {
        let Some(_slot) = routing.take(&model) else { continue };
        match fetch(&key, &model) {
            Ok((endpoints, _)) => {
                let snapshot = EndpointSnapshot { fetched_at_ms: now_ms(), endpoints };
                match write(store, &model, Kind::Snapshot, &snapshot).and_then(|()| observe(store, &model, &snapshot, &settings)) {
                    Ok(_) => updated += 1,
                    Err(e) => misses.push(Miss::Store(format!("Couldn't store {model}'s endpoints: {e}"))),
                }
            }
            Err(f) => {
                misses.push(Miss::Fetch(f));
                if f.stops_further_calls() {
                    break;
                }
            }
        }
    }
    check_outcome(updated, &misses)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chorus_core::routing::launch::REFUSE_FIRST_PARTY;

    #[test]
    fn the_preselected_choice_follows_chorus_1() {
        assert_eq!(preselect(None, None), (RoutingLaunchChoice::Default, Some("Refresh to use Balanced.".into())));
        assert_eq!(preselect(Some(RoutingLaunchChoice::Nitro), None), (RoutingLaunchChoice::Nitro, None), "Nitro needs no snapshot");
        assert_eq!(preselect(Some(RoutingLaunchChoice::Default), None), (RoutingLaunchChoice::Default, None));
        assert_eq!(preselect(Some(RoutingLaunchChoice::Fast), None).1.as_deref(), Some("Refresh to use Fast."));
    }

    #[test]
    fn a_model_rests_a_minute_between_refreshes() {
        let r = Routing::default();
        assert_eq!(r.wait_secs("a/b", 1_000), 0);
        r.ended.lock().unwrap().insert("a/b".into(), 1_000);
        assert_eq!(r.wait_secs("a/b", 1_001), 60);
        assert_eq!(r.wait_secs("a/b", 60_999), 1);
        assert_eq!(r.wait_secs("a/b", 61_000), 0);
        let slot = r.take("a/b").unwrap();
        assert!(r.take("a/b").is_none(), "one at a time");
        drop(slot);
        assert!(r.take("a/b").is_some());
    }

    /// Chorus 1's measured endpoints answer for DeepSeek V4.1 Flash (2026-10-02).
    const FIXTURE: &str = include_str!("../../../crates/chorus-core/src/routing/fixtures/endpoints-deepseek-v4.1-flash-2026-10-02.json");
    const SLUG: &str = "deepseek/deepseek-v4.1-flash";

    fn openrouter(s: &Store) -> u32 {
        use crate::store::providers::ProviderInput;
        let input = ProviderInput { kind: ProviderKind::Openrouter, name: "OpenRouter", base_url: None, credential_id: None, management_credential_id: None, cap_usd: None };
        provider_rows::insert(s, &input).unwrap()
    }

    fn stored_snapshot(s: &Store, fetched_at_ms: i64) {
        stored_snapshot_as(s, SLUG, fetched_at_ms);
    }

    /// The golden endpoints, stored under `slug`.
    fn stored_snapshot_as(s: &Store, slug: &str, fetched_at_ms: i64) {
        let value: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        let parsed = chorus_core::routing::endpoints::parse_endpoints_response(&value).unwrap();
        let snapshot = EndpointSnapshot { fetched_at_ms, endpoints: parsed.endpoints };
        write(s, slug, Kind::Snapshot, &snapshot).unwrap();
        observe(s, slug, &snapshot, &RoutingSettings::default()).unwrap();
    }

    #[test]
    fn a_launch_takes_a_tier_ranked_from_what_is_stored() {
        let s = Store::in_memory();
        let provider = openrouter(&s);
        let options = LaunchOptions { model: Some(SLUG.into()), ..LaunchOptions::default() };
        let launch = |choice| for_launch(&s, AgentKind::Opencode, Some(provider), Some(choice), &options);

        // Before any Refresh: a ranked tier is refused with the reason; Nitro and default need nothing.
        assert!(launch(RoutingLaunchChoice::Balanced).unwrap_err().contains("Refresh first"));
        assert_eq!(launch(RoutingLaunchChoice::Default).unwrap(), None);
        let nitro = launch(RoutingLaunchChoice::Nitro).unwrap();
        assert_eq!(describe(nitro.as_deref()).as_deref(), Some("Nitro: OpenRouter picks the fastest endpoint"));

        stored_snapshot(&s, now_ms());
        let view = view(&Routing::default(), &s, provider, SLUG, None).unwrap();
        let result = view.result.as_ref().unwrap();
        assert!(result.tiers.balanced.is_some() && view.registry.unwrap().source == RegistrySource::Bundled);
        assert!(view.problem.unwrap().contains("own API key"), "this provider has no key, so Refresh can't run");
        assert_eq!(view.preselect, RoutingLaunchChoice::Balanced);
        let balanced = launch(RoutingLaunchChoice::Balanced).unwrap().unwrap();
        let text = describe(Some(&balanced)).unwrap();
        assert!(text.starts_with("Balanced · ") && text.contains(&result.tiers.balanced.as_ref().unwrap().endpoints[0]), "{text}");

        // Not for another agent, and not on a stale snapshot.
        assert!(for_launch(&s, AgentKind::Claude, Some(provider), Some(RoutingLaunchChoice::Fast), &options).is_err());
        stored_snapshot(&s, now_ms() - 61 * 60 * 1000);
        assert!(launch(RoutingLaunchChoice::Fast).unwrap_err().contains("more than 60 minutes old"));
    }

    #[test]
    fn the_tiers_view_refuses_anthropic_and_openai_models_even_with_a_snapshot_stored() {
        let s = Store::in_memory();
        let provider = openrouter(&s);
        let routing = Routing::default();
        for model in ["anthropic/claude-sonnet-5", "OpenAI/gpt-5.5"] {
            stored_snapshot_as(&s, model, now_ms());
            assert!(rows::get(&s, model, Kind::Snapshot).unwrap().is_some(), "{model}: the golden snapshot is stored");
            assert!(rank_stored(&s, model, Some(provider), None, now_ms()).unwrap().is_some(), "{model}: it would rank without the refusal");
            assert_eq!(view(&routing, &s, provider, model, None).unwrap_err(), REFUSE_FIRST_PARTY, "{model}");
        }
        // A bad slug is still reported as one, and a model from another author still ranks.
        assert!(view(&routing, &s, provider, "a b", None).unwrap_err().contains("isn't an OpenRouter model id"));
        stored_snapshot(&s, now_ms());
        assert!(view(&routing, &s, provider, SLUG, None).unwrap().result.is_some());
    }

    #[test]
    fn a_refresh_of_an_anthropic_or_openai_model_is_refused_before_the_provider_and_starts_no_cooldown() {
        let s = Store::in_memory();
        let routing = Routing::default();
        let missing = 404;
        // With no such provider, a model from another author gets the provider's reason...
        assert_eq!(refresh(&routing, &s, missing, SLUG, None).unwrap_err(), "That provider no longer exists.");
        // ...and a refused one gets the refusal, first.
        for model in ["anthropic/claude-sonnet-5", "openai/gpt-5.5", "~OpenAI/o9"] {
            assert_eq!(refresh(&routing, &s, missing, model, None).unwrap_err(), REFUSE_FIRST_PARTY, "{model}");
            assert_eq!(routing.wait_secs(model, now_ms()), 0, "{model}: no cooldown");
            assert!(routing.take(model).is_some(), "{model}: no slot is held");
        }
        assert!(routing.ended.lock().unwrap().is_empty(), "no model has an end time");
        assert!(routing.busy.lock().unwrap().is_empty(), "no slot is held");
    }

    #[test]
    fn a_launch_refuses_every_tier_for_anthropic_and_openai_models_but_not_openrouter_default() {
        let s = Store::in_memory();
        let provider = openrouter(&s);
        for model in ["openai/gpt-5.5", "anthropic/claude-sonnet-5"] {
            // A stored snapshot doesn't matter: the refusal comes first.
            stored_snapshot_as(&s, model, now_ms());
            let options = LaunchOptions { model: Some(model.into()), ..LaunchOptions::default() };
            for choice in [RoutingLaunchChoice::Budget, RoutingLaunchChoice::Balanced, RoutingLaunchChoice::Fast, RoutingLaunchChoice::Nitro] {
                let launch = for_launch(&s, AgentKind::Opencode, Some(provider), Some(choice), &options);
                assert_eq!(launch.unwrap_err(), REFUSE_FIRST_PARTY, "{model} on {choice:?}");
            }
            assert_eq!(for_launch(&s, AgentKind::Opencode, Some(provider), Some(RoutingLaunchChoice::Default), &options).unwrap(), None, "{model}");
            assert_eq!(for_launch(&s, AgentKind::Opencode, Some(provider), None, &options).unwrap(), None, "{model}");
        }
        // The :nitro check still comes first.
        let nitro = LaunchOptions { model: Some("openai/gpt-5.5:nitro".into()), ..LaunchOptions::default() };
        let launch = for_launch(&s, AgentKind::Opencode, Some(provider), Some(RoutingLaunchChoice::Balanced), &nitro);
        assert_eq!(launch.unwrap_err(), REFUSE_NITRO_MODEL);
    }

    #[test]
    fn the_tiers_view_writes_no_launch_choice() {
        let s = Store::in_memory();
        let provider = openrouter(&s);
        stored_snapshot(&s, now_ms());
        assert_eq!(setting_rows::get(&s, CHOICES_KEY).unwrap(), None);
        let view = view(&Routing::default(), &s, provider, SLUG, None).unwrap();
        assert_eq!(view.preselect, RoutingLaunchChoice::Balanced, "it preselects Balanced without remembering it");
        assert_eq!(view.remembered, None);
        assert_eq!(setting_rows::get(&s, CHOICES_KEY).unwrap(), None, "no routing_launch_choices row");
    }

    #[test]
    fn remembered_choices_are_kept_per_model() {
        let s = Store::in_memory();
        assert_eq!(remembered(&s, "a/b"), None);
        remember(&s, "a/b", RoutingLaunchChoice::Fast);
        remember(&s, "c/d", RoutingLaunchChoice::Default);
        assert_eq!((remembered(&s, "a/b"), remembered(&s, "c/d")), (Some(RoutingLaunchChoice::Fast), Some(RoutingLaunchChoice::Default)));
    }

    const UNREADABLE: &str = "The saved ranking settings couldn't be read, so the defaults apply. Save or Restore defaults replaces them.";

    fn stored_row(s: &Store) -> Option<String> {
        setting_rows::get(s, SETTINGS_KEY).unwrap()
    }

    #[test]
    fn with_no_row_the_settings_view_holds_the_defaults_and_no_problem() {
        let s = Store::in_memory();
        let view = settings_view(&s).unwrap();
        assert_eq!(view, RoutingSettingsView { settings: RoutingSettings::default(), defaults: RoutingSettings::default(), stored: false, problem: None });
        assert_eq!(stored_row(&s), None, "reading stores nothing");
    }

    #[test]
    fn a_row_that_cannot_be_read_gives_the_defaults_and_the_problem_and_stays_as_it_was() {
        // Not JSON, an unknown field, and a value above `validate`'s bound.
        for raw in ["nope", r#"{"bogus":1}"#, r#"{"fallbackCount":6}"#] {
            let s = Store::in_memory();
            setting_rows::set(&s, SETTINGS_KEY, raw).unwrap();
            let view = settings_view(&s).unwrap();
            assert_eq!(view.settings, RoutingSettings::default(), "{raw}");
            assert_eq!(view.defaults, RoutingSettings::default(), "{raw}");
            assert!(view.stored, "{raw}: a row is there, even if it can't be read");
            assert_eq!(view.problem.as_deref(), Some(UNREADABLE), "{raw}");
            assert_eq!(stored_row(&s).as_deref(), Some(raw), "{raw}: reading leaves the row byte for byte as it was");
            assert_eq!(settings(&s), RoutingSettings::default(), "{raw}: the ranker uses the defaults too");
        }
    }

    #[test]
    fn a_readable_row_reads_over_the_defaults_with_no_problem() {
        let s = Store::in_memory();
        // A row written before a field existed still reads, with the defaults underneath.
        setting_rows::set(&s, SETTINGS_KEY, r#"{"fallbackCount":4}"#).unwrap();
        let view = settings_view(&s).unwrap();
        assert_eq!(view.settings, RoutingSettings { fallback_count: 4, ..RoutingSettings::default() });
        assert_eq!((view.stored, view.problem), (true, None));
        assert_eq!(view.defaults, RoutingSettings::default(), "the defaults stay the defaults");
    }

    #[test]
    fn saving_a_valid_object_stores_it_whole_and_the_view_returns_it() {
        let s = Store::in_memory();
        let mut wanted = RoutingSettings { fallback_count: 1, min_uptime_pct: 99.0, readmit_uptime_pct: 99.1, ..RoutingSettings::default() };
        wanted.data_collection = chorus_core::routing::types::DataCollection::Allow;
        wanted.tier_weights.budget = 0.25;
        let view = save_settings(&s, &wanted).unwrap();
        assert_eq!(view.settings, wanted);
        assert_eq!(view.defaults, RoutingSettings::default());
        assert_eq!((view.stored, view.problem), (true, None));
        // The whole object is stored, every field, not only what differs from the defaults.
        let row: serde_json::Value = serde_json::from_str(&stored_row(&s).unwrap()).unwrap();
        assert_eq!(row.as_object().unwrap().len(), 14, "{row}");
        assert_eq!(serde_json::from_value::<RoutingSettings>(row).unwrap(), wanted);
        assert_eq!(settings(&s), wanted, "the ranker reads it back");
        // Saving the defaults stores them too: that is not the same as having no row.
        assert!(save_settings(&s, &RoutingSettings::default()).unwrap().stored);
        assert!(stored_row(&s).is_some());
    }

    #[test]
    fn saving_an_invalid_object_returns_validates_message_and_leaves_the_row_as_it_was() {
        let invalid = RoutingSettings { fallback_count: 6, ..RoutingSettings::default() };
        // No row: still no row.
        let s = Store::in_memory();
        assert_eq!(save_settings(&s, &invalid).unwrap_err(), "fallbackCount must be at most 5");
        assert_eq!(stored_row(&s), None);
        // A readable row, and an unreadable one: both byte for byte as they were.
        for raw in [serde_json::to_string(&RoutingSettings { fallback_count: 1, ..RoutingSettings::default() }).unwrap(), "nope".to_owned()] {
            setting_rows::set(&s, SETTINGS_KEY, &raw).unwrap();
            assert_eq!(save_settings(&s, &invalid).unwrap_err(), "fallbackCount must be at most 5", "{raw}");
            assert_eq!(stored_row(&s).as_deref(), Some(raw.as_str()), "{raw}");
        }
        // The first failing rule is the one named.
        let both = RoutingSettings { readmit_uptime_pct: 99.0, tie_ratio: 3.0, ..RoutingSettings::default() };
        assert_eq!(save_settings(&s, &both).unwrap_err(), "readmitUptimePct below minUptimePct");
    }

    #[test]
    fn saving_replaces_a_row_that_could_not_be_read() {
        let s = Store::in_memory();
        setting_rows::set(&s, SETTINGS_KEY, "nope").unwrap();
        let view = save_settings(&s, &RoutingSettings { fallback_count: 3, ..RoutingSettings::default() }).unwrap();
        assert_eq!((view.settings.fallback_count, view.problem), (3, None));
    }

    #[test]
    fn reset_deletes_the_row() {
        let s = Store::in_memory();
        let allow = RoutingSettings { data_collection: chorus_core::routing::types::DataCollection::Allow, ..RoutingSettings::default() };
        save_settings(&s, &allow).unwrap();
        assert!(stored_row(&s).is_some());
        let view = reset_settings(&s).unwrap();
        assert_eq!(stored_row(&s), None, "the routing_settings row is gone");
        assert_eq!(view, RoutingSettingsView { settings: RoutingSettings::default(), defaults: RoutingSettings::default(), stored: false, problem: None });
        // With no row, and with a row that couldn't be read, it works the same.
        assert!(!reset_settings(&s).unwrap().stored);
        setting_rows::set(&s, SETTINGS_KEY, "nope").unwrap();
        assert_eq!(reset_settings(&s).unwrap().problem, None);
        assert_eq!(stored_row(&s), None);
    }

    #[test]
    fn saved_settings_move_the_next_ranking_and_reset_moves_it_back_but_a_selection_already_made_stays() {
        let s = Store::in_memory();
        let provider = openrouter(&s);
        stored_snapshot(&s, now_ms());
        let routing = Routing::default();
        let balanced_in_view = || view(&routing, &s, provider, SLUG, None).unwrap().result.unwrap().tiers.balanced.unwrap().endpoints.len();
        let balanced_for_launch = || select_for_launch(&s, provider, SLUG, RoutingLaunchTier::Balanced, None).unwrap().endpoints.len();
        assert_eq!((balanced_in_view(), balanced_for_launch()), (3, 3), "the default is the first endpoint and 2 fallbacks");

        // A launch's selection, made before the save.
        let options = LaunchOptions { model: Some(SLUG.into()), ..LaunchOptions::default() };
        let before = for_launch(&s, AgentKind::Opencode, Some(provider), Some(RoutingLaunchChoice::Balanced), &options).unwrap().unwrap();
        let described = describe(Some(&before)).unwrap();
        assert!(described.ends_with(", then 2 more"), "{described}");
        let parsed_before = parse_stored_selection(Some(&before));
        assert!(matches!(&parsed_before, StoredSelection::Selection(sel) if sel.endpoints.len() == 3));

        save_settings(&s, &RoutingSettings { fallback_count: 1, ..RoutingSettings::default() }).unwrap();
        assert_eq!((balanced_in_view(), balanced_for_launch()), (2, 2), "one fallback after the first");
        assert_eq!(describe(Some(&before)).as_deref(), Some(described.as_str()), "the earlier selection describes the same");
        assert_eq!(parse_stored_selection(Some(&before)), parsed_before, "and reads back unchanged, with its 3 endpoints");
        let after = for_launch(&s, AgentKind::Opencode, Some(provider), Some(RoutingLaunchChoice::Balanced), &options).unwrap().unwrap();
        assert!(describe(Some(&after)).unwrap().ends_with(", then 1 more"), "a launch after the save takes the new ranking");

        reset_settings(&s).unwrap();
        assert_eq!((balanced_in_view(), balanced_for_launch()), (3, 3), "back to the default after a reset");
        assert_eq!(describe(Some(&before)).as_deref(), Some(described.as_str()));
        assert_eq!(setting_rows::get(&s, CHOICES_KEY).unwrap(), None, "view and the settings commands write no routing_launch_choices row");
    }

    // ── The observer's status and Stop observing ──

    const MINUTE: i64 = 60_000;
    /// Not key-shaped, so the secret scan has nothing to flag.
    const FAKE_KEY: &str = "fake-routing-test-key";
    const NO_OWN_KEY: &str = "Routing needs OpenRouter's own API key (a management key can't fetch endpoints).";

    /// An OpenRouter provider with a key of its own (fake), so routing can use it.
    fn keyed_openrouter(s: &Store) -> u32 {
        use crate::store::providers::ProviderInput;
        let key = vault::add(s, "test", FAKE_KEY).unwrap();
        let input =
            ProviderInput { kind: ProviderKind::Openrouter, name: "OpenRouter", base_url: None, credential_id: Some(key), management_credential_id: None, cap_usd: None };
        provider_rows::insert(s, &input).unwrap()
    }

    fn golden_endpoints() -> Vec<RawEndpoint> {
        let value: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        chorus_core::routing::endpoints::parse_endpoints_response(&value).unwrap().endpoints
    }

    fn last_check(routing: &Routing) -> Option<LastCheck> {
        routing.observer.lock().unwrap().last_check.clone()
    }

    fn stopped_row(s: &Store) -> Option<String> {
        setting_rows::get(s, STOPPED_KEY).unwrap()
    }

    /// A fetch that fails the test: the tick must not have reached OpenRouter.
    fn no_request(_key: &str, model: &str) -> Result<(Vec<RawEndpoint>, u32), Failure> {
        panic!("the tick requested {model}")
    }

    /// Watches `provider` the way `routing_watch` does.
    fn watch(s: &Store, provider: u32) {
        set_watched_provider(s, Some(provider)).unwrap();
    }

    #[test]
    fn a_check_ends_as_the_stopping_answer_the_updates_or_the_first_failure() {
        use Failure::{AuthFailed, ProviderError, RateLimited, Unreachable};
        let miss = Miss::Fetch;
        // A refused key and a rate limit win, even after 2 updates.
        assert_eq!(check_outcome(2, &[miss(AuthFailed)]), CheckOutcome::KeyRefused);
        assert_eq!(check_outcome(2, &[miss(RateLimited)]), CheckOutcome::RateLimited);
        assert_eq!(check_outcome(0, &[miss(Unreachable), miss(AuthFailed)]), CheckOutcome::KeyRefused);
        assert_eq!(check_outcome(1, &[miss(ProviderError), miss(RateLimited)]), CheckOutcome::RateLimited);
        // Updates and a later failure that doesn't stop the tick.
        assert_eq!(check_outcome(2, &[miss(Unreachable)]), CheckOutcome::Updated { count: 2 });
        // No updates and two failures: the first one's message.
        assert_eq!(
            check_outcome(0, &[miss(Unreachable), miss(ProviderError)]),
            CheckOutcome::Failed { message: "Could not reach OpenRouter.".into() }
        );
        // No updates and no failures (every due model was busy).
        assert_eq!(check_outcome(0, &[]), CheckOutcome::Updated { count: 0 });
        // A failure to store the answer counts like a failed request.
        assert_eq!(check_outcome(0, &[Miss::Store("no room".into())]), CheckOutcome::Failed { message: "no room".into() });
        assert_eq!(check_outcome(1, &[Miss::Store("no room".into())]), CheckOutcome::Updated { count: 1 });
    }

    #[test]
    fn the_next_check_is_two_minutes_after_start_then_thirty_after_the_last_one() {
        let start = 1_700_000_000_000;
        assert_eq!(next_check_at(start, None), start + 120_000);
        let last = start + 5 * MINUTE;
        assert_eq!(next_check_at(start, Some(last)), last + 1_800_000);
    }

    #[test]
    fn a_model_is_due_when_stored_unrefused_not_stopped_and_25_minutes_old() {
        let now = 10_000 * MINUTE;
        let stopped = vec!["deepseek/stopped".to_owned()];
        let due = |model: &str, fetched: Option<i64>| observer_due(model, &stopped, fetched, now);
        assert!(due("deepseek/a", Some(now - 25 * MINUTE)), "exactly 25 minutes old is due");
        assert!(!due("deepseek/a", Some(now - 25 * MINUTE + 1)));
        assert!(due("deepseek/a", None), "a snapshot that can't be read is replaced by the next update");
        assert!(!due("deepseek/stopped", Some(0)), "stopped");
        for refused in ["anthropic/claude-sonnet-5", "OpenAI/gpt-5.5", "~openai/o9"] {
            assert!(!due(refused, Some(0)), "{refused}: refused");
            assert!(!due(refused, None), "{refused}: refused");
        }
    }

    #[test]
    fn the_stopped_list_reads_as_an_array_of_at_most_500_slugs() {
        assert_eq!(parse_stopped(None), Some(vec![]));
        assert_eq!(parse_stopped(Some("[]")), Some(vec![]));
        assert_eq!(parse_stopped(Some(r#"["a/b","c/d"]"#)), Some(vec!["a/b".to_owned(), "c/d".to_owned()]));
        for bad in ["nope", "{}", r#"{"a":1}"#, "[1]", r#"["a/b",null]"#, ""] {
            assert_eq!(parse_stopped(Some(bad)), None, "{bad:?}");
        }
        let many = |n: usize| serde_json::to_string(&(0..n).map(|i| format!("vendor/m{i}")).collect::<Vec<_>>()).unwrap();
        assert_eq!(parse_stopped(Some(&many(500))).map(|l| l.len()), Some(500));
        assert_eq!(parse_stopped(Some(&many(501))), None);
    }

    #[test]
    fn a_tick_with_no_watched_provider_records_no_check_and_sends_no_request() {
        let s = Store::in_memory();
        let routing = Routing::default();
        stored_snapshot_as(&s, SLUG, now_ms() - 60 * MINUTE);
        let before = now_ms();
        tick(&s, &routing, &no_request);
        assert_eq!(last_check(&routing), None, "nothing recorded");
        let status = routing.observer.lock().unwrap();
        assert!(status.last_tick_ms.is_some_and(|t| t >= before), "the schedule still moves: the next tick is 30 minutes on");
    }

    #[test]
    fn a_tick_with_an_unusable_watched_provider_records_the_reason_and_sends_no_request() {
        let s = Store::in_memory();
        let routing = Routing::default();
        stored_snapshot_as(&s, SLUG, now_ms() - 60 * MINUTE);
        // No key of its own.
        let unkeyed = openrouter(&s);
        setting_rows::set(&s, WATCH_KEY, &unkeyed.to_string()).unwrap();
        let (before, outcome) = (now_ms(), |r: &Routing| last_check(r).unwrap());
        tick(&s, &routing, &no_request);
        assert_eq!(outcome(&routing).outcome, CheckOutcome::NoProvider { reason: NO_OWN_KEY.into() });
        assert!(outcome(&routing).at_ms >= before, "timed when the tick ended");
        // A provider that was deleted.
        setting_rows::set(&s, WATCH_KEY, "404").unwrap();
        tick(&s, &routing, &no_request);
        assert_eq!(outcome(&routing).outcome, CheckOutcome::NoProvider { reason: "That provider no longer exists.".into() });
    }

    #[test]
    fn with_only_stopped_and_refused_models_stale_a_tick_finds_nothing_due_and_sends_no_request() {
        let s = Store::in_memory();
        let routing = Routing::default();
        watch(&s, keyed_openrouter(&s));
        let stale = now_ms() - 60 * MINUTE;
        stored_snapshot_as(&s, SLUG, stale);
        stored_snapshot_as(&s, "anthropic/claude-sonnet-5", stale);
        stored_snapshot_as(&s, "deepseek/fresh", now_ms() - MINUTE);
        observe_model(&routing, &s, SLUG, false).unwrap();
        tick(&s, &routing, &no_request);
        assert_eq!(last_check(&routing).unwrap().outcome, CheckOutcome::NothingDue);
    }

    #[test]
    fn a_tick_fetches_the_stale_models_that_are_not_stopped_or_refused_and_counts_the_updates() {
        let s = Store::in_memory();
        let routing = Routing::default();
        watch(&s, keyed_openrouter(&s));
        let stale = now_ms() - 60 * MINUTE;
        for model in [SLUG, "deepseek/stopped", "anthropic/claude-sonnet-5"] {
            stored_snapshot_as(&s, model, stale);
        }
        stored_snapshot_as(&s, "deepseek/fresh", now_ms() - MINUTE);
        observe_model(&routing, &s, "deepseek/stopped", false).unwrap();

        let calls = Mutex::new(Vec::new());
        let fetch = |key: &str, model: &str| {
            calls.lock().unwrap().push((key == FAKE_KEY, model.to_owned()));
            Ok((golden_endpoints(), 0))
        };
        let before = now_ms();
        tick(&s, &routing, &fetch);
        assert_eq!(*calls.lock().unwrap(), [(true, SLUG.to_owned())], "only the stale model that is neither stopped nor refused, with the watched key");
        let check = last_check(&routing).unwrap();
        assert_eq!(check.outcome, CheckOutcome::Updated { count: 1 });
        assert!(check.at_ms >= before);

        let view = observer_view(&routing, &s).unwrap();
        let model = |slug: &str| view.models.iter().find(|m| m.model == slug).unwrap().clone();
        assert!(model(SLUG).fetched_at_ms.is_some_and(|at| at >= before), "its snapshot was replaced");
        assert_eq!(model(SLUG).sample_count, 2, "and its history gained a sample");
        assert_eq!(model("deepseek/stopped").fetched_at_ms, Some(stale), "a stopped model was left as it was");
        assert_eq!(model("deepseek/stopped").sample_count, 1);
    }

    #[test]
    fn a_refused_key_or_a_rate_limit_stops_the_tick_and_wins_even_after_updates() {
        for (answer, expected) in [(Failure::AuthFailed, CheckOutcome::KeyRefused), (Failure::RateLimited, CheckOutcome::RateLimited)] {
            let s = Store::in_memory();
            let routing = Routing::default();
            watch(&s, keyed_openrouter(&s));
            // Slug order: a, b, c. The first updates, the second is refused, the third is never asked.
            for model in ["deepseek/a", "deepseek/b", "deepseek/c"] {
                stored_snapshot_as(&s, model, now_ms() - 60 * MINUTE);
            }
            let asked = Mutex::new(Vec::new());
            let fetch = |_: &str, model: &str| {
                asked.lock().unwrap().push(model.to_owned());
                if model == "deepseek/a" { Ok((golden_endpoints(), 0)) } else { Err(answer) }
            };
            tick(&s, &routing, &fetch);
            assert_eq!(*asked.lock().unwrap(), ["deepseek/a", "deepseek/b"], "{answer:?}: the tick stops at the answer");
            assert_eq!(last_check(&routing).unwrap().outcome, expected, "{answer:?}");
        }
    }

    #[test]
    fn a_failure_that_does_not_stop_the_tick_is_reported_only_when_nothing_updated() {
        let routing_with = |fail: &[&str]| {
            let s = Store::in_memory();
            let routing = Routing::default();
            watch(&s, keyed_openrouter(&s));
            for model in ["deepseek/a", "deepseek/b"] {
                stored_snapshot_as(&s, model, now_ms() - 60 * MINUTE);
            }
            let asked = Mutex::new(0);
            let fetch = |_: &str, model: &str| {
                *asked.lock().unwrap() += 1;
                if fail.contains(&model) { Err(Failure::Unreachable) } else { Ok((golden_endpoints(), 0)) }
            };
            tick(&s, &routing, &fetch);
            assert_eq!(*asked.lock().unwrap(), 2, "both were asked: this failure doesn't stop the tick");
            last_check(&routing).unwrap().outcome
        };
        assert_eq!(routing_with(&["deepseek/b"]), CheckOutcome::Updated { count: 1 });
        assert_eq!(routing_with(&["deepseek/a", "deepseek/b"]), CheckOutcome::Failed { message: "Could not reach OpenRouter.".into() });
    }

    #[test]
    fn a_model_whose_slot_is_busy_is_skipped_and_a_tick_of_only_busy_models_updates_none() {
        let s = Store::in_memory();
        let routing = Routing::default();
        watch(&s, keyed_openrouter(&s));
        stored_snapshot_as(&s, SLUG, now_ms() - 60 * MINUTE);
        let _refreshing = routing.take(SLUG).unwrap();
        tick(&s, &routing, &no_request);
        assert_eq!(last_check(&routing).unwrap().outcome, CheckOutcome::Updated { count: 0 });
    }

    #[test]
    fn a_key_that_cannot_be_read_is_a_failure_and_sends_no_request() {
        let s = Store::in_memory();
        let routing = Routing::default();
        let provider = keyed_openrouter(&s);
        watch(&s, provider);
        stored_snapshot_as(&s, SLUG, now_ms() - 60 * MINUTE);
        // Corrupt the sealed key in place: decrypting it fails.
        let credential = provider_rows::get(&s, provider).unwrap().unwrap().credential_id.unwrap();
        s.with(|c| c.execute("UPDATE credentials SET blob = x'00' WHERE id = ?1", [credential]).map(|_| ())).unwrap();
        tick(&s, &routing, &no_request);
        let CheckOutcome::Failed { message } = last_check(&routing).unwrap().outcome else { panic!("expected a failure") };
        assert!(message.starts_with("decrypting a stored key"), "{message}");
    }

    #[test]
    fn an_unreadable_stopped_list_makes_a_tick_fail_with_the_reason_and_the_view_report_it() {
        let s = Store::in_memory();
        let routing = Routing::default();
        watch(&s, keyed_openrouter(&s));
        stored_snapshot_as(&s, SLUG, now_ms() - 60 * MINUTE);
        setting_rows::set(&s, STOPPED_KEY, "nope").unwrap();
        tick(&s, &routing, &no_request);
        assert_eq!(
            last_check(&routing).unwrap().outcome,
            CheckOutcome::Failed { message: "The list of stopped models couldn't be read. Stop or observe a model to replace it.".into() }
        );
        let view = observer_view(&routing, &s).unwrap();
        assert_eq!(view.stopped_problem.as_deref(), Some("The list of stopped models couldn't be read. Stop or observe a model to replace it."));
        assert!(view.models.iter().all(|m| !m.stopped));
        assert_eq!(stopped_row(&s).as_deref(), Some("nope"), "reading and ticking leave the list as it was");
    }

    #[test]
    fn stopping_and_observing_again_change_only_the_stopped_list() {
        let s = Store::in_memory();
        let routing = Routing::default();
        stored_snapshot(&s, now_ms() - 60 * MINUTE);
        let data = |s: &Store| (rows::get(s, SLUG, Kind::Snapshot).unwrap(), rows::get(s, SLUG, Kind::Observations).unwrap());
        let before = data(&s);
        assert!(before.0.is_some() && before.1.is_some());
        assert_eq!(stopped_row(&s), None);

        let view = observe_model(&routing, &s, SLUG, false).unwrap();
        assert_eq!(stopped_row(&s).as_deref(), Some(format!(r#"["{SLUG}"]"#).as_str()));
        assert!(view.models.iter().any(|m| m.model == SLUG && m.stopped));
        assert_eq!(data(&s), before, "the snapshot and observations rows are byte for byte as they were");
        observe_model(&routing, &s, SLUG, false).unwrap();
        assert_eq!(stopped_row(&s).as_deref(), Some(format!(r#"["{SLUG}"]"#).as_str()), "stopping twice lists it once");

        let view = observe_model(&routing, &s, SLUG, true).unwrap();
        assert_eq!(stopped_row(&s).as_deref(), Some("[]"));
        assert!(view.models.iter().any(|m| m.model == SLUG && !m.stopped));
        assert_eq!(data(&s), before);
        observe_model(&routing, &s, SLUG, true).unwrap();
        assert_eq!(stopped_row(&s).as_deref(), Some("[]"), "observing a model that isn't stopped changes nothing");
    }

    #[test]
    fn an_unreadable_stopped_list_is_replaced_by_the_action() {
        for raw in ["nope", "{}", "[1]"] {
            let s = Store::in_memory();
            let routing = Routing::default();
            setting_rows::set(&s, STOPPED_KEY, raw).unwrap();
            let view = observe_model(&routing, &s, SLUG, false).unwrap();
            assert_eq!(stopped_row(&s).as_deref(), Some(format!(r#"["{SLUG}"]"#).as_str()), "{raw}: Stop gives [model]");
            assert_eq!(view.stopped_problem, None, "{raw}");

            setting_rows::set(&s, STOPPED_KEY, raw).unwrap();
            observe_model(&routing, &s, SLUG, true).unwrap();
            assert_eq!(stopped_row(&s).as_deref(), Some("[]"), "{raw}: Observe again gives []");
        }
    }

    #[test]
    fn stopping_refuses_a_bad_slug_a_nitro_slug_and_an_anthropic_or_openai_model() {
        let s = Store::in_memory();
        let routing = Routing::default();
        for observe in [false, true] {
            assert!(observe_model(&routing, &s, "a b", observe).unwrap_err().contains("isn't an OpenRouter model id"));
            assert_eq!(observe_model(&routing, &s, "deepseek/deepseek-v4.1-flash:nitro", observe).unwrap_err(), REFUSE_NITRO_MODEL);
            for model in ["openai/gpt-5.5", "Anthropic/claude-sonnet-5", "~openai/o9"] {
                assert_eq!(observe_model(&routing, &s, model, observe).unwrap_err(), REFUSE_FIRST_PARTY, "{model}");
            }
        }
        assert_eq!(stopped_row(&s), None, "a refusal writes nothing");
    }

    #[test]
    fn a_501st_stopped_slug_is_refused_and_the_list_stays() {
        let s = Store::in_memory();
        let routing = Routing::default();
        let full: Vec<String> = (0..500).map(|i| format!("vendor/m{i}")).collect();
        let raw = serde_json::to_string(&full).unwrap();
        setting_rows::set(&s, STOPPED_KEY, &raw).unwrap();
        let refused = observe_model(&routing, &s, SLUG, false).unwrap_err();
        assert!(refused.starts_with("At most 500 models can be stopped"), "{refused}");
        assert_eq!(stopped_row(&s).as_deref(), Some(raw.as_str()));
        // A model already on the list is not a new one, and freeing a place lets the new one in.
        observe_model(&routing, &s, "vendor/m7", false).unwrap();
        assert_eq!(stopped_row(&s).as_deref(), Some(raw.as_str()));
        observe_model(&routing, &s, "vendor/m7", true).unwrap();
        observe_model(&routing, &s, SLUG, false).unwrap();
        let list: Vec<String> = serde_json::from_str(&stopped_row(&s).unwrap()).unwrap();
        assert_eq!((list.len(), list.last().map(String::as_str), list.contains(&"vendor/m7".to_owned())), (500, Some(SLUG), false));
    }

    #[test]
    fn the_observer_view_lists_stored_models_in_slug_order_without_anthropic_and_openai_ones() {
        let s = Store::in_memory();
        let routing = Routing::default();
        let now = now_ms();
        // Stored out of order, one with two updates, one refused of each author, one unreadable.
        stored_snapshot_as(&s, "zeta/model", now - 3 * 60 * MINUTE);
        stored_snapshot_as(&s, "zeta/model", now - 60 * MINUTE);
        stored_snapshot_as(&s, "alpha/model", now - 10 * MINUTE);
        stored_snapshot_as(&s, "openai/gpt-5.5", now);
        stored_snapshot_as(&s, "Anthropic/claude-sonnet-5", now);
        rows::set(&s, "beta/model", Kind::Snapshot, "nope").unwrap();
        observe_model(&routing, &s, "alpha/model", false).unwrap();

        let view = observer_view(&routing, &s).unwrap();
        let models: Vec<_> = view.models.iter().map(|m| (m.model.as_str(), m.fetched_at_ms, m.sample_count, m.stopped)).collect();
        assert_eq!(
            models,
            [
                ("alpha/model", Some(now - 10 * MINUTE), 1, true),
                ("beta/model", None, 0, false),
                ("zeta/model", Some(now - 60 * MINUTE), 2, false),
            ]
        );
        assert_eq!((view.provider_id, view.provider_name, view.problem, view.last_check, view.stopped_problem), (None, None, None, None, None));
        assert_eq!(view.next_at_ms, None, "the observer hasn't started");
    }

    #[test]
    fn the_observer_view_gives_the_watched_provider_the_last_check_and_the_next_one() {
        let s = Store::in_memory();
        let routing = Routing::default();
        let start = 1_700_000_000_000;
        routing.observer.lock().unwrap().started_ms = Some(start);

        // Watching a provider with no key of its own: its id, name and the problem.
        let unkeyed = openrouter(&s);
        setting_rows::set(&s, WATCH_KEY, &unkeyed.to_string()).unwrap();
        let view = observer_view(&routing, &s).unwrap();
        assert_eq!((view.provider_id, view.provider_name.as_deref(), view.problem.as_deref()), (Some(unkeyed), Some("OpenRouter"), Some(NO_OWN_KEY)));
        assert_eq!((view.last_check, view.next_at_ms), (None, Some(start + 2 * MINUTE)), "before any check: 2 minutes after start");

        // A tick that ends records the check, and the next one is 30 minutes after it.
        routing.finish_tick(start + 2 * MINUTE + 500, Some(CheckOutcome::NothingDue));
        let view = observer_view(&routing, &s).unwrap();
        assert_eq!(view.last_check, Some(LastCheck { at_ms: start + 2 * MINUTE + 500, outcome: CheckOutcome::NothingDue }));
        assert_eq!(view.next_at_ms, Some(start + 2 * MINUTE + 500 + 30 * MINUTE));

        // A tick that found nothing watched moves the schedule and leaves the last check.
        routing.finish_tick(start + 40 * MINUTE, None);
        let view = observer_view(&routing, &s).unwrap();
        assert_eq!(view.last_check.map(|c| c.at_ms), Some(start + 2 * MINUTE + 500));
        assert_eq!(view.next_at_ms, Some(start + 70 * MINUTE));

        // A watched provider that was deleted: no name, and its problem.
        setting_rows::set(&s, WATCH_KEY, "404").unwrap();
        let view = observer_view(&routing, &s).unwrap();
        assert_eq!((view.provider_id, view.provider_name, view.problem.as_deref()), (Some(404), None, Some("That provider no longer exists.")));
    }

    // ── Speed history and the Refresh log ──

    const DAY: i64 = 24 * 60 * MINUTE;

    fn observation(tag: &str, at_ms: i64) -> RoutingObservation {
        RoutingObservation {
            tag: tag.into(),
            observed_at_ms: at_ms,
            uptime_1d: Some(99.9),
            uptime_5m: Some(100.0),
            status: 0,
            tps_p50: Some(60.0),
            tps_p90: Some(80.0),
            latency_p50_ms: Some(900.0),
            latency_p90_ms: Some(1800.0),
        }
    }

    /// Adds observations to what `stored_snapshot` left for `SLUG`.
    fn add_observations(s: &Store, extra: &[RoutingObservation]) {
        let mut all: Vec<RoutingObservation> = read(s, SLUG, Kind::Observations).unwrap();
        all.extend_from_slice(extra);
        write(s, SLUG, Kind::Observations, &all).unwrap();
    }

    fn candidate_tags(s: &Store) -> Vec<String> {
        let (result, _) = rank_stored(s, SLUG, None, None, now_ms()).unwrap().unwrap();
        result.candidates.into_iter().map(|c| c.tag).collect()
    }

    #[test]
    fn the_history_of_the_golden_model_has_one_row_per_candidate_tag_in_the_rankings_order() {
        let s = Store::in_memory();
        stored_snapshot(&s, now_ms());
        let candidates = candidate_tags(&s);
        assert!(candidates.len() >= 3, "{candidates:?}");

        let rows = model_history(&s, SLUG).unwrap();
        assert_eq!(rows.iter().map(|r| r.tag.clone()).collect::<Vec<_>>(), candidates, "one row per candidate tag, in the ranking's order");
        assert!(rows.iter().all(|r| r.samples == 1 && r.median_tps.is_some() && r.median_latency_s.is_some()), "{rows:?}");

        // A tag the ranking doesn't list comes after the candidates, though it sorts before them.
        add_observations(&s, &[observation("aaa/extra", now_ms())]);
        let rows = model_history(&s, SLUG).unwrap();
        let tags: Vec<_> = rows.iter().map(|r| r.tag.as_str()).collect();
        assert_eq!(tags.len(), candidates.len() + 1);
        assert_eq!((&tags[..candidates.len()], tags.last()), (&candidates.iter().map(String::as_str).collect::<Vec<_>>()[..], Some(&"aaa/extra")));
    }

    #[test]
    fn the_history_of_a_model_with_nothing_stored_is_empty() {
        let s = Store::in_memory();
        assert_eq!(model_history(&s, SLUG).unwrap(), vec![]);
        // Observations without a snapshot (nothing to rank) still show, by name.
        write(&s, SLUG, Kind::Observations, &[observation("b/fp8", now_ms()), observation("a/fp8", now_ms())]).unwrap();
        let tags: Vec<_> = model_history(&s, SLUG).unwrap().into_iter().map(|r| r.tag).collect();
        assert_eq!(tags, ["a/fp8", "b/fp8"]);
        // A stored value that isn't observations is no history, not an error.
        rows::set(&s, SLUG, Kind::Observations, "nope").unwrap();
        assert_eq!(model_history(&s, SLUG).unwrap(), vec![]);
        assert_eq!(rows::get(&s, SLUG, Kind::Observations).unwrap().as_deref(), Some("nope"), "reading leaves it as it was");
    }

    #[test]
    fn the_history_keeps_only_the_saved_number_of_days() {
        let s = Store::in_memory();
        stored_snapshot(&s, now_ms());
        add_observations(&s, &[observation("old/fp8", now_ms() - 3 * DAY)]);
        let tags = |s: &Store| model_history(s, SLUG).unwrap().into_iter().map(|r| r.tag).collect::<Vec<_>>();
        assert!(tags(&s).contains(&"old/fp8".to_owned()), "three days is within the default seven");
        save_settings(&s, &RoutingSettings { observation_max_age_days: 1.0, ..RoutingSettings::default() }).unwrap();
        assert!(!tags(&s).contains(&"old/fp8".to_owned()), "one day drops it");
        assert!(!tags(&s).is_empty(), "and keeps the recent ones");
        reset_settings(&s).unwrap();
        assert!(tags(&s).contains(&"old/fp8".to_owned()));
    }

    #[test]
    fn the_history_refuses_a_bad_slug_a_nitro_slug_and_an_anthropic_or_openai_model() {
        let s = Store::in_memory();
        assert!(model_history(&s, "a b").unwrap_err().contains("isn't an OpenRouter model id"));
        assert_eq!(model_history(&s, "deepseek/deepseek-v4.1-flash:nitro").unwrap_err(), REFUSE_NITRO_MODEL);
        for model in ["anthropic/claude-sonnet-5", "OpenAI/gpt-5.5", "~openai/o9"] {
            // Even with observations stored under that slug.
            write(&s, model, Kind::Observations, &[observation("a/fp8", now_ms())]).unwrap();
            assert_eq!(model_history(&s, model).unwrap_err(), REFUSE_FIRST_PARTY, "{model}");
        }
    }

    fn parts(estimate: f64, spent: f64, probed: &[&str]) -> RefreshParts {
        (estimate, spent, probed.iter().map(|t| (*t).to_owned()).collect(), vec![], 1, vec!["a note".into()])
    }

    #[test]
    fn a_successful_refresh_logs_the_endpoints_tested_the_spend_and_the_estimate() {
        let entry = refresh_entry(1_700_000_000_000, SLUG, "My OpenRouter", &Ok(parts(0.05, 0.0123, &["a/fp8", "b/bf16"])));
        assert_eq!(
            entry,
            RefreshLogEntry {
                at_ms: 1_700_000_000_000,
                model: SLUG.into(),
                provider_name: "My OpenRouter".into(),
                error: None,
                endpoints_tested: 2,
                spent_usd: 0.0123,
                estimate_usd: 0.05,
            }
        );
        let nothing_tested = refresh_entry(5, SLUG, "P", &Ok(parts(0.0, 0.0, &[])));
        assert_eq!((nothing_tested.error, nothing_tested.endpoints_tested, nothing_tested.spent_usd, nothing_tested.estimate_usd), (None, 0, 0.0, 0.0));
    }

    #[test]
    fn a_failed_refresh_logs_its_message_and_no_figures() {
        let entry = refresh_entry(42, SLUG, "My OpenRouter", &Err("Could not reach OpenRouter.".into()));
        assert_eq!(
            entry,
            RefreshLogEntry {
                at_ms: 42,
                model: SLUG.into(),
                provider_name: "My OpenRouter".into(),
                error: Some("Could not reach OpenRouter.".into()),
                endpoints_tested: 0,
                spent_usd: 0.0,
                estimate_usd: 0.0,
            }
        );
    }

    #[test]
    fn a_figure_that_is_not_finite_cannot_make_the_stored_log_unreadable() {
        let entry = refresh_entry(1, SLUG, "P", &Ok(parts(f64::INFINITY, f64::NAN, &["a/fp8"])));
        assert_eq!((entry.spent_usd, entry.estimate_usd, entry.endpoints_tested), (0.0, 0.0, 1));
        let s = Store::in_memory();
        record_refresh(&s, entry.clone());
        assert_eq!(refresh_log(&s).unwrap(), RefreshLogView { entries: vec![entry], problem: None });
    }

    fn logged(n: i64) -> RefreshLogEntry {
        RefreshLogEntry { at_ms: n, model: SLUG.into(), provider_name: "OpenRouter".into(), error: None, endpoints_tested: 2, spent_usd: 0.01, estimate_usd: 0.05 }
    }

    fn log_row(s: &Store) -> Option<String> {
        setting_rows::get(s, REFRESH_LOG_KEY).unwrap()
    }

    #[test]
    fn the_log_gets_the_newest_refresh_first_and_keeps_50() {
        let s = Store::in_memory();
        assert_eq!(log_row(&s), None, "no row until the first Refresh is logged");
        for n in 1..=3 {
            record_refresh(&s, logged(n));
        }
        let times = |s: &Store| refresh_log(s).unwrap().entries.iter().map(|e| e.at_ms).collect::<Vec<_>>();
        assert_eq!(times(&s), [3, 2, 1]);
        for n in 4..=51 {
            record_refresh(&s, logged(n));
        }
        assert_eq!(times(&s), (2..=51).rev().collect::<Vec<_>>(), "the oldest of the 51 is gone");
        let stored: serde_json::Value = serde_json::from_str(&log_row(&s).unwrap()).unwrap();
        assert_eq!(stored.as_array().unwrap().len(), 50);
        assert_eq!(refresh_log(&s).unwrap().problem, None);
    }

    #[test]
    fn a_log_that_cannot_be_read_is_left_as_it_was_and_reported() {
        for raw in ["nope", "{}", r#"[{"atMs":1,"bogus":true}]"#] {
            let s = Store::in_memory();
            setting_rows::set(&s, REFRESH_LOG_KEY, raw).unwrap();
            record_refresh(&s, logged(1));
            assert_eq!(log_row(&s).as_deref(), Some(raw), "{raw}: not logged, not rewritten");
            let view = refresh_log(&s).unwrap();
            assert_eq!((view.entries, view.problem.as_deref()), (vec![], Some("The Refresh log couldn't be read.")), "{raw}");
            assert_eq!(log_row(&s).as_deref(), Some(raw), "{raw}: reading doesn't rewrite it either");
        }
    }

    #[test]
    fn the_log_view_of_a_stored_log_is_its_entries_newest_first() {
        let s = Store::in_memory();
        assert_eq!(refresh_log(&s).unwrap(), RefreshLogView { entries: vec![], problem: None });
        setting_rows::set(&s, REFRESH_LOG_KEY, &serde_json::to_string(&[logged(9), logged(4)]).unwrap()).unwrap();
        assert_eq!(refresh_log(&s).unwrap(), RefreshLogView { entries: vec![logged(9), logged(4)], problem: None });
    }

    #[test]
    fn a_refresh_that_ends_starts_the_cooldown_and_is_logged_whether_it_worked_or_not() {
        let s = Store::in_memory();
        let routing = Routing::default();
        let provider = provider_rows::get(&s, keyed_openrouter(&s)).unwrap().unwrap();
        let before = now_ms();

        let failed = end_refresh(&routing, &s, &provider, SLUG, Err("Could not reach OpenRouter.".into()));
        assert_eq!(failed.unwrap_err(), "Could not reach OpenRouter.", "the result is passed on as it was");
        assert!(routing.wait_secs(SLUG, now_ms()) > 0, "the cooldown started");
        let log = refresh_log(&s).unwrap().entries;
        assert_eq!(log.len(), 1);
        assert_eq!((log[0].model.as_str(), log[0].provider_name.as_str(), log[0].error.as_deref()), (SLUG, "OpenRouter", Some("Could not reach OpenRouter.")));
        assert!(log[0].at_ms >= before, "timed when the Refresh ended");
        assert_eq!(routing.ended.lock().unwrap().get(SLUG).copied(), Some(log[0].at_ms), "the cooldown and the log share the end time");

        let worked = end_refresh(&routing, &s, &provider, SLUG, Ok(parts(0.05, 0.0123, &["a/fp8"]))).unwrap();
        assert_eq!(worked.2, ["a/fp8"], "and the parts too");
        let log = refresh_log(&s).unwrap().entries;
        assert_eq!(log.len(), 2);
        assert_eq!((log[0].error.as_deref(), log[0].endpoints_tested, log[0].spent_usd, log[0].estimate_usd), (None, 1, 0.0123, 0.05), "newest first");
        assert_eq!(log[1].error.as_deref(), Some("Could not reach OpenRouter."));
    }

    #[test]
    fn a_refresh_refused_before_any_request_is_not_logged() {
        let s = Store::in_memory();
        let routing = Routing::default();
        let keyed = keyed_openrouter(&s);
        let refused = |provider: u32, model: &str| refresh(&routing, &s, provider, model, None).unwrap_err();

        assert!(refused(keyed, "a b").contains("isn't an OpenRouter model id"));
        assert_eq!(refused(keyed, "openai/gpt-5.5"), REFUSE_FIRST_PARTY);
        assert_eq!(refused(404, SLUG), "That provider no longer exists.");
        // A provider with no key of its own (providers are named uniquely, so in a store of its own).
        let other = Store::in_memory();
        let unkeyed = openrouter(&other);
        assert_eq!(refresh(&routing, &other, unkeyed, SLUG, None).unwrap_err(), NO_OWN_KEY);
        assert_eq!(setting_rows::get(&other, REFRESH_LOG_KEY).unwrap(), None);
        // The cooldown, then a busy slot (cleared in between).
        routing.ended.lock().unwrap().insert(SLUG.into(), now_ms());
        assert!(refused(keyed, SLUG).contains("less than a minute ago"));
        routing.ended.lock().unwrap().clear();
        let busy = routing.take(SLUG).unwrap();
        assert!(refused(keyed, SLUG).contains("already running"));
        drop(busy);
        // A key that can't be read: still before any request.
        let credential = provider_rows::get(&s, keyed).unwrap().unwrap().credential_id.unwrap();
        s.with(|c| c.execute("UPDATE credentials SET blob = x'00' WHERE id = ?1", [credential]).map(|_| ())).unwrap();
        assert!(refused(keyed, SLUG).starts_with("decrypting a stored key"));

        assert_eq!(log_row(&s), None, "no routing_refresh_log row");
        assert!(routing.ended.lock().unwrap().is_empty(), "and none of them started a cooldown");
    }
}
