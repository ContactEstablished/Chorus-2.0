//! What the routing inspector shows beside the ranking (PLAN P12): each endpoint's speed history,
//! worked out from the stored observations, and the records of the Refresh log. Nothing here ranks
//! or changes the ranking; the medians are the ranker's own ([`median`]), and the age cut is
//! [`super::store::merge_observations`]'s.
//!
//! The app stores and reads; everything here is computed from its inputs. `now_ms` is a parameter.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use specta::Type;

use super::eligibility::median;
use super::js::by_code_unit;
use super::time::DAY_MS;
use super::types::RoutingObservation;

/// The Refresh log keeps this many entries, newest first.
pub const REFRESH_LOG_MAX: usize = 50;

/// One endpoint tag's stored observations, summarised. A `None` is unknown, not zero.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SpeedHistoryRow {
    pub tag: String,
    /// How many observations of the tag are within the age asked for.
    pub samples: u32,
    #[specta(type = f64)]
    pub newest_at_ms: i64,
    /// The newest observation's status; the inspector flags anything but 0.
    #[specta(type = f64)]
    pub newest_status: i64,
    /// The median of the positive `tps_p50` values.
    pub median_tps: Option<f64>,
    /// The median of the positive `latency_p50_ms` values, in seconds.
    pub median_latency_s: Option<f64>,
    /// The lowest `uptime_1d`.
    pub lowest_uptime_1d: Option<f64>,
}

/// The positive, finite values (zero and missing speeds carry no information).
fn positives(values: impl Iterator<Item = Option<f64>>) -> Vec<f64> {
    values.flatten().filter(|v| v.is_finite() && *v > 0.0).collect()
}

/// One tag's row from its observations; `None` for a tag with none.
fn speed_row(tag: &str, group: &[&RoutingObservation]) -> Option<SpeedHistoryRow> {
    let newest = group.iter().max_by_key(|o| o.observed_at_ms)?;
    Some(SpeedHistoryRow {
        tag: tag.to_owned(),
        samples: u32::try_from(group.len()).unwrap_or(u32::MAX),
        newest_at_ms: newest.observed_at_ms,
        newest_status: newest.status,
        median_tps: median(&positives(group.iter().map(|o| o.tps_p50))),
        median_latency_s: median(&positives(group.iter().map(|o| o.latency_p50_ms))).map(|ms| ms / 1000.0),
        lowest_uptime_1d: group.iter().filter_map(|o| o.uptime_1d).filter(|v| v.is_finite()).min_by(f64::total_cmp),
    })
}

/// The speed history of each tag that has observations no older than `max_age_days` before
/// `now_ms` (exactly that old is kept, as in `merge_observations`, and so is anything after now).
/// Rows follow `order`, then the tags not in it by name; a tag in `order` with no observations has
/// no row, and one listed twice has one.
pub fn speed_history(observations: &[RoutingObservation], order: &[String], max_age_days: f64, now_ms: i64) -> Vec<SpeedHistoryRow> {
    let cutoff = now_ms as f64 - max_age_days * DAY_MS as f64;
    let mut by_tag: BTreeMap<&str, Vec<&RoutingObservation>> = BTreeMap::new();
    for o in observations.iter().filter(|o| o.observed_at_ms as f64 >= cutoff) {
        by_tag.entry(o.tag.as_str()).or_default().push(o);
    }
    let mut rows: Vec<SpeedHistoryRow> = order
        .iter()
        .filter_map(|tag| by_tag.remove_entry(tag.as_str()))
        .filter_map(|(tag, group)| speed_row(tag, &group))
        .collect();
    // The map orders by UTF-8 bytes; names order by UTF-16 code unit, as the ranker's do.
    let mut rest: Vec<_> = by_tag.into_iter().collect();
    rest.sort_by(|(a, _), (b, _)| by_code_unit(a, b));
    rows.extend(rest.into_iter().filter_map(|(tag, group)| speed_row(tag, &group)));
    rows
}

/// How many updates the observations hold: their distinct observation times. One update writes one
/// observation per tag, all at one instant.
pub fn sample_count(observations: &[RoutingObservation]) -> u32 {
    let instants: BTreeSet<i64> = observations.iter().map(|o| o.observed_at_ms).collect();
    u32::try_from(instants.len()).unwrap_or(u32::MAX)
}

/// One Refresh that passed the checks made before any request. `error` is `None` when it
/// succeeded (the inspector says "Done"); a failure carries zero figures.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RefreshLogEntry {
    /// When the Refresh ended.
    #[specta(type = f64)]
    pub at_ms: i64,
    pub model: String,
    pub provider_name: String,
    pub error: Option<String>,
    pub endpoints_tested: u32,
    pub spent_usd: f64,
    pub estimate_usd: f64,
}

