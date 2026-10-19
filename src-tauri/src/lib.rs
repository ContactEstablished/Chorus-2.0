//! Chorus 2.0 back end: Tauri setup, commands and IO. Pure logic lives in `chorus-core` (D11).

mod agents;
mod app;
mod ipc;
mod sessions;
mod vault;

use tauri::Manager;

pub fn run() {
    let builder = ipc::builder();

    #[cfg(debug_assertions)]
    ipc::export_bindings(&builder, &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(ipc::BINDINGS_FILE))
        .expect("export TypeScript bindings");

    tauri::Builder::default()
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
            app.manage(sessions::SessionManager::new(data_dir.join("spike-scratch")));
            app::start_pulse(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Chorus");
}
