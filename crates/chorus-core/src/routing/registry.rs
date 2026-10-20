//! The bundled model registry and verification-record activity (MR-D7), ported from Chorus 1's
//! `registryCore.ts`, plus a 2.0 addition: [`entry_for`] derives an entry for a model the
//! registry doesn't list, so any OpenRouter model can be ranked.
//!
//! The registry ships inside the binary (`model-registry.json`, Chorus 1's file unchanged). Its
//! verification times are ISO text there and Unix milliseconds once parsed.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

use super::json::{self, Path};
use super::time::parse_utc_instant;
use super::types::{ModelRegistry, ModelRegistryEntry, Quantization, RawEndpoint, VerificationRecord};

/// Chorus 1's `model-registry.json`, unchanged.
pub const BUNDLED_REGISTRY_JSON: &str = include_str!("model-registry.json");

/// Where an entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum RegistrySource {
    /// Listed in the bundled registry, with reviewed precision and first parties.
    Bundled,
    /// Not listed: derived from the model's endpoints by [`entry_for`].
    Derived,
}

/// A derived entry's precision floor: endpoints declaring less (fp6, fp4, int4) are excluded.
pub const DERIVED_NATIVE_PRECISION: Quantization = Quantization::Fp8;
/// The most context a derived entry demands.
pub const DERIVED_MIN_CONTEXT_CAP: u32 = 262_144;
pub const DERIVED_PRECISION_SOURCE: &str =
    "Not in Chorus's model registry: fp8 is assumed as a quality floor, and first parties are the providers named like the model's author.";

fn instant(o: &json::Obj, key: &str, path: &Path) -> Result<i64, String> {
    let s = json::string(o, key, path, 0)?;
    parse_utc_instant(&s).ok_or_else(|| path.key(key).issue("Invalid ISO datetime"))
}

fn parse_verification(v: &Value, path: &Path) -> Result<VerificationRecord, String> {
    let o = json::object(Some(v), path)?;
    let record = VerificationRecord {
        model: json::string(o, "model", path, 1)?,
        tag: json::string(o, "tag", path, 1)?,
        reference_tag: json::string(o, "referenceTag", path, 1)?,
        suite_version: json::string(o, "suiteVersion", path, 1)?,
        score: json::number_value(o.get("score"), &path.key("score"))?,
        verified_at_ms: instant(o, "verifiedAt", path)?,
        expires_at_ms: instant(o, "expiresAt", path)?,
        reviewer: json::string(o, "reviewer", path, 1)?,
    };
    json::no_unknown_keys(o, &["model", "tag", "referenceTag", "suiteVersion", "score", "verifiedAt", "expiresAt", "reviewer"], path)?;
    if record.expires_at_ms <= record.verified_at_ms {
        return Err(path.issue("expiresAt must be after verifiedAt"));
    }
    Ok(record)
}

fn parse_entry(v: &Value, path: &Path) -> Result<ModelRegistryEntry, String> {
    let o = json::object(Some(v), path)?;
    let slug = json::string(o, "slug", path, 1)?;
    let display_name = json::string(o, "displayName", path, 1)?;
    let precision_path = path.key("nativePrecision");
    let native_precision = match o.get("nativePrecision") {
        Some(Value::Null) => None,
        Some(Value::String(s)) => Some(
            Quantization::parse(s)
                .filter(|q| *q != Quantization::Unknown)
                .ok_or_else(|| precision_path.issue("Invalid option: expected one of \"int4\"|\"int8\"|\"fp4\"|\"fp6\"|\"fp8\"|\"fp16\"|\"bf16\"|\"fp32\""))?,
        ),
        other => return Err(json::expected(&precision_path, "string or null", other)),
    };
    let native_precision_source = json::string(o, "nativePrecisionSource", path, 1)?;
    let first_party_providers = json::string_array(o.get("firstPartyProviders"), &path.key("firstPartyProviders"), 1)?;
    let verified_path = path.key("verified");
    let verified = json::array(o.get("verified"), &verified_path)?
        .iter()
        .enumerate()
        .map(|(i, r)| parse_verification(r, &verified_path.key(i)))
        .collect::<Result<Vec<_>, _>>()?;
    let min_context = json::int_in(o.get("minContext"), &path.key("minContext"), 1, i64::from(u32::MAX))? as u32;
    json::no_unknown_keys(
        o,
        &["slug", "displayName", "nativePrecision", "nativePrecisionSource", "firstPartyProviders", "verified", "minContext"],
        path,
    )?;
    if verified.iter().any(|r| r.model != slug) {
        return Err(path.issue("verification record names another model"));
    }
    Ok(ModelRegistryEntry { slug, display_name, native_precision, native_precision_source, first_party_providers, verified, min_context })
}

