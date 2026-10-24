//! Fleet addressing (P7): every 3 s, reads Claude Code's peer registry (`~/.claude/sessions/*.json`)
//! and works out each Chorus Claude session's address (`chorus_core::fleet`). Read only: the
//! messaging socket is never opened and `.key` files are never read. A folder that can't be read is
//! "unreadable", never an empty fleet, and the last good addresses are not served after a failure.
//!
//! `procStart` in Claude Code 2.1.308's files is the process creation time as a FILETIME (100 ns
//! ticks since 1601) in decimal, which is what `GetProcessTimes` gives.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chorus_core::agent::AgentKind;
use chorus_core::fleet::{self, AddressState, FleetEntry};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager as _, State};
use tauri_specta::Event;
use windows::Win32::Foundation::{CloseHandle, FILETIME, STILL_ACTIVE};
use windows::Win32::System::Threading::{GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

use crate::sessions::{SessionManager, SessionStatus};

const POLL: Duration = Duration::from_secs(3);

/// One open Chorus session in the roster.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RosterRow {
    pub session_id: u32,
    pub name: String,
    /// The address state, for Claude Code sessions.
    pub address: Option<AddressState>,
    /// The address in words (Chorus 1's), or why the session has none.
    pub described: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FleetRoster {
    /// Why the registry couldn't be read; the addresses are then unknown.
    pub unreadable: Option<String>,
    pub rows: Vec<RosterRow>,
    /// Live Claude Code sessions Chorus didn't start, by name. Listed only; no actions.
    pub others: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct FleetRosterChanged {
    pub roster: FleetRoster,
}

#[derive(Default)]
struct Sticky {
    states: HashMap<u32, AddressState>,
    acknowledged: HashSet<u32>,
}

pub struct Registry {
    roster: Mutex<FleetRoster>,
    sticky: Mutex<Sticky>,
}

/// Whether `pid` is running, and when it started (FILETIME ticks, decimal).
fn process_facts(pid: u32) -> (bool, Option<String>) {
    // SAFETY: querying another process with the least access that answers both questions.
    unsafe {
        let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return (false, None) };
        let mut code = 0u32;
        let alive = GetExitCodeProcess(handle, &mut code).is_ok() && code == STILL_ACTIVE.0 as u32;
        let (mut created, mut exited, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        let started = GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user)
            .is_ok()
            .then(|| ((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime)).to_string());
        let _ = CloseHandle(handle);
        (alive, started)
    }
}

/// The live, supported entries in `dir`. Unparsable files and dead sessions are skipped.
fn read_live(dir: &Path) -> Result<Vec<FleetEntry>, String> {
    let listing = std::fs::read_dir(dir).map_err(|e| format!("Claude Code's session folder can't be read: {e}"))?;
    let mut live = Vec::new();
    for item in listing.flatten() {
        let path = item.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let Ok(raw) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        let Ok(entry) = fleet::parse_entry(&raw) else { continue };
        if !fleet::protocol_supported(&entry) {
            continue;
        }
        let (alive, started) = process_facts(entry.pid);
        if fleet::is_live(&entry, alive, started.as_deref()) {
            live.push(entry);
        }
    }
    Ok(live)
}

impl Registry {
    fn poll(&self, manager: &SessionManager, dir: Option<&Path>) -> FleetRoster {
        let read = dir.ok_or_else(|| "The home folder isn't known.".to_owned()).and_then(read_live);
        let sessions = manager.list().unwrap_or_default();
        let mut sticky = self.sticky.lock().expect("sticky lock");
        let mut roster = FleetRoster::default();
        let (live, unreadable) = match read {
            Ok(live) => (live, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        let dupes = fleet::duplicate_names(&live);
        let mut ours = HashSet::new();
        for s in sessions.iter().filter(|s| !matches!(s.status, SessionStatus::Closed)) {
            if s.agent != AgentKind::Claude {
                roster.rows.push(RosterRow {
                    session_id: s.id,
                    name: s.name.clone(),
                    address: None,
                    described: format!("No address: the peer fleet is Claude Code's own, and {} isn't in it", s.agent.display_name()),
                });
                continue;
            }
            let entry = s.agent_session_id.as_deref().and_then(|id| live.iter().find(|e| e.session_id == id));
            if let Some(e) = entry {
                ours.insert(e.session_id.clone());
            }
            let incoming = match &unreadable {
                Some(reason) => AddressState::Unknown { reason: reason.clone() },
                None => fleet::address_state(Some(&s.name), entry, &dupes),
            };
            let acknowledged = sticky.acknowledged.remove(&s.id);
            let state = fleet::next_sticky(sticky.states.get(&s.id), incoming, acknowledged);
            sticky.states.insert(s.id, state.clone());
            roster.rows.push(RosterRow { session_id: s.id, name: s.name.clone(), described: fleet::describe(&state), address: Some(state) });
        }
        roster.others = live.iter().filter(|e| !ours.contains(&e.session_id)).map(|e| e.name.clone()).collect();
        roster.unreadable = unreadable;
        roster
    }
}

/// Starts the 3 s poller. The roster is sent to the windows whenever it changes.
pub fn start(app: &AppHandle, manager: &Arc<SessionManager>) -> Arc<Registry> {
    let registry = Arc::new(Registry { roster: Mutex::new(FleetRoster::default()), sticky: Mutex::new(Sticky::default()) });
    let dir: Option<PathBuf> = app.path().home_dir().ok().map(|h| h.join(".claude").join("sessions"));
    let (polling, manager, app) = (registry.clone(), Arc::downgrade(manager), app.clone());
    let _ = std::thread::Builder::new().name("fleet-registry".into()).spawn(move || {
        loop {
            let Some(m) = manager.upgrade() else { return };
            let roster = polling.poll(&m, dir.as_deref());
            drop(m);
            let changed = {
                let mut shown = polling.roster.lock().expect("roster lock");
                let changed = *shown != roster;
                *shown = roster.clone();
                changed
            };
            if changed {
                let _ = FleetRosterChanged { roster }.emit(&app);
            }
            std::thread::sleep(POLL);
        }
    });
    registry
}

#[tauri::command]
#[specta::specta]
pub fn fleet_roster(registry: State<'_, Arc<Registry>>) -> FleetRoster {
    registry.roster.lock().expect("roster lock").clone()
}

/// The user has seen a changed address: the next poll shows the current state.
#[tauri::command]
#[specta::specta]
pub fn fleet_acknowledge(registry: State<'_, Arc<Registry>>, session_id: u32) {
    registry.sticky.lock().expect("sticky lock").acknowledged.insert(session_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_alive_with_a_start_time() {
        let (alive, started) = process_facts(std::process::id());
        assert!(alive);
        assert!(started.is_some_and(|s| s.parse::<u64>().is_ok_and(|t| t > 0)));
    }

    #[test]
    fn a_missing_folder_is_an_error_not_an_empty_fleet() {
        assert!(read_live(Path::new("Z:\\no\\such\\folder\\for\\chorus")).is_err());
    }
}
