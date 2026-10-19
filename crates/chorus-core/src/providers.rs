//! Providers (PLAN P2): OpenRouter, OpenAI-compatible gateways and TypeSafe's Jev. This module
//! holds the shapes Chorus sends and reads; `src-tauri` does the calls. Checked against the live
//! APIs on 2026-10-19: OpenRouter's public catalog (`GET /api/v1/models`, LESSONS L17) and its
//! key management (`/api/v1/keys`, management key only), and Jev's `POST /v1/systemone` (D40).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;

use crate::agent::AgentKind;

pub const OPENROUTER_API: &str = "https://openrouter.ai/api/v1";
pub const JEV_URL: &str = "https://api.typesafe.ai/v1/systemone";
pub const JEV_MODEL: &str = "jev-latest";
/// The per-run cap for a minted key when the provider sets none. Chorus 1 measured that OpenRouter
/// holds back the cost of a request's `max_tokens` up front, so a $0.50 key refused Codex's
/// 65,536-token requests with 402.
pub const DEFAULT_CAP_USD: f64 = 1.0;
pub const MAX_CAP_USD: f64 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    /// OpenRouter: one key for many models, and a management key that mints capped keys.
    Openrouter,
    /// Any OpenAI-compatible endpoint (Responses API) at a base URL.
    Gateway,
    /// TypeSafe's Jev: typed decisions, not an agent model (D40).
    Jev,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Openrouter => "openrouter",
            Self::Gateway => "gateway",
            Self::Jev => "jev",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Openrouter, Self::Gateway, Self::Jev].into_iter().find(|k| k.as_str() == s)
    }

    /// The address a provider of this kind uses; a gateway's is the user's.
    pub fn fixed_url(self) -> Option<&'static str> {
        match self {
            Self::Openrouter => Some(OPENROUTER_API),
            Self::Jev => Some(JEV_URL),
            Self::Gateway => None,
        }
    }
}

/// A provider's name: shown in the UI and passed to Codex inside a quoted config value.
pub fn valid_provider_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && n.len() <= 60 && n.chars().all(|c| c.is_ascii_alphanumeric() || " -_.()".contains(c))
}

/// A gateway's base URL, without a trailing slash: `https://`, or plain `http://` only on this
/// machine. No quotes, spaces or backslashes, because it is passed inside a Codex config value.
pub fn clean_base_url(url: &str) -> Result<String, String> {
    let u = url.trim().trim_end_matches('/');
    let local = ["http://127.0.0.1", "http://localhost"].iter().any(|p| u.strip_prefix(p).is_some_and(|r| r.is_empty() || r.starts_with([':', '/'])));
    if !(u.starts_with("https://") || local) {
        return Err("A gateway address must start with https:// (or http://localhost for one on this machine).".into());
    }
    if u.len() > 300 || !u.chars().all(|c| c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&()*+,;=%".contains(c)) {
        return Err("That gateway address has characters Chorus can't pass on.".into());
    }
    Ok(u.to_owned())
}

/// The environment variable a route's key goes into. opencode finds OpenRouter by this one.
pub fn key_var(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Openrouter => "OPENROUTER_API_KEY",
        ProviderKind::Gateway => "CHORUS_GATEWAY_KEY",
        ProviderKind::Jev => "TYPESAFE_API_KEY",
    }
}

/// Whether `agent` can run on a provider of `kind`, with the reason when it can't (D19).
pub fn route_support(agent: AgentKind, kind: ProviderKind) -> Result<(), String> {
    match (agent, kind) {
        (_, ProviderKind::Jev) => Err("Jev answers typed questions; it can't run an agent (D40).".into()),
        (AgentKind::Codex, ProviderKind::Openrouter | ProviderKind::Gateway) => Ok(()),
        (AgentKind::Opencode, ProviderKind::Openrouter) => Ok(()),
        (AgentKind::Opencode, ProviderKind::Gateway) => {
            Err("Chorus routes opencode through OpenRouter only for now, not a gateway.".into())
        }
        (AgentKind::Claude, _) => Err(
            "Claude Code runs on your Claude login or an Anthropic key (Key below); Chorus doesn't route it through other providers yet."
                .into(),
        ),
        (AgentKind::Grok, _) => Err("Grok has no setting for another provider's address, so it runs on xAI only.".into()),
        (AgentKind::Shell, _) => Err("A shell runs no model. To give it a key, use Key below.".into()),
    }
}