/// Checks a registry file. The error is `Invalid model registry: <path>: <message>` for the
/// first problem.
pub fn parse_model_registry(json: &Value) -> Result<ModelRegistry, String> {
    let fail = |e: String| format!("Invalid model registry: {e}");
    let root = Path::default();
    let o = json::object(Some(json), &root).map_err(fail)?;
    if o.get("version").and_then(json::int_of) != Some(1) {
        return Err(fail(root.key("version").issue("Invalid input: expected 1")));
    }
    let models_path = root.key("models");
    let models = json::object(o.get("models"), &models_path).map_err(fail)?;
    json::no_unknown_keys(o, &["version", "models"], &root).map_err(fail)?;
    let mut out = ModelRegistry { version: 1, models: Default::default() };
    for (key, entry) in models {
        let entry = parse_entry(entry, &models_path.key(key)).map_err(fail)?;
        if entry.slug != *key {
            return Err(fail(root.issue("registry key differs from slug")));
        }
        out.models.insert(key.clone(), entry);
    }
    Ok(out)
}

/// The registry shipped with the app, parsed afresh on every call.
pub fn bundled_model_registry() -> Result<ModelRegistry, String> {
    let json: Value = serde_json::from_str(BUNDLED_REGISTRY_JSON).map_err(|e| format!("Invalid model registry: {e}"))?;
    parse_model_registry(&json)
}

pub fn find_model<'a>(registry: &'a ModelRegistry, slug: &str) -> Option<&'a ModelRegistryEntry> {
    registry.models.get(slug)
}

/// Active when `verified_at_ms <= now_ms < expires_at_ms`.
pub fn is_verification_active(record: &VerificationRecord, now_ms: i64) -> bool {
    record.verified_at_ms <= now_ms && now_ms < record.expires_at_ms
}

/// The active record for exactly this model and this routable tag (never a provider name or a
/// tag prefix). The latest `verified_at_ms` wins; ties keep registry order.
pub fn active_verification<'a>(entry: &'a ModelRegistryEntry, tag: &str, now_ms: i64) -> Option<&'a VerificationRecord> {
    let mut best: Option<&VerificationRecord> = None;
    for record in &entry.verified {
        if record.model != entry.slug || record.tag != tag || !is_verification_active(record, now_ms) {
            continue;
        }
        if best.is_none_or(|b| record.verified_at_ms > b.verified_at_ms) {
            best = Some(record);
        }
    }
    best
}

fn alphanumerics(s: &str) -> String {
    s.chars().filter(char::is_ascii_alphanumeric).map(|c| c.to_ascii_lowercase()).collect()
}

