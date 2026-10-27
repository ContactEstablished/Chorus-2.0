//! The `opencode serve` a run starts, and its API (the rules are in `chorus_core::opencode`). The
//! server is a hidden child in the run's job, listening on loopback behind a password that travels
//! only in the environment, and it dies with the run. `opencode --server` in the PTY is its TUI.
//! opencode 2.x serves its API under `/api/` and its web app everywhere else (LESSONS L41).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, mpsc};
use std::time::Duration;

use chorus_core::opencode::{self, ModelCheck};
use serde_json::Value;

use crate::sessions::job::Job;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// The first start in a new data folder sets that folder up.
const START_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a server whose output has closed gets to exit, so its status can be reported.
const EXIT_WAIT: Duration = Duration::from_secs(2);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// A new server lists no providers for a few seconds while it loads them (LESSONS L41).
const PROVIDERS_WAIT: Duration = Duration::from_secs(10);
/// How long a dropped event stream waits before reconnecting, and how many times it tries.
pub const RECONNECT_WAIT: Duration = Duration::from_secs(1);
pub const RECONNECT_TRIES: u32 = 30;
/// An event stream that never completes an event is cut off here.
const MAX_EVENT_BUFFER: usize = 4 * 1024 * 1024;

/// The state folder `agents::resolve` gives opencode inside a session's folder, so its TUI's tabs
/// and prompt history stay out of the user's own. (1.x's TUI also remembered a variant per model
/// there, LESSONS L5; 2.1.1 keeps the model on the session instead.)
pub fn state_home(scratch: &Path) -> PathBuf {
    scratch.join("xdg-state")
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
            Err(e) => {
                // Its output closed before the address: it is exiting, so give it a moment to.
                let mut exited = child.try_wait().ok().flatten();
                let until = std::time::Instant::now() + EXIT_WAIT;
                while exited.is_none() && e == mpsc::RecvTimeoutError::Disconnected && std::time::Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(50));
                    exited = child.try_wait().ok().flatten();
                }
                let _ = child.kill();
                return Err(match (exited, e) {
                    (Some(status), _) => format!("opencode serve exited ({status}) before it was ready"),
                    (None, mpsc::RecvTimeoutError::Disconnected) => "opencode serve closed its output before it was ready".into(),
                    (None, mpsc::RecvTimeoutError::Timeout) => format!("opencode serve wasn't ready in {} s", START_TIMEOUT.as_secs()),
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
        match self.get(&format!("/api/session/{id}"))? {
            (200, _) => Ok(true),
            (404, _) => Ok(false),
            (status, _) => Err(format!("opencode answered {status} when asked for its session")),
        }
    }

    /// A new session in `cwd`, running `model` (`provider/model`) at `variant`, the effort.
    pub fn create_session(&self, title: &str, cwd: &str, model: Option<&str>, variant: Option<&str>) -> Result<String, String> {
        let body = opencode::create_session_body(title, cwd, model, variant);
        let (status, body) = self.send(self.client.post(format!("{}/api/session", self.url)).json(&body))?;
        if status != 200 {
            return Err(format!("opencode answered {status} when asked to start a session"));
        }
        opencode::created_session_id(&body).ok_or_else(|| "opencode's new session had no id Chorus recognises".into())
    }

    /// Sets an existing session's model and effort variant, which a prompt from the TUI may have
    /// changed (`POST /api/session/{id}/model`).
    pub fn set_model(&self, id: &str, model: &str, variant: Option<&str>) -> Result<(), String> {
        let body = opencode::set_model_body(model, variant).ok_or("the model isn't provider/model")?;
        let (status, _) = self.send(self.client.post(format!("{}/api/session/{id}/model", self.url)).json(&body))?;
        match status {
            200..=299 => Ok(()),
            _ => Err(format!("opencode answered {status} when asked to set the session's model")),
        }
    }

    /// What the server knows of `model`; `None` when its answers aren't the shape Chorus checked.
    /// `keyless` is the route's provider needing no key (Ollama), which opencode lists as disabled.
    /// The second value says why keys weren't checked, when they weren't.
    pub fn check_model(&self, model: &str, keyless: bool) -> Result<(Option<ModelCheck>, Option<String>), String> {
        let deadline = std::time::Instant::now() + PROVIDERS_WAIT;
        // A keyless route's provider is Chorus's own block, so it will be listed: wait for it, since
        // the built-in providers can be listed first. Any other may never be (no key), so the first
        // non-empty list will do.
        let wanted = model.split_once('/').map(|(p, _)| p).filter(|_| keyless);
        let providers = loop {
            let (status, body) = self.get("/api/provider")?;
            if status != 200 {
                return Err(format!("opencode answered {status} when asked for its providers"));
            }
            let ready = match wanted {
                Some(p) => opencode::lists_provider(&body, p),
                None => opencode::listed(&body) > 0,
            };
            if ready || std::time::Instant::now() >= deadline {
                break body;
            }
            std::thread::sleep(Duration::from_millis(250));
        };
        let (status, models) = self.get("/api/model")?;
        if status != 200 {
            return Err(format!("opencode answered {status} when asked for its models"));
        }
        // Which providers have a key or login. Without this answer the check makes no claim about
        // keys, and still checks the provider, model and effort.
        let answer = self.get("/api/integration");
        let skipped = if keyless { None } else { opencode::key_check_skipped(&answer) };
        let integrations = match answer {
            Ok((200, body)) => body,
            _ => String::new(),
        };
        Ok((opencode::check_model(&providers, &models, &integrations, model, keyless), skipped))
    }

    /// The server's event stream (`GET /api/event`), open until the server ends.
    pub fn events(&self) -> Result<EventStream, String> {
        // No overall timeout: the stream stays open for the whole run.
        let client = reqwest::Client::builder()
            .connect_timeout(REQUEST_TIMEOUT)
            .no_proxy()
            .build()
            .map_err(|e| format!("making the HTTP client: {e}"))?;
        let request = client.get(format!("{}/api/event", self.url)).basic_auth(opencode::USERNAME, Some(&self.password));
        let response = tauri::async_runtime::block_on(request.send()).map_err(|e| format!("opencode's event stream: {e}"))?;
        if !response.status().is_success() {
            return Err(format!("opencode answered {} for its event stream", response.status().as_u16()));
        }
        Ok(EventStream { response, buffer: Vec::new() })
    }
}

/// opencode's events as they arrive.
pub struct EventStream {
    response: reqwest::Response,
    /// Bytes of an event not yet complete. Kept as bytes so a character split across chunks
    /// isn't mangled.
    buffer: Vec<u8>,
}

impl EventStream {
    /// The next complete events; `None` when the stream ends.
    pub fn next(&mut self) -> Option<Vec<Value>> {
        loop {
            let chunk = tauri::async_runtime::block_on(self.response.chunk()).ok()??;
            self.buffer.extend(chunk.iter().filter(|b| **b != b'\r'));
            let Some(end) = self.buffer.windows(2).rposition(|w| w == b"\n\n") else {
                if self.buffer.len() > MAX_EVENT_BUFFER {
                    self.buffer.clear();
                }
                continue;
            };
            let complete: Vec<u8> = self.buffer.drain(..end + 2).collect();
            let mut text = String::from_utf8_lossy(&complete).into_owned();
            let events = opencode::take_sse_events(&mut text);
            if !events.is_empty() {
                return Some(events);
            }
        }
    }
}
