//! Fixtures and helpers shared by the routing tests: Chorus 1's golden 2026-10-02 endpoints
//! answer for DeepSeek V4.1 Flash, row builders, and its deterministic shuffle.

use serde_json::Value;

use super::endpoints::{extract_observations, parse_endpoints_response};
use super::registry::{bundled_model_registry, find_model};
use super::time::parse_utc_instant;
use super::types::*;

pub const ENDPOINTS_FIXTURE: &str = include_str!("fixtures/endpoints-deepseek-v4.1-flash-2026-10-02.json");
pub const PREFLIGHT_GUARDRAILS_FIXTURE: &str = include_str!("fixtures/preflight-guardrails-2026-10-02.json");
pub const PREFLIGHT_DATA_POLICY_FIXTURE: &str = include_str!("fixtures/preflight-data-policy-2026-10-02.json");

pub const SLUG: &str = "deepseek/deepseek-v4.1-flash";
pub const NOW: &str = "2026-10-02T09:20:00Z";
pub const FETCHED_AT: &str = "2026-10-02T09:05:00Z";
pub const CHECKED_AT: &str = "2026-10-02T09:15:39Z";

/// Unix milliseconds of a UTC ISO instant.
pub fn ms(iso: &str) -> i64 {
    parse_utc_instant(iso).unwrap_or_else(|| panic!("not an instant: {iso}"))
}

pub fn fixture_json() -> Value {
    serde_json::from_str(ENDPOINTS_FIXTURE).unwrap()
}

pub fn fixture_fetched_at_ms() -> i64 {
    ms(fixture_json()["fetchedAt"].as_str().unwrap())
}

pub fn golden_endpoints() -> Vec<RawEndpoint> {
    parse_endpoints_response(&fixture_json()).unwrap().endpoints
}

pub fn golden_snapshot() -> EndpointSnapshot {
    EndpointSnapshot { fetched_at_ms: fixture_fetched_at_ms(), endpoints: golden_endpoints() }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_owned()).collect()
}

/// The fixture's account facts: guardrail and data-policy removals, checked at [`CHECKED_AT`].
pub fn golden_account() -> AccountEligibility {
    let f = fixture_json();
    AccountEligibility {
        guardrail_removed: Some(strings(&f["guardrailRemoved"])),
        data_policy_removed: Some(strings(&f["dataPolicyDenyRemoved"])),
        checked_at_ms: Some(ms(CHECKED_AT)),
    }
}

/// The fixture's `cacheVerified`, each record checked at `checked_at_ms`.
pub fn golden_cache(checked_at_ms: i64) -> CacheVerification {
    let f = fixture_json();
    f["cacheVerified"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(tag, v)| (tag.clone(), CacheRecord { verified: v.as_bool().unwrap(), checked_at_ms }))
        .collect()
}

pub fn bundled_model() -> ModelRegistryEntry {
    find_model(&bundled_model_registry().unwrap(), SLUG).unwrap().clone()
}

pub fn pct(p50: Option<f64>, p90: Option<f64>) -> Option<Percentiles> {
    Some(Percentiles { p50, p90 })
}

pub fn pricing(prompt: &str, completion: &str, cache_read: Option<&str>) -> RawPricing {
    RawPricing {
        prompt: prompt.into(),
        completion: completion.into(),
        input_cache_read: cache_read.map(str::to_owned),
        ..RawPricing::default()
    }
}

/// The ranker tests' valid fp8 row: 100 tok/s, 500 ms, $0.1/M prompt, $0.4/M completion, $0.01/M
/// cache read.
pub fn base_row(tag: &str) -> RawEndpoint {
    RawEndpoint {
        tag: tag.into(),
        provider_name: "Provider".into(),
        quantization: Some("fp8".into()),
        context_length: 1_048_576,
        max_completion_tokens: Some(131_072),
        pricing: pricing("0.0000001", "0.0000004", Some("0.00000001")),
        supported_parameters: ["tools", "tool_choice", "max_tokens", "reasoning"].map(str::to_owned).to_vec(),
        status: 0,
        uptime_last_1d: Some(99.9),
        uptime_last_5m: Some(99.9),
        throughput_last_30m: pct(Some(100.0), Some(150.0)),
        latency_last_30m: pct(Some(500.0), Some(900.0)),
        ..RawEndpoint::default()
    }
}

