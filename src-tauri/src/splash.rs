//! The startup splash window (spike S7; shown at launch in P4).
//!
//! A separate, short-lived window, so it can never hold up the main window or session restore: the
//! back end restores sessions on its own and the splash only reflects progress. It closes itself.

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "splash";

/// Opens the splash for `sessions` restoring sessions. `reduce_motion` forces the static version.
/// Async: building a window inside a synchronous command deadlocks on Windows.
#[tauri::command]
#[specta::specta]
pub async fn spike_splash_open(app: AppHandle, sessions: u8, reduce_motion: bool) -> Result<(), String> {
    if let Some(old) = app.get_webview_window(LABEL) {
        let _ = old.close();
    }
    let motion = if reduce_motion { "&motion=reduce" } else { "" };
    let url = format!("index.html?view=splash&sessions={}{motion}", sessions.min(24));
    WebviewWindowBuilder::new(&app, LABEL, WebviewUrl::App(url.into()))
        .title("Chorus starting")
        .inner_size(760.0, 460.0)
        .center()
        .decorations(false)
        .resizable(false)
        .skip_taskbar(true)
        .shadow(true)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}
