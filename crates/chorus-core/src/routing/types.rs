//! The routing vocabulary, settings, records and results, ported from Chorus 1's
//! `src/shared/routing.ts` (only what the ported modules need).
//!
//! Chorus 1 carried times as ISO strings; here every time is Unix milliseconds (`*_ms: i64`).
//! Specta refuses to export `i64` to TypeScript, so the front-end types mark those fields
//! `#[specta(type = f64)]`: a millisecond timestamp fits a JavaScript number exactly.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

// ── Vocabulary ──

/// An endpoint's declared precision, in Chorus 1's `QUANTIZATIONS` order (the order every list
/// of them uses).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Quantization {
    Int4,
    Int8,
    Fp4,
    Fp6,
    Fp8,
    Fp16,
    Bf16,
    Fp32,
    /// Undeclared, unrecognised, or different across the rows that share a tag.
    Unknown,
}

impl Quantization {
    /// In `QUANTIZATIONS` order.
    pub const ALL: [Self; 9] =
        [Self::Int4, Self::Int8, Self::Fp4, Self::Fp6, Self::Fp8, Self::Fp16, Self::Bf16, Self::Fp32, Self::Unknown];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Int4 => "int4",
            Self::Int8 => "int8",
            Self::Fp4 => "fp4",
            Self::Fp6 => "fp6",
            Self::Fp8 => "fp8",
            Self::Fp16 => "fp16",
            Self::Bf16 => "bf16",
            Self::Fp32 => "fp32",
            Self::Unknown => "unknown",
        }
    }

    /// The exact (lower-case) name, or `None`.
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|q| q.as_str() == s)
    }

    /// C6: a lower-cased member of the vocabulary; anything else, or nothing, is `Unknown`.
    pub fn normalize(raw: Option<&str>) -> Self {
        raw.and_then(|r| Self::parse(&r.to_lowercase())).unwrap_or(Self::Unknown)
    }
}

/// The three tiers the ranker orders. Nitro is never ranked: OpenRouter chooses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum RankedTier {
    Budget,
    Balanced,
    Fast,
}

impl RankedTier {
    pub const ALL: [Self; 3] = [Self::Budget, Self::Balanced, Self::Fast];

    pub fn label(self) -> &'static str {
        match self {
            Self::Budget => "Budget",
            Self::Balanced => "Balanced",
            Self::Fast => "Fast",
        }
    }
}

/// Which measured workload a ranking is for (MR-D6, MR-D9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum RoutingProfileId {
    /// A session someone is typing into.
    Interactive,
    /// Chorus 1's Team helper turns. 2.0 has no Team helpers yet; the profile is kept because its
    /// golden results pin the ranker.
    Helper,
}

impl RoutingProfileId {
    pub fn profile(self) -> &'static RoutingProfile {
        match self {
            Self::Interactive => &INTERACTIVE_PROFILE,
            Self::Helper => &HELPER_PROFILE,
        }
    }
}

/// The share of a turn's tokens that are fresh prompt, cached prompt and output. Sums to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TokenShares {
    pub fresh: f64,
    pub cached: f64,
    pub output: f64,
}

/// A measured workload: how long a turn's output is, its token mix and prompt size.
#[derive(Debug, Clone, PartialEq)]
pub struct RoutingProfile {
    pub id: RoutingProfileId,
    /// N in the effective throughput, reasoning included.
    pub expected_output_tokens: u32,
    pub token_shares: TokenShares,
    /// Evaluates `min_prompt_tokens` pricing overrides.
    pub typical_prompt_tokens: u64,
    pub min_max_completion: u64,
    pub source: &'static str,
}

pub const INTERACTIVE_PROFILE: RoutingProfile = RoutingProfile {
    id: RoutingProfileId::Interactive,
    expected_output_tokens: 300,
    token_shares: TokenShares { fresh: 0.057, cached: 0.932, output: 0.011 },
    typical_prompt_tokens: 113_651,
    min_max_completion: 65_536,
    source: "Local OpenCode history, interactive DeepSeek V4.1 Flash turns 2026-08-07 to 2026-10-02 (285 turns for N); measured 2026-10-02 (MR-D6, MR-D9).",
};

