//! The pure half of the routing store, ported from Chorus 1's `storeCore.ts`: the observation
//! history's merge with its retention and per-tag cap (K5, C13), the cache-record merge with its
//! cap, and the stored settings value read with defaults underneath (K6). Chorus 1 kept these in
//! JSON files; 2.0 keeps them in SQLite, so the file names, paths and envelopes are not ported.
//! The records are [`RoutingObservation`], [`super::types::CacheRecord`],
//! [`super::types::AccountEligibility`] and [`super::types::EndpointSnapshot`].

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fmt;

use serde_json::Value;

use super::js::by_code_unit;
use super::time::DAY_MS;
use super::types::{CacheVerification, RoutingObservation, RoutingSettings};

/// C13: 7 days × 48 observer ticks = 336, plus headroom for refreshes.
pub const OBSERVATIONS_MAX_PER_TAG: u32 = 400;
pub const CACHE_RECORDS_MAX: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionError {
    /// Not finite, or not above 0: it would keep everything or nothing, silently.
    MaxAgeDays,
    /// 0 would drop everything.
    MaxPerTag,
}

impl fmt::Display for RetentionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MaxAgeDays => "Invalid retention: maxAgeDays",
            Self::MaxPerTag => "Invalid retention: maxPerTag",
        })
    }
}

impl std::error::Error for RetentionError {}

/// The observation history with retention (K5, C13), relative to `now_ms`:
/// 1. `existing` then `added`, one per tag and instant; the later wins, so `added` replaces.
/// 2. Older than `max_age_days` before now is dropped; exactly that old is kept, and so is
///    anything after now.
/// 3. The newest `max_per_tag` of each tag survive.
/// 4. Sorted by time, oldest first, then by tag.
pub fn merge_observations(
    existing: &[RoutingObservation],
    added: &[RoutingObservation],
    now_ms: i64,
    max_age_days: f64,
    max_per_tag: u32,
) -> Result<Vec<RoutingObservation>, RetentionError> {
    if !max_age_days.is_finite() || max_age_days <= 0.0 {
        return Err(RetentionError::MaxAgeDays);
    }
    if max_per_tag == 0 {
        return Err(RetentionError::MaxPerTag);
    }
    let cutoff = now_ms as f64 - max_age_days * DAY_MS as f64;

    let mut unique: BTreeMap<(&str, i64), &RoutingObservation> = BTreeMap::new();
    for o in existing.iter().chain(added) {
        unique.insert((o.tag.as_str(), o.observed_at_ms), o);
    }
    let mut by_tag: BTreeMap<&str, Vec<&RoutingObservation>> = BTreeMap::new();
    for ((tag, _), o) in unique {
        if (o.observed_at_ms as f64) >= cutoff {
            by_tag.entry(tag).or_default().push(o);
        }
    }
    let mut kept: Vec<RoutingObservation> = Vec::new();
    for mut group in by_tag.into_values() {
        group.sort_by_key(|o| Reverse(o.observed_at_ms));
        kept.extend(group.into_iter().take(max_per_tag as usize).cloned());
    }
    kept.sort_by(|a, b| a.observed_at_ms.cmp(&b.observed_at_ms).then_with(|| by_code_unit(&a.tag, &b.tag)));
    Ok(kept)
}

/// Each tag in `added` replaces its record in `existing`. Above [`CACHE_RECORDS_MAX`], the
/// newest by check time survive (ties by tag).
pub fn merge_cache_verifications(existing: &CacheVerification, added: &CacheVerification) -> CacheVerification {
    let mut merged = existing.clone();
    merged.extend(added.iter().map(|(k, v)| (k.clone(), *v)));
    if merged.len() <= CACHE_RECORDS_MAX {
        return merged;
    }
    let mut entries: Vec<_> = merged.into_iter().collect();
    entries.sort_by(|(tag_a, a), (tag_b, b)| b.checked_at_ms.cmp(&a.checked_at_ms).then_with(|| by_code_unit(tag_a, tag_b)));
    entries.truncate(CACHE_RECORDS_MAX);
    entries.into_iter().collect()
}

/// A stored setting as read, and why it fell back to the defaults, if it did.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedSetting<T> {
    pub value: T,
    pub warning: Option<String>,
}

