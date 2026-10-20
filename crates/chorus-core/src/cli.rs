//! Agent CLI versions: reading `--version` output and comparing it with the version an adapter
//! was checked against. A version Chorus hasn't checked is tried and reported, never refused
//! (LESSONS L7).

use std::fmt;

use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The first `x.y.z` in the first non-empty line, as each CLI prints it: `2.1.300 (Claude Code)`,
/// `codex-cli 0.162.1`, `1.19.3`, `grok 1.0.62 (1474f9cbdd91) [stable]`.
pub fn parse_version(output: &str) -> Option<Version> {
    let line = output.lines().map(str::trim).find(|l| !l.is_empty())?;
    line.split(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .map(|w| w.trim_start_matches('v'))
        .find_map(|w| {
            let mut parts = w.split('.');
            let v = Version {
                major: parts.next()?.parse().ok()?,
                minor: parts.next()?.parse().ok()?,
                patch: parts.next()?.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()?,
            };
            Some(v)
        })
}

/// How a found version relates to the one Chorus checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum VersionFit {
    Verified,
    Newer,
    Older,
    /// The CLI ran but printed no version Chorus could read.
    Unreadable,
}

pub fn fit(found: Option<Version>, verified: Option<Version>) -> VersionFit {
    match (found, verified) {
        (None, _) => VersionFit::Unreadable,
        (Some(_), None) => VersionFit::Verified,
        (Some(f), Some(v)) if f == v => VersionFit::Verified,
        (Some(f), Some(v)) if f > v => VersionFit::Newer,
        _ => VersionFit::Older,
    }
}

/// What a session says when its CLI isn't the version Chorus checked (fail visible, D19).
pub fn version_note(agent: &str, found: Option<&str>, verified: Option<&str>, fit: VersionFit) -> Option<String> {
    match (fit, found, verified) {
        (VersionFit::Unreadable, ..) => Some(format!("Chorus couldn't read {agent}'s version, so its options are untested here.")),
        (VersionFit::Newer, Some(f), Some(v)) => {
            Some(format!("{agent} {f} is newer than {v}, the version Chorus checked, so its options may not apply."))
        }
        (VersionFit::Older, Some(f), Some(v)) => {
            Some(format!("{agent} {f} is older than {v}, the version Chorus checked, so its options may not apply."))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_unchecked_version_gets_a_note() {
        assert_eq!(version_note("Codex", Some("0.162.1"), Some("0.162.1"), VersionFit::Verified), None);
        assert_eq!(
            version_note("Codex", Some("0.170.0"), Some("0.162.1"), VersionFit::Newer).unwrap(),
            "Codex 0.170.0 is newer than 0.162.1, the version Chorus checked, so its options may not apply."
        );
        assert!(version_note("Grok", None, Some("1.0.62"), VersionFit::Unreadable).unwrap().contains("couldn't read"));
    }

    fn v(major: u32, minor: u32, patch: u32) -> Version {
        Version { major, minor, patch }
    }

    #[test]
    fn reads_each_cli_s_version_line() {
        assert_eq!(parse_version("2.1.300 (Claude Code)\n"), Some(v(2, 1, 300)));
        assert_eq!(parse_version("codex-cli 0.162.1"), Some(v(0, 162, 1)));
        assert_eq!(parse_version("\n1.19.3\r\n"), Some(v(1, 19, 3)));
        assert_eq!(parse_version("grok 1.0.62 (1474f9cbdd91) [stable]"), Some(v(1, 0, 62)));
        // Gemini CLI 0.64.0 prints its version alone; aider prints `aider 0.86.2`.
        assert_eq!(parse_version("0.64.0\n"), Some(v(0, 64, 0)));
        assert_eq!(parse_version("aider 0.86.2"), Some(v(0, 86, 2)));
        assert_eq!(parse_version("v0.9.2-beta.1"), Some(v(0, 9, 2)));
        assert_eq!(parse_version("unknown option --version"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn compares_with_the_checked_version() {
        let checked = Some(v(0, 162, 1));
        assert_eq!(fit(Some(v(0, 162, 1)), checked), VersionFit::Verified);
        assert_eq!(fit(Some(v(0, 170, 0)), checked), VersionFit::Newer);
        assert_eq!(fit(Some(v(0, 9, 99)), checked), VersionFit::Older);
        assert_eq!(fit(None, checked), VersionFit::Unreadable);
        assert_eq!(v(1, 19, 3).to_string(), "1.19.3");
    }
}
