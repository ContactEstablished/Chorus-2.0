//! App-level facts: which build is running and where its data lives (D14, LESSONS L15).

use serde::{Deserialize, Serialize};
use specta::Type;
use std::time::{Duration, Instant};
use tauri::{AppHandle, State};
use tauri_specta::Event;

use chorus_core::build::BuildFlavor;

/// Dev or release, as the front end sees it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Flavor {
    Dev,
    Release,
}

impl From<BuildFlavor> for Flavor {
    fn from(f: BuildFlavor) -> Self {
        match f {
            BuildFlavor::Dev => Self::Dev,
            BuildFlavor::Release => Self::Release,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub product_name: String,
    pub version: String,
    pub identifier: String,
    pub flavor: Flavor,
    /// The live data directory. Shown in the window so it's always clear which data is in use.
    pub data_dir: String,
    /// The user's home folder, so paths can be shown as ~/...
    pub home_dir: Option<String>,
    pub tauri_version: String,
}

pub struct AppState {
    pub info: AppInfo,
}

/// Emitted once a second so the window can show the back end is alive. It also proves the typed
/// event path end to end.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct BackendPulse {
    pub seq: u32,
    pub uptime_secs: u32,
}

#[tauri::command]
#[specta::specta]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    state.info.clone()
}

pub fn start_pulse(app: AppHandle) {
    let started = Instant::now();
    std::thread::Builder::new()
        .name("backend-pulse".into())
        .spawn(move || {
            let mut seq: u32 = 0;
            loop {
                let uptime_secs = u32::try_from(started.elapsed().as_secs()).unwrap_or(u32::MAX);
                let pulse = BackendPulse { seq, uptime_secs };
                if pulse.emit(&app).is_err() {
                    break;
                }
                seq = seq.wrapping_add(1);
                std::thread::sleep(Duration::from_secs(1));
            }
        })
        .expect("spawn pulse thread");
}
