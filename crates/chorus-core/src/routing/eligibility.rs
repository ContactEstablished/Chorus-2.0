//! Eligibility, ported from Chorus 1's `eligibilityCore.ts`: the observation history merged with
//! the latest snapshot, each tag's window (K6, K7), its smoothed speed, and the hard gates in
//! canonical order with their exact reason texts (MR-D7, K3, K5, K6, C1–C3, C5, C6, C11).
//!
//! Gates read the current values of the collapsed latest snapshot; speeds come from the smoothed
//! window. `now_ms` is a parameter.

use std::cmp::{Ordering, Reverse};
use std::collections::BTreeMap;

use super::endpoints::{CollapsedEndpoint, collapse_status, max_or_null, min_or_null};
use super::js::{self, by_code_unit};
use super::registry::active_verification;
use super::time::DAY_MS;
use super::types::{
    AccountEligibility, CandidateCost, DataCollection, ModelRegistryEntry, Quantization, RoutingObservation, RoutingProfile, RoutingSettings,
    UnknownQuantPolicy,
};

/// How good a declared precision is; `None` for `Unknown`. bf16 and fp16 rank equal, as do fp4
/// and int4.
pub fn precision_rank(q: Quantization) -> Option<u8> {
    match q {
        Quantization::Fp32 => Some(6),
        Quantization::Bf16 | Quantization::Fp16 => Some(5),
        Quantization::Fp8 => Some(4),
        Quantization::Int8 => Some(3),
        Quantization::Fp6 => Some(2),
        Quantization::Fp4 | Quantization::Int4 => Some(1),
        Quantization::Unknown => None,
    }
}

pub const BASE_REQUIRED_PARAMETERS: [&str; 3] = ["tools", "tool_choice", "max_tokens"];

/// K5: the base parameters, plus `reasoning` when the launch sets an effort.
pub fn required_parameters(effort: Option<&str>) -> Vec<&'static str> {
    let mut out = BASE_REQUIRED_PARAMETERS.to_vec();
    if effort.is_some() {
        out.push("reasoning");
    }
    out
}

/// C1: a percentage truncated (never rounded) to `decimals` places, so a failing value can never
/// print as meeting its threshold. The `1e-9` absorbs binary representation error: 0.29 × 100 is
/// 28.999999999999996, and must still print 0.29.
pub fn truncate_pct(value: f64, decimals: u32) -> String {
    let scale = 10f64.powi(decimals as i32);
    js::to_fixed((value * scale + 1e-9).floor() / scale, decimals as usize)
}

/// The median of a sorted copy; an even count gives the mean of the middle two.
pub fn median(values: &[f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let mid = sorted.len() / 2;
    Some(if sorted.len() % 2 == 1 { sorted[mid] } else { (sorted[mid - 1] + sorted[mid]) / 2.0 })
}

/// Duplicate observations of one tag at one instant collapse with the K8/C7 rules.
fn collapse_observations(group: &[RoutingObservation]) -> RoutingObservation {
    RoutingObservation {
        tag: group[0].tag.clone(),
        observed_at_ms: group[0].observed_at_ms,
        uptime_1d: min_or_null(group.iter().map(|o| o.uptime_1d)),
        uptime_5m: min_or_null(group.iter().map(|o| o.uptime_5m)),
        status: collapse_status(group.iter().map(|o| o.status)),
        tps_p50: min_or_null(group.iter().map(|o| o.tps_p50)),
        tps_p90: min_or_null(group.iter().map(|o| o.tps_p90)),
        latency_p50_ms: max_or_null(group.iter().map(|o| o.latency_p50_ms)),
        latency_p90_ms: max_or_null(group.iter().map(|o| o.latency_p90_ms)),
    }
}

/// Stored history plus the snapshot's own observations. Observations of one tag at one instant
/// collapse with the K8/C7 rules, so the snapshot counts once even when it is already stored.
/// Sorted by tag, then newest first.
pub fn merge_history(history: &[RoutingObservation], from_snapshot: &[RoutingObservation]) -> Vec<RoutingObservation> {
    let mut groups: BTreeMap<(&str, i64), Vec<RoutingObservation>> = BTreeMap::new();
    for o in history.iter().chain(from_snapshot) {
        groups.entry((o.tag.as_str(), o.observed_at_ms)).or_default().push(o.clone());
    }
    let mut out: Vec<RoutingObservation> = groups.values().map(|g| collapse_observations(g)).collect();
    out.sort_by(|a, b| by_code_unit(&a.tag, &b.tag).then(b.observed_at_ms.cmp(&a.observed_at_ms)));
    out
}

/// K6/K7: the tag's observations at or before now and no older than `observation_max_age_days`,
/// newest first, at most `smoothing_window` of them. Expects merged history.
pub fn select_window(observations: &[RoutingObservation], tag: &str, now_ms: i64, settings: &RoutingSettings) -> Vec<RoutingObservation> {
    let oldest = now_ms as f64 - settings.observation_max_age_days * DAY_MS as f64;
    let mut window: Vec<RoutingObservation> =
        observations.iter().filter(|o| o.tag == tag && o.observed_at_ms <= now_ms && o.observed_at_ms as f64 >= oldest).cloned().collect();
    window.sort_by_key(|o| Reverse(o.observed_at_ms));
    window.truncate(settings.smoothing_window as usize);
    window
}

/// A window's speeds: medians of its positive values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SmoothedSpeed {
    pub observations: u32,
    pub limited_history: bool,
    pub tps_p50: Option<f64>,
    pub latency_p50_s: Option<f64>,
    pub tps_p90: Option<f64>,
    pub latency_p90_s: Option<f64>,
}

/// The positive, finite values (zero and missing speeds carry no information).
fn positives(values: impl Iterator<Item = Option<f64>>) -> Vec<f64> {
    values.flatten().filter(|v| v.is_finite() && *v > 0.0).collect()
}

fn median_seconds(values_ms: &[f64]) -> Option<f64> {
    median(values_ms).map(|m| m / 1000.0)
}

