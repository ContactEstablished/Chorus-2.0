//! Tokens and cost per turn, as the agent reported them (`_docs/HANDOFF-P11-measurement.md` §4).
//! Every figure is optional: unknown is not 0.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

/// Where a figure came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum UsageSource {
    /// opencode's server events (`session.step.ended`).
    OpencodeServer,
}

impl UsageSource {
    pub fn as_str(self) -> &'static str {
        match self {
            UsageSource::OpencodeServer => "opencode-server",
        }
    }
}

/// One turn's usage, or a sum of turns'.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    /// Prompt tokens not read from the cache.
    pub input: Option<u32>,
    /// Prompt tokens read from the cache.
    pub cached: Option<u32>,
    pub cache_write: Option<u32>,
    pub output: Option<u32>,
    pub reasoning: Option<u32>,
    /// What the agent says it cost, in US dollars. opencode reports 0 for its free models.
    pub cost_usd: Option<f64>,
}

fn add_u32(a: Option<u32>, b: Option<u32>) -> Option<u32> {
    match (a, b) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0).saturating_add(b.unwrap_or(0))),
    }
}

impl TokenUsage {
    /// Adds `other`. A figure stays unknown only when neither side knows it.
    pub fn add(&mut self, other: &TokenUsage) {
        self.input = add_u32(self.input, other.input);
        self.cached = add_u32(self.cached, other.cached);
        self.cache_write = add_u32(self.cache_write, other.cache_write);
        self.output = add_u32(self.output, other.output);
        self.reasoning = add_u32(self.reasoning, other.reasoning);
        self.cost_usd = match (self.cost_usd, other.cost_usd) {
            (None, None) => None,
            (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
        };
    }

    pub fn is_empty(&self) -> bool {
        *self == TokenUsage::default()
    }

    /// From an opencode 2.x `session.step.ended` event's data: `cost` and
    /// `tokens {input, output, reasoning, cache {read, write}}` (2.1.4, LESSONS L69). `None` when
    /// it carries neither.
    pub fn from_opencode_step(data: &Value) -> Option<TokenUsage> {
        let n = |p: &str| data.pointer(p).and_then(Value::as_u64).map(|v| u32::try_from(v).unwrap_or(u32::MAX));
        let usage = TokenUsage {
            input: n("/tokens/input"),
            cached: n("/tokens/cache/read"),
            cache_write: n("/tokens/cache/write"),
            output: n("/tokens/output"),
            reasoning: n("/tokens/reasoning"),
            cost_usd: data.get("cost").and_then(Value::as_f64),
        };
        (!usage.is_empty()).then_some(usage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_opencode_step_reads_its_tokens_and_cost() {
        // 2.1.4's `session.step.ended`, trimmed to its usage fields.
        let d = json!({"finish":"stop","cost":0,"tokens":{"input":3583,"output":2,"reasoning":0,"cache":{"read":4224,"write":0}}});
        let u = TokenUsage::from_opencode_step(&d).unwrap();
        assert_eq!(
            u,
            TokenUsage { input: Some(3583), cached: Some(4224), cache_write: Some(0), output: Some(2), reasoning: Some(0), cost_usd: Some(0.0) }
        );
        assert_eq!(TokenUsage::from_opencode_step(&json!({"finish":"stop"})), None, "no usage is unknown, not 0");
    }

    #[test]
    fn sums_keep_unknown_apart_from_zero() {
        let mut a = TokenUsage { input: Some(10), cost_usd: Some(0.001), ..Default::default() };
        a.add(&TokenUsage { input: Some(5), output: Some(3), ..Default::default() });
        assert_eq!(a, TokenUsage { input: Some(15), output: Some(3), cost_usd: Some(0.001), ..Default::default() });
        assert_eq!(a.cached, None);
        let mut big = TokenUsage { input: Some(u32::MAX), ..Default::default() };
        big.add(&TokenUsage { input: Some(1), ..Default::default() });
        assert_eq!(big.input, Some(u32::MAX));
    }
}
