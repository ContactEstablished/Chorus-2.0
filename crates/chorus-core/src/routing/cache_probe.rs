//! The automatic prompt-cache probe (MR-D9, MR-D18, K4, C4–C7), ported from Chorus 1's
//! `cacheProbeCore.ts`. OpenRouter's `supports_implicit_caching` doesn't predict whether an
//! endpoint serves cached reads, so caching is checked by sending one long prefix three times,
//! pinned to one endpoint, and reading `cached_tokens`. This plans which tags to probe within the
//! $0.05 cap, builds the request, and turns the three answers into a verdict and a spend.
//!
//! The prompt is Chorus 1's measured shape (C4), never user content. The run nonce, `now_ms` and
//! the stored records are parameters: the app draws the nonce and reads the clock.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use super::js::by_code_unit;
use super::preflight::ChatMessage;
use super::types::{CacheVerification, CandidateCost, TierResult};

/// MR-D18: the most one refresh may spend on probes.
pub const CACHE_PROBE_CAP_USD: f64 = 0.05;
/// MR-D9: a record older than 14 days is probed again.
pub const CACHE_PROBE_MAX_AGE_MS: i64 = 14 * 24 * 60 * 60 * 1000;
pub const CACHE_PROBE_CALLS: u32 = 3;
pub const CACHE_PROBE_MAX_TOKENS: u32 = 64;
/// Measured 4,457–4,464 (C4).
pub const CACHE_PROBE_PROMPT_TOKENS: u32 = 4500;
pub const CACHE_PROBE_FILLER_LINES: u32 = 220;
pub const CACHE_PROBE_CONCURRENCY: u32 = 5;
pub const CACHE_PROBE_GAP_MS: u32 = 1500;
pub const CACHE_PROBE_USER_TEXT: &str = "Reply with the single word OK.";

/// What a tag's probe showed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum CacheProbeOutcome {
    /// Call 2 or 3 read cached tokens.
    Verified,
    /// All three calls succeeded and each read exactly none.
    NotCached,
    /// Anything else: a failed call, a missing count, or only call 1 cached.
    Inconclusive,
}

/// Why an eligible, due tag was not probed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum ProbeSkipReason {
    /// The $ cap.
    Cap,
    /// A verification-only tag limit.
    Limit,
    /// The refresh was stopped.
    Aborted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProbeSkip {
    pub tag: String,
    pub reason: ProbeSkipReason,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PlannedProbe {
    pub tag: String,
    pub estimate_usd: f64,
}

/// Which tags a refresh probes, and why the others aren't.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProbePlan {
    /// In Balanced-score order.
    pub planned: Vec<PlannedProbe>,
    /// The sum of `planned`, in plan order.
    pub estimate_usd: f64,
    pub cap_usd: f64,
    /// Eligible tags with a record at most 14 days old, sorted by tag.
    pub fresh: Vec<String>,
    /// `Cap` or `Limit` only, in Balanced-score order.
    pub not_probed: Vec<ProbeSkip>,
}

/// What a refresh did: its probe estimate and spend, the tags it probed and skipped, and the
/// tiers it computed afterwards.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RoutingRefreshReport {
    pub estimate_usd: f64,
    pub spent_usd: f64,
    pub probed: Vec<String>,
    pub not_probed: Vec<ProbeSkip>,
    pub result: TierResult,
}

/// Chorus 1's filler, byte for byte: 220 lines, 16,305 characters.
pub fn probe_filler() -> String {
    (0..CACHE_PROBE_FILLER_LINES)
        .map(|i| format!("export function step{i}(value: number): number {{ return value * {} + {}; }}", i + 3, (i * 7) % 11))
        .collect::<Vec<_>>()
        .join("\n")
}

