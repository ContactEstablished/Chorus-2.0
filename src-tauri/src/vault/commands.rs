//! Vault commands. A key crosses IPC once, inward, when it is saved; nothing returns it.

use std::sync::Arc;

use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::sessions::SessionManager;
use crate::store::credentials::{self, CredentialRow};

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CredentialSummary {
    pub id: u32,
    pub label: String,
    /// First 16 hex digits of the key's SHA-256: enough to tell keys apart, nothing more.
    pub fingerprint: String,
    pub created_at_secs: u32,
    pub last_used_at_secs: Option<u32>,
}

impl From<CredentialRow> for CredentialSummary {
    fn from(r: CredentialRow) -> Self {
        Self {
            id: r.id,
            label: r.label,
            fingerprint: r.fingerprint,
            created_at_secs: secs(r.created_at),
            last_used_at_secs: r.last_used_at.map(secs),
        }
    }
}

pub fn secs(ms: i64) -> u32 {
    u32::try_from(ms / 1000).unwrap_or(0)
}

fn summary(m: &SessionManager, id: u32) -> Result<CredentialSummary, String> {
    credentials::get(m.store(), id)?.map(Into::into).ok_or_else(|| "that key no longer exists".into())
}

#[tauri::command]
#[specta::specta]
pub fn vault_list(state: State<'_, Arc<SessionManager>>) -> Result<Vec<CredentialSummary>, String> {
    Ok(credentials::list(state.store())?.into_iter().map(Into::into).collect())
}

#[tauri::command]
#[specta::specta]
pub async fn vault_add(state: State<'_, Arc<SessionManager>>, label: String, secret: String) -> Result<CredentialSummary, String> {
    let m = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let id = super::add(m.store(), &label, &secret)?;
        summary(&m, id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
#[specta::specta]
pub fn vault_delete(state: State<'_, Arc<SessionManager>>, id: u32) -> Result<bool, String> {
    credentials::delete(state.store(), id)
}

// ── Dev builds only: tools for the P1 exit check. A generated test key never crosses IPC, and the
// leak check returns lengths, never bytes (as in spike S4).

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LeakReport {
    /// Longest run of the key (8 bytes or more) found in the text passed in, such as a pane's screen.
    pub text: u32,
    /// Every file under the data folder with any run of the key, and the longest run in it.
    pub files: Vec<FileLeak>,
    pub files_scanned: u32,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FileLeak {
    pub path: String,
    pub longest: u32,
}

/// Generates a random OpenRouter-shaped fake key, seals and stores it.
#[tauri::command]
#[specta::specta]
pub fn dev_vault_add_test_key(state: State<'_, Arc<SessionManager>>, label: String) -> Result<CredentialSummary, String> {
    if !cfg!(debug_assertions) {
        return Err("only available in dev builds".into());
    }
    let hex: String = super::random_bytes(32).iter().map(|b| format!("{b:02x}")).collect();
    let id = super::add(state.store(), &label, &format!("sk-or-v1-{hex}"))?;
    summary(&state, id)
}

/// Looks for any run of a stored key in `text` and in every file under the data folder (database,
/// its WAL, scrollback files).
#[tauri::command]
#[specta::specta]
pub async fn dev_leak_check(state: State<'_, Arc<SessionManager>>, id: u32, text: String) -> Result<LeakReport, String> {
    if !cfg!(debug_assertions) {
        return Err("only available in dev builds".into());
    }
    let m = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        use chorus_core::scrub::longest_shared_run;
        let key = super::decrypt(m.store(), id)?.into_bytes();
        let mut files = Vec::new();
        let mut scanned = 0u32;
        let mut stack = vec![m.data_dir().to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    // WebView2's own cache is not ours to scan.
                    if !path.ends_with("EBWebView") {
                        stack.push(path);
                    }
                    continue;
                }
                let Ok(bytes) = std::fs::read(&path) else { continue };
                scanned += 1;
                let longest = longest_shared_run(&key, &bytes, 8);
                if longest > 0 {
                    files.push(FileLeak { path: path.display().to_string(), longest: longest as u32 });
                }
            }
        }
        Ok(LeakReport { text: longest_shared_run(&key, text.as_bytes(), 8) as u32, files, files_scanned: scanned })
    })
    .await
    .map_err(|e| e.to_string())?
}
