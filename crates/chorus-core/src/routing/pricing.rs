//! Time-of-day pricing overrides (K11, MR-D13, C4), the price collapse of rows sharing a tag
//! (K8), and the blended cost at a profile's token mix (K11, MR-D9, C5). Ported from Chorus 1's
//! `pricingCore.ts`. `now_ms` is a parameter; the clock is never read.

use super::js;
use super::time::{utc_hhmm, utc_weekday};
use super::types::{CandidateCost, RawPricing, RawPricingOverride, TokenShares};

/// One row's prices at an instant, in USD per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedPricing {
    pub prompt_per_m: f64,
    pub completion_per_m: f64,
    pub cache_read_per_m: Option<f64>,
    pub cache_write_per_m: Option<f64>,
    pub override_applied: bool,
}

/// A tag's prices: the highest of its rows'.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CollapsedPricing {
    pub prompt_per_m: f64,
    pub completion_per_m: f64,
    pub cache_price_per_m: f64,
    pub override_applied: bool,
}

/// Index = `getUTCDay()`.
const UTC_WEEKDAYS: [&str; 7] = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];

/// OpenRouter's per-token USD text to USD per million tokens (JavaScript's `Number(text)`, so
/// text that is not a number gives NaN). -0 comes back as 0.
pub fn per_token_to_per_million(price: &str) -> f64 {
    price.trim().parse::<f64>().unwrap_or(f64::NAN) * 1_000_000.0 + 0.0
}

/// Whether one override applies at `now_ms` (K11, C4). Times are HHMM UTC with seconds dropped:
/// the start is inclusive, the end exclusive, and `end <= start` wraps midnight. `utc_days` is
/// tested against the UTC weekday of now (any case; an unknown name or an empty list never
/// matches). `min_prompt_tokens` applies only when `prompt_tokens` is strictly above it.
pub fn override_applies(o: &RawPricingOverride, now_ms: i64, prompt_tokens: u64) -> bool {
    let day = UTC_WEEKDAYS[utc_weekday(now_ms) as usize];
    let t = utc_hhmm(now_ms);
    if o.min_prompt_tokens.is_some_and(|min| prompt_tokens <= min) {
        return false;
    }
    if o.utc_days.as_ref().is_some_and(|days| !days.iter().any(|d| d.to_lowercase() == day)) {
        return false;
    }
    if o.utc_start.is_none() && o.utc_end.is_none() {
        return true;
    }
    let start = o.utc_start.unwrap_or(0);
    let end = o.utc_end.unwrap_or(0);
    if end > start { start <= t && t < end } else { t >= start || t < end }
}

/// Base prices with every applicable override applied in order: a later entry wins per key, and
/// a key an override doesn't carry keeps the earlier value (in the end, the base). A missing
/// cache key resolves to `None`.
pub fn resolve_pricing(pricing: &RawPricing, now_ms: i64, prompt_tokens: u64) -> ResolvedPricing {
    let mut prompt = pricing.prompt.as_str();
    let mut completion = pricing.completion.as_str();
    let mut cache_read = pricing.input_cache_read.as_deref();
    let mut cache_write = pricing.input_cache_write.as_deref();
    let mut override_applied = false;
    for o in pricing.overrides.iter().flatten().filter(|o| override_applies(o, now_ms, prompt_tokens)) {
        override_applied = true;
        prompt = o.prompt.as_deref().unwrap_or(prompt);
        completion = o.completion.as_deref().unwrap_or(completion);
        cache_read = o.input_cache_read.as_deref().or(cache_read);
        cache_write = o.input_cache_write.as_deref().or(cache_write);
    }
    ResolvedPricing {
        prompt_per_m: per_token_to_per_million(prompt),
        completion_per_m: per_token_to_per_million(completion),
        cache_read_per_m: cache_read.map(per_token_to_per_million),
        cache_write_per_m: cache_write.map(per_token_to_per_million),
        override_applied,
    }
}

/// The prices of rows sharing a tag (K8: the highest of each). A row without a cache-read price
/// contributes its prompt price. No rows gives non-finite prices, which [`blended_cost`] refuses.
pub fn collapse_pricing(rows: &[ResolvedPricing]) -> CollapsedPricing {
    let highest = |pick: fn(&ResolvedPricing) -> f64| rows.iter().map(pick).fold(f64::NEG_INFINITY, js::max);
    CollapsedPricing {
        prompt_per_m: highest(|r| r.prompt_per_m),
        completion_per_m: highest(|r| r.completion_per_m),
        cache_price_per_m: highest(|r| r.cache_read_per_m.unwrap_or(r.prompt_per_m)),
        override_applied: rows.iter().any(|r| r.override_applied),
    }
}

