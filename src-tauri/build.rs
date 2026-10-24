#[path = "command_list.rs"]
mod command_list;

fn main() {
    // D92: each window may call only the commands its capability grants. The app manifest lists
    // every command `ipc.rs` registers, so none is left out; the main window's set grants them all,
    // and the strip's and splash's capabilities name their few.
    println!("cargo:rerun-if-changed=src/ipc.rs");
    let ipc = std::fs::read_to_string("src/ipc.rs").expect("read src/ipc.rs");
    let commands = command_list::registered_commands(&ipc);
    assert!(!commands.is_empty(), "no commands found in src/ipc.rs");
    let set = command_list::main_window_set(&commands);
    let set_path = std::path::Path::new("permissions/generated/main-window.toml");
    // Written only when it changes: tauri-build reruns this script whenever `permissions` changes.
    if std::fs::read_to_string(set_path).ok().as_deref() != Some(set.as_str()) {
        std::fs::create_dir_all("permissions/generated").expect("create permissions/generated");
        std::fs::write(set_path, set).expect("write the main window's permission set");
    }
    let commands: Vec<&'static str> = commands.into_iter().map(|c| &*Box::leak(c.into_boxed_str())).collect();
    let app = tauri_build::AppManifest::new().commands(Box::leak(commands.into_boxed_slice()));

    // tauri-build embeds its app manifest only into bins, so `cargo test` binaries would start
    // without Common Controls v6 and die with STATUS_ENTRYPOINT_NOT_FOUND. Embed our own copy of
    // the same manifest into every linked target instead.
    let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows).app_manifest(app))
        .expect("tauri-build failed");

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").expect("manifest dir"))
            .join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}