/// The registry's entry for `slug`, or one derived from its endpoints when the registry has
/// none (2.0; Chorus 1 refused such a model). A derived entry assumes fp8 as a quality floor,
/// so fp6, fp4 and int4 endpoints are excluded and an undeclared one needs its first party.
/// First parties are the providers whose name, as lower-case letters and digits, is the
/// author's (the slug before `/`). It demands the largest endpoint context, capped at 262,144
/// tokens, and has no verification records.
pub fn entry_for(registry: &ModelRegistry, slug: &str, endpoints: &[RawEndpoint]) -> (ModelRegistryEntry, RegistrySource) {
    if let Some(entry) = find_model(registry, slug) {
        return (entry.clone(), RegistrySource::Bundled);
    }
    let author = alphanumerics(slug.split('/').next().unwrap_or(slug));
    let mut first_party: Vec<String> = endpoints
        .iter()
        .map(|e| e.provider_name.clone())
        .filter(|name| !author.is_empty() && alphanumerics(name) == author)
        .collect();
    first_party.sort_by(|a, b| super::js::by_code_unit(a, b));
    first_party.dedup();
    let largest = endpoints.iter().map(|e| e.context_length).max().unwrap_or(u64::from(DERIVED_MIN_CONTEXT_CAP));
    let entry = ModelRegistryEntry {
        slug: slug.to_owned(),
        display_name: slug.to_owned(),
        native_precision: Some(DERIVED_NATIVE_PRECISION),
        native_precision_source: DERIVED_PRECISION_SOURCE.to_owned(),
        first_party_providers: first_party,
        verified: Vec::new(),
        min_context: largest.min(u64::from(DERIVED_MIN_CONTEXT_CAP)).max(1) as u32,
    };
    (entry, RegistrySource::Derived)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routing::test_support::{SLUG, base_row, ms};
    use serde_json::json;

    fn record_json(over: Value) -> Value {
        let mut r = json!({
            "model": SLUG, "tag": "together", "referenceTag": "deepinfra/fp8", "suiteVersion": "1", "score": 0.97,
            "verifiedAt": "2026-10-01T00:00:00Z", "expiresAt": "2026-11-01T00:00:00Z", "reviewer": "coordinator"
        });
        for (k, v) in over.as_object().unwrap() {
            r[k] = v.clone();
        }
        r
    }

    fn record(over: Value) -> VerificationRecord {
        parse_verification(&record_json(over), &Path::default()).unwrap()
    }

    /// The bundled entry with `verified`, checked by the entry rules.
    fn entry(verified: Vec<VerificationRecord>) -> ModelRegistryEntry {
        let e = ModelRegistryEntry { verified, ..bundled_model_registry().unwrap().models[SLUG].clone() };
        assert!(e.verified.iter().all(|r| r.model == e.slug));
        e
    }

    fn valid_entry() -> Value {
        json!({ "slug": "a/b", "displayName": "A B", "nativePrecision": "fp8", "nativePrecisionSource": "test",
                "firstPartyProviders": ["A"], "verified": [], "minContext": 1000 })
    }

    fn file(models: Value, version: Value) -> Value {
        json!({ "version": version, "models": models })
    }

    fn with(mut v: Value, key: &str, value: Value) -> Value {
        v[key] = value;
        v
    }

    #[test]
    fn r1_the_bundled_registry_holds_only_deepseek_v4_1_flash() {
        let registry = bundled_model_registry().unwrap();
        assert_eq!(registry.version, 1);
        assert_eq!(registry.models.keys().collect::<Vec<_>>(), [SLUG]);
        let model = find_model(&registry, SLUG).unwrap();
        assert_eq!(model.slug, SLUG);
        assert_eq!(model.display_name, "DeepSeek V4.1 Flash");
        assert_eq!(model.native_precision, Some(Quantization::Fp8));
        assert_eq!(model.first_party_providers, ["DeepSeek"]);
        assert!(model.verified.is_empty());
        assert_eq!(model.min_context, 262_144);
        // Each call is a fresh value: changing one cannot change the next.
        let mut a = bundled_model_registry().unwrap();
        a.models.get_mut(SLUG).unwrap().first_party_providers.push("Someone".into());
        assert_eq!(bundled_model_registry().unwrap().models[SLUG].first_party_providers, ["DeepSeek"]);
    }

    #[test]
    fn r2_a_key_differing_from_its_slug_version_2_and_native_unknown_are_refused() {
        assert!(parse_model_registry(&file(json!({ "a/b": valid_entry() }), json!(1))).is_ok());
        let refused = [
            file(json!({ "a/b": with(valid_entry(), "slug", json!("c/d")) }), json!(1)),
            file(json!({ "a/b": valid_entry() }), json!(2)),
            file(json!({ "a/b": with(valid_entry(), "nativePrecision", json!("unknown")) }), json!(1)),
            file(json!({ "a/b": with(valid_entry(), "extra", json!(1)) }), json!(1)),
            json!({ "version": 1, "models": {}, "extra": true }),
        ];
        for f in refused {
            let err = parse_model_registry(&f).unwrap_err();
            assert!(err.starts_with("Invalid model registry"), "{err}");
        }
    }

    #[test]
    fn r2_the_message_names_the_first_issue_as_path_and_message() {
        let err = parse_model_registry(&file(json!({ "a/b": with(valid_entry(), "nativePrecision", json!("unknown")) }), json!(1))).unwrap_err();
        assert!(err.starts_with("Invalid model registry: models.a/b.nativePrecision: "), "{err}");
    }

    #[test]
    fn r2_a_verification_record_naming_another_model_is_refused() {
        let bad = with(valid_entry(), "verified", json!([record_json(json!({ "model": "x/y" }))]));
        assert!(parse_model_registry(&file(json!({ "a/b": bad }), json!(1))).unwrap_err().starts_with("Invalid model registry"));
        // And a record that expires before it was verified.
        let backwards = record_json(json!({ "model": "a/b", "expiresAt": "2026-09-01T00:00:00Z" }));
        let err = parse_model_registry(&file(json!({ "a/b": with(valid_entry(), "verified", json!([backwards])) }), json!(1))).unwrap_err();
        assert_eq!(err, "Invalid model registry: models.a/b.verified.0: expiresAt must be after verifiedAt");
        let local_time = record_json(json!({ "model": "a/b", "verifiedAt": "2026-10-01T00:00:00" }));
        let err = parse_model_registry(&file(json!({ "a/b": with(valid_entry(), "verified", json!([local_time])) }), json!(1))).unwrap_err();
        assert!(err.starts_with("Invalid model registry: models.a/b.verified.0.verifiedAt: "), "{err}");
    }

    #[test]
    fn r3_native_precision_null_closed_weights_is_accepted() {
        let registry = parse_model_registry(&file(json!({ "a/b": with(valid_entry(), "nativePrecision", Value::Null) }), json!(1))).unwrap();
        assert_eq!(registry.models["a/b"].native_precision, None);
    }

    #[test]
    fn r4_find_model_with_an_unknown_slug_is_none() {
        let registry = bundled_model_registry().unwrap();
        for slug in ["nope/nope", "constructor", "toString"] {
            assert!(find_model(&registry, slug).is_none(), "{slug}");
        }
    }

    #[test]
    fn r5_verified_at_is_inclusive_and_expires_at_exclusive() {
        let r = record(json!({}));
        assert!(is_verification_active(&r, ms("2026-10-01T00:00:00Z")));
        assert!(is_verification_active(&r, ms("2026-10-31T23:59:59Z")));
        assert!(!is_verification_active(&r, ms("2026-11-01T00:00:00Z")));
        assert!(!is_verification_active(&r, ms("2026-09-30T23:59:59Z")));
        let e = entry(vec![r.clone()]);
        assert_eq!(active_verification(&e, "together", ms("2026-10-01T00:00:00Z")), Some(&r));
        assert_eq!(active_verification(&e, "together", ms("2026-10-31T23:59:59Z")), Some(&r));
        assert_eq!(active_verification(&e, "together", ms("2026-11-01T00:00:00Z")), None);
        assert_eq!(active_verification(&e, "together", ms("2026-09-30T23:59:59Z")), None);
    }

    #[test]
    fn r6_a_record_matches_its_exact_tag_only() {
        let e = entry(vec![record(json!({ "tag": "together" }))]);
        let now = ms("2026-10-15T00:00:00Z");
        assert!(active_verification(&e, "together", now).is_some());
        assert!(active_verification(&e, "together/fp8", now).is_none());
        assert!(active_verification(&e, "Together", now).is_none());
    }

    #[test]
    fn r7_the_later_verified_at_wins_and_ties_keep_registry_order() {
        let now = ms("2026-10-15T00:00:00Z");
        let older = record(json!({ "verifiedAt": "2026-10-01T00:00:00Z", "score": 0.9 }));
        let newer = record(json!({ "verifiedAt": "2026-10-10T00:00:00Z", "score": 0.95 }));
        assert_eq!(active_verification(&entry(vec![older.clone(), newer.clone()]), "together", now), Some(&newer));
        assert_eq!(active_verification(&entry(vec![newer.clone(), older]), "together", now), Some(&newer));
        let first = record(json!({ "reviewer": "first" }));
        let second = record(json!({ "reviewer": "second" }));
        let reviewer = |e: &ModelRegistryEntry| active_verification(e, "together", now).map(|r| r.reviewer.clone());
        assert_eq!(reviewer(&entry(vec![first.clone(), second.clone()])).as_deref(), Some("first"));
        assert_eq!(reviewer(&entry(vec![second, first])).as_deref(), Some("second"));
    }

    #[test]
    fn an_expired_newer_record_does_not_shadow_an_active_older_one() {
        let now = ms("2026-10-15T00:00:00Z");
        let active = record(json!({ "verifiedAt": "2026-10-01T00:00:00Z", "expiresAt": "2026-11-01T00:00:00Z" }));
        let expired = record(json!({ "verifiedAt": "2026-10-05T00:00:00Z", "expiresAt": "2026-10-06T00:00:00Z" }));
        assert_eq!(active_verification(&entry(vec![active.clone(), expired]), "together", now), Some(&active));
    }

    #[test]
    fn entry_for_returns_a_bundled_entry_unchanged() {
        let registry = bundled_model_registry().unwrap();
        let (e, source) = entry_for(&registry, SLUG, &[]);
        assert_eq!((e, source), (registry.models[SLUG].clone(), RegistrySource::Bundled));
    }

    #[test]
    fn entry_for_derives_an_unlisted_model_from_its_endpoints() {
        let registry = bundled_model_registry().unwrap();
        let at = |provider: &str, context: u64| RawEndpoint { provider_name: provider.into(), context_length: context, ..base_row("t") };
        let endpoints = [at("Z.AI", 131_072), at("Together", 200_000), at("z-ai", 131_072), at("Fireworks", 202_752)];
        let (e, source) = entry_for(&registry, "z-ai/glm-5.3", &endpoints);
        assert_eq!(source, RegistrySource::Derived);
        assert_eq!(e.slug, "z-ai/glm-5.3");
        assert_eq!(e.display_name, "z-ai/glm-5.3");
        assert_eq!(e.native_precision, Some(Quantization::Fp8));
        assert_eq!(e.native_precision_source, DERIVED_PRECISION_SOURCE);
        assert_eq!(e.first_party_providers, ["Z.AI", "z-ai"], "named like the author, in code-unit order");
        assert_eq!(e.min_context, 202_752, "the largest endpoint context");
        assert!(e.verified.is_empty());
        // A context above the cap is capped; no endpoints demand the cap itself.
        assert_eq!(entry_for(&registry, "a/b", &[at("A", 1_048_576)]).0.min_context, 262_144);
        assert_eq!(entry_for(&registry, "a/b", &[]).0.min_context, 262_144);
        // Matching is on letters and digits only; a provider merely containing the author is not first party.
        let (e, _) = entry_for(&registry, "Mistral-AI/x", &[at("mistralai", 1), at("Mistral", 1), at("Mistral AI (EU)", 1)]);
        assert_eq!(e.first_party_providers, ["mistralai"]);
        // A slug with no author part takes the whole slug; an empty author matches nothing.
        assert_eq!(entry_for(&registry, "acme", &[at("ACME", 1)]).0.first_party_providers, ["ACME"]);
        assert!(entry_for(&registry, "/x", &[at("", 1), at("-", 1)]).0.first_party_providers.is_empty());
    }

    #[test]
    fn a_derived_entry_excludes_below_fp8_and_admits_its_first_partys_undeclared_rows() {
        use crate::routing::test_support::Inputs;
        let rows = vec![
            RawEndpoint { quantization: Some("fp4".into()), ..base_row("a/fp4") },
            RawEndpoint { quantization: Some("fp6".into()), ..base_row("a/fp6") },
            RawEndpoint { quantization: Some("int4".into()), ..base_row("a/int4") },
            RawEndpoint { quantization: Some("bf16".into()), ..base_row("a/bf16") },
            RawEndpoint { quantization: None, provider_name: "Acme".into(), ..base_row("acme") },
            RawEndpoint { quantization: None, provider_name: "Other".into(), ..base_row("other") },
        ];
        let mut inputs = Inputs::synthetic(rows.clone());
        inputs.model = entry_for(&bundled_model_registry().unwrap(), "acme/model-1", &rows).0;
        let ranked = crate::routing::ranker::rank_candidates(&inputs.input());
        let reasons = |tag: &str| ranked.candidates.iter().find(|c| c.tag == tag).unwrap().excluded_by.clone();
        assert_eq!(reasons("a/fp4"), ["fp4 below native fp8"]);
        assert_eq!(reasons("a/fp6"), ["fp6 below native fp8"]);
        assert_eq!(reasons("a/int4"), ["int4 below native fp8"]);
        assert!(reasons("a/bf16").is_empty());
        assert!(reasons("acme").is_empty(), "the first party's undeclared row is admitted at fp8");
        assert_eq!(reasons("other"), ["quantization not declared"]);
    }
}
