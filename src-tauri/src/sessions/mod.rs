//! The session engine (P1, ARCHITECTURE §4). The back end owns every agent process; windows only
//! attach to a session id and can come and go.
//!
//! ```text
//! child ─► ConPTY ─► reader thread ─► scrubber + batcher ─► Hub ─┬─► scrollback mirror (disk)
//!                                                                └─► attached views (channels)
//! ```
//!
//! A session is a database row that outlives its processes; each launch is a run in its own Job
//! Object. Output goes through one emit path (the Hub), so the mirror and every view get the same
//! scrubbed bytes, and a view that attaches is sent the mirror first, then live output, with nothing
//! lost or repeated in between. The lifecycle rules (why a run ended, what to relaunch at boot)
//! are in `chorus_core::session`.

pub mod commands;
pub mod actions;
pub mod delivery;
pub mod composer;
pub mod images;
pub mod job;
pub mod mirror;
pub mod profiles;
pub mod workspace;
pub mod worktrees;

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use chorus_core::activity::Activity;
use chorus_core::attention::{Attention, attention};
use chorus_core::agent::{LaunchOptions, ResumeKind, uuid_v4};
use chorus_core::hooks as hk;
use chorus_core::batch::{PTY_BATCH, SCRUB_CARRY_RELEASE_MS};
use chorus_core::cli::{VersionFit, fit, parse_version};
use chorus_core::conpty::{CURSOR_REPORT, Handshake};
use chorus_core::env::{ChildEnvInput, compose_child_env, valid_secret_var};
use chorus_core::launch::{self as launch_rules, Preset, Role, Slot};
use chorus_core::opencode::{self as oc, Observed};
use chorus_core::projects::ProjectState;
use chorus_core::providers::{self as prov, ProviderKind};
use chorus_core::scrollback::{boundary, exit_line, restart_screen};
use chorus_core::scrub::{StreamScrubber, builtin_patterns};
use chorus_core::session::{ExitCause, SessionState, after_exit, classify_exit, clean_name, plan_boot};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::ipc::{InvokeResponseBody, IpcResponse};
use tauri_specta::Event;

use crate::agents::detect::Detector;
use crate::agents::opencode::{Api, Server};
use crate::agents::{self, AgentKind};
use crate::events::recorder::Recorder;
use crate::events::{HookRefused, same_token};
use crate::store::history;
use crate::store::projects as project_rows;
use crate::store::providers as provider_rows;
use crate::store::tags as tag_rows;
use crate::store::worktrees as worktree_rows;
use crate::store::sessions::{self as rows, NewRun, NewSession, SessionRow};
use crate::store::{Store, credentials};
use crate::routing;
use crate::vault;
use chorus_core::routing::launch::{REFUSE_STORED_INVALID, RoutingLaunchChoice, StoredSelection, parse_stored_selection};
use job::Job;
use mirror::Mirror;
use workspace::Workspace;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Running,
    Exited,
    Closed,
}

impl From<SessionState> for SessionStatus {
    fn from(s: SessionState) -> Self {
        match s {
            SessionState::Running => Self::Running,
            SessionState::Exited => Self::Exited,
            SessionState::Closed => Self::Closed,
        }
    }
}

/// Everything the UI shows about a session.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: u32,
    pub name: String,
    pub agent: AgentKind,
    /// The agent reports turns to Chorus; without it a running session is only "Running" (D47).
    pub has_event_channel: bool,
    pub cwd: String,
    pub status: SessionStatus,
    /// A process is running now. `status` can be `running` without one for a moment while a
    /// Restart or a relaunch is under way.
    pub live: bool,
    pub pid: Option<u32>,
    pub locked: bool,
    /// Why the session isn't running, or what went wrong, in words to show as is.
    pub note: Option<String>,
    /// The user's note on what the session is for.
    pub description: Option<String>,
    pub credential_label: Option<String>,
    pub credential_env: Option<String>,
    pub last_exit_code: Option<u32>,
    /// How the last run ended (`exited`, `closed`, `restarted`, `chorus-closed`,
    /// `chorus-crashed`, `system-shutdown`), or none while it runs.
    pub last_end: Option<String>,
    pub created_at_secs: u32,
    pub run_started_at_secs: Option<u32>,
    pub scrollback_error: Option<String>,
    /// What the session asked its agent for, with the declared defaults filled in.
    pub options: LaunchOptions,
    /// The agent's own conversation id, once Chorus has one.
    pub agent_session_id: Option<String>,
    /// The CLI version detected for the last run, and how it compares with the version Chorus
    /// checked the agent's options against.
    pub cli_version: Option<String>,
    pub version_fit: Option<VersionFit>,
    /// The last run continued the agent's conversation rather than starting one.
    pub resumed: bool,
    /// What the agent is doing, from its events (ARCHITECTURE §6); `unknown` without a channel.
    pub activity: Activity,
    /// Working, but neither an event nor output for a while: shown, never assumed (§6).
    pub no_signal: bool,
    /// Whether it needs you, is busy or is finished, in the design's words (`chorus_core::attention`).
    pub attention: Attention,
    /// What the running agent's newest turn really used, read back from the agent (opencode).
    pub observed: Option<Observed>,
    /// How that turn differs from `options`, one phrase each; empty when it matches.
    pub observed_differs: Vec<String>,
    /// The provider the agent runs on, if not its own login or key.
    pub provider_id: Option<u32>,
    pub provider_name: Option<String>,
    /// The newest run's minted OpenRouter key, if it had one.
    pub capped_key: Option<CappedKeyInfo>,
    pub project_id: Option<u32>,
    /// Free-form labels beside the project (D52), in the order given.
    pub tags: Vec<String>,
    /// The worktree New session made for it (D53), if any.
    pub worktree: Option<workspace::WorktreeInfo>,
    /// The routing tier it runs with on OpenRouter, in words (D57).
    pub routing: Option<String>,
    /// The newest turn's final message, else its prompt, already scrubbed; shown on cards.
    pub latest: Option<String>,
    /// When that turn started, or 0.
    pub latest_at_secs: u32,
}

/// A key minted for one run (`providers::keys`). The key itself is never shown or kept.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CappedKeyInfo {
    pub limit_usd: f64,
    /// Not revoked yet: the run is live, or revoking failed (it expires within a day).
    pub live: bool,
    /// What OpenRouter's analytics settled on, once known.
    pub cost_usd: Option<f64>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct SessionUpdated {
    pub session: SessionSummary,
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateSession {
    pub agent: AgentKind,
    pub cwd: String,
    pub name: Option<String>,
    /// A stored key to inject as an environment variable, decrypted only at launch.
    pub credential: Option<CredentialUse>,
    pub cols: u16,
    pub rows: u16,
    /// Checked against what the agent declares; anything it lacks is refused with a reason.
    pub options: LaunchOptions,
    /// A provider to run the agent on (`providers_list`); `options.model` is then that
    /// provider's model id.
    #[serde(default)]
    pub provider_id: Option<u32>,
    /// What the session is for, shown on its card and header.
    #[serde(default)]
    pub description: Option<String>,
    /// The project it belongs to; left out, the project whose folder holds `cwd`, if any.
    #[serde(default)]
    pub project_id: Option<u32>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// In `cwd`, or in a new worktree of its repository.
    #[serde(default)]
    pub workspace: workspace::Workspace,
    /// opencode on OpenRouter: a routing tier, or `default` for none (D57). Absent elsewhere.
    #[serde(default)]
    pub routing: Option<RoutingLaunchChoice>,
}

/// Starts the sessions a preset makes. `session` is the first; the others are derived from it.
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LaunchRequest {
    pub preset: Preset,
    pub session: CreateSession,
    /// How many agents a Swarm starts.
    #[serde(default)]
    pub count: Option<u8>,
}

/// One session a launch tried to start.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LaunchOutcome {
    pub agent: AgentKind,
    pub role: Option<Role>,
    pub session: Option<SessionSummary>,
    pub error: Option<String>,
}

/// A preset as the launch form shows it for one agent.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PresetChoice {
    pub preset: Preset,
    pub label: String,
    pub blurb: String,
    pub note: Option<String>,
    /// The sessions it would start; empty when it can't.
    pub slots: Vec<Slot>,
    /// Why it can't, for this agent on this machine.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CredentialUse {
    pub id: u32,
    pub env_var: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CloseReport {
    pub killed: u32,
    /// Windowed processes (a browser the agent opened) left running.
    pub spared: u32,
    /// What happened to the session's worktree, when it had one and its removal was asked for.
    pub worktree_note: Option<String>,
}

const MAX_WRITE_BYTES: usize = 1 << 20;
const MAX_CWD_BYTES: usize = 1024;
/// How long Close and Restart wait for a run's output to drain before moving on.
const FINISH_TIMEOUT: Duration = Duration::from_secs(5);
/// How long they then wait for the run's ended processes to be gone, so a worktree they worked in
/// can be removed.
const GONE_TIMEOUT: Duration = Duration::from_secs(3);
const BOUNDARY_RESTARTED: &str = "── Restarted ──";
/// How often an open Claude Code turn's transcript is checked for an interrupt.
const TRANSCRIPT_POLL: Duration = Duration::from_secs(2);
/// The most of a transcript read in one check.
const MAX_TRANSCRIPT_READ: u64 = 4 * 1024 * 1024;
/// The most turns one history request returns.
const MAX_HISTORY_TURNS: u32 = 500;
/// How often "no signal" is checked.
const SIGNAL_CHECK: Duration = Duration::from_secs(10);
/// How long raw agent events are kept for diagnosis.
const EVENT_LOG_KEEP_MS: i64 = 14 * 24 * 60 * 60 * 1000;
/// A Claude Code run that exits within this of starting, in a folder it doesn't trust, ended at its
/// trust question.
const TRUST_EXIT_WINDOW: Duration = Duration::from_secs(300);
/// How long quitting waits for minted keys to be revoked.
const SHUTDOWN_REVOKE_WAIT: Duration = Duration::from_secs(4);
const BOUNDARY_RELAUNCHED: &str = "── Chorus restarted: session relaunched ──";

