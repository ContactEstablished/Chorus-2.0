//! Fleet addressing (P7): which name each Claude Code session answers to in Claude Code's own
//! peer fleet (`ListAgents` / `SendMessage`). Ported from Chorus 1's `fleetRegistryCore.ts` with
//! its rules, re-checked against Claude Code 2.1.308's `~/.claude/sessions/<pid>.json`.
//!
//! Chorus only reads those files. It never opens the messaging socket and never reads a `.key`.
//! Process facts (alive, start time) are gathered by the caller and passed in.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use specta::Type;

/// The only registry shape this code understands; other versions are left out, not guessed at.
pub const SUPPORTED_PEER_PROTOCOL: i64 = 1;

/// A registry entry after validation. Unknown keys are dropped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FleetEntry {
    pub pid: u32,
    /// Claude Code's conversation id, the same id Chorus launched it with.
    pub session_id: String,
    pub cwd: String,
    /// The process's start time as Claude Code wrote it, compared by equality only.
    #[serde(skip)]
    pub proc_start: String,
    #[serde(skip)]
    pub peer_protocol: i64,
    /// The address. Live state: shown, never stored.
    pub name: String,
    /// `idle`, `busy`, `shell`, or `unrecognised` for a word this code hasn't seen.
    pub status: String,
    /// `user` when it was asked for, `derived` when Claude Code chose it.
    pub name_source: Option<String>,
}

fn text(raw: &Value, key: &str) -> Result<String, String> {
    match raw.get(key) {
        Some(Value::String(s)) if !s.is_empty() => Ok(s.clone()),
        _ => Err(format!("{key}: missing or not text")),
    }
}

/// Validates one registry file's parsed contents. Never panics: a torn or half-written file is an
/// ordinary value, and one bad file never ends a poll over the others.
pub fn parse_entry(raw: &Value) -> Result<FleetEntry, String> {
    let pid = raw.get("pid").and_then(Value::as_u64).filter(|p| *p > 0 && *p <= u64::from(u32::MAX)).ok_or("pid: missing or not a positive integer")?;
    let peer_protocol = raw.get("peerProtocol").and_then(Value::as_i64).ok_or("peerProtocol: missing or not an integer")?;
    let status = raw.get("status").and_then(Value::as_str).unwrap_or("");
    Ok(FleetEntry {
        pid: pid as u32,
        session_id: text(raw, "sessionId")?,
        cwd: text(raw, "cwd")?,
        proc_start: text(raw, "procStart")?,
        peer_protocol,
        name: text(raw, "name")?,
        status: if matches!(status, "idle" | "busy" | "shell") { status.to_owned() } else { "unrecognised".to_owned() },
        name_source: raw.get("nameSource").and_then(Value::as_str).map(str::to_owned),
    })
}

pub fn protocol_supported(entry: &FleetEntry) -> bool {
    entry.peer_protocol == SUPPORTED_PEER_PROTOCOL
}

/// Both halves are needed: a hard-killed session leaves its file behind, and pids are recycled, so
/// the pid must be alive *and* have started when the file says.
pub fn is_live(entry: &FleetEntry, alive: bool, started: Option<&str>) -> bool {
    alive && started == Some(entry.proc_start.as_str())
}

/// Trimmed and case-folded, so " Bob" and "bob" are one address.
pub fn normalise(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Normalised names held by more than one of the (live) entries.
pub fn duplicate_names<'a>(entries: impl IntoIterator<Item = &'a FleetEntry>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut dupes = Vec::new();
    for e in entries {
        let key = normalise(&e.name);
        if !seen.insert(key.clone()) && !dupes.contains(&key) {
            dupes.push(key);
        }
    }
    dupes
}

/// What Chorus can say about a session's address: exactly three states. A collision is a cause
/// on `changed`, given only when another live session holds the requested name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AddressState {
    Verified { address: String },
    #[serde(rename_all = "camelCase")]
    Changed { requested: String, current: String, collision: bool },
    Unknown { reason: String },
}

pub fn address_state(requested: Option<&str>, entry: Option<&FleetEntry>, duplicates: &[String]) -> AddressState {
    let Some(entry) = entry else {
        return AddressState::Unknown { reason: "no live registry entry for this session".into() };
    };
    let Some(requested) = requested.filter(|r| !normalise(r).is_empty()) else {
        return AddressState::Verified { address: entry.name.clone() };
    };
    if normalise(requested) == normalise(&entry.name) {
        // Claude Code 2.1.308 lets two live sessions hold one name, so a message to it is ambiguous.
        if duplicates.contains(&normalise(&entry.name)) {
            return AddressState::Changed { requested: requested.to_owned(), current: entry.name.clone(), collision: true };
        }
        return AddressState::Verified { address: entry.name.clone() };
    }
    AddressState::Changed {
        requested: requested.to_owned(),
        current: entry.name.clone(),
        collision: duplicates.contains(&normalise(requested)),
    }
}

/// A `changed` stays until it is acknowledged: losing the registry, or the name coming back, is
/// not taken as the user having seen the change. Only a newer divergence replaces it.
pub fn next_sticky(previous: Option<&AddressState>, incoming: AddressState, acknowledged: bool) -> AddressState {
    match previous {
        Some(prev @ AddressState::Changed { .. }) if !acknowledged => match incoming {
            AddressState::Changed { .. } => incoming,
            _ => prev.clone(),
        },
        _ => incoming,
    }
}

