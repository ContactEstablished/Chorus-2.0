//! What a routing tier means at launch: the pure decisions of Chorus 1's `launchCore.ts`. A
//! chosen tier resolves to the exact selection a session runs with, or to a refusal (no
//! snapshot, a stale one, an empty tier); a launch's facts decide whether it is routed, refused,
//! or runs on OpenRouter default; a stored selection is checked before a relaunch reuses it.
//!
//! "OpenRouter default" sends nothing of routing's: no provider object and the plain model id
//! (a `:nitro` id still declares its effort variants, [`super::payload::nitro_variants`]). It is
//! remembered as the [`RoutingLaunchChoice::Default`] choice.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use super::js;
use super::payload::{NITRO_SUFFIX, build_nitro_selection};
use super::types::{OpenRouterProviderPrefs, ProviderDataCollection, RankedTier, RoutingSettings, TierResult, is_model_slug};
use crate::agent::AgentKind;

/// The tiers a launch can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum RoutingLaunchTier {
    Budget,
    Balanced,
    Fast,
    Nitro,
}

impl RoutingLaunchTier {
    pub fn label(self) -> &'static str {
        match self {
            Self::Budget => "Budget",
            Self::Balanced => "Balanced",
            Self::Fast => "Fast",
            Self::Nitro => "Nitro",
        }
    }

    /// The ranked tier, or `None` for Nitro.
    pub fn ranked(self) -> Option<RankedTier> {
        match self {
            Self::Budget => Some(RankedTier::Budget),
            Self::Balanced => Some(RankedTier::Balanced),
            Self::Fast => Some(RankedTier::Fast),
            Self::Nitro => None,
        }
    }
}

/// What a launch can remember per model (K8): a tier, or `Default` for an eligible launch that
/// sent none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum RoutingLaunchChoice {
    Budget,
    Balanced,
    Fast,
    Nitro,
    Default,
}

impl From<RoutingLaunchTier> for RoutingLaunchChoice {
    fn from(t: RoutingLaunchTier) -> Self {
        match t {
            RoutingLaunchTier::Budget => Self::Budget,
            RoutingLaunchTier::Balanced => Self::Balanced,
            RoutingLaunchTier::Fast => Self::Fast,
            RoutingLaunchTier::Nitro => Self::Nitro,
        }
    }
}

// ── Resolution (K2, K5, MR-D26) ──

pub const LAUNCH_NO_SNAPSHOT: &str = "No endpoint snapshot is stored for this model yet. Refresh first.";
pub const LAUNCH_FAILED: &str = "Routing operation failed.";

pub fn stale_snapshot_message(snapshot_max_age_minutes: f64) -> String {
    format!("The endpoint snapshot for this model is more than {} minutes old. Refresh first.", js::number(snapshot_max_age_minutes))
}

pub fn empty_tier_message(tier: RankedTier) -> String {
    format!("{} has no eligible endpoints. Choose another tier or OpenRouter default.", tier.label())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum LaunchRefusalCode {
    NoSnapshot,
    /// The tiers given are another model's.
    OperationFailed,
    /// MR-D26: a ranked tier on a snapshot older than `snapshot_max_age_minutes`.
    SnapshotStale,
    TierEmpty,
}

/// Why a tier can't be used for this launch, in words to show as they are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LaunchRefusal {
    pub code: LaunchRefusalCode,
    pub message: String,
}

/// K5: the exact routing a session launched with, stored with it and reused unchanged by a
/// relaunch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoutingLaunchSelection {
    pub tier: RoutingLaunchTier,
    /// The base slug, never suffixed.
    pub model: String,
    /// The id opencode sends: the slug, or the slug plus `:nitro`.
    pub sent_model_id: String,
    /// `None` only for Nitro when collection is allowed.
    pub provider: Option<OpenRouterProviderPrefs>,
    /// The `order` tags; empty for Nitro.
    pub endpoints: Vec<String>,
    #[specta(type = f64)]
    pub computed_at_ms: i64,
    /// `None` exactly for Nitro.
    #[specta(type = Option<f64>)]
    pub snapshot_fetched_at_ms: Option<i64>,
}

impl RoutingLaunchSelection {
    /// The OpenRouter `provider` object this launch sends, if any.
    pub fn provider_value(&self) -> Option<Value> {
        self.provider.as_ref().map(OpenRouterProviderPrefs::to_value)
    }

