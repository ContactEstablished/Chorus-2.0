//! The child environment: an allow-list copied from the parent plus imposed values (LESSONS L13).
//!
//! Ported from Chorus 1's `adapters/env.ts`, with one change: 2.0 builds the minimal environment
//! for every launch, not only for launches that carry a key. Names are added to [`BASELINE`] only
//! when something is observed to break without them, and the observation is recorded beside it.

use std::collections::BTreeMap;

/// Copied from the parent when present. Each entry is a channel for host state into the child,
/// so this list stays short.
pub const BASELINE: &[&str] = &[
    "PATH",
    "SystemRoot",
    "TEMP",
    "TMP",
    "HOMEDRIVE",
    "HOMEPATH",
    "USERPROFILE",
    // Additions from 2.0's S1 pass go here, each with what broke without it.
];

/// Imposed on every child, whatever the parent had. TERM=dumb inherited from a host shell once put
/// Codex into a renderer that interleaved escapes between characters and defeated scrubbing
/// (Chorus 1 F28).
pub const PINNED: &[(&str, &str)] = &[("TERM", "xterm-256color"), ("COLORTERM", "truecolor")];

pub struct ChildEnvInput<'a> {
    /// The parent environment, passed in so this stays pure.
    pub parent: &'a [(String, String)],
    /// Names an adapter needs beyond [`BASELINE`].
    pub required: &'a [&'a str],
    /// Non-secret values an adapter imposes (for example auto-update switches).
    pub additions: &'a [(&'a str, &'a str)],
    /// Decrypted keys. Applied last, so nothing can displace them.
    pub secrets: &'a [(String, String)],
}

/// The complete environment for a child. Keys keep the allow-list's spelling; lookup in the
/// parent is case-insensitive, as Windows' is (a native launch often provides `Path`, not `PATH`).
pub fn compose_child_env(input: &ChildEnvInput<'_>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for name in BASELINE.iter().chain(input.required.iter()) {
        if let Some((_, value)) = input.parent.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)) {
            out.insert((*name).to_owned(), value.clone());
        }
    }
    for (k, v) in PINNED.iter().chain(input.additions.iter()) {
        out.insert((*k).to_owned(), (*v).to_owned());
    }
    for (k, v) in input.secrets {
        out.insert(k.clone(), v.clone());
    }
    out
}

/// Whether `name` may carry a secret: an upper-case identifier of at most 64 characters that does not
/// shadow a baseline or pinned variable.
pub fn valid_secret_var(name: &str) -> bool {
    let mut chars = name.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_uppercase() || c == '_');
    first_ok
        && name.len() <= 64
        && chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && !BASELINE.iter().chain(PINNED.iter().map(|(k, _)| k)).any(|b| b.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parent() -> Vec<(String, String)> {
        [
            ("Path", "C:\\Windows;C:\\tools"),
            ("SystemRoot", "C:\\Windows"),
            ("USERPROFILE", "C:\\Users\\someone"),
            ("TERM", "dumb"),
            ("OPENAI_API_KEY", "ambient-credential"),
            ("CLAUDE_CODE_CHILD_SESSION", "1"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
    }

    #[test]
    fn copies_only_the_allow_list_and_finds_path_case_insensitively() {
        let p = parent();
        let env = compose_child_env(&ChildEnvInput { parent: &p, required: &[], additions: &[], secrets: &[] });
        assert_eq!(env.get("PATH").map(String::as_str), Some("C:\\Windows;C:\\tools"));
        assert_eq!(env.get("USERPROFILE").map(String::as_str), Some("C:\\Users\\someone"));
        assert!(!env.contains_key("OPENAI_API_KEY"), "ambient credentials must not leak");
        assert!(!env.contains_key("CLAUDE_CODE_CHILD_SESSION"), "the launcher's session marker must not leak");
        assert!(!env.contains_key("TEMP"), "absent names are skipped, not emitted empty");
    }

    #[test]
    fn pins_terminal_values_over_the_parent() {
        let p = parent();
        let env = compose_child_env(&ChildEnvInput { parent: &p, required: &["TERM"], additions: &[], secrets: &[] });
        assert_eq!(env.get("TERM").map(String::as_str), Some("xterm-256color"));
        assert_eq!(env.get("COLORTERM").map(String::as_str), Some("truecolor"));
    }

    #[test]
    fn secret_variable_names_are_checked() {
        assert!(valid_secret_var("OPENROUTER_API_KEY"));
        assert!(!valid_secret_var("PATH"));
        assert!(!valid_secret_var("Path"));
        assert!(!valid_secret_var("TERM"));
        assert!(!valid_secret_var("lower_case"));
        assert!(!valid_secret_var("1ABC"));
        assert!(!valid_secret_var(""));
        assert!(!valid_secret_var(&"A".repeat(65)));
    }

    #[test]
    fn additions_beat_pins_and_secrets_beat_everything() {
        let p = parent();
        let secrets = vec![("ANTHROPIC_API_KEY".to_owned(), "from-the-vault".to_owned())];
        let env = compose_child_env(&ChildEnvInput {
            parent: &p,
            required: &[],
            additions: &[("DISABLE_AUTOUPDATER", "1"), ("ANTHROPIC_API_KEY", "addition")],
            secrets: &secrets,
        });
        assert_eq!(env.get("DISABLE_AUTOUPDATER").map(String::as_str), Some("1"));
        assert_eq!(env.get("ANTHROPIC_API_KEY").map(String::as_str), Some("from-the-vault"));
    }
}