fn check_size(cols: u16, rows: u16) -> Result<PtySize, String> {
    if !(2..=1000).contains(&cols) || !(2..=1000).contains(&rows) {
        return Err(format!("terminal size {cols}x{rows} is out of range"));
    }
    Ok(PtySize { cols, rows, pixel_width: 0, pixel_height: 0 })
}

/// A fresh secret for a run (opencode's server password, a hook token): 24 random bytes as hex.
fn random_token() -> String {
    vault::random_bytes(24).iter().map(|b| format!("{b:02x}")).collect()
}

/// The complete lines between `from` and `to` in a file, and how many bytes they took (a line
/// still being written is left for next time).
fn read_new_lines(path: &str, from: u64, to: u64) -> Option<(Vec<String>, u64)> {
    use std::io::{Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    f.seek(SeekFrom::Start(from)).ok()?;
    let mut buf = Vec::new();
    f.take((to - from).min(MAX_TRANSCRIPT_READ)).read_to_end(&mut buf).ok()?;
    let end = buf.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    let lines = String::from_utf8_lossy(&buf[..end]).lines().map(str::to_owned).collect();
    Some((lines, end as u64))
}

fn version_note(kind: AgentKind, d: &agents::detect::Detection) -> Option<String> {
    chorus_core::cli::version_note(kind.display_name(), d.version.as_deref(), kind.spec().verified, d.fit?)
}

/// A folder for a new session: a full path to a folder that exists, with any short (8.3) names
/// written in full.
fn check_cwd(input: &str) -> Result<String, String> {
    let s = input.trim().trim_matches('"').trim();
    if s.is_empty() || s.len() > MAX_CWD_BYTES {
        return Err("Choose a folder for the session.".into());
    }
    let path = Path::new(s);
    if !path.is_absolute() {
        return Err(format!("{s} is not a full path (for example C:\\Projects\\app)."));
    }
    if !path.is_dir() {
        return Err(format!("{s} is not a folder that exists."));
    }
    Ok(long_path(s))
}

/// `path` with its short (8.3) names written in full (`C:\Users\RUNNER~1` → `C:\Users\runneradmin`),
/// as git writes it, so one folder compares equal to git's view of it (LESSONS L43); `path` itself
/// when Windows can't expand it.
fn long_path(path: &str) -> String {
    use windows::Win32::Storage::FileSystem::GetLongPathNameW;
    let short = windows::core::HSTRING::from(path);
    let mut buf = vec![0u16; MAX_CWD_BYTES];
    // SAFETY: `short` is a valid wide string and `buf` a writable buffer whose length is passed.
    let n = unsafe { GetLongPathNameW(&short, Some(&mut buf)) } as usize;
    let long = (n > 0 && n < buf.len()).then(|| String::from_utf16_lossy(&buf[..n]));
    long.filter(|l| l.len() <= MAX_CWD_BYTES).unwrap_or_else(|| path.to_owned())
}

fn system_shutting_down() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SHUTTINGDOWN};
    // SAFETY: a plain query with no pointers.
    unsafe { GetSystemMetrics(SM_SHUTTINGDOWN) != 0 }
}

/// Where a view receives bytes. Returns false once the view is gone.
pub type Sink = Box<dyn Fn(&[u8]) -> bool + Send>;

/// One session's single emit path: the mirror and the attached views.
struct Hub {
    inner: Mutex<HubInner>,
}

struct HubInner {
    mirror: Option<Mirror>,
    views: Vec<(u32, Sink)>,
    next_view: u32,
}

impl Hub {
    fn new(mirror: Mirror) -> Self {
        Self { inner: Mutex::new(HubInner { mirror: Some(mirror), views: Vec::new(), next_view: 0 }) }
    }

    fn emit(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let mut g = self.inner.lock().expect("hub lock");
        // Persist before broadcasting: a failing view must never cost the history.
        if let Some(m) = g.mirror.as_mut() {
            m.append(bytes);
        }
        g.views.retain(|(_, sink)| sink(bytes));
    }

    /// Sends the history, then registers the view, under one lock, so live output can't slip in
    /// between or arrive twice.
    fn attach(&self, sink: Sink) -> u32 {
        let mut g = self.inner.lock().expect("hub lock");
        let history = g.mirror.as_mut().map(Mirror::read_all).unwrap_or_default();
        if !history.is_empty() {
            sink(&history);
        }
        g.next_view += 1;
        let id = g.next_view;
        g.views.push((id, sink));
        id
    }

    fn detach(&self, view: u32) {
        self.inner.lock().expect("hub lock").views.retain(|(id, _)| *id != view);
    }

    /// Marks the start of a new run: rewrites the mirror's history and shows attached views the
    /// same screen: the old one and the dividing line pushed into scrollback.
    fn new_run(&self, boundary_text: &str) {
        let mut g = self.inner.lock().expect("hub lock");
        if let Some(m) = g.mirror.as_mut() {
            m.begin_new_run(boundary_text);
        }
        let shown = restart_screen(boundary_text);
        g.views.retain(|(_, sink)| sink(&shown));
    }

    fn scrollback_error(&self) -> Option<String> {
        self.inner.lock().expect("hub lock").mirror.as_ref().and_then(|m| m.error.clone())
    }

    fn remove(&self) {
        let mut g = self.inner.lock().expect("hub lock");
        g.views.clear();
        if let Some(m) = g.mirror.take() {
            m.remove();
        }
    }
}

/// One launch of a session's process.
struct Run {
    id: u32,
    pid: u32,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    job: Job,
    /// Set before Chorus itself ends the process (Close, Restart), so the exit isn't taken for the
    /// agent quitting.
    requested: Mutex<Option<ExitCause>>,
    started: Instant,
    /// Filled by the exit watcher for the output thread, which writes the exit line last.
    exit: Mutex<Option<(ExitCause, u32)>>,
    finished: (Mutex<bool>, Condvar),
    /// opencode's server, which ends with the run.
    server: Option<Server>,
    codex_bridge: Option<Arc<codex_bridge::Bridge>>,
    /// The ledger row of the OpenRouter key minted for this run, revoked when it ends.
    capped: Option<u32>,
    /// The options as this run's agent took them (a provider's model in the agent's own form).
    asked: LaunchOptions,
    /// What the newest turn ran with, read back from the agent.
    observed: Mutex<Option<Observed>>,
    /// The token this run's hooks must present; only the agent's environment holds it.
    hook_token: Option<String>,
    /// This run's events, turned into state and history.
    recorder: Mutex<Recorder>,
    /// When the run last printed anything (Unix ms): output keeps a quiet turn from looking dead.
    last_output_ms: AtomicI64,
    /// Whether "no signal" was last reported, so a change is sent once.
    no_signal_shown: AtomicBool,
}

impl Run {
    fn end_processes(&self) -> job::CloseOutcome {
        match self.job.close_sparing_windows(self.pid, 1) {
            Ok(outcome) => outcome,
            Err(_) => {
                let killed = self.job.process_ids().unwrap_or_default();
                let _ = self.job.terminate(1);
                job::CloseOutcome { killed, spared: Vec::new() }
            }
        }
    }

    fn mark_finished(&self) {
        let (done, cv) = &self.finished;
        *done.lock().expect("finished lock") = true;
        cv.notify_all();
    }

    fn wait_finished(&self, timeout: Duration) -> bool {
        let (done, cv) = &self.finished;
        let guard = done.lock().expect("finished lock");
        let (guard, _) = cv.wait_timeout_while(guard, timeout, |d| !*d).expect("finished wait");
        *guard
    }
}

/// A session that is open in this app run (not closed).
struct Live {
    id: u32,
    action: Mutex<()>,
    hub: Hub,
    run: Mutex<Option<Arc<Run>>>,
}

impl Live {
    fn current_run(&self) -> Option<Arc<Run>> {
        self.run.lock().expect("run lock").clone()
    }
}

pub type Notify = Box<dyn Fn(SessionSummary) + Send + Sync>;

pub struct SessionManager {
    store: Store,
    data_dir: PathBuf,
    live: Mutex<HashMap<u32, Arc<Live>>>,
    worktree_lock: Mutex<()>,
    quitting: AtomicBool,
    notify: Notify,
    agents: Detector,
    /// The event listener's port, or why it isn't running.
    hook_listener: OnceLock<Result<u16, String>>,
    /// How far the restore at start has got, for the splash.
    restore: Mutex<RestoreProgress>,
}

/// The sessions Chorus relaunches at start, and how many are back (each started or failed).
#[derive(Debug, Clone, Default, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RestoreProgress {
    pub total: u32,
    pub restored: u32,
    /// Each relaunching session's project colour, or none for the theme's accent.
    pub colours: Vec<Option<String>>,
}

impl SessionManager {
    pub fn new(store: Store, data_dir: PathBuf, notify: Notify) -> Arc<Self> {
        Arc::new(Self {
            store,
            data_dir,
            live: Mutex::new(HashMap::new()),
            worktree_lock: Mutex::new(()),
            quitting: AtomicBool::new(false),
            notify,
            agents: Detector::default(),
            hook_listener: OnceLock::new(),
            restore: Mutex::new(RestoreProgress::default()),
        })
    }

    pub fn restore_progress(&self) -> RestoreProgress {
        self.restore.lock().expect("restore lock").clone()
    }