/// A rank input's owned parts.
#[derive(Debug, Clone)]
pub struct Inputs {
    pub model: ModelRegistryEntry,
    pub snapshot: EndpointSnapshot,
    pub history: Vec<RoutingObservation>,
    pub account: AccountEligibility,
    pub cache: CacheVerification,
    pub profile: RoutingProfileId,
    pub effort: Option<String>,
    pub settings: RoutingSettings,
    pub now_ms: i64,
}

impl Inputs {
    /// Chorus 1's golden input: the fixture, its own observations, its account and cache facts.
    pub fn golden(profile: RoutingProfileId) -> Self {
        let snapshot = golden_snapshot();
        Self {
            model: bundled_model(),
            history: extract_observations(&snapshot),
            snapshot,
            account: golden_account(),
            cache: golden_cache(ms(CHECKED_AT)),
            profile,
            effort: Some("low".into()),
            settings: RoutingSettings::default(),
            now_ms: ms(NOW),
        }
    }

    /// `rows` in a snapshot 15 minutes old, history its own observations, a clean account and
    /// every tag cache-verified.
    pub fn synthetic(rows: Vec<RawEndpoint>) -> Self {
        let snapshot = EndpointSnapshot { fetched_at_ms: ms(FETCHED_AT), endpoints: rows };
        let checked = ms(CHECKED_AT);
        Self {
            model: bundled_model(),
            history: extract_observations(&snapshot),
            cache: snapshot.endpoints.iter().map(|r| (r.tag.clone(), CacheRecord { verified: true, checked_at_ms: checked })).collect(),
            snapshot,
            account: AccountEligibility { guardrail_removed: Some(vec![]), data_policy_removed: Some(vec![]), checked_at_ms: Some(checked) },
            profile: RoutingProfileId::Interactive,
            effort: Some("low".into()),
            settings: RoutingSettings::default(),
            now_ms: ms(NOW),
        }
    }

    pub fn input(&self) -> RankInput<'_> {
        RankInput {
            model: &self.model,
            snapshot: &self.snapshot,
            history: &self.history,
            account: &self.account,
            cache: &self.cache,
            profile: self.profile,
            effort: self.effort.as_deref(),
            settings: &self.settings,
            now_ms: self.now_ms,
        }
    }
}

/// Relative tolerance 1e-4, as Chorus 1's specification prescribes.
#[track_caller]
pub fn close(actual: Option<f64>, expected: f64) {
    let a = actual.unwrap_or_else(|| panic!("expected a number near {expected}, got None"));
    assert!((a / expected - 1.0).abs() < 1e-4, "{a} vs {expected}");
}

/// Exactly the double JavaScript reads from the same JSON text (the workspace enables serde_json's
/// `float_roundtrip`, so its parser agrees with `JSON.parse`).
#[track_caller]
pub fn assert_same(actual: Option<f64>, expected: f64) {
    let a = actual.unwrap_or_else(|| panic!("expected {expected}, got None"));
    assert!(a.to_bits() == expected.to_bits(), "{a} vs {expected}");
}

/// Chorus 1's K2 check, "every result is plain JSON": `v` serialises and reads back with the same
/// shape, strings, flags and numbers (exactly: `float_roundtrip` is on), and holds no -0.
#[track_caller]
pub fn assert_round_trip<T: serde::Serialize + serde::de::DeserializeOwned>(v: &T) {
    let text = serde_json::to_string(v).unwrap();
    let back: T = serde_json::from_str(&text).unwrap();
    json_close(&serde_json::to_value(v).unwrap(), &serde_json::to_value(&back).unwrap(), "$");
}

#[track_caller]
fn json_close(a: &Value, b: &Value, path: &str) {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) if x.is_f64() || y.is_f64() => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            assert!(!(x == 0.0 && x.is_sign_negative()), "{path} is -0");
            assert!(x.to_bits() == y.to_bits(), "{path}: {x} vs {y}");
        }
        (Value::Array(x), Value::Array(y)) => {
            assert_eq!(x.len(), y.len(), "{path}");
            for (i, (x, y)) in x.iter().zip(y).enumerate() {
                json_close(x, y, &format!("{path}[{i}]"));
            }
        }
        (Value::Object(x), Value::Object(y)) => {
            assert_eq!(x.keys().collect::<Vec<_>>(), y.keys().collect::<Vec<_>>(), "{path}");
            for (k, x) in x {
                json_close(x, &y[k], &format!("{path}.{k}"));
            }
        }
        _ => assert_eq!(a, b, "{path}"),
    }
}

