//! Chorus 2.0 back end: Tauri setup, commands and IO. Pure logic lives in `chorus-core` (D11).

mod agents;
mod app;
mod fleet;
mod ipc;
mod lock;
mod sessions;
mod splash;
mod store;
mod vault;

use std::sync::Arc;

use tauri::Manager;
use tauri_specta::Event;

use sessions::{SessionManager, SessionUpdated};

pub fn run() {
    let builder = ipc::builder();

    #[cfg(debug_assertions)]
    ipc::export_bindings(&builder, &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(ipc::BINDINGS_FILE))
        .expect("export TypeScript bindings");

    let app = tauri::Builder::default()
        // First, so a second launch hands over before it restores (and doubles) any session.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);

            // Refuse to run a debug build against release data, or the reverse (D14).
            let config = app.config();
            let identifier = config.identifier.clone();
            let flavor = chorus_core::build::flavor_for(&identifier, cfg!(debug_assertions))?;

            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            app.manage(app::AppState {
                info: app::AppInfo {
                    product_name: config.product_name.clone().unwrap_or_else(|| "Chorus".into()),
                    version: app.package_info().version.to_string(),
                    identifier,
                    flavor: flavor.into(),
                    data_dir: data_dir.display().to_string(),
                    tauri_version: tauri::VERSION.to_string(),
                },
            });

            let store = store::Store::open(&data_dir.join("chorus.db"))?;
            let handle = app.handle().clone();
            let manager = SessionManager::new(
                store,
                data_dir,
                Box::new(move |session| {
                    let _ = SessionUpdated { session }.emit(&handle);
                }),
            );
            // Sessions open now, so views can attach; they relaunch off this thread, because an
            // opencode session waits for its server.
            let relaunch = manager.reopen()?;
            let relaunching = manager.clone();
            std::thread::Builder::new().name("relaunch".into()).spawn(move || relaunching.relaunch(&relaunch))?;
            // Look for the agent CLIs without holding up the window. Runs relaunched before this
            // finishes get their CLI version filled in afterwards.
            let detecting = manager.clone();
            std::thread::Builder::new().name("detect-agents".into()).spawn(move || {
                detecting.refresh_agents();
                detecting.notify_all();
            })?;
            app.manage(manager);
            app::start_pulse(app.handle().clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Chorus");

    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event
            && let Some(manager) = handle.try_state::<Arc<SessionManager>>()
        {
            manager.shutdown();
        }
    });
}