    /// Chorus 1's strict rules (C4), so a stored row edited by hand can never relaunch with an id,
    /// provider or order that disagree. Nitro's provider is none or `{ data_collection }` alone;
    /// a ranked tier's pins its order with no fallback and names its quantizations.
    pub fn is_consistent(&self) -> bool {
        let prefs_ok = |p: &OpenRouterProviderPrefs| {
            p.order.as_ref().is_none_or(|o| o.iter().all(|t| !t.is_empty())) && p.allow_fallbacks != Some(true) && p.require_parameters != Some(false)
        };
        if !is_model_slug(&self.model)
            || !is_model_slug(&self.sent_model_id)
            || self.model.ends_with(NITRO_SUFFIX)
            || self.endpoints.iter().any(String::is_empty)
            || !self.provider.as_ref().is_none_or(prefs_ok)
        {
            return false;
        }
        match self.tier {
            RoutingLaunchTier::Nitro => {
                let deny_only = OpenRouterProviderPrefs { data_collection: Some(ProviderDataCollection::Deny), ..OpenRouterProviderPrefs::default() };
                self.sent_model_id == format!("{}{NITRO_SUFFIX}", self.model)
                    && self.endpoints.is_empty()
                    && self.snapshot_fetched_at_ms.is_none()
                    && self.provider.as_ref().is_none_or(|p| *p == deny_only)
            }
            _ => self.provider.as_ref().is_some_and(|p| {
                self.sent_model_id == self.model
                    && p.allow_fallbacks == Some(false)
                    && p.require_parameters == Some(true)
                    && p.quantizations.is_some()
                    && !self.endpoints.is_empty()
                    && self.snapshot_fetched_at_ms.is_some()
                    && p.order.as_deref().unwrap_or_default() == self.endpoints.as_slice()
            }),
        }
    }
}

/// Resolves a tier for a launch at `computed_at_ms`. Nitro (C3) always resolves and ignores
/// `result`. A ranked tier (C5), first refusal winning: no tiers; tiers for another model; a
/// stale snapshot (before emptiness, since a refresh may fill the tier); an empty tier.
pub fn resolve_launch_selection(
    tier: RoutingLaunchTier,
    model: &str,
    result: Option<&TierResult>,
    settings: &RoutingSettings,
    computed_at_ms: i64,
) -> Result<RoutingLaunchSelection, LaunchRefusal> {
    let refuse = |code, message: String| Err(LaunchRefusal { code, message });
    let Some(ranked) = tier.ranked() else {
        let nitro = build_nitro_selection(model, None, &[], settings);
        return Ok(RoutingLaunchSelection {
            tier,
            model: model.to_owned(),
            sent_model_id: nitro.model,
            provider: nitro.provider,
            endpoints: Vec::new(),
            computed_at_ms,
            snapshot_fetched_at_ms: None,
        });
    };
    let Some(result) = result else { return refuse(LaunchRefusalCode::NoSnapshot, LAUNCH_NO_SNAPSHOT.into()) };
    if result.model != model {
        return refuse(LaunchRefusalCode::OperationFailed, LAUNCH_FAILED.into());
    }
    if result.stale {
        return refuse(LaunchRefusalCode::SnapshotStale, stale_snapshot_message(settings.snapshot_max_age_minutes));
    }
    let Some(chosen) = result.tiers.get(ranked) else { return refuse(LaunchRefusalCode::TierEmpty, empty_tier_message(ranked)) };
    Ok(RoutingLaunchSelection {
        tier,
        model: model.to_owned(),
        sent_model_id: model.to_owned(),
        provider: Some(chosen.provider.clone()),
        endpoints: chosen.endpoints.clone(),
        computed_at_ms,
        snapshot_fetched_at_ms: Some(result.snapshot_fetched_at_ms),
    })
}

/// A stored selection, as read back.
#[derive(Debug, Clone, PartialEq)]
pub enum StoredSelection {
    None,
    /// Not JSON, not the shape, or inconsistent.
    Invalid,
    Selection(RoutingLaunchSelection),
}

/// A session's stored routing: `None` → none; anything that isn't a consistent selection → invalid.
pub fn parse_stored_selection(text: Option<&str>) -> StoredSelection {
    let Some(text) = text else { return StoredSelection::None };
    match serde_json::from_str::<RoutingLaunchSelection>(text) {
        Ok(s) if s.is_consistent() => StoredSelection::Selection(s),
        _ => StoredSelection::Invalid,
    }
}

// ── Launch planning (K3, K7, K10; C8) ──

pub const REFUSE_NOT_OPENCODE: &str = "Routing tiers apply only to OpenCode launches.";
pub const REFUSE_UNAVAILABLE: &str = "Model routing is not available right now. Launch with OpenRouter default instead.";
pub const REFUSE_NO_CREDENTIAL: &str = "Routing tiers need an OpenRouter API-key credential.";
pub const REFUSE_CREDENTIAL: &str =
    "This credential cannot be used for model routing. Routing needs an OpenRouter API-key credential for the OpenRouter gateway.";
pub const REFUSE_NO_MODEL: &str = "Choose a model to use a routing tier.";
pub const REFUSE_NITRO_MODEL: &str = "This model already ends in :nitro. Choose the base model and the Nitro tier instead.";
pub const REFUSE_UNKNOWN_MODEL: &str = "Model routing does not know this model.";
pub const REFUSE_PROFILE_ENV: &str =
    "The launch profile sets OPENCODE_CONFIG_CONTENT, which a routed launch also needs. Remove it from the profile, or launch with OpenRouter default.";