/// What a supported route adds to a launch.
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    /// Where the key goes: the provider's own key, or a capped key minted for the run.
    pub key_var: &'static str,
    /// Arguments before the agent's options (Codex's provider definition).
    pub args: Vec<String>,
    /// The model as this agent names it (`openrouter/<id>` for opencode).
    pub model: String,
    /// opencode: a `provider` block for its configuration, so a model newer than its own list
    /// still runs, with the catalog's efforts as its variants.
    pub opencode_provider: Option<Value>,
}

/// The id Codex knows the route's provider by (`-c model_provider=`).
pub const CODEX_PROVIDER_ID: &str = "chorus";

/// The launch additions for `agent` on `provider` with `model` (the provider's own model id).
/// `catalog` is what Chorus's catalog knows of the model, if anything.
pub fn route(
    agent: AgentKind,
    kind: ProviderKind,
    name: &str,
    base_url: &str,
    model: &str,
    catalog: Option<&CatalogModel>,
) -> Result<Route, String> {
    route_support(agent, kind)?;
    let key_var = key_var(kind);
    match agent {
        AgentKind::Codex => {
            let url = match kind.fixed_url() {
                Some(u) => u.to_owned(),
                None => clean_base_url(base_url)?,
            };
            if !valid_provider_name(name) {
                return Err(format!("{name:?} can't be used as a provider name."));
            }
            // Ported from Chorus 1's measured route (codex.ts): Codex 0.162.1 takes a custom
            // provider from `-c` and accepts only the Responses wire API.
            let p = CODEX_PROVIDER_ID;
            let mut overrides = vec![
                format!("model_provider=\"{p}\""),
                format!("model_providers.{p}.name=\"{}\"", name.trim()),
                format!("model_providers.{p}.base_url=\"{url}\""),
                format!("model_providers.{p}.env_key=\"{key_var}\""),
                format!("model_providers.{p}.wire_api=\"responses\""),
            ];
            // Codex has no metadata for most provider models and falls back to defaults; the
            // context window is the one figure it takes from config (`model_context_window`).
            if let Some(context) = catalog.and_then(|c| c.context_length) {
                overrides.push(format!("model_context_window={context}"));
            }
            let args = overrides.into_iter().flat_map(|a| ["-c".to_owned(), a]).collect();
            Ok(Route { key_var, args, model: model.to_owned(), opencode_provider: None })
        }
        AgentKind::Opencode => {
            let mut entry = serde_json::Map::new();
            if let Some(c) = catalog {
                entry.insert("name".into(), json!(c.name));
                if let (Some(context), Some(output)) = (c.context_length, c.max_output) {
                    entry.insert("limit".into(), json!({ "context": context, "output": output }));
                }
                if !c.efforts.is_empty() {
                    // The shape opencode 1.19.3 gives its own OpenRouter models' variants.
                    let variants: serde_json::Map<String, Value> =
                        c.efforts.iter().map(|e| (e.clone(), json!({ "reasoning": { "effort": e } }))).collect();
                    entry.insert("variants".into(), Value::Object(variants));
                }
            }
            Ok(Route {
                key_var,
                args: Vec::new(),
                model: format!("openrouter/{model}"),
                opencode_provider: Some(json!({ "openrouter": { "models": { model: Value::Object(entry) } } })),
            })
        }
        _ => unreachable!("route_support refused it"),
    }
}

/// `POST /analytics/query` with the management key: what a key has cost between two times.
pub fn key_cost_query(hash: &str, from_ms: i64, to_ms: i64) -> String {
    json!({
        "metrics": ["total_usage"],
        "filters": [{ "field": "api_key_id", "operator": "eq", "value": hash }],
        "time_range": { "start": iso_utc_secs(from_ms), "end": iso_utc_secs(to_ms) }
    })
    .to_string()
}

/// The cost from [`key_cost_query`]'s answer, which is nested twice (`data.data[0].total_usage`).
/// `Some(None)`: no row yet, which means not posted, never $0. `None`: not that shape.
pub fn parse_key_cost(body: &str) -> Option<Option<f64>> {
    let v: Value = serde_json::from_str(body).ok()?;
    let rows = v.pointer("/data/data")?.as_array()?;
    let Some(first) = rows.first() else { return Some(None) };
    let cost = match first.get("total_usage")? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) if !s.trim().is_empty() => s.trim().parse().ok(),
        _ => None,
    };
    Some(cost.filter(|c: &f64| c.is_finite() && *c >= 0.0))
}