pub const HELPER_PROFILE: RoutingProfile = RoutingProfile {
    id: RoutingProfileId::Helper,
    expected_output_tokens: 460,
    token_shares: TokenShares { fresh: 0.038, cached: 0.957, output: 0.005 },
    typical_prompt_tokens: 178_058,
    min_max_completion: 64_000,
    source: "Local OpenCode history, Team helper worktree DeepSeek V4.1 Flash turns 2026-08-07 to 2026-10-02 (881 turns for N); measured 2026-10-02 (MR-D6, MR-D9).",
};

/// A routable endpoint tag as Chorus accepts it from OpenRouter: `[A-Za-z0-9][A-Za-z0-9._:/-]{0,63}`
/// (Chorus 1's `ROUTING_TAG_PATTERN`).
pub fn is_routing_tag(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_alphanumeric()
        && b[1..].iter().all(|c| c.is_ascii_alphanumeric() || b"._:/-".contains(c))
}

/// A model slug as routing accepts it: `[A-Za-z0-9._:/@~-]{1,200}` (Chorus 1's
/// `routingModelSlugSchema`).
pub fn is_model_slug(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty() && b.len() <= 200 && b.iter().all(|c| c.is_ascii_alphanumeric() || b"._:/@~-".contains(c))
}

// ── Settings ──

/// What an endpoint that declares no quantization needs to be admitted (MR-D7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum UnknownQuantPolicy {
    /// Never admitted.
    Strict,
    /// Admitted at native precision from the model's first party, or with an active verification
    /// record for the tag.
    FirstPartyAndVerified,
}

/// OpenRouter's `provider.data_collection` preference (MR-D11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum DataCollection {
    Deny,
    Allow,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TierWeights {
    pub budget: f64,
    pub balanced: f64,
    pub fast: f64,
}

impl TierWeights {
    pub fn get(&self, tier: RankedTier) -> f64 {
        match tier {
            RankedTier::Budget => self.budget,
            RankedTier::Balanced => self.balanced,
            RankedTier::Fast => self.fast,
        }
    }
}

/// The ranker's knobs. [`RoutingSettings::validate`] is Chorus 1's `routingSettingsSchema`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoutingSettings {
    pub min_uptime_pct: f64,
    /// An endpoint that fell below the minimum inside its window needs this much to come back.
    pub readmit_uptime_pct: f64,
    pub outage_guard_5m_pct: f64,
    pub unknown_quant_policy: UnknownQuantPolicy,
    pub smoothing_window: u32,
    pub stable_min_observations: u32,
    pub observation_max_age_days: f64,
    pub snapshot_max_age_minutes: f64,
    pub budget_min_tps: f64,
    pub budget_median_fraction: f64,
    pub tier_weights: TierWeights,
    pub tie_ratio: f64,
    pub fallback_count: u32,
    pub data_collection: DataCollection,
}

impl Default for RoutingSettings {
    fn default() -> Self {
        Self {
            min_uptime_pct: 99.5,
            readmit_uptime_pct: 99.6,
            outage_guard_5m_pct: 95.0,
            unknown_quant_policy: UnknownQuantPolicy::FirstPartyAndVerified,
            smoothing_window: 6,
            stable_min_observations: 3,
            observation_max_age_days: 7.0,
            snapshot_max_age_minutes: 60.0,
            budget_min_tps: 30.0,
            budget_median_fraction: 0.5,
            tier_weights: TierWeights { budget: 0.3, balanced: 0.5, fast: 1.0 },
            tie_ratio: 1.01,
            fallback_count: 2,
            data_collection: DataCollection::Deny,
        }
    }
}

