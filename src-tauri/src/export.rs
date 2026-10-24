//! Transcript export (P7): a session's recorded history as Markdown (`chorus_core::export`), saved
//! where the user chooses in Windows' save dialog (called from Rust only, as the folder picker is,
//! D54). Optionally the agent's own transcript file goes beside it, scrubbed with the built-in
//! patterns and every vault key. Nothing is overwritten without the dialog's own confirmation.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chorus_core::export::{self, ExportAsk, ExportSession, ExportTool, ExportTurn};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager as _, State};

use crate::sessions::SessionManager;
use crate::store::history;

type ManagerState<'a> = State<'a, Arc<SessionManager>>;

/// Every turn a session can have is exported; this only bounds a runaway history.
const MAX_TURNS: u32 = 100_000;
/// The agent's own transcript is read whole to scrub it; larger files aren't copied.
const MAX_TRANSCRIPT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: String,
    pub turns: u32,
    /// Where the scrubbed copy of the agent's transcript went, if it was asked for and written.
    pub transcript_path: Option<String>,
    /// Why the agent's transcript wasn't copied, when it was asked for.
    pub transcript_note: Option<String>,
}

/// Removes secrets from bytes: the built-in patterns and every vault key.
type Scrub = Box<dyn Fn(&[u8]) -> Vec<u8> + Send>;

fn scrubber(manager: &SessionManager) -> Result<Scrub, String> {
    let (keys, unreadable) = crate::vault::all_for_scrubbing(manager.store());
    if unreadable != 0 {
        return Err("A vault key could not be read for scrubbing, so nothing was exported.".into());
    }
    let patterns = chorus_core::scrub::builtin_patterns();
    Ok(Box::new(move |bytes: &[u8]| chorus_core::scrub::scrub_all(bytes, &keys, &patterns)))
}

/// Where the copy of the agent's transcript goes: beside the export, with the source's extension.
fn transcript_target(export: &Path, source: &Path) -> PathBuf {
    let stem = export.file_stem().map_or_else(|| "transcript".into(), |s| s.to_string_lossy().into_owned());
    let ext = source.extension().map_or_else(|| "txt".into(), |e| e.to_string_lossy().into_owned());
    export.with_file_name(format!("{stem} (agent transcript, scrubbed).{ext}"))
}

/// Exports a session's history. Returns none when the save dialog is cancelled.
#[tauri::command]
#[specta::specta]
pub async fn session_export(app: AppHandle, state: ManagerState<'_>, session_id: u32, include_transcript: bool) -> Result<Option<ExportResult>, String> {
    let manager = state.inner().clone();
    // The history (every turn) is read, rendered and scrubbed off the async runtime.
    let preparing = manager.clone();
    let Rendered { text, file_name: default_name, turns, scrub } =
        tauri::async_runtime::spawn_blocking(move || render(&preparing, session_id)).await.map_err(|e| e.to_string())??;

    let chosen = tauri::async_runtime::spawn_blocking(move || {
        use tauri_plugin_dialog::DialogExt;
        let mut dialog =
            app.dialog().file().set_title("Export transcript").set_file_name(default_name).add_filter("Markdown", &["md"]);
        if let Some(window) = app.get_webview_window("main") {
            dialog = dialog.set_parent(&window);
        }
        dialog.blocking_save_file().and_then(|p| p.into_path().ok())
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(path) = chosen else { return Ok(None) };
    if !path.is_absolute() || !path.parent().is_some_and(Path::is_dir) {
        return Err(format!("{} isn't in a folder that exists.", path.display()));
    }
    let transcript = if include_transcript { Some(history::session_transcript(manager.store(), session_id)?) } else { None };
    // Reading and scrubbing a transcript of tens of MB mustn't hold up an async worker.
    tauri::async_runtime::spawn_blocking(move || write_files(&path, &text, transcript, &scrub, turns))
        .await
        .map_err(|e| e.to_string())?
        .map(Some)
}

/// The session's history as scrubbed Markdown, a file name for it, its turn count, and the scrubber
/// for the agent's transcript.
struct Rendered {
    text: Vec<u8>,
    file_name: String,
    turns: u32,
    scrub: Scrub,
}

fn render(manager: &SessionManager, session_id: u32) -> Result<Rendered, String> {
    let summary = manager.summary(session_id)?;
    let turns = history::recent_turns(manager.store(), session_id, MAX_TURNS)?;
    let rendered: Vec<ExportTurn> = turns
        .iter()
        .map(|t| ExportTurn {
            prompt: t.prompt.as_deref(),
            source: t.prompt_source.as_deref(),
            started_at: i64::from(t.started_at_secs),
            ended_at: t.ended_at_secs.map(i64::from),
            end_reason: t.end_reason.as_deref(),
            final_message: t.final_message.as_deref(),
            tools: t
                .tools
                .iter()
                .map(|x| ExportTool { tool: &x.tool, target: x.target.as_deref(), failed: x.failed, finished: x.finished })
                .collect(),
            asks: t.asks.iter().map(|a| ExportAsk { kind: &a.kind, subject: &a.subject, answer: a.answer.as_deref() }).collect(),
        })
        .collect();
    let now = crate::store::now_ms() / 1000;
    let session = ExportSession { name: &summary.name, agent: summary.agent.display_name(), cwd: &summary.cwd, exported_at: now };
    let scrub = scrubber(manager)?;
    let text = scrub(export::markdown(&session, &rendered).as_bytes());
    Ok(Rendered { text, file_name: export::file_name(&summary.name), turns: rendered.len() as u32, scrub })
}

/// Writes the export and, when asked (`transcript` is `Some`), the scrubbed agent transcript.
fn write_files(path: &Path, text: &[u8], transcript: Option<Option<String>>, scrub: &dyn Fn(&[u8]) -> Vec<u8>, turns: u32) -> Result<ExportResult, String> {
    std::fs::write(path, text).map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
    let (mut transcript_path, mut transcript_note) = (None, None);
    match transcript {
        None => {}
        Some(None) => transcript_note = Some("This session has no agent transcript on record.".into()),
        Some(Some(source)) => {
            let source = PathBuf::from(source);
            let target = transcript_target(path, &source);
            match std::fs::metadata(&source) {
                Err(e) => transcript_note = Some(format!("The agent's transcript couldn't be read: {e}")),
                Ok(m) if m.len() > MAX_TRANSCRIPT_BYTES => {
                    transcript_note = Some(format!("The agent's transcript is larger than {} MB, so it wasn't copied.", MAX_TRANSCRIPT_BYTES >> 20));
                }
                Ok(_) if target.exists() => {
                    transcript_note = Some(format!("{} already exists, so the agent's transcript wasn't written.", target.display()));
                }
                Ok(_) => match std::fs::read(&source) {
                    Err(e) => transcript_note = Some(format!("The agent's transcript couldn't be read: {e}")),
                    Ok(bytes) => {
                        std::fs::write(&target, scrub(&bytes)).map_err(|e| format!("Couldn't write {}: {e}", target.display()))?;
                        transcript_path = Some(target.display().to_string());
                    }
                },
            }
        }
    }
    Ok(ExportResult { path: path.display().to_string(), turns, transcript_path, transcript_note })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_transcript_copy_sits_beside_the_export() {
        let target = transcript_target(Path::new("C:/out/Ada transcript.md"), Path::new("C:/agent/abc.jsonl"));
        assert_eq!(target, Path::new("C:/out/Ada transcript (agent transcript, scrubbed).jsonl"));
    }
}