/// A model as the catalog lists it, with prices per million tokens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogModel {
    /// `vendor/model`, with `:free` for the free variants.
    pub id: String,
    pub name: String,
    pub prompt_per_mtok: Option<f64>,
    pub completion_per_mtok: Option<f64>,
    pub context_length: Option<u32>,
    pub max_output: Option<u32>,
    /// The reasoning efforts the model takes, lowest first; empty when it takes none.
    pub efforts: Vec<String>,
    pub tools: bool,
    pub free: bool,
}

/// Effort names across the catalog, lowest first.
const EFFORT_LADDER: [&str; 7] = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];

/// A price string per token (`"0.000003"`) as dollars per million tokens.
fn per_mtok(v: Option<&Value>) -> Option<f64> {
    let p: f64 = v?.as_str()?.parse().ok()?;
    (p.is_finite() && p >= 0.0).then(|| (p * 1_000_000.0 * 1e6).round() / 1e6)
}

/// OpenRouter's `GET /models` answer as catalog entries. Entries without an id are skipped;
/// `None` when the answer isn't a model list at all.
pub fn parse_openrouter_catalog(body: &str) -> Option<Vec<CatalogModel>> {
    let v: Value = serde_json::from_str(body).ok()?;
    let list = v.get("data")?.as_array()?;
    let num = |x: Option<&Value>| x.and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
    Some(
        list.iter()
            .filter_map(|m| {
                let id = m.get("id")?.as_str()?.to_owned();
                let pricing = m.get("pricing");
                let mut efforts: Vec<String> = m
                    .pointer("/reasoning/supported_efforts")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                    .unwrap_or_default();
                efforts.sort_by_key(|e| EFFORT_LADDER.iter().position(|l| l == e).unwrap_or(EFFORT_LADDER.len()));
                let tools = m
                    .get("supported_parameters")
                    .and_then(Value::as_array)
                    .is_some_and(|a| a.iter().any(|p| p.as_str() == Some("tools")));
                let prompt = per_mtok(pricing.and_then(|p| p.get("prompt")));
                let completion = per_mtok(pricing.and_then(|p| p.get("completion")));
                Some(CatalogModel {
                    name: m.get("name").and_then(Value::as_str).unwrap_or(&id).to_owned(),
                    free: id.ends_with(":free") || (prompt == Some(0.0) && completion == Some(0.0)),
                    id,
                    prompt_per_mtok: prompt,
                    completion_per_mtok: completion,
                    context_length: num(m.get("context_length")),
                    max_output: num(m.pointer("/top_provider/max_completion_tokens")),
                    efforts,
                    tools,
                })
            })
            .collect(),
    )
}

/// An OpenAI-compatible gateway's `GET /models` answer (`{"data":[{"id":…}]}`): ids only, as such
/// gateways rarely publish prices.
pub fn parse_gateway_catalog(body: &str) -> Option<Vec<CatalogModel>> {
    let v: Value = serde_json::from_str(body).ok()?;
    let list = v.get("data")?.as_array()?;
    Some(
        list.iter()
            .filter_map(|m| m.get("id")?.as_str())
            .map(|id| CatalogModel {
                id: id.to_owned(),
                name: id.to_owned(),
                prompt_per_mtok: None,
                completion_per_mtok: None,
                context_length: None,
                max_output: None,
                efforts: Vec::new(),
                tools: false,
                free: false,
            })
            .collect(),
    )
}

/// Every key Chorus 2.0 mints starts with this, so a sweep at start-up can find the ones a crash
/// left behind and never touches anyone else's (Chorus 1 names its own differently).
pub const CAPPED_KEY_PREFIX: &str = "chorus2-key-";
/// A minted key also expires by itself, in case Chorus can't revoke it.
pub const CAPPED_KEY_LIFETIME_MS: i64 = 24 * 60 * 60 * 1000;

pub fn capped_key_name(ledger_id: u32) -> String {
    format!("{CAPPED_KEY_PREFIX}{ledger_id}")
}