/// Chorus 1's wording.
pub fn describe(state: &AddressState) -> String {
    match state {
        AddressState::Verified { address } => address.clone(),
        AddressState::Changed { requested, current, collision: true } if normalise(requested) == normalise(current) => {
            format!("{current} (shared with another live session)")
        }
        AddressState::Changed { requested, current, collision: true } => format!("{current} (requested {requested} — taken)"),
        AddressState::Changed { requested, current, .. } => format!("{current} (requested {requested})"),
        AddressState::Unknown { .. } => "Address unknown".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn raw(pid: u32, session: &str, name: &str) -> Value {
        json!({ "pid": pid, "sessionId": session, "cwd": "C:\\w", "procStart": "1343", "peerProtocol": 1,
                "name": name, "status": "idle", "nameSource": "user", "peerFeatures": ["notify_idle"] })
    }

    fn entry(pid: u32, session: &str, name: &str) -> FleetEntry {
        parse_entry(&raw(pid, session, name)).unwrap()
    }

    #[test]
    fn parses_and_drops_unknown_keys() {
        let e = entry(7, "s1", "Ada");
        assert_eq!((e.pid, e.session_id.as_str(), e.name.as_str(), e.status.as_str()), (7, "s1", "Ada", "idle"));
        assert!(protocol_supported(&e));
    }

    #[test]
    fn malformed_files_are_values_not_panics() {
        for bad in [json!({}), json!(null), json!("x"), json!({"pid": -1}), json!({"pid": 5, "sessionId": ""})] {
            assert!(parse_entry(&bad).is_err());
        }
        let mut odd = raw(1, "s", "n");
        odd["status"] = json!("thinking");
        assert_eq!(parse_entry(&odd).unwrap().status, "unrecognised");
        odd["peerProtocol"] = json!(2);
        assert!(!protocol_supported(&parse_entry(&odd).unwrap()));
    }

    #[test]
    fn a_recycled_pid_or_a_leaked_file_is_not_live() {
        let e = entry(7, "s1", "Ada");
        assert!(is_live(&e, true, Some("1343")));
        assert!(!is_live(&e, false, Some("1343")), "process gone, file leaked");
        assert!(!is_live(&e, true, Some("9999")), "pid recycled by another process");
        assert!(!is_live(&e, true, None));
    }

    #[test]
    fn addresses_are_verified_changed_or_unknown() {
        let e = entry(7, "s1", "Ada");
        assert_eq!(address_state(Some(" ada "), Some(&e), &[]), AddressState::Verified { address: "Ada".into() });
        assert_eq!(address_state(None, Some(&e), &[]), AddressState::Verified { address: "Ada".into() });
        assert!(matches!(address_state(Some("Bob"), None, &[]), AddressState::Unknown { .. }));
        assert_eq!(
            address_state(Some("Bob"), Some(&e), &[]),
            AddressState::Changed { requested: "Bob".into(), current: "Ada".into(), collision: false }
        );
    }

    #[test]
    fn a_taken_name_is_a_collision() {
        let live = [entry(1, "a", "Bob"), entry(2, "b", "bob "), entry(3, "c", "Cy")];
        let dupes = duplicate_names(&live);
        assert_eq!(dupes, ["bob"]);
        let mine = entry(4, "d", "bob-2");
        let state = address_state(Some("Bob"), Some(&mine), &dupes);
        assert_eq!(state, AddressState::Changed { requested: "Bob".into(), current: "bob-2".into(), collision: true });
        assert_eq!(describe(&state), "bob-2 (requested Bob — taken)");
    }

    #[test]
    fn a_name_two_live_sessions_hold_is_a_collision() {
        let live = [entry(1, "a", "Bob"), entry(2, "b", "Bob")];
        let dupes = duplicate_names(&live);
        let state = address_state(Some("Bob"), Some(&live[0]), &dupes);
        assert_eq!(state, AddressState::Changed { requested: "Bob".into(), current: "Bob".into(), collision: true });
        assert_eq!(describe(&state), "Bob (shared with another live session)");
        // A session Chorus didn't name isn't flagged for what it was given.
        assert!(matches!(address_state(None, Some(&live[0]), &dupes), AddressState::Verified { .. }));
    }

    #[test]
    fn changed_is_sticky_until_acknowledged() {
        let changed = AddressState::Changed { requested: "Bob".into(), current: "x".into(), collision: false };
        let verified = AddressState::Verified { address: "Bob".into() };
        let unknown = AddressState::Unknown { reason: "r".into() };
        assert_eq!(next_sticky(Some(&changed), verified.clone(), false), changed);
        assert_eq!(next_sticky(Some(&changed), unknown, false), changed);
        assert_eq!(next_sticky(Some(&changed), verified.clone(), true), verified);
        let newer = AddressState::Changed { requested: "Bob".into(), current: "y".into(), collision: true };
        assert_eq!(next_sticky(Some(&changed), newer.clone(), false), newer);
        assert_eq!(next_sticky(None, verified.clone(), false), verified);
    }
}