    /// Where hooks post to (`events::listener`); launches after this get hooks.
    pub fn set_hook_listener(&self, listener: Result<u16, String>) {
        let _ = self.hook_listener.set(listener);
    }

    /// Claude Code's hook settings for a session, in Chorus's data (never the repo, LESSONS L1).
    /// It holds the address only; the token is in the child's environment.
    fn write_claude_settings(&self, session_id: u32, url: &str) -> Result<PathBuf, String> {
        let dir = self.data_dir.join("hooks");
        std::fs::create_dir_all(&dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        let path = dir.join(format!("claude-session-{session_id}.json"));
        std::fs::write(&path, hk::claude_settings(url)).map_err(|e| format!("writing {}: {e}", path.display()))?;
        Ok(path)
    }

    /// A hook post from a session's agent: checked against the run's token, then recorded before
    /// the reply goes back.
    pub fn receive_hook(&self, session_id: u32, token: &str, body: &[u8]) -> Result<Option<crate::events::replies::HookReceiver>, HookRefused> {
        let live = self.open_live(session_id).map_err(|_| HookRefused::NotFound)?;
        let run = live.current_run().ok_or(HookRefused::NotFound)?;
        let expected = run.hook_token.as_deref().ok_or(HookRefused::NotFound)?;
        if !same_token(expected, token) {
            return Err(HookRefused::Unauthorized);
        }
        let payload: serde_json::Value = serde_json::from_slice(body).map_err(|_| HookRefused::BadBody)?;
        let agent = self.row(session_id).map_err(|_| HookRefused::NotFound)?.agent;
        let (source, events) = match agent {
            AgentKind::Claude => ("claude-hook", chorus_core::events::from_claude_hook(&payload)),
            AgentKind::Codex => ("codex-hook", chorus_core::events::from_codex_hook(&payload)),
            _ => return Err(HookRefused::NotFound),
        };
        let (before, reply) = {
            let mut recorder = run.recorder.lock().expect("recorder lock");
            if run.requested.lock().expect("requested lock").is_some() || run.exit.lock().expect("exit lock").is_some() { return Err(HookRefused::NotFound); }
            let before = recorder.activity();
            let last_ask = recorder.newest_ask();
            recorder.record(&self.store, source, &payload, &events, crate::store::now_ms());
            let reply = if agent == AgentKind::Claude && last_ask != recorder.newest_ask() {
                recorder.hold_reply(&self.store, &payload)
            } else { None };
            (before != recorder.activity(), reply)
        };
        if before {
            self.notify_id(session_id);
        }
        Ok(reply)
    }

    /// A session's newest turns, oldest first, and how complete its records are.
    pub fn history(&self, id: u32, limit: u32) -> Result<history::SessionHistory, String> {
        self.row(id)?;
        let (total, ended, with_prompt) = history::turn_counts(&self.store, id)?;
        Ok(history::SessionHistory {
            turns: history::recent_turns(&self.store, id, limit.clamp(1, MAX_HISTORY_TURNS))?,
            total,
            ended,
            with_prompt,
            transcript_path: history::session_transcript(&self.store, id)?,
        })
    }

    /// Every few seconds: tells the front end when a session starts or stops showing "no signal".
    pub fn watch_signals(self: &Arc<Self>) {
        let m = self.clone();
        let _ = std::thread::Builder::new().name("signal-watch".into()).spawn(move || {
            loop {
                std::thread::sleep(SIGNAL_CHECK);
                let runs: Vec<(u32, Arc<Run>)> = {
                    let live = m.live.lock().expect("live lock");
                    live.values().filter_map(|l| l.current_run().map(|r| (l.id, r))).collect()
                };
                for (id, run) in runs {
                    let stale = run.recorder.lock().expect("recorder lock").stale(crate::store::now_ms(), run.last_output_ms.load(Ordering::Relaxed));
                    if run.no_signal_shown.swap(stale, Ordering::SeqCst) != stale {
                        m.notify_id(id);
                    }
                }
            }
        });
    }

    pub fn is_quitting(&self) -> bool { self.quitting.load(Ordering::SeqCst) }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn agents(&self) -> &Detector {
        &self.agents
    }

    /// Detects every agent CLI again. Runs started before the first detection get its version
    /// filled in.
    pub fn refresh_agents(&self) -> Vec<(AgentKind, agents::detect::Detection)> {
        let parent: Vec<(String, String)> = std::env::vars().collect();
        let found = self.agents.refresh(&parent, &self.data_dir.join("detect"));
        for (kind, d) in &found {
            if let Some(v) = &d.version {
                let _ = rows::fill_cli_version(&self.store, *kind, v);
            }
        }
        found
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    fn scrollback_dir(&self) -> PathBuf {
        self.data_dir.join("scrollback")
    }

    /// Per-session folder for agent state that must be isolated (opencode, LESSONS L5).
    fn state_dir(&self, id: u32) -> PathBuf {
        self.data_dir.join("state").join(id.to_string())
    }

    fn live_for(&self, id: u32) -> Arc<Live> {
        let mut live = self.live.lock().expect("live lock");
        live.entry(id)
            .or_insert_with(|| {
                Arc::new(Live { id, action: Mutex::new(()), hub: Hub::new(Mirror::open(&self.scrollback_dir(), id)), run: Mutex::new(None) })
            })
            .clone()
    }

    fn open_live(&self, id: u32) -> Result<Arc<Live>, String> {
        self.live.lock().expect("live lock").get(&id).cloned().ok_or_else(|| format!("session {id} is not open"))
    }

    fn running(&self, id: u32) -> Result<Arc<Run>, String> {
        self.open_live(id)?.current_run().ok_or_else(|| "the session isn't running".into())
    }

    fn row(&self, id: u32) -> Result<SessionRow, String> {
        rows::get(&self.store, id)?.ok_or_else(|| format!("no session {id}"))
    }

    fn summary_of(&self, row: &SessionRow) -> SessionSummary {
        let live = self.live.lock().expect("live lock").get(&row.id).cloned();
        let run = live.as_ref().and_then(|l| l.current_run());
        let observed = run.as_ref().and_then(|r| r.observed.lock().expect("observed lock").clone());
        let last = rows::last_run(&self.store, row.id).ok().flatten();
        let credential_label =
            row.credential_id.and_then(|id| credentials::get(&self.store, id).ok().flatten()).map(|c| c.label);
        let latest = history::latest(&self.store, row.id).ok().flatten();
        SessionSummary {
            id: row.id,
            name: row.name.clone(),
            agent: row.agent,
            has_event_channel: row.agent.spec().has_event_channel,
            cwd: row.cwd.clone(),
            status: row.state.into(),
            live: run.is_some(),
            pid: run.as_ref().map(|r| r.pid),
            locked: row.locked,
            note: row.note.clone(),
            description: row.description.clone(),
            credential_label,
            credential_env: row.credential_env.clone(),
            last_exit_code: last.as_ref().and_then(|r| r.exit_code),
            last_end: last.as_ref().and_then(|r| r.end_reason).map(|e| e.as_str().to_owned()),
            created_at_secs: vault::commands::secs(row.created_at),
            run_started_at_secs: last.as_ref().filter(|r| r.ended_at.is_none()).map(|r| vault::commands::secs(r.started_at)),
            scrollback_error: live.as_ref().and_then(|l| l.hub.scrollback_error()),
            options: row.options.clone(),
            agent_session_id: row.agent_session_id.clone(),
            version_fit: last.as_ref().and_then(|r| r.cli_version.as_deref()).map(|v| {
                fit(parse_version(v), row.agent.spec().verified.and_then(parse_version))
            }),
            cli_version: last.as_ref().and_then(|r| r.cli_version.clone()),
            resumed: last.as_ref().is_some_and(|r| r.resumed),
            activity: run.as_ref().map_or(Activity::Unknown, |r| r.recorder.lock().expect("recorder lock").activity()),
            no_signal: run.as_ref().is_some_and(|r| r.no_signal_shown.load(Ordering::SeqCst)),
            attention: attention(
                row.state,
                run.is_some(),
                run.as_ref().map_or(Activity::Unknown, |r| r.recorder.lock().expect("recorder lock").activity()),
                last.as_ref().and_then(|r| r.end_reason),
                last.as_ref().and_then(|r| r.exit_code),
            ),
            observed_differs: match (&observed, &run) {
                (Some(o), Some(r)) => oc::differences(row.agent.spec(), &r.asked, o),
                _ => Vec::new(),
            },
            provider_id: row.provider_id,
            provider_name: row.provider_id.map(|id| {
                provider_rows::get(&self.store, id).ok().flatten().map_or_else(|| "(deleted provider)".to_owned(), |p| p.name)
            }),
            capped_key: provider_rows::last_capped_key(&self.store, row.id).ok().flatten().map(|k| CappedKeyInfo {
                limit_usd: k.limit_usd,
                live: k.revoked_at.is_none() && k.hash.is_some(),
                cost_usd: k.cost_usd,
                note: k.cost_note,
            }),
            observed,
            project_id: row.project_id,
            tags: tag_rows::get(&self.store, row.id).unwrap_or_default(),
            worktree: row.worktree_id.and_then(|w| self.worktree_info(w)),
            routing: routing::describe(row.routing.as_deref()),
            latest_at_secs: latest.as_ref().map_or(0, |(_, at)| vault::commands::secs(*at)),
            latest: latest.map(|(text, _)| text),
        }
    }

    pub fn summary(&self, id: u32) -> Result<SessionSummary, String> {
        Ok(self.summary_of(&self.row(id)?))
    }

    fn notify_id(&self, id: u32) {
        if let Ok(s) = self.summary(id) {
            (self.notify)(s);
        }
    }

    /// Sends every open session's summary to the front end again.
    pub fn notify_all(&self) {
        for s in self.list().unwrap_or_default() {
            (self.notify)(s);
        }
    }

    pub fn list(&self) -> Result<Vec<SessionSummary>, String> {
        Ok(rows::list_open(&self.store)?.iter().map(|r| self.summary_of(r)).collect())
    }

    /// At start-up: close out runs a previous Chorus left open, then relaunch every session that
    /// should be running. The app does the two halves on different threads.
    #[cfg(test)]
    pub fn restore(self: &Arc<Self>) -> Result<(), String> {
        let relaunch = self.reopen()?;
        self.relaunch(&relaunch);
        Ok(())
    }

    /// The first half of [`Self::restore`]: heals the runs a previous Chorus left open and opens
    /// every session, so views can attach. Returns the sessions to relaunch.
    pub fn reopen(&self) -> Result<Vec<u32>, String> {
        let open = rows::list_open(&self.store)?;
        let plan = plan_boot(&open.iter().map(|r| (r.id, r.state)).collect::<Vec<_>>(), &rows::open_runs(&self.store)?);
        for run in plan.heal {
            rows::end_run(&self.store, run, chorus_core::session::RunEnd::ChorusCrashed, None)?;
        }
        mirror::prune(&self.scrollback_dir(), &open.iter().map(|r| r.id).collect::<Vec<_>>());
        for row in &open {
            self.live_for(row.id);
            // Turns the previous Chorus left open ended with it.
            let _ = history::close_open_turns(&self.store, row.id);
        }
        let _ = history::prune_events(&self.store, EVENT_LOG_KEEP_MS);
        let relaunch: Vec<&SessionRow> = open.iter().filter(|r| plan.relaunch.contains(&r.id)).collect();
        let colours = relaunch
            .iter()
            .map(|r| r.project_id.and_then(|p| project_rows::get(&self.store, p).ok().flatten()).map(|p| p.colour))
            .collect();
        *self.restore.lock().expect("restore lock") = RestoreProgress { total: relaunch.len() as u32, restored: 0, colours };
        Ok(relaunch.iter().map(|r| r.id).collect())
    }

    /// The second half: launches each session again. It can take seconds (an opencode session
    /// waits for its server), so the app calls it off the setup thread.
    pub fn relaunch(self: &Arc<Self>, ids: &[u32]) {
        // Off the setup thread, before any session is relaunched. Each start also validates its
        // worktree, so a repository Git couldn't read cannot silently fall back to another cwd.
        if let Err(e) = self.reconcile_worktrees() { eprintln!("chorus: worktree reconciliation: {e}"); }
        for id in ids {
            let live = self.live_for(*id);
            live.hub.new_run(BOUNDARY_RELAUNCHED);
            if let Err(e) = self.start_run(&live) {
                self.record_failure(&live, &e);
            }
            self.restore.lock().expect("restore lock").restored += 1;
        }
    }

    /// Checks a session's settings (also a launch profile's): options the agent declares, a key
    /// that exists under a usable variable name, and a provider the agent can run on with a model.
    pub fn check_settings(
        &self,
        agent: AgentKind,
        options: &LaunchOptions,
        provider_id: Option<u32>,
        credential: Option<&CredentialUse>,
    ) -> Result<LaunchOptions, String> {
        if let Some(c) = credential {
            if !valid_secret_var(&c.env_var) {
                return Err(format!("{:?} can't be used as the key's variable name.", c.env_var));
            }
            credentials::get(&self.store, c.id)?.ok_or("That key no longer exists.")?;
        }
        let options = agent.spec().resolve_on(options, provider_id.is_some())?;
        if let Some(pid) = provider_id {
            let provider = provider_rows::get(&self.store, pid)?.ok_or("That provider no longer exists.")?;
            prov::route_support(agent, provider.kind)?;
            if options.model.is_none() {
                return Err(format!("Choose a model for {}.", provider.name));
            }
        }
        Ok(options)
    }

    pub fn create(self: &Arc<Self>, req: CreateSession) -> Result<SessionSummary, String> {
        self.create_in(req, None)
    }

    /// Creates a session. `join` is a worktree an earlier session of the same launch made, which
    /// this one works in too (its `cwd` is already inside it). Everything is checked before a new
    /// worktree is made, so a refused request leaves nothing behind.
    fn create_in(self: &Arc<Self>, req: CreateSession, join: Option<u32>) -> Result<SessionSummary, String> {
        check_size(req.cols, req.rows)?;
        let asked = check_cwd(&req.cwd)?;
        let options = self.check_settings(req.agent, &req.options, req.provider_id, req.credential.as_ref())?;
        let description = launch_rules::clean_description(req.description.as_deref())?;
        let tags = chorus_core::tags::clean_tags(&req.tags)?;
        let routing = routing::for_launch(&self.store, req.agent, req.provider_id, req.routing, &options)?;
        let spec = req.agent.spec();
        // Chorus names the conversation up front where the agent lets it, so every later run can
        // resume it.
        let conversation =
            (spec.resume.kind() == ResumeKind::Assigned).then(|| uuid_v4(vault::random_bytes(16).try_into().expect("16 bytes")));
        let fallback = format!("{} {}", req.agent.display_name(), rows::count_by_agent(&self.store, req.agent)? + 1);
        let name = clean_name(req.name.as_deref(), &fallback)?;
        let project_id = self.project_for(req.project_id, &asked)?;
        let workspace_guard = self.worktree_lock.lock().expect("worktree lock");
        let reused = match (join, req.workspace) {
            (None, Workspace::ExistingWorktree(id)) => Some((id, self.reuse_worktree(id, &asked)?)),
            (Some(id), _) => { self.validate_worktree_cwd(id, &asked)?; Some((id, asked.clone())) }
            (None, Workspace::Here) => self.managed_worktree_at(&asked)?.map(|id| (id, asked.clone())),
            _ => None,
        };
        let made = match (join, req.workspace) {
            (None, Workspace::NewWorktree) => Some(self.make_worktree(&asked)?),
            _ => None,
        };
        let cwd = reused.as_ref().map(|(_, cwd)| cwd.clone()).or_else(|| made.as_ref().map(|m| m.cwd.clone())).unwrap_or_else(|| asked.clone());
        let worktree_id = reused.as_ref().map(|(id, _)| *id).or(made.as_ref().map(|m| m.id));
        let inserted = rows::insert(
            &self.store,
            &NewSession {
                name: &name,
                agent: req.agent,
                cwd: &cwd,
                credential_id: req.credential.as_ref().map(|c| c.id),
                credential_env: req.credential.as_ref().map(|c| c.env_var.as_str()),
                cols: req.cols,
                rows: req.rows,
                options: &options,
                agent_session_id: conversation.as_deref(),
                provider_id: req.provider_id,
                description: description.as_deref(),
                project_id,
                worktree_id,
                routing: routing.as_deref(),
            },
        );
        let id = match inserted {
            Ok(id) => id,
            Err(e) => {
                if let Some(m) = &made {
                    self.forget_worktree(m);
                }
                return Err(e);
            }
        };
        drop(workspace_guard);
        tag_rows::set(&self.store, id, &tags)?;
        if join.is_none() {
            self.remember_folder(&asked);
        }
        if req.agent == AgentKind::Claude
            && let Some(w) = worktree_id
        {
            self.trust_worktree_for_claude(w, &asked);
        }
        if let (Some(choice), Some(model)) = (req.routing, options.model.as_deref())
            && req.agent == AgentKind::Opencode
            && req.provider_id.is_some()
        {
            routing::remember(&self.store, model, choice);
        }
        let live = self.live_for(id);
        if let Err(e) = self.start_run(&live) {
            self.record_failure(&live, &e);
        }
        let summary = self.summary(id)?;
        (self.notify)(summary.clone());
        Ok(summary)
    }

    /// A launch that failed is shown, not hidden: the session stays, exited, with the reason in its
    /// note and in its terminal.
    fn record_failure(&self, live: &Live, error: &str) {
        let _ = rows::set_state(&self.store, live.id, SessionState::Exited, Some(&format!("Couldn't start: {error}")));
        live.hub.emit(&boundary(&format!("── Couldn't start: {error} ──")));
        self.notify_id(live.id);
    }

    /// Launches a run. It starts at the top-left of a blank screen: a new session's, or the one
    /// `restart_screen` left.
    fn start_run(self: &Arc<Self>, live: &Arc<Live>) -> Result<(), String> {
        let row = self.row(live.id)?;
        let size = check_size(row.cols, row.rows)?;
        if let Some(w) = row.worktree_id { self.validate_worktree_cwd(w, &row.cwd)?; }
        if !Path::new(&row.cwd).is_dir() {
            return Err(format!("the folder {} no longer exists", row.cwd));
        }
        let state_dir = self.state_dir(row.id);
        std::fs::create_dir_all(&state_dir).map_err(|e| format!("creating {}: {e}", state_dir.display()))?;

        let parent: Vec<(String, String)> = std::env::vars().collect();
        let mut launch = agents::resolve(row.agent, &parent, &state_dir)?;
        if row.agent == AgentKind::Claude {
            launch.additions.push(("CLAUDE_CODE_ENABLE_TODO_TOOLS", "1".into()));
        }
        if row.agent == AgentKind::Codex {
            // Codex writes this into its rollout, which is how the session finds its conversation.
            launch.additions.push(("CODEX_INTERNAL_ORIGINATOR_OVERRIDE", chorus_core::codex::originator_for(row.id)));
        }
        let mut secrets: Vec<(String, String)> = Vec::new();
        match (row.credential_id, &row.credential_env) {
            (Some(cid), Some(var)) => secrets.push((var.clone(), vault::decrypt(&self.store, cid)?)),
            (None, Some(_)) => return Err("the key this session uses has been deleted".into()),
            _ => {}
        }
        let spec = row.agent.spec();
        let mut notes: Vec<String> = Vec::new();
        if row.agent == AgentKind::Aider && !state_dir.join("aider.chat.history.md").exists()
            && Path::new(&row.cwd).join(".aider.chat.history.md").is_file()
        {
            notes.push("Aider uses this session's own history. The repository's previous chat is kept in .aider.chat.history.md and isn't imported.".into());
        }
        if row.agent == AgentKind::Gemini {
            let repository = row.worktree_id.and_then(|id| worktree_rows::get(&self.store, id).ok().flatten())
                .filter(|w| !w.removed && chorus_core::folders::inside(&row.cwd, &w.path))
                .filter(|w| crate::git::facts(Path::new(&row.cwd)).ok().flatten()
                    .is_some_and(|f| chorus_core::folders::same(&f.main_root, &w.repo_root)))
                .map(|w| w.repo_root);
            match agents::gemini::launch_trusted(&parent, &row.cwd, repository.as_deref()) {
                Ok(true) => launch.args.push("--skip-trust".into()),
                Ok(false) => {}
                Err(note) => notes.push(note),
            }
        }
        // A provider: its key in the environment (a key minted for this run where it can be), and
        // the model, arguments or opencode block its route needs. A minted key is revoked if the
        // launch fails from here on.
        let mut options = row.options.clone();
        let mut route_args: Vec<String> = Vec::new();
        let mut opencode_provider: Option<serde_json::Value> = None;
        let mut capped = CappedGuard { manager: self.clone(), ledger: None };
        if let Some(provider_id) = row.provider_id {
            let provider = provider_rows::get(&self.store, provider_id)?.ok_or("the provider this session runs on was deleted")?;
            let model = row.options.model.clone().ok_or("this session has a provider but no model")?;
            let catalog = provider_rows::catalog_model(&self.store, provider_id, &model).ok().flatten();
            let mut route = prov::route(row.agent, provider.kind, &provider.name, provider.base_url.as_deref().unwrap_or(""), &model, catalog.as_ref())?;
            // A routing tier (D57): the selection stored at launch, never ranked again.
            match parse_stored_selection(row.routing.as_deref()) {
                StoredSelection::None => {}
                StoredSelection::Invalid => return Err(REFUSE_STORED_INVALID.into()),
                StoredSelection::Selection(s) => {
                    let effort = oc::wanted_variant(spec, &row.options);
                    prov::apply_routing(&mut route, &model, &s, catalog.as_ref().map(|c| c.efforts.as_slice()), effort.as_deref());
                }
            }
            // A provider that takes no key (Ollama) gets none, and nothing is minted for it.
            if let Some(key_var) = route.key_var {
                let own_key = || -> Result<String, String> {
                    let cid = provider.credential_id.ok_or_else(|| format!("{} has no key", provider.name))?;
                    vault::decrypt(&self.store, cid)
                };
                let key = if provider.kind == ProviderKind::Openrouter && provider.management_credential_id.is_some() {
                    match crate::providers::keys::mint_for_run(&self.store, &provider, row.id) {
                        Ok(minted) => {
                            capped.ledger = Some(minted.ledger_id);
                            notes.push(format!("This run has its own OpenRouter key, capped at ${:.2}.", minted.limit_usd));
                            minted.key
                        }
                        Err(e) => {
                            let key = own_key().map_err(|k| format!("couldn't mint a capped key ({e}), and {k}"))?;
                            notes.push(format!("Couldn't mint a capped key ({e}), so this run uses the {} key, without a cap.", provider.name));
                            key
                        }
                    }
                } else {
                    own_key()?
                };
                secrets.push((key_var.to_owned(), key));
            }
            if provider.kind == ProviderKind::Ollama {
                // Chorus never starts Ollama, so say when it isn't there and when the model is odd.
                match crate::providers::ollama_base(provider.base_url.as_deref())
                    .and_then(|base| crate::providers::ollama_tags(&base).map(|models| (base, models)))
                {
                    Ok((_, models)) if !models.iter().any(|m| m.id == model) => {
                        notes.push(format!("Ollama doesn't list {model}, so this turn may fail: pull it or pick another model."));
                    }
                    Ok(_) => {}
                    Err(e) => notes.push(e),
                }
            }
            options.model = Some(route.model);
            route_args = route.args;
            opencode_provider = route.opencode_provider;
        }
        // opencode: its server's password, which the server and its TUI both read from the
        // environment (and which is redacted like a key), and the options as server configuration.
        let server_password = (row.agent == AgentKind::Opencode).then(random_token);
        if let Some(p) = &server_password {
            secrets.push((oc::PASSWORD_VAR.to_owned(), p.clone()));
            if let Some(config) = oc::config_content(spec, &options, opencode_provider.as_ref()) {
                launch.additions.push((oc::CONFIG_VAR, config));
            }
        }
        // Hooks: the agent posts its events to Chorus's listener, with a token only its environment
        // holds (D36, D37).
        let mut hook_args: Vec<String> = Vec::new();
        let mut hook_token = None;
        let codex_control = row.agent == AgentKind::Codex && self.agents.get(row.agent).and_then(|d| d.version).as_deref() == Some("0.163.0");
        let codex_token = codex_control.then(random_token);
        if let Some(token) = &codex_token { secrets.push((codex_bridge::TOKEN_VAR.into(),token.clone())); }
        if row.agent == AgentKind::Claude || (row.agent == AgentKind::Codex && !codex_control) {
            match self.hook_listener.get() {
                Some(Ok(port)) => {
                    let url = hk::hook_url(*port, row.id);
                    let setup = match row.agent {
                        AgentKind::Claude => self.write_claude_settings(row.id, &url).map(|p| vec!["--settings".to_owned(), p.display().to_string()]),
                        _ => agents::hook_relay()
                            .map(|relay| hk::codex_hook_args(&relay.display().to_string(), &url))
                            .ok_or_else(|| "chorus-hook.exe isn't beside Chorus".to_owned()),
                    };
                    match setup {
                        Ok(a) => {
                            let token = random_token();
                            secrets.push((hk::TOKEN_VAR.to_owned(), token.clone()));
                            hook_token = Some(token);
                            hook_args = a;
                        }
                        Err(e) => notes.push(format!("No events from {} this run ({e}), so its state isn't tracked.", row.agent.display_name())),
                    }
                }
                Some(Err(e)) => notes.push(format!("No agent events this run: {e}.")),
                None => notes.push("No agent events this run: the event listener isn't running.".into()),
            }
        }
        let (mut redact, unreadable) = vault::all_for_scrubbing(&self.store);
        redact.extend(secrets.iter().map(|(_, v)| v.clone().into_bytes()));
        let history_redact = Arc::new(redact.clone());
        let additions: Vec<(&str, &str)> = launch.additions.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let env = compose_child_env(&ChildEnvInput { parent: &parent, required: &[], additions: &additions, secrets: &secrets });

        let job = Job::new().map_err(|e| format!("creating the job object: {e}"))?;
        let mut args = launch.args.clone();
        let mut codex_bridge = None;
        let (server, resumed) = if let Some(token) = &codex_token {
            let mut extra = route_args.clone(); extra.extend(spec.option_args(&options));
            let resume = row.agent_session_id.as_deref().filter(|id| agents::conversation_exists(row.agent,id,&parent));
            let bridge = codex_bridge::Bridge::start(&launch,&extra,&env,&row.cwd,resume,&job,token)?;
            args.extend(["--remote".into(),bridge.url.clone(),"--remote-auth-token-env".into(),codex_bridge::TOKEN_VAR.into()]);
            if let Some(id)=&bridge.resume { args.extend(["resume".into(),id.clone()]); }
            codex_bridge = Some(bridge);
            (None,resume.is_some())
        } else if let Some(password) = &server_password {
            let (server, conversation, existed) =
                self.start_opencode(&row, &options, &launch, &env, password, &job, (&parent, &state_dir), &mut notes)?;
            args.extend(oc::tui_args(server.url(), &conversation));
            (Some((server, conversation)), existed)
        } else {
            // The CLI's own arguments, a provider's, then the options, then the conversation, which
            // goes last because for Codex it is a subcommand.
            args.extend(route_args);
            args.extend(hook_args);
            args.extend(spec.option_args(&options));
            // Gemini CLI refuses to create an id whose chat file already exists, and to resume one
            // with no turns in it. A session that ended before its first prompt has exactly that,
            // so it gets a fresh id rather than a launch that fails with exit 42.
            let renewed = (row.agent == AgentKind::Gemini)
                .then_some(row.agent_session_id.as_deref())
                .flatten()
                .filter(|id| agents::gemini_chat_state(id, &parent) == agents::GeminiChat::Empty)
                .map(|_| uuid_v4(vault::random_bytes(16).try_into().expect("16 bytes")));
            if let Some(id) = &renewed {
                rows::set_agent_session_id(&self.store, row.id, id)?;
            }
            let conversation = renewed.as_deref().or(row.agent_session_id.as_deref());
            let exists = conversation.is_some_and(|id| agents::conversation_exists(row.agent, id, &parent));
            args.extend(spec.conversation_args(conversation, exists));
            (None, exists && spec.resume.kind() != ResumeKind::None)
        };
        let detected = self.agents.get(row.agent);
        let cli_version = detected.as_ref().and_then(|d| d.version.clone());

        let mut cmd = CommandBuilder::new(&launch.program);
        cmd.args(&args);
        cmd.cwd(&row.cwd);
        cmd.env_clear();
        for (k, v) in &env {
            cmd.env(k, v);
        }

        let launched_ms = crate::store::now_ms();
        let pair = native_pty_system().openpty(size).map_err(|e| e.to_string())?;
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| format!("starting {}: {e}", launch.program.display()))?;
        drop(pair.slave);
        if let Some(handle) = child.as_raw_handle()
            && let Err(e) = job.assign(windows::Win32::Foundation::HANDLE(handle))
        {
            let _ = child.kill();
            return Err(format!("putting the process in its job: {e}"));
        }
        let pid = child.process_id().unwrap_or(0);
        job.adopt_strays(pid);
        let reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        let run_id = rows::start_run(
            &self.store,
            &NewRun {
                session_id: row.id,
                pid: Some(pid),
                program: &launch.program.display().to_string(),
                args: &args,
                cli_version: cli_version.as_deref(),
                resumed,
            },
        )?;
        if let Some(cid) = row.credential_id {
            let _ = credentials::touch(&self.store, cid);
        }
        let capped = capped.disarm();
        if let Some(ledger) = capped {
            let _ = provider_rows::set_capped_run(&self.store, ledger, run_id);
        }
        if unreadable > 0 {
            notes.push(format!("{unreadable} stored key(s) couldn't be decrypted, so they aren't redacted in this session."));
        }
        if row.agent == AgentKind::Claude && agents::claude_folder_trusted(&parent, &row.cwd) == Some(false) {
            notes.push(chorus_core::claude::TRUST_NOTE_BEFORE.to_owned());
        }
        if let Some(n) = detected.as_ref().and_then(|d| version_note(row.agent, d)) {
            notes.push(n);
        }
        rows::set_note(&self.store, row.id, (!notes.is_empty()).then(|| notes.join(" ")).as_deref())?;

        let (server, conversation) = server.unzip();
        let run = Arc::new(Run {
            id: run_id,
            pid,
            started: Instant::now(),
            writer: Mutex::new(writer),
            master: Mutex::new(Some(pair.master)),
            job,
            requested: Mutex::new(None),
            exit: Mutex::new(None),
            finished: (Mutex::new(false), Condvar::new()),
            server,
            codex_bridge,
            capped,
            asked: options,
            observed: Mutex::new(None),
            recorder: Mutex::new(Recorder::new(
                row.id,
                run_id,
                if row.agent == AgentKind::Opencode || codex_control { "server" } else { "hook" },
                history_redact,
                Arc::new(builtin_patterns()),
            )),
            hook_token,
            last_output_ms: AtomicI64::new(crate::store::now_ms()),
            no_signal_shown: AtomicBool::new(false),
        });
        *live.run.lock().expect("run lock") = Some(run.clone());
        if spec.resume.kind() == ResumeKind::Discovered && row.agent_session_id.is_none() && run.codex_bridge.is_none() {
            self.watch_for_conversation(&row, &run, launched_ms, &parent);
        }
        if let (Some(server), Some(conversation)) = (&run.server, conversation) {
            self.watch_opencode(row.id, &run, server.api.clone(), conversation);
        }
        if let Some(bridge) = &run.codex_bridge { bridge.bind(self,row.id,&run); }
        if row.agent == AgentKind::Claude && run.hook_token.is_some() {
            self.watch_claude_transcript(row.id, &run, launched_ms);
        }
        self.spawn_threads(live.clone(), run, child, reader, StreamScrubber::new(redact, builtin_patterns()));
        self.notify_id(row.id);
        Ok(())
    }

    /// Starts the run's opencode server and finds the session's conversation in it, or starts one.
    /// Returns the server, the conversation id, and whether that conversation already existed.
    #[allow(clippy::too_many_arguments)]
    fn start_opencode(
        &self,
        row: &SessionRow,
        options: &LaunchOptions,
        launch: &agents::Launch,
        env: &BTreeMap<String, String>,
        password: &str,
        job: &Job,
        (parent, state_dir): (&[(String, String)], &Path),
        notes: &mut Vec<String>,
    ) -> Result<(Server, String, bool), String> {
        // A 1.x can't be driven at all. The version is read here when there is no detection yet (a
        // session relaunched at start-up can come first), or when the last one was too old, since
        // the refusal asks the user to update.
        let verified = row.agent.spec().verified;
        let detected = self
            .agents
            .get(AgentKind::Opencode)
            .filter(|d| oc::too_old(d.version.as_deref(), verified).is_none())
            .unwrap_or_else(|| agents::detect::detect(AgentKind::Opencode, parent, state_dir));
        if let Some(refusal) = oc::too_old(detected.version.as_deref(), verified) {
            return Err(refusal);
        }
        let server = Server::start(&launch.program, &launch.args, &row.cwd, env, password, job)?;
        let existing = match row.agent_session_id.as_deref() {
            Some(id) if server.api.session_exists(id)? => Some(id.to_owned()),
            _ => None,
        };
        let variant = oc::wanted_variant(row.agent.spec(), options);
        let (conversation, existed) = match existing {
            Some(id) => {
                // The session keeps whatever its last prompt ran with, a model or effort picked in
                // the TUI included; the run starts with the session's own.
                if let Some(model) = &options.model
                    && let Err(e) = server.api.set_model(&id, model, variant.as_deref())
                {
                    notes.push(format!("Chorus couldn't set the resumed conversation's model, so it keeps its last one: {e}"));
                }
                (id, true)
            }
            None => {
                let id = server.api.create_session(&row.name, &row.cwd, options.model.as_deref(), variant.as_deref())?;
                rows::set_agent_session_id(&self.store, row.id, &id)?;
                (id, false)
            }
        };
        // opencode lists a provider that takes no key (Ollama) as disabled, which is not the same as
        // the model not running.
        let keyless = row.provider_id.is_some_and(|id| {
            provider_rows::get(&self.store, id).ok().flatten().is_some_and(|p| p.kind == ProviderKind::Ollama)
        });
        if let Some(model) = &options.model {
            match server.api.check_model(model, keyless) {
                Ok(Some(check)) => notes.extend(oc::model_note(model, variant.as_deref(), &check)),
                Ok(None) => notes.push("opencode's model list isn't in the shape Chorus checked, so the model wasn't checked.".into()),
                Err(e) => notes.push(format!("Chorus couldn't check the model with opencode: {e}")),
            }
        }
        Ok((server, conversation, existed))
    }

    /// What each opencode turn ran with and the turn events, from the server's event stream while the
    /// run lives. A dropped stream reconnects; opencode 2.1.1 has no way to replay what it missed.
    fn watch_opencode(self: &Arc<Self>, session_id: u32, run: &Arc<Run>, api: Api, conversation: String) {
        let name = format!("opencode-events-{}", run.id);
        let (m, run) = (self.clone(), run.clone());
        let _ = std::thread::Builder::new().name(name).spawn(move || {
            let mut parser = oc::EventParser::new(&conversation);
            let mut tries = 0;
            while !run.wait_finished(Duration::ZERO) && tries < agents::opencode::RECONNECT_TRIES {
                if let Ok(mut stream) = api.events() {
                    tries = 0;
                    while let Some(events) = stream.next() {
                        for event in events {
                            m.record_opencode(session_id, &run, &event, &mut parser, &conversation);
                        }
                    }
                }
                tries += 1;
                if run.wait_finished(agents::opencode::RECONNECT_WAIT) {
                    break;
                }
            }
        });
    }

    /// Claude Code sends no hook when the user interrupts a turn (LESSONS L24), but its transcript
    /// records it. Poll main assistant text too, including messages flushed just after Stop.
    /// Timestamps and run identity prevent replaying older conversations on resume.
    fn watch_claude_transcript(self: &Arc<Self>, session_id: u32, run: &Arc<Run>, launched_ms: i64) {
        let name = format!("claude-transcript-{}", run.id);
        let (m, run) = (self.clone(), run.clone());
        let _ = std::thread::Builder::new().name(name).spawn(move || {
            let mut seen: Option<(String, u64)> = None;
            while !run.wait_finished(TRANSCRIPT_POLL) {
                let (path, open) = {
                    let r = run.recorder.lock().expect("recorder lock");
                    (r.transcript_path(), r.has_open_turn())
                };
                let Some(path) = path else { continue };
                let len = std::fs::metadata(&path).map(|md| md.len()).unwrap_or(0);
                let from = match &seen {
                    Some((p, at)) if *p == path && *at <= len => *at,
                    _ => len.saturating_sub(MAX_TRANSCRIPT_READ),
                };
                if len == from {
                    seen = Some((path, len));
                    continue;
                }
                let Some((lines, read)) = read_new_lines(&path, from, len) else { continue };
                seen = Some((path, from + read));
                for line in &lines {
                    for (key, at, content) in chorus_core::progress::claude_message(line).into_iter().filter(|(_, at, _)| *at >= launched_ms) {
                        if let Ok(Some(row)) = history::progress_turn(&m.store, run.id, at) {
                            run.recorder.lock().expect("recorder lock").progress_at(&m.store, row, &key, content, at);
                        }
                    }
                }
                if open && lines.iter().any(|l| chorus_core::progress::current_interrupt(l,launched_ms)) {
                    let event = chorus_core::events::AgentEvent::TurnInterrupted { turn: None };
                    let changed = {
                        let mut r = run.recorder.lock().expect("recorder lock");
                        let before = r.activity();
                        r.record(&m.store, "claude-transcript", &serde_json::json!({ "interrupted": true }), &[event], crate::store::now_ms());
                        before != r.activity()
                    };
                    if changed {
                        m.notify_id(session_id);
                    }
                }
            }
        });
    }

    /// One event from opencode's server: what a turn ran with, and the turn events it means.
    fn record_opencode(&self, session_id: u32, run: &Run, event: &serde_json::Value, parser: &mut oc::EventParser, conversation: &str) {
        let mut changed = false;
        if let Some(seen) = oc::observed_from_event(event, conversation) {
            let mut slot = run.observed.lock().expect("observed lock");
            changed |= slot.as_ref() != Some(&seen);
            *slot = Some(seen);
        }
        let events = parser.push(event);
        if !events.is_empty() {
            let mut recorder = run.recorder.lock().expect("recorder lock");
            let before = recorder.activity();
            recorder.record(&self.store, "opencode", event, &events, crate::store::now_ms());
            changed |= before != recorder.activity();
        }
        if changed {
            self.notify_id(session_id);
        }
    }

    /// Codex names its conversation once the first prompt is sent. Look for it every few seconds
    /// while the run lives, and once more when it ends.
    fn watch_for_conversation(self: &Arc<Self>, row: &SessionRow, run: &Arc<Run>, launched_ms: i64, parent: &[(String, String)]) {
        let Some(root) = agents::codex::sessions_root(parent) else {
            return;
        };
        let (m, run, id, cwd) = (self.clone(), run.clone(), row.id, row.cwd.clone());
        let originator = chorus_core::codex::originator_for(id);
        let _ = std::thread::Builder::new().name(format!("codex-find-{}", run.id)).spawn(move || {
            loop {
                let ended = run.wait_finished(agents::codex::POLL);
                if let Some(found) = agents::codex::find(&root, &originator, &cwd, launched_ms) {
                    if rows::set_agent_session_id(&m.store, id, &found).is_ok() {
                        m.notify_id(id);
                    }
                    return;
                }
                if ended {
                    return;
                }
            }
        });
    }

    fn spawn_threads(
        self: &Arc<Self>,
        live: Arc<Live>,
        run: Arc<Run>,
        mut child: Box<dyn Child + Send + Sync>,
        mut reader: Box<dyn Read + Send>,
        mut scrubber: StreamScrubber,
    ) {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        std::thread::Builder::new()
            .name(format!("pty-read-{}", run.id))
            .spawn(move || {
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
            })
            .expect("spawn reader thread");

        // Scrub, batch and emit. Nothing reaches the mirror or a view unscrubbed, and the reader
        // never waits on either.
        let (m, l, r) = (self.clone(), live.clone(), run.clone());
        std::thread::Builder::new()
            .name(format!("pty-out-{}", run.id))
            .spawn(move || {
                let mut pending: Vec<u8> = Vec::new();
                let mut oldest: Option<Instant> = None;
                let mut last_input = Instant::now();
                let mut handshake = Handshake::new();
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
                            r.last_output_ms.store(crate::store::now_ms(), Ordering::Relaxed);
                            // ConPTY waits for this answer before it draws anything.
                            let (chunk, answer) = handshake.push(&chunk);
                            if answer {
                                let mut w = r.writer.lock().expect("writer lock");
                                let _ = w.write_all(CURSOR_REPORT).and_then(|()| w.flush());
                            }
                            let safe = scrubber.push(&chunk);
                            if !safe.is_empty() {
                                oldest.get_or_insert_with(Instant::now);
                                pending.extend_from_slice(&safe);
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            // Output went quiet with a possible key start held back: release it.
                            if scrubber.pending() > 0
                                && last_input.elapsed() >= Duration::from_millis(SCRUB_CARRY_RELEASE_MS)
                            {
                                let safe = scrubber.flush();
                                if !safe.is_empty() {
                                    oldest.get_or_insert_with(Instant::now);
                                    pending.extend_from_slice(&safe);
                                }
                            }
                        }
                        Err(RecvTimeoutError::Disconnected) => {
                            let held = handshake.flush();
                            pending.extend(scrubber.push(&held));
                            pending.extend(scrubber.flush());
                            l.hub.emit(&std::mem::take(&mut pending));
                            break;
                        }
                    }
                    let waited = oldest.map_or(0, |t| t.elapsed().as_millis() as u64);
                    if PTY_BATCH.should_flush(pending.len(), waited) {
                        l.hub.emit(&std::mem::take(&mut pending));
                        oldest = None;
                    }
                }
                if let Some((ExitCause::ByItself, code)) = *r.exit.lock().expect("exit lock") {
                    l.hub.emit(&exit_line(code));
                }
                {
                    let mut slot = l.run.lock().expect("run lock");
                    if slot.as_ref().is_some_and(|cur| cur.id == r.id) {
                        *slot = None;
                    }
                }
                r.mark_finished();
                m.notify_id(l.id);
            })
            .expect("spawn output thread");

