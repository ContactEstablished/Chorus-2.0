//! The two zero-cost account preflights (MR-D12, MR-D18, K3, C1, C2), ported from Chorus 1's
//! `preflightCore.ts`. A chat completion pinned to an endpoint that doesn't exist, with
//! fallbacks off, is refused by OpenRouter with a 404 whose message names every endpoint the
//! account's own filters removed. This builds that request and reads the 404.
//!
//! The message is free text OpenRouter doesn't version, so the parse is strict everywhere and
//! count-checked on the target step: any drift is an issue (unknown, never "allowed"), never an
//! empty list.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use super::js::by_code_unit;
use super::json::{self, Path};
use super::types::{ProviderDataCollection, RawEndpoint, is_routing_tag};

/// The `order` entry no endpoint carries, so every candidate is removed and the 404 says why.
pub const PREFLIGHT_NONEXISTENT_TAG: &str = "chorus-preflight-none";
pub const PREFLIGHT_MARKER: &str = "Every candidate endpoint was removed during routing: ";
/// The only steps a removal clause may name. A clause naming another is a step mismatch: if
/// OpenRouter renamed the Guardrails step, the target would otherwise find no clause and read
/// as "nothing removed", the one wrong answer a renamed step must never produce. Funnel steps
/// without a clause (`Apply Status Sorting` removes nothing) stay allowed.
pub const PREFLIGHT_KNOWN_CLAUSE_STEPS: [&str; 3] = ["Filter by Guardrails", "Filter by Data Policy", "Filter by Fallback"];
const INITIAL_STEP: &str = "Initial Endpoints";

/// Which of the account's filters a preflight reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum PreflightStep {
    /// Account guardrails (training, provider bans). Sent without `data_collection`.
    Guardrails,
    /// The data policy under `data_collection: "deny"`.
    DataPolicy,
}

impl PreflightStep {
    /// The funnel step OpenRouter names for it.
    pub fn step_name(self) -> &'static str {
        match self {
            Self::Guardrails => "Filter by Guardrails",
            Self::DataPolicy => "Filter by Data Policy",
        }
    }
}

/// Why a preflight answer gave no list. Every one means "unknown" (MR-D12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum PreflightIssue {
    UnexpectedStatus,
    UnrecognizedBody,
    UnrecognizedMessage,
    UnbalancedReason,
    BadTag,
    FunnelInconsistent,
    StepMismatch,
    CountMismatch,
}

/// A chat message in a request body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatMessage {
    pub role: &'static str,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreflightProvider {
    pub order: Vec<String>,
    pub allow_fallbacks: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_collection: Option<ProviderDataCollection>,
}

/// `POST /api/v1/chat/completions`'s body. Serialised, its keys are in Chorus 1's measured order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreflightRequestBody {
    pub model: String,
    pub provider: PreflightProvider,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: u32,
}

/// The guardrails preflight has no `data_collection`; the data-policy one adds `"deny"` (MR-D18).
pub fn preflight_request_body(model: &str, step: PreflightStep) -> PreflightRequestBody {
    PreflightRequestBody {
        model: model.to_owned(),
        provider: PreflightProvider {
            order: vec![PREFLIGHT_NONEXISTENT_TAG.to_owned()],
            allow_fallbacks: false,
            data_collection: (step == PreflightStep::DataPolicy).then_some(ProviderDataCollection::Deny),
        },
        messages: vec![ChatMessage { role: "user", content: "OK".into() }],
        max_tokens: 1,
    }
}

/// How many rows each tag has in a snapshot (the funnel counts rows, not tags).
pub fn rows_per_tag(endpoints: &[RawEndpoint]) -> BTreeMap<String, u32> {
    let mut out = BTreeMap::new();
    for row in endpoints {
        *out.entry(row.tag.clone()).or_insert(0) += 1;
    }
    out
}

