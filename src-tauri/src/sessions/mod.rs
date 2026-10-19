//! The PTY host (spike S1; the full SessionManager is P1).
//!
//! The back end owns every agent process. A session is a ConPTY with one child in a Job Object;
//! output is read on a thread, coalesced by `chorus_core::batch`, and streamed to the attached view
//! over a `tauri::ipc::Channel` as raw bytes (no JSON). The front end only asks; it never spawns.

pub mod job;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chorus_core::batch::{PTY_BATCH, SCRUB_CARRY_RELEASE_MS};
use chorus_core::env::{ChildEnvInput, compose_child_env, valid_secret_var};
use chorus_core::scrub::{StreamScrubber, builtin_patterns};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::ipc::{Channel, InvokeResponseBody, IpcResponse};
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::agents::{self, AgentKind};
use job::Job;

/// Raw PTY output for an attached view. Sent as bytes, so the webview gets an `ArrayBuffer`.
pub struct PtyBytes(pub Vec<u8>);

impl IpcResponse for PtyBytes {
    fn body(self) -> tauri::Result<InvokeResponseBody> {
        Ok(InvokeResponseBody::Raw(self.0))
    }
}

impl Type for PtyBytes {
    fn definition(_: &mut specta::Types) -> specta::datatype::DataType {
        specta_typescript::define("ArrayBuffer").into()
    }
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SpawnRequest {
    pub agent: AgentKind,
    pub cols: u16,
    pub rows: u16,
    /// A vault key to inject into the child's environment, decrypted only now.
    pub credential: Option<CredentialRef>,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CredentialRef {
    pub label: String,
    pub env_var: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SpawnResult {
    pub session_id: u32,
    pub pid: u32,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    /// Names only, so the UI can show what the child received without showing values.
    pub env_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CloseReport {
    /// Every process that was in the session's job when Close was pressed.
    pub job_pids: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct PtyExited {
    pub session_id: u32,
    pub exit_code: u32,
}

struct Session {
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    job: Job,
}

pub struct SessionManager {
    sessions: Mutex<HashMap<u32, Arc<Session>>>,
    next_id: AtomicU32,
    /// Agents run here during the spikes: inside 2.0's data folder, outside any repo.
    scratch_root: PathBuf,
    /// DPAPI blobs by label. In memory for the S4 spike; SQLite in P1.
    vault: Mutex<HashMap<String, Vec<u8>>>,
}

const MAX_WRITE_BYTES: usize = 1 << 20;

fn check_size(cols: u16, rows: u16) -> Result<PtySize, String> {
    if !(2..=1000).contains(&cols) || !(2..=1000).contains(&rows) {
        return Err(format!("terminal size {cols}x{rows} is out of range"));
    }
    Ok(PtySize { cols, rows, pixel_width: 0, pixel_height: 0 })
}

impl SessionManager {
    pub fn new(scratch_root: PathBuf) -> Self {
        Self { sessions: Mutex::new(HashMap::new()), next_id: AtomicU32::new(1), scratch_root, vault: Mutex::default() }
    }

    /// Decrypts a stored key for a launch. The plaintext lives only as long as the launch needs it.
    fn decrypt(&self, label: &str) -> Result<Vec<u8>, String> {
        let vault = self.vault.lock().expect("vault lock");
        let sealed = vault.get(label).ok_or_else(|| format!("no key labelled {label:?}"))?;
        crate::vault::unprotect(sealed).map_err(|e| format!("decrypting {label:?}: {e}"))
    }

    fn get(&self, id: u32) -> Result<Arc<Session>, String> {
        self.sessions.lock().expect("sessions lock").get(&id).cloned().ok_or_else(|| format!("no session {id}"))
    }

    fn spawn(&self, app: AppHandle, req: SpawnRequest, output: Channel<PtyBytes>) -> Result<SpawnResult, String> {
        let size = check_size(req.cols, req.rows)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let cwd = self.scratch_root.join(format!("{:?}", req.agent).to_lowercase());
        let state_dir = self.scratch_root.join("state").join(format!("session-{id}"));
        std::fs::create_dir_all(&cwd).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&state_dir).map_err(|e| e.to_string())?;

        let parent: Vec<(String, String)> = std::env::vars().collect();
        let launch = agents::resolve(req.agent, &parent, &state_dir)?;
        let additions: Vec<(&str, &str)> = launch.additions.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let mut secrets: Vec<(String, String)> = Vec::new();
        if let Some(cred) = &req.credential {
            if !valid_secret_var(&cred.env_var) {
                return Err(format!("{:?} is not a usable variable name for a key", cred.env_var));
            }
            let key = String::from_utf8(self.decrypt(&cred.label)?).map_err(|_| "stored key is not text")?;
            secrets.push((cred.env_var.clone(), key));
        }
        let env = compose_child_env(&ChildEnvInput { parent: &parent, required: &[], additions: &additions, secrets: &secrets });
        let scrubber = StreamScrubber::new(secrets.iter().map(|(_, v)| v.clone().into_bytes()), builtin_patterns());

        let mut cmd = CommandBuilder::new(&launch.program);
        cmd.args(&launch.args);
        cmd.cwd(&cwd);
        cmd.env_clear();
        for (k, v) in &env {
            cmd.env(k, v);
        }

        let pair = native_pty_system().openpty(size).map_err(|e| e.to_string())?;
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| format!("spawning {}: {e}", launch.program.display()))?;
        drop(pair.slave);

        let job = Job::new().map_err(|e| e.to_string())?;
        if let Some(handle) = child.as_raw_handle() {
            job.assign(windows::Win32::Foundation::HANDLE(handle)).map_err(|e| e.to_string())?;
        }
        let pid = child.process_id().unwrap_or(0);
        let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let session = Arc::new(Session {
            writer: Mutex::new(writer),
            master: Mutex::new(Some(pair.master)),
            killer: Mutex::new(child.clone_killer()),
            job,
        });
        self.sessions.lock().expect("sessions lock").insert(id, session.clone());

        stream_output(reader, scrubber, output);

        // Exit watcher: report the exit, then close the pseudoconsole so the reader sees EOF
        // (ConPTY keeps the pipe open until it is closed).
        std::thread::spawn(move || {
            let exit_code = child.wait().map(|s| s.exit_code()).unwrap_or(u32::MAX);
            std::thread::sleep(Duration::from_millis(250));
            let _ = PtyExited { session_id: id, exit_code }.emit(&app);
            session.master.lock().expect("master lock").take();
        });

        Ok(SpawnResult {
            session_id: id,
            pid,
            program: launch.program.display().to_string(),
            args: launch.args,
            cwd: cwd.display().to_string(),
            env_names: env.keys().cloned().collect(),
        })
    }

    fn close(&self, id: u32) -> Result<CloseReport, String> {
        let session = self.sessions.lock().expect("sessions lock").remove(&id).ok_or_else(|| format!("no session {id}"))?;
        let job_pids = session.job.process_ids().unwrap_or_default();
        session.job.terminate(1).map_err(|e| e.to_string())?;
        let _ = session.killer.lock().expect("killer lock").kill();
        Ok(CloseReport { job_pids })
    }
}

/// Reader thread → scrubber → batching thread → channel. Nothing reaches the view unscrubbed, and
/// the reader never blocks on the webview.
fn stream_output(mut reader: Box<dyn Read + Send>, mut scrubber: StreamScrubber, output: Channel<PtyBytes>) {
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    std::thread::spawn(move || {
        let mut pending: Vec<u8> = Vec::new();
        let mut oldest: Option<Instant> = None;
        let mut last_input = Instant::now();
        loop {
            let waited = oldest.map_or(0, |t| t.elapsed().as_millis() as u64);
            let next = if !pending.is_empty() {
                rx.recv_timeout(Duration::from_millis(PTY_BATCH.wait_ms(waited)))
            } else if scrubber.pending() > 0 {
                rx.recv_timeout(Duration::from_millis(SCRUB_CARRY_RELEASE_MS))
            } else {
                rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
            };
            match next {
                Ok(chunk) => {
                    last_input = Instant::now();
                    let safe = scrubber.push(&chunk);
                    if !safe.is_empty() {
                        oldest.get_or_insert_with(Instant::now);
                        pending.extend_from_slice(&safe);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    // Output went quiet with a possible key start held back: release it.
                    if scrubber.pending() > 0 && last_input.elapsed() >= Duration::from_millis(SCRUB_CARRY_RELEASE_MS) {
                        let safe = scrubber.flush();
                        if !safe.is_empty() {
                            oldest.get_or_insert_with(Instant::now);
                            pending.extend_from_slice(&safe);
                        }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    pending.extend(scrubber.flush());
                    if !pending.is_empty() {
                        let _ = output.send(PtyBytes(std::mem::take(&mut pending)));
                    }
                    break;
                }
            }
            let waited = oldest.map_or(0, |t| t.elapsed().as_millis() as u64);
            if PTY_BATCH.should_flush(pending.len(), waited) {
                // A closed view is not an error for the session; keep draining the PTY.
                let _ = output.send(PtyBytes(std::mem::take(&mut pending)));
                oldest = None;
            }
        }
    });
}

#[tauri::command]
#[specta::specta]
pub fn pty_spawn(
    app: AppHandle,
    state: State<'_, SessionManager>,
    request: SpawnRequest,
    output: Channel<PtyBytes>,
) -> Result<SpawnResult, String> {
    state.spawn(app, request, output)
}

#[tauri::command]
#[specta::specta]
pub fn pty_write(state: State<'_, SessionManager>, session_id: u32, data: String) -> Result<(), String> {
    if data.len() > MAX_WRITE_BYTES {
        return Err("write too large".into());
    }
    let session = state.get(session_id)?;
    let mut writer = session.writer.lock().expect("writer lock");
    writer.write_all(data.as_bytes()).and_then(|()| writer.flush()).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn pty_resize(state: State<'_, SessionManager>, session_id: u32, cols: u16, rows: u16) -> Result<(), String> {
    let size = check_size(cols, rows)?;
    let session = state.get(session_id)?;
    let master = session.master.lock().expect("master lock");
    master.as_ref().ok_or("session has exited")?.resize(size).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn pty_close(state: State<'_, SessionManager>, session_id: u32) -> Result<CloseReport, String> {
    state.close(session_id)
}

// ── Spike S4 helpers. The key never crosses IPC: only its length, fingerprint and yes/no answers.

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct FakeKeyInfo {
    pub label: String,
    pub key_len: u32,
    pub blob_len: u32,
    /// First 16 hex digits of the key's SHA-256.
    pub fingerprint: String,
    /// Where the sealed blob was written, so it can be inspected at rest.
    pub blob_path: String,
}

/// Generates a random, OpenRouter-shaped fake key, seals it with DPAPI and stores the blob.
#[tauri::command]
#[specta::specta]
pub fn spike_vault_add_fake_key(state: State<'_, SessionManager>, label: String) -> Result<FakeKeyInfo, String> {
    if label.is_empty() || label.len() > 64 || !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("label must be 1-64 letters, digits or dashes".into());
    }
    let hex: String = crate::vault::random_bytes(32).iter().map(|b| format!("{b:02x}")).collect();
    let key = format!("sk-or-v1-{hex}");
    let sealed = crate::vault::protect(key.as_bytes()).map_err(|e| e.to_string())?;
    let dir = state.scratch_root.join("vault");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let blob_path = dir.join(format!("{label}.bin"));
    std::fs::write(&blob_path, &sealed).map_err(|e| e.to_string())?;
    let info = FakeKeyInfo {
        label: label.clone(),
        key_len: key.len() as u32,
        blob_len: sealed.len() as u32,
        fingerprint: crate::vault::fingerprint(key.as_bytes()),
        blob_path: blob_path.display().to_string(),
    };
    state.vault.lock().expect("vault lock").insert(label, sealed);
    Ok(info)
}

/// The length of the longest piece of the stored key that appears in `text` (0 to the key's
/// length). Catches partial leaks; returns a number, never the key.
#[tauri::command]
#[specta::specta]
pub fn spike_vault_contains(state: State<'_, SessionManager>, label: String, text: String) -> Result<u32, String> {
    if text.len() > MAX_WRITE_BYTES {
        return Err("text too large".into());
    }
    let key = state.decrypt(&label)?;
    let hay = text.as_bytes();
    let longest = (1..=key.len())
        .rev()
        .find(|&n| key.windows(n).any(|piece| hay.windows(n).any(|w| w == piece)))
        .unwrap_or(0);
    Ok(longest as u32)
}
