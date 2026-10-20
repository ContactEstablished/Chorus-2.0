//! The only routing requests Chorus sends to OpenRouter (ported from Chorus 1's `routingClient.ts`):
//! the keyed endpoints list, the two zero-cost account checks, and one cache-probe call. One
//! destination (OpenRouter's API, never followed through a redirect), a 10 s limit, a size cap per
//! answer, no retry. The key goes into the `Authorization` header and nowhere else. A body that
//! isn't 2xx is never read, except a preflight's expected 404, and only tags, numbers and a fixed
//! vocabulary leave this module.

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;

use chorus_core::providers::OPENROUTER_API;
use chorus_core::routing::cache_probe::{ProbeCall, extract_probe_usage, probe_request_body};
use chorus_core::routing::endpoints::parse_endpoints_response;
use chorus_core::routing::preflight::{PreflightIssue, PreflightStep, parse_preflight, preflight_request_body};
use chorus_core::routing::types::{RawEndpoint, is_routing_tag};
use chorus_core::scrub::{builtin_patterns, scrub_all};
use serde::Serialize;
use specta::Type;

const TIMEOUT: Duration = Duration::from_secs(10);
const ENDPOINTS_CAP: usize = 2_000_000;
const PREFLIGHT_CAP: usize = 65_536;
const PROBE_CAP: usize = 262_144;

/// Why a routing request got no usable answer (Chorus 1's fixed vocabulary).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Failure {
    Unreachable,
    AuthFailed,
    RateLimited,
    ProviderError,
    UnexpectedStatus,
    Unrecognized,
    ModelMismatch,
}

impl Failure {
    pub fn message(self) -> &'static str {
        match self {
            Self::Unreachable => "Could not reach OpenRouter.",
            Self::AuthFailed => "Authentication failed — the credential was rejected.",
            Self::RateLimited => "Rate limited by OpenRouter.",
            Self::ProviderError => "OpenRouter returned an error.",
            Self::UnexpectedStatus => "Unexpected response from OpenRouter.",
            Self::Unrecognized => "OpenRouter returned an unrecognized response.",
            Self::ModelMismatch => "OpenRouter returned endpoints for a different model.",
        }
    }

    /// An answer that stops every further keyed call in the same refresh or tick.
    pub fn stops_further_calls(self) -> bool {
        matches!(self, Self::AuthFailed | Self::RateLimited)
    }

    fn for_status(status: u16) -> Self {
        match status {
            401 | 403 => Self::AuthFailed,
            429 => Self::RateLimited,
            s if s >= 500 => Self::ProviderError,
            _ => Self::UnexpectedStatus,
        }
    }
}

fn client() -> Option<&'static reqwest::Client> {
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| reqwest::Client::builder().timeout(TIMEOUT).redirect(reqwest::redirect::Policy::none()).build().ok())
        .as_ref()
}

/// One request: its status, and its body only when `read` says to read it (within `cap`). `None`
/// when it couldn't be sent or answered; the error is dropped whole, since it can name the request.
fn send(url: &str, key: &str, body: Option<String>, read: impl Fn(u16) -> bool, cap: usize) -> Option<(u16, Option<Vec<u8>>)> {
    let client = client()?;
    let mut request = match &body {
        Some(_) => client.post(url),
        None => client.get(url),
    }
    .header(reqwest::header::ACCEPT, "application/json")
    .bearer_auth(key);
    if let Some(body) = body {
        request = request.header(reqwest::header::CONTENT_TYPE, "application/json").body(body);
    }
    tauri::async_runtime::block_on(async move {
        let mut response = request.send().await.ok()?;
        let status = response.status().as_u16();
        if !read(status) {
            // Dropped unread: the answer most likely to echo a key is one never read.
            return Some((status, None));
        }
        let mut out = Vec::new();
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) if out.len() + chunk.len() <= cap => out.extend_from_slice(&chunk),
                // Over the cap, or unreadable: as if there were no body.
                Ok(Some(_)) | Err(_) => return Some((status, None)),
                Ok(None) => return Some((status, Some(out))),
            }
        }
    })
}

fn json(bytes: Option<Vec<u8>>) -> Option<serde_json::Value> {
    serde_json::from_slice(&bytes?).ok()
}

/// A tag that a key-shaped scrub would change is never trusted.
fn clean_tag(tag: &str) -> bool {
    is_routing_tag(tag) && scrub_all(tag.as_bytes(), &[], &builtin_patterns()) == tag.as_bytes()
}

fn is_2xx(status: u16) -> bool {
    (200..300).contains(&status)
}

