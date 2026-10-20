//! Ranking, ported from Chorus 1's `rankerCore.ts`: effective throughput (MR-D6), tier scores,
//! the Budget floor (K10), the deterministic K9 tie-break, and [`rank_candidates`], which explains
//! every candidate. The result never depends on the order of the snapshot's rows or the history.

use std::cmp::Ordering;

use super::eligibility::{GateContext, GateFamily, GateResult, SmoothedSpeed, evaluate_gates, median, merge_history, select_window, smooth_speed};
use super::endpoints::{CollapsedEndpoint, collapse_endpoints, extract_observations};
use super::js::{self, by_code_unit};
use super::pricing::{blended_cost, collapse_pricing, resolve_pricing};
use super::types::{CandidateCost, CandidateExplanation, NitroLikely, RankInput, RankedTier, RoutingSettings, TierScores};

/// MR-D6: tokens per second over a whole turn of `n` output tokens, first-token latency included.
pub fn effective_tps(n: f64, latency_s: f64, tps_p50: f64) -> f64 {
    n / (latency_s + n / tps_p50)
}

/// `weight·ln(tps_eff) − (1 − weight)·ln(blended)`; never -0.
pub fn tier_score(weight: f64, tps_eff: f64, blended: f64) -> f64 {
    weight * tps_eff.ln() - (1.0 - weight) * blended.ln() + 0.0
}

/// K10: `max(budget_min_tps, budget_median_fraction × median)`; `None` for no eligible speeds.
pub fn budget_floor(eligible_tps: &[f64], settings: &RoutingSettings) -> Option<f64> {
    median(eligible_tps).map(|m| js::max(settings.budget_min_tps, settings.budget_median_fraction * m) + 0.0)
}

/// What the K9 order reads of a scored candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderEntry {
    pub tag: String,
    pub score: f64,
    pub uptime_1d: f64,
    pub latency_p50_s: f64,
    pub blended: f64,
}

fn desc(a: f64, b: f64) -> Ordering {
    b.partial_cmp(&a).unwrap_or(Ordering::Equal)
}

fn asc(a: f64, b: f64) -> Ordering {
    a.partial_cmp(&b).unwrap_or(Ordering::Equal)
}

/// Higher uptime, lower latency, lower cost, then tag: lexicographic, hence transitive.
fn by_cluster_keys(a: &OrderEntry, b: &OrderEntry) -> Ordering {
    desc(a.uptime_1d, b.uptime_1d)
        .then_with(|| asc(a.latency_p50_s, b.latency_p50_s))
        .then_with(|| asc(a.blended, b.blended))
        .then_with(|| by_code_unit(&a.tag, &b.tag))
}

/// K9. Sort by score, highest first (equal scores by tag). Repeatedly take the highest remaining
/// score `s`; its cluster is every remaining entry scoring at least `s − ln(tie_ratio)`; order the
/// cluster by uptime, latency and cost, append it, and continue. Clusters are anchored on the
/// highest remaining score, never chained, so the output never depends on the input order.
pub fn order_by_score(entries: &[OrderEntry], tie_ratio: f64) -> Vec<OrderEntry> {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| desc(a.score, b.score).then_with(|| by_code_unit(&a.tag, &b.tag)));
    let margin = tie_ratio.ln();
    let mut out = Vec::with_capacity(sorted.len());
    let mut start = 0;
    while start < sorted.len() {
        let threshold = sorted[start].score - margin;
        let end = start + 1 + sorted[start + 1..].iter().take_while(|e| e.score >= threshold).count();
        let mut cluster = sorted[start..end].to_vec();
        cluster.sort_by(by_cluster_keys);
        out.extend(cluster);
        start = end;
    }
    out
}

/// Each tier's selected tags, in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TierOrders {
    pub budget: Vec<String>,
    pub balanced: Vec<String>,
    pub fast: Vec<String>,
}

impl TierOrders {
    pub fn get(&self, tier: RankedTier) -> &[String] {
        match tier {
            RankedTier::Budget => &self.budget,
            RankedTier::Balanced => &self.balanced,
            RankedTier::Fast => &self.fast,
        }
    }

