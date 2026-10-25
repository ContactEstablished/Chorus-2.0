//! The typed IPC surface: every command and event the front end can use (D12).
//!
//! TypeScript bindings are generated from this builder into `src/ipc/bindings.ts`. Debug builds
//! regenerate them on launch, and `bindings_are_current` fails `cargo test` if the committed file
//! is stale. Never edit the generated file by hand.

use specta_typescript::Typescript;
use std::path::Path;
use tauri_specta::{Builder, collect_commands, collect_events};

use crate::{agents, app, attention, export, fleet, fleet_registry, lock, narrator, prefs, projects, providers, routing, sessions, splash, vault};
use crate::{automations, bots, documents, instructions, notes};

/// Where the generated bindings live, relative to this crate.
#[cfg_attr(not(debug_assertions), allow(dead_code, reason = "debug builds and tests regenerate the bindings"))]
pub const BINDINGS_FILE: &str = "../src/ipc/bindings.ts";

pub fn builder() -> Builder<tauri::Wry> {
    use sessions::commands as s;
    use vault::commands as v;
    Builder::<tauri::Wry>::new()
        .commands(collect_commands![
            app::app_info,
            narrator::narrator_settings,
            narrator::narrator_configure,
            narrator::narrator_project_set,
            narrator::session_narrator,
            narrator::narrator_summarize,
            narrator::narrator_action_set,
            prefs::prefs_get,
            prefs::prefs_set,
            projects::projects_list,
            projects::project_palette,
            projects::project_save,
            projects::project_set_state,
            projects::projects_reorder,
            projects::project_delete,
            projects::session_set_project,
            agents::commands::agents_list,
            agents::commands::launch_args,
            providers::commands::providers_list,
            providers::commands::provider_save,
            providers::commands::provider_delete,
            providers::commands::provider_test,
            providers::commands::provider_refresh_catalog,
            providers::commands::provider_catalog,
            providers::commands::provider_set_shortlist,
            routing::commands::routing_tiers,
            routing::commands::routing_refresh,
            routing::commands::routing_watch,
            s::sessions_list,
            s::session_open_folder,
            s::session_create,
            s::sessions_launch,
            s::launch_presets,
            s::session_rename,
            s::session_set_description,
            s::session_history,
            sessions::profiles::profiles_list,
            sessions::profiles::profile_save,
            sessions::profiles::profile_delete,
            s::session_attach,
            s::session_detach,
            s::session_write,
            s::session_resize,
            s::session_restart,
            s::session_close,
            s::session_close_check,
            s::session_set_tags,
            s::tags_known,
            s::session_suggest_name,
            s::folder_check,
            s::worktrees_list,
            s::session_git_status,
            s::folders_recent,
            s::folder_pick,
            s::session_set_locked,
            lock::lock_pin_status,
            lock::lock_pin_set,
            lock::lock_pin_clear,
            v::vault_list,
            v::vault_add,
            v::vault_delete,
            v::dev_vault_add_test_key,
            v::dev_leak_check,
            splash::restore_progress,
            splash::splash_replay,
            // P6: shared action routing and explicit channel availability.
            sessions::actions::session_action_capabilities,
            sessions::actions::session_action,
            sessions::actions::session_pending_replies,
            sessions::composer::session_composer,
            sessions::composer::session_draft_save,
            sessions::composer::session_composer_submit,
            sessions::composer::session_restart_send,
            sessions::composer::session_queue_change,
            sessions::composer::session_queue_send,
            sessions::images::session_image_add,
            sessions::images::session_image_remove,
            sessions::images::session_image_read,
            // P7: notifications, the tray and the fleet strip.
            attention::notifications_list,
            attention::notification_read,
            attention::notifications_read_all,
            fleet::fleet_shown,
            fleet::fleet_set_shown,
            fleet::fleet_open_session,
            fleet::fleet_probe,
            export::session_export,
            fleet_registry::fleet_roster,
            fleet_registry::fleet_acknowledge,
            // p8: Bots, notes, instructions, documents and automations.
            bots::bots_list,
            bots::bot_save,
            bots::bot_set_archived,
            notes::notes_list,
            notes::note_save,
            notes::note_delete,
            instructions::session_instructions,
            instructions::instructions_support,
            documents::documents_list,
            documents::document_open,
            automations::automations_list,
            automations::automation_save,
            automations::automation_set_enabled,
            automations::automation_delete,
            automations::automation_run_now,
            automations::automation_runs,
        ])
        .events(collect_events![
            app::BackendPulse,
            sessions::SessionUpdated,
            fleet::FleetClick,
            // P7
            attention::NotificationAdded,
            attention::AttentionNavigate,
            fleet::FleetShown,
            fleet_registry::FleetRosterChanged,
            prefs::PrefsChanged,
        ])
}

#[cfg_attr(not(debug_assertions), allow(dead_code, reason = "debug builds and tests regenerate the bindings"))]
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

    mod command_list {
        include!(concat!(env!("CARGO_MANIFEST_DIR"), "/command_list.rs"));
    }

    /// D92: the app manifest `build.rs` reads from `collect_commands!` is exactly what the builder
    /// registers (the bindings are generated from it), and each window is granted only its commands.
    #[test]
    fn command_permissions_match_registration() {
        use std::collections::BTreeSet;
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let read = |path: &str| std::fs::read_to_string(root.join(path)).expect(path);
        let registered = command_list::registered_commands(&read("src/ipc.rs"));
        let bindings = read(BINDINGS_FILE);
        let invoked: BTreeSet<String> = bindings
            .split("__TAURI_INVOKE")
            .skip(1)
            .filter_map(|call| {
                let call = &call[..call.find('\n')?];
                let name = &call[call.find("(\"")? + 2..];
                Some(name[..name.find('"')?].to_owned())
            })
            .collect();
        let unique: BTreeSet<String> = registered.iter().cloned().collect();
        assert_eq!(unique.len(), registered.len(), "a command is registered twice");
        assert_eq!(unique, invoked, "build.rs's reading of collect_commands! differs from the bindings");

        let grants = |window: &str| -> BTreeSet<String> {
            let capability: serde_json::Value = serde_json::from_str(&read(&format!("capabilities/{window}.json"))).expect(window);
            capability["permissions"].as_array().expect("permissions").iter()
                .filter_map(serde_json::Value::as_str).filter(|p| !p.contains(':')).map(str::to_owned).collect()
        };
        let named = |commands: &[&str]| commands.iter().map(|c| command_list::allow(c)).collect::<BTreeSet<_>>();
        assert_eq!(grants("main"), BTreeSet::from(["main-window".to_owned()]));
        assert_eq!(grants("fleet"), named(&["sessions_list", "prefs_get", "fleet_open_session"]));
        assert_eq!(grants("splash"), named(&["restore_progress"]));
        let set = command_list::main_window_set(&registered);
        assert!(registered.iter().all(|c| set.contains(&format!("\"{}\"", command_list::allow(c)))));
    }
}