        // Exit watcher: record why the run ended, then close the pseudoconsole so the reader sees
        // EOF (ConPTY keeps the pipe open until it is closed).
        let m = self.clone();
        std::thread::Builder::new()
            .name(format!("pty-wait-{}", run.id))
            .spawn(move || {
                let code = child.wait().map(|s| s.exit_code()).unwrap_or(u32::MAX);
                let requested = run.requested.lock().expect("requested lock").take();
                let cause = classify_exit(requested, m.quitting.load(Ordering::SeqCst), system_shutting_down());
                let (end, state) = after_exit(cause);
                let _ = rows::end_run(&m.store, run.id, end, Some(code));
                run.recorder.lock().expect("recorder lock").run_ended(&m.store);
                if state == Some(SessionState::Exited) {
                    let note = m.exit_note(live.id, code, run.started.elapsed()).unwrap_or_else(|| format!("Exited with code {code}."));
                    let _ = rows::set_state(&m.store, live.id, SessionState::Exited, Some(&note));
                }
                *run.exit.lock().expect("exit lock") = Some((cause, code));
                if let Some(ledger) = run.capped {
                    m.settle_capped_key(ledger);
                }
                std::thread::sleep(Duration::from_millis(250));
                run.master.lock().expect("master lock").take();
                m.notify_id(live.id);
            })
            .expect("spawn exit watcher");
    }

    pub fn attach(&self, id: u32, sink: Sink) -> Result<u32, String> {
        Ok(self.open_live(id)?.hub.attach(sink))
    }

    pub fn detach(&self, id: u32, view: u32) {
        if let Ok(live) = self.open_live(id) {
            live.hub.detach(view);
        }
    }

    pub fn write(&self, id: u32, data: &str) -> Result<(), String> {
        if data.len() > MAX_WRITE_BYTES {
            return Err("write too large".into());
        }
        let live = self.open_live(id)?;
        let _action = live.action.lock().expect("action lock");
        let run = self.running(id)?;
        let mut recorder = run.recorder.lock().expect("recorder lock");
        if chorus_core::composer::raw_changes_prompt(data) {
            recorder.raw_dirty = true;
            recorder.input_epoch = recorder.input_epoch.wrapping_add(1);
        }
        let mut writer = run.writer.lock().expect("writer lock");
        writer.write_all(data.as_bytes()).and_then(|()| writer.flush()).map_err(|e| e.to_string())
    }

    /// Resizes the live process and remembers the size for the next launch.
    pub fn resize(&self, id: u32, cols: u16, rows_: u16) -> Result<(), String> {
        let size = check_size(cols, rows_)?;
        rows::set_size(&self.store, id, cols, rows_)?;
        if let Ok(run) = self.running(id)
            && let Some(master) = run.master.lock().expect("master lock").as_ref()
        {
            master.resize(size).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn refuse_if_locked(&self, row: &SessionRow, verb: &str) -> Result<(), String> {
        if row.locked {
            return Err(format!("{} is locked. Unlock it before you {verb} it.", row.name));
        }
        Ok(())
    }

    /// Ends a live run on Chorus's request and waits for its output to drain and its processes to
    /// be gone.
    fn end_run_for(&self, live: &Live, cause: ExitCause) -> job::CloseOutcome {
        let Some(run) = live.current_run() else { return job::CloseOutcome::default() };
        *run.requested.lock().expect("requested lock") = Some(cause);
        let outcome = run.end_processes();
        run.wait_finished(FINISH_TIMEOUT);
        run.job.wait_gone(&outcome.killed, GONE_TIMEOUT);
        outcome
    }

    /// Closes a session; with `remove_worktree`, also its worktree when nothing would be lost.
    pub fn close(&self, id: u32, remove_worktree: bool) -> Result<CloseReport, String> {
        let live = self.open_live(id).ok();
        let _action = live.as_ref().map(|l| l.action.lock().expect("action lock"));
        let row = self.row(id)?;
        self.refuse_if_locked(&row, "close")?;
        let outcome = self.open_live(id).map(|live| self.end_run_for(&live, ExitCause::UserClose)).unwrap_or_default();
        rows::set_state(&self.store, id, SessionState::Closed, None)?;
        if let Some(live) = self.live.lock().expect("live lock").remove(&id) {
            live.hub.remove();
        }
        let _ = std::fs::remove_dir_all(self.state_dir(id));
        let worktree_note = row.worktree_id.filter(|_| remove_worktree).and_then(|w| self.retire_worktree(id, w));
        self.notify_id(id);
        Ok(CloseReport { killed: outcome.killed.len() as u32, spared: outcome.spared.len() as u32, worktree_note })
    }

    pub fn restart(self: &Arc<Self>, id: u32) -> Result<SessionSummary, String> {
        if self.row(id)?.state == SessionState::Closed {
            return Err("a closed session can't be restarted".into());
        }
        let action_live = self.open_live(id)?;
        let _action = action_live.action.lock().expect("action lock");
        let row = self.row(id)?;
        if row.state == SessionState::Closed {
            return Err("a closed session can't be restarted".into());
        }
        self.refuse_if_locked(&row, "restart")?;
        let live = self.live_for(id);
        self.end_run_for(&live, ExitCause::UserRestart);
        live.hub.new_run(BOUNDARY_RESTARTED);
        rows::set_state(&self.store, id, SessionState::Running, None)?;
        if let Err(e) = self.start_run(&live) {
            self.record_failure(&live, &e);
        }
        let summary = self.summary(id)?;
        (self.notify)(summary.clone());
        Ok(summary)
    }

    /// The agents a launch can use: those found on this machine, or all of them before the first
    /// detection finishes (a launch then fails visibly if one is missing).
    fn installed(&self) -> Vec<AgentKind> {
        AgentKind::ALL.into_iter().filter(|k| self.agents.get(*k).is_none_or(|d| d.found)).collect()
    }

    /// Every preset as it would work for `agent` here.
    pub fn presets(&self, agent: AgentKind) -> Vec<PresetChoice> {
        let installed = self.installed();
        launch_rules::PRESETS
            .iter()
            .map(|p| {
                let plan = launch_rules::plan(*p, agent, &installed, launch_rules::SWARM_MIN);
                PresetChoice {
                    preset: *p,
                    label: p.label().to_owned(),
                    blurb: p.blurb().to_owned(),
                    note: p.note().map(str::to_owned),
                    reason: plan.as_ref().err().cloned(),
                    slots: plan.unwrap_or_default(),
                }
            })
            .collect()
    }

    /// Starts the sessions `preset` makes. The first is `session` as given; a reviewer or a shell
    /// is derived from it (same folder or worktree, its agent's defaults, no key), and each of a
    /// Swarm's agents gets a worktree and a name of its own. If the first can't be created nothing
    /// is; a later one that fails is reported and the others stay.
    pub fn launch(self: &Arc<Self>, req: LaunchRequest) -> Result<Vec<LaunchOutcome>, String> {
        let slots = launch_rules::plan(req.preset, req.session.agent, &self.installed(), req.count.unwrap_or(1))?;
        if req.preset == Preset::Swarm {
            if matches!(req.session.workspace, Workspace::ExistingWorktree(_)) {
                return Err("A Swarm makes a new worktree for each agent; choose New worktree.".into());
            }
            return self.launch_swarm(&req.session, &slots);
        }
        let first = self.create(req.session.clone())?;
        let mut out = vec![LaunchOutcome { agent: first.agent, role: slots[0].role, session: Some(first.clone()), error: None }];
        let worktree = self.row(first.id)?.worktree_id;
        for slot in &slots[1..] {
            let derived = CreateSession {
                agent: slot.agent,
                cwd: first.cwd.clone(),
                name: Some(launch_rules::slot_name(&first.name, slot.role)),
                credential: None,
                cols: req.session.cols,
                rows: req.session.rows,
                options: LaunchOptions::default(),
                provider_id: None,
                description: launch_rules::slot_description(&first.name, slot.role),
                project_id: first.project_id,
                tags: first.tags.clone(),
                workspace: Workspace::Here,
                routing: None,
            };
            let (session, error) = match self.create_in(derived, worktree) {
                Ok(s) => (Some(s), None),
                Err(e) => (None, Some(e)),
            };
            out.push(LaunchOutcome { agent: slot.agent, role: slot.role, session, error });
        }
        Ok(out)
    }

    /// A Swarm: the same agent and settings in each slot, each in a new worktree; the first keeps
    /// the name given, the others get names from the pool.
    fn launch_swarm(self: &Arc<Self>, session: &CreateSession, slots: &[Slot]) -> Result<Vec<LaunchOutcome>, String> {
        let mut out = Vec::new();
        let mut named: Vec<String> = Vec::new();
        for (i, slot) in slots.iter().enumerate() {
            let mut req = CreateSession { workspace: Workspace::NewWorktree, ..session.clone() };
            if i > 0 {
                req.name = Some(self.suggest_name(&named));
            }
            match self.create(req) {
                Ok(s) => {
                    named.push(s.name.clone());
                    out.push(LaunchOutcome { agent: slot.agent, role: slot.role, session: Some(s), error: None });
                }
                Err(e) if i == 0 => return Err(e),
                Err(e) => out.push(LaunchOutcome { agent: slot.agent, role: slot.role, session: None, error: Some(e) }),
            }
        }
        Ok(out)
    }

    pub fn rename(&self, id: u32, name: &str) -> Result<SessionSummary, String> {
        self.row(id)?;
        if name.trim().is_empty() {
            return Err("A session needs a name.".into());
        }
        rows::set_name(&self.store, id, &clean_name(Some(name), "")?)?;
        let summary = self.summary(id)?;
        (self.notify)(summary.clone());
        Ok(summary)
    }

    pub fn set_description(&self, id: u32, text: Option<&str>) -> Result<SessionSummary, String> {
        self.row(id)?;
        rows::set_description(&self.store, id, launch_rules::clean_description(text)?.as_deref())?;
        let summary = self.summary(id)?;
        (self.notify)(summary.clone());
        Ok(summary)
    }

    /// The project a new session joins: the one asked for, which must exist and not be archived,
    /// else the closest project whose folder holds `cwd`.
    fn project_for(&self, asked: Option<u32>, cwd: &str) -> Result<Option<u32>, String> {
        if let Some(id) = asked {
            return match project_rows::get(&self.store, id)? {
                Some(p) if p.state != ProjectState::Archived => Ok(Some(id)),
                Some(p) => Err(format!("{} is archived; restore it before adding sessions.", p.name)),
                None => Err(format!("no project {id}")),
            };
        }
        let projects = project_rows::list(&self.store)?;
        Ok(chorus_core::projects::project_for_folder(
            projects.iter().filter(|p| p.state != ProjectState::Archived).filter_map(|p| p.folder.as_deref().map(|f| (p.id, f))),
            cwd,
        ))
    }

    /// Moves a session into a project, or out of any with `None`.
    pub fn set_project(&self, id: u32, project_id: Option<u32>) -> Result<SessionSummary, String> {
        self.row(id)?;
        if let Some(pid) = project_id {
            match project_rows::get(&self.store, pid)? {
                Some(p) if p.state == ProjectState::Archived => {
                    return Err(format!("{} is archived; restore it before adding sessions.", p.name));
                }
                Some(_) => {}
                None => return Err(format!("no project {pid}")),
            }
        }
        rows::set_project(&self.store, id, project_id)?;
        let summary = self.summary(id)?;
        (self.notify)(summary.clone());
        Ok(summary)
    }

    pub fn set_locked(&self, id: u32, locked: bool, pin: Option<&str>) -> Result<SessionSummary, String> {
        let live = self.open_live(id).ok();
        let _action = live.as_ref().map(|l| l.action.lock().expect("action lock"));
        self.row(id)?;
        if !locked {
            crate::lock::check_unlock(&self.store, pin)?;
        }
        rows::set_locked(&self.store, id, locked)?;
        let summary = self.summary(id)?;
        (self.notify)(summary.clone());
        Ok(summary)
    }

    /// Chorus is quitting: record every live run as ended by that, so the next start relaunches
    /// their sessions instead of treating them as crashed. The processes die with Chorus's jobs.
    pub fn shutdown(self: &Arc<Self>) {
        self.quitting.store(true, Ordering::SeqCst);
        let mut capped = Vec::new();
        for live in self.live.lock().expect("live lock").values() {
            if let Some(run) = live.current_run() {
                let _ = rows::end_run(&self.store, run.id, chorus_core::session::RunEnd::ChorusClosed, None);
                capped.extend(run.capped);
            }
        }
        // Revoke the runs' minted keys before the app goes, waiting a few seconds at most; the next
        // start-up sweeps any this misses, and each expires within a day anyway.
        let (tx, rx) = mpsc::channel::<()>();
        for ledger in &capped {
            let (m, tx, ledger) = (self.clone(), tx.clone(), *ledger);
            let _ = std::thread::Builder::new().name(format!("revoke-{ledger}")).spawn(move || {
                crate::providers::keys::revoke_now(m.store(), ledger);
                let _ = tx.send(());
            });
        }
        let deadline = Instant::now() + SHUTDOWN_REVOKE_WAIT;
        for _ in &capped {
            if rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).is_err() {
                break;
            }
        }
    }

    /// Why a run that ended by itself ended, when Chorus can tell from a structured source: Claude
    /// Code exiting soon after start in a folder it still doesn't trust answered No to its trust
    /// question (LESSONS L33).
    fn exit_note(&self, id: u32, code: u32, ran: Duration) -> Option<String> {
        let row = self.row(id).ok()?;
        let parent: Vec<(String, String)> = std::env::vars().collect();
        (row.agent == AgentKind::Claude
            && code != 0
            && ran < TRUST_EXIT_WINDOW
            && agents::claude_folder_trusted(&parent, &row.cwd) == Some(false))
        .then(|| chorus_core::claude::TRUST_NOTE_AFTER.to_owned())
    }

    /// Revokes a run's minted key and reads its cost, on a thread of its own.
    fn settle_capped_key(self: &Arc<Self>, ledger: u32) {
        let m = self.clone();
        let _ = std::thread::Builder::new().name(format!("settle-key-{ledger}")).spawn(move || {
            crate::providers::keys::settle(m.store(), ledger);
            m.notify_all();
        });
    }
}

/// Revokes a key minted for a launch that then failed, unless the run took it over.
struct CappedGuard {
    manager: Arc<SessionManager>,
    ledger: Option<u32>,
}

impl CappedGuard {
    fn disarm(&mut self) -> Option<u32> {
        self.ledger.take()
    }
}

impl Drop for CappedGuard {
    fn drop(&mut self) {
        if let Some(ledger) = self.ledger.take() {
            self.manager.settle_capped_key(ledger);
        }
    }
}

#[cfg(test)]
mod tests;
mod codex_bridge;
