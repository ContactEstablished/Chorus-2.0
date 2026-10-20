//! Look and layout preferences (`chorus_core::prefs`), kept as one JSON value in settings. The
//! zoom is the webview's own, applied here at start and on every change.

use std::sync::Arc;

use chorus_core::prefs::Prefs;
use tauri::{AppHandle, Manager as _, State};

use crate::sessions::SessionManager;
use crate::store::{Store, settings};

type Manager<'a> = State<'a, Arc<SessionManager>>;

const KEY: &str = "ui.prefs";

pub fn load(store: &Store) -> Prefs {
    Prefs::from_stored(settings::get(store, KEY).ok().flatten().as_deref())
}

/// Sets the main window's zoom. A failure leaves the old zoom and says why.
pub fn apply_zoom(app: &AppHandle, zoom: f64) -> Result<(), String> {
    let window = app.get_webview_window("main").ok_or("The main window isn't open.")?;
    window.set_zoom(zoom).map_err(|e| format!("Couldn't zoom the window: {e}"))
}

#[tauri::command]
#[specta::specta]
pub fn prefs_get(state: Manager<'_>) -> Prefs {
    load(state.store())
}

/// Stores the preferences, with anything out of range brought back in, and applies the zoom.
#[tauri::command]
#[specta::specta]
pub fn prefs_set(app: AppHandle, state: Manager<'_>, prefs: Prefs) -> Result<Prefs, String> {
    let text = serde_json::to_string(&prefs).map_err(|e| e.to_string())?;
    let clean = Prefs::from_stored(Some(&text));
    let before = load(state.store());
    settings::set(state.store(), KEY, &serde_json::to_string(&clean).map_err(|e| e.to_string())?)?;
    if (before.zoom - clean.zoom).abs() > f64::EPSILON {
        apply_zoom(&app, clean.zoom)?;
    }
    Ok(clean)
}
