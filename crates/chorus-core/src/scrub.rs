//! Secret scrubbing for PTY output, scrollback, history and logs (ARCHITECTURE §8).
//!
//! Two kinds of match, both replaced by [`PLACEHOLDER`]:
//! - **exact values**: every key in the vault that a session was given, longest first;
//! - **key shapes** from `secret-patterns.json`, the same list `scripts/secret-grep.mjs` uses, for
//!   keys that never went through the vault (an agent printing a `.env`, say).
//!
//! The stream scrubber holds back any tail of a chunk that could be the start of a match, so a key
//! split across two reads is still caught. Ported from Chorus 1's `scrubber.ts` (exact values) and
//! `logger.ts` (patterns). Known residual, as in Chorus 1: a TUI that interleaves escape sequences
//! between a key's characters defeats substring matching (the pinned `TERM` avoids the known case).

use serde::Deserialize;

pub const PLACEHOLDER: &[u8] = b"[REDACTED-CREDENTIAL]";

/// A carry longer than this is scrubbed and released anyway, so output can't stall indefinitely.
pub const MAX_CARRY: usize = 8 * 1024;

const PATTERNS_JSON: &str = include_str!("../secret-patterns.json");

#[derive(Debug, Clone, PartialEq, Eq)]
enum Atom {
    Byte(u8),
    Class(Vec<(u8, u8)>),
}

impl Atom {
    fn matches(&self, b: u8) -> bool {
        match self {
            Atom::Byte(x) => *x == b,
            Atom::Class(ranges) => ranges.iter().any(|&(lo, hi)| (lo..=hi).contains(&b)),
        }
    }
}

/// `prefix` atoms, then `tail` repeated `min..=max` times. This is the whole pattern language
/// `secret-patterns.json` uses, so no regex engine is needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    pub name: String,
    prefix: Vec<Atom>,
    tail: Atom,
    min: usize,
    max: Option<usize>,
}

#[derive(Deserialize)]
struct PatternFile {
    patterns: Vec<PatternEntry>,
}

#[derive(Deserialize)]
struct PatternEntry {
    name: String,
    source: String,
}

/// The shared pattern list, compiled. Panics only if the bundled JSON is malformed, which the unit
/// tests rule out.
pub fn builtin_patterns() -> Vec<Pattern> {
    let file: PatternFile = serde_json::from_str(PATTERNS_JSON).expect("secret-patterns.json parses");
    file.patterns
        .into_iter()
        .map(|p| parse_pattern(&p.name, &p.source).unwrap_or_else(|e| panic!("pattern {}: {e}", p.name)))
        .collect()
}

pub fn parse_pattern(name: &str, source: &str) -> Result<Pattern, String> {
    let bytes = source.as_bytes();
    let mut atoms = Vec::new();
    let mut quant: Option<(usize, Option<usize>)> = None;
    let mut i = 0;
    while i < bytes.len() {
        if quant.is_some() {
            return Err("only the last element may have a quantifier".into());
        }
        match bytes[i] {
            b'[' => {
                let end = source[i..].find(']').ok_or("unclosed [")? + i;
                atoms.push(Atom::Class(parse_class(&bytes[i + 1..end])?));
                i = end + 1;
            }
            b'{' => {
                let end = source[i..].find('}').ok_or("unclosed {")? + i;
                let body = &source[i + 1..end];
                quant = Some(match body.split_once(',') {
                    Some((lo, "")) => (lo.parse().map_err(|_| "bad quantifier")?, None),
                    Some((lo, hi)) => {
                        (lo.parse().map_err(|_| "bad quantifier")?, Some(hi.parse().map_err(|_| "bad quantifier")?))
                    }
                    None => {
                        let n = body.parse().map_err(|_| "bad quantifier")?;
                        (n, Some(n))
                    }
                });
                i = end + 1;
            }
            b'\\' | b'(' | b')' | b'|' | b'*' | b'+' | b'?' | b'.' | b'^' | b'$' => {
                return Err(format!("unsupported regex syntax {:?}", bytes[i] as char));
            }
            b => {
                atoms.push(Atom::Byte(b));
                i += 1;
            }
        }
    }
    let (min, max) = quant.ok_or("pattern must end with a quantified class")?;
    let tail = atoms.pop().ok_or("empty pattern")?;
    if !matches!(tail, Atom::Class(_)) || min == 0 {
        return Err("pattern must end with a class repeated at least once".into());
    }
    Ok(Pattern { name: name.to_owned(), prefix: atoms, tail, min, max })
}

