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
pub mod job;
pub mod mirror;

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use chorus_core::agent::{LaunchOptions, ResumeKind, uuid_v4};
use chorus_core::batch::{PTY_BATCH, SCRUB_CARRY_RELEASE_MS};
use chorus_core::cli::{VersionFit, fit, parse_version};
use chorus_core::conpty::{CURSOR_REPORT, Handshake};
use chorus_core::env::{ChildEnvInput, compose_child_env, valid_secret_var};
use chorus_core::opencode::{self as oc, Observed};
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
use crate::store::sessions::{self as rows, NewRun, NewSession, SessionRow};
use crate::store::{Store, credentials};
use crate::vault;
use job::Job;
use mirror::Mirror;

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
    pub cwd: String,
    pub status: SessionStatus,
    /// A process is running now. `status` can be `running` without one for a moment while a
    /// Restart or a relaunch is under way.
    pub live: bool,
    pub pid: Option<u32>,
    pub locked: bool,
    /// Why the session isn't running, or what went wrong, in words to show as is.
    pub note: Option<String>,
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
    /// What the running agent's newest turn really used, read back from the agent (opencode).
    pub observed: Option<Observed>,
    /// How that turn differs from `options`, one phrase each; empty when it matches.
    pub observed_differs: Vec<String>,
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
}

#[derive(Debug, Clone, Deserialize, Type)]
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
}

const MAX_WRITE_BYTES: usize = 1 << 20;
const MAX_CWD_BYTES: usize = 1024;
/// How long Close and Restart wait for a run's output to drain before moving on.
const FINISH_TIMEOUT: Duration = Duration::from_secs(5);
const BOUNDARY_RESTARTED: &str = "── Restarted ──";
const BOUNDARY_RELAUNCHED: &str = "── Chorus restarted: session relaunched ──";

fn check_size(cols: u16, rows: u16) -> Result<PtySize, String> {
    if !(2..=1000).contains(&cols) || !(2..=1000).contains(&rows) {
        return Err(format!("terminal size {cols}x{rows} is out of range"));
    }
    Ok(PtySize { cols, rows, pixel_width: 0, pixel_height: 0 })
}

/// A fresh password for a run's opencode server: 24 random bytes as hex.
fn server_password() -> String {
    vault::random_bytes(24).iter().map(|b| format!("{b:02x}")).collect()
}

fn version_note(kind: AgentKind, d: &agents::detect::Detection) -> Option<String> {
    chorus_core::cli::version_note(kind.display_name(), d.version.as_deref(), kind.spec().verified, d.fit?)
}

/// A folder for a new session: a full path to a folder that exists.
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
    Ok(s.to_owned())
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
    /// Filled by the exit watcher for the output thread, which writes the exit line last.
    exit: Mutex<Option<(ExitCause, u32)>>,
    finished: (Mutex<bool>, Condvar),
    /// opencode's server, which ends with the run.
    server: Option<Server>,
    /// What the newest turn ran with, read back from the agent.
    observed: Mutex<Option<Observed>>,
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
    quitting: AtomicBool,
    notify: Notify,
    agents: Detector,
}

