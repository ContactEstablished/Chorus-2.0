//! OpenRouter's `GET /api/v1/models/{slug}/endpoints` answer as validated rows, rows that share a
//! tag collapsed into one candidate (K8, C7), and the tag-keyed observations a snapshot adds to
//! the history. Ported from Chorus 1's `endpointsCore.ts`.
//!
//! Every output is sorted by tag in code-unit order, and rows inside a group combine with
//! order-free operations (min, max, intersection, sorted lists), so the row order of the answer
//! never changes a result.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::js::{self, by_code_unit};
use super::json::{self, Obj, Path};
use super::types::{EndpointSnapshot, Percentiles, Quantization, RawEndpoint, RawPricing, RawPricingOverride, RoutingObservation};

/// A row that failed validation, by its index in the answer.
#[derive(Debug, Clone, PartialEq)]
pub struct RejectedRow {
    pub index: usize,
    /// The row's `tag` when it is a string.
    pub tag: Option<String>,
    /// `<path>: <message>` for the first problem.
    pub issue: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EndpointsParseResult {
    pub model_id: String,
    /// The valid rows, in answer order.
    pub endpoints: Vec<RawEndpoint>,
    pub rejected: Vec<RejectedRow>,
}

/// One tag's rows as one candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct CollapsedEndpoint {
    pub tag: String,
    pub provider_name: String,
    pub rows: u32,
    pub quantization: Quantization,
    pub row_quantizations: Vec<Quantization>,
    pub mixed_quantization: bool,
    pub uptime_1d: Option<f64>,
    pub uptime_5m: Option<f64>,
    pub status: i64,
    pub context_length: u64,
    pub max_completion: Option<u64>,
    pub supported_parameters: Vec<String>,
    /// The rows' prices, sorted by their JSON text.
    pub pricing: Vec<RawPricing>,
    pub tps_p50: Option<f64>,
    pub tps_p90: Option<f64>,
    pub latency_p50_ms: Option<f64>,
    pub latency_p90_ms: Option<f64>,
}

const MAX_SAFE: i64 = 9_007_199_254_740_991;
const PRICE_PATTERN: &str = r"/^-?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?$/";

/// OpenRouter's per-token USD price text: `-?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?`.
pub fn is_price_string(s: &str) -> bool {
    let b = s.as_bytes();
    let run = |mut i: usize| {
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        i
    };
    let start = usize::from(b.first() == Some(&b'-'));
    let mut i = run(start);
    let int_digits = i - start;
    if b.get(i) == Some(&b'.') {
        let frac_start = i + 1;
        i = run(frac_start);
        if int_digits == 0 && i == frac_start {
            return false;
        }
    } else if int_digits == 0 {
        return false;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let exp_start = i;
        i = run(exp_start);
        if i == exp_start {
            return false;
        }
    }
    i == b.len()
}

fn price(v: Option<&Value>, path: &Path) -> Result<String, String> {
    let s = json::string_value(v, path, 0)?;
    if is_price_string(&s) { Ok(s) } else { Err(path.issue(&format!("Invalid string: must match pattern {PRICE_PATTERN}"))) }
}

fn optional_price(o: &Obj, key: &str, path: &Path) -> Result<Option<String>, String> {
    o.get(key).map(|v| price(Some(v), &path.key(key))).transpose()
}

fn optional_int(o: &Obj, key: &str, path: &Path, min: i64, max: i64) -> Result<Option<i64>, String> {
    o.get(key).map(|v| json::int_in(Some(v), &path.key(key), min, max)).transpose()
}

fn nullish_positive(o: &Obj, key: &str, path: &Path) -> Result<Option<u64>, String> {
    json::nullish(o, key).map(|v| json::int_in(Some(v), &path.key(key), 1, MAX_SAFE).map(|n| n as u64)).transpose()
}