/// C5: the nonce is per run and the tag is part of the prefix, so two tags never share one.
pub fn probe_system_text(run_nonce: &str, tag: &str) -> String {
    format!("Session {run_nonce} {tag}. Reference module follows.\n{}", probe_filler())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProbeProvider {
    pub order: Vec<String>,
    pub allow_fallbacks: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProbeReasoning {
    pub effort: &'static str,
}

/// A probe call's body. No `data_collection`: the probe carries no user content, and this is the
/// shape Chorus 1 measured. Serialised, its keys are in that order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProbeRequestBody {
    pub model: String,
    pub provider: ProbeProvider,
    pub messages: Vec<ChatMessage>,
    pub reasoning: ProbeReasoning,
    pub max_tokens: u32,
}

pub fn probe_request_body(model: &str, tag: &str, run_nonce: &str) -> ProbeRequestBody {
    ProbeRequestBody {
        model: model.to_owned(),
        provider: ProbeProvider { order: vec![tag.to_owned()], allow_fallbacks: false },
        messages: vec![
            ChatMessage { role: "system", content: probe_system_text(run_nonce, tag) },
            ChatMessage { role: "user", content: CACHE_PROBE_USER_TEXT.to_owned() },
        ],
        reasoning: ProbeReasoning { effort: "low" },
        max_tokens: CACHE_PROBE_MAX_TOKENS,
    }
}

/// The worst case for one tag: three calls at uncached prices, 4,500 prompt and 64 output
/// tokens each, in USD.
pub fn probe_tag_estimate_usd(cost: &CandidateCost) -> f64 {
    let per_call = cost.prompt_per_m * f64::from(CACHE_PROBE_PROMPT_TOKENS) + cost.completion_per_m * f64::from(CACHE_PROBE_MAX_TOKENS);
    f64::from(CACHE_PROBE_CALLS) * per_call / 1_000_000.0
}

#[derive(Debug, Clone, Copy)]
pub struct ProbePlanInput<'a> {
    /// The refresh's own tiers.
    pub result: &'a TierResult,
    /// As stored.
    pub cache: &'a CacheVerification,
    pub now_ms: i64,
    pub cap_usd: f64,
    /// Verification only (C21): at most this many tags.
    pub tag_limit: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbePlanError {
    /// The cap is NaN, infinite or negative: it would plan nothing, or everything, silently.
    InvalidCap,
}

impl std::fmt::Display for ProbePlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Invalid cap")
    }
}

impl std::error::Error for ProbePlanError {}

/// Plans the probe (K4, C7). Eligible candidates are walked in Balanced-score order; a tag is
/// due when it has no record or its record is more than 14 days old. The cap cut is a prefix:
/// the first due tag that would take the estimate over the cap, and every later one, is
/// skipped as `Cap`, never skipping ahead to a cheaper tag.
pub fn plan_cache_probe(input: &ProbePlanInput<'_>) -> Result<ProbePlan, ProbePlanError> {
    if !input.cap_usd.is_finite() || input.cap_usd < 0.0 {
        return Err(ProbePlanError::InvalidCap);
    }

    // Eligible candidates, by Balanced score (highest first), ties by tag.
    let mut eligible: Vec<(&str, f64, f64)> = input
        .result
        .candidates
        .iter()
        .filter(|c| c.excluded_by.is_empty())
        .filter_map(|c| Some((c.tag.as_str(), c.scores.balanced?, probe_tag_estimate_usd(c.cost.as_ref()?))))
        .collect();
    eligible.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal).then_with(|| by_code_unit(a.0, b.0)));

    // Due or fresh. Exactly 14 days old is still fresh.
    let (due, mut fresh): (Vec<_>, Vec<_>) =
        eligible.into_iter().partition(|(tag, _, _)| input.cache.get(*tag).is_none_or(|r| input.now_ms - r.checked_at_ms > CACHE_PROBE_MAX_AGE_MS));
    fresh.sort_by(|a, b| by_code_unit(a.0, b.0));

    // Walk the due tags.
    let mut planned = Vec::new();
    let mut not_probed = Vec::new();
    let mut cap_cut = false;
    let mut cum = 0.0;
    for (tag, _, estimate_usd) in due {
        let skip = |reason| ProbeSkip { tag: tag.to_owned(), reason };
        if cap_cut {
            not_probed.push(skip(ProbeSkipReason::Cap));
        } else if input.tag_limit.is_some_and(|limit| planned.len() >= limit as usize) {
            not_probed.push(skip(ProbeSkipReason::Limit));
        } else if cum + estimate_usd > input.cap_usd {
            cap_cut = true;
            not_probed.push(skip(ProbeSkipReason::Cap));
        } else {
            planned.push(PlannedProbe { tag: tag.to_owned(), estimate_usd });
            cum += estimate_usd;
        }
    }

    Ok(ProbePlan { planned, estimate_usd: cum, cap_usd: input.cap_usd, fresh: fresh.into_iter().map(|(t, _, _)| t.to_owned()).collect(), not_probed })
}

