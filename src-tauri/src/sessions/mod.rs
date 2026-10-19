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

use chorus_core::batch::PTY_BATCH;
use chorus_core::env::{ChildEnvInput, compose_child_env};
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
        Self { sessions: Mutex::new(HashMap::new()), next_id: AtomicU32::new(1), scratch_root }
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
        let env = compose_child_env(&ChildEnvInput { parent: &parent, required: &[], additions: &additions, secrets: &[] });

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

        stream_output(reader, output);

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

/// Reader thread → batching thread → channel. The reader never blocks on the webview.
fn stream_output(mut reader: Box<dyn Read + Send>, output: Channel<PtyBytes>) {
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
        loop {
            let waited = oldest.map_or(0, |t| t.elapsed().as_millis() as u64);
            let next = if pending.is_empty() {
                rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
            } else {
                rx.recv_timeout(Duration::from_millis(PTY_BATCH.wait_ms(waited)))
            };
            match next {
                Ok(chunk) => {
                    oldest.get_or_insert_with(Instant::now);
                    pending.extend_from_slice(&chunk);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
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
