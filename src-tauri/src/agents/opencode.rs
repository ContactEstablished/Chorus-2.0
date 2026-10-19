//! The `opencode serve` a run starts, and its API (the rules are in `chorus_core::opencode`). The
//! server is a hidden child in the run's job, listening on loopback behind a password that travels
//! only in the environment, and it dies with the run. `opencode attach` in the PTY is its TUI.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

use chorus_core::opencode::{self, ModelCheck, Observed};
use serde_json::json;

use crate::sessions::job::Job;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// The first start in a new data folder sets that folder up.
const START_TIMEOUT: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// How often a run reads back what its newest turn ran with.
pub const POLL: Duration = Duration::from_secs(3);

/// The state folder `agents::resolve` gives opencode inside a session's folder. Its TUI keeps the
/// last variant it used per model there, and that beats the configuration (LESSONS L5).
pub fn state_home(scratch: &Path) -> PathBuf {
    scratch.join("xdg-state")
}

/// Drops the TUI's remembered model choices, so a run starts with the model and effort the
/// session asks for. The folder is the session's own, never the user's opencode state.
pub fn forget_tui_choice(scratch: &Path) {
    let _ = std::fs::remove_file(state_home(scratch).join("opencode").join("model.json"));
}

/// The server's API, shared with the run's read-back thread.
#[derive(Clone)]
pub struct Api {
    url: String,
    password: String,
    client: reqwest::Client,
}

pub struct Server {
    child: Mutex<Child>,
    pub api: Api,
}

impl Server {
    /// Starts `opencode serve` in `cwd` with `env` (which carries the password and configuration),
    /// puts it in `job`, and waits until it says where it listens.
    pub fn start(
        program: &Path,
        lead_args: &[String],
        cwd: &str,
        env: &BTreeMap<String, String>,
        password: &str,
        job: &Job,
    ) -> Result<Self, String> {
        let mut child = Command::new(program)
            .args(lead_args)
            .args(opencode::serve_args())
            .env_clear()
            .envs(env)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("starting opencode serve: {e}"))?;
        if let Err(e) = job.assign(windows::Win32::Foundation::HANDLE(child.as_raw_handle())) {
            let _ = child.kill();
            return Err(format!("putting opencode serve in its job: {e}"));
        }
        let stdout = child.stdout.take().ok_or("opencode serve has no output")?;
        let (tx, rx) = mpsc::channel::<String>();
        // Reads until the address appears, then keeps draining so the server never blocks on a
        // full pipe.
        let _ = std::thread::Builder::new().name("opencode-serve-out".into()).spawn(move || {
            let mut sent = false;
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if !sent && let Some(url) = opencode::listening_url(&line) {
                    sent = tx.send(url).is_ok();
                }
            }
        });
        let url = match rx.recv_timeout(START_TIMEOUT) {
            Ok(url) => url,
            Err(_) => {
                let exited = child.try_wait().ok().flatten();
                let _ = child.kill();
                return Err(match exited {
                    Some(status) => format!("opencode serve exited ({status}) before it was ready"),
                    None => format!("opencode serve wasn't ready in {} s", START_TIMEOUT.as_secs()),
                });
            }
        };
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .no_proxy()
            .build()
            .map_err(|e| format!("making the HTTP client: {e}"))?;
        Ok(Self { child: Mutex::new(child), api: Api { url, password: password.to_owned(), client } })
    }

    pub fn url(&self) -> &str {
        &self.api.url
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let mut child = self.child.lock().expect("server lock");
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Api {
    fn send(&self, request: reqwest::RequestBuilder) -> Result<(u16, String), String> {
        let request = request.basic_auth(opencode::USERNAME, Some(&self.password));
        tauri::async_runtime::block_on(async move {
            let response = request.send().await.map_err(|e| format!("opencode's server didn't answer: {e}"))?;
            let status = response.status().as_u16();
            let body = response.text().await.map_err(|e| format!("reading opencode's answer: {e}"))?;
            Ok((status, body))
        })
    }

    fn get(&self, path: &str) -> Result<(u16, String), String> {
        self.send(self.client.get(format!("{}{path}", self.url)))
    }

    /// Whether the server has the session (a 404 means it doesn't).
    pub fn session_exists(&self, id: &str) -> Result<bool, String> {
        if !opencode::is_session_id(id) {
            return Ok(false);
        }
        match self.get(&format!("/session/{id}"))? {
            (200, _) => Ok(true),
            (404, _) => Ok(false),
            (status, _) => Err(format!("opencode answered {status} when asked for its session")),
        }
    }

    pub fn create_session(&self, title: &str) -> Result<String, String> {
        let (status, body) = self.send(self.client.post(format!("{}/session", self.url)).json(&json!({ "title": title })))?;
        if status != 200 {
            return Err(format!("opencode answered {status} when asked to start a session"));
        }
        opencode::created_session_id(&body).ok_or_else(|| "opencode's new session had no id Chorus recognises".into())
    }

    /// What the server knows of `model`; `None` when its answer isn't the shape Chorus checked.
    pub fn check_model(&self, model: &str) -> Result<Option<ModelCheck>, String> {
        let (status, body) = self.get("/provider")?;
        if status != 200 {
            return Err(format!("opencode answered {status} when asked for its models"));
        }
        Ok(opencode::check_model(&body, model))
    }

    pub fn latest_user_turn(&self, session_id: &str) -> Result<Option<Observed>, String> {
        let (status, body) = self.get(&format!("/session/{session_id}/message?limit={}", opencode::READ_BACK_MESSAGES))?;
        if status != 200 {
            return Err(format!("opencode answered {status} when asked for the session's messages"));
        }
        Ok(opencode::latest_user_turn(&body))
    }
}
