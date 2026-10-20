//! What a tier sends to OpenRouter (K12, C8, MR-D4, MR-D5, MR-D11), ported from Chorus 1's
//! `payloadCore.ts`, plus the `:nitro` id and effort variants from its `launchCore.ts`.
//!
//! Budget, Balanced and Fast pin their endpoints by tag in `order` with no fallback outside it;
//! they never carry a live sort, an `ignore` or `only` list, a price cap or a `preferred_*` key.
//! Nitro sends the `:nitro` id and lets OpenRouter choose. Every tier sends
//! `data_collection: "deny"` unless the settings allow collection, and then the key is absent.
//! The app puts these values into opencode's configuration; nothing here knows its shape.

use serde_json::{Map, Value, json};

use super::types::{
    CandidateExplanation, DataCollection, NitroLikely, NitroSelection, OpenRouterProviderPrefs, ProviderDataCollection, Quantization, RoutingSettings,
};

/// MR-D4: Nitro is OpenRouter's `<slug>:nitro`.
pub const NITRO_SUFFIX: &str = ":nitro";

/// The base model id: one trailing `:nitro` removed, nothing else.
pub fn base_model_id(id: &str) -> &str {
    id.strip_suffix(NITRO_SUFFIX).unwrap_or(id)
}

pub fn nitro_model_id(slug: &str) -> String {
    format!("{slug}{NITRO_SUFFIX}")
}

/// K12, C8: the distinct declared row quantizations of the selected candidates, in
/// `QUANTIZATIONS` order. OpenRouter filters on declared values, so an admitted first-party
/// `Unknown` contributes `Unknown`, never its effective precision.
pub fn quantizations_for(selected: &[&CandidateExplanation]) -> Vec<Quantization> {
    Quantization::ALL.into_iter().filter(|q| selected.iter().any(|c| c.row_quantizations.contains(q))).collect()
}

fn deny(settings: &RoutingSettings) -> Option<ProviderDataCollection> {
    (settings.data_collection == DataCollection::Deny).then_some(ProviderDataCollection::Deny)
}

/// MR-D5, MR-D11: `order` (tags, never display names), no fallbacks, required parameters, the
/// quantizations, and `data_collection` only under deny.
pub fn build_ranked_provider(selected: &[&CandidateExplanation], settings: &RoutingSettings) -> OpenRouterProviderPrefs {
    OpenRouterProviderPrefs {
        order: Some(selected.iter().map(|c| c.tag.clone()).collect()),
        allow_fallbacks: Some(false),
        require_parameters: Some(true),
        quantizations: Some(quantizations_for(selected)),
        data_collection: deny(settings),
    }
}

/// MR-D4, MR-D11: `<slug>:nitro`, with `{ data_collection: "deny" }` under deny and no provider
/// object under allow. `likely` and `likely_fails_rules` are the ranker's preview, copied.
pub fn build_nitro_selection(slug: &str, likely: Option<&NitroLikely>, likely_fails_rules: &[String], settings: &RoutingSettings) -> NitroSelection {
    NitroSelection {
        model: nitro_model_id(slug),
        provider: deny(settings).map(|d| OpenRouterProviderPrefs { data_collection: Some(d), ..OpenRouterProviderPrefs::default() }),
        likely: likely.cloned(),
        likely_fails_rules: likely_fails_rules.to_vec(),
    }
}

impl OpenRouterProviderPrefs {
    /// The `provider` object of an OpenRouter request, with only the keys that are set.
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        if let Some(order) = &self.order {
            m.insert("order".into(), json!(order));
        }
        if let Some(b) = self.allow_fallbacks {
            m.insert("allow_fallbacks".into(), json!(b));
        }
        if let Some(b) = self.require_parameters {
            m.insert("require_parameters".into(), json!(b));
        }
        if let Some(q) = &self.quantizations {
            m.insert("quantizations".into(), json!(q.iter().map(|q| q.as_str()).collect::<Vec<_>>()));
        }
        if self.data_collection.is_some() {
            m.insert("data_collection".into(), json!("deny"));
        }
        Value::Object(m)
    }
}

// ── Effort variants (K6, K13) ──

