//! OpenRouter routing tiers for opencode (D57, brought forward from PLAN P12), run as Chorus 1 runs
//! them. A Refresh, with an OpenRouter provider's own key, fetches a model's endpoints, runs the two
//! zero-cost account checks, ranks, tests prompt caching on the endpoints that need it (at most
//! $0.05) and ranks again. The observer refetches only the endpoints of every refreshed model every
//! 30 minutes, with the provider the user chose for it. A launch ranks what is stored and keeps the
//! selection with the session, so Restart reuses it. Every decision is `chorus_core::routing`; this
//! module does the IO.

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
    AccountEligibility, CacheRecord, CacheVerification, EndpointSnapshot, Quantization, RankInput, RankedTier, RoutingObservation, RoutingProfileId,
    RoutingSettings, TierResult, is_model_slug,
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

/// Refreshes under way and when each model's last one ended.
#[derive(Default)]
pub struct Routing {
    busy: Mutex<HashSet<String>>,
    ended: Mutex<HashMap<String, i64>>,
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

/// The ranker's settings: the defaults, under anything stored (P12's inspector will edit them).
pub fn settings(store: &Store) -> RoutingSettings {
    parse_routing_settings_value(setting_rows::get(store, SETTINGS_KEY).ok().flatten().as_deref()).value
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
    routing.ended.lock().expect("routing lock").insert(model.to_owned(), now_ms());
    let (estimate_usd, spent_usd, probed, not_probed, rejected_rows, notes) = result?;
    Ok(RefreshOutcome { view: view(routing, store, provider_id, model, effort)?, estimate_usd, spent_usd, probed, not_probed, rejected_rows, notes })
}

type RefreshParts = (f64, f64, Vec<String>, Vec<ProbeSkip>, u32, Vec<String>);

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

/// Every 30 minutes (the first two minutes after start): with the watched provider's key, the free
/// endpoints request for each refreshed model not refreshed in the last 25 minutes. No account
/// check, no cache test. An answer that refuses the key or rate-limits stops the tick.
pub fn start_observer(manager: Arc<SessionManager>, routing: Arc<Routing>) {
    let _ = std::thread::Builder::new().name("routing-observer".into()).spawn(move || {
        std::thread::sleep(OBSERVER_FIRST);
        loop {
            observe_tick(manager.store(), &routing);
            std::thread::sleep(OBSERVER_EVERY);
        }
    });
}

fn observe_tick(store: &Store, routing: &Routing) {
    let Some(provider) = watched_provider(store).and_then(|id| routing_provider(store, id).ok()) else { return };
    let models = rows::models_with_snapshot(store).unwrap_or_default();
    let now = now_ms();
    let due: Vec<String> = models
        .into_iter()
        .filter(|m| read::<EndpointSnapshot>(store, m, Kind::Snapshot).is_none_or(|s| now - s.fetched_at_ms >= OBSERVER_FRESH_MS))
        .collect();
    if due.is_empty() {
        return;
    }
    let Ok(key) = key_for(store, &provider) else { return };
    let settings = settings(store);
    for model in due {
        let Some(_slot) = routing.take(&model) else { continue };
        match client::fetch_endpoints(&key, &model) {
            Ok((endpoints, _)) => {
                let snapshot = EndpointSnapshot { fetched_at_ms: now_ms(), endpoints };
                if write(store, &model, Kind::Snapshot, &snapshot).is_ok() {
                    let _ = observe(store, &model, &snapshot, &settings);
                }
            }
            Err(f) if f.stops_further_calls() => return,
            Err(_) => {}
        }
    }
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
}