fn parse_override(v: &Value, path: &Path) -> Result<RawPricingOverride, String> {
    let o = json::object(Some(v), path)?;
    Ok(RawPricingOverride {
        utc_start: optional_int(o, "utc_start", path, 0, 2400)?.map(|n| n as u32),
        utc_end: optional_int(o, "utc_end", path, 0, 2400)?.map(|n| n as u32),
        utc_days: o.get("utc_days").map(|v| json::string_array(Some(v), &path.key("utc_days"), 0)).transpose()?,
        min_prompt_tokens: optional_int(o, "min_prompt_tokens", path, 0, MAX_SAFE)?.map(|n| n as u64),
        prompt: optional_price(o, "prompt", path)?,
        completion: optional_price(o, "completion", path)?,
        input_cache_read: optional_price(o, "input_cache_read", path)?,
        input_cache_write: optional_price(o, "input_cache_write", path)?,
    })
}

fn parse_pricing(v: Option<&Value>, path: &Path) -> Result<RawPricing, String> {
    let o = json::object(v, path)?;
    let overrides = match o.get("overrides") {
        None => None,
        Some(list) => {
            let p = path.key("overrides");
            Some(json::array(Some(list), &p)?.iter().enumerate().map(|(i, item)| parse_override(item, &p.key(i))).collect::<Result<_, _>>()?)
        }
    };
    Ok(RawPricing {
        prompt: price(o.get("prompt"), &path.key("prompt"))?,
        completion: price(o.get("completion"), &path.key("completion"))?,
        input_cache_read: optional_price(o, "input_cache_read", path)?,
        input_cache_write: optional_price(o, "input_cache_write", path)?,
        discount: json::optional_number(o, "discount", path)?,
        overrides,
    })
}

fn parse_percentiles(o: &Obj, key: &str, path: &Path) -> Result<Option<Percentiles>, String> {
    let Some(v) = json::nullish(o, key) else { return Ok(None) };
    let p = path.key(key);
    let inner = json::object(Some(v), &p)?;
    Ok(Some(Percentiles { p50: json::nullish_number(inner, "p50", &p)?, p90: json::nullish_number(inner, "p90", &p)? }))
}

fn parse_tool_choice(o: &Obj, path: &Path) -> Result<Option<BTreeMap<String, bool>>, String> {
    let Some(v) = json::nullish(o, "supports_tool_choice") else { return Ok(None) };
    let p = path.key("supports_tool_choice");
    let record = json::object(Some(v), &p)?;
    record.iter().map(|(k, b)| Ok((k.clone(), json::bool_value(Some(b), &p.key(k))?))).collect::<Result<_, String>>().map(Some)
}

/// One row, checked field by field in Chorus 1's schema order; the error is the first problem
/// as `<path>: <message>`. Unknown fields are dropped.
pub fn parse_endpoint_row(row: &Value) -> Result<RawEndpoint, String> {
    let root = Path::default();
    let o = json::object(Some(row), &root)?;
    Ok(RawEndpoint {
        tag: json::string(o, "tag", &root, 1)?,
        provider_name: json::string(o, "provider_name", &root, 1)?,
        quantization: json::nullish_string(o, "quantization", &root)?,
        context_length: json::int_in(o.get("context_length"), &root.key("context_length"), 1, MAX_SAFE)? as u64,
        max_completion_tokens: nullish_positive(o, "max_completion_tokens", &root)?,
        max_prompt_tokens: nullish_positive(o, "max_prompt_tokens", &root)?,
        pricing: parse_pricing(o.get("pricing"), &root.key("pricing"))?,
        supported_parameters: json::string_array(o.get("supported_parameters"), &root.key("supported_parameters"), 0)?,
        supports_tool_choice: parse_tool_choice(o, &root)?,
        status: json::int_value(o.get("status"), &root.key("status"))?,
        uptime_last_1d: json::nullish_number(o, "uptime_last_1d", &root)?,
        uptime_last_5m: json::nullish_number(o, "uptime_last_5m", &root)?,
        uptime_last_30m: json::nullish_number(o, "uptime_last_30m", &root)?,
        supports_implicit_caching: json::nullish(o, "supports_implicit_caching")
            .map(|v| json::bool_value(Some(v), &root.key("supports_implicit_caching")))
            .transpose()?,
        throughput_last_30m: parse_percentiles(o, "throughput_last_30m", &root)?,
        latency_last_30m: parse_percentiles(o, "latency_last_30m", &root)?,
    })
}