/// The blended $/M at a profile's token mix (K11, MR-D9). The cached share earns the cache price
/// only when the tag's cache was verified; otherwise it is priced as fresh prompt. `None` (C5:
/// no usable price) when a price is negative or not finite, or the blend is not positive.
pub fn blended_cost(prices: &CollapsedPricing, shares: &TokenShares, cache_verified: bool) -> Option<CandidateCost> {
    let CollapsedPricing { prompt_per_m, completion_per_m, cache_price_per_m, override_applied } = *prices;
    if ![prompt_per_m, completion_per_m, cache_price_per_m].iter().all(|p| p.is_finite() && *p >= 0.0) {
        return None;
    }
    let cached_price = if cache_verified { cache_price_per_m } else { prompt_per_m };
    let fresh = shares.fresh * prompt_per_m;
    let cached = shares.cached * cached_price;
    let output = shares.output * completion_per_m;
    let blended = fresh + cached + output;
    if !blended.is_finite() || blended <= 0.0 {
        return None;
    }
    Some(CandidateCost {
        blended,
        fresh,
        cached,
        output,
        cache_credit: cache_verified,
        override_applied,
        prompt_per_m,
        completion_per_m,
        cache_read_per_m: cache_price_per_m,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::endpoints::collapse_endpoints;
    use crate::routing::test_support::*;
    use crate::routing::types::{INTERACTIVE_PROFILE, RawPricing};

    // Table P base: prompt 0.10/M, completion 0.40/M, cache read 0.01/M; override price 0.20/M.
    const OVR: &str = "0.0000002";
    const WEEKDAYS: [&str; 5] = ["monday", "tuesday", "wednesday", "thursday", "friday"];

    fn base() -> RawPricing {
        pricing("0.0000001", "0.0000004", Some("0.00000001"))
    }

    fn with_overrides(overrides: Vec<RawPricingOverride>) -> RawPricing {
        RawPricing { overrides: Some(overrides), ..base() }
    }

    fn ovr(prompt: &str) -> RawPricingOverride {
        RawPricingOverride { prompt: Some(prompt.into()), ..RawPricingOverride::default() }
    }

    fn days(names: &[&str]) -> Option<Vec<String>> {
        Some(names.iter().map(|d| (*d).to_owned()).collect())
    }

    /// The resolved prompt price (0.20 when the override applies, else 0.10) and the applied flag.
    #[track_caller]
    fn expect_prompt(p: &RawPricing, now: &str, applied: bool, prompt_tokens: u64) {
        let r = resolve_pricing(p, ms(now), prompt_tokens);
        close(Some(r.prompt_per_m), if applied { 0.2 } else { 0.1 });
        assert_eq!(r.override_applied, applied, "{now}");
    }

    fn pricing_of(tag: &str) -> Vec<RawPricing> {
        collapse_endpoints(&golden_endpoints()).into_iter().find(|c| c.tag == tag).unwrap().pricing
    }

    #[test]
    fn per_token_prices_scale_to_per_million() {
        close(Some(per_token_to_per_million("0.0000003")), 0.3);
        close(Some(per_token_to_per_million("0.000000002115")), 0.002115);
        close(Some(per_token_to_per_million("3e-7")), 0.3);
        assert!(per_token_to_per_million("abc").is_nan());
    }

    #[test]
    fn negative_zero_normalises_to_zero_through_a_cost() {
        assert!(per_token_to_per_million("-0").is_sign_positive());
        let prices = collapse_pricing(&[resolve_pricing(&pricing("-0", "0.0000004", None), ms("2026-10-02T12:00:00Z"), 0)]);
        let cost = blended_cost(&prices, &INTERACTIVE_PROFILE.token_shares, true).unwrap();
        assert!(cost.fresh == 0.0 && cost.fresh.is_sign_positive());
        assert_round_trip(&cost);
    }

    #[test]
    fn p1_weekday_window_start_inclusive_end_exclusive_seconds_ignored() {
        let p1 = with_overrides(vec![RawPricingOverride {
            utc_start: Some(100),
            utc_end: Some(400),
            utc_days: days(&WEEKDAYS),
            ..ovr(OVR)
        }]);
        expect_prompt(&p1, "2026-10-02T01:00:00Z", true, 0);
        expect_prompt(&p1, "2026-10-02T03:59:59Z", true, 0);
        expect_prompt(&p1, "2026-10-02T04:00:00Z", false, 0);
        expect_prompt(&p1, "2026-10-02T00:59:00Z", false, 0);
        // P2: not on Saturday.
        expect_prompt(&p1, "2026-10-03T02:00:00Z", false, 0);
    }

    #[test]
    fn p3_days_without_a_window_cover_the_whole_listed_days() {
        let p = with_overrides(vec![RawPricingOverride { utc_days: days(&["saturday", "sunday"]), ..ovr(OVR) }]);
        expect_prompt(&p, "2026-10-03T00:00:00Z", true, 0);
        expect_prompt(&p, "2026-10-04T23:59:00Z", true, 0);
        expect_prompt(&p, "2026-10-02T12:00:00Z", false, 0);
    }

    #[test]
    fn p4_a_window_with_end_at_or_before_start_wraps_midnight() {
        let p = with_overrides(vec![RawPricingOverride { utc_start: Some(2200), utc_end: Some(200), ..ovr(OVR) }]);
        expect_prompt(&p, "2026-10-02T22:00:00Z", true, 0);
        expect_prompt(&p, "2026-10-02T23:30:00Z", true, 0);
        expect_prompt(&p, "2026-10-02T01:59:00Z", true, 0);
        expect_prompt(&p, "2026-10-02T02:00:00Z", false, 0);
        expect_prompt(&p, "2026-10-02T21:59:00Z", false, 0);
    }

    #[test]
    fn p5_utc_end_0_means_until_midnight() {
        let p = with_overrides(vec![RawPricingOverride { utc_start: Some(1400), utc_end: Some(0), ..ovr(OVR) }]);
        expect_prompt(&p, "2026-10-02T14:00:00Z", true, 0);
        expect_prompt(&p, "2026-10-02T23:59:00Z", true, 0);
        expect_prompt(&p, "2026-10-02T00:00:00Z", false, 0);
        expect_prompt(&p, "2026-10-02T13:59:00Z", false, 0);
    }

    #[test]
    fn p6_start_equal_to_end_covers_the_whole_day() {
        let p = with_overrides(vec![RawPricingOverride { utc_start: Some(0), utc_end: Some(0), ..ovr(OVR) }]);
        expect_prompt(&p, "2026-10-02T00:00:00Z", true, 0);
        expect_prompt(&p, "2026-10-02T12:34:00Z", true, 0);
    }

    #[test]
    fn p7_min_prompt_tokens_applies_only_strictly_above() {
        let p = with_overrides(vec![RawPricingOverride { min_prompt_tokens: Some(128_000), ..ovr(OVR) }]);
        expect_prompt(&p, "2026-10-02T12:00:00Z", false, 128_000);
        expect_prompt(&p, "2026-10-02T12:00:00Z", true, 128_001);
    }

    #[test]
    fn p8_later_applicable_entries_win_per_key() {
        let p = with_overrides(vec![RawPricingOverride { completion: Some("0.0000009".into()), ..ovr("0.0000003") }, ovr(OVR)]);
        let r = resolve_pricing(&p, ms("2026-10-02T12:00:00Z"), 0);
        close(Some(r.prompt_per_m), 0.2);
        close(Some(r.completion_per_m), 0.9);
        assert!(r.override_applied);
    }

    #[test]
    fn p9_keys_an_override_does_not_carry_inherit_the_base() {
        let r = resolve_pricing(&with_overrides(vec![ovr(OVR)]), ms("2026-10-02T12:00:00Z"), 0);
        close(Some(r.prompt_per_m), 0.2);
        close(Some(r.completion_per_m), 0.4);
        close(r.cache_read_per_m, 0.01);
        assert_eq!(r.cache_write_per_m, None);
    }

    #[test]
    fn p10_input_cache_write_is_resolved_and_absent_stays_none() {
        let write = RawPricingOverride { input_cache_write: Some("0.00000005".into()), ..RawPricingOverride::default() };
        let r = resolve_pricing(&with_overrides(vec![write]), ms("2026-10-02T12:00:00Z"), 0);
        close(r.cache_write_per_m, 0.05);
        close(Some(r.prompt_per_m), 0.1);
        assert_eq!(resolve_pricing(&base(), ms("2026-10-02T12:00:00Z"), 0).cache_write_per_m, None);
    }

    #[test]
    fn p11_day_names_compare_in_any_case_and_an_unknown_name_never_matches() {
        expect_prompt(&with_overrides(vec![RawPricingOverride { utc_days: days(&["Saturday"]), ..ovr(OVR) }]), "2026-10-03T10:00:00Z", true, 0);
        let caturday = with_overrides(vec![RawPricingOverride { utc_days: days(&["caturday"]), ..ovr(OVR) }]);
        for day in ["2026-10-03", "2026-10-04", "2026-10-05", "2026-10-06", "2026-10-07", "2026-10-08", "2026-10-09"] {
            expect_prompt(&caturday, &format!("{day}T10:00:00Z"), false, 0);
        }
        assert!(!override_applies(&RawPricingOverride { utc_days: Some(vec![]), ..ovr(OVR) }, ms("2026-10-03T10:00:00Z"), 0));
    }

    #[test]
    fn p12_utc_days_is_tested_against_now_also_after_a_wrap() {
        let p = with_overrides(vec![RawPricingOverride { utc_start: Some(2200), utc_end: Some(200), utc_days: days(&["friday"]), ..ovr(OVR) }]);
        expect_prompt(&p, "2026-10-02T23:00:00Z", true, 0);
        expect_prompt(&p, "2026-10-03T01:00:00Z", false, 0);
    }

    #[test]
    fn p13_no_overrides_gives_the_base_prices() {
        let r = resolve_pricing(&base(), ms("2026-10-02T12:00:00Z"), 0);
        close(Some(r.prompt_per_m), 0.1);
        close(Some(r.completion_per_m), 0.4);
        close(r.cache_read_per_m, 0.01);
        assert_eq!((r.cache_write_per_m, r.override_applied), (None, false));
    }

    #[test]
    fn a_non_applying_override_and_a_window_with_only_a_start() {
        let r = resolve_pricing(
            &with_overrides(vec![RawPricingOverride { utc_start: Some(100), utc_end: Some(400), ..ovr(OVR) }]),
            ms("2026-10-02T12:00:00Z"),
            0,
        );
        assert!(!r.override_applied);
        let only_start = with_overrides(vec![RawPricingOverride { utc_start: Some(1800), ..ovr(OVR) }]);
        expect_prompt(&only_start, "2026-10-02T18:00:00Z", true, 0);
        expect_prompt(&only_start, "2026-10-02T17:59:00Z", false, 0);
    }

    #[test]
    fn table_d_real_schedules_from_the_fixture() {
        let tokens = INTERACTIVE_PROFILE.typical_prompt_tokens;
        let cases: [(&str, &str, [f64; 3], bool); 9] = [
            ("deepseek", "2026-10-02T02:00:00Z", [0.3, 1.2, 0.006], true),
            ("deepseek", "2026-10-02T04:30:00Z", [0.15, 0.6, 0.003], true),
            ("deepseek", "2026-10-03T02:00:00Z", [0.15, 0.6, 0.003], true),
            ("deepseek", "2026-10-03T12:00:00Z", [0.15, 0.6, 0.003], true),
            ("deepseek", "2026-10-02T09:20:00Z", [0.3, 1.2, 0.006], true),
            ("alibaba", "2026-10-02T09:20:00Z", [0.3, 1.2, 0.03], true),
            ("alibaba", "2026-10-02T15:00:00Z", [0.15, 0.6, 0.015], true),
            ("deepinfra/fp8", "2026-10-02T09:20:00Z", [0.14, 0.42, 0.0042], false),
            ("deepinfra/fp8", "2026-10-03T03:00:00Z", [0.14, 0.42, 0.0042], false),
        ];
        for (tag, now, prices, applied) in cases {
            let rows = pricing_of(tag);
            assert_eq!(rows.len(), 1);
            let r = resolve_pricing(&rows[0], ms(now), tokens);
            close(Some(r.prompt_per_m), prices[0]);
            close(Some(r.completion_per_m), prices[1]);
            close(r.cache_read_per_m, prices[2]);
            assert_eq!(r.override_applied, applied, "{tag} at {now}");
        }
    }

    fn prices_for(tag: &str) -> CollapsedPricing {
        let resolved: Vec<_> = pricing_of(tag).iter().map(|p| resolve_pricing(p, ms(NOW), INTERACTIVE_PROFILE.typical_prompt_tokens)).collect();
        collapse_pricing(&resolved)
    }

    fn cache_verified(tag: &str) -> Option<bool> {
        fixture_json()["cacheVerified"][tag].as_bool()
    }

    #[test]
    fn b1_deepinfra_with_a_verified_cache_earns_the_cache_credit() {
        let cost = blended_cost(&prices_for("deepinfra/fp8"), &INTERACTIVE_PROFILE.token_shares, cache_verified("deepinfra/fp8") == Some(true)).unwrap();
        close(Some(cost.blended), 0.0165144);
        close(Some(cost.fresh), 0.00798);
        close(Some(cost.cached), 0.0039144);
        close(Some(cost.output), 0.00462);
        assert!(cost.cache_credit && !cost.override_applied);
        close(Some(cost.prompt_per_m), 0.14);
        close(Some(cost.completion_per_m), 0.42);
        close(Some(cost.cache_read_per_m), 0.0042);
    }

    #[test]
    fn b2_deepinfra_without_verification_prices_the_cached_share_as_prompt() {
        let cost = blended_cost(&prices_for("deepinfra/fp8"), &INTERACTIVE_PROFILE.token_shares, false).unwrap();
        close(Some(cost.blended), 0.14308);
        assert!(!cost.cache_credit);
        close(Some(cost.cache_read_per_m), 0.0042);
    }

    #[test]
    fn b3_baidu_whose_cache_is_recorded_as_not_verified() {
        assert_eq!(cache_verified("baidu/fp8"), Some(false));
        let cost = blended_cost(&prices_for("baidu/fp8"), &INTERACTIVE_PROFILE.token_shares, false).unwrap();
        close(Some(cost.blended), 0.30959);
        assert!(!cost.cache_credit);
    }

    #[test]
    fn b4_a_row_without_a_cache_price_verified_prices_the_cached_share_as_prompt() {
        let resolved = resolve_pricing(&pricing("0.0000001", "0.0000004", None), ms(NOW), 0);
        assert_eq!(resolved.cache_read_per_m, None);
        let prices = collapse_pricing(&[resolved]);
        close(Some(prices.cache_price_per_m), 0.1);
        let cost = blended_cost(&prices, &INTERACTIVE_PROFILE.token_shares, true).unwrap();
        close(Some(cost.cached), 0.932 * 0.1);
        close(Some(cost.blended), 0.057 * 0.1 + 0.932 * 0.1 + 0.011 * 0.4);
        assert!(cost.cache_credit);
        close(Some(cost.cache_read_per_m), 0.1);
    }

    #[test]
    fn b5_the_two_baseten_fp8_rows_collapse_to_the_prices_of_one() {
        let rows = pricing_of("baseten/fp8");
        assert_eq!(rows.len(), 2);
        let resolved: Vec<_> = rows.iter().map(|p| resolve_pricing(p, ms(NOW), INTERACTIVE_PROFILE.typical_prompt_tokens)).collect();
        assert_eq!(collapse_pricing(&resolved), collapse_pricing(&resolved[..1]));
        assert_eq!(collapse_pricing(&resolved), collapse_pricing(&resolved[1..]));
    }

    #[test]
    fn collapse_takes_the_highest_price_and_ors_the_override_flag() {
        let a = ResolvedPricing { prompt_per_m: 0.1, completion_per_m: 0.5, cache_read_per_m: Some(0.02), cache_write_per_m: None, override_applied: false };
        let b = ResolvedPricing { prompt_per_m: 0.2, completion_per_m: 0.4, cache_read_per_m: None, cache_write_per_m: None, override_applied: true };
        assert_eq!(
            collapse_pricing(&[a, b]),
            CollapsedPricing { prompt_per_m: 0.2, completion_per_m: 0.5, cache_price_per_m: 0.2, override_applied: true }
        );
        assert_eq!(collapse_pricing(&[b, a]), collapse_pricing(&[a, b]));
    }

    #[test]
    fn b6_a_negative_or_all_zero_price_has_no_cost() {
        let shares = &INTERACTIVE_PROFILE.token_shares;
        let negative = collapse_pricing(&[resolve_pricing(&pricing("-1", "0.0000004", Some("0.00000001")), ms(NOW), 0)]);
        assert_eq!(blended_cost(&negative, shares, true), None);
        let neg = CollapsedPricing { prompt_per_m: -1.0, completion_per_m: 0.4, cache_price_per_m: 0.01, override_applied: false };
        assert_eq!(blended_cost(&neg, shares, false), None);
        let zero = collapse_pricing(&[resolve_pricing(&pricing("0", "0", Some("0")), ms(NOW), 0)]);
        assert_eq!(blended_cost(&zero, shares, true), None);
        assert_eq!(blended_cost(&zero, shares, false), None);
        assert_eq!(blended_cost(&collapse_pricing(&[]), shares, false), None);
        let nan = CollapsedPricing { prompt_per_m: f64::NAN, ..neg };
        assert_eq!(blended_cost(&nan, shares, false), None);
    }

    #[test]
    fn costs_survive_a_json_round_trip() {
        let cost = blended_cost(&prices_for("deepinfra/fp8"), &INTERACTIVE_PROFILE.token_shares, true).unwrap();
        assert_round_trip(&cost);
    }
}