impl SessionManager {
    pub fn new(store: Store, data_dir: PathBuf, notify: Notify) -> Arc<Self> {
        Arc::new(Self {
            store,
            data_dir,
            live: Mutex::new(HashMap::new()),
            quitting: AtomicBool::new(false),
            notify,
            agents: Detector::default(),
        })
    }

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
                Arc::new(Live { id, hub: Hub::new(Mirror::open(&self.scrollback_dir(), id)), run: Mutex::new(None) })
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
        SessionSummary {
            id: row.id,
            name: row.name.clone(),
            agent: row.agent,
            cwd: row.cwd.clone(),
            status: row.state.into(),
            live: run.is_some(),
            pid: run.as_ref().map(|r| r.pid),
            locked: row.locked,
            note: row.note.clone(),
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
            observed_differs: observed.as_ref().map(|o| oc::differences(row.agent.spec(), &row.options, o)).unwrap_or_default(),
            observed,
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
        }
        Ok(open.iter().map(|r| r.id).filter(|id| plan.relaunch.contains(id)).collect())
    }

    /// The second half: launches each session again. It can take seconds (an opencode session
    /// waits for its server), so the app calls it off the setup thread.
    pub fn relaunch(self: &Arc<Self>, ids: &[u32]) {
        for id in ids {
            let live = self.live_for(*id);
            live.hub.new_run(BOUNDARY_RELAUNCHED);
            if let Err(e) = self.start_run(&live) {
                self.record_failure(&live, &e);
            }
        }
    }

    pub fn create(self: &Arc<Self>, req: CreateSession) -> Result<SessionSummary, String> {
        check_size(req.cols, req.rows)?;
        let cwd = check_cwd(&req.cwd)?;
        if let Some(c) = &req.credential {
            if !valid_secret_var(&c.env_var) {
                return Err(format!("{:?} can't be used as the key's variable name.", c.env_var));
            }
            credentials::get(&self.store, c.id)?.ok_or("That key no longer exists.")?;
        }
        let spec = req.agent.spec();
        let options = spec.resolve(&req.options)?;
        // Chorus names the conversation up front where the agent lets it, so every later run can
        // resume it.
        let conversation =
            (spec.resume.kind() == ResumeKind::Assigned).then(|| uuid_v4(vault::random_bytes(16).try_into().expect("16 bytes")));
        let fallback = format!("{} {}", req.agent.display_name(), rows::count_by_agent(&self.store, req.agent)? + 1);
        let name = clean_name(req.name.as_deref(), &fallback)?;
        let id = rows::insert(
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
            },
        )?;
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
        if !Path::new(&row.cwd).is_dir() {
            return Err(format!("the folder {} no longer exists", row.cwd));
        }
        let state_dir = self.state_dir(row.id);
        std::fs::create_dir_all(&state_dir).map_err(|e| format!("creating {}: {e}", state_dir.display()))?;

        let parent: Vec<(String, String)> = std::env::vars().collect();
        let mut launch = agents::resolve(row.agent, &parent, &state_dir)?;
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
        // opencode: its server's password, which the server and `attach` both read from the
        // environment (and which is redacted like a key), and the options as server configuration.
        let server_password = (row.agent == AgentKind::Opencode).then(server_password);
        if let Some(p) = &server_password {
            secrets.push((oc::PASSWORD_VAR.to_owned(), p.clone()));
            if let Some(config) = oc::config_content(spec, &row.options) {
                launch.additions.push((oc::CONFIG_VAR, config));
            }
        }
        let (mut redact, unreadable) = vault::all_for_scrubbing(&self.store);
        redact.extend(secrets.iter().map(|(_, v)| v.clone().into_bytes()));
        let additions: Vec<(&str, &str)> = launch.additions.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let env = compose_child_env(&ChildEnvInput { parent: &parent, required: &[], additions: &additions, secrets: &secrets });

        let job = Job::new().map_err(|e| format!("creating the job object: {e}"))?;
        let mut notes: Vec<String> = Vec::new();
        let mut args = launch.args.clone();
        let (server, resumed) = if let Some(password) = &server_password {
            let (server, conversation, existed) = self.start_opencode(&row, &launch, &env, password, &state_dir, &job, &mut notes)?;
            args.extend(oc::attach_args(server.url(), &conversation));
            (Some((server, conversation)), existed)
        } else {
            // The CLI's own arguments, then the options, then the conversation, which goes last
            // because for Codex it is a subcommand.
            args.extend(spec.option_args(&row.options));
            let conversation = row.agent_session_id.as_deref();
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
        if unreadable > 0 {
            notes.push(format!("{unreadable} stored key(s) couldn't be decrypted, so they aren't redacted in this session."));
        }
        if let Some(n) = detected.as_ref().and_then(|d| version_note(row.agent, d)) {
            notes.push(n);
        }
        rows::set_note(&self.store, row.id, (!notes.is_empty()).then(|| notes.join(" ")).as_deref())?;

        let (server, conversation) = server.unzip();
        let run = Arc::new(Run {
            id: run_id,
            pid,
            writer: Mutex::new(writer),
            master: Mutex::new(Some(pair.master)),
            job,
            requested: Mutex::new(None),
            exit: Mutex::new(None),
            finished: (Mutex::new(false), Condvar::new()),
            server,
            observed: Mutex::new(None),
        });
        *live.run.lock().expect("run lock") = Some(run.clone());
        if spec.resume.kind() == ResumeKind::Discovered && row.agent_session_id.is_none() {
            self.watch_for_conversation(&row, &run, launched_ms, &parent);
        }
        if let (Some(server), Some(conversation)) = (&run.server, conversation) {
            self.watch_opencode(row.id, &run, server.api.clone(), conversation);
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
        launch: &agents::Launch,
        env: &BTreeMap<String, String>,
        password: &str,
        state_dir: &Path,
        job: &Job,
        notes: &mut Vec<String>,
    ) -> Result<(Server, String, bool), String> {
        if row.options.model.is_some() || row.options.effort.is_some() {
            agents::opencode::forget_tui_choice(state_dir);
        }
        let server = Server::start(&launch.program, &launch.args, &row.cwd, env, password, job)?;
        let existing = match row.agent_session_id.as_deref() {
            Some(id) if server.api.session_exists(id)? => Some(id.to_owned()),
            _ => None,
        };
        let (conversation, existed) = match existing {
            Some(id) => (id, true),
            None => {
                let id = server.api.create_session(&row.name)?;
                rows::set_agent_session_id(&self.store, row.id, &id)?;
                (id, false)
            }
        };
        if let Some(model) = &row.options.model {
            let wanted = oc::wanted_variant(row.agent.spec(), &row.options);
            match server.api.check_model(model) {
                Ok(Some(check)) => notes.extend(oc::model_note(model, wanted.as_deref(), &check)),
                Ok(None) => notes.push("opencode's model list isn't in the shape Chorus checked, so the model wasn't checked.".into()),
                Err(e) => notes.push(format!("Chorus couldn't check the model with opencode: {e}")),
            }
        }
        Ok((server, conversation, existed))
    }

    /// What each new opencode turn ran with, read back every few seconds while the run lives
    /// (P3 moves this onto the server's events).
    fn watch_opencode(self: &Arc<Self>, session_id: u32, run: &Arc<Run>, api: Api, conversation: String) {
        let name = format!("opencode-watch-{}", run.id);
        let (m, run) = (self.clone(), run.clone());
        let _ = std::thread::Builder::new().name(name).spawn(move || {
            while !run.wait_finished(agents::opencode::POLL) {
                let Ok(Some(seen)) = api.latest_user_turn(&conversation) else { continue };
                let changed = {
                    let mut slot = run.observed.lock().expect("observed lock");
                    let changed = slot.as_ref() != Some(&seen);
                    *slot = Some(seen);
                    changed
                };
                if changed {
                    m.notify_id(session_id);
                }
            }
        });
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
                if state == Some(SessionState::Exited) {
                    let _ = rows::set_state(&m.store, live.id, SessionState::Exited, Some(&format!("Exited with code {code}.")));
                }
                *run.exit.lock().expect("exit lock") = Some((cause, code));
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
        let run = self.running(id)?;
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

    /// Ends a live run on Chorus's request and waits for its output to drain.
    fn end_run_for(&self, live: &Live, cause: ExitCause) -> job::CloseOutcome {
        let Some(run) = live.current_run() else { return job::CloseOutcome::default() };
        *run.requested.lock().expect("requested lock") = Some(cause);
        let outcome = run.end_processes();
        run.wait_finished(FINISH_TIMEOUT);
        outcome
    }

    pub fn close(&self, id: u32) -> Result<CloseReport, String> {
        let row = self.row(id)?;
        self.refuse_if_locked(&row, "close")?;
        let outcome = self.open_live(id).map(|live| self.end_run_for(&live, ExitCause::UserClose)).unwrap_or_default();
        rows::set_state(&self.store, id, SessionState::Closed, None)?;
        if let Some(live) = self.live.lock().expect("live lock").remove(&id) {
            live.hub.remove();
        }
        let _ = std::fs::remove_dir_all(self.state_dir(id));
        self.notify_id(id);
        Ok(CloseReport { killed: outcome.killed.len() as u32, spared: outcome.spared.len() as u32 })
    }

    pub fn restart(self: &Arc<Self>, id: u32) -> Result<SessionSummary, String> {
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

    pub fn set_locked(&self, id: u32, locked: bool, pin: Option<&str>) -> Result<SessionSummary, String> {
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
    pub fn shutdown(&self) {
        self.quitting.store(true, Ordering::SeqCst);
        for live in self.live.lock().expect("live lock").values() {
            if let Some(run) = live.current_run() {
                let _ = rows::end_run(&self.store, run.id, chorus_core::session::RunEnd::ChorusClosed, None);
            }
        }
    }
}

#[cfg(test)]
mod tests;