fn envelope(json: &Value) -> Result<(String, &Vec<Value>), String> {
    let root = Path::default();
    let data_path = root.key("data");
    let data = json::object(json::object(Some(json), &root)?.get("data"), &data_path)?;
    let id = json::string(data, "id", &data_path, 1)?;
    Ok((id, json::array(data.get("endpoints"), &data_path.key("endpoints"))?))
}

/// Checks the envelope (an error when it is wrong), then each row on its own: a failing row is
/// reported in `rejected` without dropping the others.
pub fn parse_endpoints_response(json: &Value) -> Result<EndpointsParseResult, String> {
    let (model_id, rows) = envelope(json).map_err(|e| format!("Invalid endpoints response: {e}"))?;
    let mut endpoints = Vec::new();
    let mut rejected = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        match parse_endpoint_row(row) {
            Ok(r) => endpoints.push(r),
            Err(issue) => rejected.push(RejectedRow { index, tag: row.get("tag").and_then(Value::as_str).map(str::to_owned), issue }),
        }
    }
    Ok(EndpointsParseResult { model_id, endpoints, rejected })
}

// ── C7 collapse primitives, shared with the history merge ──

/// C7 minimum: `None` when any value is missing, and for no values; -0 comes back as 0.
pub fn min_or_null(values: impl IntoIterator<Item = Option<f64>>) -> Option<f64> {
    let mut out: Option<f64> = None;
    for v in values {
        let v = v?;
        out = Some(out.map_or(v, |o| js::min(o, v)));
    }
    out.map(|x| x + 0.0)
}

/// C7 maximum: `None` when any value is missing, and for no values; -0 comes back as 0.
pub fn max_or_null(values: impl IntoIterator<Item = Option<f64>>) -> Option<f64> {
    let mut out: Option<f64> = None;
    for v in values {
        let v = v?;
        out = Some(out.map_or(v, |o| js::max(o, v)));
    }
    out.map(|x| x + 0.0)
}

/// K8/C7 status: 0 when every status is 0 (and for none), else the smallest non-zero value.
pub fn collapse_status(statuses: impl IntoIterator<Item = i64>) -> i64 {
    statuses.into_iter().fold(0, |out, s| if s != 0 && (out == 0 || s < out) { s } else { out })
}

/// The distinct quantizations a set of rows declares, in `QUANTIZATIONS` order.
fn declared_quantizations(rows: &[&RawEndpoint]) -> Vec<Quantization> {
    let declared: BTreeSet<Quantization> = rows.iter().map(|r| Quantization::normalize(r.quantization.as_deref())).collect();
    declared.into_iter().collect()
}

fn collapse_group(tag: &str, rows: &[&RawEndpoint]) -> CollapsedEndpoint {
    let row_quantizations = declared_quantizations(rows);
    let mixed_quantization = row_quantizations.len() > 1;
    let first_params: Vec<&String> = {
        let mut seen = BTreeSet::new();
        rows[0].supported_parameters.iter().filter(|p| seen.insert(p.as_str())).collect()
    };
    let mut supported_parameters: Vec<String> = first_params
        .into_iter()
        .filter(|p| rows.iter().all(|r| r.supported_parameters.contains(p)))
        .cloned()
        .collect();
    supported_parameters.sort_by(|a, b| by_code_unit(a, b));
    let mut pricing: Vec<(String, RawPricing)> =
        rows.iter().map(|r| (serde_json::to_string(&r.pricing).unwrap_or_default(), r.pricing.clone())).collect();
    pricing.sort_by(|a, b| by_code_unit(&a.0, &b.0));
    let mut names: Vec<&str> = rows.iter().map(|r| r.provider_name.as_str()).collect();
    names.sort_by(|a, b| by_code_unit(a, b));
    let throughput = |pick: fn(&Percentiles) -> Option<f64>| rows.iter().map(move |r| r.throughput_last_30m.as_ref().and_then(pick));
    let latency = |pick: fn(&Percentiles) -> Option<f64>| rows.iter().map(move |r| r.latency_last_30m.as_ref().and_then(pick));
    CollapsedEndpoint {
        tag: tag.to_owned(),
        provider_name: names[0].to_owned(),
        rows: rows.len() as u32,
        quantization: if mixed_quantization { Quantization::Unknown } else { row_quantizations[0] },
        row_quantizations,
        mixed_quantization,
        uptime_1d: min_or_null(rows.iter().map(|r| r.uptime_last_1d)),
        uptime_5m: min_or_null(rows.iter().map(|r| r.uptime_last_5m)),
        status: collapse_status(rows.iter().map(|r| r.status)),
        context_length: rows.iter().map(|r| r.context_length).min().unwrap_or(0),
        max_completion: rows.iter().filter_map(|r| r.max_completion_tokens).min(),
        supported_parameters,
        pricing: pricing.into_iter().map(|(_, p)| p).collect(),
        tps_p50: min_or_null(throughput(|p| p.p50)),
        tps_p90: min_or_null(throughput(|p| p.p90)),
        latency_p50_ms: max_or_null(latency(|p| p.p50)),
        latency_p90_ms: max_or_null(latency(|p| p.p90)),
    }
}