/// An effort token as routing accepts one: `[a-z0-9_-]{1,40}`.
pub fn is_effort(s: &str) -> bool {
    !s.is_empty() && s.len() <= 40 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// C9: the valid tokens, first occurrence kept, in order.
pub fn clean_efforts<S: AsRef<str>>(efforts: &[S]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in efforts.iter().map(AsRef::as_ref) {
        if is_effort(e) && !out.iter().any(|o| o == e) {
            out.push(e.to_owned());
        }
    }
    out
}

/// K6: the model's catalog efforts when it lists any; else the launch's own effort; else none.
pub fn routing_variant_efforts(catalog_efforts: Option<&[String]>, launch_effort: Option<&str>) -> Vec<String> {
    let listed = clean_efforts(catalog_efforts.unwrap_or_default());
    if !listed.is_empty() {
        return listed;
    }
    launch_effort.filter(|e| is_effort(e)).map(|e| vec![e.to_owned()]).unwrap_or_default()
}

/// opencode's model `variants`: `{ <effort>: { reasoning: { effort } } }` for each cleaned
/// effort, or `None` when none survives. A Nitro launch declares them, because opencode drops
/// a `:nitro` id's effort without them; a ranked tier never does, since the plain id's variants
/// are opencode's own.
pub fn variants_value<S: AsRef<str>>(efforts: &[S]) -> Option<Value> {
    let cleaned = clean_efforts(efforts);
    (!cleaned.is_empty()).then(|| Value::Object(cleaned.into_iter().map(|e| (e.clone(), json!({ "reasoning": { "effort": e } }))).collect()))
}

/// K13: the variants an unrouted launch of a `:nitro` id still needs (OpenRouter default with a
/// Nitro model: no provider object, only the variants). `None` unless the id ends in `:nitro`
/// and an effort survives cleaning.
pub fn nitro_variants<S: AsRef<str>>(sent_model_id: &str, efforts: &[S]) -> Option<Value> {
    if sent_model_id.ends_with(NITRO_SUFFIX) { variants_value(efforts) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::types::{CandidateCost, TierScores};

    const SLUG: &str = "deepseek/deepseek-v4.1-flash";
    const FORBIDDEN: [&str; 4] = ["sort", "ignore", "only", "max_price"];

    /// An eligible explanation; tests name only the fields they mean.
    fn explained(tag: &str, row_quantizations: &[Quantization]) -> CandidateExplanation {
        let quantization = if row_quantizations.len() == 1 { row_quantizations[0] } else { Quantization::Unknown };
        CandidateExplanation {
            tag: tag.into(),
            provider_name: "Provider".into(),
            rows: row_quantizations.len() as u32,
            quantization,
            row_quantizations: row_quantizations.to_vec(),
            effective_quantization: quantization,
            uptime_1d: Some(99.9),
            uptime_5m: Some(99.9),
            status: 0,
            observations: 1,
            limited_history: true,
            tps_p50: Some(100.0),
            latency_p50_s: Some(0.5),
            tps_p90: Some(150.0),
            latency_p90_s: Some(0.9),
            effective_tps: Some(85.7142857),
            cost: Some(CandidateCost {
                blended: 0.01942,
                fresh: 0.0057,
                cached: 0.00932,
                output: 0.0044,
                cache_credit: true,
                override_applied: false,
                prompt_per_m: 0.1,
                completion_per_m: 0.4,
                cache_read_per_m: 0.01,
            }),
            excluded_by: vec![],
            budget_floor_excluded: false,
            scores: TierScores::default(),
        }
    }

    fn a() -> CandidateExplanation {
        explained("a/fp8", &[Quantization::Fp8])
    }
    fn b() -> CandidateExplanation {
        explained("b/fp8", &[Quantization::Fp8])
    }
    fn c() -> CandidateExplanation {
        explained("c/fp32", &[Quantization::Fp32])
    }
    /// An admitted first-party candidate that declares no quantization: OpenRouter sees `unknown`.
    fn first_party() -> CandidateExplanation {
        CandidateExplanation {
            provider_name: "DeepSeek".into(),
            effective_quantization: Quantization::Fp8,
            ..explained("deepseek", &[Quantization::Unknown])
        }
    }
    fn mixed() -> CandidateExplanation {
        explained("mixed/fp8", &[Quantization::Fp8, Quantization::Fp16])
    }

    fn deny_settings() -> RoutingSettings {
        RoutingSettings::default()
    }
    fn allow_settings() -> RoutingSettings {
        RoutingSettings { data_collection: DataCollection::Allow, ..RoutingSettings::default() }
    }

    fn keys_deep(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Array(a) => a.iter().for_each(|x| keys_deep(x, out)),
            Value::Object(o) => {
                for (k, x) in o {
                    out.push(k.clone());
                    keys_deep(x, out);
                }
            }
            _ => {}
        }
    }

    fn assert_no_forbidden_keys(v: &Value) {
        let mut keys = Vec::new();
        keys_deep(v, &mut keys);
        for k in keys {
            assert!(!FORBIDDEN.contains(&k.as_str()) && !k.starts_with("preferred_"), "{k}");
        }
    }

    use Quantization::{Fp8, Fp16, Fp32, Unknown};

    #[test]
    fn y1_the_distinct_union_in_quantizations_order() {
        assert_eq!(quantizations_for(&[&a(), &b(), &c()]), [Fp8, Fp32]);
        assert_eq!(quantizations_for(&[&c(), &b(), &a()]), [Fp8, Fp32]);
    }

    #[test]
    fn y2_an_admitted_first_party_unknown_adds_unknown_never_its_effective_fp8() {
        assert_eq!(quantizations_for(&[&a(), &b(), &c(), &first_party()]), [Fp8, Fp32, Unknown]);
        assert_eq!(quantizations_for(&[&first_party(), &c(), &a()]), [Fp8, Fp32, Unknown]);
        assert_eq!(quantizations_for(&[&first_party()]), [Unknown]);
    }

    #[test]
    fn y3_a_mixed_tag_contributes_every_declared_row_value() {
        assert_eq!(quantizations_for(&[&mixed()]), [Fp8, Fp16]);
        assert_eq!(quantizations_for(&[&c(), &mixed(), &a()]), [Fp8, Fp16, Fp32]);
        assert!(quantizations_for(&[]).is_empty());
    }

    #[test]
    fn y4_deny_gives_the_full_payload_with_keys_in_contract_order() {
        let provider = build_ranked_provider(&[&b(), &a(), &c()], &deny_settings());
        assert_eq!(
            serde_json::to_string(&provider).unwrap(),
            r#"{"order":["b/fp8","a/fp8","c/fp32"],"allow_fallbacks":false,"require_parameters":true,"quantizations":["fp8","fp32"],"data_collection":"deny"}"#
        );
        assert_eq!(
            provider.to_value(),
            json!({ "order": ["b/fp8", "a/fp8", "c/fp32"], "allow_fallbacks": false, "require_parameters": true, "quantizations": ["fp8", "fp32"], "data_collection": "deny" })
        );
        assert_eq!(serde_json::to_value(&provider).unwrap(), provider.to_value(), "the derived serialisation agrees");
        assert_no_forbidden_keys(&provider.to_value());
    }

    #[test]
    fn y5_allow_omits_the_data_collection_key() {
        let provider = build_ranked_provider(&[&a(), &first_party()], &allow_settings());
        let text = serde_json::to_string(&provider).unwrap();
        assert_eq!(text, r#"{"order":["a/fp8","deepseek"],"allow_fallbacks":false,"require_parameters":true,"quantizations":["fp8","unknown"]}"#);
        assert!(!text.contains("data_collection"));
        assert!(provider.to_value().get("data_collection").is_none());
        assert_no_forbidden_keys(&provider.to_value());
    }

    #[test]
    fn order_carries_tags_never_provider_display_names() {
        let provider = build_ranked_provider(&[&first_party(), &a()], &deny_settings());
        assert_eq!(provider.order.as_deref(), Some(&["deepseek".to_owned(), "a/fp8".to_owned()][..]));
    }

    #[test]
    fn y6_nitro_under_deny_sends_data_collection_deny_with_the_nitro_id() {
        let likely = NitroLikely { tag: "together".into(), provider_name: "Together".into(), tps_p50: 223.0 };
        let rules = vec!["quantization not declared".to_owned()];
        let nitro = build_nitro_selection(SLUG, Some(&likely), &rules, &deny_settings());
        assert_eq!(nitro.model, "deepseek/deepseek-v4.1-flash:nitro");
        assert_eq!(nitro.provider.as_ref().map(OpenRouterProviderPrefs::to_value), Some(json!({ "data_collection": "deny" })));
        assert_eq!(nitro.likely, Some(likely.clone()));
        assert_eq!(nitro.likely_fails_rules, rules);
        assert_eq!(
            serde_json::to_string(&nitro).unwrap(),
            r#"{"model":"deepseek/deepseek-v4.1-flash:nitro","provider":{"data_collection":"deny"},"likely":{"tag":"together","providerName":"Together","tpsP50":223.0},"likelyFailsRules":["quantization not declared"]}"#
        );
        assert_no_forbidden_keys(&serde_json::to_value(&nitro).unwrap());
        // Under allow there is no provider object at all.
        let allow = build_nitro_selection(SLUG, Some(&likely), &rules, &allow_settings());
        assert_eq!((allow.provider, allow.model.as_str()), (None, "deepseek/deepseek-v4.1-flash:nitro"));
        // A missing preview stays missing.
        let none = build_nitro_selection(SLUG, None, &[], &deny_settings());
        assert_eq!((none.likely, none.likely_fails_rules.len()), (None, 0));
    }

    #[test]
    fn nitro_ids_add_and_remove_one_suffix() {
        assert_eq!(nitro_model_id(SLUG), "deepseek/deepseek-v4.1-flash:nitro");
        assert_eq!(base_model_id("deepseek/deepseek-v4.1-flash:nitro"), SLUG);
        assert_eq!(base_model_id(SLUG), SLUG);
        assert_eq!(base_model_id("x:nitro:nitro"), "x:nitro", "only one suffix comes off");
    }

    #[test]
    fn l10_variant_efforts_come_from_the_catalog_else_the_launch_else_none() {
        let list = |s: &[&str]| s.iter().map(|e| (*e).to_owned()).collect::<Vec<_>>();
        assert_eq!(routing_variant_efforts(Some(&list(&["low", "medium", "high"])), Some("low")), ["low", "medium", "high"]);
        assert_eq!(routing_variant_efforts(None, Some("high")), ["high"]);
        assert_eq!(routing_variant_efforts(Some(&[]), Some("high")), ["high"]);
        assert!(routing_variant_efforts(None, None).is_empty());
        assert_eq!(routing_variant_efforts(Some(&list(&["high", "HIGH", "high", "x y", "low"])), None), ["high", "low"]);
        assert_eq!(routing_variant_efforts(Some(&list(&["HIGH"])), Some("low")), ["low"]);
        assert!(routing_variant_efforts(None, Some("LOW")).is_empty());
        assert!(is_effort("x_high-2") && !is_effort("") && !is_effort(&"a".repeat(41)));
    }

    #[test]
    fn l8_and_l17_variants_are_one_reasoning_effort_each_and_only_for_nitro() {
        let three = json!({ "low": { "reasoning": { "effort": "low" } }, "medium": { "reasoning": { "effort": "medium" } }, "high": { "reasoning": { "effort": "high" } } });
        assert_eq!(variants_value(&["low", "medium", "high"]), Some(three.clone()));
        assert_eq!(nitro_variants("deepseek/deepseek-v4.1-flash:nitro", &["low", "medium", "high"]), Some(three));
        assert_eq!(nitro_variants("deepseek/deepseek-v4.1-flash:nitro", &["low"]), Some(json!({ "low": { "reasoning": { "effort": "low" } } })));
        assert_eq!(nitro_variants::<&str>("deepseek/deepseek-v4.1-flash:nitro", &[]), None);
        assert_eq!(nitro_variants(SLUG, &["low"]), None, "a plain id never declares variants");
        assert_eq!(
            nitro_variants("deepseek/deepseek-v4.1-flash:nitro", &["high", "HIGH", "high", "x y", "low"]),
            Some(json!({ "high": { "reasoning": { "effort": "high" } }, "low": { "reasoning": { "effort": "low" } } }))
        );
        assert_eq!(nitro_variants("z-ai/glm-5.3:nitro", &["high"]), Some(json!({ "high": { "reasoning": { "effort": "high" } } })));
        let text = nitro_variants("x:nitro", &["low"]).unwrap().to_string();
        assert!(!text.contains("options") && !text.contains("data_collection"));
    }
}