/// The routing settings from their stored JSON, with the defaults underneath, so a row written
/// before a field existed still reads. A row that isn't JSON, isn't an object, has an unknown
/// field or fails [`RoutingSettings::validate`] reads as the defaults, with a warning that never
/// quotes the row.
pub fn parse_routing_settings_value(raw: Option<&str>) -> ParsedSetting<RoutingSettings> {
    const SUBJECT: &str = "stored routing settings";
    let fallback = |warning: Option<String>| ParsedSetting { value: RoutingSettings::default(), warning };
    let Some(raw) = raw else { return fallback(None) };
    let Ok(json) = serde_json::from_str::<Value>(raw) else { return fallback(Some(format!("{SUBJECT} were not JSON; using defaults"))) };
    let invalid = || fallback(Some(format!("{SUBJECT} did not validate; using defaults")));
    let (Value::Object(stored), Ok(Value::Object(mut merged))) = (json, serde_json::to_value(RoutingSettings::default())) else { return invalid() };
    merged.extend(stored);
    match serde_json::from_value::<RoutingSettings>(Value::Object(merged)) {
        Ok(settings) if settings.validate().is_ok() => ParsedSetting { value: settings, warning: None },
        _ => invalid(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::endpoints::extract_observations;
    use crate::routing::test_support::*;
    use crate::routing::time::MINUTE_MS;
    use crate::routing::types::CacheRecord;

    fn obs(tag: &str, at: &str) -> RoutingObservation {
        obs_ms(tag, ms(at))
    }

    fn obs_ms(tag: &str, at_ms: i64) -> RoutingObservation {
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

    #[test]
    fn c8_the_same_tag_and_instant_collapse_to_one_and_the_added_one_wins() {
        let existing = RoutingObservation { tps_p50: Some(10.0), ..obs("deepinfra/fp8", "2026-10-02T09:05:00Z") };
        let added = RoutingObservation { tps_p50: Some(20.0), ..obs("deepinfra/fp8", "2026-10-02T09:05:00.000Z") };
        let merged = merge_observations(&[existing], std::slice::from_ref(&added), ms("2026-10-02T09:30:00Z"), 7.0, OBSERVATIONS_MAX_PER_TAG).unwrap();
        assert_eq!(merged, [added]);
    }

    #[test]
    fn c9_exactly_max_age_days_old_is_kept_one_second_older_is_dropped_and_after_now_is_kept() {
        let merged = merge_observations(
            &[obs("a", "2026-10-02T09:05:00Z"), obs("a", "2026-10-02T09:04:59Z")],
            &[obs("a", "2026-10-09T09:10:00Z")],
            ms("2026-10-09T09:05:00Z"),
            7.0,
            OBSERVATIONS_MAX_PER_TAG,
        )
        .unwrap();
        assert_eq!(merged.iter().map(|o| o.observed_at_ms).collect::<Vec<_>>(), [ms("2026-10-02T09:05:00Z"), ms("2026-10-09T09:10:00Z")]);
    }

    #[test]
    fn c10_450_observations_for_one_tag_keep_the_newest_400_oldest_first() {
        assert_eq!(OBSERVATIONS_MAX_PER_TAG, 400);
        let start = ms("2026-10-01T00:00:00Z");
        let all: Vec<_> = (0..450).map(|i| obs_ms("a", start + i * 30 * MINUTE_MS)).collect();
        // 10 days covers all 450 (9.35 days), so the per-tag bound is what cuts.
        let now = all[449].observed_at_ms;
        let merged = merge_observations(&[], &all, now, 10.0, OBSERVATIONS_MAX_PER_TAG).unwrap();
        assert_eq!(merged.len(), 400);
        assert_eq!(merged, all[50..]);
        assert_eq!(merge_observations(&all[..10], &[], now, 10.0, 3).unwrap(), all[7..10], "the bound is a parameter");
    }

    #[test]
    fn c11_two_tags_at_one_instant_sort_by_tag_and_the_inputs_are_untouched() {
        let existing = vec![obs("m/x", "2026-10-02T09:00:00Z")];
        let added = vec![obs("z/y", "2026-10-02T09:05:00Z"), obs("a/y", "2026-10-02T09:05:00Z")];
        let (existing_before, added_before) = (existing.clone(), added.clone());
        let merged = merge_observations(&existing, &added, ms("2026-10-02T09:30:00Z"), 7.0, OBSERVATIONS_MAX_PER_TAG).unwrap();
        assert_eq!(
            merged.iter().map(|o| (o.observed_at_ms, o.tag.as_str())).collect::<Vec<_>>(),
            [(ms("2026-10-02T09:00:00Z"), "m/x"), (ms("2026-10-02T09:05:00Z"), "a/y"), (ms("2026-10-02T09:05:00Z"), "z/y")]
        );
        assert_eq!((existing, added), (existing_before, added_before));
    }

    #[test]
    fn c12_retention_limits_must_be_usable() {
        let now = ms("2026-10-02T09:30:00Z");
        let one = [obs("a", "2026-10-02T09:05:00Z")];
        for max_age_days in [f64::NAN, f64::INFINITY, 0.0, -1.0] {
            let err = merge_observations(&one, &[], now, max_age_days, 400).unwrap_err();
            assert_eq!((err, err.to_string().as_str()), (RetentionError::MaxAgeDays, "Invalid retention: maxAgeDays"));
        }
        let err = merge_observations(&one, &[], now, 7.0, 0).unwrap_err();
        assert_eq!((err, err.to_string().as_str()), (RetentionError::MaxPerTag, "Invalid retention: maxPerTag"));
        assert_eq!(merge_observations(&one, &[], now, 0.5, 1).unwrap(), one, "a fractional age and a limit of 1 are valid");
    }

    #[test]
    fn the_golden_observations_merge_into_an_empty_history_unchanged_in_content() {
        let obs = extract_observations(&golden_snapshot());
        let merged = merge_observations(&[], &obs, ms(NOW), 7.0, OBSERVATIONS_MAX_PER_TAG).unwrap();
        assert_eq!(merged, obs, "one instant, so tag order is the order");
        assert_round_trip(&merged);
    }

    #[test]
    fn c13_cache_merge_replaces_per_tag_sorts_keys_and_keeps_the_newest_500() {
        let (t1, t2) = (ms("2026-10-01T00:00:00Z"), ms("2026-10-02T00:00:00Z"));
        let rec = |verified, checked_at_ms| CacheRecord { verified, checked_at_ms };
        let existing: CacheVerification = [("b".to_owned(), rec(false, t1)), ("a".to_owned(), rec(true, t1))].into();
        let added: CacheVerification = [("b".to_owned(), rec(true, t2))].into();
        let merged = merge_cache_verifications(&existing, &added);
        assert_eq!(merged, [("a".to_owned(), rec(true, t1)), ("b".to_owned(), rec(true, t2))].into());
        assert_eq!(merged.keys().collect::<Vec<_>>(), ["a", "b"]);

        assert_eq!(CACHE_RECORDS_MAX, 500);
        let start = ms("2026-09-01T00:00:00Z");
        // Tag order is the reverse of age order, so a cut by tag would keep the wrong one.
        let many: CacheVerification = (0..501).map(|i| (format!("t{:03}", 500 - i), rec(true, start + i * MINUTE_MS))).collect();
        let kept = merge_cache_verifications(&many, &CacheVerification::new());
        assert_eq!(kept.len(), 500);
        assert!(!kept.contains_key("t500"), "the oldest goes");
        let tied: CacheVerification = (0..501).map(|i| (format!("t{i:03}"), rec(false, t1))).collect();
        assert!(!merge_cache_verifications(&tied, &CacheVerification::new()).contains_key("t500"), "ties keep the code-unit-first tag");
    }

    #[test]
    fn c14_and_c15_no_row_or_a_partial_row_reads_with_defaults_underneath() {
        assert_eq!(parse_routing_settings_value(None), ParsedSetting { value: RoutingSettings::default(), warning: None });
        let partial = parse_routing_settings_value(Some(r#"{"minUptimePct":99.4}"#));
        assert_eq!(partial, ParsedSetting { value: RoutingSettings { min_uptime_pct: 99.4, ..RoutingSettings::default() }, warning: None });
        assert_eq!(parse_routing_settings_value(Some("{}")).value, RoutingSettings::default());
        let stored = serde_json::to_string(&RoutingSettings { fallback_count: 1, ..RoutingSettings::default() }).unwrap();
        assert_eq!(parse_routing_settings_value(Some(&stored)).value.fallback_count, 1, "a whole stored row reads back");
    }

    #[test]
    fn c16_unknown_keys_non_json_non_objects_and_rule_failures_read_as_defaults_with_a_warning() {
        let not_valid = Some("stored routing settings did not validate; using defaults".to_owned());
        for raw in [r#"{"bogus":1}"#, "[1]", "5", "null", r#"{"readmitUptimePct":99}"#, r#"{"tierWeights":{"fast":1}}"#] {
            assert_eq!(parse_routing_settings_value(Some(raw)), ParsedSetting { value: RoutingSettings::default(), warning: not_valid.clone() }, "{raw}");
        }
        assert_eq!(
            parse_routing_settings_value(Some("nope")),
            ParsedSetting { value: RoutingSettings::default(), warning: Some("stored routing settings were not JSON; using defaults".into()) }
        );
    }
}