/// Rows sharing a tag become one candidate (K8, C7). Sorted by tag.
pub fn collapse_endpoints(endpoints: &[RawEndpoint]) -> Vec<CollapsedEndpoint> {
    let mut groups: BTreeMap<&str, Vec<&RawEndpoint>> = BTreeMap::new();
    for row in endpoints {
        groups.entry(row.tag.as_str()).or_default().push(row);
    }
    let mut tags: Vec<&str> = groups.keys().copied().collect();
    tags.sort_by(|a, b| by_code_unit(a, b));
    tags.into_iter().filter_map(|tag| groups.get(tag).map(|rows| collapse_group(tag, rows))).collect()
}

/// One observation per tag at the snapshot's fetch time, sorted by tag.
pub fn extract_observations(snapshot: &EndpointSnapshot) -> Vec<RoutingObservation> {
    collapse_endpoints(&snapshot.endpoints)
        .into_iter()
        .map(|c| RoutingObservation {
            tag: c.tag,
            observed_at_ms: snapshot.fetched_at_ms,
            uptime_1d: c.uptime_1d,
            uptime_5m: c.uptime_5m,
            status: c.status,
            tps_p50: c.tps_p50,
            tps_p90: c.tps_p90,
            latency_p50_ms: c.latency_p50_ms,
            latency_p90_ms: c.latency_p90_ms,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::test_support::*;
    use serde_json::json;

    /// Table E's minimal valid row; tests name only the fields they mean.
    fn row() -> RawEndpoint {
        RawEndpoint {
            tag: "x/y".into(),
            provider_name: "X".into(),
            context_length: 262_144,
            max_completion_tokens: Some(65_536),
            uptime_last_5m: Some(100.0),
            throughput_last_30m: pct(Some(80.0), Some(120.0)),
            latency_last_30m: pct(Some(900.0), Some(1800.0)),
            ..base_row("x/y")
        }
    }

    fn one(rows: &[RawEndpoint]) -> CollapsedEndpoint {
        let mut out = collapse_endpoints(rows);
        assert_eq!(out.len(), 1);
        out.remove(0)
    }

    #[test]
    fn e1_the_fixture_parses_to_33_rows_with_no_rejections() {
        let fixture = fixture_json();
        let parsed = parse_endpoints_response(&fixture).unwrap();
        assert_eq!(parsed.model_id, "deepseek/deepseek-v4.1-flash");
        assert_eq!(parsed.endpoints.len(), 33);
        assert!(parsed.rejected.is_empty());
        let tags: Vec<&str> = fixture["data"]["endpoints"].as_array().unwrap().iter().map(|e| e["tag"].as_str().unwrap()).collect();
        assert_eq!(parsed.endpoints.iter().map(|e| e.tag.as_str()).collect::<Vec<_>>(), tags, "valid rows keep input order");
    }

    #[test]
    fn e2_a_malformed_row_is_rejected_by_index_without_dropping_the_others() {
        let mut broken = fixture_json();
        broken["data"]["endpoints"][5].as_object_mut().unwrap().remove("tag");
        let result = parse_endpoints_response(&broken).unwrap();
        assert_eq!(result.endpoints.len(), 32);
        assert_eq!(result.rejected.len(), 1);
        assert_eq!((result.rejected[0].index, result.rejected[0].tag.as_deref()), (5, None));
        assert!(result.rejected[0].issue.starts_with("tag: "), "{}", result.rejected[0].issue);
        assert!(!result.endpoints.iter().any(|e| e.tag == "sail-research/fp4"));
    }

    #[test]
    fn e2_a_rejected_row_whose_tag_is_a_string_reports_that_tag() {
        let mut broken = fixture_json();
        broken["data"]["endpoints"][10]["pricing"]["prompt"] = json!("abc");
        let result = parse_endpoints_response(&broken).unwrap();
        assert_eq!(result.endpoints.len(), 32);
        assert_eq!(result.rejected.len(), 1);
        let r = &result.rejected[0];
        assert_eq!((r.index, r.tag.as_deref()), (10, Some("deepinfra/fp8")));
        assert!(r.issue.starts_with("pricing.prompt: "), "{}", r.issue);
    }

    #[test]
    fn e3_an_invalid_envelope_is_an_error() {
        for bad in [json!({}), json!({ "data": { "id": "x" } }), json!(null)] {
            let err = parse_endpoints_response(&bad).unwrap_err();
            assert!(err.starts_with("Invalid endpoints response: "), "{err}");
        }
    }

    #[test]
    fn row_checks_follow_the_schema() {
        let valid = serde_json::to_value(row()).unwrap();
        assert_eq!(parse_endpoint_row(&valid).unwrap(), row());
        let with = |key: &str, value: Value| {
            let mut v = valid.clone();
            v[key] = value;
            parse_endpoint_row(&v).unwrap_err()
        };
        assert!(with("context_length", json!(0)).starts_with("context_length: Too small"));
        assert!(with("context_length", json!(1.5)).starts_with("context_length: Invalid input: expected int"));
        assert!(with("status", json!("0")).starts_with("status: Invalid input: expected number"));
        assert!(with("supported_parameters", json!(["tools", 3])).starts_with("supported_parameters.1: "));
        assert!(with("throughput_last_30m", json!({ "p50": "fast" })).starts_with("throughput_last_30m.p50: "));
        let mut no_cache = valid.clone();
        no_cache["pricing"]["input_cache_read"] = Value::Null;
        assert!(parse_endpoint_row(&no_cache).unwrap_err().starts_with("pricing.input_cache_read: "), "optional is not nullable");
        let mut ovr = valid.clone();
        ovr["pricing"]["overrides"] = json!([{ "utc_start": 2401 }]);
        assert!(parse_endpoint_row(&ovr).unwrap_err().starts_with("pricing.overrides.0.utc_start: Too big"));
        // Nullish fields take null; whole-number floats are integers; unknown keys are dropped.
        let mut lenient = valid.clone();
        lenient["quantization"] = Value::Null;
        lenient["max_completion_tokens"] = Value::Null;
        lenient["context_length"] = json!(262_144.0);
        lenient["extra"] = json!({ "anything": true });
        let parsed = parse_endpoint_row(&lenient).unwrap();
        assert_eq!((parsed.quantization, parsed.max_completion_tokens, parsed.context_length), (None, None, 262_144));
        assert!(parse_endpoint_row(&json!([1])).unwrap_err().starts_with("(root): "));
    }

    #[test]
    fn price_strings_match_chorus_1s_pattern() {
        for good in ["0.0000003", "3e-7", "1.", ".5", "-0", "1E+5", "0"] {
            assert!(is_price_string(good), "{good}");
        }
        for bad in ["", ".", "-", "abc", "1e", "1.2.3", " 1", "+1", "1e+"] {
            assert!(!is_price_string(bad), "{bad}");
        }
    }

    #[test]
    fn e5_the_fixture_collapses_to_32_groups_and_baseten_fp8_combines_its_two_rows() {
        let collapsed = collapse_endpoints(&golden_endpoints());
        assert_eq!(collapsed.len(), 32);
        let baseten = collapsed.iter().find(|c| c.tag == "baseten/fp8").unwrap();
        assert_eq!(baseten.rows, 2);
        assert_eq!(baseten.provider_name, "BaseTen");
        assert_eq!(baseten.quantization, Quantization::Fp8);
        assert!(!baseten.mixed_quantization);
        assert_eq!(baseten.row_quantizations, [Quantization::Fp8]);
        assert_eq!(baseten.tps_p50, Some(62.0));
        assert_eq!(baseten.tps_p90, Some(293.0));
        assert_eq!(baseten.latency_p50_ms, Some(305.0));
        assert_same(baseten.latency_p90_ms, 1456.5000000000027);
        assert_same(baseten.uptime_1d, 99.92966632878922);
        assert_eq!(baseten.uptime_5m, Some(99.2));
        assert_eq!(baseten.status, 0);
        assert_eq!(baseten.pricing.len(), 2);
        assert!(collapsed.iter().filter(|c| c.tag != "baseten/fp8").all(|c| c.rows == 1), "single-row groups pass through");
    }

    #[test]
    fn e5_groups_are_sorted_by_tag_in_code_unit_order() {
        let tags: Vec<String> = collapse_endpoints(&golden_endpoints()).into_iter().map(|c| c.tag).collect();
        let mut sorted = tags.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(tags, sorted);
        let mixed: Vec<String> = collapse_endpoints(&[RawEndpoint { tag: "a/x".into(), ..row() }, RawEndpoint { tag: "B/x".into(), ..row() }])
            .into_iter()
            .map(|c| c.tag)
            .collect();
        assert_eq!(mixed, ["B/x", "a/x"]);
    }

    #[test]
    fn e6_mixed_quantization_becomes_unknown_and_lists_rows_in_quantizations_order() {
        let c = one(&[RawEndpoint { quantization: Some("fp16".into()), ..row() }, RawEndpoint { quantization: Some("fp8".into()), ..row() }]);
        assert_eq!(c.quantization, Quantization::Unknown);
        assert!(c.mixed_quantization);
        assert_eq!(c.row_quantizations, [Quantization::Fp8, Quantization::Fp16]);
        let undeclared = one(&[RawEndpoint { quantization: Some("fp8".into()), ..row() }, RawEndpoint { quantization: None, ..row() }]);
        assert_eq!(undeclared.quantization, Quantization::Unknown);
        assert!(undeclared.mixed_quantization);
        assert_eq!(undeclared.row_quantizations, [Quantization::Fp8, Quantization::Unknown]);
    }

    #[test]
    fn e7_any_non_zero_status_wins() {
        let status = |s: &[i64]| one(&s.iter().map(|&status| RawEndpoint { status, ..row() }).collect::<Vec<_>>()).status;
        assert_eq!(status(&[0, -2]), -2);
        assert_eq!(status(&[1, -2, 0]), -2);
        assert_eq!(status(&[0, 0]), 0);
    }

    #[test]
    fn e8_a_missing_uptime_or_speed_in_any_row_makes_the_tag_value_missing() {
        let uptime = one(&[RawEndpoint { uptime_last_1d: None, ..row() }, RawEndpoint { uptime_last_1d: Some(99.9), ..row() }]);
        assert_eq!((uptime.uptime_1d, uptime.uptime_5m), (None, Some(100.0)));
        let speed = one(&[RawEndpoint { throughput_last_30m: None, ..row() }, row()]);
        assert_eq!((speed.tps_p50, speed.tps_p90, speed.latency_p50_ms), (None, None, Some(900.0)));
        let partial = one(&[RawEndpoint { latency_last_30m: pct(Some(700.0), None), ..row() }, row()]);
        assert_eq!((partial.latency_p50_ms, partial.latency_p90_ms), (Some(900.0), None));
    }

    #[test]
    fn e8_otherwise_uptime_and_tps_take_the_minimum_latency_the_maximum_context_the_minimum() {
        let c = one(&[
            RawEndpoint {
                uptime_last_1d: Some(99.7),
                uptime_last_5m: Some(98.0),
                throughput_last_30m: pct(Some(50.0), Some(200.0)),
                latency_last_30m: pct(Some(400.0), Some(900.0)),
                context_length: 300_000,
                ..row()
            },
            RawEndpoint {
                uptime_last_1d: Some(99.9),
                uptime_last_5m: Some(99.0),
                throughput_last_30m: pct(Some(70.0), Some(100.0)),
                latency_last_30m: pct(Some(300.0), Some(1200.0)),
                context_length: 262_144,
                ..row()
            },
        ]);
        assert_eq!((c.uptime_1d, c.uptime_5m, c.tps_p50, c.tps_p90), (Some(99.7), Some(98.0), Some(50.0), Some(100.0)));
        assert_eq!((c.latency_p50_ms, c.latency_p90_ms, c.context_length), (Some(400.0), Some(1200.0), 262_144));
    }

    #[test]
    fn e9_supported_parameters_are_intersected_and_sorted() {
        let params = |p: &[&str]| p.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let c = one(&[
            RawEndpoint { supported_parameters: params(&["tools", "tool_choice", "reasoning"]), ..row() },
            RawEndpoint { supported_parameters: params(&["tools", "reasoning", "max_tokens"]), ..row() },
        ]);
        assert_eq!(c.supported_parameters, ["reasoning", "tools"]);
    }

    #[test]
    fn e10_a_missing_max_completion_is_ignored_and_all_missing_stays_missing() {
        let max = |a: Option<u64>, b: Option<u64>| {
            one(&[RawEndpoint { max_completion_tokens: a, ..row() }, RawEndpoint { max_completion_tokens: b, ..row() }]).max_completion
        };
        assert_eq!(max(None, Some(131_072)), Some(131_072));
        assert_eq!(max(None, None), None);
        assert_eq!(max(Some(200_000), Some(131_072)), Some(131_072));
    }

    #[test]
    fn e11_reversed_and_rotated_rows_give_equal_collapse_and_observations() {
        let endpoints = golden_endpoints();
        let at = fixture_fetched_at_ms();
        let base = collapse_endpoints(&endpoints);
        let base_obs = extract_observations(&EndpointSnapshot { fetched_at_ms: at, endpoints: endpoints.clone() });
        let reversed: Vec<_> = endpoints.iter().rev().cloned().collect();
        let rotate = |n: usize| [&endpoints[n..], &endpoints[..n]].concat();
        // Rotating by 26 puts the second baseten/fp8 row first.
        for shuffled in [reversed, rotate(13), rotate(26)] {
            assert_eq!(collapse_endpoints(&shuffled), base);
            assert_eq!(extract_observations(&EndpointSnapshot { fetched_at_ms: at, endpoints: shuffled }), base_obs);
        }
    }

    #[test]
    fn e11_a_group_with_differing_provider_names_and_pricing_is_order_free() {
        let zeta = RawEndpoint {
            provider_name: "Zeta".into(),
            pricing: pricing("0.0000003", "0.0000012", Some("0.000000006")),
            uptime_last_1d: Some(99.7),
            throughput_last_30m: pct(Some(70.0), Some(110.0)),
            latency_last_30m: pct(Some(600.0), Some(1500.0)),
            ..row()
        };
        let alpha = RawEndpoint {
            provider_name: "Alpha".into(),
            pricing: pricing("0.0000001", "0.0000004", None),
            uptime_last_1d: Some(99.9),
            throughput_last_30m: pct(Some(90.0), Some(100.0)),
            latency_last_30m: pct(Some(400.0), Some(1800.0)),
            ..row()
        };
        let forward = one(&[zeta.clone(), alpha.clone()]);
        assert_eq!(forward.provider_name, "Alpha");
        assert_eq!(forward.pricing, [alpha.pricing.clone(), zeta.pricing.clone()], "alpha's JSON sorts first");
        assert_eq!((forward.uptime_1d, forward.tps_p50, forward.tps_p90), (Some(99.7), Some(70.0), Some(100.0)));
        assert_eq!((forward.latency_p50_ms, forward.latency_p90_ms), (Some(600.0), Some(1800.0)));
        assert_eq!(collapse_endpoints(&[alpha.clone(), zeta.clone()]), collapse_endpoints(&[zeta.clone(), alpha.clone()]));
        let obs = |rows: Vec<RawEndpoint>| extract_observations(&EndpointSnapshot { fetched_at_ms: 0, endpoints: rows });
        assert_eq!(obs(vec![alpha.clone(), zeta.clone()]), obs(vec![zeta, alpha]));
    }

    #[test]
    fn e12_32_observations_at_fetch_time_sorted_by_tag() {
        let at = fixture_fetched_at_ms();
        let obs = extract_observations(&EndpointSnapshot { fetched_at_ms: at, endpoints: golden_endpoints() });
        assert_eq!(obs.len(), 32);
        assert!(obs.iter().all(|o| o.observed_at_ms == ms("2026-10-02T09:05:00Z")));
        let tags: Vec<&str> = obs.iter().map(|o| o.tag.as_str()).collect();
        let mut sorted = tags.clone();
        sorted.sort();
        assert_eq!(tags, sorted);
        let baseten = obs.iter().find(|o| o.tag == "baseten/fp8").unwrap();
        assert_eq!((baseten.status, baseten.uptime_5m, baseten.tps_p50, baseten.tps_p90), (0, Some(99.2), Some(62.0), Some(293.0)));
        assert_eq!(baseten.latency_p50_ms, Some(305.0));
        assert_same(baseten.uptime_1d, 99.92966632878922);
        assert_same(baseten.latency_p90_ms, 1456.5000000000027);
        assert_round_trip(&obs);
    }

    #[test]
    fn a_negative_zero_collapses_to_zero() {
        let c = one(&[RawEndpoint { uptime_last_5m: Some(-0.0), throughput_last_30m: pct(Some(-0.0), Some(1.0)), ..row() }, row()]);
        assert!(c.uptime_5m.unwrap().is_sign_positive() && c.uptime_5m == Some(0.0));
        assert!(c.tps_p50.unwrap().is_sign_positive() && c.tps_p50 == Some(0.0));
        let obs = extract_observations(&EndpointSnapshot { fetched_at_ms: 0, endpoints: vec![RawEndpoint { latency_last_30m: pct(Some(-0.0), Some(-0.0)), ..row() }] });
        assert!(obs[0].latency_p50_ms.unwrap().is_sign_positive() && obs[0].latency_p90_ms.unwrap().is_sign_positive());
    }

    #[test]
    fn min_or_null_and_max_or_null() {
        assert_eq!(min_or_null([Some(3.0), Some(1.0), Some(2.0)]), Some(1.0));
        assert_eq!(min_or_null([Some(5.0)]), Some(5.0));
        assert_eq!(min_or_null([Some(3.0), None, Some(1.0)]), None);
        assert_eq!(min_or_null([]), None);
        assert!(min_or_null([Some(-0.0)]).unwrap().is_sign_positive());
        assert!(min_or_null([Some(0.0), Some(-0.0)]).unwrap().is_sign_positive());
        assert_eq!(max_or_null([Some(3.0), Some(1.0), Some(2.0)]), Some(3.0));
        assert_eq!(max_or_null([None, Some(3.0)]), None);
        assert_eq!(max_or_null([]), None);
        assert!(max_or_null([Some(-0.0), Some(-1.0)]).unwrap().is_sign_positive());
    }

    #[test]
    fn collapse_status_rules() {
        assert_eq!(collapse_status([0, 0]), 0);
        assert_eq!(collapse_status([]), 0);
        assert_eq!(collapse_status([0, -2]), -2);
        assert_eq!(collapse_status([-2, 0]), -2);
        assert_eq!(collapse_status([1, 0, -2]), -2);
        assert_eq!(collapse_status([3, 1]), 1);
    }

    #[test]
    fn a_stored_snapshot_reads_back_through_serde() {
        let snapshot = EndpointSnapshot { fetched_at_ms: fixture_fetched_at_ms(), endpoints: golden_endpoints() };
        let text = serde_json::to_string(&snapshot).unwrap();
        assert_eq!(serde_json::from_str::<EndpointSnapshot>(&text).unwrap(), snapshot);
        let rows: Value = serde_json::from_str(&text).unwrap();
        for (i, r) in rows["endpoints"].as_array().unwrap().iter().enumerate() {
            assert_eq!(parse_endpoint_row(r).as_ref(), Ok(&snapshot.endpoints[i]), "row {i} passes the strict check too");
        }
    }
}
