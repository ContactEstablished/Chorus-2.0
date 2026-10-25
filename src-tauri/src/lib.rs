//! Chorus 2.0 back end: Tauri setup, commands and IO. Pure logic lives in `chorus-core` (D11).

mod agents;
mod app;
mod attention;
mod events;
mod export;
mod fleet;
mod fleet_registry;
mod git;
mod ipc;
mod lock;
mod narrator;
mod prefs;
mod projects;
mod providers;
mod routing;
mod sessions;
mod splash;
mod store;
mod tray;
mod vault;
// P8: Bots, notes, instructions, documents and automations.
mod automations;
mod bots;
mod documents;
mod instructions;
mod notes;
mod p8;
mod relay;
mod run_loop;

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
        // New session's folder picker, used from Rust only: the front end gets no dialog permissions.
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::Builder::new().open_js_links_on_click(false).build())
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
                    home_dir: app.path().home_dir().ok().map(|h| h.display().to_string()),
                    tauri_version: tauri::VERSION.to_string(),
                },
            });

            let store = store::Store::open(&data_dir.join("chorus.db"))?;
            let handle = app.handle().clone();
            // P7: the tray count and the notification centre see every summary the views see.
            let attention = attention::Attention::new(app.handle().clone());
            let watching = attention.clone();
            let manager = SessionManager::new(
                store,
                data_dir,
                Box::new(move |session| {
                    let _ = SessionUpdated { session: session.clone() }.emit(&handle);
                    watching.observe(&session);
                }),
            );
            attention.bind(&manager);
            app.manage(attention);
            if let Err(e) = tray::install(app.handle(), fleet::wanted(&manager)) {
                eprintln!("tray: {e}");
            }
            // The strip and the tray mustn't keep Chorus running: closing the main window quits,
            // as it did before they existed.
            if let Some(main) = app.get_webview_window("main") {
                let quitting = app.handle().clone();
                main.on_window_event(move |event| {
                    if let tauri::WindowEvent::Destroyed = event {
                        quitting.exit(0);
                    }
                });
            }
            // Sessions open now, so views can attach; they relaunch off this thread, because an
            // opencode session waits for its server.
            // The listener agent hooks post to, before any session launches (P3).
            manager.set_hook_listener(events::listener::start(manager.clone()));
            manager.watch_signals();
            sessions::composer::start(&manager)?;
            narrator::start(&manager)?;
            p8::start(&manager)?;
            // P15: a loop step that was running when Chorus stopped is ended before sessions relaunch.
            let loop_handle = app.handle().clone();
            run_loop::prepare(&manager, Box::new(move |run_id| {
                let _ = run_loop::LoopUpdated { run_id }.emit(&loop_handle);
            }));
            let relaunch = manager.reopen()?;
            // Revoke OpenRouter keys an earlier Chorus minted and couldn't revoke. Only keys from
            // before this start are touched, so it can run beside the relaunches, which mint new ones.
            let up_to = store::providers::max_capped_id(manager.store())?;
            let sweeping = manager.clone();
            std::thread::Builder::new()
                .name("sweep-keys".into())
                .spawn(move || providers::keys::sweep(sweeping.store(), up_to))?;
            let relaunching = manager.clone();
            std::thread::Builder::new().name("relaunch".into()).spawn(move || relaunching.relaunch(&relaunch))?;
            run_loop::start(&manager);
            // P15: the Telegram relay, for the loop's questions and stops.
            relay::start(&manager)?;
            // Look for the agent CLIs without holding up the window. Runs relaunched before this
            // finishes get their CLI version filled in afterwards.
            let detecting = manager.clone();
            std::thread::Builder::new().name("detect-agents".into()).spawn(move || {
                detecting.refresh_agents();
                detecting.notify_all();
            })?;
            // The stored zoom, before the window shows anything. A failure is shown in Settings,
            // where the zoom can be set again.
            let shown = prefs::load(manager.store());
            if let Err(e) = prefs::apply_zoom(app.handle(), shown.zoom) {
                eprintln!("{e}");
            }
            // The splash plays beside the restore (already under way) and never holds it up.
            if shown.splash
                && let Err(e) = splash::open_at_start(app.handle(), shown.theme)
            {
                eprintln!("splash: {e}");
            }
            // Routing tiers (D57): the observer refetches refreshed models' endpoints, when the user chose
            // a provider for it.
            let routing = std::sync::Arc::new(routing::Routing::default());
            routing::start_observer(manager.clone(), routing.clone());
            app.manage(routing);
            let strip = manager.clone();
            app.manage(manager);
            fleet::start(app.handle(), &strip);
            app.manage(fleet_registry::start(app.handle(), &strip));
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