/// Chorus 1's deterministic Fisher-Yates with a small LCG, so a failing shuffle reproduces.
pub fn shuffled<T: Clone>(items: &[T], seed: u32) -> Vec<T> {
    let mut out = items.to_vec();
    let mut s = seed;
    for i in (1..out.len()).rev() {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let j = s as usize % (i + 1);
        out.swap(i, j);
    }
    out
}

// ── Golden expectations (Chorus 1's ImplementationSpec-1-2 and 1-3; never edited to fit the code) ──

/// The 14 eligible tags of the golden run, sorted.
pub const GOLDEN_ELIGIBLE: [&str; 14] = [
    "atlas-cloud/fp8",
    "baidu/fp8",
    "baseten/fast",
    "baseten/fp8",
    "deepinfra/fp8",
    "gmicloud/fp8",
    "makora/fp8",
    "morph/fp8",
    "nextbit/fp8",
    "novita/fp8",
    "parasail/fp8",
    "siliconflow/fp8",
    "streamlake/fp8",
    "venice/fp8",
];

/// The golden run's 18 excluded tags and their reasons, sorted by tag.
pub fn golden_exclusions() -> Vec<(&'static str, Vec<&'static str>)> {
    let undeclared = || vec!["quantization not declared"];
    vec![
        ("alibaba", vec!["uptime 97.18% < 99.5%", "currently degraded (5m uptime 92.3%)", "status -2", "quantization not declared"]),
        ("coreweave/fp8", vec!["uptime 98.34% < 99.5%"]),
        ("decart/fp4", vec!["fp4 below native fp8"]),
        ("deepseek", vec!["uptime 99.34% < 99.5%", "removed by account guardrail", "removed by data policy"]),
        ("dekallm", vec!["uptime 99.18% < 99.5%", "quantization not declared"]),
        ("digitalocean", undeclared()),
        ("fireworks", undeclared()),
        ("fireworks/us", undeclared()),
        ("inference-net", vec!["uptime 99.49% < 99.5%", "quantization not declared"]),
        ("io-net/fp8", vec!["uptime 98.22% < 99.5%"]),
        ("ionstream", undeclared()),
        ("modal", undeclared()),
        ("open-inference/fp4", vec!["uptime 99.28% < 99.5%", "fp4 below native fp8"]),
        ("phala", undeclared()),
        ("relace", vec!["uptime 99.30% < 99.5%", "quantization not declared"]),
        ("sail-research/fp4", vec!["fp4 below native fp8"]),
        ("together", undeclared()),
        ("wafer", vec!["uptime 99.41% < 99.5%", "quantization not declared"]),
    ]
}

/// The golden Budget, Balanced and Fast orders.
pub fn golden_selections(profile: RoutingProfileId) -> [[&'static str; 3]; 3] {
    match profile {
        RoutingProfileId::Interactive => [
            ["deepinfra/fp8", "streamlake/fp8", "gmicloud/fp8"],
            ["deepinfra/fp8", "streamlake/fp8", "makora/fp8"],
            ["venice/fp8", "baidu/fp8", "parasail/fp8"],
        ],
        RoutingProfileId::Helper => [
            ["streamlake/fp8", "deepinfra/fp8", "gmicloud/fp8"],
            ["streamlake/fp8", "venice/fp8", "gmicloud/fp8"],
            ["venice/fp8", "baidu/fp8", "parasail/fp8"],
        ],
    }
}

pub fn permutations<T: Clone>(items: &[T]) -> Vec<Vec<T>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    (0..items.len())
        .flat_map(|i| {
            let mut rest = items.to_vec();
            let item = rest.remove(i);
            permutations(&rest).into_iter().map(move |mut p| {
                p.insert(0, item.clone());
                p
            })
        })
        .collect()
}
