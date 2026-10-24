use super::{SessionManager, commands::Manager};
use crate::store::{composer, images};
use chorus_core::agent::AgentKind;
use std::io::Write;

pub fn unsupported(agent: AgentKind) -> Option<String> {
    (!matches!(agent, AgentKind::Claude | AgentKind::Codex))
        .then(|| "Image delivery is available for Claude and Codex. Use Raw for this agent.".into())
}

/// Recreate from the durable bytes before each delivery. Source files may have moved or vanished.
pub(super) fn materialize(manager: &SessionManager, session: u32, images: &[images::ComposerImage]) -> Result<Vec<String>, String> {
    if images.is_empty() {
        return Ok(Vec::new());
    }
    let dir = manager.data_dir().join("images").join(session.to_string());
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not save attached images: {e}"))?;
    let mut paths = Vec::new();
    for image in images {
        let bytes = images::bytes(&manager.store, session, image.id)?;
        chorus_core::attachments::validate_png(&bytes)?;
        let path = dir.join(format!("{}.png", image.id));
        let mut file = std::fs::File::create(&path).map_err(|e| format!("Could not save an attached image: {e}"))?;
        file.write_all(&bytes).and_then(|()| file.sync_all()).map_err(|e| format!("Could not save an attached image: {e}"))?;
        paths.push(path.to_string_lossy().into_owned());
    }
    Ok(paths)
}

#[tauri::command]
#[specta::specta]
pub async fn session_image_add(
    state: Manager<'_>,
    session_id: u32,
    revision: u32,
    name: String,
    png: Vec<u8>,
) -> Result<composer::Draft, String> {
    let m = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let row = m.row(session_id)?;
        if let Some(reason) = unsupported(row.agent) {
            return Err(reason);
        }
        let name: String = name.chars().filter(|c| !c.is_control()).take(120).collect();
        let (keys, unreadable) = crate::vault::all_for_scrubbing(&m.store);
        if unreadable != 0 {
            return Err("A vault key could not be read for scrubbing. Image was not saved.".into());
        }
        let name = String::from_utf8_lossy(&chorus_core::scrub::scrub_all(name.as_bytes(), &keys, &chorus_core::scrub::builtin_patterns()))
            .into_owned();
        images::add(&m.store, session_id, revision, &name, &png)?;
        composer::draft(&m.store, session_id)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
#[specta::specta]
pub async fn session_image_remove(state: Manager<'_>, session_id: u32, revision: u32, image_id: u32) -> Result<composer::Draft, String> {
    let m = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        m.row(session_id)?;
        images::remove(&m.store, session_id, revision, image_id)?;
        composer::draft(&m.store, session_id)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
#[specta::specta]
pub async fn session_image_read(state: Manager<'_>, session_id: u32, image_id: u32) -> Result<Vec<u8>, String> {
    let m = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        m.row(session_id)?;
        images::bytes(&m.store, session_id, image_id)
    })
    .await
    .map_err(|e| e.to_string())?
}
