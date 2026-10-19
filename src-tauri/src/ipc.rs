//! The typed IPC surface: every command and event the front end can use (D12).
//!
//! TypeScript bindings are generated from this builder into `src/ipc/bindings.ts`. Debug builds
//! regenerate them on launch, and `bindings_are_current` fails `cargo test` if the committed file
//! is stale. Never edit the generated file by hand.

use specta_typescript::Typescript;
use std::path::Path;
use tauri_specta::{Builder, collect_commands, collect_events};

use crate::{app, sessions};

/// Where the generated bindings live, relative to this crate.
pub const BINDINGS_FILE: &str = "../src/ipc/bindings.ts";

pub fn builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            app::app_info,
            sessions::pty_spawn,
            sessions::pty_write,
            sessions::pty_resize,
            sessions::pty_close,
            sessions::spike_vault_add_fake_key,
            sessions::spike_vault_contains,
        ])
        .events(collect_events![app::BackendPulse, sessions::PtyExited])
}

pub fn export_bindings(builder: &Builder<tauri::Wry>, path: &Path) -> Result<(), specta_typescript::Error> {
    builder.export(
        Typescript::default().header("// Source: src-tauri/src/ipc.rs. Regenerate with `npm run bindings`.\n"),
        path,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn bindings_are_current() {
        let committed = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(BINDINGS_FILE);
        let fresh = std::env::temp_dir().join(format!("chorus-bindings-{}.ts", std::process::id()));
        export_bindings(&builder(), &fresh).expect("export bindings");
        let fresh_text = std::fs::read_to_string(&fresh).expect("read fresh bindings");
        let _ = std::fs::remove_file(&fresh);

        let committed_text = std::fs::read_to_string(&committed).unwrap_or_default();
        if fresh_text != committed_text {
            if std::env::var_os("CHORUS_UPDATE_BINDINGS").is_some() {
                std::fs::write(&committed, &fresh_text).expect("write bindings");
            } else {
                panic!("src/ipc/bindings.ts is stale; run `npm run bindings` to regenerate it");
            }
        }
    }
}