/// K7: medians of the window's positive values, tps and latency counted separately.
/// `observations` is the smaller count; fewer than `stable_min_observations` marks the candidate
/// `limited_history` (it is still fully gated). p90 values are for display only.
pub fn smooth_speed(window: &[RoutingObservation], settings: &RoutingSettings) -> SmoothedSpeed {
    let tps = positives(window.iter().map(|o| o.tps_p50));
    let latency = positives(window.iter().map(|o| o.latency_p50_ms));
    let observations = tps.len().min(latency.len()) as u32;
    SmoothedSpeed {
        observations,
        limited_history: observations < settings.stable_min_observations,
        tps_p50: median(&tps),
        latency_p50_s: median_seconds(&latency),
        tps_p90: median(&positives(window.iter().map(|o| o.tps_p90))),
        latency_p90_s: median_seconds(&positives(window.iter().map(|o| o.latency_p90_ms))),
    }
}

/// Which kind of rule a reason comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateFamily {
    Reliability,
    Account,
    Precision,
    Capability,
    Speed,
    Price,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateReason {
    pub family: GateFamily,
    pub text: String,
}

#[derive(Debug, Clone, Copy)]
pub struct GateContext<'a> {
    pub model: &'a ModelRegistryEntry,
    pub profile: &'a RoutingProfile,
    pub effort: Option<&'a str>,
    pub account: &'a AccountEligibility,
    pub settings: &'a RoutingSettings,
    pub now_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateResult {
    /// Every failing rule, in canonical order. Empty means eligible.
    pub reasons: Vec<GateReason>,
    /// Native precision for an admitted undeclared candidate.
    pub effective_quantization: Quantization,
}

fn listed(list: Option<&Vec<String>>, tag: &str) -> bool {
    list.is_some_and(|l| l.iter().any(|t| t == tag))
}