impl RoutingSettings {
    /// The semantic rules of Chorus 1's schema: percentages in 0–100, weights in 0–1, positive
    /// ages, `tieRatio` at least 1, readmission at least the minimum, and no more stable
    /// observations than the window holds. The error names the first field that fails.
    pub fn validate(&self) -> Result<(), String> {
        let pct = |v: f64| v.is_finite() && (0.0..=100.0).contains(&v);
        let weight = |v: f64| v.is_finite() && (0.0..=1.0).contains(&v);
        let positive = |v: f64| v.is_finite() && v > 0.0;
        let w = &self.tier_weights;
        let checks: [(bool, &str); 15] = [
            (pct(self.min_uptime_pct), "minUptimePct must be between 0 and 100"),
            (pct(self.readmit_uptime_pct), "readmitUptimePct must be between 0 and 100"),
            (pct(self.outage_guard_5m_pct), "outageGuard5mPct must be between 0 and 100"),
            (self.smoothing_window >= 1, "smoothingWindow must be at least 1"),
            (self.stable_min_observations >= 1, "stableMinObservations must be at least 1"),
            (positive(self.observation_max_age_days), "observationMaxAgeDays must be positive"),
            (positive(self.snapshot_max_age_minutes), "snapshotMaxAgeMinutes must be positive"),
            (self.budget_min_tps.is_finite() && self.budget_min_tps >= 0.0, "budgetMinTps must not be negative"),
            (weight(self.budget_median_fraction), "budgetMedianFraction must be between 0 and 1"),
            (weight(w.budget), "tierWeights.budget must be between 0 and 1"),
            (weight(w.balanced), "tierWeights.balanced must be between 0 and 1"),
            (weight(w.fast), "tierWeights.fast must be between 0 and 1"),
            (self.tie_ratio.is_finite() && self.tie_ratio >= 1.0, "tieRatio must be at least 1"),
            (self.readmit_uptime_pct >= self.min_uptime_pct, "readmitUptimePct below minUptimePct"),
            (self.stable_min_observations <= self.smoothing_window, "stableMinObservations exceeds smoothingWindow"),
        ];
        match checks.into_iter().find(|(ok, _)| !ok) {
            Some((_, message)) => Err(message.to_owned()),
            None => Ok(()),
        }
    }
}

// ── Raw OpenRouter rows (`GET /api/v1/models/{slug}/endpoints`) ──
// Field names are OpenRouter's, so a stored snapshot reads back with the same parser.

/// A time-of-day or prompt-size price override (K11, MR-D13). Prices are per-token USD strings.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RawPricingOverride {
    /// HHMM UTC, inclusive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utc_start: Option<u32>,
    /// HHMM UTC, exclusive; `end <= start` wraps midnight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utc_end: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utc_days: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_prompt_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_cache_read: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_cache_write: Option<String>,
}

/// An endpoint's prices: per-token USD as decimal strings (`"0.0000003"`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RawPricing {
    pub prompt: String,
    pub completion: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_cache_read: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_cache_write: Option<String>,
    /// Read but unused (0 on every row Chorus 1 saw).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discount: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overrides: Option<Vec<RawPricingOverride>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Percentiles {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p50: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p90: Option<f64>,
}

/// One row of the endpoints response. Several rows can share a tag (they collapse into one
/// candidate). Unknown fields are dropped.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct RawEndpoint {
    pub tag: String,
    pub provider_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantization: Option<String>,
    pub context_length: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_completion_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_prompt_tokens: Option<u64>,
    pub pricing: RawPricing,
    pub supported_parameters: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_tool_choice: Option<BTreeMap<String, bool>>,
    /// 0 is healthy; anything else excludes the endpoint.
    pub status: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_last_1d: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_last_5m: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uptime_last_30m: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_implicit_caching: Option<bool>,
    /// Tokens per second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throughput_last_30m: Option<Percentiles>,
    /// Milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_last_30m: Option<Percentiles>,
}

/// The latest endpoints response for a model, as validated rows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EndpointSnapshot {
    pub fetched_at_ms: i64,
    pub endpoints: Vec<RawEndpoint>,
}

// ── Chorus records ──

/// One tag's health and speed at one instant: the history the ranker smooths over.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoutingObservation {
    pub tag: String,
    pub observed_at_ms: i64,
    pub uptime_1d: Option<f64>,
    pub uptime_5m: Option<f64>,
    pub status: i64,
    pub tps_p50: Option<f64>,
    pub tps_p90: Option<f64>,
    pub latency_p50_ms: Option<f64>,
    pub latency_p90_ms: Option<f64>,
}