    fn set(&mut self, tier: RankedTier, tags: Vec<String>) {
        match tier {
            RankedTier::Budget => self.budget = tags,
            RankedTier::Balanced => self.balanced = tags,
            RankedTier::Fast => self.fast = tags,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RankedCandidates {
    /// Sorted by tag.
    pub candidates: Vec<CandidateExplanation>,
    /// At most `1 + fallback_count` tags per tier.
    pub selections: TierOrders,
    pub median_eligible_tps: Option<f64>,
    pub budget_floor_tps: Option<f64>,
    pub nitro_likely: Option<NitroLikely>,
    pub nitro_fails_rules: Vec<String>,
}

/// An eligible candidate's ranking inputs. Passing rules 1, 12 and 13 means its uptime, both
/// smoothed speeds and its cost exist.
struct Eligible {
    uptime_1d: f64,
    tps_p50: f64,
    latency_p50_s: f64,
    blended: f64,
    tps_eff: f64,
    budget_floor_excluded: bool,
    scores: TierScores,
}

struct Evaluated {
    c: CollapsedEndpoint,
    speed: SmoothedSpeed,
    cost: Option<CandidateCost>,
    gates: GateResult,
    tps_eff: Option<f64>,
    eligible: Option<Eligible>,
}

/// Ranks every routable tag of the latest snapshot (Chorus 1's steps 1–7): gates and smoothed
/// speeds per candidate, the eligible median and Budget floor, the scores per tier, K9 selections,
/// and Nitro's likely endpoint (only the account and capability filters apply to it; the
/// reliability and precision rules it fails are listed).
pub fn rank_candidates(input: &RankInput<'_>) -> RankedCandidates {
    let settings = input.settings;
    let now_ms = input.now_ms;
    let profile = input.profile.profile();
    let ctx = GateContext { model: input.model, profile, effort: input.effort, account: input.account, settings, now_ms };

    // 1. Candidates and the merged history.
    let collapsed = collapse_endpoints(&input.snapshot.endpoints);
    let history = merge_history(input.history, &extract_observations(input.snapshot));

    // 2. Window, smoothed speed, cost and gates per candidate.
    let mut evaluated: Vec<Evaluated> = collapsed
        .into_iter()
        .map(|c| {
            let window = select_window(&history, &c.tag, now_ms, settings);
            let speed = smooth_speed(&window, settings);
            let resolved: Vec<_> = c.pricing.iter().map(|p| resolve_pricing(p, now_ms, profile.typical_prompt_tokens)).collect();
            // Chorus 1 credits any stored `verified: true` record, however old: the 14-day
            // freshness (cache_probe::CACHE_PROBE_MAX_AGE_MS) only decides when to probe again.
            let cache_verified = input.cache.get(&c.tag).is_some_and(|r| r.verified);
            let cost = blended_cost(&collapse_pricing(&resolved), &profile.token_shares, cache_verified);
            let gates = evaluate_gates(&c, &window, &speed, cost.as_ref(), &ctx);
            let tps_eff = speed.tps_p50.zip(speed.latency_p50_s).map(|(tps, lat)| effective_tps(f64::from(profile.expected_output_tokens), lat, tps));
            let eligible = match (gates.reasons.is_empty(), c.uptime_1d, speed.tps_p50, speed.latency_p50_s, cost, tps_eff) {
                (true, Some(uptime_1d), Some(tps_p50), Some(latency_p50_s), Some(cost), Some(tps_eff)) => Some(Eligible {
                    uptime_1d,
                    tps_p50,
                    latency_p50_s,
                    blended: cost.blended,
                    tps_eff,
                    budget_floor_excluded: false,
                    scores: TierScores::default(),
                }),
                _ => None,
            };
            Evaluated { c, speed, cost, gates, tps_eff, eligible }
        })
        .collect();

    // 3. The eligible set, its median speed and the Budget floor (K10).
    let eligible_tps: Vec<f64> = evaluated.iter().filter_map(|e| e.eligible.as_ref().map(|r| r.tps_p50)).collect();
    let median_eligible_tps = median(&eligible_tps);
    let budget_floor_tps = budget_floor(&eligible_tps, settings);

    // 4. Scores (the floor is None only when nothing is eligible). Equal to the floor passes.
    if let Some(floor) = budget_floor_tps {
        for r in evaluated.iter_mut().filter_map(|e| e.eligible.as_mut()) {
            r.budget_floor_excluded = r.tps_p50 < floor;
            for tier in RankedTier::ALL {
                if tier == RankedTier::Budget && r.budget_floor_excluded {
                    continue;
                }
                r.scores.set(tier, tier_score(settings.tier_weights.get(tier), r.tps_eff, r.blended));
            }
        }
    }

    // 5. K9 selections per tier.
    let mut selections = TierOrders::default();
    for tier in RankedTier::ALL {
        let entries: Vec<OrderEntry> = evaluated
            .iter()
            .filter_map(|e| {
                let r = e.eligible.as_ref()?;
                Some(OrderEntry { tag: e.c.tag.clone(), score: r.scores.get(tier)?, uptime_1d: r.uptime_1d, latency_p50_s: r.latency_p50_s, blended: r.blended })
            })
            .collect();
        let tags = order_by_score(&entries, settings.tie_ratio).into_iter().take(1 + settings.fallback_count as usize).map(|e| e.tag).collect();
        selections.set(tier, tags);
    }

    // 6. Nitro likely (K12, C3): passes the account and capability filters, has a smoothed speed.
    let filtered_out = |e: &Evaluated| e.gates.reasons.iter().any(|r| matches!(r.family, GateFamily::Account | GateFamily::Capability));
    let mut nitro: Option<(&Evaluated, f64)> = None;
    for e in &evaluated {
        let Some(tps) = e.speed.tps_p50 else { continue };
        if filtered_out(e) {
            continue;
        }
        let better = match nitro {
            None => true,
            Some((best, best_tps)) => tps > best_tps || (tps == best_tps && by_code_unit(&e.c.tag, &best.c.tag) == Ordering::Less),
        };
        if better {
            nitro = Some((e, tps));
        }
    }
    let nitro_likely = nitro.map(|(e, tps)| NitroLikely { tag: e.c.tag.clone(), provider_name: e.c.provider_name.clone(), tps_p50: tps });
    let nitro_fails_rules = nitro.map_or_else(Vec::new, |(e, _)| {
        e.gates.reasons.iter().filter(|r| matches!(r.family, GateFamily::Reliability | GateFamily::Precision)).map(|r| r.text.clone()).collect()
    });

    // 7. Explanations, sorted by tag.
    let mut candidates: Vec<CandidateExplanation> = evaluated
        .into_iter()
        .map(|e| CandidateExplanation {
            tag: e.c.tag,
            provider_name: e.c.provider_name,
            rows: e.c.rows,
            quantization: e.c.quantization,
            row_quantizations: e.c.row_quantizations,
            effective_quantization: e.gates.effective_quantization,
            uptime_1d: e.c.uptime_1d,
            uptime_5m: e.c.uptime_5m,
            status: e.c.status,
            observations: e.speed.observations,
            limited_history: e.speed.limited_history,
            tps_p50: e.speed.tps_p50,
            latency_p50_s: e.speed.latency_p50_s,
            tps_p90: e.speed.tps_p90,
            latency_p90_s: e.speed.latency_p90_s,
            effective_tps: e.tps_eff,
            cost: e.cost,
            excluded_by: e.gates.reasons.into_iter().map(|r| r.text).collect(),
            budget_floor_excluded: e.eligible.as_ref().is_some_and(|r| r.budget_floor_excluded),
            scores: e.eligible.map(|r| r.scores).unwrap_or_default(),
        })
        .collect();
    candidates.sort_by(|a, b| by_code_unit(&a.tag, &b.tag));

    RankedCandidates { candidates, selections, median_eligible_tps, budget_floor_tps, nitro_likely, nitro_fails_rules }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::test_support::*;
    use crate::routing::time::MINUTE_MS;
    use crate::routing::types::{
        AccountEligibility, CacheRecord, DataCollection, RawEndpoint, RoutingObservation, RoutingProfileId, RoutingSettings,
    };

    fn tie() -> f64 {
        RoutingSettings::default().tie_ratio.ln()
    }

    fn row(tag: &str) -> RawEndpoint {
        base_row(tag)
    }

    fn tps(tag: &str, p50: f64, p90: f64) -> RawEndpoint {
        RawEndpoint { throughput_last_30m: pct(Some(p50), Some(p90)), ..row(tag) }
    }

    fn rank(inputs: &Inputs) -> RankedCandidates {
        rank_candidates(&inputs.input())
    }

    fn candidate<'a>(result: &'a RankedCandidates, tag: &str) -> &'a CandidateExplanation {
        result.candidates.iter().find(|c| c.tag == tag).unwrap_or_else(|| panic!("no candidate {tag}"))
    }

    fn entry(tag: &str, score: f64, uptime_1d: f64) -> OrderEntry {
        entry_full(tag, score, uptime_1d, 1.0, 0.02)
    }

    fn entry_full(tag: &str, score: f64, uptime_1d: f64, latency_p50_s: f64, blended: f64) -> OrderEntry {
        OrderEntry { tag: tag.into(), score, uptime_1d, latency_p50_s, blended }
    }

    fn tags(entries: &[OrderEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.tag.as_str()).collect()
    }

    #[test]
    fn k_a_effective_throughput() {
        close(Some(effective_tps(300.0, 1.4285, 63.0)), 48.4621);
    }

    #[test]
    fn k_b_at_weight_0_3_ten_percent_pricier_and_twice_as_fast_wins() {
        let a = tier_score(0.3, 50.0, 1.0);
        let b = tier_score(0.3, 100.0, 1.1);
        close(Some(a), 1.17361);
        close(Some(b), 1.31483);
        assert!(b > a);
    }

    #[test]
    fn k_c_at_weight_0_3_twice_as_pricey_and_twice_as_fast_loses() {
        let a = tier_score(0.3, 50.0, 1.0);
        let b = tier_score(0.3, 100.0, 2.0);
        close(Some(b), 0.89635);
        assert!(a > b);
    }

    #[test]
    fn tier_score_never_returns_negative_zero() {
        assert!(tier_score(0.0, 0.5, 1.0).is_sign_positive() && tier_score(0.0, 0.5, 1.0) == 0.0);
        assert!(tier_score(1.0, 1.0, 0.5).is_sign_positive() && tier_score(1.0, 1.0, 0.5) == 0.0);
    }

    #[test]
    fn budget_floor_is_the_larger_of_the_minimum_and_the_median_fraction() {
        let s = RoutingSettings::default();
        assert_eq!(budget_floor(&[], &s), None);
        assert_eq!(budget_floor(&[17.0, 80.0, 90.0, 100.0, 110.0], &s), Some(45.0));
        assert_eq!(budget_floor(&[20.0, 40.0], &s), Some(30.0));
        assert_eq!(budget_floor(&[62.0, 63.0, 67.0, 88.0], &s), Some(32.5));
        let negative_zero = RoutingSettings { budget_min_tps: -0.0, budget_median_fraction: -0.0, ..s };
        assert!(budget_floor(&[10.0], &negative_zero).unwrap().is_sign_positive());
    }

    #[test]
    fn k_d_a_cheap_17_tps_candidate_is_floor_excluded_but_scored_for_balanced_and_fast() {
        let cheap = RawEndpoint { pricing: pricing("0.00000001", "0.00000004", Some("0.000000001")), ..tps("cheap/fp8", 17.0, 30.0) };
        let mut rows = vec![cheap];
        rows.extend([80.0, 90.0, 100.0, 110.0].into_iter().enumerate().map(|(i, t)| tps(&format!("p{i}/fp8"), t, t)));
        let result = rank(&Inputs::synthetic(rows));
        assert_eq!(result.median_eligible_tps, Some(90.0));
        assert_eq!(result.budget_floor_tps, Some(45.0));
        let c = candidate(&result, "cheap/fp8");
        assert!(c.excluded_by.is_empty());
        assert!(c.budget_floor_excluded);
        assert_eq!(c.scores.budget, None);
        assert!(c.scores.balanced.is_some() && c.scores.fast.is_some());
        assert!(!result.selections.budget.contains(&"cheap/fp8".to_owned()));
        assert_eq!(result.selections.budget.len(), 3);
        // Without the floor it would have won Budget: the floor is what removes it.
        let unfloored = tier_score(0.3, c.effective_tps.unwrap(), c.cost.unwrap().blended);
        for other in result.candidates.iter().filter_map(|x| x.scores.budget) {
            assert!(unfloored > other);
        }
        assert_eq!(result.candidates.iter().filter(|x| x.budget_floor_excluded).map(|x| x.tag.as_str()).collect::<Vec<_>>(), ["cheap/fp8"]);
    }

    #[test]
    fn k_e_a_candidate_exactly_at_the_floor_is_not_excluded() {
        let rows = [45.0, 90.0, 100.0].into_iter().enumerate().map(|(i, t)| tps(&format!("p{i}/fp8"), t, t)).collect();
        let result = rank(&Inputs::synthetic(rows));
        assert_eq!(result.budget_floor_tps, Some(45.0));
        let at_floor = candidate(&result, "p0/fp8");
        assert_eq!(at_floor.tps_p50, Some(45.0));
        assert!(!at_floor.budget_floor_excluded);
        assert!(at_floor.scores.budget.is_some());
        assert!(result.selections.budget.contains(&"p0/fp8".to_owned()));
    }

    #[test]
    fn k_f_the_non_transitive_triple_orders_b_a_c_under_every_permutation() {
        let (a, b, c) = (entry("A", 1.0, 99.6), entry("B", 0.992, 99.7), entry("C", 0.984, 99.8));
        // The configuration a pairwise comparator cycles on: A~B and B~C tie, A and C do not.
        assert!(a.score - b.score < tie() && b.score - c.score < tie() && a.score - c.score > tie());
        let perms = permutations(&[a, b, c]);
        assert_eq!(perms.len(), 6);
        for p in perms {
            assert_eq!(tags(&order_by_score(&p, 1.01)), ["B", "A", "C"]);
        }
    }

    #[test]
    fn k_g_fully_tied_entries_order_by_tag_in_code_unit_order() {
        let items: Vec<_> = ["b", "B", "a", "A"].into_iter().map(|t| entry(t, 1.0, 99.9)).collect();
        for p in permutations(&items) {
            assert_eq!(tags(&order_by_score(&p, 1.01)), ["A", "B", "a", "b"]);
        }
    }

    #[test]
    fn k9_inside_a_cluster_higher_uptime_then_lower_latency_then_lower_cost() {
        let items = [
            entry_full("cost-hi", 1.0, 99.9, 1.0, 0.03),
            entry_full("cost-lo", 1.0, 99.9, 1.0, 0.01),
            entry_full("lat-lo", 0.999, 99.9, 0.5, 0.05),
            entry_full("up-hi", 0.995, 99.95, 2.0, 0.05),
            entry_full("next", 0.98, 100.0, 0.1, 0.001), // outside the cluster: its better keys cannot lift it
        ];
        for p in permutations(&items) {
            assert_eq!(tags(&order_by_score(&p, 1.01)), ["up-hi", "lat-lo", "cost-lo", "cost-hi", "next"]);
        }
    }

    #[test]
    fn k9_the_cluster_bound_is_inclusive() {
        let items = [entry("a", 1.0, 99.6), entry("b", 1.0, 99.9)];
        for p in permutations(&items) {
            assert_eq!(tags(&order_by_score(&p, 1.0)), ["b", "a"], "with tieRatio 1 equal scores still order by uptime");
        }
        assert_eq!(tags(&order_by_score(&[entry("a", 1.0, 99.6), entry("b", 0.999999, 99.9)], 1.0)), ["a", "b"]);
    }

    #[test]
    fn k9_clusters_are_anchored_on_the_highest_remaining_score_not_chained() {
        let step = tie() * 0.6;
        let items = [entry("s0", 1.0, 99.5), entry("s1", 1.0 - step, 99.7), entry("s2", 1.0 - 2.0 * step, 99.9)];
        for p in permutations(&items) {
            assert_eq!(tags(&order_by_score(&p, 1.01)), ["s1", "s0", "s2"]);
        }
        assert!(order_by_score(&[], 1.01).is_empty());
        let input = [items[2].clone(), items[0].clone(), items[1].clone()];
        order_by_score(&input, 1.01);
        assert_eq!(tags(&input), ["s2", "s0", "s1"], "the input is not changed");
    }

    #[test]
    fn k_i_only_two_eligible_candidates_give_selections_of_two() {
        let rows = vec![row("a/fp8"), tps("b/fp8", 150.0, 200.0), RawEndpoint { quantization: Some("fp4".into()), ..row("c/fp4") }];
        let result = rank(&Inputs::synthetic(rows));
        for tier in RankedTier::ALL {
            assert_eq!(result.selections.get(tier).len(), 2);
        }
        assert_eq!(result.selections.fast, ["b/fp8", "a/fp8"]);
    }

    #[test]
    fn a_candidate_without_a_verified_cache_record_gets_no_cache_credit() {
        let mut none = Inputs::synthetic(vec![row("a/fp8")]);
        none.cache.clear();
        assert!(!candidate(&rank(&none), "a/fp8").cost.unwrap().cache_credit);
        let mut unverified = Inputs::synthetic(vec![row("a/fp8")]);
        unverified.cache.insert("a/fp8".into(), CacheRecord { verified: false, checked_at_ms: ms(CHECKED_AT) });
        assert!(!candidate(&rank(&unverified), "a/fp8").cost.unwrap().cache_credit);
        let mut proto = Inputs::synthetic(vec![row("constructor")]);
        proto.cache.clear();
        assert!(!candidate(&rank(&proto), "constructor").cost.unwrap().cache_credit);
        assert!(candidate(&rank(&Inputs::synthetic(vec![row("a/fp8")])), "a/fp8").cost.unwrap().cache_credit);
    }

    #[test]
    fn a_verified_cache_record_is_credited_however_old_as_in_chorus_1() {
        let mut old = Inputs::synthetic(vec![row("a/fp8")]);
        old.cache.insert("a/fp8".into(), CacheRecord { verified: true, checked_at_ms: ms("2025-01-01T00:00:00Z") });
        assert!(candidate(&rank(&old), "a/fp8").cost.unwrap().cache_credit);
    }

    #[test]
    fn no_eligible_candidate_gives_empty_selections_and_no_median_or_floor() {
        let result = rank(&Inputs::synthetic(vec![RawEndpoint { quantization: Some("fp4".into()), ..row("a/fp4") }]));
        assert_eq!(result.selections, TierOrders::default());
        assert_eq!((result.median_eligible_tps, result.budget_floor_tps), (None, None));
        assert_eq!(result.nitro_likely, Some(NitroLikely { tag: "a/fp4".into(), provider_name: "Provider".into(), tps_p50: 100.0 }));
        assert_eq!(result.nitro_fails_rules, ["fp4 below native fp8"]);
    }

    #[test]
    fn nitro_likely_is_the_fastest_passing_the_account_and_capability_filters() {
        let rows = vec![
            tps("fast-guarded", 400.0, 500.0),
            RawEndpoint { supported_parameters: ["tool_choice", "max_tokens", "reasoning"].map(str::to_owned).to_vec(), ..tps("fast-no-tools", 350.0, 500.0) },
            tps("policy", 300.0, 500.0),
            RawEndpoint { provider_name: "Flaky".into(), quantization: None, uptime_last_1d: Some(99.1), status: -2, ..tps("flaky", 250.0, 500.0) },
            tps("steady", 120.0, 150.0),
        ];
        let account = AccountEligibility {
            guardrail_removed: Some(vec!["fast-guarded".into()]),
            data_policy_removed: Some(vec!["policy".into()]),
            checked_at_ms: Some(ms(CHECKED_AT)),
        };
        let mut inputs = Inputs::synthetic(rows);
        inputs.account = account;
        let result = rank(&inputs);
        assert_eq!(result.nitro_likely, Some(NitroLikely { tag: "flaky".into(), provider_name: "Flaky".into(), tps_p50: 250.0 }));
        assert_eq!(result.nitro_fails_rules, ["uptime 99.10% < 99.5%", "status -2", "quantization not declared"]);
        // Under 'allow' the data-policy list does not apply, so 'policy' is in the pool.
        inputs.settings.data_collection = DataCollection::Allow;
        let allow = rank(&inputs);
        assert_eq!(allow.nitro_likely, Some(NitroLikely { tag: "policy".into(), provider_name: "Provider".into(), tps_p50: 300.0 }));
        assert!(allow.nitro_fails_rules.is_empty());
    }

    #[test]
    fn nitro_lists_neither_speed_nor_price_reasons_ties_go_to_the_lower_tag_and_none_when_empty() {
        let tied = vec![
            RawEndpoint { latency_last_30m: None, pricing: pricing("0", "0", Some("0")), ..row("b") },
            RawEndpoint { latency_last_30m: None, ..row("a") },
        ];
        let result = rank(&Inputs::synthetic(tied.into_iter().rev().collect()));
        assert_eq!(result.nitro_likely, Some(NitroLikely { tag: "a".into(), provider_name: "Provider".into(), tps_p50: 100.0 }));
        assert_eq!(candidate(&result, "a").excluded_by, ["no speed data"]);
        assert!(result.nitro_fails_rules.is_empty());
        let empty = rank(&Inputs::synthetic(vec![RawEndpoint { context_length: 1000, ..row("a") }, RawEndpoint { throughput_last_30m: None, ..row("b") }]));
        assert_eq!(empty.nitro_likely, None);
        assert!(empty.nitro_fails_rules.is_empty());
    }

    // ── Table F: the 2026-10-02 fixture ──

    fn excluded(result: &RankedCandidates) -> Vec<(&str, Vec<&str>)> {
        result
            .candidates
            .iter()
            .filter(|c| !c.excluded_by.is_empty())
            .map(|c| (c.tag.as_str(), c.excluded_by.iter().map(String::as_str).collect()))
            .collect()
    }

    #[test]
    fn table_f_the_golden_fixture_for_both_profiles() {
        for profile in [RoutingProfileId::Interactive, RoutingProfileId::Helper] {
            let golden = Inputs::golden(profile);
            let result = rank(&golden);
            // 32 candidates, each tag once, sorted by tag.
            let tag_list: Vec<&str> = result.candidates.iter().map(|c| c.tag.as_str()).collect();
            assert_eq!(tag_list.len(), 32);
            let mut sorted = tag_list.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(tag_list, sorted);
            // The 14 eligible candidates and the 18-row exclusion table, exactly.
            assert_eq!(result.candidates.iter().filter(|c| c.excluded_by.is_empty()).map(|c| c.tag.as_str()).collect::<Vec<_>>(), GOLDEN_ELIGIBLE);
            assert_eq!(excluded(&result), golden_exclusions());
            // Median 90.5, floor 45.25; only morph/fp8 (17) and baseten/fast (31) are below it.
            close(result.median_eligible_tps, 90.5);
            close(result.budget_floor_tps, 45.25);
            let below: Vec<_> = result.candidates.iter().filter(|c| c.budget_floor_excluded).map(|c| (c.tag.as_str(), c.tps_p50)).collect();
            assert_eq!(below, [("baseten/fast", Some(31.0)), ("morph/fp8", Some(17.0))]);
            let morph = candidate(&result, "morph/fp8");
            assert!(morph.scores.budget.is_none() && morph.scores.balanced.is_some() && morph.scores.fast.is_some());
            // Every candidate has limited history from one observation.
            assert!(result.candidates.iter().all(|c| c.observations == 1 && c.limited_history));
            // Nitro likely is together, failing 'quantization not declared'.
            assert_eq!(result.nitro_likely, Some(NitroLikely { tag: "together".into(), provider_name: "Together".into(), tps_p50: 223.0 }));
            assert_eq!(result.nitro_fails_rules, ["quantization not declared"]);
            // Golden selections.
            let [budget, balanced, fast] = golden_selections(profile);
            assert_eq!(result.selections.budget, budget, "{profile:?}");
            assert_eq!(result.selections.balanced, balanced, "{profile:?}");
            assert_eq!(result.selections.fast, fast, "{profile:?}");
            // Scores exist exactly for the eligible; excluded ones carry none and no floor flag.
            for c in &result.candidates {
                let eligible = c.excluded_by.is_empty();
                assert_eq!(eligible, c.scores.balanced.is_some() && c.scores.fast.is_some(), "{}", c.tag);
                if eligible {
                    assert_eq!(c.scores.budget.is_some(), !c.budget_floor_excluded, "{}", c.tag);
                } else {
                    assert_eq!(c.scores, TierScores::default(), "{}", c.tag);
                    assert!(!c.budget_floor_excluded, "{}", c.tag);
                }
            }
            // K-h: reversed endpoints and shuffled history give an equal result.
            for seed in [1, 7, 42] {
                let mut shuffled_input = golden.clone();
                shuffled_input.snapshot.endpoints.reverse();
                shuffled_input.history = shuffled(&golden.history, seed);
                assert_eq!(rank(&shuffled_input), result, "seed {seed}");
            }
            let mut rotated = golden.clone();
            rotated.snapshot.endpoints = shuffled(&golden.snapshot.endpoints, 99);
            assert_eq!(rank(&rotated), result);
        }
    }

    #[test]
    fn table_f_spot_values() {
        let interactive = rank(&Inputs::golden(RoutingProfileId::Interactive));
        let helper = rank(&Inputs::golden(RoutingProfileId::Helper));
        let ic = |tag: &str| candidate(&interactive, tag).clone();
        let hc = |tag: &str| candidate(&helper, tag).clone();

        // Interactive Budget: deepinfra and streamlake share a cluster; uptime 99.96 beats 99.60.
        close(ic("deepinfra/fp8").scores.budget, 4.0367);
        close(ic("streamlake/fp8").scores.budget, 4.03282);
        assert!(ic("deepinfra/fp8").scores.budget.unwrap() - ic("streamlake/fp8").scores.budget.unwrap() < tie());
        assert!(ic("deepinfra/fp8").uptime_1d.unwrap() > ic("streamlake/fp8").uptime_1d.unwrap());
        // Interactive Balanced: streamlake scores higher but deepinfra leads on uptime in one cluster.
        close(ic("streamlake/fp8").scores.balanced, 3.99985);
        close(ic("deepinfra/fp8").scores.balanced, 3.99215);
        assert!(ic("streamlake/fp8").scores.balanced.unwrap() - ic("deepinfra/fp8").scores.balanced.unwrap() < tie());
        close(ic("venice/fp8").scores.fast, 4.78673);
        // Helper Budget and Balanced.
        close(hc("streamlake/fp8").scores.budget, 4.36728);
        close(hc("deepinfra/fp8").scores.budget, 4.31882);
        close(hc("venice/fp8").scores.balanced, 4.23081);
        close(hc("gmicloud/fp8").scores.balanced, 4.22583);
        assert!(hc("venice/fp8").scores.balanced.unwrap() - hc("gmicloud/fp8").scores.balanced.unwrap() < tie());
        // Effective throughput.
        close(ic("venice/fp8").effective_tps, 119.909);
        close(ic("deepinfra/fp8").effective_tps, 48.4621);
        close(ic("baidu/fp8").effective_tps, 89.7453);
        close(ic("deepinfra/fp8").latency_p50_s, 1.4285);
        // Blended cost.
        let blended = |c: CandidateExplanation| c.cost.map(|k| k.blended);
        close(blended(ic("deepinfra/fp8")), 0.0165144);
        close(blended(ic("streamlake/fp8")), 0.0168692);
        close(blended(ic("gmicloud/fp8")), 0.0215352);
        close(blended(ic("baidu/fp8")), 0.30959);
        close(blended(ic("venice/fp8")), 0.044865);
        assert!(!ic("baidu/fp8").cost.unwrap().cache_credit);
        assert!(ic("deepinfra/fp8").cost.unwrap().cache_credit);
        assert!(fixture_json()["cacheVerified"].get("wafer").is_none(), "wafer has no cache record, so no credit");
        assert!(!ic("wafer").cost.unwrap().cache_credit);
    }

    #[test]
    fn explanation_records_follow_the_contract() {
        let interactive = rank(&Inputs::golden(RoutingProfileId::Interactive));
        let baseten = candidate(&interactive, "baseten/fp8");
        let keys: Vec<String> = serde_json::to_value(baseten).unwrap().as_object().unwrap().keys().cloned().collect();
        let mut expected = [
            "tag", "providerName", "rows", "quantization", "rowQuantizations", "effectiveQuantization", "uptime1d", "uptime5m", "status",
            "observations", "limitedHistory", "tpsP50", "latencyP50S", "tpsP90", "latencyP90S", "effectiveTps", "cost", "excludedBy",
            "budgetFloorExcluded", "scores",
        ];
        expected.sort();
        assert_eq!(keys, expected);
        assert_eq!((baseten.provider_name.as_str(), baseten.rows), ("BaseTen", 2));
        assert_eq!((baseten.quantization, baseten.effective_quantization), (crate::routing::types::Quantization::Fp8, crate::routing::types::Quantization::Fp8));
        assert_eq!(baseten.row_quantizations, [crate::routing::types::Quantization::Fp8]);
        assert_same(baseten.uptime_1d, 99.92966632878922);
        assert_eq!((baseten.uptime_5m, baseten.status, baseten.tps_p50, baseten.tps_p90), (Some(99.2), 0, Some(62.0), Some(293.0)));
        close(baseten.latency_p50_s, 0.305);
        close(baseten.latency_p90_s, 1.4565);
        assert_eq!(candidate(&interactive, "baseten/fast").effective_quantization, crate::routing::types::Quantization::Fp32);
        // An excluded candidate still carries its cost and effective throughput.
        let together = candidate(&interactive, "together");
        assert!(together.cost.is_some() && together.effective_tps.is_some());
        assert_eq!(together.effective_quantization, crate::routing::types::Quantization::Unknown);
        // K2: the result is plain JSON.
        assert_round_trip(&interactive.candidates);
    }

    #[test]
    fn a_stored_low_observation_keeps_a_recovering_endpoint_out_until_99_6() {
        let mut inputs = Inputs::synthetic(vec![row("a/fp8"), RawEndpoint { uptime_last_1d: Some(99.55), ..row("b/fp8") }]);
        let earlier = RoutingObservation { observed_at_ms: ms(NOW) - 45 * MINUTE_MS, uptime_1d: Some(99.4), ..inputs.history[1].clone() };
        inputs.history.push(earlier);
        let result = rank(&inputs);
        assert_eq!(candidate(&result, "b/fp8").excluded_by, ["readmission needs 99.6% (now 99.55%)"]);
        assert_eq!(candidate(&result, "b/fp8").observations, 2);
        assert_eq!(result.selections.fast, ["a/fp8"]);
    }

    #[test]
    fn a_stored_copy_of_the_snapshot_is_counted_once() {
        let base = Inputs::synthetic(vec![row("a/fp8")]);
        let mut stored = base.clone();
        stored.history = vec![RoutingObservation { observed_at_ms: ms("2026-10-02T09:05:00.000Z"), ..base.history[0].clone() }];
        assert_eq!(candidate(&rank(&stored), "a/fp8").observations, 1);
        let mut empty = base.clone();
        empty.history.clear();
        assert_eq!(rank(&empty), rank(&base));
    }

    // ── K-h over a rich history ──

    const RICH_TAGS: [&str; 5] = ["alpha/fp8", "Beta/fp8", "delta/fp8", "eps/fp8", "gamma/fp8"];

    fn rich_rows() -> Vec<RawEndpoint> {
        let uptimes = [99.55, 99.65, 99.58, 99.9, 99.52];
        let mut rows: Vec<RawEndpoint> = RICH_TAGS
            .iter()
            .enumerate()
            .map(|(i, tag)| RawEndpoint {
                uptime_last_1d: Some(uptimes[i]),
                throughput_last_30m: pct(Some(60.0 + 25.0 * i as f64), Some(200.0)),
                latency_last_30m: pct(Some(400.0 + 150.0 * i as f64), Some(2000.0)),
                pricing: pricing(&format!("0.000000{}", i + 1), "0.0000004", Some("0.00000001")),
                ..row(tag)
            })
            .collect();
        // A second row sharing a tag, so collapse runs under shuffled endpoints too.
        rows.push(RawEndpoint { provider_name: "Gamma".into(), uptime_last_1d: Some(99.8), throughput_last_30m: pct(Some(70.0), Some(210.0)), ..row("gamma/fp8") });
        rows
    }

    /// Chorus 1's rich history: 12 observations per tag, every 3rd duplicated at the same instant
    /// with other values, plus a stored copy of the snapshot, one after now and one older than 7
    /// days. For the even-indexed tags the only low uptime inside the window is the k = 3
    /// duplicate, so readmission depends on that duplicate collapsing to the minimum.
    fn rich_history() -> Vec<RoutingObservation> {
        let mut s: u32 = 12345;
        let mut next = move || {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            f64::from(s) / 4_294_967_296.0
        };
        let mut sample = |tag: &str, at_ms: i64, k: u32, low: bool| {
            let tps = if k.is_multiple_of(5) {
                Some(0.0)
            } else if k.is_multiple_of(7) {
                None
            } else {
                Some((40.0 + next() * 160.0).round())
            };
            let uptime_1d = Some(if low { 99.4 + next() * 0.09 } else { 99.5 + next() * 0.5 });
            let uptime_5m = if k.is_multiple_of(11) { None } else { Some(96.0 + next() * 4.0) };
            let latency_p50_ms = if k.is_multiple_of(6) { None } else { Some((300.0 + next() * 2000.0).round()) };
            let latency_p90_ms = Some((2500.0 + next() * 3000.0).round());
            RoutingObservation {
                tag: tag.into(),
                observed_at_ms: at_ms,
                uptime_1d,
                uptime_5m,
                status: 0,
                tps_p50: tps,
                tps_p90: tps.map(|t| t * 2.0),
                latency_p50_ms,
                latency_p90_ms,
            }
        };
        let before = |minutes: i64| ms(NOW) - minutes * MINUTE_MS;
        let mut out = Vec::new();
        for (i, tag) in RICH_TAGS.iter().enumerate() {
            for k in 1..=12u32 {
                let minutes = 15 + 30 * i64::from(k) + 7 * i64::from(k * k);
                out.push(sample(tag, before(minutes), k, k == 8)); // k = 8 is low but outside the window
                if k.is_multiple_of(3) {
                    out.push(sample(tag, before(minutes), k + 1, k == 3 && i.is_multiple_of(2)));
                }
            }
            out.push(sample(tag, ms("2026-10-02T09:05:00.000Z"), 2, false)); // the snapshot's own instant
            out.push(sample(tag, before(-10), 1, true)); // after now
            out.push(sample(tag, before(7 * 24 * 60 + 30), 4, true)); // older than 7 days
        }
        out
    }

    #[test]
    fn k_h_the_rich_history_is_used_and_order_never_matters() {
        let history = rich_history();
        let mut inputs = Inputs::synthetic(rich_rows());
        inputs.history = history.clone();
        let base = rank(&inputs);
        assert_eq!(history.len(), RICH_TAGS.len() * (12 + 4 + 3));
        assert_eq!(base.candidates.len(), RICH_TAGS.len());
        assert!(base.candidates.iter().map(|c| c.observations).max().unwrap() >= 3);
        // Readmission comes only from the low k = 3 duplicates.
        let reasons: Vec<(&str, Vec<&str>)> =
            base.candidates.iter().map(|c| (c.tag.as_str(), c.excluded_by.iter().map(String::as_str).collect())).collect();
        assert_eq!(
            reasons,
            [
                ("Beta/fp8", vec![]),
                ("alpha/fp8", vec!["readmission needs 99.6% (now 99.55%)"]),
                ("delta/fp8", vec!["readmission needs 99.6% (now 99.58%)"]),
                ("eps/fp8", vec![]),
                ("gamma/fp8", vec!["readmission needs 99.6% (now 99.52%)"]),
            ]
        );
        assert_eq!(base.selections.fast.len(), 2);
        for seed in [1u32, 2, 3, 5, 8, 13] {
            let mut shuffled_input = Inputs::synthetic(shuffled(&rich_rows(), seed));
            shuffled_input.history = shuffled(&history, seed * 31 + 7);
            assert_eq!(rank(&shuffled_input), base, "seed {seed}");
        }
        let mut reversed = Inputs::synthetic(rich_rows().into_iter().rev().collect());
        reversed.history = history.into_iter().rev().collect();
        assert_eq!(rank(&reversed), base);
    }
}