/// The stored Refresh log isn't a JSON array of entries. It never quotes the stored text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshLogError;

impl fmt::Display for RefreshLogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the Refresh log is not a JSON array of entries")
    }
}

impl std::error::Error for RefreshLogError {}

/// The Refresh log from its stored JSON. No row is an empty log; anything that isn't an array of
/// entries (an unknown field included) is an error.
pub fn parse_refresh_log(raw: Option<&str>) -> Result<Vec<RefreshLogEntry>, RefreshLogError> {
    match raw {
        None => Ok(Vec::new()),
        Some(raw) => serde_json::from_str(raw).map_err(|_| RefreshLogError),
    }
}

/// `entry` first, then `log` (newest first), cut to [`REFRESH_LOG_MAX`].
pub fn append_refresh_log(log: &[RefreshLogEntry], entry: RefreshLogEntry) -> Vec<RefreshLogEntry> {
    let mut out = Vec::with_capacity(REFRESH_LOG_MAX.min(log.len() + 1));
    out.push(entry);
    out.extend(log.iter().take(REFRESH_LOG_MAX - 1).cloned());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::endpoints::extract_observations;
    use crate::routing::test_support::*;
    use crate::routing::time::MINUTE_MS;

    fn obs(tag: &str, at_ms: i64) -> RoutingObservation {
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

    fn tags(rows: &[SpeedHistoryRow]) -> Vec<&str> {
        rows.iter().map(|r| r.tag.as_str()).collect()
    }

    fn owned(tags: &[&str]) -> Vec<String> {
        tags.iter().map(|t| (*t).to_owned()).collect()
    }

    #[test]
    fn a_row_has_the_count_the_newest_sample_and_medians_of_the_positive_values() {
        let now = ms(NOW);
        let at = |minutes_ago: i64| now - minutes_ago * MINUTE_MS;
        // Not in time order: the newest sample isn't the last one given. The newest has no usable
        // values and a status of 2, so the medians come from the other two (an even count, so the
        // mean of the middle two).
        let observations = [
            RoutingObservation { tps_p50: Some(10.0), latency_p50_ms: Some(1000.0), uptime_1d: Some(99.0), ..obs("a", at(60)) },
            RoutingObservation { status: 2, tps_p50: Some(0.0), latency_p50_ms: None, uptime_1d: None, ..obs("a", at(5)) },
            RoutingObservation { tps_p50: Some(30.0), latency_p50_ms: Some(3000.0), uptime_1d: Some(98.5), ..obs("a", at(30)) },
        ];
        let rows = speed_history(&observations, &[], 7.0, now);
        assert_eq!(
            rows,
            [SpeedHistoryRow {
                tag: "a".into(),
                samples: 3,
                newest_at_ms: at(5),
                newest_status: 2,
                median_tps: Some(20.0),
                median_latency_s: Some(2.0),
                lowest_uptime_1d: Some(98.5),
            }]
        );
    }

    #[test]
    fn an_odd_count_takes_the_middle_value_and_latency_is_in_seconds() {
        let now = ms(NOW);
        let observations: Vec<_> = [(5.0, 100.0), (50.0, 300.0), (20.0, 200.0)]
            .into_iter()
            .enumerate()
            .map(|(i, (tps, latency))| RoutingObservation { tps_p50: Some(tps), latency_p50_ms: Some(latency), ..obs("b", now - i as i64 * MINUTE_MS) })
            .collect();
        let rows = speed_history(&observations, &[], 7.0, now);
        assert_eq!((rows[0].median_tps, rows[0].median_latency_s), (Some(20.0), Some(0.2)));
    }

    #[test]
    fn values_that_are_missing_zero_negative_or_not_finite_are_none_but_the_tag_still_has_a_row() {
        let now = ms(NOW);
        let bad = |tps: Option<f64>, latency: Option<f64>, uptime: Option<f64>, minutes_ago: i64| RoutingObservation {
            tps_p50: tps,
            latency_p50_ms: latency,
            uptime_1d: uptime,
            ..obs("c", now - minutes_ago * MINUTE_MS)
        };
        let observations = [
            bad(None, None, None, 1),
            bad(Some(0.0), Some(0.0), None, 2),
            bad(Some(-5.0), Some(-100.0), None, 3),
            bad(Some(f64::NAN), Some(f64::INFINITY), Some(f64::NAN), 4),
        ];
        assert_eq!(
            speed_history(&observations, &[], 7.0, now),
            [SpeedHistoryRow {
                tag: "c".into(),
                samples: 4,
                newest_at_ms: now - MINUTE_MS,
                newest_status: 0,
                median_tps: None,
                median_latency_s: None,
                lowest_uptime_1d: None,
            }]
        );
    }

    #[test]
    fn the_lowest_uptime_is_kept_even_when_it_is_zero() {
        let now = ms(NOW);
        let observations: Vec<_> = [Some(100.0), Some(0.0), Some(57.5), None]
            .into_iter()
            .enumerate()
            .map(|(i, uptime)| RoutingObservation { uptime_1d: uptime, ..obs("d", now - i as i64 * MINUTE_MS) })
            .collect();
        assert_eq!(speed_history(&observations, &[], 7.0, now)[0].lowest_uptime_1d, Some(0.0));
    }

    #[test]
    fn observations_older_than_the_age_are_left_out_and_exactly_that_old_is_kept() {
        let now = ms(NOW);
        let observations = [
            obs("a", now - 7 * DAY_MS - 1),
            obs("a", now - 7 * DAY_MS),
            obs("a", now + 5 * MINUTE_MS),
            obs("only-old", now - 8 * DAY_MS),
            obs("only-just-too-old", now - 7 * DAY_MS - 1),
        ];
        let rows = speed_history(&observations, &[], 7.0, now);
        assert_eq!(tags(&rows), ["a"], "a tag with nothing in range has no row");
        assert_eq!((rows[0].samples, rows[0].newest_at_ms), (2, now + 5 * MINUTE_MS), "the one a millisecond too old doesn't count; one after now does");

        let half_a_day = speed_history(&observations, &[], 0.5, now);
        assert_eq!((tags(&half_a_day), half_a_day[0].samples), (vec!["a"], 1), "the age is a parameter, and may be a fraction");
        let in_the_order = speed_history(&observations, &owned(&["only-old", "a"]), 7.0, now);
        assert_eq!(tags(&in_the_order), ["a"], "naming a tag in the order doesn't bring back what is too old");
    }

    #[test]
    fn rows_follow_the_order_given_then_the_rest_by_name() {
        let now = ms(NOW);
        let observations = ["m", "z", "a", "b"].map(|t| obs(t, now));
        let order = owned(&["z", "gone", "m", "z"]);
        assert_eq!(tags(&speed_history(&observations, &order, 7.0, now)), ["z", "m", "a", "b"], "an unknown tag has no row, and a repeated one has one");
        assert_eq!(tags(&speed_history(&observations, &[], 7.0, now)), ["a", "b", "m", "z"]);
        assert!(speed_history(&[], &order, 7.0, now).is_empty(), "nothing observed, nothing shown");
    }

    #[test]
    fn the_rest_sort_by_utf16_code_unit_not_by_utf8_byte() {
        // U+1F600 is the surrogate pair D83D DE00 in UTF-16, so it sorts before U+FF5E, though its
        // UTF-8 bytes (F0 9F 98 80) sort after EF BD 9E.
        let now = ms(NOW);
        let observations = ["\u{ff5e}", "\u{1f600}"].map(|t| obs(t, now));
        assert_eq!(tags(&speed_history(&observations, &[], 7.0, now)), ["\u{1f600}", "\u{ff5e}"]);
    }

    #[test]
    fn the_golden_observations_give_one_row_per_tag_that_repeats_the_snapshots_numbers() {
        let observations = extract_observations(&golden_snapshot());
        assert!(
            observations.iter().all(|o| o.tps_p50.is_some_and(|v| v > 0.0) && o.latency_p50_ms.is_some_and(|v| v > 0.0)),
            "the fixture gives every tag a speed and a latency, so the medians below are the numbers themselves"
        );
        let rows = speed_history(&observations, &[], 7.0, ms(NOW));
        assert_eq!(rows.len(), observations.len(), "one row per tag");
        let mut distinct = tags(&rows);
        distinct.dedup();
        assert_eq!(distinct.len(), rows.len(), "and no tag twice");
        for o in &observations {
            let row = rows.iter().find(|r| r.tag == o.tag).unwrap_or_else(|| panic!("no row for {}", o.tag));
            assert_eq!(row.samples, 1, "{}", o.tag);
            assert_eq!(row.median_tps, o.tps_p50, "{}", o.tag);
            assert_eq!(row.median_latency_s, o.latency_p50_ms.map(|v| v / 1000.0), "{}", o.tag);
            assert_eq!((row.newest_at_ms, row.newest_status, row.lowest_uptime_1d), (o.observed_at_ms, o.status, o.uptime_1d), "{}", o.tag);
        }
    }

    #[test]
    fn a_row_serialises_in_camel_case() {
        let row = SpeedHistoryRow {
            tag: "a".into(),
            samples: 1,
            newest_at_ms: 5,
            newest_status: 0,
            median_tps: Some(1.5),
            median_latency_s: None,
            lowest_uptime_1d: Some(99.0),
        };
        let json = serde_json::to_value(&row).unwrap();
        let mut keys: Vec<_> = json.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut expected = ["tag", "samples", "newestAtMs", "newestStatus", "medianTps", "medianLatencyS", "lowestUptime1d"];
        expected.sort_unstable();
        assert_eq!(keys, expected);
        assert_eq!(json["medianLatencyS"], serde_json::Value::Null, "unknown stays null, never 0");
    }

    #[test]
    fn three_tags_observed_at_two_instants_are_two_samples() {
        let (t1, t2) = (ms("2026-10-02T09:05:00Z"), ms("2026-10-02T09:35:00Z"));
        assert_eq!(sample_count(&[obs("a", t1), obs("b", t1), obs("c", t2)]), 2);
        assert_eq!(sample_count(&[obs("a", t1), obs("a", t1)]), 1, "the same instant twice is one update");
        assert_eq!(sample_count(&[]), 0);
        assert_eq!(sample_count(&extract_observations(&golden_snapshot())), 1, "a snapshot's observations are one update");
    }

    fn entry(n: i64) -> RefreshLogEntry {
        RefreshLogEntry {
            at_ms: n * 1000,
            model: format!("vendor/model-{n}"),
            provider_name: "OpenRouter".into(),
            error: None,
            endpoints_tested: 3,
            spent_usd: 0.0123,
            estimate_usd: 0.05,
        }
    }

    fn log_of(count: i64) -> Vec<RefreshLogEntry> {
        (1..=count).fold(Vec::new(), |log, n| append_refresh_log(&log, entry(n)))
    }

    fn times(log: &[RefreshLogEntry]) -> Vec<i64> {
        log.iter().map(|e| e.at_ms / 1000).collect()
    }

    #[test]
    fn the_log_keeps_the_newest_50_with_the_new_entry_first() {
        assert_eq!(REFRESH_LOG_MAX, 50);
        assert_eq!(append_refresh_log(&[], entry(1)), [entry(1)]);
        assert_eq!(times(&append_refresh_log(&[entry(1)], entry(2))), [2, 1]);

        let full = log_of(50);
        assert_eq!(times(&full), (1..=50).rev().collect::<Vec<_>>());
        let appended = append_refresh_log(&full, entry(51));
        assert_eq!(times(&appended), (2..=51).rev().collect::<Vec<_>>(), "the oldest of the 51 goes");
        assert_eq!(appended.len(), 50);
        assert_eq!(full.len(), 50, "the log given is untouched");

        let too_long: Vec<_> = (1..=60).rev().map(entry).collect();
        assert_eq!(times(&append_refresh_log(&too_long, entry(61))), (12..=61).rev().collect::<Vec<_>>(), "a stored log over the limit is cut too");
    }

    #[test]
    fn no_stored_log_is_an_empty_one_and_text_that_isnt_entries_is_an_error() {
        assert_eq!(parse_refresh_log(None), Ok(vec![]));
        assert_eq!(parse_refresh_log(Some("[]")), Ok(vec![]));
        let one = serde_json::to_string(&[entry(1)]).unwrap();
        assert_eq!(parse_refresh_log(Some(&one)), Ok(vec![entry(1)]));

        let with_unknown_field = r#"[{"atMs":1,"model":"a/b","providerName":"P","error":null,"endpointsTested":1,"spentUsd":0,"estimateUsd":0,"bogus":1}]"#;
        let missing_field = r#"[{"atMs":1,"model":"a/b","providerName":"P","error":null,"endpointsTested":1,"spentUsd":0}]"#;
        for raw in ["nope", "{}", "null", "5", "[1]", "[{}]", with_unknown_field, missing_field] {
            assert_eq!(parse_refresh_log(Some(raw)), Err(RefreshLogError), "{raw}");
        }
        assert_eq!(RefreshLogError.to_string(), "the Refresh log is not a JSON array of entries", "the error doesn't quote the stored text");
    }

    #[test]
    fn an_appended_log_reads_back_equal_and_a_failure_keeps_its_message() {
        let failed = RefreshLogEntry { error: Some("OpenRouter refused the key.".into()), endpoints_tested: 0, spent_usd: 0.0, estimate_usd: 0.0, ..entry(4) };
        let log = append_refresh_log(&log_of(3), failed.clone());
        assert_eq!(log[0], failed);
        let text = serde_json::to_string(&log).unwrap();
        assert_eq!(parse_refresh_log(Some(&text)), Ok(log.clone()));
        assert_round_trip(&log);
    }

    #[test]
    fn an_entry_serialises_in_camel_case() {
        let json = serde_json::to_value(entry(1)).unwrap();
        let mut keys: Vec<_> = json.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut expected = ["atMs", "model", "providerName", "error", "endpointsTested", "spentUsd", "estimateUsd"];
        expected.sort_unstable();
        assert_eq!(keys, expected);
        assert_eq!(json["error"], serde_json::Value::Null, "a Refresh that worked has no error");
    }
}