/// MR-D7: a reviewed check that one tag of one model serves it at native quality. Applies to
/// exactly that model and tag while `verified_at_ms <= now < expires_at_ms`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerificationRecord {
    pub model: String,
    pub tag: String,
    pub reference_tag: String,
    pub suite_version: String,
    pub score: f64,
    #[specta(type = f64)]
    pub verified_at_ms: i64,
    #[specta(type = f64)]
    pub expires_at_ms: i64,
    pub reviewer: String,
}

/// What routing knows about a model beyond its endpoints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelRegistryEntry {
    pub slug: String,
    pub display_name: String,
    /// `None` for closed weights: the precision filter is skipped. Never `Unknown`.
    pub native_precision: Option<Quantization>,
    pub native_precision_source: String,
    /// `provider_name` values whose undeclared quantization is trusted at native precision.
    pub first_party_providers: Vec<String>,
    pub verified: Vec<VerificationRecord>,
    pub min_context: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRegistry {
    pub version: u32,
    pub models: BTreeMap<String, ModelRegistryEntry>,
}

/// What the account's own filters remove (K3), from the two preflights. `None` means the
/// preflight was not parsed: unknown, never "nothing removed".
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountEligibility {
    pub guardrail_removed: Option<Vec<String>>,
    pub data_policy_removed: Option<Vec<String>>,
    pub checked_at_ms: Option<i64>,
}

/// Whether a tag served cached reads when probed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CacheRecord {
    pub verified: bool,
    pub checked_at_ms: i64,
}

/// Cache probe results by tag.
pub type CacheVerification = BTreeMap<String, CacheRecord>;

/// Everything one ranking reads. `now_ms` is its only time source (MR-G8).
#[derive(Debug, Clone, Copy)]
pub struct RankInput<'a> {
    pub model: &'a ModelRegistryEntry,
    pub snapshot: &'a EndpointSnapshot,
    pub history: &'a [RoutingObservation],
    pub account: &'a AccountEligibility,
    pub cache: &'a CacheVerification,
    pub profile: RoutingProfileId,
    /// The launch's effort; `Some` makes `reasoning` a required parameter.
    pub effort: Option<&'a str>,
    pub settings: &'a RoutingSettings,
    pub now_ms: i64,
}

// ── Results ──

/// The `"deny"` literal OpenRouter's `provider.data_collection` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderDataCollection {
    Deny,
}

/// OpenRouter's request `provider` object, with OpenRouter's own (snake_case) keys. Absent keys
/// are left out, never sent as null. [`OpenRouterProviderPrefs::to_value`] is the JSON to send.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct OpenRouterProviderPrefs {
    /// Endpoint tags, never display names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub order: Option<Vec<String>>,
    /// Only ever `false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub allow_fallbacks: Option<bool>,
    /// Only ever `true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub require_parameters: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub quantizations: Option<Vec<Quantization>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub data_collection: Option<ProviderDataCollection>,
}

/// One ranked tier: its endpoints in order and the provider object that pins them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TierSelection {
    pub tier: RankedTier,
    /// The slug, never suffixed.
    pub model: String,
    pub provider: OpenRouterProviderPrefs,
    /// The `order` tags.
    pub endpoints: Vec<String>,
    pub limited_history: bool,
    pub limited_fallbacks: bool,
    pub rationale: String,
}

/// The endpoint Nitro would most likely use: a labelled preview, not a choice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NitroLikely {
    pub tag: String,
    pub provider_name: String,
    pub tps_p50: f64,
}

/// Nitro: the `:nitro` model id, and OpenRouter picks the fastest endpoint (MR-D4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NitroSelection {
    /// The slug plus `:nitro`.
    pub model: String,
    /// `None` under `DataCollection::Allow`.
    pub provider: Option<OpenRouterProviderPrefs>,
    pub likely: Option<NitroLikely>,
    /// The reliability and precision rules the likely endpoint fails.
    pub likely_fails_rules: Vec<String>,
}

