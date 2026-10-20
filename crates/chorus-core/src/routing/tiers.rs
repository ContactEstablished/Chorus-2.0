//! The ranked tiers view, ported from Chorus 1's `routingCore.ts`: [`compute_tiers`] ranks a
//! snapshot for one profile and effort and returns the three ranked tiers, Nitro, the
//! explanation of every candidate, snapshot staleness (K7), the account state (K3), and the
//! warnings W1–W7 with their exact texts.

use std::fmt;

use super::js;
use super::payload::{build_nitro_selection, build_ranked_provider};
use super::ranker::rank_candidates;
use super::time::MINUTE_MS;
use super::types::{
    AccountCheck, CandidateExplanation, DataCollection, RankInput, RankedTier, TierResult, TierSelection, TierSelections,
};

/// W2.
pub const WARN_ACCOUNT_UNCHECKED: &str = "Account guardrails were not checked; a pinned endpoint may be refused.";
/// W3.
pub const WARN_DATA_POLICY_UNCHECKED: &str = "Data-policy removals were not checked.";
/// W7.
pub const WARN_NITRO_NONE: &str = "Nitro: no endpoint in the snapshot passes the capability filters.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TierError {
    /// The snapshot is dated after `now_ms` (C9): the clock moved backwards.
    SnapshotAfterNow,
    /// A selected tag had no candidate, throughput or cost. The ranker never produces one; it is
    /// reported rather than shown as a tier.
    Inconsistent(String),
}

