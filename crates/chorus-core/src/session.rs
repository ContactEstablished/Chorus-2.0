//! Session and run lifecycle rules (ARCHITECTURE §4): what a session should be doing, why a run
//! ended, and what to do at boot. The SessionManager in `src-tauri` applies them.
//!
//! A *session* is long-lived; each launch of its process is a *run*. `SessionState::Running` means
//! "should be running": a session in that state is relaunched when Chorus starts, so a clean quit,
//! a crash of Chorus and a reboot all bring it back. Only the agent exiting by itself or a Close
//! changes that.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionState {
    Running,
    /// The process ended by itself (or could not start). Kept with its scrollback; Restart runs
    /// it again.
    Exited,
    /// Closed by the user. Gone from the UI; the row stays for history.
    Closed,
}

impl SessionState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Exited => "exited",
            Self::Closed => "closed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "running" => Self::Running,
            "exited" => Self::Exited,
            "closed" => Self::Closed,
            _ => return None,
        })
    }
}

/// Why a run ended, as recorded on the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunEnd {
    /// The process exited by itself.
    Exited,
    /// The user pressed Close.
    Closed,
    /// The user pressed Restart; a new run follows in the same session.
    Restarted,
    /// Chorus quit cleanly while the run was live.
    ChorusClosed,
    /// Found still open at the next start: Chorus stopped without closing it (a crash, or a kill).
    ChorusCrashed,
    /// The process ended because Windows was shutting down.
    SystemShutdown,
}

impl RunEnd {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exited => "exited",
            Self::Closed => "closed",
            Self::Restarted => "restarted",
            Self::ChorusClosed => "chorus-closed",
            Self::ChorusCrashed => "chorus-crashed",
            Self::SystemShutdown => "system-shutdown",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "exited" => Self::Exited,
            "closed" => Self::Closed,
            "restarted" => Self::Restarted,
            "chorus-closed" => Self::ChorusClosed,
            "chorus-crashed" => Self::ChorusCrashed,
            "system-shutdown" => Self::SystemShutdown,
            _ => return None,
        })
    }
}

/// What made a live run's process end, as the SessionManager observed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCause {
    /// Nobody asked: the agent or shell exited, or crashed.
    ByItself,
    UserClose,
    UserRestart,
    /// Chorus is quitting; every process in its jobs is about to die with it.
    ChorusQuitting,
    /// Windows reports it is shutting down. Children get their shutdown signal before Chorus does,
    /// so their exits must not count as "exited by itself", or a reboot would not relaunch them.
    SystemShuttingDown,
}

/// How a run's end is recorded, and the session's new state (`None` leaves it unchanged).
pub fn after_exit(cause: ExitCause) -> (RunEnd, Option<SessionState>) {
    match cause {
        ExitCause::ByItself => (RunEnd::Exited, Some(SessionState::Exited)),
        ExitCause::UserClose => (RunEnd::Closed, Some(SessionState::Closed)),
        ExitCause::UserRestart => (RunEnd::Restarted, None),
        ExitCause::ChorusQuitting => (RunEnd::ChorusClosed, None),
        ExitCause::SystemShuttingDown => (RunEnd::SystemShutdown, None),
    }
}

/// Decides the cause of an exit from what the manager knows at that moment. An explicit request
/// (Close, Restart) wins; then Chorus quitting; then Windows shutting down.
pub fn classify_exit(requested: Option<ExitCause>, chorus_quitting: bool, system_shutting_down: bool) -> ExitCause {
    match requested {
        Some(cause) => cause,
        None if chorus_quitting => ExitCause::ChorusQuitting,
        None if system_shutting_down => ExitCause::SystemShuttingDown,
        None => ExitCause::ByItself,
    }
}

/// What to do when Chorus starts.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BootPlan {
    /// Sessions to launch again, in id order.
    pub relaunch: Vec<u32>,
    /// Runs left open by a Chorus that stopped without closing them; their processes died with it
    /// (each run is in a Job Object that closes with Chorus), so they are closed out as crashed.
    pub heal: Vec<u32>,
}