/// A candidate's blended price at the profile's token mix, in USD per million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CandidateCost {
    pub blended: f64,
    /// Fresh share × prompt price.
    pub fresh: f64,
    /// Cached share × (cache price when credited, else prompt price).
    pub cached: f64,
    /// Output share × completion price.
    pub output: f64,
    pub cache_credit: bool,
    pub override_applied: bool,
    pub prompt_per_m: f64,
    pub completion_per_m: f64,
    /// The cache price used when credited (the prompt price when a row lists none).
    pub cache_read_per_m: f64,
}

/// A candidate's score per tier; `None` where it is not scored.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TierScores {
    pub budget: Option<f64>,
    pub balanced: Option<f64>,
    pub fast: Option<f64>,
}

impl TierScores {
    pub fn get(&self, tier: RankedTier) -> Option<f64> {
        match tier {
            RankedTier::Budget => self.budget,
            RankedTier::Balanced => self.balanced,
            RankedTier::Fast => self.fast,
        }
    }

    pub fn set(&mut self, tier: RankedTier, score: f64) {
        match tier {
            RankedTier::Budget => self.budget = Some(score),
            RankedTier::Balanced => self.balanced = Some(score),
            RankedTier::Fast => self.fast = Some(score),
        }
    }
}

/// Everything the ranker knows about one routable tag, eligible or not.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CandidateExplanation {
    pub tag: String,
    pub provider_name: String,
    pub rows: u32,
    /// Declared; `Unknown` when the rows disagree.
    pub quantization: Quantization,
    /// The distinct declared row values, in `QUANTIZATIONS` order.
    pub row_quantizations: Vec<Quantization>,
    /// Native precision for an admitted undeclared candidate.
    pub effective_quantization: Quantization,
    pub uptime_1d: Option<f64>,
    pub uptime_5m: Option<f64>,
    #[specta(type = f64)]
    pub status: i64,
    /// Speed observations used for smoothing.
    pub observations: u32,
    pub limited_history: bool,
    pub tps_p50: Option<f64>,
    pub latency_p50_s: Option<f64>,
    pub tps_p90: Option<f64>,
    pub latency_p90_s: Option<f64>,
    pub effective_tps: Option<f64>,
    /// `None`: no usable price.
    pub cost: Option<CandidateCost>,
    /// Every failing rule, in canonical order. Empty means eligible.
    pub excluded_by: Vec<String>,
    pub budget_floor_excluded: bool,
    pub scores: TierScores,
}

/// Whether the account guardrail preflight was read (K3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum AccountCheck {
    Checked,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TierSelections {
    pub budget: Option<TierSelection>,
    pub balanced: Option<TierSelection>,
    pub fast: Option<TierSelection>,
}

impl TierSelections {
    pub fn get(&self, tier: RankedTier) -> Option<&TierSelection> {
        match tier {
            RankedTier::Budget => self.budget.as_ref(),
            RankedTier::Balanced => self.balanced.as_ref(),
            RankedTier::Fast => self.fast.as_ref(),
        }
    }
}