/// The ledger row (`capped_keys.id`) a key was minted for, from its name.
pub fn capped_key_ledger(name: &str) -> Option<u32> {
    name.strip_prefix(CAPPED_KEY_PREFIX)?.parse().ok()
}

/// `POST /keys` with the management key: a key that stops at `limit_usd` and expires.
pub fn capped_key_body(ledger_id: u32, limit_usd: f64, now_ms: i64) -> String {
    json!({
        "name": capped_key_name(ledger_id),
        "limit": limit_usd,
        "expires_at": iso_utc_secs(now_ms + CAPPED_KEY_LIFETIME_MS),
    })
    .to_string()
}

/// A minted key: the plaintext (returned only once, so it lives only in the child's environment)
/// and the hash that names it for usage and revocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MintedKey {
    pub key: String,
    pub hash: String,
}

pub fn parse_minted_key(body: &str) -> Option<MintedKey> {
    let v: Value = serde_json::from_str(body).ok()?;
    let key = v.get("key")?.as_str()?.to_owned();
    let hash = v.pointer("/data/hash")?.as_str()?.to_owned();
    (!key.is_empty() && is_key_hash(&hash)).then_some(MintedKey { key, hash })
}

/// A key hash is hex; anything else never goes into a URL path.
pub fn is_key_hash(s: &str) -> bool {
    (16..=128).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// A key's spend so far in dollars (`GET /keys/{hash}` → `data.usage`).
pub fn parse_key_usage(body: &str) -> Option<f64> {
    let v: Value = serde_json::from_str(body).ok()?;
    v.pointer("/data/usage")?.as_f64().filter(|u| u.is_finite() && *u >= 0.0)
}

/// `(hash, name)` of every key in `GET /keys`.
pub fn parse_key_list(body: &str) -> Option<Vec<(String, String)>> {
    let v: Value = serde_json::from_str(body).ok()?;
    Some(
        v.get("data")?
            .as_array()?
            .iter()
            .filter_map(|k| Some((k.get("hash")?.as_str()?.to_owned(), k.get("name")?.as_str()?.to_owned())))
            .collect(),
    )
}

/// Unix milliseconds as `YYYY-MM-DDTHH:MM:SSZ` (OpenRouter rejects timestamps without seconds).
pub fn iso_utc_secs(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// The smallest Jev request: one yes/no question. Its answer proves the key works.
pub fn jev_test_body() -> String {
    json!({
        "state": "Chorus is checking that this API key works.",
        "model": JEV_MODEL,
        "questions": { "ok": { "type": "noul", "instructions": "Is this a connection test?" } }
    })
    .to_string()
}

/// The model that answered (`jev-1.14.0`), from a 200 answer to [`jev_test_body`].
pub fn jev_answer_model(body: &str) -> Option<String> {
    let v: Value = serde_json::from_str(body).ok()?;
    v.pointer("/answers/ok/noul")?.as_f64()?;
    Some(v.get("model")?.as_str()?.to_owned())
}

/// What a connection test found, in words to show as is.
pub fn test_failure(service: &str, status: u16) -> String {
    match status {
        401 | 403 => format!("{service} refused the key ({status}): it is wrong, revoked or lacks access."),
        402 => format!("{service} says the account is out of credit (402)."),
        429 | 529 => format!("{service} is rate-limiting or overloaded ({status}); try again shortly."),
        _ => format!("{service} answered {status}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG: &str = r#"{"data":[
        {"id":"minimax/minimax-m3.1-flash:free","name":"MiniMax: MiniMax M3.1 Flash (free)","context_length":1000000,
         "pricing":{"prompt":"0","completion":"0"},"top_provider":{"max_completion_tokens":524288},
         "supported_parameters":["reasoning","tools"],"reasoning":{"supported_efforts":["max","xhigh","high","medium","low"]}},
        {"id":"anthropic/claude-opus-5.5","name":"Anthropic: Claude Opus 5.5","context_length":1000000,
         "pricing":{"prompt":"0.000005","completion":"0.000025"},"supported_parameters":["tools"]},
        {"name":"no id"}],"total_count":3}"#;

    #[test]
    fn reads_the_openrouter_catalog() {
        let models = parse_openrouter_catalog(CATALOG).unwrap();
        assert_eq!(models.len(), 2);
        let free = &models[0];
        assert!(free.free && free.tools);
        assert_eq!(free.efforts, ["low", "medium", "high", "xhigh", "max"]);
        assert_eq!((free.context_length, free.max_output), (Some(1_000_000), Some(524_288)));
        let opus = &models[1];
        assert_eq!((opus.prompt_per_mtok, opus.completion_per_mtok), (Some(5.0), Some(25.0)));
        assert!(!opus.free && opus.efforts.is_empty() && opus.max_output.is_none());
        assert!(parse_openrouter_catalog("[]").is_none());
    }

    #[test]
    fn reads_a_gateway_catalog() {
        let models = parse_gateway_catalog(r#"{"object":"list","data":[{"id":"gpt-5.5"},{"id":"llama-4"},{"x":1}]}"#).unwrap();
        assert_eq!(models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["gpt-5.5", "llama-4"]);
    }

    #[test]
    fn capped_keys_are_named_capped_and_expire() {
        let body: Value = serde_json::from_str(&capped_key_body(12, 0.5, 1_792_427_729_900)).unwrap();
        assert_eq!(body["name"], "chorus2-key-12");
        assert_eq!(body["limit"], 0.5);
        assert_eq!(body["expires_at"], "2026-10-20T16:35:29Z");
        assert_eq!(capped_key_ledger("chorus2-key-12"), Some(12));
        assert_eq!(capped_key_ledger("chorus-key-12"), None);
        assert_eq!(capped_key_ledger("chorus2-key-x"), None);
        let created = r#"{"data":{"hash":"f01d52606dc8f0a8303a7b5cc3fa07109c2e346cec7c0a16b40de462992ce943","name":"chorus2-key-12","limit":0.5},"key":"minted-value"}"#;
        assert_eq!(
            parse_minted_key(created),
            Some(MintedKey { key: "minted-value".into(), hash: "f01d52606dc8f0a8303a7b5cc3fa07109c2e346cec7c0a16b40de462992ce943".into() })
        );
        assert_eq!(parse_minted_key(r#"{"data":{"hash":"../x"},"key":"k"}"#), None);
        assert_eq!(parse_key_usage(r#"{"data":{"usage":0.0123}}"#), Some(0.0123));
        assert_eq!(
            parse_key_list(r#"{"data":[{"hash":"aa","name":"chorus2-key-1"},{"hash":"bb"}]}"#).unwrap(),
            [("aa".to_owned(), "chorus2-key-1".to_owned())]
        );
        assert_eq!(iso_utc_secs(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn which_agents_take_which_providers() {
        use ProviderKind::*;
        assert!(route_support(AgentKind::Codex, Openrouter).is_ok() && route_support(AgentKind::Codex, Gateway).is_ok());
        assert!(route_support(AgentKind::Opencode, Openrouter).is_ok());
        for (agent, kind) in [
            (AgentKind::Opencode, Gateway),
            (AgentKind::Claude, Openrouter),
            (AgentKind::Grok, Openrouter),
            (AgentKind::Shell, Gateway),
            (AgentKind::Codex, Jev),
        ] {
            assert!(!route_support(agent, kind).unwrap_err().is_empty(), "{agent:?} {kind:?}");
        }
    }

    #[test]
    fn codex_gets_its_provider_from_config_overrides() {
        let r = route(AgentKind::Codex, ProviderKind::Openrouter, "OpenRouter", "", "minimax/minimax-m3.1-flash:free", None).unwrap();
        assert_eq!(r.key_var, "OPENROUTER_API_KEY");
        assert_eq!(r.model, "minimax/minimax-m3.1-flash:free");
        assert_eq!(
            r.args,
            [
                "-c",
                "model_provider=\"chorus\"",
                "-c",
                "model_providers.chorus.name=\"OpenRouter\"",
                "-c",
                "model_providers.chorus.base_url=\"https://openrouter.ai/api/v1\"",
                "-c",
                "model_providers.chorus.env_key=\"OPENROUTER_API_KEY\"",
                "-c",
                "model_providers.chorus.wire_api=\"responses\""
            ]
        );
        // With the catalog's context window.
        let models = parse_openrouter_catalog(CATALOG).unwrap();
        let known = route(AgentKind::Codex, ProviderKind::Openrouter, "OpenRouter", "", &models[0].id, Some(&models[0])).unwrap();
        assert_eq!(known.args[known.args.len() - 2..], ["-c", "model_context_window=1000000"]);
        let g = route(AgentKind::Codex, ProviderKind::Gateway, "Work gateway", "https://llm.example.com/v1/", "gpt-5.5", None).unwrap();
        assert!(g.args.contains(&"model_providers.chorus.base_url=\"https://llm.example.com/v1\"".to_owned()));
        assert_eq!(g.key_var, "CHORUS_GATEWAY_KEY");
        assert!(route(AgentKind::Codex, ProviderKind::Gateway, "x", "http://example.com", "m", None).is_err());
        assert!(route(AgentKind::Codex, ProviderKind::Gateway, "a\"b", "https://x.example", "m", None).is_err());
    }

    #[test]
    fn opencode_gets_the_model_with_its_catalog_efforts() {
        let models = parse_openrouter_catalog(CATALOG).unwrap();
        let r = route(AgentKind::Opencode, ProviderKind::Openrouter, "OpenRouter", "", &models[0].id, Some(&models[0])).unwrap();
        assert_eq!(r.model, "openrouter/minimax/minimax-m3.1-flash:free");
        let entry = &r.opencode_provider.unwrap()["openrouter"]["models"]["minimax/minimax-m3.1-flash:free"];
        assert_eq!(entry["variants"]["high"], json!({ "reasoning": { "effort": "high" } }));
        assert_eq!(entry["limit"], json!({ "context": 1_000_000, "output": 524_288 }));
        // Unknown to the catalog: still declared, so opencode runs it.
        let bare = route(AgentKind::Opencode, ProviderKind::Openrouter, "OpenRouter", "", "x/y", None).unwrap();
        assert_eq!(bare.opencode_provider.unwrap(), json!({ "openrouter": { "models": { "x/y": {} } } }));
    }

    #[test]
    fn base_urls_and_names_are_checked() {
        assert_eq!(clean_base_url(" https://gw.example.com/v1/ ").unwrap(), "https://gw.example.com/v1");
        assert!(clean_base_url("http://localhost:8080/v1").is_ok() && clean_base_url("http://127.0.0.1").is_ok());
        for bad in ["http://gw.example.com", "http://localhost.evil.com", "ftp://x", "https://a b", "https://a\"b", "https://a\\b"] {
            assert!(clean_base_url(bad).is_err(), "{bad}");
        }
        assert!(valid_provider_name("OpenRouter (work)") && !valid_provider_name("") && !valid_provider_name("a\"b"));
        assert_eq!(ProviderKind::parse("gateway"), Some(ProviderKind::Gateway));
    }

    #[test]
    fn a_keys_cost_is_read_from_analytics() {
        let q: Value = serde_json::from_str(&key_cost_query("ab12", 0, 60_000)).unwrap();
        assert_eq!(q["filters"][0], json!({ "field": "api_key_id", "operator": "eq", "value": "ab12" }));
        assert_eq!(q["time_range"]["end"], "1970-01-01T00:01:00Z");
        assert_eq!(parse_key_cost(r#"{"data":{"data":[{"total_usage":0.0042}],"metadata":{}}}"#), Some(Some(0.0042)));
        assert_eq!(parse_key_cost(r#"{"data":{"data":[{"total_usage":"0.5"}]}}"#), Some(Some(0.5)));
        // Not posted yet is unknown, never $0.
        assert_eq!(parse_key_cost(r#"{"data":{"data":[]}}"#), Some(None));
        assert_eq!(parse_key_cost(r#"{"data":[]}"#), None);
    }

    #[test]
    fn the_jev_test_is_one_yes_no_question() {
        let body: Value = serde_json::from_str(&jev_test_body()).unwrap();
        assert_eq!(body["model"], "jev-latest");
        assert_eq!(body["questions"]["ok"]["type"], "noul");
        let answer = r#"{"model":"jev-1.14.0","answers":{"ok":{"type":"noul","noul":0.98}},"usage":{"input_tokens":290,"output_tokens":20}}"#;
        assert_eq!(jev_answer_model(answer).as_deref(), Some("jev-1.14.0"));
        assert_eq!(jev_answer_model(r#"{"model":"jev-1.14.0","answers":{}}"#), None);
        assert!(test_failure("Jev", 401).contains("refused the key"));
    }
}