/// The three usage numbers of a probe answer.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ProbeUsage {
    pub prompt_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
}

fn count_of(v: Option<&Value>) -> Option<u64> {
    let v = v?;
    if let Some(n) = v.as_u64() {
        return Some(n);
    }
    let f = v.as_f64()?;
    (f.fract() == 0.0 && f >= 0.0).then_some(f as u64)
}

/// Each usage number on its own; any missing level gives `None`. The completion text is never
/// read.
pub fn extract_probe_usage(body: &Value) -> ProbeUsage {
    let usage = body.as_object().and_then(|b| b.get("usage")).and_then(Value::as_object);
    let member = |key: &str| usage.and_then(|u| u.get(key));
    ProbeUsage {
        prompt_tokens: count_of(member("prompt_tokens")),
        cached_tokens: count_of(member("prompt_tokens_details").and_then(Value::as_object).and_then(|d| d.get("cached_tokens"))),
        cost_usd: member("cost").and_then(Value::as_f64).filter(|c| c.is_finite() && *c >= 0.0),
    }
}

/// One probe call: a 2xx with its usage, or no 2xx at all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProbeCall {
    /// A 2xx. A body that couldn't be read is `ProbeUsage::default()`: it may still have been
    /// billed, so it is charged its estimate.
    Ok(ProbeUsage),
    Failed,
}

/// K4, C6. `Verified` when call 2 or 3 reports cached tokens (a hit on call 1 is not evidence:
/// the nonce makes it cold). `NotCached` only when all three calls succeeded and each reported
/// exactly 0. Anything else is `Inconclusive`.
pub fn evaluate_probe(calls: &[ProbeCall]) -> CacheProbeOutcome {
    let hit = |i: usize| matches!(calls.get(i), Some(ProbeCall::Ok(u)) if u.cached_tokens.is_some_and(|n| n > 0));
    if hit(1) || hit(2) {
        return CacheProbeOutcome::Verified;
    }
    if calls.len() == CACHE_PROBE_CALLS as usize && calls.iter().all(|c| matches!(c, ProbeCall::Ok(u) if u.cached_tokens == Some(0))) {
        return CacheProbeOutcome::NotCached;
    }
    CacheProbeOutcome::Inconclusive
}

