//! Calls to providers: connection tests, model lists, and OpenRouter's capped keys (the shapes are
//! in `chorus_core::providers`). Keys arrive decrypted for one call and go only into an
//! `Authorization` header; no message here ever contains one.

pub mod commands;
pub mod keys;

use std::sync::OnceLock;
use std::time::Duration;

use chorus_core::providers::{self as p, CatalogModel, MintedKey, ProviderKind};

const TIMEOUT: Duration = Duration::from_secs(20);
/// A model list is a few megabytes at most (OpenRouter's is about 2).
const MAX_BODY: usize = 16 * 1024 * 1024;
const MAX_KEY_PAGES: usize = 20;

pub struct Reply {
    pub status: u16,
    pub body: String,
}

fn client() -> Result<&'static reqwest::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| reqwest::Client::builder().timeout(TIMEOUT).build().map_err(|e| format!("making the HTTP client: {e}")))
        .as_ref()
        .map_err(Clone::clone)
}

/// One request. `bearer` is a key, used only as the `Authorization` header.
pub fn send(method: reqwest::Method, url: &str, bearer: Option<&str>, json_body: Option<String>) -> Result<Reply, String> {
    let mut request = client()?.request(method, url);
    if let Some(key) = bearer {
        request = request.bearer_auth(key);
    }
    if let Some(body) = json_body {
        request = request.header(reqwest::header::CONTENT_TYPE, "application/json").body(body);
    }
    let host = url.split('/').nth(2).unwrap_or(url).to_owned();
    tauri::async_runtime::block_on(async move {
        // The error names the host only: never the request, which carries the key.
        let response = request.send().await.map_err(|e| format!("couldn't reach {host}: {}", short(&e)))?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await.map_err(|e| format!("reading {host}'s answer: {}", short(&e)))?;
        if bytes.len() > MAX_BODY {
            return Err(format!("{host}'s answer was larger than {} MB", MAX_BODY >> 20));
        }
        Ok(Reply { status, body: String::from_utf8_lossy(&bytes).into_owned() })
    })
}

fn short(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() {
        "it timed out"
    } else if e.is_connect() {
        "no connection"
    } else {
        "the request failed"
    }
}

fn get(url: &str, bearer: Option<&str>) -> Result<Reply, String> {
    send(reqwest::Method::GET, url, bearer, None)
}

/// What a provider's connection test found. `key` is its API key, `management` OpenRouter's
/// management key; either may be missing.
pub fn test(kind: ProviderKind, base_url: Option<&str>, key: Option<&str>, management: Option<&str>) -> (bool, String) {
    let mut ok = true;
    let mut notes: Vec<String> = Vec::new();
    let mut check = |result: Result<String, String>| match result {
        Ok(n) => notes.push(n),
        Err(n) => {
            ok = false;
            notes.push(n);
        }
    };
    match kind {
        ProviderKind::Openrouter => {
            match key {
                Some(k) => check(openrouter_key(k)),
                None if management.is_none() => check(Err("No key is set.".into())),
                None => {}
            }
            if let Some(m) = management {
                check(management_key(m));
            }
        }
        ProviderKind::Gateway => check(match (base_url, key) {
            (Some(url), Some(k)) => gateway(url, k),
            (None, _) => Err("No address is set.".into()),
            (_, None) => Err("No key is set.".into()),
        }),
        ProviderKind::Jev => check(match key {
            Some(k) => jev(k),
            None => Err("No key is set.".into()),
        }),
        ProviderKind::Ollama => check(ollama_base(base_url).and_then(|base| {
            ollama_tags(&base).map(|models| format!("Ollama answered at {base} and lists {} local models.", models.len()))
        })),
    }
    (ok, notes.join(" "))
}

/// Ollama's address: the provider's own, or its default. Only `https://`, `localhost` or
/// `127.0.0.1` pass (a local model is never reached over plain http elsewhere).
pub fn ollama_base(base_url: Option<&str>) -> Result<String, String> {
    match base_url.map(str::trim).filter(|u| !u.is_empty()) {
        Some(u) => p::clean_base_url(u),
        None => Ok(p::OLLAMA_URL.to_owned()),
    }
}

/// Ollama's local models, or the sentence that says why there are none: Chorus never starts it.
pub fn ollama_tags(base: &str) -> Result<Vec<CatalogModel>, String> {
    let r = get(&format!("{base}/api/tags"), None).map_err(|_| format!("Ollama isn't running at {base}."))?;
    if r.status != 200 {
        return Err(format!("Ollama answered {} at {base}.", r.status));
    }
    p::parse_ollama_tags(&r.body).ok_or_else(|| format!("Ollama's list at {base} isn't in the shape Chorus reads."))
}

/// `GET /key` answers for the key itself; it shows the key's spend and limit.
fn openrouter_key(key: &str) -> Result<String, String> {
    let r = get(&format!("{}/key", p::OPENROUTER_API), Some(key))?;
    if r.status != 200 {
        return Err(p::test_failure("OpenRouter", r.status));
    }
    let v: serde_json::Value = serde_json::from_str(&r.body).map_err(|_| "OpenRouter's answer wasn't JSON.".to_owned())?;
    let usage = v.pointer("/data/usage").and_then(serde_json::Value::as_f64);
    let limit = v.pointer("/data/limit").and_then(serde_json::Value::as_f64);
    Ok(match (usage, limit) {
        (Some(u), Some(l)) => format!("The key works (${u:.2} used of its ${l:.2} limit)."),
        (Some(u), None) => format!("The key works (${u:.2} used, no limit)."),
        _ => "The key works.".into(),
    })
}