/// `(message, [(step, endpoint_count)])` when the body has the 404's shape. Extra keys are ignored.
fn body_shape(body: &Value) -> Result<(String, Vec<(String, i64)>), String> {
    let root = Path::default();
    let error_path = root.key("error");
    let error = json::object(json::object(Some(body), &root)?.get("error"), &error_path)?;
    let message = json::string(error, "message", &error_path, 0)?;
    let metadata_path = error_path.key("metadata");
    let metadata = json::object(error.get("metadata"), &metadata_path)?;
    let funnel_path = metadata_path.key("routing_funnel");
    let funnel = json::array(metadata.get("routing_funnel"), &funnel_path)?;
    if funnel.is_empty() {
        return Err(funnel_path.issue("Too small"));
    }
    let steps = funnel
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let p = funnel_path.key(i);
            let o = json::object(Some(entry), &p)?;
            Ok((json::string(o, "step", &p, 1)?, json::int_in(o.get("endpoint_count"), &p.key("endpoint_count"), 0, i64::MAX)?))
        })
        .collect::<Result<_, String>>()?;
    Ok((message, steps))
}

/// Splits before each `; Filter by `, so a `; ` inside a parenthesised reason never splits a
/// clause (Chorus 1's `/; (?=Filter by )/`).
fn split_clauses(tail: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    while let Some(off) = tail[start..].find("; Filter by ") {
        out.push(&tail[start..start + off]);
        start += off + 2;
    }
    out.push(&tail[start..]);
    out
}

/// `^(Filter by .+?) removed (.+)$` without a regex engine: the shortest step that leaves a
/// non-empty rest. `.` matches no line terminator, so a clause with one never matches.
fn parse_clause(text: &str) -> Option<(&str, &str)> {
    const PREFIX: &str = "Filter by ";
    const REMOVED: &str = " removed ";
    if text.contains(['\n', '\r', '\u{2028}', '\u{2029}']) || !text.starts_with(PREFIX) {
        return None;
    }
    text.match_indices(REMOVED)
        .filter(|(i, _)| *i > PREFIX.len())
        .map(|(i, _)| (&text[..i], &text[i + REMOVED.len()..]))
        .find(|(_, rest)| !rest.is_empty())
}

enum Strip<'a> {
    Kept(&'a str),
    Unbalanced,
    NoSpace,
}

/// Rule 5: strips one trailing parenthesised reason, nested parentheses included, by walking
/// back from the last `)` until the depth returns to 0. The reason must follow a space.
fn strip_reason(rest: &str) -> Strip<'_> {
    if !rest.ends_with(')') {
        return Strip::Kept(rest);
    }
    let b = rest.as_bytes();
    let mut depth = 0;
    for i in (0..b.len()).rev() {
        match b[i] {
            b')' => depth += 1,
            b'(' => {
                depth -= 1;
                if depth == 0 {
                    return if i == 0 || b[i - 1] != b' ' { Strip::NoSpace } else { Strip::Kept(&rest[..i - 1]) };
                }
            }
            _ => {}
        }
    }
    Strip::Unbalanced
}

