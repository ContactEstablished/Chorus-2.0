//! The startup splash window (spike S7, P4).
//!
//! A separate, short-lived window, so it can never hold up the main window or session restore: the
//! back end restores sessions on its own and the splash only reflects progress (`restore_progress`).
//! It closes itself.

use std::sync::Arc;

use chorus_core::prefs::Theme;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::sessions::{RestoreProgress, SessionManager};

pub const LABEL: &str = "splash";

fn open(app: &AppHandle, query: &str) -> Result<(), String> {
    if let Some(old) = app.get_webview_window(LABEL) {
        let _ = old.close();
    }
    WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App(format!("index.html?view=splash&{query}").into()))
        .title("Chorus starting")
        .inner_size(760.0, 460.0)
        .center()
        .decorations(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .shadow(true)
        .build()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Opens the splash as Chorus starts, in the chosen theme, showing the real restore.
pub fn open_at_start(app: &AppHandle, theme: Theme) -> Result<(), String> {
    let theme = match theme {
        Theme::Glacier => "glacier",
        Theme::Dusk => "dusk",
    };
    open(app, &format!("live=1&theme={theme}"))
}

/// How far the restore at start has got: the splash asks as it plays.
#[tauri::command]
#[specta::specta]
pub fn restore_progress(state: State<'_, Arc<SessionManager>>) -> RestoreProgress {
    state.restore_progress()
}

/// Replays the splash with `sessions` simulated restores (Settings → Appearance). `reduce_motion`
/// forces the static version. Async: building a window inside a synchronous command deadlocks on
/// Windows.
#[tauri::command]
#[specta::specta]
pub async fn splash_replay(app: AppHandle, sessions: u8, reduce_motion: bool) -> Result<(), String> {
    let motion = if reduce_motion { "&motion=reduce" } else { "" };
    open(&app, &format!("sessions={}{motion}", sessions.min(24)))
}