impl fmt::Display for TierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SnapshotAfterNow => f.write_str("snapshot.fetchedAt is after now"),
            Self::Inconsistent(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for TierError {}

/// `<tag> first: <tps> effective tok/s, $<cost>/M blended; then <rest>` (or `; no fallbacks`),
/// plus ` (limited history)` when the tier has it.
fn rationale(primary: &CandidateExplanation, rest: &[String], limited_history: bool) -> Result<String, TierError> {
    let (Some(tps), Some(cost)) = (primary.effective_tps, primary.cost) else {
        return Err(TierError::Inconsistent(format!("Selected candidate {} has no effective throughput or cost", primary.tag)));
    };
    let then = if rest.is_empty() { "; no fallbacks".to_owned() } else { format!("; then {}", rest.join(", ")) };
    let limited = if limited_history { " (limited history)" } else { "" };
    Ok(format!("{} first: {} effective tok/s, ${}/M blended{then}{limited}", primary.tag, js::to_fixed(tps, 1), js::to_precision(cost.blended, 3)))
}

/// Ranks the snapshot and returns every tier, Nitro, each candidate's explanation and the
/// warnings. Errs when the snapshot was fetched after `now_ms` (C9). The settings and profile
/// are trusted: validate settings ([`super::types::RoutingSettings::validate`]) at the boundary.
pub fn compute_tiers(input: &RankInput<'_>) -> Result<TierResult, TierError> {
    let s = input.settings;

    // 1. Times (C9).
    let now_ms = input.now_ms;
    let fetched_ms = input.snapshot.fetched_at_ms;
    if fetched_ms > now_ms {
        return Err(TierError::SnapshotAfterNow);
    }

    // 2. Ranking.
    let ranked = rank_candidates(input);

    // 3. Snapshot age (K7). The ranker still computes when stale; launch refuses it.
    let age_ms = now_ms - fetched_ms;
    let snapshot_age_minutes = u32::try_from(age_ms.div_euclid(MINUTE_MS)).unwrap_or(u32::MAX);
    let stale = age_ms as f64 > s.snapshot_max_age_minutes * MINUTE_MS as f64;

    // 4. Account (K3): an unparsed guardrail preflight is never reported as checked.
    let account_eligibility = if input.account.guardrail_removed.is_none() { AccountCheck::Unknown } else { AccountCheck::Checked };

    // 5. Tier selections.
    let mut tiers = TierSelections::default();
    for tier in RankedTier::ALL {
        let tags = ranked.selections.get(tier);
        if tags.is_empty() {
            continue;
        }
        let selected = tags
            .iter()
            .map(|tag| {
                ranked.candidates.iter().find(|c| &c.tag == tag).ok_or_else(|| TierError::Inconsistent(format!("Selected tag {tag} has no candidate")))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let limited_history = selected.iter().any(|c| c.limited_history);
        let selection = TierSelection {
            tier,
            model: input.model.slug.clone(),
            provider: build_ranked_provider(&selected, s),
            endpoints: tags.to_vec(),
            limited_history,
            limited_fallbacks: tags.len() < 1 + s.fallback_count as usize,
            rationale: rationale(selected[0], &tags[1..], limited_history)?,
        };
        match tier {
            RankedTier::Budget => tiers.budget = Some(selection),
            RankedTier::Balanced => tiers.balanced = Some(selection),
            RankedTier::Fast => tiers.fast = Some(selection),
        }
    }

    // 6. Nitro: never ranked; `likely` is a labelled preview.
    let nitro = build_nitro_selection(&input.model.slug, ranked.nitro_likely.as_ref(), &ranked.nitro_fails_rules, s);

    // Warnings W1–W7, in order, each only when its condition holds.
    let mut warnings = Vec::new();
    if stale {
        warnings.push(format!(
            "Snapshot is {snapshot_age_minutes} minutes old (limit {}); refresh before launching.",
            js::number(s.snapshot_max_age_minutes)
        ));
    }
    if account_eligibility == AccountCheck::Unknown {
        warnings.push(WARN_ACCOUNT_UNCHECKED.to_owned());
    }
    if s.data_collection == DataCollection::Deny && input.account.data_policy_removed.is_none() {
        warnings.push(WARN_DATA_POLICY_UNCHECKED.to_owned());
    }
    for tier in RankedTier::ALL {
        let n = ranked.selections.get(tier).len();
        if n == 0 {
            warnings.push(format!("{}: no eligible endpoints.", tier.label()));
        } else if n < 1 + s.fallback_count as usize {
            warnings.push(format!("{}: only {n} eligible endpoint{}; limited fallbacks.", tier.label(), if n == 1 { "" } else { "s" }));
        }
    }
    let eligible: Vec<&CandidateExplanation> = ranked.candidates.iter().filter(|c| c.excluded_by.is_empty()).collect();
    let limited = eligible.iter().filter(|c| c.limited_history).count();
    if limited > 0 {
        warnings.push(format!(
            "Limited history: {limited} of {} eligible endpoints have fewer than {} speed observations.",
            eligible.len(),
            s.stable_min_observations
        ));
    }
    if nitro.likely.is_none() {
        warnings.push(WARN_NITRO_NONE.to_owned());
    }

    Ok(TierResult {
        model: input.model.slug.clone(),
        profile: input.profile,
        computed_at_ms: now_ms,
        snapshot_fetched_at_ms: fetched_ms,
        snapshot_age_minutes,
        stale,
        account_eligibility,
        median_eligible_tps: ranked.median_eligible_tps,
        budget_floor_tps: ranked.budget_floor_tps,
        tiers,
        nitro,
        candidates: ranked.candidates,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::endpoints::extract_observations;
    use crate::routing::test_support::*;
    use crate::routing::types::{
        AccountEligibility, EndpointSnapshot, OpenRouterProviderPrefs, ProviderDataCollection, Quantization, RawEndpoint, RoutingProfileId,
        RoutingSettings,
    };
    use serde_json::{Value, json};

    const LIMITED_14: &str = "Limited history: 14 of 14 eligible endpoints have fewer than 3 speed observations.";
    const FORBIDDEN: [&str; 4] = ["sort", "ignore", "only", "max_price"];
    const PROFILES: [RoutingProfileId; 2] = [RoutingProfileId::Interactive, RoutingProfileId::Helper];

    fn compute(inputs: &Inputs) -> TierResult {
        compute_tiers(&inputs.input()).unwrap()
    }

    fn golden(profile: RoutingProfileId) -> TierResult {
        compute(&Inputs::golden(profile))
    }

    fn with_settings(profile: RoutingProfileId, settings: RoutingSettings) -> Inputs {
        Inputs { settings, ..Inputs::golden(profile) }
    }

    fn allow() -> RoutingSettings {
        RoutingSettings { data_collection: DataCollection::Allow, ..RoutingSettings::default() }
    }

    fn at(now: &str) -> TierResult {
        compute(&Inputs { now_ms: ms(now), ..Inputs::golden(RoutingProfileId::Interactive) })
    }

    fn unknown_account() -> AccountEligibility {
        AccountEligibility { guardrail_removed: None, data_policy_removed: None, checked_at_ms: None }
    }

    fn candidate<'a>(result: &'a TierResult, tag: &str) -> &'a CandidateExplanation {
        result.candidates.iter().find(|c| c.tag == tag).unwrap_or_else(|| panic!("no candidate {tag}"))
    }

    fn orders(result: &TierResult) -> [Option<Vec<String>>; 3] {
        RankedTier::ALL.map(|t| result.tiers.get(t).map(|s| s.endpoints.clone()))
    }

    fn golden_orders(profile: RoutingProfileId) -> [Option<Vec<String>>; 3] {
        golden_selections(profile).map(|tags| Some(tags.map(String::from).to_vec()))
    }

    fn keys_deep(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Array(a) => a.iter().for_each(|x| keys_deep(x, out)),
            Value::Object(o) => o.iter().for_each(|(k, x)| {
                out.push(k.clone());
                keys_deep(x, out);
            }),
            _ => {}
        }
    }

    fn keys_of(v: &Value) -> Vec<String> {
        let mut out = Vec::new();
        keys_deep(v, &mut out);
        out
    }

    /// Every provider object, Nitro's included, that exists.
    fn providers(result: &TierResult) -> Vec<Value> {
        let mut out: Vec<Value> = RankedTier::ALL.iter().filter_map(|t| result.tiers.get(*t)).map(|s| s.provider.to_value()).collect();
        out.extend(result.nitro.provider.as_ref().map(OpenRouterProviderPrefs::to_value));
        out
    }

    fn excluded(result: &TierResult) -> Vec<(&str, Vec<&str>)> {
        result.candidates.iter().filter(|c| !c.excluded_by.is_empty()).map(|c| (c.tag.as_str(), c.excluded_by.iter().map(String::as_str).collect())).collect()
    }

    fn strings(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_owned()).collect()
    }

    /// The fixture with every eligible tag except `keep` removed (excluded rows stay).
    fn reduced(keep: &[&str]) -> Inputs {
        let mut inputs = Inputs::golden(RoutingProfileId::Interactive);
        inputs.snapshot.endpoints.retain(|r| !GOLDEN_ELIGIBLE.contains(&r.tag.as_str()) || keep.contains(&r.tag.as_str()));
        inputs.history = extract_observations(&inputs.snapshot);
        inputs
    }

    #[test]
    fn z1_golden_header_fields() {
        for profile in PROFILES {
            let result = golden(profile);
            let keys: Vec<String> = serde_json::to_value(&result).unwrap().as_object().unwrap().keys().cloned().collect();
            let mut expected = strings(&[
                "model", "profile", "computedAtMs", "snapshotFetchedAtMs", "snapshotAgeMinutes", "stale", "accountEligibility", "medianEligibleTps",
                "budgetFloorTps", "tiers", "nitro", "candidates", "warnings",
            ]);
            expected.sort();
            assert_eq!(keys, expected);
            assert_eq!((result.model.as_str(), result.profile), (SLUG, profile));
            assert_eq!((result.computed_at_ms, result.snapshot_fetched_at_ms), (ms(NOW), ms(FETCHED_AT)));
            assert_eq!((result.snapshot_age_minutes, result.stale, result.account_eligibility), (15, false, AccountCheck::Checked));
            close(result.median_eligible_tps, 90.5);
            close(result.budget_floor_tps, 45.25);
        }
    }

    #[test]
    fn z1_golden_32_candidates_14_eligible_18_excluded() {
        for profile in PROFILES {
            let result = golden(profile);
            assert_eq!(result.candidates.len(), 32);
            assert_eq!(result.candidates.iter().filter(|c| c.excluded_by.is_empty()).map(|c| c.tag.as_str()).collect::<Vec<_>>(), GOLDEN_ELIGIBLE);
            assert_eq!(excluded(&result), golden_exclusions());
        }
    }

    #[test]
    fn z1_golden_tier_orders_and_payloads() {
        for profile in PROFILES {
            let result = golden(profile);
            assert_eq!(orders(&result), golden_orders(profile));
            for (i, tier) in RankedTier::ALL.into_iter().enumerate() {
                let s = result.tiers.get(tier).unwrap();
                let keys: Vec<String> = serde_json::to_value(s).unwrap().as_object().unwrap().keys().cloned().collect();
                let mut expected = strings(&["tier", "model", "provider", "endpoints", "limitedHistory", "limitedFallbacks", "rationale"]);
                expected.sort();
                assert_eq!(keys, expected);
                assert_eq!((s.tier, s.model.as_str()), (tier, SLUG));
                let order = golden_selections(profile)[i];
                assert_eq!(
                    serde_json::to_string(&s.provider).unwrap(),
                    format!(
                        r#"{{"order":{},"allow_fallbacks":false,"require_parameters":true,"quantizations":["fp8"],"data_collection":"deny"}}"#,
                        serde_json::to_string(&order).unwrap()
                    )
                );
                assert!(s.limited_history && !s.limited_fallbacks, "{tier:?}");
            }
        }
    }

    #[test]
    fn z1_golden_nitro_and_warnings() {
        for profile in PROFILES {
            let result = golden(profile);
            assert_eq!(
                serde_json::to_value(&result.nitro).unwrap(),
                json!({
                    "model": "deepseek/deepseek-v4.1-flash:nitro",
                    "provider": { "data_collection": "deny" },
                    "likely": { "tag": "together", "providerName": "Together", "tpsP50": 223.0 },
                    "likelyFailsRules": ["quantization not declared"]
                })
            );
            assert_eq!(result.warnings, [LIMITED_14]);
        }
    }

    #[test]
    fn z1_interactive_budget_in_full_and_the_fast_rationale() {
        let interactive = golden(RoutingProfileId::Interactive);
        let order = strings(&["deepinfra/fp8", "streamlake/fp8", "gmicloud/fp8"]);
        assert_eq!(
            interactive.tiers.budget,
            Some(TierSelection {
                tier: RankedTier::Budget,
                model: SLUG.into(),
                provider: OpenRouterProviderPrefs {
                    order: Some(order.clone()),
                    allow_fallbacks: Some(false),
                    require_parameters: Some(true),
                    quantizations: Some(vec![Quantization::Fp8]),
                    data_collection: Some(ProviderDataCollection::Deny),
                },
                endpoints: order,
                limited_history: true,
                limited_fallbacks: false,
                rationale: "deepinfra/fp8 first: 48.5 effective tok/s, $0.0165/M blended; then streamlake/fp8, gmicloud/fp8 (limited history)".into(),
            })
        );
        assert_eq!(
            interactive.tiers.fast.unwrap().rationale,
            "venice/fp8 first: 119.9 effective tok/s, $0.0449/M blended; then baidu/fp8, parasail/fp8 (limited history)"
        );
    }

    #[test]
    fn z1_spot_values() {
        let i = golden(RoutingProfileId::Interactive);
        let h = golden(RoutingProfileId::Helper);
        close(candidate(&i, "deepinfra/fp8").scores.budget, 4.0367);
        close(candidate(&i, "streamlake/fp8").scores.budget, 4.03282);
        close(candidate(&i, "streamlake/fp8").scores.balanced, 3.99985);
        close(candidate(&i, "deepinfra/fp8").scores.balanced, 3.99215);
        close(candidate(&i, "venice/fp8").scores.fast, 4.78673);
        close(candidate(&i, "venice/fp8").effective_tps, 119.909);
        close(candidate(&i, "deepinfra/fp8").effective_tps, 48.4621);
        close(candidate(&i, "baidu/fp8").effective_tps, 89.7453);
        let blended = |tag: &str| candidate(&i, tag).cost.map(|c| c.blended);
        close(blended("deepinfra/fp8"), 0.0165144);
        close(blended("streamlake/fp8"), 0.0168692);
        close(blended("gmicloud/fp8"), 0.0215352);
        close(blended("baidu/fp8"), 0.30959);
        close(blended("venice/fp8"), 0.044865);
        assert!(!candidate(&i, "baidu/fp8").cost.unwrap().cache_credit);
        close(candidate(&h, "streamlake/fp8").scores.budget, 4.36728);
        close(candidate(&h, "deepinfra/fp8").scores.budget, 4.31882);
        close(candidate(&h, "venice/fp8").scores.balanced, 4.23081);
        close(candidate(&h, "gmicloud/fp8").scores.balanced, 4.22583);
    }

    #[test]
    fn z3_no_live_sort_or_preference_in_any_provider_object_and_deny_by_default() {
        for profile in PROFILES {
            for result in [golden(profile), compute(&with_settings(profile, allow()))] {
                let all = providers(&result);
                assert!(!all.is_empty());
                for key in all.iter().flat_map(keys_of) {
                    assert!(key != "sort" && !key.starts_with("preferred_") && !FORBIDDEN.contains(&key.as_str()), "{key}");
                }
            }
            let all = providers(&golden(profile));
            assert_eq!(all.len(), 4);
            assert!(all.iter().all(|p| p["data_collection"] == "deny"), "every tier, Nitro included, denies by default (MR-D11)");
        }
    }

    #[test]
    fn z4_allow_keeps_the_orders_and_sends_no_data_collection_anywhere() {
        for profile in PROFILES {
            let result = compute(&with_settings(profile, allow()));
            assert_eq!(orders(&result), golden_orders(profile));
            assert!(!keys_of(&serde_json::to_value(&result).unwrap()).contains(&"data_collection".to_owned()));
            for tier in RankedTier::ALL {
                let provider = &result.tiers.get(tier).unwrap().provider;
                let text = serde_json::to_string(provider).unwrap();
                assert!(text.starts_with(r#"{"order":"#) && text.ends_with(r#""allow_fallbacks":false,"require_parameters":true,"quantizations":["fp8"]}"#), "{text}");
                let mut keys = keys_of(&provider.to_value());
                keys.sort();
                assert_eq!(keys, ["allow_fallbacks", "order", "quantizations", "require_parameters"]);
            }
            assert_eq!(result.nitro.provider, None);
            assert_eq!(result.nitro.model, "deepseek/deepseek-v4.1-flash:nitro");
            assert_eq!(result.warnings, [LIMITED_14]);
        }
    }

    #[test]
    fn z4_w3_needs_deny() {
        for profile in PROFILES {
            let mut unchecked_allow = with_settings(profile, allow());
            unchecked_allow.account.data_policy_removed = None;
            assert_eq!(compute(&unchecked_allow).warnings, [LIMITED_14]);
            let mut unchecked_deny = Inputs::golden(profile);
            unchecked_deny.account.data_policy_removed = None;
            let denied = compute(&unchecked_deny);
            assert_eq!(denied.account_eligibility, AccountCheck::Checked);
            assert_eq!(denied.warnings, [WARN_DATA_POLICY_UNCHECKED, LIMITED_14]);
        }
    }

    #[test]
    fn z5_61_minutes_is_stale_with_w1_first_and_tiers_still_computed() {
        let stale = at("2026-10-02T10:06:00Z");
        assert_eq!((stale.snapshot_age_minutes, stale.stale, stale.computed_at_ms), (61, true, ms("2026-10-02T10:06:00Z")));
        assert_eq!(stale.warnings, ["Snapshot is 61 minutes old (limit 60); refresh before launching.", LIMITED_14]);
        assert_eq!(orders(&stale), golden_orders(RoutingProfileId::Interactive));
    }

    #[test]
    fn z5_60_minutes_is_not_stale_and_one_millisecond_more_is() {
        let fresh = at("2026-10-02T10:05:00Z");
        assert_eq!((fresh.snapshot_age_minutes, fresh.stale), (60, false));
        assert_eq!(fresh.warnings, [LIMITED_14]);
        let just_over = at("2026-10-02T10:05:00.001Z");
        assert_eq!((just_over.snapshot_age_minutes, just_over.stale), (60, true));
        assert_eq!(just_over.warnings[0], "Snapshot is 60 minutes old (limit 60); refresh before launching.");
        let same = at("2026-10-02T09:05:00.000Z");
        assert_eq!((same.snapshot_age_minutes, same.stale), (0, false));
    }

    #[test]
    fn z6_unparsed_preflights_are_unknown_with_w2_and_w3_and_tiers_still_computed() {
        for profile in PROFILES {
            let result = compute(&Inputs { account: unknown_account(), ..Inputs::golden(profile) });
            assert_eq!(result.account_eligibility, AccountCheck::Unknown);
            assert_eq!(result.warnings, [WARN_ACCOUNT_UNCHECKED, WARN_DATA_POLICY_UNCHECKED, LIMITED_14]);
            assert_eq!(candidate(&result, "deepseek").excluded_by, ["uptime 99.34% < 99.5%"]);
            assert_eq!(orders(&result), golden_orders(profile));
        }
    }

    #[test]
    fn z7_with_min_uptime_100_nothing_is_eligible() {
        let none = compute(&with_settings(
            RoutingProfileId::Interactive,
            RoutingSettings { min_uptime_pct: 100.0, readmit_uptime_pct: 100.0, ..RoutingSettings::default() },
        ));
        assert_eq!(none.tiers, TierSelections::default());
        assert_eq!(none.warnings, ["Budget: no eligible endpoints.", "Balanced: no eligible endpoints.", "Fast: no eligible endpoints."]);
        assert_eq!((none.median_eligible_tps, none.budget_floor_tps), (None, None));
        let likely = none.nitro.likely.unwrap();
        assert_eq!((likely.tag.as_str(), likely.provider_name.as_str(), likely.tps_p50), ("together", "Together", 223.0));
        assert_eq!(none.nitro.likely_fails_rules, ["uptime 99.96% < 100%", "quantization not declared"]);
        assert_eq!(none.nitro.provider.map(|p| p.to_value()), Some(json!({ "data_collection": "deny" })));
    }

    #[test]
    fn z8_two_eligible_tags_give_limited_fallbacks_and_w5_per_tier() {
        let two = compute(&reduced(&["deepinfra/fp8", "streamlake/fp8"]));
        assert_eq!(two.candidates.iter().filter(|c| c.excluded_by.is_empty()).map(|c| c.tag.as_str()).collect::<Vec<_>>(), ["deepinfra/fp8", "streamlake/fp8"]);
        for tier in RankedTier::ALL {
            let s = two.tiers.get(tier).unwrap();
            let mut sorted = s.endpoints.clone();
            sorted.sort();
            assert_eq!(sorted, ["deepinfra/fp8", "streamlake/fp8"]);
            assert_eq!(s.provider.order.as_ref(), Some(&s.endpoints));
            assert!(s.limited_fallbacks);
        }
        assert_eq!(
            two.warnings,
            [
                "Budget: only 2 eligible endpoints; limited fallbacks.",
                "Balanced: only 2 eligible endpoints; limited fallbacks.",
                "Fast: only 2 eligible endpoints; limited fallbacks.",
                "Limited history: 2 of 2 eligible endpoints have fewer than 3 speed observations.",
            ]
        );
        assert_round_trip(&two);
    }

    #[test]
    fn z8_one_eligible_tag_gives_the_singular_w5_and_no_fallbacks() {
        let one = compute(&reduced(&["venice/fp8"]));
        let venice = Some(strings(&["venice/fp8"]));
        assert_eq!(orders(&one), [venice.clone(), venice.clone(), venice]);
        let fast = one.tiers.fast.clone().unwrap();
        assert!(fast.limited_fallbacks);
        assert_eq!(fast.rationale, "venice/fp8 first: 119.9 effective tok/s, $0.0449/M blended; no fallbacks (limited history)");
        assert_eq!(
            one.warnings,
            [
                "Budget: only 1 eligible endpoint; limited fallbacks.",
                "Balanced: only 1 eligible endpoint; limited fallbacks.",
                "Fast: only 1 eligible endpoint; limited fallbacks.",
                "Limited history: 1 of 1 eligible endpoints have fewer than 3 speed observations.",
            ]
        );
        let mut no_fallbacks = reduced(&["venice/fp8"]);
        no_fallbacks.settings.fallback_count = 0;
        let result = compute(&no_fallbacks);
        assert!(!result.tiers.fast.unwrap().limited_fallbacks);
        assert_eq!(result.warnings, ["Limited history: 1 of 1 eligible endpoints have fewer than 3 speed observations."]);
    }

    #[test]
    fn z9_a_snapshot_fetched_after_now_is_refused() {
        let mut inputs = Inputs::golden(RoutingProfileId::Interactive);
        inputs.snapshot.fetched_at_ms = ms("2026-10-02T09:21:00Z");
        let err = compute_tiers(&inputs.input()).unwrap_err();
        assert_eq!(err, TierError::SnapshotAfterNow);
        assert_eq!(err.to_string(), "snapshot.fetchedAt is after now");
        // Chorus 1 also refused times that weren't UTC ISO instants. 2.0 takes milliseconds; the
        // text rules live in time::parse_utc_instant and are tested there.
        inputs.snapshot.fetched_at_ms = inputs.now_ms;
        assert_eq!(compute(&inputs).snapshot_age_minutes, 0);
    }

    #[test]
    fn z10_reversed_endpoints_and_shuffled_history_give_an_equal_result() {
        for profile in PROFILES {
            let base = Inputs::golden(profile);
            let expected = compute(&base);
            for seed in [1, 7, 42] {
                let mut shuffled_input = base.clone();
                shuffled_input.snapshot.endpoints.reverse();
                shuffled_input.history = shuffled(&base.history, seed);
                assert_eq!(compute(&shuffled_input), expected, "seed {seed}");
            }
        }
    }

    #[test]
    fn z11_a_first_party_unknown_at_the_top_of_budget_sends_its_declared_unknown() {
        let fast = |tag: &str| RawEndpoint { throughput_last_30m: pct(Some(200.0), Some(250.0)), ..base_row(tag) };
        let result = compute(&Inputs::synthetic(vec![
            fast("a/fp8"),
            fast("b/fp8"),
            fast("c/fp8"),
            RawEndpoint {
                provider_name: "DeepSeek".into(),
                quantization: None,
                throughput_last_30m: pct(Some(150.0), Some(200.0)),
                pricing: pricing("0.00000001", "0.00000004", Some("0.000000001")),
                ..base_row("deepseek")
            },
        ]));
        let deepseek = candidate(&result, "deepseek");
        assert!(deepseek.excluded_by.is_empty());
        assert_eq!((deepseek.row_quantizations.clone(), deepseek.effective_quantization), (vec![Quantization::Unknown], Quantization::Fp8));
        let budget = result.tiers.budget.clone().unwrap();
        assert_eq!(budget.endpoints[0], "deepseek");
        assert_eq!(budget.provider.quantizations, Some(vec![Quantization::Fp8, Quantization::Unknown]));
        let fast_tier = result.tiers.fast.clone().unwrap();
        assert_eq!(fast_tier.endpoints, ["a/fp8", "b/fp8", "c/fp8"]);
        assert_eq!(fast_tier.provider.quantizations, Some(vec![Quantization::Fp8]), "a tier that doesn't select it filters on fp8 only");
        assert_round_trip(&result);
    }

    #[test]
    fn templates_a_tier_with_stable_history_has_no_limited_suffix_and_no_w6() {
        let rows: Vec<RawEndpoint> = [("a/fp8", 100.0), ("b/fp8", 150.0), ("c/fp8", 200.0)]
            .into_iter()
            .map(|(tag, p50)| RawEndpoint { throughput_last_30m: pct(Some(p50), Some(p50)), ..base_row(tag) })
            .collect();
        // Three observations per tag: the snapshot plus two earlier ones.
        let history = ["2026-10-02T09:05:00Z", "2026-10-02T08:35:00Z", "2026-10-02T08:05:00Z"]
            .into_iter()
            .flat_map(|at| extract_observations(&EndpointSnapshot { fetched_at_ms: ms(at), endpoints: rows.clone() }))
            .collect();
        let result = compute(&Inputs { history, ..Inputs::synthetic(rows) });
        assert!(result.candidates.iter().all(|c| !c.limited_history && c.observations == 3));
        // c: 300 / (0.5 + 300 / 200) = 150 tok/s; 0.057 × 0.1 + 0.932 × 0.01 + 0.011 × 0.4 = $0.01942/M.
        let fast = result.tiers.fast.unwrap();
        assert_eq!(fast.rationale, "c/fp8 first: 150.0 effective tok/s, $0.0194/M blended; then b/fp8, a/fp8");
        assert!(!fast.limited_history);
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn templates_w1_w2_w3_w4_per_tier_and_w7_in_order() {
        let no_tools = RawEndpoint { supported_parameters: strings(&["tool_choice", "max_tokens", "reasoning"]), ..base_row("a/fp8") };
        let result = compute(&Inputs { account: unknown_account(), now_ms: ms("2026-10-02T11:05:00Z"), ..Inputs::synthetic(vec![no_tools]) });
        assert_eq!((result.nitro.likely.clone(), result.nitro.likely_fails_rules.len()), (None, 0));
        assert_eq!(
            result.warnings,
            [
                "Snapshot is 120 minutes old (limit 60); refresh before launching.",
                WARN_ACCOUNT_UNCHECKED,
                WARN_DATA_POLICY_UNCHECKED,
                "Budget: no eligible endpoints.",
                "Balanced: no eligible endpoints.",
                "Fast: no eligible endpoints.",
                WARN_NITRO_NONE,
            ]
        );
    }

    #[test]
    fn templates_w1_w2_w3_w5_per_tier_and_w6_in_order() {
        let result = compute(&Inputs { account: unknown_account(), now_ms: ms("2026-10-02T11:05:00Z"), ..Inputs::synthetic(vec![base_row("a/fp8")]) });
        assert_eq!(
            result.warnings,
            [
                "Snapshot is 120 minutes old (limit 60); refresh before launching.",
                WARN_ACCOUNT_UNCHECKED,
                WARN_DATA_POLICY_UNCHECKED,
                "Budget: only 1 eligible endpoint; limited fallbacks.",
                "Balanced: only 1 eligible endpoint; limited fallbacks.",
                "Fast: only 1 eligible endpoint; limited fallbacks.",
                "Limited history: 1 of 1 eligible endpoints have fewer than 3 speed observations.",
            ]
        );
    }

    #[test]
    fn z2_every_result_survives_a_json_round_trip() {
        let results = [
            golden(RoutingProfileId::Interactive),
            golden(RoutingProfileId::Helper),
            compute(&with_settings(RoutingProfileId::Interactive, allow())),
            at("2026-10-02T10:06:00Z"),
            at("2026-10-02T10:05:00Z"),
            compute(&Inputs { account: unknown_account(), ..Inputs::golden(RoutingProfileId::Interactive) }),
            compute(&Inputs { account: unknown_account(), ..Inputs::golden(RoutingProfileId::Helper) }),
            compute(&with_settings(
                RoutingProfileId::Interactive,
                RoutingSettings { min_uptime_pct: 100.0, readmit_uptime_pct: 100.0, ..RoutingSettings::default() },
            )),
        ];
        for r in results {
            assert_round_trip(&r);
        }
    }
}