fn parse_class(body: &[u8]) -> Result<Vec<(u8, u8)>, String> {
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < body.len() {
        if i + 2 < body.len() && body[i + 1] == b'-' {
            ranges.push((body[i], body[i + 2]));
            i += 3;
        } else {
            ranges.push((body[i], body[i]));
            i += 1;
        }
    }
    if ranges.is_empty() { Err("empty class".into()) } else { Ok(ranges) }
}

impl Pattern {
    /// Length of the longest match starting at `at`, if any.
    fn match_at(&self, s: &[u8], at: usize) -> Option<usize> {
        let mut i = at;
        for atom in &self.prefix {
            if i >= s.len() || !atom.matches(s[i]) {
                return None;
            }
            i += 1;
        }
        let limit = self.max.unwrap_or(usize::MAX);
        let mut n = 0;
        while i < s.len() && n < limit && self.tail.matches(s[i]) {
            i += 1;
            n += 1;
        }
        (n >= self.min).then_some(i - at)
    }

    /// True when `s[at..]` could still grow into a match that extends past the end of `s`.
    fn open_at_end(&self, s: &[u8], at: usize) -> bool {
        let mut i = at;
        for atom in &self.prefix {
            if i == s.len() {
                return true;
            }
            if !atom.matches(s[i]) {
                return false;
            }
            i += 1;
        }
        let mut n = 0;
        while i < s.len() && self.tail.matches(s[i]) {
            i += 1;
            n += 1;
        }
        i == s.len() && self.max.is_none_or(|m| n < m)
    }
}