/// Every failing hard gate, in Chorus 1's canonical order (rules 1–13), with its exact text.
/// Rules 1–3 exclude each other. Current values come from `candidate` (the collapsed latest
/// snapshot); the readmission look-back uses `window`; speeds come from `speed`.
pub fn evaluate_gates(
    candidate: &CollapsedEndpoint,
    window: &[RoutingObservation],
    speed: &SmoothedSpeed,
    cost: Option<&CandidateCost>,
    ctx: &GateContext<'_>,
) -> GateResult {
    let s = ctx.settings;
    let mut reasons = Vec::new();
    let mut fail = |family: GateFamily, text: String| reasons.push(GateReason { family, text });

    // Reliability (K6, C1, C2).
    match candidate.uptime_1d {
        None => fail(GateFamily::Reliability, "no uptime data".into()),
        Some(u) if u < s.min_uptime_pct => {
            fail(GateFamily::Reliability, format!("uptime {}% < {}%", truncate_pct(u, 2), js::number(s.min_uptime_pct)));
        }
        Some(u) if u < s.readmit_uptime_pct && window.iter().any(|o| o.uptime_1d.is_some_and(|w| w < s.min_uptime_pct)) => {
            fail(GateFamily::Reliability, format!("readmission needs {}% (now {}%)", js::number(s.readmit_uptime_pct), truncate_pct(u, 2)));
        }
        Some(_) => {}
    }
    if let Some(u5) = candidate.uptime_5m.filter(|u5| *u5 < s.outage_guard_5m_pct) {
        fail(GateFamily::Reliability, format!("currently degraded (5m uptime {}%)", truncate_pct(u5, 1)));
    }
    if candidate.status != 0 {
        fail(GateFamily::Reliability, format!("status {}", candidate.status));
    }

    // Account (K3, C3).
    if listed(ctx.account.guardrail_removed.as_ref(), &candidate.tag) {
        fail(GateFamily::Account, "removed by account guardrail".into());
    }
    if s.data_collection == DataCollection::Deny && listed(ctx.account.data_policy_removed.as_ref(), &candidate.tag) {
        fail(GateFamily::Account, "removed by data policy".into());
    }

    // Precision (MR-D7, C6, C11).
    let q = candidate.quantization;
    let mut effective_quantization = q;
    if let Some(native) = ctx.model.native_precision {
        let native_rank = precision_rank(native);
        let below = |r: Quantization| precision_rank(r).zip(native_rank).is_some_and(|(r, n)| r < n);
        // C11: a mixed tag with a declared row below native is never admitted, not by the first
        // party and not by a verification record. The first such row, in QUANTIZATIONS order, is named.
        let below_native = if candidate.mixed_quantization { candidate.row_quantizations.iter().copied().find(|r| below(*r)) } else { None };
        if let Some(row) = below_native {
            effective_quantization = Quantization::Unknown;
            fail(GateFamily::Precision, format!("{} below native {}", row.as_str(), native.as_str()));
        } else if q == Quantization::Unknown {
            let admitted = s.unknown_quant_policy == UnknownQuantPolicy::FirstPartyAndVerified
                && (ctx.model.first_party_providers.contains(&candidate.provider_name)
                    || active_verification(ctx.model, &candidate.tag, ctx.now_ms).is_some());
            if admitted {
                effective_quantization = native;
            } else {
                effective_quantization = Quantization::Unknown;
                let text = if candidate.mixed_quantization { "quantization differs across rows sharing this tag" } else { "quantization not declared" };
                fail(GateFamily::Precision, text.into());
            }
        } else if below(q) {
            fail(GateFamily::Precision, format!("{} below native {}", q.as_str(), native.as_str()));
        }
    }

    // Capability (K5).
    for p in required_parameters(ctx.effort) {
        if !candidate.supported_parameters.iter().any(|sp| sp == p) {
            fail(GateFamily::Capability, format!("missing parameter: {p}"));
        }
    }
    if candidate.context_length < u64::from(ctx.model.min_context) {
        fail(GateFamily::Capability, format!("context {} < {}", candidate.context_length, ctx.model.min_context));
    }
    if let Some(max) = candidate.max_completion.filter(|m| *m < ctx.profile.min_max_completion) {
        fail(GateFamily::Capability, format!("max output {max} < {}", ctx.profile.min_max_completion));
    }

    // Speed (K7).
    if speed.tps_p50.is_none() || speed.latency_p50_s.is_none() {
        fail(GateFamily::Speed, "no speed data".into());
    }

    // Price (C5).
    if cost.is_none() {
        fail(GateFamily::Price, "no usable price".into());
    }

    GateResult { reasons, effective_quantization }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::endpoints::{collapse_endpoints, extract_observations};
    use crate::routing::pricing::{blended_cost, collapse_pricing, resolve_pricing};
    use crate::routing::test_support::*;
    use crate::routing::time::MINUTE_MS;
    use crate::routing::types::{EndpointSnapshot, RawEndpoint, RoutingProfileId, VerificationRecord};

    const TAG: &str = "x/fp8";
    const DAY_MINUTES: i64 = 24 * 60;

    /// The eligibility tests' row: the ranker's, plus `temperature`.
    fn row(tag: &str) -> RawEndpoint {
        let mut r = base_row(tag);
        r.supported_parameters.push("temperature".into());
        r
    }

    fn quant(tag: &str, q: Option<&str>) -> RawEndpoint {
        RawEndpoint { quantization: q.map(str::to_owned), ..row(tag) }
    }

    /// A clean observation `minutes_before_now` before NOW (negative: after NOW).
    fn obs(tag: &str, minutes_before_now: i64) -> RoutingObservation {
        RoutingObservation {
            tag: tag.into(),
            observed_at_ms: ms(NOW) - minutes_before_now * MINUTE_MS,
            uptime_1d: Some(99.9),
            uptime_5m: Some(99.9),
            status: 0,
            tps_p50: Some(100.0),
            tps_p90: Some(150.0),
            latency_p50_ms: Some(500.0),
            latency_p90_ms: Some(900.0),
        }
    }

    fn obs_uptime(tag: &str, minutes: i64, uptime: Option<f64>) -> RoutingObservation {
        RoutingObservation { uptime_1d: uptime, ..obs(tag, minutes) }
    }

    fn account() -> AccountEligibility {
        AccountEligibility { guardrail_removed: Some(vec![]), data_policy_removed: Some(vec![]), checked_at_ms: Some(ms(CHECKED_AT)) }
    }

    struct Setup {
        history: Vec<RoutingObservation>,
        model: ModelRegistryEntry,
        profile: RoutingProfileId,
        effort: Option<&'static str>,
        account: AccountEligibility,
        settings: RoutingSettings,
    }

    impl Default for Setup {
        fn default() -> Self {
            Self {
                history: vec![],
                model: bundled_model(),
                profile: RoutingProfileId::Interactive,
                effort: Some("low"),
                account: account(),
                settings: RoutingSettings::default(),
            }
        }
    }

    struct Evaluated {
        reasons: Vec<String>,
        families: Vec<GateFamily>,
        effective_quantization: Quantization,
        window: Vec<RoutingObservation>,
        speed: SmoothedSpeed,
    }

    /// The ranker's step-2 pipeline for a snapshot holding one tag.
    fn evaluate(rows: Vec<RawEndpoint>, setup: Setup) -> Evaluated {
        let profile = setup.profile.profile();
        let snapshot = EndpointSnapshot { fetched_at_ms: ms(FETCHED_AT), endpoints: rows };
        let collapsed = collapse_endpoints(&snapshot.endpoints);
        assert_eq!(collapsed.len(), 1);
        let candidate = &collapsed[0];
        let history = merge_history(&setup.history, &extract_observations(&snapshot));
        let window = select_window(&history, &candidate.tag, ms(NOW), &setup.settings);
        let speed = smooth_speed(&window, &setup.settings);
        let resolved: Vec<_> = candidate.pricing.iter().map(|p| resolve_pricing(p, ms(NOW), profile.typical_prompt_tokens)).collect();
        let cost = blended_cost(&collapse_pricing(&resolved), &profile.token_shares, true);
        let ctx = GateContext {
            model: &setup.model,
            profile,
            effort: setup.effort,
            account: &setup.account,
            settings: &setup.settings,
            now_ms: ms(NOW),
        };
        let gates = evaluate_gates(candidate, &window, &speed, cost.as_ref(), &ctx);
        Evaluated {
            reasons: gates.reasons.iter().map(|r| r.text.clone()).collect(),
            families: gates.reasons.iter().map(|r| r.family).collect(),
            effective_quantization: gates.effective_quantization,
            window,
            speed,
        }
    }

    fn reasons_of(rows: Vec<RawEndpoint>, setup: Setup) -> Vec<String> {
        evaluate(rows, setup).reasons
    }

    fn history(h: Vec<RoutingObservation>) -> Setup {
        Setup { history: h, ..Setup::default() }
    }

    fn verification(tag: &str, verified_at: &str, expires_at: &str) -> VerificationRecord {
        VerificationRecord {
            model: SLUG.into(),
            tag: tag.into(),
            reference_tag: "deepseek".into(),
            suite_version: "1".into(),
            score: 0.98,
            verified_at_ms: ms(verified_at),
            expires_at_ms: ms(expires_at),
            reviewer: "test".into(),
        }
    }

    fn with_verified(records: Vec<VerificationRecord>) -> Setup {
        Setup { model: ModelRegistryEntry { verified: records, ..bundled_model() }, ..Setup::default() }
    }

    fn closed_weights() -> Setup {
        Setup { model: ModelRegistryEntry { native_precision: None, ..bundled_model() }, ..Setup::default() }
    }

    const NONE: [&str; 0] = [];

    #[test]
    fn precision_ranks_order_the_declared_precisions() {
        use Quantization::*;
        let ranks: Vec<_> = [Fp32, Bf16, Fp16, Fp8, Int8, Fp6, Fp4, Int4, Unknown].map(precision_rank).to_vec();
        assert_eq!(ranks, [Some(6), Some(5), Some(5), Some(4), Some(3), Some(2), Some(1), Some(1), None]);
    }

    #[test]
    fn required_parameters_add_reasoning_when_an_effort_is_set() {
        assert_eq!(BASE_REQUIRED_PARAMETERS, ["tools", "tool_choice", "max_tokens"]);
        assert_eq!(required_parameters(None), ["tools", "tool_choice", "max_tokens"]);
        assert_eq!(required_parameters(Some("low")), ["tools", "tool_choice", "max_tokens", "reasoning"]);
        assert_eq!(required_parameters(Some("")), ["tools", "tool_choice", "max_tokens", "reasoning"]);
        required_parameters(None).push("x");
        assert_eq!(required_parameters(None).len(), 3);
    }

    #[test]
    fn truncate_pct_truncates_never_rounds_and_survives_representation_error() {
        assert_eq!(truncate_pct(99.4951480572302, 2), "99.49");
        assert_eq!(truncate_pct(99.55, 2), "99.55");
        assert_eq!(truncate_pct(94.9, 1), "94.9");
        assert_eq!(truncate_pct(92.36641221374046, 1), "92.3");
        assert_eq!(truncate_pct(99.999, 2), "99.99");
        assert_eq!(truncate_pct(99.6, 2), "99.60");
        assert_eq!(truncate_pct(0.29, 2), "0.29");
        assert_eq!(truncate_pct(2.3, 2), "2.30");
        assert_eq!(truncate_pct(100.0, 1), "100.0");
        assert_eq!(truncate_pct(0.0, 2), "0.00");
    }

    #[test]
    fn g1_a_low_observation_in_the_window_demands_readmission_at_99_6() {
        let h = vec![obs_uptime(TAG, 30, Some(99.49))];
        assert_eq!(reasons_of(vec![RawEndpoint { uptime_last_1d: Some(99.55), ..row(TAG) }], history(h)), ["readmission needs 99.6% (now 99.55%)"]);
    }

    #[test]
    fn g2_the_same_history_now_at_99_60_is_readmitted() {
        let h = vec![obs_uptime(TAG, 30, Some(99.49))];
        assert_eq!(reasons_of(vec![RawEndpoint { uptime_last_1d: Some(99.6), ..row(TAG) }], history(h)), NONE);
    }

    #[test]
    fn g3_the_low_observation_as_the_7th_newest_is_outside_the_window() {
        let newer: Vec<_> = [150, 120, 90, 60, 30].into_iter().map(|m| obs_uptime(TAG, m, Some(99.9))).collect();
        let low = obs_uptime(TAG, 180, Some(99.49));
        let r = || vec![RawEndpoint { uptime_last_1d: Some(99.55), ..row(TAG) }];
        let result = evaluate(r(), history([vec![low.clone()], newer.clone()].concat()));
        assert_eq!(result.window.len(), 6);
        assert!(!result.window.iter().any(|o| o.uptime_1d == Some(99.49)));
        assert_eq!(result.reasons, NONE);
        // Control: with only four newer observations it is the 6th-newest, inside the window.
        assert_eq!(reasons_of(r(), history([vec![low], newer[1..].to_vec()].concat())), ["readmission needs 99.6% (now 99.55%)"]);
    }

    #[test]
    fn g4_a_low_observation_8_days_old_is_outside_the_window() {
        let r = || vec![RawEndpoint { uptime_last_1d: Some(99.55), ..row(TAG) }];
        assert_eq!(reasons_of(r(), history(vec![obs_uptime(TAG, 8 * DAY_MINUTES, Some(99.49))])), NONE);
        // Control: 6 days 23 hours old is inside.
        assert_eq!(reasons_of(r(), history(vec![obs_uptime(TAG, 7 * DAY_MINUTES - 60, Some(99.49))])), ["readmission needs 99.6% (now 99.55%)"]);
    }

    #[test]
    fn rule_3_ignores_a_missing_uptime_and_needs_a_value_strictly_below_the_minimum() {
        let r = || vec![RawEndpoint { uptime_last_1d: Some(99.55), ..row(TAG) }];
        assert_eq!(reasons_of(r(), history(vec![obs_uptime(TAG, 30, None)])), NONE);
        assert_eq!(reasons_of(r(), history(vec![obs_uptime(TAG, 30, Some(99.5))])), NONE);
    }

    #[test]
    fn rule_2_boundary_exactly_99_5_passes_clean_and_needs_readmission_after_a_low_one() {
        let r = || vec![RawEndpoint { uptime_last_1d: Some(99.5), ..row(TAG) }];
        assert_eq!(reasons_of(r(), history(vec![obs(TAG, 30)])), NONE);
        assert_eq!(reasons_of(r(), history(vec![obs_uptime(TAG, 30, Some(99.49))])), ["readmission needs 99.6% (now 99.50%)"]);
    }

    #[test]
    fn g5_a_current_value_below_the_minimum_gives_only_the_uptime_reason() {
        let result = evaluate(vec![RawEndpoint { uptime_last_1d: Some(99.49), ..row(TAG) }], history(vec![obs_uptime(TAG, 30, Some(99.4))]));
        assert_eq!(result.reasons, ["uptime 99.49% < 99.5%"]);
        assert_eq!(result.families, [GateFamily::Reliability]);
    }

    #[test]
    fn g6_a_5_minute_outage_excludes_immediately_despite_a_clean_history() {
        let clean = || history([30, 60, 90, 120, 150].into_iter().map(|m| obs(TAG, m)).collect());
        assert_eq!(reasons_of(vec![RawEndpoint { uptime_last_5m: Some(94.9), ..row(TAG) }], clean()), ["currently degraded (5m uptime 94.9%)"]);
        assert_eq!(reasons_of(vec![RawEndpoint { uptime_last_5m: Some(95.0), ..row(TAG) }], clean()), NONE);
    }

    #[test]
    fn g7_a_non_zero_status_excludes_immediately_despite_a_clean_history() {
        let clean = history([30, 60, 90, 120, 150].into_iter().map(|m| obs(TAG, m)).collect());
        assert_eq!(reasons_of(vec![RawEndpoint { status: -2, ..row(TAG) }], clean), ["status -2"]);
    }

    #[test]
    fn g8_missing_1_day_uptime_excludes_and_missing_5_minute_uptime_does_not() {
        assert_eq!(reasons_of(vec![RawEndpoint { uptime_last_1d: None, ..row(TAG) }], Setup::default()), ["no uptime data"]);
        // Rules 1–3 are exclusive: a low observation in the window adds nothing.
        let low = history(vec![obs_uptime(TAG, 30, Some(99.0))]);
        assert_eq!(reasons_of(vec![RawEndpoint { uptime_last_1d: None, ..row(TAG) }], low), ["no uptime data"]);
        assert_eq!(reasons_of(vec![RawEndpoint { uptime_last_5m: None, ..row(TAG) }], Setup::default()), NONE);
    }

    #[test]
    fn g9_99_4951_prints_truncated_as_99_49() {
        let r = RawEndpoint { uptime_last_1d: Some(99.4951480572302), ..row("inference-net") };
        assert_eq!(reasons_of(vec![r], Setup::default()), ["uptime 99.49% < 99.5%"]);
    }

    #[test]
    fn g10_guardrail_removal_excludes_and_an_unparsed_list_excludes_nothing() {
        let removed = Setup { account: AccountEligibility { guardrail_removed: Some(vec![TAG.into()]), ..account() }, ..Setup::default() };
        let result = evaluate(vec![row(TAG)], removed);
        assert_eq!(result.reasons, ["removed by account guardrail"]);
        assert_eq!(result.families, [GateFamily::Account]);
        let unparsed = Setup { account: AccountEligibility { guardrail_removed: None, ..account() }, ..Setup::default() };
        assert_eq!(reasons_of(vec![row(TAG)], unparsed), NONE);
        let prefix = Setup { account: AccountEligibility { guardrail_removed: Some(vec!["x".into()]), ..account() }, ..Setup::default() };
        assert_eq!(reasons_of(vec![row(TAG)], prefix), NONE, "a prefix never matches");
    }

    #[test]
    fn g11_data_policy_removal_excludes_under_deny_only() {
        let removed = || AccountEligibility { data_policy_removed: Some(vec![TAG.into()]), ..account() };
        assert_eq!(reasons_of(vec![row(TAG)], Setup { account: removed(), ..Setup::default() }), ["removed by data policy"]);
        let allow = RoutingSettings { data_collection: DataCollection::Allow, ..RoutingSettings::default() };
        assert_eq!(reasons_of(vec![row(TAG)], Setup { account: removed(), settings: allow, ..Setup::default() }), NONE);
        let unparsed = AccountEligibility { data_policy_removed: None, ..account() };
        assert_eq!(reasons_of(vec![row(TAG)], Setup { account: unparsed, ..Setup::default() }), NONE);
    }

    #[test]
    fn g12_undeclared_quantization_from_a_non_first_party_provider_is_excluded() {
        let together = |q: Option<&str>| RawEndpoint { provider_name: "Together".into(), ..quant("together", q) };
        let result = evaluate(vec![together(None)], Setup::default());
        assert_eq!(result.reasons, ["quantization not declared"]);
        assert_eq!(result.families, [GateFamily::Precision]);
        assert_eq!(result.effective_quantization, Quantization::Unknown);
        assert_eq!(reasons_of(vec![together(Some("unknown"))], Setup::default()), ["quantization not declared"]);
    }

    #[test]
    fn g13_undeclared_quantization_from_the_first_party_is_admitted_at_native() {
        let result = evaluate(vec![RawEndpoint { provider_name: "DeepSeek".into(), ..quant("deepseek", None) }], Setup::default());
        assert_eq!(result.reasons, NONE);
        assert_eq!(result.effective_quantization, Quantization::Fp8);
    }

    #[test]
    fn g14_a_verification_record_admits_only_its_exact_tag_while_active() {
        let undeclared = || vec![RawEndpoint { provider_name: "Wafer".into(), ..quant("wafer", None) }];
        let admitted = evaluate(undeclared(), with_verified(vec![verification("wafer", "2026-09-01T00:00:00Z", "2026-12-01T00:00:00Z")]));
        assert_eq!(admitted.reasons, NONE);
        assert_eq!(admitted.effective_quantization, Quantization::Fp8);
        let expired = with_verified(vec![verification("wafer", "2026-08-01T00:00:00Z", "2026-10-02T09:20:00Z")]);
        assert_eq!(reasons_of(undeclared(), expired), ["quantization not declared"]);
        let other_tag = with_verified(vec![verification("wafer/fp8", "2026-09-01T00:00:00Z", "2026-12-01T00:00:00Z")]);
        assert_eq!(reasons_of(undeclared(), other_tag), ["quantization not declared"]);
    }

    #[test]
    fn g15_strict_policy_excludes_undeclared_quantization_even_from_the_first_party() {
        let strict = Setup { settings: RoutingSettings { unknown_quant_policy: UnknownQuantPolicy::Strict, ..RoutingSettings::default() }, ..Setup::default() };
        let result = evaluate(vec![RawEndpoint { provider_name: "DeepSeek".into(), ..quant("deepseek", None) }], strict);
        assert_eq!(result.reasons, ["quantization not declared"]);
        assert_eq!(result.effective_quantization, Quantization::Unknown);
    }

    #[test]
    fn g16_rows_that_disagree_on_quantization_take_the_undeclared_path_with_their_own_reason() {
        let result = evaluate(vec![quant(TAG, Some("fp8")), quant(TAG, Some("fp16"))], Setup::default());
        assert_eq!(result.reasons, ["quantization differs across rows sharing this tag"]);
        assert_eq!(result.effective_quantization, Quantization::Unknown);
        assert_eq!(reasons_of(vec![quant(TAG, Some("fp8")), quant(TAG, None)], Setup::default()), ["quantization differs across rows sharing this tag"]);
        // A first-party mixed tag is admitted like any undeclared one.
        let ds = |q: &str| RawEndpoint { provider_name: "DeepSeek".into(), ..quant("deepseek", Some(q)) };
        let first_party = evaluate(vec![ds("fp8"), ds("fp16")], Setup::default());
        assert_eq!(first_party.reasons, NONE);
        assert_eq!(first_party.effective_quantization, Quantization::Fp8);
    }

    #[test]
    fn g17_below_native_precision_is_excluded_and_above_native_passes() {
        let fp4 = evaluate(vec![quant(TAG, Some("fp4"))], Setup::default());
        assert_eq!(fp4.reasons, ["fp4 below native fp8"]);
        assert_eq!(fp4.effective_quantization, Quantization::Fp4);
        assert_eq!(reasons_of(vec![quant(TAG, Some("int8"))], Setup::default()), ["int8 below native fp8"]);
        let fp32 = evaluate(vec![quant(TAG, Some("fp32"))], Setup::default());
        assert_eq!(fp32.reasons, NONE);
        assert_eq!(fp32.effective_quantization, Quantization::Fp32);
        assert_eq!(reasons_of(vec![quant(TAG, Some("bf16"))], Setup::default()), NONE);
    }

    #[test]
    fn g18_closed_weights_skip_the_precision_filter() {
        let result = evaluate(vec![quant(TAG, Some("fp4"))], closed_weights());
        assert_eq!(result.reasons, NONE);
        assert_eq!(result.effective_quantization, Quantization::Fp4);
        let undeclared = evaluate(vec![quant(TAG, None)], closed_weights());
        assert_eq!(undeclared.reasons, NONE);
        assert_eq!(undeclared.effective_quantization, Quantization::Unknown);
    }

    fn deepseek_rows(quants: &[Option<&str>]) -> Vec<RawEndpoint> {
        quants.iter().map(|q| RawEndpoint { provider_name: "DeepSeek".into(), ..quant("deepseek", *q) }).collect()
    }

    #[test]
    fn c11_a_first_party_fp4_and_int4_names_the_first_below_native_row() {
        let result = evaluate(deepseek_rows(&[Some("fp4"), Some("int4")]), Setup::default());
        assert_eq!(result.reasons, ["int4 below native fp8"]);
        assert_eq!(result.families, [GateFamily::Precision]);
        assert_eq!(result.effective_quantization, Quantization::Unknown);
    }

    #[test]
    fn c11_b_first_party_fp8_and_fp4() {
        let result = evaluate(deepseek_rows(&[Some("fp8"), Some("fp4")]), Setup::default());
        assert_eq!(result.reasons, ["fp4 below native fp8"]);
        assert_eq!(result.effective_quantization, Quantization::Unknown);
    }

    #[test]
    fn c11_c_non_first_party_fp8_and_fp4_gets_the_below_native_reason() {
        let result = evaluate(vec![quant(TAG, Some("fp8")), quant(TAG, Some("fp4"))], Setup::default());
        assert_eq!(result.reasons, ["fp4 below native fp8"]);
        assert_eq!(result.effective_quantization, Quantization::Unknown);
    }

    #[test]
    fn c11_d_first_party_fp8_and_fp16_is_still_admitted_at_native() {
        let result = evaluate(deepseek_rows(&[Some("fp8"), Some("fp16")]), Setup::default());
        assert_eq!(result.reasons, NONE);
        assert_eq!(result.effective_quantization, Quantization::Fp8);
        assert_eq!(evaluate(deepseek_rows(&[Some("fp8"), None]), Setup::default()).reasons, NONE, "undeclared is not below native");
    }

    #[test]
    fn c11_e_an_active_verification_does_not_admit_fp8_and_fp4() {
        let verified = || with_verified(vec![verification(TAG, "2026-09-01T00:00:00Z", "2026-12-01T00:00:00Z")]);
        let result = evaluate(vec![quant(TAG, Some("fp8")), quant(TAG, Some("fp4"))], verified());
        assert_eq!(result.reasons, ["fp4 below native fp8"]);
        assert_eq!(result.effective_quantization, Quantization::Unknown);
        assert_eq!(evaluate(vec![quant(TAG, Some("fp8")), quant(TAG, Some("fp16"))], verified()).reasons, NONE);
    }

    #[test]
    fn c11_f_closed_weights_skip_the_rule() {
        let result = evaluate(deepseek_rows(&[Some("fp4"), Some("int4")]), closed_weights());
        assert_eq!(result.reasons, NONE);
        assert_eq!(result.effective_quantization, Quantization::Unknown);
    }

    #[test]
    fn g19_reasoning_is_required_only_when_an_effort_is_set() {
        let params = |p: &[&str]| RawEndpoint { supported_parameters: p.iter().map(|s| (*s).to_owned()).collect(), ..row(TAG) };
        let result = evaluate(vec![params(&["tools", "tool_choice", "max_tokens"])], Setup::default());
        assert_eq!(result.reasons, ["missing parameter: reasoning"]);
        assert_eq!(result.families, [GateFamily::Capability]);
        assert_eq!(reasons_of(vec![params(&["tools", "tool_choice", "max_tokens"])], Setup { effort: None, ..Setup::default() }), NONE);
        assert_eq!(
            reasons_of(vec![params(&["max_tokens"])], Setup::default()),
            ["missing parameter: tools", "missing parameter: tool_choice", "missing parameter: reasoning"]
        );
    }

    #[test]
    fn g20_context_below_the_registry_minimum() {
        assert_eq!(reasons_of(vec![RawEndpoint { context_length: 131_072, ..row(TAG) }], Setup::default()), ["context 131072 < 262144"]);
        assert_eq!(reasons_of(vec![RawEndpoint { context_length: 262_144, ..row(TAG) }], Setup::default()), NONE);
    }

    #[test]
    fn g21_max_output_below_the_profile_minimum_and_missing_is_ignored() {
        let max = |m: Option<u64>| vec![RawEndpoint { max_completion_tokens: m, ..row(TAG) }];
        assert_eq!(reasons_of(max(Some(32_768)), Setup::default()), ["max output 32768 < 65536"]);
        assert_eq!(reasons_of(max(Some(64_000)), Setup { profile: RoutingProfileId::Helper, ..Setup::default() }), NONE);
        assert_eq!(reasons_of(max(Some(64_000)), Setup::default()), ["max output 64000 < 65536"]);
        assert_eq!(reasons_of(max(None), Setup::default()), NONE);
    }

    #[test]
    fn g22_zero_or_missing_speed_gives_no_speed_data() {
        let zero = evaluate(vec![RawEndpoint { throughput_last_30m: pct(Some(0.0), Some(10.0)), ..row(TAG) }], Setup::default());
        assert_eq!(zero.reasons, ["no speed data"]);
        assert_eq!(zero.families, [GateFamily::Speed]);
        assert_eq!(zero.speed.tps_p50, None);
        assert_eq!(reasons_of(vec![RawEndpoint { latency_last_30m: None, ..row(TAG) }], Setup::default()), ["no speed data"]);
        assert_eq!(reasons_of(vec![RawEndpoint { latency_last_30m: pct(None, Some(900.0)), ..row(TAG) }], Setup::default()), ["no speed data"]);
        let neither = RawEndpoint { throughput_last_30m: None, latency_last_30m: None, ..row(TAG) };
        assert_eq!(reasons_of(vec![neither], Setup::default()), ["no speed data"]);
    }

    #[test]
    fn g23_an_unusable_price_excludes() {
        let r = RawEndpoint { pricing: pricing("-1", "0.0000004", Some("0.00000001")), ..row(TAG) };
        let result = evaluate(vec![r], Setup::default());
        assert_eq!(result.reasons, ["no usable price"]);
        assert_eq!(result.families, [GateFamily::Price]);
    }

    #[test]
    fn g24_an_alibaba_like_row_lists_every_failing_rule_in_canonical_order() {
        let r = RawEndpoint {
            provider_name: "Alibaba".into(),
            quantization: None,
            uptime_last_1d: Some(97.18),
            uptime_last_5m: Some(92.366),
            status: -2,
            ..row("alibaba")
        };
        let result = evaluate(vec![r], Setup::default());
        assert_eq!(result.reasons, ["uptime 97.18% < 99.5%", "currently degraded (5m uptime 92.3%)", "status -2", "quantization not declared"]);
        use GateFamily::*;
        assert_eq!(result.families, [Reliability, Reliability, Reliability, Precision]);
    }

    #[test]
    fn every_family_in_canonical_order_when_everything_fails() {
        let r = RawEndpoint {
            quantization: Some("fp4".into()),
            uptime_last_1d: None,
            uptime_last_5m: Some(50.0),
            status: 1,
            supported_parameters: vec![],
            context_length: 1000,
            max_completion_tokens: Some(1000),
            throughput_last_30m: None,
            pricing: pricing("0", "0", Some("0")),
            ..row(TAG)
        };
        let all_removed = AccountEligibility { guardrail_removed: Some(vec![TAG.into()]), data_policy_removed: Some(vec![TAG.into()]), ..account() };
        let result = evaluate(vec![r], Setup { account: all_removed, ..Setup::default() });
        assert_eq!(
            result.reasons,
            [
                "no uptime data",
                "currently degraded (5m uptime 50.0%)",
                "status 1",
                "removed by account guardrail",
                "removed by data policy",
                "fp4 below native fp8",
                "missing parameter: tools",
                "missing parameter: tool_choice",
                "missing parameter: max_tokens",
                "missing parameter: reasoning",
                "context 1000 < 262144",
                "max output 1000 < 65536",
                "no speed data",
                "no usable price",
            ]
        );
        use GateFamily::*;
        assert_eq!(
            result.families,
            [Reliability, Reliability, Reliability, Account, Account, Precision, Capability, Capability, Capability, Capability, Capability, Capability, Speed, Price]
        );
    }

    #[test]
    fn m1_medians_of_the_positive_values_and_a_zero_speed_is_ignored() {
        let settings = RoutingSettings::default();
        let window: Vec<_> = [100.0, 50.0, 80.0, 0.0, 90.0, 70.0]
            .into_iter()
            .enumerate()
            .map(|(i, t)| RoutingObservation { tps_p50: Some(t), latency_p50_ms: Some(400.0 + 100.0 * i as f64), ..obs(TAG, 10 * (i as i64 + 1)) })
            .collect();
        let speed = smooth_speed(&window, &settings);
        assert_eq!(speed.tps_p50, Some(80.0));
        assert_eq!(speed.observations, 5);
        assert!(!speed.limited_history);
        close(speed.latency_p50_s, 0.65);
    }

    #[test]
    fn m2_median() {
        assert_eq!(median(&[62.0, 63.0, 67.0, 88.0]), Some(65.0));
        assert_eq!(median(&[88.0, 62.0, 67.0, 63.0]), Some(65.0));
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&[7.0]), Some(7.0));
        assert_eq!(median(&[]), None);
        let input = [3.0, 1.0, 2.0];
        median(&input);
        assert_eq!(input, [3.0, 1.0, 2.0], "the input is not changed");
    }

    #[test]
    fn m3_two_observations_mark_limited_history_and_every_gate_still_applies() {
        let failing = evaluate(vec![RawEndpoint { status: -2, ..row(TAG) }], history(vec![obs(TAG, 45)]));
        assert_eq!(failing.speed.observations, 2);
        assert!(failing.speed.limited_history);
        assert_eq!(failing.reasons, ["status -2"]);
        let passing = evaluate(vec![row(TAG)], history(vec![obs(TAG, 45)]));
        assert!(passing.speed.limited_history);
        assert_eq!(passing.reasons, NONE, "limited history is not itself a reason");
        assert!(!evaluate(vec![row(TAG)], history(vec![obs(TAG, 45), obs(TAG, 75)])).speed.limited_history, "three are stable");
    }

    #[test]
    fn observations_is_the_smaller_of_the_positive_tps_and_latency_counts() {
        let window = [
            RoutingObservation { latency_p50_ms: None, ..obs(TAG, 10) },
            obs(TAG, 20),
            RoutingObservation { latency_p50_ms: Some(0.0), ..obs(TAG, 30) },
        ];
        let speed = smooth_speed(&window, &RoutingSettings::default());
        assert_eq!(speed.observations, 1);
        assert!(speed.limited_history);
        assert_eq!(speed.tps_p50, Some(100.0));
        close(speed.latency_p50_s, 0.5);
    }

    #[test]
    fn m4_only_the_six_newest_observations_are_used() {
        let settings = RoutingSettings::default();
        let mut h: Vec<_> = [60, 120, 180, 240, 300, 360]
            .into_iter()
            .enumerate()
            .map(|(i, m)| RoutingObservation { tps_p50: Some(100.0 * (i as f64 + 1.0)), ..obs(TAG, m) })
            .collect();
        h.push(RoutingObservation { tps_p50: Some(1.0), ..obs(TAG, 420) });
        h.push(RoutingObservation { tps_p50: Some(2.0), ..obs(TAG, 480) });
        let window = select_window(&merge_history(&h, &[]), TAG, ms(NOW), &settings);
        assert_eq!(window.iter().map(|o| o.tps_p50.unwrap()).collect::<Vec<_>>(), [100.0, 200.0, 300.0, 400.0, 500.0, 600.0]);
        assert_eq!(smooth_speed(&window, &settings).tps_p50, Some(350.0));
    }

    #[test]
    fn m5_an_observation_after_now_or_older_than_7_days_is_ignored_and_both_bounds_are_inside() {
        let settings = RoutingSettings::default();
        let at = |m: i64, t: f64| RoutingObservation { tps_p50: Some(t), ..obs(TAG, m) };
        let h = [at(-1, 1.0), at(7 * DAY_MINUTES + 1, 2.0), at(0, 10.0), at(7 * DAY_MINUTES, 20.0)];
        let window = select_window(&merge_history(&h, &[]), TAG, ms(NOW), &settings);
        assert_eq!(window.iter().map(|o| o.tps_p50.unwrap()).collect::<Vec<_>>(), [10.0, 20.0]);
        assert!(select_window(&[obs("other", 10)], TAG, ms(NOW), &settings).is_empty(), "other tags never enter");
    }

    #[test]
    fn m6_latency_converts_from_milliseconds_to_seconds() {
        let window = [RoutingObservation { latency_p50_ms: Some(1428.5), latency_p90_ms: Some(3794.9), ..obs(TAG, 15) }];
        let speed = smooth_speed(&window, &RoutingSettings::default());
        close(speed.latency_p50_s, 1.4285);
        close(speed.latency_p90_s, 3.7949);
    }

    #[test]
    fn m7_a_stored_copy_of_the_snapshot_observation_is_counted_once() {
        let settings = RoutingSettings::default();
        let snapshot_obs = extract_observations(&EndpointSnapshot { fetched_at_ms: ms(FETCHED_AT), endpoints: vec![row(TAG)] });
        // `…:00Z` and `…:00.000Z` are one instant: in milliseconds they are the same number.
        let stored = RoutingObservation { observed_at_ms: ms("2026-10-02T09:05:00.000Z"), ..snapshot_obs[0].clone() };
        let merged = merge_history(std::slice::from_ref(&stored), &snapshot_obs);
        assert_eq!(merged.len(), 1);
        assert_eq!(smooth_speed(&select_window(&merged, TAG, ms(NOW), &settings), &settings).observations, 1);
        assert_eq!(evaluate(vec![row(TAG)], history(vec![stored])).speed.observations, 1);
    }

    #[test]
    fn merge_history_collapses_duplicates_with_the_k8_c7_rules() {
        let at = ms("2026-10-02T09:05:00Z");
        let a = RoutingObservation {
            tag: "t".into(),
            observed_at_ms: at,
            uptime_1d: Some(99.8),
            uptime_5m: Some(99.0),
            status: 0,
            tps_p50: Some(80.0),
            tps_p90: Some(120.0),
            latency_p50_ms: Some(400.0),
            latency_p90_ms: Some(900.0),
        };
        let b = RoutingObservation {
            uptime_1d: Some(99.9),
            uptime_5m: Some(98.0),
            status: -2,
            tps_p50: Some(90.0),
            tps_p90: Some(110.0),
            latency_p50_ms: Some(500.0),
            latency_p90_ms: Some(800.0),
            ..a.clone()
        };
        assert_eq!(
            merge_history(std::slice::from_ref(&a), std::slice::from_ref(&b)),
            [RoutingObservation {
                uptime_1d: Some(99.8),
                uptime_5m: Some(98.0),
                status: -2,
                tps_p50: Some(80.0),
                tps_p90: Some(110.0),
                latency_p50_ms: Some(500.0),
                latency_p90_ms: Some(900.0),
                ..a.clone()
            }]
        );
        let with_missing = merge_history(&[RoutingObservation { uptime_1d: None, latency_p50_ms: None, ..a }], &[b]);
        assert_eq!((with_missing[0].uptime_1d, with_missing[0].latency_p50_ms, with_missing[0].tps_p50), (None, None, Some(80.0)));
    }

    #[test]
    fn merge_history_sorts_by_tag_then_newest_first_whatever_the_input_order() {
        let list = [obs("b", 30), obs("a", 60), obs("B", 10), obs("a", 15), obs("b", 90)];
        let merged = merge_history(&list, &[]);
        let key = |o: &RoutingObservation| (o.tag.clone(), o.observed_at_ms);
        assert_eq!(
            merged.iter().map(key).collect::<Vec<_>>(),
            [key(&obs("B", 10)), key(&obs("a", 15)), key(&obs("a", 60)), key(&obs("b", 30)), key(&obs("b", 90))]
        );
        let reversed: Vec<_> = list.iter().rev().cloned().collect();
        assert_eq!(merge_history(&reversed, &[]), merged);
        assert_eq!(merge_history(&list[2..], &list[..2]), merged);
    }

    #[test]
    fn merged_history_has_no_negative_zero_and_survives_json() {
        let merged = merge_history(&[RoutingObservation { uptime_5m: Some(-0.0), ..obs("a", 30) }, RoutingObservation { uptime_5m: Some(0.0), ..obs("a", 30) }], &[]);
        assert!(merged[0].uptime_5m.unwrap().is_sign_positive());
        assert_round_trip(&merged);
    }
}