/// Reads one preflight answer (Chorus 1's rules 1–11 in order; the first failure wins). Each
/// per-clause rule runs over every clause before the next rule starts. Rule 9a, between 9 and
/// 10, requires every clause to name a known step. Only the target step is count-checked, with
/// each tag weighing its rows in the snapshot (1 when the tag is unknown).
pub fn parse_preflight(status: u16, body: &Value, step: PreflightStep, rows_per_tag: &BTreeMap<String, u32>) -> Result<Vec<String>, PreflightIssue> {
    use PreflightIssue::*;

    // 1. Status.
    if status != 404 {
        return Err(UnexpectedStatus);
    }

    // 2. Body shape.
    let (message, funnel) = body_shape(body).map_err(|_| UnrecognizedBody)?;

    // 3. Marker: the tail is everything after its first occurrence, less one trailing `.`.
    let at = message.find(PREFLIGHT_MARKER).ok_or(UnrecognizedMessage)?;
    let tail = &message[at + PREFLIGHT_MARKER.len()..];
    let tail = tail.strip_suffix('.').unwrap_or(tail);

    // 4. Clauses.
    let clauses = split_clauses(tail).into_iter().map(parse_clause).collect::<Option<Vec<_>>>().ok_or(UnrecognizedMessage)?;

    // 5. Reasons.
    let mut stripped = Vec::with_capacity(clauses.len());
    for (clause_step, rest) in clauses {
        match strip_reason(rest) {
            Strip::Kept(r) => stripped.push((clause_step, r)),
            Strip::Unbalanced => return Err(UnbalancedReason),
            Strip::NoSpace => return Err(UnrecognizedMessage),
        }
    }

    // 6. Tags: every piece a routable tag; deduplicated and sorted by code unit.
    let mut tagged: Vec<(&str, Vec<String>)> = Vec::with_capacity(stripped.len());
    for (clause_step, rest) in stripped {
        let pieces: Vec<&str> = rest.split(", ").collect();
        if !pieces.iter().all(|p| is_routing_tag(p)) {
            return Err(BadTag);
        }
        let mut tags: Vec<String> = pieces.into_iter().map(str::to_owned).collect();
        tags.sort_by(|a, b| by_code_unit(a, b));
        tags.dedup();
        tagged.push((clause_step, tags));
    }

    // 7. One clause per step.
    for (i, (s, _)) in tagged.iter().enumerate() {
        if tagged[..i].iter().any(|(earlier, _)| earlier == s) {
            return Err(UnrecognizedMessage);
        }
    }

    // 8. Funnel: starts at Initial Endpoints, never grows, ends at 0, repeats no step.
    let (first, last) = (&funnel[0], &funnel[funnel.len() - 1]);
    if first.0 != INITIAL_STEP || last.1 != 0 {
        return Err(FunnelInconsistent);
    }
    let mut removals: BTreeMap<&str, i64> = BTreeMap::new();
    for pair in funnel.windows(2) {
        let (prev, cur) = (&pair[0], &pair[1]);
        let removal = prev.1 - cur.1;
        if removal < 0 || removals.contains_key(cur.0.as_str()) || cur.0 == INITIAL_STEP {
            return Err(FunnelInconsistent);
        }
        removals.insert(cur.0.as_str(), removal);
    }

    // 9. Clauses and positive removals correspond one to one.
    if tagged.iter().any(|(s, _)| removals.get(s).is_none_or(|r| *r <= 0)) {
        return Err(StepMismatch);
    }
    if removals.iter().any(|(s, r)| *r > 0 && !tagged.iter().any(|(t, _)| t == s)) {
        return Err(StepMismatch);
    }

    // 9a. Every clause names a known step, so a renamed target reads as unknown.
    if tagged.iter().any(|(s, _)| !PREFLIGHT_KNOWN_CLAUSE_STEPS.contains(s)) {
        return Err(StepMismatch);
    }

    // 10. No clause for the target: by rules 9 and 9a its removal is 0 or the step is absent.
    let target = step.step_name();
    let Some((_, tags)) = tagged.into_iter().find(|(s, _)| *s == target) else { return Ok(Vec::new()) };

    // 11. Count (C1): the funnel counts rows, so each tag weighs its row count (1 when unknown).
    let weighted: i64 = tags.iter().map(|t| i64::from(rows_per_tag.get(t).copied().unwrap_or(1))).sum();
    if Some(&weighted) != removals.get(target) {
        return Err(CountMismatch);
    }
    Ok(tags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::test_support::*;
    use serde_json::json;
    use PreflightIssue::*;
    use PreflightStep::{DataPolicy, Guardrails};

    const PREFIX: &str = "No endpoints found for x/y. Every candidate endpoint was removed during routing: ";

    fn guardrails_fixture() -> Value {
        serde_json::from_str(PREFLIGHT_GUARDRAILS_FIXTURE).unwrap()
    }

    fn data_policy_fixture() -> Value {
        serde_json::from_str(PREFLIGHT_DATA_POLICY_FIXTURE).unwrap()
    }

    fn golden_rows() -> BTreeMap<String, u32> {
        rows_per_tag(&golden_endpoints())
    }

    fn none() -> BTreeMap<String, u32> {
        BTreeMap::new()
    }

    /// A synthetic 404 body.
    fn synthetic(tail: &str, funnel: &[(&str, i64)]) -> Value {
        let funnel: Vec<Value> = funnel.iter().map(|(step, n)| json!({ "step": step, "endpoint_count": n })).collect();
        json!({ "error": { "message": format!("{PREFIX}{tail}"), "metadata": { "routing_funnel": funnel } } })
    }

    fn parse(body: &Value, step: PreflightStep, rows: &BTreeMap<String, u32>) -> Result<Vec<String>, PreflightIssue> {
        parse_preflight(404, body, step, rows)
    }

    fn ok(tags: &[&str]) -> Result<Vec<String>, PreflightIssue> {
        Ok(tags.iter().map(|t| (*t).to_owned()).collect())
    }

    const GUARDED: [(&str, i64); 3] = [("Initial Endpoints", 2), ("Filter by Guardrails", 1), ("Filter by Fallback", 0)];

    #[test]
    fn the_fixtures_name_the_golden_tags_and_are_404s() {
        let mut golden_tags: Vec<String> = golden_rows().into_keys().collect();
        golden_tags.sort();
        assert_eq!(golden_tags.len(), 32);
        let dp = data_policy_fixture();
        let bodies = [guardrails_fixture()["body"].clone(), dp["withoutDeny"]["body"].clone(), dp["withDeny"]["body"].clone()];
        for body in &bodies {
            let message = body["error"]["message"].as_str().unwrap();
            let fallback = message.split("Filter by Fallback removed ").nth(1).unwrap().strip_suffix('.').unwrap();
            let tags: Vec<&str> = fallback.split(", ").collect();
            assert_eq!(tags.len(), 31, "31 tags for 32 rows: baseten/fp8 has two");
            let mut all: Vec<String> = tags.iter().map(|t| (*t).to_owned()).chain(["deepseek".to_owned()]).collect();
            all.sort();
            all.dedup();
            assert_eq!(all, golden_tags);
        }
        assert_eq!([&guardrails_fixture()["status"], &dp["withoutDeny"]["status"], &dp["withDeny"]["status"]], [&json!(404); 3]);
    }

    #[test]
    fn p1_to_p6_the_real_2026_10_02_bodies() {
        let g = guardrails_fixture()["body"].clone();
        let dp = data_policy_fixture();
        let (without, with) = (dp["withoutDeny"]["body"].clone(), dp["withDeny"]["body"].clone());
        assert_eq!(parse(&g, Guardrails, &golden_rows()), ok(&["deepseek"]), "P1");
        assert_eq!(parse(&g, DataPolicy, &golden_rows()), ok(&[]), "P2: no Data Policy clause or step");
        assert_eq!(parse(&without, Guardrails, &golden_rows()), ok(&["deepseek"]), "P3: no Apply Status Sorting");
        assert_eq!(parse(&with, DataPolicy, &golden_rows()), ok(&["deepseek"]), "P4");
        // P5: under deny the Guardrails step is absent, so this reading says "nothing removed"
        // while the guardrail does remove deepseek. That is why there are two preflights.
        assert_eq!(parse(&with, Guardrails, &golden_rows()), ok(&[]), "P5");
        assert_eq!(parse(&g, Guardrails, &none()), ok(&["deepseek"]), "P6: unknown tags count 1");
    }

    #[test]
    fn p7_nested_parentheses_in_the_reason_and_two_tags() {
        let body = synthetic(
            "Filter by Guardrails removed a/fp8, b (Reason (x) (y)); Filter by Fallback removed c.",
            &[("Initial Endpoints", 3), ("Filter by Guardrails", 1), ("Filter by Fallback", 0)],
        );
        assert_eq!(parse(&body, Guardrails, &none()), ok(&["a/fp8", "b"]));
    }

    #[test]
    fn p8_to_p10_rows_weigh_the_count() {
        let body = synthetic(
            "Filter by Guardrails removed baseten/fp8, x (r); Filter by Fallback removed c.",
            &[("Initial Endpoints", 4), ("Filter by Guardrails", 1), ("Filter by Fallback", 0)],
        );
        let rows: BTreeMap<String, u32> = [("baseten/fp8".to_owned(), 2)].into();
        assert_eq!(parse(&body, Guardrails, &rows), ok(&["baseten/fp8", "x"]), "P8: 2 + 1 = 3");
        assert_eq!(parse(&body, Guardrails, &none()), Err(CountMismatch), "P9: 2 tags is not 3 rows");
        let one = synthetic(
            "Filter by Guardrails removed deepseek (r); Filter by Fallback removed c.",
            &[("Initial Endpoints", 4), ("Filter by Guardrails", 2), ("Filter by Fallback", 0)],
        );
        assert_eq!(parse(&one, Guardrails, &none()), Err(CountMismatch), "P10");
    }

    #[test]
    fn p11_to_p14_status_body_and_marker() {
        let g = guardrails_fixture()["body"].clone();
        assert_eq!(parse_preflight(400, &g, Guardrails, &golden_rows()), Err(UnexpectedStatus));
        assert_eq!(parse_preflight(200, &g, Guardrails, &golden_rows()), Err(UnexpectedStatus));
        let message = g["error"]["message"].clone();
        assert_eq!(parse(&json!({ "error": { "message": message, "metadata": {} } }), Guardrails, &golden_rows()), Err(UnrecognizedBody), "P12");
        let numeric = json!({ "error": { "message": 5, "metadata": { "routing_funnel": [{ "step": "Initial Endpoints", "endpoint_count": 1 }] } } });
        for body in [numeric, Value::Null, json!("text")] {
            assert_eq!(parse(&body, Guardrails, &none()), Err(UnrecognizedBody), "P13");
        }
        let max_price = json!({ "error": { "message": "No endpoints found that satisfy the max price for this request",
            "metadata": { "routing_funnel": [{ "step": "Initial Endpoints", "endpoint_count": 33 }] } } });
        assert_eq!(parse(&max_price, Guardrails, &none()), Err(UnrecognizedMessage), "P14");
    }

    #[test]
    fn p15_to_p20_reasons_and_tags() {
        let at = |tail: &str| parse(&synthetic(tail, &GUARDED), Guardrails, &none());
        assert_eq!(at("Filter by Guardrails removed deepseek (r)); Filter by Fallback removed c."), Err(UnbalancedReason), "P15");
        assert_eq!(at("Filter by Guardrails removed deepseek(r); Filter by Fallback removed c."), Err(UnrecognizedMessage), "P16");
        assert_eq!(at("Filter by Guardrails removed deep seek; Filter by Fallback removed c."), Err(BadTag), "P17");
        // P18: assembled at run time so this file never holds a complete key shape.
        let key_shaped = format!("sk-or-v1-{}", "0".repeat(64));
        assert_eq!(key_shaped.len(), 73);
        assert_eq!(at(&format!("Filter by Guardrails removed {key_shaped}; Filter by Fallback removed c.")), Err(BadTag), "P18");
        assert_eq!(at("Filter by Guardrails removed deepseek, deepseek; Filter by Fallback removed c."), ok(&["deepseek"]), "P19");
        assert_eq!(at("Filter by Guardrails removed deepseek (a; b); Filter by Fallback removed c."), ok(&["deepseek"]), "P20");
    }

    #[test]
    fn p21_to_p25_steps_and_funnels() {
        let p21 = synthetic("Filter by Guardrails removed deepseek (r); Filter by Fallback removed c.", &[("Initial Endpoints", 2), ("Filter by Fallback", 0)]);
        assert_eq!(parse(&p21, Guardrails, &none()), Err(StepMismatch), "P21: a clause without a funnel step");
        let p22 = synthetic("Filter by Fallback removed c, d.", &[("Initial Endpoints", 3), ("Filter by Guardrails", 2), ("Filter by Fallback", 0)]);
        assert_eq!(parse(&p22, Guardrails, &none()), Err(StepMismatch), "P22: a removal without a clause");
        let p23 = synthetic(
            "Filter by Guardrails removed a (r); Filter by Fallback removed c.",
            &[("Initial Endpoints", 3), ("Filter by Guardrails", 2), ("Filter by Fallback", 1)],
        );
        assert_eq!(parse(&p23, Guardrails, &none()), Err(FunnelInconsistent), "P23");
        let p24 = synthetic("Filter by Fallback removed c.", &[("Initial Endpoints", 1), ("Filter by Guardrails", 1), ("Filter by Fallback", 0)]);
        assert_eq!(parse(&p24, Guardrails, &none()), ok(&[]), "P24: the target with zero removal");
        let p25 = synthetic("Filter by Guardrails removed a, b (r).", &[("Initial Endpoints", 2), ("Filter by Guardrails", 0)]);
        assert_eq!(parse(&p25, Guardrails, &none()), ok(&["a", "b"]), "P25");
    }

    #[test]
    fn funnel_drift_and_clause_shapes() {
        let tail = "Filter by Guardrails removed deepseek (r); Filter by Fallback removed c.";
        let drifts: [&[(&str, i64)]; 3] = [
            &[("Start", 2), ("Filter by Guardrails", 1), ("Filter by Fallback", 0)],
            &[("Initial Endpoints", 2), ("Filter by Guardrails", 3), ("Filter by Fallback", 0)],
            &[("Initial Endpoints", 3), ("Filter by Guardrails", 2), ("Filter by Guardrails", 1), ("Filter by Fallback", 0)],
        ];
        for funnel in drifts {
            assert_eq!(parse(&synthetic(tail, funnel), Guardrails, &none()), Err(FunnelInconsistent), "{funnel:?}");
        }
        let twice = synthetic(
            "Filter by Guardrails removed a; Filter by Guardrails removed b; Filter by Fallback removed c.",
            &[("Initial Endpoints", 3), ("Filter by Guardrails", 1), ("Filter by Fallback", 0)],
        );
        assert_eq!(parse(&twice, Guardrails, &none()), Err(UnrecognizedMessage), "a step in two clauses");
        let dropped = synthetic("Filter by Guardrails dropped deepseek; Filter by Fallback removed c.", &GUARDED);
        assert_eq!(parse(&dropped, Guardrails, &none()), Err(UnrecognizedMessage), "not 'Filter by <Step> removed <tags>'");
        let newline = synthetic("Filter by Guardrails removed deep\nseek; Filter by Fallback removed c.", &GUARDED);
        assert_eq!(parse(&newline, Guardrails, &none()), Err(UnrecognizedMessage), "'.' matches no line break");
        // Every failure is an issue, never an empty list.
        assert!(parse_preflight(500, &guardrails_fixture()["body"], Guardrails, &golden_rows()).is_err());
        assert!(parse(&Value::Null, Guardrails, &golden_rows()).is_err());
    }

    #[test]
    fn clauses_split_and_match_like_chorus_1s_regexes() {
        assert_eq!(split_clauses("a (x; y); Filter by B removed c"), ["a (x; y)", "Filter by B removed c"]);
        assert_eq!(split_clauses(""), [""]);
        assert_eq!(split_clauses("; Filter by X removed a"), ["", "Filter by X removed a"]);
        assert_eq!(parse_clause("Filter by X removed a"), Some(("Filter by X", "a")));
        assert_eq!(parse_clause("Filter by X removed  removed b"), Some(("Filter by X", " removed b")), "the shortest step wins");
        assert_eq!(parse_clause("Filter by X removed "), None);
        assert_eq!(parse_clause("Filter by  removed a"), None, "the step needs a character");
        assert_eq!(parse_clause("Filter by A removed  removed b"), Some(("Filter by A", " removed b")));
    }

    #[test]
    fn m1_an_unknown_clause_step_reads_as_unknown_never_as_nothing_removed() {
        assert_eq!(PREFLIGHT_KNOWN_CLAUSE_STEPS, ["Filter by Guardrails", "Filter by Data Policy", "Filter by Fallback"]);
        // The real guardrails body with its step renamed in the message and the funnel.
        let mut renamed = guardrails_fixture()["body"].clone();
        let message = renamed["error"]["message"].as_str().unwrap().replace("Filter by Guardrails removed", "Filter by Guardrail Policy removed");
        renamed["error"]["message"] = json!(message);
        for entry in renamed["error"]["metadata"]["routing_funnel"].as_array_mut().unwrap() {
            if entry["step"] == "Filter by Guardrails" {
                entry["step"] = json!("Filter by Guardrail Policy");
            }
        }
        assert_eq!(parse(&renamed, Guardrails, &golden_rows()), Err(StepMismatch));
        assert_eq!(parse(&renamed, DataPolicy, &golden_rows()), Err(StepMismatch));
        let synthetic_renamed = synthetic(
            "Filter by Guardrail Policy removed deepseek (r); Filter by Fallback removed c.",
            &[("Initial Endpoints", 2), ("Filter by Guardrail Policy", 1), ("Filter by Fallback", 0)],
        );
        assert_eq!(parse(&synthetic_renamed, Guardrails, &none()), Err(StepMismatch));
        assert_eq!(parse(&synthetic_renamed, DataPolicy, &none()), Err(StepMismatch));
        let extra = synthetic(
            "Filter by Guardrails removed deepseek (r); Filter by Ignored Providers removed x; Filter by Fallback removed c.",
            &[("Initial Endpoints", 3), ("Filter by Guardrails", 2), ("Filter by Ignored Providers", 1), ("Filter by Fallback", 0)],
        );
        assert_eq!(parse(&extra, Guardrails, &none()), Err(StepMismatch));
        assert_eq!(parse(&extra, DataPolicy, &none()), Err(StepMismatch));
        // An unknown funnel step that removes nothing and has no clause stays allowed.
        let sorting = synthetic(
            "Filter by Guardrails removed deepseek (r); Filter by Fallback removed c.",
            &[("Initial Endpoints", 2), ("Filter by Guardrails", 1), ("Some Future Sorting", 1), ("Filter by Fallback", 0)],
        );
        assert_eq!(parse(&sorting, Guardrails, &none()), ok(&["deepseek"]));
    }

    #[test]
    fn row_counts_are_ordinary_keys() {
        let body = synthetic(
            "Filter by Guardrails removed constructor, toString (r); Filter by Fallback removed c.",
            &[("Initial Endpoints", 3), ("Filter by Guardrails", 1), ("Filter by Fallback", 0)],
        );
        assert_eq!(parse(&body, Guardrails, &none()), ok(&["constructor", "toString"]));
        let at = |tag: &str| RawEndpoint { tag: tag.into(), ..golden_endpoints()[0].clone() };
        let counts = rows_per_tag(&[at("constructor"), at("toString"), at("toString"), at("__proto__")]);
        assert_eq!(counts.keys().collect::<Vec<_>>(), ["__proto__", "constructor", "toString"]);
        assert_eq!((counts["constructor"], counts["toString"], counts["__proto__"]), (1, 2, 1));
    }

    #[test]
    fn p26_the_exact_serialised_request_bodies() {
        assert_eq!(
            serde_json::to_string(&preflight_request_body(SLUG, Guardrails)).unwrap(),
            r#"{"model":"deepseek/deepseek-v4.1-flash","provider":{"order":["chorus-preflight-none"],"allow_fallbacks":false},"messages":[{"role":"user","content":"OK"}],"max_tokens":1}"#
        );
        assert_eq!(
            serde_json::to_string(&preflight_request_body(SLUG, DataPolicy)).unwrap(),
            r#"{"model":"deepseek/deepseek-v4.1-flash","provider":{"order":["chorus-preflight-none"],"allow_fallbacks":false,"data_collection":"deny"},"messages":[{"role":"user","content":"OK"}],"max_tokens":1}"#
        );
    }

    #[test]
    fn p27_rows_per_tag_of_the_golden_endpoints_and_the_constants() {
        let rows = golden_rows();
        assert_eq!(golden_endpoints().len(), 33);
        assert_eq!(rows.len(), 32);
        assert_eq!(rows["baseten/fp8"], 2);
        assert_eq!(rows.values().filter(|n| **n != 1).collect::<Vec<_>>(), [&2]);
        assert_eq!(PREFLIGHT_NONEXISTENT_TAG, "chorus-preflight-none");
        assert_eq!(PREFLIGHT_MARKER, "Every candidate endpoint was removed during routing: ");
        assert_eq!((Guardrails.step_name(), DataPolicy.step_name()), ("Filter by Guardrails", "Filter by Data Policy"));
    }
}