/// `sessions` are `(id, state)`; `open_runs` are runs with no end recorded. Every open run is dead
/// at this point: only this process could own it, and the single-instance guard means no other
/// Chorus is running against the same data.
pub fn plan_boot(sessions: &[(u32, SessionState)], open_runs: &[u32]) -> BootPlan {
    let mut relaunch: Vec<u32> =
        sessions.iter().filter(|(_, state)| *state == SessionState::Running).map(|(id, _)| *id).collect();
    relaunch.sort_unstable();
    let mut heal = open_runs.to_vec();
    heal.sort_unstable();
    BootPlan { relaunch, heal }
}

/// Bounds for a session's name, enforced at the IPC boundary.
pub const MAX_NAME_CHARS: usize = 80;

/// Trims and checks a session name; an empty name is replaced by `fallback`.
pub fn clean_name(name: Option<&str>, fallback: &str) -> Result<String, String> {
    let name = name.map(str::trim).filter(|n| !n.is_empty()).unwrap_or(fallback);
    if name.chars().count() > MAX_NAME_CHARS {
        return Err(format!("a session name can be at most {MAX_NAME_CHARS} characters"));
    }
    if name.chars().any(char::is_control) {
        return Err("a session name cannot contain control characters".into());
    }
    Ok(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_and_ends_round_trip_through_their_stored_names() {
        for s in [SessionState::Running, SessionState::Exited, SessionState::Closed] {
            assert_eq!(SessionState::parse(s.as_str()), Some(s));
        }
        for e in [
            RunEnd::Exited,
            RunEnd::Closed,
            RunEnd::Restarted,
            RunEnd::ChorusClosed,
            RunEnd::ChorusCrashed,
            RunEnd::SystemShutdown,
        ] {
            assert_eq!(RunEnd::parse(e.as_str()), Some(e));
        }
        assert_eq!(SessionState::parse("Running"), None);
    }

    #[test]
    fn only_exiting_by_itself_or_close_stops_a_session_being_relaunched() {
        assert_eq!(after_exit(ExitCause::ByItself), (RunEnd::Exited, Some(SessionState::Exited)));
        assert_eq!(after_exit(ExitCause::UserClose), (RunEnd::Closed, Some(SessionState::Closed)));
        assert_eq!(after_exit(ExitCause::UserRestart), (RunEnd::Restarted, None));
        assert_eq!(after_exit(ExitCause::ChorusQuitting), (RunEnd::ChorusClosed, None));
        assert_eq!(after_exit(ExitCause::SystemShuttingDown), (RunEnd::SystemShutdown, None));
    }

    #[test]
    fn an_explicit_request_wins_then_quitting_then_shutdown() {
        assert_eq!(classify_exit(Some(ExitCause::UserClose), true, true), ExitCause::UserClose);
        assert_eq!(classify_exit(None, true, true), ExitCause::ChorusQuitting);
        assert_eq!(classify_exit(None, false, true), ExitCause::SystemShuttingDown);
        assert_eq!(classify_exit(None, false, false), ExitCause::ByItself);
    }

    #[test]
    fn boot_relaunches_running_sessions_and_heals_open_runs() {
        let plan = plan_boot(
            &[(3, SessionState::Running), (1, SessionState::Exited), (2, SessionState::Running), (4, SessionState::Closed)],
            &[9, 7],
        );
        assert_eq!(plan, BootPlan { relaunch: vec![2, 3], heal: vec![7, 9] });
        assert_eq!(plan_boot(&[], &[]), BootPlan::default());
    }

    #[test]
    fn names_are_trimmed_bounded_and_defaulted() {
        assert_eq!(clean_name(Some("  build  "), "x").unwrap(), "build");
        assert_eq!(clean_name(Some("   "), "Shell 1").unwrap(), "Shell 1");
        assert_eq!(clean_name(None, "Shell 1").unwrap(), "Shell 1");
        assert!(clean_name(Some(&"a".repeat(81)), "x").is_err());
        assert!(clean_name(Some("a\u{7}b"), "x").is_err());
    }
}