/// A match to replace: `[start, end)`.
fn find_matches(s: &[u8], secrets: &[Vec<u8>], patterns: &[Pattern]) -> Vec<(usize, usize)> {
    let mut found: Vec<(usize, usize)> = Vec::new();
    for secret in secrets {
        let mut at = 0;
        while let Some(pos) = find(&s[at..], secret) {
            found.push((at + pos, at + pos + secret.len()));
            at += pos + secret.len();
        }
    }
    for p in patterns {
        let mut at = 0;
        while at < s.len() {
            match p.match_at(s, at) {
                Some(len) => {
                    found.push((at, at + len));
                    at += len;
                }
                None => at += 1,
            }
        }
    }
    // Merge overlaps so every byte of every match is replaced exactly once.
    found.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (a, b) in found {
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    merged
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

fn replace(s: &[u8], matches: &[(usize, usize)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut at = 0;
    for &(a, b) in matches {
        out.extend_from_slice(&s[at..a]);
        out.extend_from_slice(PLACEHOLDER);
        at = b;
    }
    out.extend_from_slice(&s[at..]);
    out
}

/// One-shot scrub of a complete text (history rows, logs, exports).
pub fn scrub_all(s: &[u8], secrets: &[Vec<u8>], patterns: &[Pattern]) -> Vec<u8> {
    replace(s, &find_matches(s, secrets, patterns))
}

/// Scrub decoded strings before JSON escaping can hide a credential's quotes or newlines.
/// Keys can contain user text too (for example a question-to-answer map).
pub fn scrub_json(value: &serde_json::Value, secrets: &[Vec<u8>], patterns: &[Pattern]) -> serde_json::Value {
    use serde_json::Value;
    let clean = |s: &str| String::from_utf8_lossy(&scrub_all(s.as_bytes(), secrets, patterns)).into_owned();
    match value {
        Value::String(s) => Value::String(clean(s)),
        Value::Array(a) => Value::Array(a.iter().map(|v| scrub_json(v, secrets, patterns)).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (clean(k), scrub_json(v, secrets, patterns))).collect()),
        v => v.clone(),
    }
}

/// Scrubs a byte stream chunk by chunk.
pub struct StreamScrubber {
    secrets: Vec<Vec<u8>>,
    patterns: Vec<Pattern>,
    carry: Vec<u8>,
    /// After an idle flush released a redacted *prefix* of a secret, the rest of that secret, so its
    /// remainder is dropped if it arrives next.
    pending_rest: Option<Vec<u8>>,
}

impl StreamScrubber {
    pub fn new(secrets: impl IntoIterator<Item = Vec<u8>>, patterns: Vec<Pattern>) -> Self {
        let mut secrets: Vec<Vec<u8>> = secrets.into_iter().filter(|s| !s.is_empty()).collect();
        secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
        secrets.dedup();
        Self { secrets, patterns, carry: Vec::new(), pending_rest: None }
    }

    /// Bytes held back, waiting to see whether they start a match.
    pub fn pending(&self) -> usize {
        self.carry.len()
    }

    /// Takes the next chunk and returns what is safe to show now.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        let mut work = std::mem::take(&mut self.carry);
        work.extend_from_slice(chunk);
        let mut skip = 0;
        if let Some(rest) = self.pending_rest.take() {
            let n = rest.iter().zip(&work).take_while(|(a, b)| a == b).count();
            if n == rest.len() || n == work.len() {
                skip = n;
                if n < rest.len() {
                    self.pending_rest = Some(rest[n..].to_vec());
                }
            }
        }
        let work = &work[skip..];

        let mut hold_from = self.open_tail_start(work);
        if work.len() - hold_from > MAX_CARRY {
            hold_from = work.len();
        }
        // A complete match that straddles the hold point is held whole.
        let matches = find_matches(work, &self.secrets, &self.patterns);
        for &(a, b) in &matches {
            if a < hold_from && b > hold_from {
                hold_from = a;
            }
        }
        self.carry = work[hold_from..].to_vec();
        let emit: Vec<_> = matches.into_iter().filter(|&(_, b)| b <= hold_from).collect();
        replace(&work[..hold_from], &emit)
    }

    /// Releases the carry when the stream goes idle or ends. A carried prefix of a known secret (8
    /// bytes or more) is released as the placeholder, and the rest of that secret is dropped if it
    /// follows.
    pub fn flush(&mut self) -> Vec<u8> {
        let carry = std::mem::take(&mut self.carry);
        if carry.is_empty() {
            return carry;
        }
        for secret in &self.secrets {
            for start in 0..carry.len() {
                let tail = &carry[start..];
                if tail.len() >= 8 && tail.len() < secret.len() && secret.starts_with(tail) {
                    self.pending_rest = Some(secret[tail.len()..].to_vec());
                    let mut out = scrub_all(&carry[..start], &self.secrets, &self.patterns);
                    out.extend_from_slice(PLACEHOLDER);
                    return out;
                }
            }
        }
        scrub_all(&carry, &self.secrets, &self.patterns)
    }

    /// Start of the earliest tail of `s` that could grow into a match.
    fn open_tail_start(&self, s: &[u8]) -> usize {
        let mut start = s.len();
        let longest = self.secrets.first().map_or(0, Vec::len);
        for k in (1..longest.min(s.len() + 1)).rev() {
            let tail = &s[s.len() - k..];
            if self.secrets.iter().any(|sec| sec.len() > k && sec.starts_with(tail)) {
                start = s.len() - k;
                break;
            }
        }
        let window = s.len().saturating_sub(MAX_CARRY);
        for at in window..start {
            if self.patterns.iter().any(|p| p.open_at_end(s, at)) {
                return at;
            }
        }
        start
    }
}

/// The length of the longest run of `secret` that appears in `hay`, counting only runs of at
/// least `min` bytes (0 if none). Used to prove a key leaked nowhere, partially or whole, without
/// ever showing it: callers report the number, never the bytes.
pub fn longest_shared_run(secret: &[u8], hay: &[u8], min: usize) -> usize {
    if min == 0 || secret.len() < min || hay.len() < min {
        return 0;
    }
    let pieces: std::collections::HashMap<&[u8], Vec<usize>> =
        secret.windows(min).enumerate().fold(std::collections::HashMap::new(), |mut m, (i, w)| {
            m.entry(w).or_insert_with(Vec::new).push(i);
            m
        });
    let mut best = 0;
    for (h, w) in hay.windows(min).enumerate() {
        let Some(starts) = pieces.get(w) else { continue };
        for &s in starts {
            let n = secret[s..].iter().zip(&hay[h..]).take_while(|(a, b)| a == b).count();
            best = best.max(n);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_strings_and_keys_are_scrubbed_before_escaping() {
        let secret = "private\"quoted\ncredential";
        let value = serde_json::json!({secret: [secret, {"nested": secret}], "safe": true});
        let cleaned = scrub_json(&value, &[secret.as_bytes().to_vec()], &[]);
        let text = cleaned.to_string();
        assert!(!text.contains("quoted") && !text.contains("credential"));
        assert_eq!(cleaned["safe"], true);
        assert_eq!(cleaned["[REDACTED-CREDENTIAL]"][0], "[REDACTED-CREDENTIAL]");
    }

    #[test]
    fn longest_shared_run_finds_partial_leaks() {
        let key = b"sk-or-v1-0123456789abcdef";
        assert_eq!(longest_shared_run(key, b"nothing here", 8), 0);
        assert_eq!(longest_shared_run(key, b"x 0123456789ab y", 8), 12);
        assert_eq!(longest_shared_run(key, &[b"..".as_slice(), key, b".."].concat(), 8), key.len());
        assert_eq!(longest_shared_run(key, b"0123456", 8), 0);
    }

    // Fake keys are assembled at run time, so the repo's own secret scan never sees a key shape.
    fn fake(prefix: &str, n: usize) -> String {
        format!("{prefix}{}", "Ab3_".repeat(n / 4 + 1)[..n].to_owned())
    }

    fn stream(s: &mut StreamScrubber, chunks: &[&[u8]]) -> String {
        let mut out = Vec::new();
        for c in chunks {
            out.extend(s.push(c));
        }
        out.extend(s.flush());
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn builtin_patterns_compile_in_order() {
        let names: Vec<_> = builtin_patterns().into_iter().map(|p| p.name).collect();
        assert_eq!(
            names,
            ["anthropic", "openrouter", "openai-project", "openai-classic", "github", "aws-access-key-id", "google-api-key", "telegram-bot-token"]
        );
    }

    #[test]
    fn a_google_api_key_is_redacted_and_a_lookalike_is_not() {
        let p = builtin_patterns();
        // Gemini's GEMINI_API_KEY: AIza and 35 more characters.
        let key = fake("AIza", 35);
        let out = String::from_utf8(scrub_all(format!("GEMINI_API_KEY={key} done").as_bytes(), &[], &p)).unwrap();
        assert_eq!(out, "GEMINI_API_KEY=[REDACTED-CREDENTIAL] done");
        // The same letters without the prefix, and a short one, are ordinary text.
        let plain = format!("AIzb{} and AIza short", "B".repeat(35));
        assert_eq!(String::from_utf8(scrub_all(plain.as_bytes(), &[], &p)).unwrap(), plain);
    }

    #[test]
    fn patterns_redact_key_shapes_and_spare_ordinary_text() {
        let p = builtin_patterns();
        let key = fake(&["sk", "or", "v1-"].join("-"), 48);
        let text = format!("key={key} path=C:\\Users\\x\\repo sha=0123456789abcdef0123456789abcdef01234567 id=550e8400-e29b-41d4-a716-446655440000");
        let out = String::from_utf8(scrub_all(text.as_bytes(), &[], &p)).unwrap();
        assert!(!out.contains(&key));
        assert!(out.starts_with("key=[REDACTED-CREDENTIAL] path=C:\\Users\\x\\repo sha=0123456789abcdef"));
        assert!(out.ends_with("id=550e8400-e29b-41d4-a716-446655440000"));
    }

    #[test]
    fn exact_values_are_redacted_even_without_a_known_shape() {
        let secret = b"plainvaluewithoutprefix123".to_vec();
        let out = scrub_all(b"a plainvaluewithoutprefix123 b", &[secret], &[]);
        assert_eq!(out, b"a [REDACTED-CREDENTIAL] b");
    }

    #[test]
    fn a_secret_split_across_chunks_is_caught() {
        let secret = "hunter2-very-secret-value".to_owned();
        let mut s = StreamScrubber::new([secret.clone().into_bytes()], vec![]);
        let (a, b) = secret.split_at(9);
        let out = stream(&mut s, &[format!("x {a}").as_bytes(), format!("{b} y").as_bytes()]);
        assert_eq!(out, "x [REDACTED-CREDENTIAL] y");
    }

    #[test]
    fn a_key_shape_split_across_chunks_is_caught() {
        let key = fake(&["sk", "ant-"].join("-"), 40);
        let mut s = StreamScrubber::new([], builtin_patterns());
        for cut in [1, 3, 7, 12, 30] {
            let (a, b) = key.split_at(cut);
            let out = stream(&mut s, &[format!("pre {a}").as_bytes(), format!("{b} post").as_bytes()]);
            assert_eq!(out, "pre [REDACTED-CREDENTIAL] post", "cut at {cut}");
        }
    }

    #[test]
    fn ordinary_output_passes_through_with_only_a_tiny_hold() {
        let mut s = StreamScrubber::new([b"zzzz-secret-zzzz".to_vec()], builtin_patterns());
        let out = s.push(b"hello world\r\n");
        assert_eq!(out, b"hello world\r\n");
        assert_eq!(s.pending(), 0);
        let out = s.push(b"ends with s");
        assert_eq!(out, b"ends with ", "a trailing 's' could start 'sk-'");
        assert_eq!(s.flush(), b"s");
    }

    #[test]
    fn idle_flush_of_a_secret_prefix_redacts_it_and_drops_the_rest() {
        let secret = "0123456789-abcdefghij-SECRET".to_owned();
        let mut s = StreamScrubber::new([secret.clone().into_bytes()], vec![]);
        let mut out = s.push(format!("a {}", &secret[..12]).as_bytes());
        out.extend(s.flush());
        out.extend(s.push(format!("{} b", &secret[12..]).as_bytes()));
        out.extend(s.flush());
        assert_eq!(String::from_utf8(out).unwrap(), "a [REDACTED-CREDENTIAL] b");
    }

    #[test]
    fn longest_secret_wins_when_one_contains_another() {
        let out = scrub_all(b"xx ABCDEF yy", &[b"ABCDEF".to_vec(), b"ABC".to_vec()], &[]);
        assert_eq!(out, b"xx [REDACTED-CREDENTIAL] yy");
    }

    #[test]
    fn the_pattern_language_rejects_what_it_cannot_match() {
        assert!(parse_pattern("x", "sk-.*").is_err());
        assert!(parse_pattern("x", "abc").is_err());
        assert!(parse_pattern("x", "a(b|c)[a-z]{3,}").is_err());
        assert!(parse_pattern("x", "AKIA[0-9A-Z]{16}").is_ok());
    }
}