/// `GET /models/{slug}/endpoints`: the model's endpoints, and how many rows were rejected.
pub fn fetch_endpoints(key: &str, model: &str) -> Result<(Vec<RawEndpoint>, u32), Failure> {
    if key.is_empty() {
        return Err(Failure::AuthFailed);
    }
    let path: Vec<String> = model.split('/').map(urlencoding).collect();
    let url = format!("{OPENROUTER_API}/models/{}/endpoints", path.join("/"));
    let (status, body) = send(&url, key, None, is_2xx, ENDPOINTS_CAP).ok_or(Failure::Unreachable)?;
    if !is_2xx(status) {
        return Err(Failure::for_status(status));
    }
    let value = json(body).ok_or(Failure::Unrecognized)?;
    let parsed = parse_endpoints_response(&value).map_err(|_| Failure::Unrecognized)?;
    if parsed.model_id != model {
        return Err(Failure::ModelMismatch);
    }
    let total = parsed.endpoints.len();
    let endpoints: Vec<RawEndpoint> = parsed.endpoints.into_iter().filter(|e| clean_tag(&e.tag)).collect();
    if endpoints.is_empty() {
        return Err(Failure::Unrecognized);
    }
    let rejected = (parsed.rejected.len() + total - endpoints.len()) as u32;
    Ok((endpoints, rejected))
}

/// One zero-cost account check: a request pinned to a tag that doesn't exist, whose expected 404
/// lists what the account's filters removed. `Ok(Err(issue))` is "unknown".
pub fn preflight(key: &str, model: &str, step: PreflightStep, rows: &BTreeMap<String, u32>) -> Result<Result<Vec<String>, PreflightIssue>, Failure> {
    if key.is_empty() {
        return Err(Failure::AuthFailed);
    }
    let body = serde_json::to_string(&preflight_request_body(model, step)).map_err(|_| Failure::Unrecognized)?;
    let url = format!("{OPENROUTER_API}/chat/completions");
    let (status, body) = send(&url, key, Some(body), |s| s == 404, PREFLIGHT_CAP).ok_or(Failure::Unreachable)?;
    if status == 404 {
        let Some(value) = json(body) else { return Ok(Err(PreflightIssue::UnrecognizedBody)) };
        return Ok(match parse_preflight(404, &value, step, rows) {
            Ok(removed) if removed.iter().all(|t| clean_tag(t)) => Ok(removed),
            Ok(_) => Err(PreflightIssue::BadTag),
            Err(issue) => Err(issue),
        });
    }
    // A 2xx means the made-up tag was served: the check learned nothing.
    if is_2xx(status) {
        return Ok(Err(PreflightIssue::UnexpectedStatus));
    }
    Err(Failure::for_status(status))
}

/// One cache-probe call pinned to `tag`. Only the usage numbers leave.
pub fn probe(key: &str, model: &str, tag: &str, nonce: &str) -> (ProbeCall, Option<Failure>) {
    if key.is_empty() {
        return (ProbeCall::Failed, Some(Failure::AuthFailed));
    }
    let Ok(body) = serde_json::to_string(&probe_request_body(model, tag, nonce)) else { return (ProbeCall::Failed, Some(Failure::Unrecognized)) };
    let url = format!("{OPENROUTER_API}/chat/completions");
    match send(&url, key, Some(body), is_2xx, PROBE_CAP) {
        None => (ProbeCall::Failed, Some(Failure::Unreachable)),
        Some((status, _)) if !is_2xx(status) => (ProbeCall::Failed, Some(Failure::for_status(status))),
        // A 2xx whose body couldn't be read may still have been billed: it counts its estimate.
        Some((_, body)) => (ProbeCall::Ok(json(body).map(|v| extract_probe_usage(&v)).unwrap_or_default()), None),
    }
}

/// Percent-encodes one path segment (RFC 3986 unreserved characters pass).
fn urlencoding(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_and_segments_map_as_in_chorus_1() {
        assert_eq!(Failure::for_status(401), Failure::AuthFailed);
        assert_eq!(Failure::for_status(429), Failure::RateLimited);
        assert_eq!(Failure::for_status(503), Failure::ProviderError);
        assert_eq!(Failure::for_status(302), Failure::UnexpectedStatus);
        assert!(Failure::RateLimited.stops_further_calls() && !Failure::Unreachable.stops_further_calls());
        assert_eq!(urlencoding("deepseek-v4.1-flash:nitro"), "deepseek-v4.1-flash%3Anitro");
        assert!(clean_tag("coreweave/fp8") && !clean_tag("bad tag"));
        assert_eq!(fetch_endpoints("", "a/b").unwrap_err(), Failure::AuthFailed);
    }
}