pub const REFUSE_STORED_INVALID: &str = "This session's saved routing could not be read. Start a new session from the launch dialog.";
pub const REFUSE_RELAUNCH_UNAVAILABLE: &str = "Model routing is not available right now, so this routed session cannot be relaunched.";
pub const REFUSE_RELAUNCH_CREDENTIAL: &str = "This session was routed, but its launch profile's credential can no longer be used for model routing.";
pub const REFUSE_FIRST_PARTY: &str = "Chorus doesn't route Anthropic or OpenAI models through OpenRouter. Run them in Claude Code or Codex on your own sign-in.";

/// Why routing won't take this model: Anthropic and OpenAI models run in Claude Code and Codex on
/// the user's own sign-in. The author is the text before the first `/` (the whole slug without
/// one), minus one leading `~`, compared ignoring ASCII case. A stored selection isn't checked
/// here: a session that already has one keeps it.
pub fn routing_refusal(model: &str) -> Option<&'static str> {
    let author = model.split('/').next().unwrap_or_default();
    let author = author.strip_prefix('~').unwrap_or(author);
    (author.eq_ignore_ascii_case("anthropic") || author.eq_ignore_ascii_case("openai")).then_some(REFUSE_FIRST_PARTY)
}

/// What a launch knows when it decides about routing.
#[derive(Debug, Clone, Copy)]
pub struct RoutingLaunchFacts<'a> {
    pub agent: AgentKind,
    /// The chosen tier; `None` is OpenRouter default.
    pub tier: Option<RoutingLaunchTier>,
    /// The launch's credential.
    pub credential_id: Option<u32>,
    pub model: Option<&'a str>,
    /// False when routing can't run right now (its credentials or models couldn't be read).
    pub routing_available: bool,
    /// The credentials routing accepts.
    pub routing_credential_ids: &'a [u32],
    /// The models routing can rank.
    pub known_models: &'a [String],
    /// The launch profile's environment variable names.
    pub profile_env_keys: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingLaunchPlan {
    /// Today's launch; nothing is recorded.
    Unrouted,
    /// Eligible, but no tier chosen: OpenRouter default, recorded as the `Default` choice.
    Default { model: String },
    Routed { model: String, tier: RoutingLaunchTier, credential_id: u32 },
    /// A chosen tier is never silently dropped.
    Refused { reason: String },
}

/// Windows environment names ignore case, so the K7 collision does too.
fn sets_config_content(keys: &[String]) -> bool {
    keys.iter().any(|k| k.eq_ignore_ascii_case("OPENCODE_CONFIG_CONTENT"))
}

/// K3, first failure wins, in this order.
fn launch_eligibility<'a>(f: &RoutingLaunchFacts<'a>) -> Result<(&'a str, u32), &'static str> {
    if f.agent != AgentKind::Opencode {
        return Err(REFUSE_NOT_OPENCODE);
    }
    if !f.routing_available {
        return Err(REFUSE_UNAVAILABLE);
    }
    let credential = f.credential_id.ok_or(REFUSE_NO_CREDENTIAL)?;
    if !f.routing_credential_ids.contains(&credential) {
        return Err(REFUSE_CREDENTIAL);
    }
    let model = f.model.ok_or(REFUSE_NO_MODEL)?;
    if model.ends_with(NITRO_SUFFIX) {
        return Err(REFUSE_NITRO_MODEL);
    }
    if !f.known_models.iter().any(|m| m == model) {
        return Err(REFUSE_UNKNOWN_MODEL);
    }
    Ok((model, credential))
}

/// K3, K7, K8 (C8). A profile that sets `OPENCODE_CONFIG_CONTENT` makes a launch not routable
/// (K7). With no tier: eligible and not blocked is `Default`, otherwise `Unrouted`. With a tier:
/// ineligible or blocked is `Refused`, otherwise `Routed`.
pub fn plan_routing_launch(f: &RoutingLaunchFacts<'_>) -> RoutingLaunchPlan {
    let eligibility = launch_eligibility(f);
    let blocked = sets_config_content(f.profile_env_keys);
    let Some(tier) = f.tier else {
        return match eligibility {
            Ok((model, _)) if !blocked => RoutingLaunchPlan::Default { model: model.to_owned() },
            _ => RoutingLaunchPlan::Unrouted,
        };
    };
    match eligibility {
        Err(reason) => RoutingLaunchPlan::Refused { reason: reason.to_owned() },
        Ok(_) if blocked => RoutingLaunchPlan::Refused { reason: REFUSE_PROFILE_ENV.to_owned() },
        Ok((model, credential_id)) => RoutingLaunchPlan::Routed { model: model.to_owned(), tier, credential_id },
    }
}

/// What a relaunch knows.
#[derive(Debug, Clone, Copy)]
pub struct RoutingRelaunchFacts<'a> {
    pub agent: AgentKind,
    /// The session's stored selection, as text.
    pub routing_json: Option<&'a str>,
    /// The launch profile's credential.
    pub credential_id: Option<u32>,
    pub routing_available: bool,
    pub routing_credential_ids: &'a [u32],
    pub profile_env_keys: &'a [String],
}