/// A management key can list keys but not run models.
fn management_key(key: &str) -> Result<String, String> {
    let r = get(&format!("{}/keys?offset=0", p::OPENROUTER_API), Some(key))?;
    match r.status {
        200 => Ok("The management key works, so each run gets its own capped key.".into()),
        s => Err(format!("Management key: {}", p::test_failure("OpenRouter", s))),
    }
}

fn gateway(base_url: &str, key: &str) -> Result<String, String> {
    let r = get(&format!("{base_url}/models"), Some(key))?;
    if r.status != 200 {
        return Err(p::test_failure("The gateway", r.status));
    }
    match p::parse_catalog(&r.body) {
        Some(m) => Ok(format!("The gateway answered and lists {} models.", m.len())),
        None => Ok("The gateway answered, but its model list isn't the OpenAI shape.".into()),
    }
}

/// One tiny question, the only way to check a Jev key (about $0.00001).
fn jev(key: &str) -> Result<String, String> {
    let r = send(reqwest::Method::POST, p::JEV_URL, Some(key), Some(p::jev_test_body()))?;
    if r.status != 200 {
        return Err(p::test_failure("Jev", r.status));
    }
    match p::jev_answer_model(&r.body) {
        Some(model) => Ok(format!("Jev answered ({model}).")),
        None => Err("Jev answered, but not in the shape Chorus checked.".into()),
    }
}

/// The provider's model list. OpenRouter's is public; a gateway's needs its key.
pub fn fetch_catalog(kind: ProviderKind, base_url: Option<&str>, key: Option<&str>) -> Result<Vec<CatalogModel>, String> {
    if kind == ProviderKind::Ollama {
        return ollama_tags(&ollama_base(base_url)?);
    }
    let url = match (kind, base_url) {
        (ProviderKind::Openrouter, _) => format!("{}/models", p::OPENROUTER_API),
        (ProviderKind::Gateway, Some(u)) => format!("{u}/models"),
        (ProviderKind::Gateway, None) => return Err("No address is set.".into()),
        // Ollama returned above.
        (ProviderKind::Jev | ProviderKind::Ollama, _) => return Err("Jev has no model list to choose from.".into()),
    };
    let bearer = if kind == ProviderKind::Gateway { key } else { None };
    let r = get(&url, bearer)?;
    if r.status != 200 {
        return Err(p::test_failure("The model list", r.status));
    }
    p::parse_catalog(&r.body).ok_or_else(|| "The model list isn't in the shape Chorus reads.".into())
}

/// Mints a key that stops at `limit_usd` and expires within a day.
pub fn mint(management: &str, ledger_id: u32, limit_usd: f64) -> Result<MintedKey, String> {
    let body = p::capped_key_body(ledger_id, limit_usd, crate::store::now_ms());
    let r = send(reqwest::Method::POST, &format!("{}/keys", p::OPENROUTER_API), Some(management), Some(body))?;
    if !(200..300).contains(&r.status) {
        return Err(p::test_failure("OpenRouter", r.status));
    }
    p::parse_minted_key(&r.body).ok_or_else(|| "OpenRouter's new key wasn't in the shape Chorus reads.".into())
}

/// Deletes a minted key. A key that is already gone counts as revoked.
pub fn revoke(management: &str, hash: &str) -> Result<(), String> {
    if !p::is_key_hash(hash) {
        return Err("not a key hash".into());
    }
    let r = send(reqwest::Method::DELETE, &format!("{}/keys/{hash}", p::OPENROUTER_API), Some(management), None)?;
    match r.status {
        200..300 | 404 => Ok(()),
        s => Err(p::test_failure("OpenRouter", s)),
    }
}

/// What OpenRouter's analytics say a key cost between two times; `None` until it is posted.
pub fn key_cost(management: &str, hash: &str, from_ms: i64, to_ms: i64) -> Result<Option<f64>, String> {
    let body = p::key_cost_query(hash, from_ms, to_ms);
    let r = send(reqwest::Method::POST, &format!("{}/analytics/query", p::OPENROUTER_API), Some(management), Some(body))?;
    if r.status != 200 {
        return Err(p::test_failure("OpenRouter analytics", r.status));
    }
    p::parse_key_cost(&r.body).ok_or_else(|| "OpenRouter's analytics answer wasn't in the shape Chorus reads.".into())
}

/// Every key on the account, as `(hash, name)`.
pub fn list_keys(management: &str) -> Result<Vec<(String, String)>, String> {
    let mut all = Vec::new();
    for page in 0..MAX_KEY_PAGES {
        let r = get(&format!("{}/keys?offset={}", p::OPENROUTER_API, all.len()), Some(management))?;
        if r.status != 200 {
            return Err(p::test_failure("OpenRouter", r.status));
        }
        let keys = p::parse_key_list(&r.body).ok_or("OpenRouter's key list wasn't in the shape Chorus reads.")?;
        if keys.is_empty() || page + 1 == MAX_KEY_PAGES {
            all.extend(keys);
            break;
        }
        all.extend(keys);
    }
    Ok(all)
}