/// The ranked tiers view: every tier, Nitro, the explanation of each candidate, and warnings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TierResult {
    pub model: String,
    pub profile: RoutingProfileId,
    /// The `now_ms` it was computed at.
    #[specta(type = f64)]
    pub computed_at_ms: i64,
    #[specta(type = f64)]
    pub snapshot_fetched_at_ms: i64,
    pub snapshot_age_minutes: u32,
    pub stale: bool,
    pub account_eligibility: AccountCheck,
    pub median_eligible_tps: Option<f64>,
    pub budget_floor_tps: Option<f64>,
    pub tiers: TierSelections,
    pub nitro: NitroSelection,
    /// Sorted by tag.
    pub candidates: Vec<CandidateExplanation>,
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantizations_normalise_and_keep_their_order() {
        assert_eq!(Quantization::normalize(Some("fp8")), Quantization::Fp8);
        assert_eq!(Quantization::normalize(Some("FP8")), Quantization::Fp8);
        assert_eq!(Quantization::normalize(Some("BF16")), Quantization::Bf16);
        assert_eq!(Quantization::normalize(Some("fp6e3")), Quantization::Unknown);
        assert_eq!(Quantization::normalize(Some("unknown")), Quantization::Unknown);
        assert_eq!(Quantization::normalize(None), Quantization::Unknown);
        let mut all = Quantization::ALL;
        all.reverse();
        all.sort();
        assert_eq!(all, Quantization::ALL);
        assert_eq!(serde_json::to_string(&Quantization::ALL.to_vec()).unwrap(), r#"["int4","int8","fp4","fp6","fp8","fp16","bf16","fp32","unknown"]"#);
    }

    #[test]
    fn tags_and_slugs_follow_chorus_1s_patterns() {
        assert!(is_routing_tag("baseten/fp8") && is_routing_tag("a") && is_routing_tag(&"a".repeat(64)));
        assert!(!is_routing_tag("") && !is_routing_tag("-a") && !is_routing_tag("deep seek") && !is_routing_tag(&"a".repeat(65)));
        assert!(is_routing_tag("a_b.c:d/e-f") && !is_routing_tag("_a") && !is_routing_tag("a+b"));
        assert!(is_model_slug("deepseek/deepseek-v4.1-flash:nitro") && is_model_slug("a@b~c"));
        assert!(!is_model_slug("") && !is_model_slug("a b") && !is_model_slug(&"a".repeat(201)));
    }

    #[test]
    fn the_default_settings_are_chorus_1s_and_valid() {
        let s = RoutingSettings::default();
        assert_eq!((s.min_uptime_pct, s.readmit_uptime_pct, s.outage_guard_5m_pct), (99.5, 99.6, 95.0));
        assert_eq!((s.smoothing_window, s.stable_min_observations, s.fallback_count), (6, 3, 2));
        assert_eq!((s.observation_max_age_days, s.snapshot_max_age_minutes), (7.0, 60.0));
        assert_eq!((s.budget_min_tps, s.budget_median_fraction, s.tie_ratio), (30.0, 0.5, 1.01));
        assert_eq!(s.tier_weights, TierWeights { budget: 0.3, balanced: 0.5, fast: 1.0 });
        assert_eq!((s.unknown_quant_policy, s.data_collection), (UnknownQuantPolicy::FirstPartyAndVerified, DataCollection::Deny));
        assert_eq!(s.validate(), Ok(()));
        let bad = |f: fn(&mut RoutingSettings)| {
            let mut s = RoutingSettings::default();
            f(&mut s);
            s.validate().unwrap_err()
        };
        assert_eq!(bad(|s| s.readmit_uptime_pct = 99.0), "readmitUptimePct below minUptimePct");
        assert_eq!(bad(|s| s.stable_min_observations = 7), "stableMinObservations exceeds smoothingWindow");
        assert_eq!(bad(|s| s.min_uptime_pct = 101.0), "minUptimePct must be between 0 and 100");
        assert_eq!(bad(|s| s.tie_ratio = 0.99), "tieRatio must be at least 1");
        assert_eq!(bad(|s| s.tier_weights.fast = f64::NAN), "tierWeights.fast must be between 0 and 1");
        assert_eq!(bad(|s| s.snapshot_max_age_minutes = 0.0), "snapshotMaxAgeMinutes must be positive");
    }

    #[test]
    fn the_profiles_are_chorus_1s_measured_ones() {
        let i = RoutingProfileId::Interactive.profile();
        assert_eq!((i.expected_output_tokens, i.typical_prompt_tokens, i.min_max_completion), (300, 113_651, 65_536));
        let h = RoutingProfileId::Helper.profile();
        assert_eq!((h.expected_output_tokens, h.typical_prompt_tokens, h.min_max_completion), (460, 178_058, 64_000));
        for p in [i, h] {
            let s = p.token_shares;
            assert!((s.fresh + s.cached + s.output - 1.0).abs() <= 1e-6, "{:?}", p.id);
        }
    }
}