#[derive(Debug, Clone, PartialEq)]
pub enum RoutingRelaunchPlan {
    Unrouted,
    /// Reapply the stored selection unchanged; there is no re-rank.
    Routed(RoutingLaunchSelection),
    Refused { reason: String },
}

/// K10, first failure wins: an unreadable selection, the agent, routing's availability, the
/// credential, the profile's environment.
pub fn plan_routing_relaunch(f: &RoutingRelaunchFacts<'_>) -> RoutingRelaunchPlan {
    let refuse = |reason: &str| RoutingRelaunchPlan::Refused { reason: reason.to_owned() };
    let selection = match parse_stored_selection(f.routing_json) {
        StoredSelection::None => return RoutingRelaunchPlan::Unrouted,
        StoredSelection::Invalid => return refuse(REFUSE_STORED_INVALID),
        StoredSelection::Selection(s) => s,
    };
    if f.agent != AgentKind::Opencode {
        return refuse(REFUSE_NOT_OPENCODE);
    }
    if !f.routing_available {
        return refuse(REFUSE_RELAUNCH_UNAVAILABLE);
    }
    if !f.credential_id.is_some_and(|c| f.routing_credential_ids.contains(&c)) {
        return refuse(REFUSE_RELAUNCH_CREDENTIAL);
    }
    if sets_config_content(f.profile_env_keys) {
        return refuse(REFUSE_PROFILE_ENV);
    }
    RoutingRelaunchPlan::Routed(selection)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::test_support::*;
    use crate::routing::tiers::compute_tiers;
    use crate::routing::types::{DataCollection, Quantization, RoutingProfileId};
    use serde_json::json;

    const STALE_NOW: &str = "2026-10-02T10:05:00.001Z";
    const MESSAGE_STALE_60: &str = "The endpoint snapshot for this model is more than 60 minutes old. Refresh first.";
    const C: u32 = 7;
    const D: u32 = 9;

    fn strings(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_owned()).collect()
    }

    fn provider(order: &[&str]) -> OpenRouterProviderPrefs {
        OpenRouterProviderPrefs {
            order: Some(strings(order)),
            allow_fallbacks: Some(false),
            require_parameters: Some(true),
            quantizations: Some(vec![Quantization::Fp8]),
            data_collection: Some(ProviderDataCollection::Deny),
        }
    }

    const BUDGET: [&str; 3] = ["deepinfra/fp8", "streamlake/fp8", "gmicloud/fp8"];
    const BALANCED: [&str; 3] = ["deepinfra/fp8", "streamlake/fp8", "makora/fp8"];
    const FAST: [&str; 3] = ["venice/fp8", "baidu/fp8", "parasail/fp8"];

    fn balanced_selection() -> RoutingLaunchSelection {
        RoutingLaunchSelection {
            tier: RoutingLaunchTier::Balanced,
            model: SLUG.into(),
            sent_model_id: SLUG.into(),
            provider: Some(provider(&BALANCED)),
            endpoints: strings(&BALANCED),
            computed_at_ms: ms(NOW),
            snapshot_fetched_at_ms: Some(ms(FETCHED_AT)),
        }
    }

    fn nitro_selection() -> RoutingLaunchSelection {
        RoutingLaunchSelection {
            tier: RoutingLaunchTier::Nitro,
            model: SLUG.into(),
            sent_model_id: format!("{SLUG}:nitro"),
            provider: Some(OpenRouterProviderPrefs { data_collection: Some(ProviderDataCollection::Deny), ..OpenRouterProviderPrefs::default() }),
            endpoints: vec![],
            computed_at_ms: ms(NOW),
            snapshot_fetched_at_ms: None,
        }
    }

    fn golden_at(now: &str, settings: RoutingSettings) -> TierResult {
        let inputs = Inputs { now_ms: ms(now), settings, ..Inputs::golden(RoutingProfileId::Interactive) };
        compute_tiers(&inputs.input()).unwrap()
    }

    fn golden() -> TierResult {
        golden_at(NOW, RoutingSettings::default())
    }

    fn resolve(tier: RoutingLaunchTier, result: Option<&TierResult>, settings: &RoutingSettings) -> Result<RoutingLaunchSelection, LaunchRefusal> {
        resolve_launch_selection(tier, SLUG, result, settings, ms(NOW))
    }

    fn refusal(code: LaunchRefusalCode, message: &str) -> Result<RoutingLaunchSelection, LaunchRefusal> {
        Err(LaunchRefusal { code, message: message.into() })
    }

    use RoutingLaunchTier::{Balanced, Budget, Fast, Nitro};

    #[test]
    fn l1_balanced_on_the_golden_result_is_the_balanced_selection_exactly() {
        assert_eq!(resolve(Balanced, Some(&golden()), &RoutingSettings::default()), Ok(balanced_selection()));
    }

    #[test]
    fn l2_budget_and_fast_take_their_golden_orders_providers_and_times() {
        let s = RoutingSettings::default();
        let g = golden();
        let expect = |tier, order: &[&str]| RoutingLaunchSelection { tier, provider: Some(provider(order)), endpoints: strings(order), ..balanced_selection() };
        assert_eq!(resolve(Budget, Some(&g), &s), Ok(expect(Budget, &BUDGET)));
        assert_eq!(resolve(Fast, Some(&g), &s), Ok(expect(Fast, &FAST)));
        for tier in [Budget, Fast] {
            let sel = resolve(tier, Some(&g), &s).unwrap();
            assert_eq!((sel.sent_model_id.as_str(), sel.computed_at_ms, sel.snapshot_fetched_at_ms), (SLUG, ms(NOW), Some(ms(FETCHED_AT))));
        }
    }

    #[test]
    fn l3_nitro_ignores_the_result_and_under_allow_has_no_provider() {
        let s = RoutingSettings::default();
        let (g, stale) = (golden(), golden_at(STALE_NOW, RoutingSettings::default()));
        for result in [Some(&g), None, Some(&stale)] {
            assert_eq!(resolve(Nitro, result, &s), Ok(nitro_selection()));
        }
        let allow = RoutingSettings { data_collection: DataCollection::Allow, ..RoutingSettings::default() };
        assert_eq!(resolve(Nitro, Some(&g), &allow), Ok(RoutingLaunchSelection { provider: None, ..nitro_selection() }));
    }

    #[test]
    fn l4_no_snapshot_stale_the_60_minute_boundary_another_model_and_a_30_minute_setting() {
        let s = RoutingSettings::default();
        assert_eq!(resolve(Balanced, None, &s), refusal(LaunchRefusalCode::NoSnapshot, LAUNCH_NO_SNAPSHOT));
        assert_eq!(resolve(Balanced, Some(&golden_at(STALE_NOW, s.clone())), &s), refusal(LaunchRefusalCode::SnapshotStale, MESSAGE_STALE_60));
        assert_eq!(resolve(Balanced, Some(&golden_at("2026-10-02T10:05:00Z", s.clone())), &s).unwrap().endpoints, BALANCED);
        let other = TierResult { model: "other/model".into(), ..golden() };
        assert_eq!(resolve(Balanced, Some(&other), &s), refusal(LaunchRefusalCode::OperationFailed, "Routing operation failed."));
        let s30 = RoutingSettings { snapshot_max_age_minutes: 30.0, ..RoutingSettings::default() };
        assert_eq!(
            resolve(Balanced, Some(&golden_at("2026-10-02T09:35:00.001Z", s30.clone())), &s30),
            refusal(LaunchRefusalCode::SnapshotStale, "The endpoint snapshot for this model is more than 30 minutes old. Refresh first.")
        );
    }

    #[test]
    fn l5_an_empty_tier_is_tier_empty_with_its_label_and_staleness_is_checked_first() {
        let floor = RoutingSettings { budget_min_tps: 100_000.0, ..RoutingSettings::default() };
        assert_eq!(
            resolve(Budget, Some(&golden_at(NOW, floor.clone())), &floor),
            refusal(LaunchRefusalCode::TierEmpty, "Budget has no eligible endpoints. Choose another tier or OpenRouter default.")
        );
        assert_eq!(resolve(Balanced, Some(&golden_at(NOW, floor.clone())), &floor).unwrap().endpoints, BALANCED);
        let uptime = RoutingSettings { min_uptime_pct: 100.0, readmit_uptime_pct: 100.0, ..RoutingSettings::default() };
        assert_eq!(
            resolve(Balanced, Some(&golden_at(NOW, uptime.clone())), &uptime),
            refusal(LaunchRefusalCode::TierEmpty, "Balanced has no eligible endpoints. Choose another tier or OpenRouter default.")
        );
        assert_eq!(
            resolve(Fast, Some(&golden_at(NOW, uptime.clone())), &uptime),
            refusal(LaunchRefusalCode::TierEmpty, "Fast has no eligible endpoints. Choose another tier or OpenRouter default.")
        );
        assert_eq!(resolve(Budget, Some(&golden_at(STALE_NOW, floor.clone())), &floor), refusal(LaunchRefusalCode::SnapshotStale, MESSAGE_STALE_60));
    }

    #[test]
    fn l6_the_selection_shares_nothing_with_the_result() {
        let result = golden();
        let mut sel = resolve(Balanced, Some(&result), &RoutingSettings::default()).unwrap();
        sel.provider.as_mut().unwrap().order.as_mut().unwrap().push("x".into());
        sel.endpoints.push("x".into());
        assert_eq!(sel.provider.as_ref().unwrap().order.as_ref().unwrap().len(), 4);
        let balanced = result.tiers.balanced.as_ref().unwrap();
        assert_eq!((balanced.provider.order.clone(), balanced.endpoints.clone()), (Some(strings(&BALANCED)), strings(&BALANCED)));
    }

    #[test]
    fn l7_every_ok_selection_is_consistent_and_reads_back_from_storage() {
        let s = RoutingSettings::default();
        let allow = RoutingSettings { data_collection: DataCollection::Allow, ..RoutingSettings::default() };
        let (g, stale) = (golden(), golden_at(STALE_NOW, RoutingSettings::default()));
        let selections = [
            resolve(Balanced, Some(&g), &s),
            resolve(Budget, Some(&g), &s),
            resolve(Fast, Some(&g), &s),
            resolve(Nitro, Some(&g), &s),
            resolve(Nitro, None, &s),
            resolve(Nitro, Some(&stale), &s),
            resolve(Nitro, Some(&g), &allow),
        ];
        for sel in selections.map(Result::unwrap) {
            assert!(sel.is_consistent(), "{:?}", sel.tier);
            let text = serde_json::to_string(&sel).unwrap();
            assert_eq!(parse_stored_selection(Some(&text)), StoredSelection::Selection(sel.clone()), "{:?}", sel.tier);
        }
    }

    #[test]
    fn the_provider_sent_for_a_selection() {
        assert_eq!(
            balanced_selection().provider_value(),
            Some(json!({ "order": BALANCED, "allow_fallbacks": false, "require_parameters": true, "quantizations": ["fp8"], "data_collection": "deny" }))
        );
        assert_eq!(nitro_selection().provider_value(), Some(json!({ "data_collection": "deny" })));
        assert_eq!(RoutingLaunchSelection { provider: None, ..nitro_selection() }.provider_value(), None);
    }

    #[test]
    fn inconsistent_selections_are_refused() {
        let bad = [
            RoutingLaunchSelection { sent_model_id: format!("{SLUG}:nitro"), ..balanced_selection() },
            RoutingLaunchSelection { model: format!("{SLUG}:nitro"), sent_model_id: format!("{SLUG}:nitro"), ..balanced_selection() },
            RoutingLaunchSelection { endpoints: strings(&BUDGET), ..balanced_selection() },
            RoutingLaunchSelection { endpoints: vec![], provider: Some(provider(&[])), ..balanced_selection() },
            RoutingLaunchSelection { snapshot_fetched_at_ms: None, ..balanced_selection() },
            RoutingLaunchSelection { provider: None, ..balanced_selection() },
            RoutingLaunchSelection { provider: Some(OpenRouterProviderPrefs { allow_fallbacks: Some(true), ..provider(&BALANCED) }), ..balanced_selection() },
            RoutingLaunchSelection { provider: Some(OpenRouterProviderPrefs { quantizations: None, ..provider(&BALANCED) }), ..balanced_selection() },
            RoutingLaunchSelection { model: "a b".into(), ..balanced_selection() },
            RoutingLaunchSelection { sent_model_id: SLUG.into(), ..nitro_selection() },
            RoutingLaunchSelection { endpoints: strings(&["x"]), ..nitro_selection() },
            RoutingLaunchSelection { snapshot_fetched_at_ms: Some(0), ..nitro_selection() },
            RoutingLaunchSelection { provider: Some(provider(&BALANCED)), ..nitro_selection() },
        ];
        for s in bad {
            assert!(!s.is_consistent(), "{s:?}");
        }
    }

    fn known() -> Vec<String> {
        vec![SLUG.to_owned()]
    }

    fn facts<'a>(known: &'a [String], env: &'a [String]) -> RoutingLaunchFacts<'a> {
        RoutingLaunchFacts {
            agent: AgentKind::Opencode,
            tier: Some(Balanced),
            credential_id: Some(C),
            model: Some(SLUG),
            routing_available: true,
            routing_credential_ids: &[C],
            known_models: known,
            profile_env_keys: env,
        }
    }

    #[test]
    fn l11_routed_nitro_and_default_and_each_refusal_with_its_exact_text() {
        let (k, none) = (known(), vec![]);
        let base = facts(&k, &none);
        let routed = |tier| RoutingLaunchPlan::Routed { model: SLUG.into(), tier, credential_id: C };
        assert_eq!(plan_routing_launch(&base), routed(Balanced));
        assert_eq!(plan_routing_launch(&RoutingLaunchFacts { tier: Some(Nitro), ..base }), routed(Nitro));
        assert_eq!(plan_routing_launch(&RoutingLaunchFacts { tier: None, ..base }), RoutingLaunchPlan::Default { model: SLUG.into() });
        let with_nitro_known = vec![SLUG.to_owned(), format!("{SLUG}:nitro")];
        let nitro_model = format!("{SLUG}:nitro");
        let (upper, lower) = (strings(&["OPENCODE_CONFIG_CONTENT"]), strings(&["opencode_config_content"]));
        let refusals: Vec<(RoutingLaunchFacts, &str)> = vec![
            (RoutingLaunchFacts { agent: AgentKind::Claude, ..base }, REFUSE_NOT_OPENCODE),
            (RoutingLaunchFacts { routing_available: false, ..base }, REFUSE_UNAVAILABLE),
            (RoutingLaunchFacts { credential_id: None, ..base }, REFUSE_NO_CREDENTIAL),
            (RoutingLaunchFacts { credential_id: Some(D), ..base }, REFUSE_CREDENTIAL),
            (RoutingLaunchFacts { model: None, ..base }, REFUSE_NO_MODEL),
            (RoutingLaunchFacts { model: Some(nitro_model.as_str()), known_models: &with_nitro_known, ..base }, REFUSE_NITRO_MODEL),
            (RoutingLaunchFacts { model: Some("z-ai/glm-5.3"), ..base }, REFUSE_UNKNOWN_MODEL),
            (RoutingLaunchFacts { profile_env_keys: &upper, ..base }, REFUSE_PROFILE_ENV),
            (RoutingLaunchFacts { profile_env_keys: &lower, ..base }, REFUSE_PROFILE_ENV),
        ];
        for (f, reason) in refusals {
            assert_eq!(plan_routing_launch(&f), RoutingLaunchPlan::Refused { reason: reason.into() });
            // C8: an ineligible or blocked launch without a tier is today's launch, never Default.
            assert_eq!(plan_routing_launch(&RoutingLaunchFacts { tier: None, ..f }), RoutingLaunchPlan::Unrouted, "{reason}");
        }
        let first_wins = RoutingLaunchFacts { agent: AgentKind::Claude, routing_available: false, credential_id: None, ..base };
        assert_eq!(plan_routing_launch(&first_wins), RoutingLaunchPlan::Refused { reason: REFUSE_NOT_OPENCODE.into() });
        let second = RoutingLaunchFacts { routing_available: false, credential_id: None, ..base };
        assert_eq!(plan_routing_launch(&second), RoutingLaunchPlan::Refused { reason: REFUSE_UNAVAILABLE.into() });
    }

    #[test]
    fn l11_the_refusal_texts_are_chorus_1s() {
        assert_eq!(REFUSE_NOT_OPENCODE, "Routing tiers apply only to OpenCode launches.");
        assert_eq!(REFUSE_UNAVAILABLE, "Model routing is not available right now. Launch with OpenRouter default instead.");
        assert_eq!(REFUSE_NO_CREDENTIAL, "Routing tiers need an OpenRouter API-key credential.");
        assert_eq!(
            REFUSE_CREDENTIAL,
            "This credential cannot be used for model routing. Routing needs an OpenRouter API-key credential for the OpenRouter gateway."
        );
        assert_eq!(REFUSE_NO_MODEL, "Choose a model to use a routing tier.");
        assert_eq!(REFUSE_NITRO_MODEL, "This model already ends in :nitro. Choose the base model and the Nitro tier instead.");
        assert_eq!(REFUSE_UNKNOWN_MODEL, "Model routing does not know this model.");
        assert_eq!(
            REFUSE_PROFILE_ENV,
            "The launch profile sets OPENCODE_CONFIG_CONTENT, which a routed launch also needs. Remove it from the profile, or launch with OpenRouter default."
        );
        assert_eq!(REFUSE_STORED_INVALID, "This session's saved routing could not be read. Start a new session from the launch dialog.");
        assert_eq!(REFUSE_RELAUNCH_UNAVAILABLE, "Model routing is not available right now, so this routed session cannot be relaunched.");
        assert_eq!(REFUSE_RELAUNCH_CREDENTIAL, "This session was routed, but its launch profile's credential can no longer be used for model routing.");
    }

    #[test]
    fn the_first_party_refusal_text_is_decisions_md_exactly() {
        assert_eq!(
            REFUSE_FIRST_PARTY,
            "Chorus doesn't route Anthropic or OpenAI models through OpenRouter. Run them in Claude Code or Codex on your own sign-in."
        );
        assert!(!REFUSE_FIRST_PARTY.contains('\n') && !REFUSE_FIRST_PARTY.contains('’'), "one line, straight apostrophes");
    }

    #[test]
    fn anthropic_and_openai_models_are_refused_whatever_their_case_or_tilde() {
        for model in [
            "anthropic/claude-sonnet-5",
            "Anthropic/claude-opus-5.5",
            "openai/gpt-5.5",
            "OPENAI/o9",
            "~anthropic/claude-sonnet-latest",
            "openai",
        ] {
            assert_eq!(routing_refusal(model), Some(REFUSE_FIRST_PARTY), "{model}");
        }
    }

    #[test]
    fn other_authors_are_allowed_even_when_they_only_look_alike() {
        for model in ["deepseek/deepseek-v4.1-flash", "anthropic-labs/x", "meta/openai-compat", "~deepseek/x", SLUG] {
            assert_eq!(routing_refusal(model), None, "{model}");
        }
        // Only the text before the first `/` is the author, and only one `~` is dropped.
        for model in ["~~anthropic/x", "x/anthropic/y", "openai-x", "anthropics", "", "/anthropic", "~"] {
            assert_eq!(routing_refusal(model), None, "{model}");
        }
    }

    #[test]
    fn a_stored_selection_for_a_first_party_model_still_reads_back_and_relaunches() {
        const MODEL: &str = "anthropic/claude-sonnet-5";
        let balanced = RoutingLaunchSelection { model: MODEL.into(), sent_model_id: MODEL.into(), ..balanced_selection() };
        let nitro = RoutingLaunchSelection { model: MODEL.into(), sent_model_id: format!("{MODEL}:nitro"), ..nitro_selection() };
        assert!(routing_refusal(MODEL).is_some(), "the model is one routing now refuses");
        for selection in [balanced, nitro] {
            let stored = serde_json::to_string(&selection).unwrap();
            assert_eq!(parse_stored_selection(Some(&stored)), StoredSelection::Selection(selection.clone()), "{:?}", selection.tier);
            let facts = RoutingRelaunchFacts {
                agent: AgentKind::Opencode,
                routing_json: Some(&stored),
                credential_id: Some(C),
                routing_available: true,
                routing_credential_ids: &[C],
                profile_env_keys: &[],
            };
            assert_eq!(plan_routing_relaunch(&facts), RoutingRelaunchPlan::Routed(selection.clone()), "{:?}", selection.tier);
        }
    }

    #[test]
    fn l12_relaunch_reapplies_a_stored_selection_or_refuses() {
        let stored = serde_json::to_string(&balanced_selection()).unwrap();
        let env = strings(&["OPENCODE_CONFIG_CONTENT"]);
        let base = RoutingRelaunchFacts {
            agent: AgentKind::Opencode,
            routing_json: Some(&stored),
            credential_id: Some(C),
            routing_available: true,
            routing_credential_ids: &[C],
            profile_env_keys: &[],
        };
        let refused = |reason: &str| RoutingRelaunchPlan::Refused { reason: reason.into() };
        assert_eq!(plan_routing_relaunch(&base), RoutingRelaunchPlan::Routed(balanced_selection()));
        assert_eq!(plan_routing_relaunch(&RoutingRelaunchFacts { routing_json: None, ..base }), RoutingRelaunchPlan::Unrouted);
        let unrouted_anyway = RoutingRelaunchFacts { routing_json: None, agent: AgentKind::Claude, routing_available: false, ..base };
        assert_eq!(plan_routing_relaunch(&unrouted_anyway), RoutingRelaunchPlan::Unrouted);
        let mut extra: Value = serde_json::from_str(&stored).unwrap();
        extra["extra"] = json!(1);
        let mut mismatched: Value = serde_json::from_str(&stored).unwrap();
        mismatched["sentModelId"] = json!(format!("{SLUG}:nitro"));
        for text in [String::new(), "not json".into(), extra.to_string(), mismatched.to_string()] {
            assert_eq!(plan_routing_relaunch(&RoutingRelaunchFacts { routing_json: Some(&text), ..base }), refused(REFUSE_STORED_INVALID), "{text}");
        }
        assert_eq!(plan_routing_relaunch(&RoutingRelaunchFacts { agent: AgentKind::Codex, ..base }), refused(REFUSE_NOT_OPENCODE));
        assert_eq!(plan_routing_relaunch(&RoutingRelaunchFacts { routing_available: false, ..base }), refused(REFUSE_RELAUNCH_UNAVAILABLE));
        assert_eq!(plan_routing_relaunch(&RoutingRelaunchFacts { credential_id: None, ..base }), refused(REFUSE_RELAUNCH_CREDENTIAL));
        assert_eq!(plan_routing_relaunch(&RoutingRelaunchFacts { credential_id: Some(D), ..base }), refused(REFUSE_RELAUNCH_CREDENTIAL));
        assert_eq!(plan_routing_relaunch(&RoutingRelaunchFacts { profile_env_keys: &env, ..base }), refused(REFUSE_PROFILE_ENV));
        assert_eq!(parse_stored_selection(None), StoredSelection::None);
        let nitro = serde_json::to_string(&nitro_selection()).unwrap();
        assert_eq!(parse_stored_selection(Some(&nitro)), StoredSelection::Selection(nitro_selection()));
    }

    #[test]
    fn l16_labels_and_messages() {
        assert_eq!([Budget, Balanced, Fast, Nitro].map(RoutingLaunchTier::label), ["Budget", "Balanced", "Fast", "Nitro"]);
        assert_eq!(empty_tier_message(RankedTier::Fast), "Fast has no eligible endpoints. Choose another tier or OpenRouter default.");
        assert_eq!(stale_snapshot_message(60.0), MESSAGE_STALE_60);
        assert_eq!(LAUNCH_NO_SNAPSHOT, "No endpoint snapshot is stored for this model yet. Refresh first.");
        assert_eq!(RoutingLaunchChoice::from(Nitro), RoutingLaunchChoice::Nitro);
        assert_eq!(serde_json::to_string(&RoutingLaunchChoice::Default).unwrap(), r#""default""#);
    }
}