/// C6: a 2xx without a cost counts its per-call worst case; a call with no 2xx counts 0.
pub fn probe_call_spend_usd(call: &ProbeCall, per_call_estimate_usd: f64) -> f64 {
    match call {
        ProbeCall::Failed => 0.0,
        ProbeCall::Ok(u) => u.cost_usd.unwrap_or(per_call_estimate_usd),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::test_support::*;
    use crate::routing::tiers::compute_tiers;
    use crate::routing::types::{CacheRecord, CandidateExplanation, RoutingProfileId, TierScores};
    use serde_json::json;

    /// Money compares to 1e-12 absolute, as Chorus 1's specification prescribes.
    #[track_caller]
    fn money(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-12, "{actual} vs {expected}");
    }

    /// The golden tiers, computed with an empty cache.
    fn result() -> TierResult {
        let mut inputs = Inputs::golden(RoutingProfileId::Interactive);
        inputs.cache.clear();
        compute_tiers(&inputs.input()).unwrap()
    }

    const Q1_ORDER: [&str; 14] = [
        "atlas-cloud/fp8",
        "morph/fp8",
        "makora/fp8",
        "streamlake/fp8",
        "deepinfra/fp8",
        "venice/fp8",
        "gmicloud/fp8",
        "baidu/fp8",
        "parasail/fp8",
        "nextbit/fp8",
        "novita/fp8",
        "baseten/fp8",
        "siliconflow/fp8",
        "baseten/fast",
    ];

    fn eligible_sorted() -> Vec<&'static str> {
        let mut v = Q1_ORDER.to_vec();
        v.sort();
        v
    }

    fn plan_with(result: &TierResult, cache: &CacheVerification, now: &str, cap_usd: f64, tag_limit: Option<u32>) -> ProbePlan {
        plan_cache_probe(&ProbePlanInput { result, cache, now_ms: ms(now), cap_usd, tag_limit }).unwrap()
    }

    fn planned_tags(p: &ProbePlan) -> Vec<&str> {
        p.planned.iter().map(|x| x.tag.as_str()).collect()
    }

    fn skips(tags: &[&str], reason: ProbeSkipReason) -> Vec<ProbeSkip> {
        tags.iter().map(|t| ProbeSkip { tag: (*t).to_owned(), reason }).collect()
    }

    fn candidate<'a>(r: &'a TierResult, tag: &str) -> &'a CandidateExplanation {
        r.candidates.iter().find(|c| c.tag == tag).unwrap()
    }

    #[test]
    fn the_base_result_has_the_14_eligible_tags_of_the_golden_run() {
        let r = result();
        assert_eq!(r.candidates.iter().filter(|c| c.excluded_by.is_empty()).map(|c| c.tag.as_str()).collect::<Vec<_>>(), eligible_sorted());
    }

    #[test]
    fn q1_cap_0_05_no_limit_plans_all_14_in_balanced_order() {
        let r = result();
        let p = plan_with(&r, &CacheVerification::new(), NOW, 0.05, None);
        assert_eq!(planned_tags(&p), Q1_ORDER);
        money(p.estimate_usd, 0.0493873956);
        assert!(p.not_probed.is_empty() && p.fresh.is_empty());
        assert_eq!(p.cap_usd, 0.05);
        assert_eq!(p.estimate_usd, p.planned.iter().fold(0.0, |sum, x| sum + x.estimate_usd), "the sum, in plan order");
        let scores: Vec<f64> = p.planned.iter().map(|x| candidate(&r, &x.tag).scores.balanced.unwrap()).collect();
        assert!(scores.windows(2).all(|w| w[0] >= w[1]));
    }

    #[test]
    fn q2_cap_0_02_plans_the_first_seven_and_cuts_the_rest_by_the_cap() {
        let r = result();
        let p = plan_with(&r, &CacheVerification::new(), NOW, 0.02, None);
        assert_eq!(planned_tags(&p), Q1_ORDER[..7]);
        money(p.estimate_usd, 0.017288676);
        assert_eq!(p.not_probed, skips(&Q1_ORDER[7..], ProbeSkipReason::Cap));
        let baidu = probe_tag_estimate_usd(candidate(&r, "baidu/fp8").cost.as_ref().unwrap());
        money(p.estimate_usd + baidu, 0.0215647956);
    }

    #[test]
    fn c7_the_cap_cut_is_a_prefix_and_a_cheaper_later_tag_is_still_cut() {
        let mut r = result();
        let base = candidate(&r, "atlas-cloud/fp8").clone();
        let per_tag = |usd: f64| usd * 1_000_000.0 / (3.0 * 4500.0);
        let priced = |tag: &str, balanced: f64, prompt_per_m: f64| CandidateExplanation {
            tag: tag.into(),
            scores: TierScores { balanced: Some(balanced), ..TierScores::default() },
            cost: base.cost.map(|c| CandidateCost { prompt_per_m, completion_per_m: 0.0, ..c }),
            ..base.clone()
        };
        r.candidates = vec![priced("a/fp8", 3.0, per_tag(0.01)), priced("b/fp8", 2.0, per_tag(0.05)), priced("c/fp8", 1.0, per_tag(0.001))];
        let p = plan_with(&r, &CacheVerification::new(), NOW, 0.02, None);
        assert_eq!(planned_tags(&p), ["a/fp8"]);
        assert_eq!(p.not_probed, skips(&["b/fp8", "c/fp8"], ProbeSkipReason::Cap));
    }

    #[test]
    fn c7_a_tag_limit_cuts_the_same_way_and_wins_over_a_later_cap_cut() {
        let p = plan_with(&result(), &CacheVerification::new(), NOW, 0.02, Some(7));
        assert_eq!(planned_tags(&p), Q1_ORDER[..7]);
        assert_eq!(p.not_probed, skips(&Q1_ORDER[7..], ProbeSkipReason::Limit));
    }

    #[test]
    fn q3_cap_0_05_with_tag_limit_2() {
        let p = plan_with(&result(), &CacheVerification::new(), NOW, 0.05, Some(2));
        assert_eq!(planned_tags(&p), ["atlas-cloud/fp8", "morph/fp8"]);
        money(p.estimate_usd, 0.002497428);
        assert_eq!(p.not_probed, skips(&Q1_ORDER[2..], ProbeSkipReason::Limit));
    }

    #[test]
    fn q4_every_eligible_tag_with_a_fresh_record_plans_nothing() {
        let p = plan_with(&result(), &golden_cache(ms(CHECKED_AT)), NOW, 0.05, None);
        assert!(p.planned.is_empty() && p.not_probed.is_empty());
        assert_eq!(p.estimate_usd, 0.0);
        assert_eq!(p.fresh, eligible_sorted());
    }

    #[test]
    fn q5_exactly_14_days_old_is_fresh_and_one_second_more_is_due() {
        let r = result();
        let cache = golden_cache(ms(CHECKED_AT));
        let at_boundary = plan_with(&r, &cache, "2026-10-16T09:15:39Z", 0.05, None);
        assert_eq!(at_boundary.fresh, eligible_sorted());
        assert!(at_boundary.planned.is_empty());
        let past = plan_with(&r, &cache, "2026-10-16T09:15:40Z", 0.05, None);
        assert!(past.fresh.is_empty());
        assert_eq!(planned_tags(&past), Q1_ORDER);
        money(past.estimate_usd, 0.0493873956);
        assert_eq!(CACHE_PROBE_MAX_AGE_MS, 14 * 24 * 60 * 60 * 1000);
    }

    #[test]
    fn q6_a_record_for_an_excluded_tag_is_never_planned_and_never_fresh() {
        let r = result();
        for checked in [CHECKED_AT, "2026-01-01T00:00:00Z"] {
            let cache: CacheVerification = [
                ("together".to_owned(), CacheRecord { verified: true, checked_at_ms: ms(checked) }),
                ("deepseek".to_owned(), CacheRecord { verified: false, checked_at_ms: ms(checked) }),
            ]
            .into();
            let p = plan_with(&r, &cache, NOW, 0.05, None);
            let named: Vec<&str> = planned_tags(&p).into_iter().chain(p.fresh.iter().map(String::as_str)).chain(p.not_probed.iter().map(|s| s.tag.as_str())).collect();
            assert!(!named.contains(&"together") && !named.contains(&"deepseek"));
            assert_eq!(planned_tags(&p), Q1_ORDER);
        }
    }

    #[test]
    fn q7_per_tag_estimates() {
        let r = result();
        let expected = [
            ("atlas-cloud/fp8", 0.002011788),
            ("morph/fp8", 0.00048564),
            ("venice/fp8", 0.0053505),
            ("baidu/fp8", 0.0042761196),
            ("baseten/fast", 0.0085608),
        ];
        let planned = plan_with(&r, &CacheVerification::new(), NOW, 0.05, None).planned;
        for (tag, usd) in expected {
            money(probe_tag_estimate_usd(candidate(&r, tag).cost.as_ref().unwrap()), usd);
            money(planned.iter().find(|x| x.tag == tag).unwrap().estimate_usd, usd);
        }
    }

    #[test]
    fn q9_reversed_candidates_give_an_equal_plan() {
        let r = result();
        let mut reversed = r.clone();
        reversed.candidates.reverse();
        assert_eq!(plan_with(&reversed, &CacheVerification::new(), NOW, 0.05, None), plan_with(&r, &CacheVerification::new(), NOW, 0.05, None));
    }

    #[test]
    fn a_nan_infinite_or_negative_cap_is_refused_and_zero_cuts_everything() {
        let r = result();
        let cache = CacheVerification::new();
        for cap_usd in [f64::NAN, f64::INFINITY, -0.01] {
            let err = plan_cache_probe(&ProbePlanInput { result: &r, cache: &cache, now_ms: ms(NOW), cap_usd, tag_limit: None }).unwrap_err();
            assert_eq!((err, err.to_string().as_str()), (ProbePlanError::InvalidCap, "Invalid cap"));
        }
        let zero = plan_with(&r, &cache, NOW, 0.0, None);
        assert!(zero.planned.is_empty());
        assert_eq!(zero.not_probed, skips(&Q1_ORDER, ProbeSkipReason::Cap));
        assert_eq!(plan_with(&r, &cache, NOW, 0.05, Some(0)).not_probed, skips(&Q1_ORDER, ProbeSkipReason::Limit));
    }

    #[test]
    fn the_plan_is_plain_json() {
        assert_round_trip(&plan_with(&result(), &CacheVerification::new(), NOW, 0.02, None));
    }

    fn call(status: u16, prompt: u64, cached: u64, cost: f64) -> ProbeCall {
        if status == 200 { ProbeCall::Ok(ProbeUsage { prompt_tokens: Some(prompt), cached_tokens: Some(cached), cost_usd: Some(cost) }) } else { ProbeCall::Failed }
    }

    fn spend(calls: &[ProbeCall]) -> f64 {
        calls.iter().fold(0.0, |sum, c| sum + probe_call_spend_usd(c, 0.0))
    }

    fn ok_cached(cached: Option<u64>) -> ProbeCall {
        ProbeCall::Ok(ProbeUsage { prompt_tokens: Some(4460), cached_tokens: cached, cost_usd: Some(0.0001) })
    }

    #[test]
    fn e1_to_e4_real_probes() {
        let atlas = [call(200, 4460, 0, 0.000656496), call(200, 4460, 4352, 0.0000833592), call(200, 4460, 4352, 0.0001126872)];
        assert_eq!(evaluate_probe(&atlas), CacheProbeOutcome::Verified);
        money(spend(&atlas), 0.0008525424);
        let baidu = [call(200, 4462, 0, 0.0013564422), call(200, 4462, 0, 0.0013900086), call(200, 4462, 0, 0.0014139846)];
        assert_eq!(evaluate_probe(&baidu), CacheProbeOutcome::NotCached);
        money(spend(&baidu), 0.0041604354);
        let baseten = [call(200, 4462, 0, 0.0014154), call(200, 4462, 0, 0.0013542), call(200, 4462, 4352, 0.000079064)];
        assert_eq!(evaluate_probe(&baseten), CacheProbeOutcome::Verified, "verified on call 3");
        assert_eq!(evaluate_probe(&baseten[..2]), CacheProbeOutcome::Inconclusive);
        let morph = [call(200, 4459, 0, 0.00014049), call(200, 4459, 0, 0.00013923), call(200, 4459, 4352, 0.000064906)];
        assert_eq!(evaluate_probe(&morph), CacheProbeOutcome::Verified);
        money(spend(&morph), 0.000344626);
    }

    #[test]
    fn e5_to_e8_failed_missing_and_first_call_hits() {
        assert_eq!(evaluate_probe(&[ok_cached(Some(0)), ProbeCall::Failed, ok_cached(Some(0))]), CacheProbeOutcome::Inconclusive, "E5");
        assert_eq!(evaluate_probe(&[ok_cached(Some(0)), ok_cached(None), ok_cached(Some(0))]), CacheProbeOutcome::Inconclusive, "E6");
        assert_eq!(evaluate_probe(&[ok_cached(Some(4352)), ok_cached(Some(0)), ok_cached(Some(0))]), CacheProbeOutcome::Inconclusive, "E7");
        assert_eq!(evaluate_probe(&[ok_cached(Some(0)), ok_cached(Some(4352))]), CacheProbeOutcome::Verified, "E8: an aborted refresh");
    }

    #[test]
    fn e9_spend_without_a_cost_is_the_estimate_and_a_failed_call_is_free() {
        let no_cost = ProbeCall::Ok(ProbeUsage { prompt_tokens: Some(4460), cached_tokens: Some(0), cost_usd: None });
        assert_eq!(probe_call_spend_usd(&no_cost, 0.003), 0.003);
        assert_eq!(probe_call_spend_usd(&ProbeCall::Failed, 0.003), 0.0);
        assert_eq!(probe_call_spend_usd(&ProbeCall::Ok(ProbeUsage::default()), 0.003), 0.003, "an unreadable 2xx is charged");
    }

    #[test]
    fn e10_e11_usage_extraction() {
        let real = json!({ "usage": { "prompt_tokens": 4460, "prompt_tokens_details": { "cached_tokens": 4352 }, "cost": 0.0000833592 } });
        assert_eq!(extract_probe_usage(&real), ProbeUsage { prompt_tokens: Some(4460), cached_tokens: Some(4352), cost_usd: Some(0.0000833592) });
        for empty in [json!({}), Value::Null, json!({ "usage": { "cost": "x", "prompt_tokens": -1 } }), json!({ "usage": [1] })] {
            assert_eq!(extract_probe_usage(&empty), ProbeUsage::default(), "{empty}");
        }
        let each = |v: Value| extract_probe_usage(&json!({ "usage": v }));
        assert_eq!(each(json!({ "prompt_tokens": 10 })), ProbeUsage { prompt_tokens: Some(10), ..ProbeUsage::default() });
        assert_eq!(each(json!({ "prompt_tokens_details": { "cached_tokens": 0 } })), ProbeUsage { cached_tokens: Some(0), ..ProbeUsage::default() });
        assert_eq!(each(json!({ "cost": 0 })), ProbeUsage { cost_usd: Some(0.0), ..ProbeUsage::default() });
        assert_eq!(each(json!({ "prompt_tokens": 1.5 })), ProbeUsage::default());
        assert_eq!(each(json!({ "prompt_tokens": 4460.0 })).prompt_tokens, Some(4460), "a whole float counts, as in JavaScript");
    }

    /// SHA-256, for pinning the filler byte for byte without a crypto crate.
    fn sha256_hex(data: &[u8]) -> String {
        const K: [u32; 64] = [
            0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
            0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
            0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
            0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
            0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
            0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
        ];
        let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
        let mut msg = data.to_vec();
        msg.push(0x80);
        while msg.len() % 64 != 56 {
            msg.push(0);
        }
        msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
        for chunk in msg.chunks(64) {
            let mut w = [0u32; 64];
            for i in 0..16 {
                w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
            }
            for i in 16..64 {
                let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
                let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
                w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
            }
            let mut v = h;
            for i in 0..64 {
                let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
                let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
                let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
                let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
                let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
                let t2 = s0.wrapping_add(maj);
                v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
            }
            for (a, b) in h.iter_mut().zip(v) {
                *a = a.wrapping_add(b);
            }
        }
        h.iter().map(|x| format!("{x:08x}")).collect()
    }

    #[test]
    fn b1_the_filler_is_chorus_1s_byte_for_byte() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", "the hash itself");
        let filler = probe_filler();
        let lines: Vec<&str> = filler.split('\n').collect();
        assert_eq!(lines.len(), CACHE_PROBE_FILLER_LINES as usize);
        assert_eq!(lines[0], "export function step0(value: number): number { return value * 3 + 0; }");
        assert_eq!(lines[219], "export function step219(value: number): number { return value * 222 + 4; }");
        assert_eq!(filler.len(), 16_305);
        assert_eq!(sha256_hex(filler.as_bytes()), "e0832703a43d98fc68a4809d982df851025aab4cb4f1cc8fc64a884d9cbca08c");
    }

    #[test]
    fn b2_the_probe_body_with_keys_in_exact_order_and_no_data_collection() {
        let body = probe_request_body(SLUG, "atlas-cloud/fp8", "N");
        let text = serde_json::to_string(&body).unwrap();
        let system = probe_system_text("N", "atlas-cloud/fp8");
        assert!(system.starts_with("Session N atlas-cloud/fp8. Reference module follows.\n"));
        assert_eq!(system, format!("Session N atlas-cloud/fp8. Reference module follows.\n{}", probe_filler()));
        assert_eq!(
            text,
            format!(
                r#"{{"model":"deepseek/deepseek-v4.1-flash","provider":{{"order":["atlas-cloud/fp8"],"allow_fallbacks":false}},"messages":[{{"role":"system","content":{}}},{{"role":"user","content":"Reply with the single word OK."}}],"reasoning":{{"effort":"low"}},"max_tokens":64}}"#,
                serde_json::to_string(&system).unwrap()
            )
        );
        assert!(!text.contains("data_collection"));
    }

    #[test]
    fn b3_one_nonce_two_tags_give_different_system_content() {
        let a = &probe_request_body(SLUG, "atlas-cloud/fp8", "N").messages[0].content;
        let b = &probe_request_body(SLUG, "morph/fp8", "N").messages[0].content;
        assert_ne!(a, b);
    }

    #[test]
    fn constants() {
        assert_eq!(CACHE_PROBE_CAP_USD, 0.05);
        assert_eq!((CACHE_PROBE_CALLS, CACHE_PROBE_MAX_TOKENS, CACHE_PROBE_PROMPT_TOKENS), (3, 64, 4500));
        assert_eq!((CACHE_PROBE_CONCURRENCY, CACHE_PROBE_GAP_MS), (5, 1500));
        assert_eq!(CACHE_PROBE_USER_TEXT, "Reply with the single word OK.");
    }
}
