//! Council (P10, D136–D138): Settings → Council (the provider, members and limits), and the
//! Council screen's commands: the Docket, starting and cancelling a run, and reading one back.

mod code;
mod local;
mod run;

pub use local::serve as serve_local_turn;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use chorus_core::council::{
    self as core, TurnOutcome, Phase, CouncilRole, brief,
    local::RunsOn,
    tools::CouncilLimits,
    verdict::{self, QuestionState, QuestionSummary, Ruling},
};
use chorus_core::providers::ProviderKind;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tauri_specta::Event;

use crate::sessions::SessionManager;
use crate::store::council::{self as db, MemberRow, RosterEntry, RunRow, RunStatus, ToolCallRow, TurnRow};
use crate::store::{Store, projects, providers as provider_rows, settings};

type Manager<'a> = State<'a, Arc<SessionManager>>;
type Councils<'a> = State<'a, Arc<Council>>;

const CONFIG: &str = "council.config";
/// Output tokens when a member doesn't set its own (Chorus 1's measured defaults).
const DEFAULT_MEMBER_TOKENS: u32 = 16_000;
const DEFAULT_ARBITER_TOKENS: u32 = 32_000;

/// A run changed: the Council screen reads it again.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct CouncilUpdated {
    pub run_id: u32,
}

/// The run in progress, if any, and how to tell the screen about changes.
pub struct Council {
    active: Mutex<Option<(u32, Arc<AtomicBool>)>>,
    notify: Arc<dyn Fn(u32) + Send + Sync>,
}

impl Council {
    pub fn new(notify: Box<dyn Fn(u32) + Send + Sync>) -> Self {
        Self { active: Mutex::new(None), notify: Arc::from(notify) }
    }
}

/// At start-up: a run Chorus was in the middle of is marked abandoned (its key is revoked by the
/// start-up key sweep).
pub fn prepare(store: &Store) -> Result<(), String> {
    db::heal(store)
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CouncilConfig {
    /// The OpenRouter provider whose management key caps each run's spend.
    pub provider_id: Option<u32>,
    pub limits: CouncilLimits,
}

fn config(store: &Store) -> Result<CouncilConfig, String> {
    settings::get(store, CONFIG)?
        .map(|s| serde_json::from_str(&s).map_err(|_| "Council settings couldn't be read; save them again.".to_owned()))
        .unwrap_or(Ok(CouncilConfig::default()))
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderChoice {
    pub id: u32,
    pub name: String,
    /// Why it can't be the Council's provider.
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MemberView {
    pub member: MemberRow,
    /// What the run will ask for: the member's own, or the role's default, held to the model's
    /// own output limit.
    pub effective_max_tokens: u32,
    pub context_length: Option<u32>,
    /// Why this member can't take part as things stand.
    pub problem: Option<String>,
    /// Something worth knowing that doesn't stop it (a local CLI's version).
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CouncilSettings {
    pub config: CouncilConfig,
    pub default_limits: CouncilLimits,
    pub providers: Vec<ProviderChoice>,
    pub members: Vec<MemberView>,
    /// Why a council can't run, or none when it can.
    pub reason: Option<String>,
}

fn provider_problem(store: &Store, p: &provider_rows::ProviderRow) -> Option<String> {
    if p.kind != ProviderKind::Openrouter {
        return Some("Not an OpenRouter provider.".into());
    }
    match p.management_credential_id {
        None => return Some("Add a management key: a council caps each run's spend with a key minted from it.".into()),
        Some(id) if crate::store::credentials::blob(store, id).ok().flatten().is_none() => {
            return Some("Its management key was deleted.".into());
        }
        _ => {}
    }
    None
}

/// The member as a run would take it, or why it can't.
/// Why a local member's CLI can't run now, or none (`local::problem`, or a stand-in in tests).
type LocalCheck<'a> = &'a dyn Fn(RunsOn) -> Option<String>;

fn member_view(store: &Store, provider: Option<u32>, m: &MemberRow, local: LocalCheck<'_>) -> MemberView {
    let default = if m.role == CouncilRole::Arbiter { DEFAULT_ARBITER_TOKENS } else { DEFAULT_MEMBER_TOKENS };
    let wanted = m.max_tokens.unwrap_or(default);
    // A local member's CLI decides its own output and context limits.
    if m.runs_on.is_local() {
        return MemberView { member: m.clone(), effective_max_tokens: wanted, context_length: None, problem: local(m.runs_on), note: None };
    }
    let catalog = provider.and_then(|p| provider_rows::catalog_model(store, p, &m.model).ok().flatten());
    let problem = match (provider, &catalog) {
        (None, _) => Some("Choose the Council's OpenRouter provider first.".to_owned()),
        (Some(_), None) => Some(format!("`{}` isn't in the provider's model list. Fix the name, or refresh the list in Settings → Providers.", m.model)),
        (Some(_), Some(c)) if c.tools != Some(true) => Some(format!(
            "`{}` doesn't take tools, so it couldn't read the project's code. Choose a model that does.",
            m.model
        )),
        _ => None,
    };
    let effective = catalog.as_ref().and_then(|c| c.max_output).map_or(wanted, |max| wanted.min(max));
    MemberView { member: m.clone(), effective_max_tokens: effective, context_length: catalog.and_then(|c| c.context_length), problem, note: None }
}

/// The provider and roster a run would use now, or the one reason it can't start.
/// The provider (when a member runs on OpenRouter) and roster a run would use now, or the one
/// reason it can't start.
fn readiness(store: &Store, cfg: &CouncilConfig, local: LocalCheck<'_>) -> Result<(Option<provider_rows::ProviderRow>, Vec<RosterEntry>), String> {
    let members = db::members(store)?;
    let provider = if members.iter().any(|m| m.runs_on == RunsOn::Openrouter) {
        let id = cfg.provider_id.ok_or("Choose the Council's OpenRouter provider in Settings → Council.")?;
        let provider = provider_rows::get(store, id)?.ok_or("The Council's provider was deleted. Choose another in Settings → Council.")?;
        if let Some(problem) = provider_problem(store, &provider) {
            return Err(format!("The Council's provider \"{}\": {problem}", provider.name));
        }
        Some(provider)
    } else {
        None
    };
    cfg.limits.check()?;
    let views: Vec<MemberView> = members.iter().map(|m| member_view(store, provider.as_ref().map(|p| p.id), m, local)).collect();
    if let Some(v) = views.iter().find(|v| v.problem.is_some()) {
        return Err(format!("Council member '{}': {}", v.member.label, v.problem.as_deref().unwrap_or("")));
    }
    let arbiters: Vec<_> = views.iter().filter(|v| v.member.role == CouncilRole::Arbiter).collect();
    let deliberators = views.iter().filter(|v| v.member.role == CouncilRole::Member).count();
    match arbiters.len() {
        0 => return Err("This council has no arbiter. Mark exactly one member as the arbiter in Settings → Council.".into()),
        1 => {}
        n => return Err(format!("This council has {n} arbiters. Mark exactly one in Settings → Council.")),
    }
    if deliberators < core::MIN_MEMBERS {
        return Err(format!("A council needs at least two members besides the arbiter; this one has {deliberators}."));
    }
    if views.len() > core::MAX_PARTICIPANTS {
        return Err(format!("A council is limited to {} participants; this one has {}.", core::MAX_PARTICIPANTS, views.len()));
    }
    // Members first, the arbiter last: the order the run and its findings list them.
    let mut roster: Vec<RosterEntry> = views
        .iter()
        .map(|v| RosterEntry {
            id: v.member.id,
            label: v.member.label.clone(),
            model: v.member.model.clone(),
            role: v.member.role,
            max_tokens: v.effective_max_tokens,
            context_length: v.context_length,
            runs_on: v.member.runs_on,
        })
        .collect();
    roster.sort_by_key(|r| r.role == CouncilRole::Arbiter);
    Ok((provider, roster))
}

fn settings_view(m: &SessionManager) -> Result<CouncilSettings, String> {
    let store = m.store();
    let check = |r: RunsOn| local::problem(m, r);
    let cfg = config(store)?;
    let providers = provider_rows::list(store)?
        .into_iter()
        .filter(|p| p.kind == ProviderKind::Openrouter)
        .map(|p| ProviderChoice { id: p.id, name: p.name.clone(), problem: provider_problem(store, &p) })
        .collect();
    let members = db::members(store)?
        .iter()
        .map(|row| MemberView { note: local::version_note(m, row.runs_on), ..member_view(store, cfg.provider_id, row, &check) })
        .collect();
    Ok(CouncilSettings {
        reason: readiness(store, &cfg, &check).err(),
        config: cfg,
        default_limits: CouncilLimits::default(),
        providers,
        members,
    })
}

#[tauri::command]
#[specta::specta]
pub fn council_settings(state: Manager<'_>) -> Result<CouncilSettings, String> {
    settings_view(&state)
}

#[tauri::command]
#[specta::specta]
pub fn council_configure(state: Manager<'_>, config: CouncilConfig) -> Result<CouncilSettings, String> {
    let store = state.store();
    if let Some(id) = config.provider_id {
        let p = provider_rows::get(store, id)?.ok_or("That provider no longer exists.")?;
        if p.kind != ProviderKind::Openrouter {
            return Err("Choose an OpenRouter provider.".into());
        }
    }
    config.limits.check()?;
    settings::set(store, CONFIG, &serde_json::to_string(&config).map_err(|e| e.to_string())?)?;
    settings_view(&state)
}

#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MemberInput {
    pub id: Option<u32>,
    pub label: String,
    pub model: String,
    pub role: CouncilRole,
    pub max_tokens: Option<u32>,
    /// Where its turns run (D140).
    pub runs_on: RunsOn,
}

#[tauri::command]
#[specta::specta]
pub fn council_member_save(state: Manager<'_>, input: MemberInput) -> Result<CouncilSettings, String> {
    let store = state.store();
    let label = input.label.trim();
    if label.is_empty() || label.chars().count() > 60 {
        return Err("A council member needs a name of 1–60 characters.".into());
    }
    let model = if input.runs_on.is_local() {
        // An OpenRouter id is turned into the CLI's own name (`anthropic/claude-opus-5.5` →
        // `claude-opus-5-5`), so switching a member to a subscription keeps working.
        core::local::local_model(input.runs_on, &input.model)?
    } else {
        let model = input.model.trim().to_owned();
        if !chorus_core::agent::valid_model(&model) {
            return Err("That isn't a model id (for example `anthropic/claude-opus-5`).".into());
        }
        model
    };
    let model = model.as_str();
    if input.max_tokens.is_some_and(|n| !(256..=200_000).contains(&n)) {
        return Err("Max output tokens must be between 256 and 200,000, or empty for the default.".into());
    }
    // A local CLI decides its own output limit.
    let max_tokens = if input.runs_on.is_local() { None } else { input.max_tokens };
    db::save_member(store, input.id, label, model, input.role, max_tokens, input.runs_on)?;
    settings_view(&state)
}

#[tauri::command]
#[specta::specta]
pub fn council_member_delete(state: Manager<'_>, id: u32) -> Result<CouncilSettings, String> {
    db::delete_member(state.store(), id)?;
    settings_view(&state)
}

/// Picks a brief (`.md`), starting in the project's folder.
#[tauri::command]
#[specta::specta]
pub async fn council_brief_pick(app: tauri::AppHandle, start: Option<String>) -> Option<String> {
    use tauri::Manager as _;
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app.dialog().file().set_title("Choose a council brief").add_filter("Markdown", &["md"]);
        if let Some(start) = start.filter(|s| Path::new(s).is_dir()) {
            dialog = dialog.set_directory(start);
        }
        if let Some(window) = app.get_webview_window("main") {
            dialog = dialog.set_parent(&window);
        }
        dialog.blocking_pick_file().and_then(|p| p.into_path().ok()).map(|p| p.display().to_string())
    })
    .await
    .ok()
    .flatten()
}

/// Picks the folder a council reads, which may be a copy of the project (a redacted one, say).
#[tauri::command]
#[specta::specta]
pub async fn council_folder_pick(app: tauri::AppHandle, start: Option<String>) -> Option<String> {
    use tauri::Manager as _;
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app.dialog().file().set_title("Choose the folder the council reads");
        if let Some(start) = start.filter(|s| Path::new(s).is_dir()) {
            dialog = dialog.set_directory(start);
        }
        if let Some(window) = app.get_webview_window("main") {
            dialog = dialog.set_parent(&window);
        }
        dialog.blocking_pick_folder().and_then(|p| p.into_path().ok()).map(|p| p.display().to_string())
    })
    .await
    .ok()
    .flatten()
}

/// The brief at `path`, checked: a local `.md` file within the size limit.
fn read_brief(path: &str) -> Result<(PathBuf, String), String> {
    let p = PathBuf::from(path.trim());
    if path.contains('\0') || !p.is_absolute() {
        return Err("A brief must be an absolute path.".into());
    }
    if path.starts_with("\\\\") || path.starts_with("//") {
        return Err("A brief must be a local file, not on a network share.".into());
    }
    if !p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")) {
        return Err("A brief must be a .md file.".into());
    }
    let meta = std::fs::metadata(&p).map_err(|_| "That brief doesn't exist, or can't be read.".to_owned())?;
    if !meta.is_file() {
        return Err("That path isn't a file.".into());
    }
    if meta.len() > core::MAX_BRIEF_BYTES as u64 {
        return Err(format!(
            "That brief is {} KB; the limit is {} KB. Every council member reads all of it in every round.",
            meta.len().div_ceil(1000),
            core::MAX_BRIEF_BYTES / 1000
        ));
    }
    let bytes = std::fs::read(&p).map_err(|e| format!("Couldn't read the brief: {e}"))?;
    let text = String::from_utf8(bytes).map_err(|_| "The brief isn't UTF-8 text.".to_owned())?;
    Ok((p, text))
}

/// Everything a brief must pass before a paid run: the shape checks, and no credential (a known
/// shape or any saved key's exact value). Returns its questions.
fn check_brief(store: &Store, text: &str) -> Result<Vec<String>, String> {
    let questions = brief::check(text, &chorus_core::scrub::builtin_patterns())?;
    let (secrets, failed) = crate::vault::all_for_scrubbing(store);
    if failed > 0 {
        return Err("Some saved keys couldn't be loaded, so Chorus can't check the brief for them. Nothing was sent.".into());
    }
    if secrets.iter().any(|s| !s.is_empty() && text.as_bytes().windows(s.len()).any(|w| w == s.as_slice())) {
        return Err("This brief contains one of your saved keys, so the run was refused before any model read it. Remove it and run again.".into());
    }
    Ok(questions)
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BriefCheck {
    pub path: String,
    pub questions: Vec<String>,
    /// Why it can't be run, or none.
    pub problem: Option<String>,
    /// Where the findings will be written.
    pub findings_path: String,
}

/// Reads a chosen brief and says what the council would be asked, before anything is spent.
#[tauri::command]
#[specta::specta]
pub fn council_brief_check(state: Manager<'_>, path: String) -> Result<BriefCheck, String> {
    let (p, text) = read_brief(&path)?;
    let checked = check_brief(state.store(), &text);
    Ok(BriefCheck {
        path: p.display().to_string(),
        questions: checked.clone().unwrap_or_default(),
        problem: checked.err(),
        findings_path: run::findings_path(&p).display().to_string(),
    })
}

fn project_folder(store: &Store, project_id: u32) -> Result<(projects::ProjectRow, PathBuf), String> {
    let project = projects::get(store, project_id)?.ok_or("That project no longer exists.")?;
    let folder = project
        .folder
        .clone()
        .filter(|f| !f.trim().is_empty())
        .ok_or("This project has no folder, so a council would have no code to read. Give it one in Settings → Projects.")?;
    let path = PathBuf::from(folder);
    if !path.is_dir() {
        return Err(format!("This project's folder ({}) doesn't exist.", path.display()));
    }
    Ok((project, path))
}

/// The folder a run reads: the one chosen (any existing folder, such as a redacted copy of the
/// project), or the project's own. The run is listed under `project_id` either way.
fn run_folder(store: &Store, project_id: u32, chosen: Option<&str>) -> Result<(projects::ProjectRow, PathBuf), String> {
    match chosen.map(str::trim).filter(|f| !f.is_empty()) {
        Some(f) => {
            let project = projects::get(store, project_id)?.ok_or("That project no longer exists.")?;
            let folder = crate::projects::resolve_folder(f).map_err(|e| format!("The folder to read: {e}"))?;
            Ok((project, PathBuf::from(folder)))
        }
        None => project_folder(store, project_id),
    }
}

#[tauri::command]
#[specta::specta]
pub fn council_start(state: Manager<'_>, council: Councils<'_>, project_id: u32, brief_path: String, folder: Option<String>) -> Result<u32, String> {
    let store = state.store();
    let (project, folder) = run_folder(store, project_id, folder.as_deref())?;
    let (brief_path, text) = read_brief(&brief_path)?;
    check_brief(store, &text)?;
    let cfg = config(store)?;
    let (provider, roster) = readiness(store, &cfg, &|r| local::problem(&state, r))?;
    let mut active = council.active.lock().map_err(|_| "Council state is unavailable.")?;
    let run_id = db::open_run(
        store,
        &db::NewRun {
            project_id,
            project_folder: &folder.display().to_string(),
            brief_path: &brief_path.display().to_string(),
            brief_text: &text,
            roster: &roster,
            limits_json: &serde_json::to_string(&cfg.limits).map_err(|e| e.to_string())?,
            provider_id: provider.as_ref().map(|p| p.id),
        },
    )?;
    let cancel = Arc::new(AtomicBool::new(false));
    *active = Some((run_id, cancel.clone()));
    drop(active);
    let ctx = run::Ctx {
        manager: state.inner().clone(),
        cancel,
        notify: council.notify.clone(),
        run_id,
        project_name: project.name,
        limits: cfg.limits,
        provider_id: provider.as_ref().map(|p| p.id),
    };
    let councils = council.inner().clone();
    let started = std::thread::Builder::new().name(format!("council-{run_id}")).spawn(move || {
        run::drive(ctx);
        if let Ok(mut a) = councils.active.lock()
            && a.as_ref().is_some_and(|(id, _)| *id == run_id)
        {
            *a = None;
        }
    });
    if let Err(e) = started {
        let _ = db::finish_run(store, run_id, RunStatus::Failed, Some(&format!("Couldn't start the run: {e}")));
        return Err(format!("Couldn't start the run: {e}"));
    }
    (council.notify)(run_id);
    Ok(run_id)
}

#[tauri::command]
#[specta::specta]
pub fn council_cancel(council: Councils<'_>, run_id: u32) -> Result<(), String> {
    let active = council.active.lock().map_err(|_| "Council state is unavailable.")?;
    match active.as_ref() {
        Some((id, cancel)) if *id == run_id => {
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
        _ => Err("That council isn't running.".into()),
    }
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocketRow {
    pub id: u32,
    pub brief_name: String,
    pub brief_path: String,
    pub status: RunStatus,
    pub reason: Option<String>,
    pub started_at: f64,
    pub ended_at: Option<f64>,
    pub turns: u32,
    pub tool_calls: u32,
    pub cost: Cost,
    /// "2 approved · 1 revise · 3 of 3 ruled · members split on 1", or none.
    pub digest: Option<String>,
    pub has_findings: bool,
    /// The folder the run read: the project's, or a copy chosen for it.
    pub folder: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DocketView {
    pub project_id: u32,
    pub project_name: String,
    /// The project's folder, which a new council reads unless another is chosen.
    pub folder: Option<String>,
    /// Why the project's own folder can't be read (so another must be chosen), or none.
    pub folder_problem: Option<String>,
    /// Why no council can run now (the settings), or none.
    pub reason: Option<String>,
    pub running: Option<u32>,
    pub runs: Vec<DocketRow>,
}

/// What a run cost, from two sources that are never merged: the per-response `usage.cost`
/// OpenRouter reports (summed), and the run key's spend from analytics after it was revoked.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Cost {
    pub reported_usd: Option<f64>,
    pub turns_reporting: u32,
    pub turns: u32,
    pub key_usd: Option<f64>,
    pub key_note: Option<String>,
    pub cap_usd: f64,
    pub tokens_in: Option<u32>,
    pub tokens_out: Option<u32>,
    /// Ended turns that ran on a local subscription: no dollar cost is reported for them (D140).
    pub subscription_turns: u32,
}

fn cost(store: &Store, run: &RunRow, turns: &[TurnRow]) -> Cost {
    let ended: Vec<&TurnRow> = turns.iter().filter(|t| t.outcome.is_some()).collect();
    let reporting: Vec<f64> = ended.iter().filter_map(|t| t.cost_usd).collect();
    let sum = |f: fn(&TurnRow) -> Option<u32>| {
        let v: Vec<u32> = turns.iter().filter_map(f).collect();
        (!v.is_empty()).then(|| v.iter().sum())
    };
    let key = run.key_ledger_id.and_then(|id| provider_rows::capped_key(store, id).ok().flatten());
    let limits: CouncilLimits = serde_json::from_str(&run.limits_json).unwrap_or_default();
    Cost {
        reported_usd: (!reporting.is_empty()).then(|| reporting.iter().sum()),
        turns_reporting: reporting.len() as u32,
        turns: ended.len() as u32,
        key_usd: key.as_ref().and_then(|k| k.cost_usd),
        key_note: key.and_then(|k| k.cost_note),
        cap_usd: limits.cap_usd,
        tokens_in: sum(|t| t.tokens_in),
        tokens_out: sum(|t| t.tokens_out),
        subscription_turns: ended
            .iter()
            .filter(|t| run.roster.iter().any(|r| r.id == t.member_id && r.runs_on.is_local()))
            .count() as u32,
    }
}

fn brief_name(path: &str) -> String {
    Path::new(path).file_name().map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned())
}

/// The arbiter's rulings next to the members' consensus, one row per question.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct VerdictRow {
    pub index: u32,
    pub question: String,
    pub consensus: Option<QuestionState>,
    pub ruling: Option<Ruling>,
}

fn verdicts(run: &RunRow, turns: &[TurnRow], summary: &[QuestionSummary]) -> (Vec<VerdictRow>, bool) {
    let questions = brief::parse_questions(&run.brief_text);
    let arbitration = turns
        .iter()
        .find(|t| t.phase == Phase::Arbitration && t.outcome == Some(TurnOutcome::Answered))
        .and_then(|t| t.content.as_deref())
        .map(verdict::parse_arbiter);
    let rows = questions
        .iter()
        .enumerate()
        .map(|(i, q)| VerdictRow {
            index: i as u32,
            question: verdict::clip(q, 200),
            consensus: summary.iter().find(|s| s.index as usize == i).map(|s| s.state),
            ruling: arbitration.as_ref().and_then(|a| a.rulings.get(&i).copied()),
        })
        .collect();
    (rows, arbitration.is_some_and(|a| a.block_present))
}

fn digest(rows: &[VerdictRow], arbiter_asked: bool) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if arbiter_asked {
        for r in Ruling::ALL {
            let n = rows.iter().filter(|v| v.ruling == Some(r)).count();
            if n > 0 {
                parts.push(format!("{n} {}", r.words()));
            }
        }
        let ruled = rows.iter().filter(|v| v.ruling.is_some()).count();
        parts.push(format!("{ruled} of {} ruled", rows.len()));
    }
    let split = rows.iter().filter(|v| matches!(v.consensus, Some(QuestionState::Split | QuestionState::Disagreed))).count();
    if split > 0 {
        parts.push(format!("members split on {split}"));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn summary_of(run: &RunRow) -> Vec<QuestionSummary> {
    run.summary_json.as_deref().and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default()
}

#[tauri::command]
#[specta::specta]
pub fn council_docket(state: Manager<'_>, project_id: u32) -> Result<DocketView, String> {
    let store = state.store();
    let project = projects::get(store, project_id)?.ok_or("That project no longer exists.")?;
    let folder_problem = project_folder(store, project_id).err();
    let reason = match config(store) {
        Ok(cfg) => readiness(store, &cfg, &|r| local::problem(&state, r)).err(),
        Err(e) => Some(e),
    };
    let runs = db::runs_for_project(store, project_id)?
        .into_iter()
        .map(|r| {
            let turns = db::turns(store, r.id)?;
            let (rows, asked) = verdicts(&r, &turns, &summary_of(&r));
            Ok(DocketRow {
                id: r.id,
                brief_name: brief_name(&r.brief_path),
                brief_path: r.brief_path.clone(),
                status: r.status,
                reason: r.reason.clone(),
                started_at: r.started_at as f64,
                ended_at: r.ended_at.map(|t| t as f64),
                turns: turns.len() as u32,
                tool_calls: turns.iter().map(|t| t.tool_calls).sum(),
                cost: cost(store, &r, &turns),
                digest: digest(&rows, asked),
                has_findings: r.findings_path.is_some(),
                folder: r.project_folder.clone(),
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(DocketView {
        project_id,
        project_name: project.name,
        folder: project.folder,
        folder_problem,
        reason,
        running: db::running(store)?,
        runs,
    })
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TurnView {
    pub turn: TurnRow,
    pub label: String,
    pub model: String,
    /// While in flight: the last tool call it made.
    pub doing: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CouncilRunView {
    pub id: u32,
    pub project_id: u32,
    pub brief_name: String,
    pub brief_path: String,
    pub status: RunStatus,
    pub reason: Option<String>,
    pub phase: Phase,
    pub roster: Vec<RosterEntry>,
    pub limits: CouncilLimits,
    pub questions: Vec<String>,
    pub turns: Vec<TurnView>,
    pub summary: Vec<QuestionSummary>,
    pub verdicts: Vec<VerdictRow>,
    pub arbiter_asked: bool,
    pub findings_path: Option<String>,
    /// The folder the run read.
    pub folder: String,
    pub findings_text: Option<String>,
    pub findings_problem: Option<String>,
    pub cost: Cost,
    pub started_at: f64,
    pub ended_at: Option<f64>,
}

#[tauri::command]
#[specta::specta]
pub fn council_run(state: Manager<'_>, run_id: u32) -> Result<CouncilRunView, String> {
    let store = state.store();
    let run = db::run(store, run_id)?.ok_or("That council run no longer exists.")?;
    let turns = db::turns(store, run_id)?;
    let summary = summary_of(&run);
    let (rows, asked) = verdicts(&run, &turns, &summary);
    let phase = if run.status == RunStatus::Complete {
        Phase::Done
    } else {
        [Phase::Synthesis, Phase::Arbitration, Phase::Critique, Phase::Positions]
            .into_iter()
            .find(|p| turns.iter().any(|t| t.phase == *p))
            .unwrap_or(Phase::Positions)
    };
    let views = turns
        .iter()
        .map(|t| {
            let who = run.roster.iter().find(|r| r.id == t.member_id);
            Ok(TurnView {
                turn: t.clone(),
                label: who.map_or_else(|| format!("member {}", t.member_id), |r| r.label.clone()),
                model: who.map(|r| r.model.clone()).unwrap_or_default(),
                doing: if t.outcome.is_none() { db::last_tool_call(store, t.id)? } else { None },
            })
        })
        .collect::<Result<_, String>>()?;
    let (findings_text, findings_problem) = run::read_findings(&run);
    Ok(CouncilRunView {
        id: run.id,
        project_id: run.project_id,
        brief_name: brief_name(&run.brief_path),
        brief_path: run.brief_path.clone(),
        status: run.status,
        reason: run.reason.clone(),
        phase,
        roster: run.roster.clone(),
        limits: serde_json::from_str(&run.limits_json).unwrap_or_default(),
        questions: brief::parse_questions(&run.brief_text),
        cost: cost(store, &run, &turns),
        turns: views,
        summary,
        verdicts: rows,
        arbiter_asked: asked,
        findings_path: run.findings_path.clone(),
        folder: run.project_folder.clone(),
        findings_text,
        findings_problem,
        started_at: run.started_at as f64,
        ended_at: run.ended_at.map(|t| t as f64),
    })
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TurnDetail {
    pub prompt: String,
    pub tool_calls: Vec<ToolCallRow>,
}

#[tauri::command]
#[specta::specta]
pub fn council_turn(state: Manager<'_>, turn_id: u32) -> Result<TurnDetail, String> {
    let store = state.store();
    let (_, prompt) = db::turn_prompt(store, turn_id)?.ok_or("That turn no longer exists.")?;
    Ok(TurnDetail { prompt, tool_calls: db::tool_calls(store, turn_id)? })
}

/// Removes a run from the Docket. The findings file beside the brief is left alone.
#[tauri::command]
#[specta::specta]
pub fn council_run_delete(state: Manager<'_>, run_id: u32) -> Result<(), String> {
    if !db::delete_run(state.store(), run_id)? {
        return Err("That run is still going, or was already removed.".into());
    }
    Ok(())
}

/// Opens the run's findings file in the default app.
#[tauri::command]
#[specta::specta]
pub fn council_findings_open(app: tauri::AppHandle, state: Manager<'_>, run_id: u32) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let run = db::run(state.store(), run_id)?.ok_or("That council run no longer exists.")?;
    let path = run.findings_path.ok_or("This run wrote no findings file.")?;
    if !Path::new(&path).is_file() {
        return Err(format!("The findings file is no longer at {path}."));
    }
    app.opener().open_path(path, None::<&str>).map_err(|e| format!("Opening the findings: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_counts_rulings_and_splits() {
        let row = |ruling, consensus| VerdictRow { index: 0, question: String::new(), consensus, ruling };
        let rows = vec![
            row(Some(Ruling::Approved), Some(QuestionState::Agreed)),
            row(Some(Ruling::Revise), Some(QuestionState::Split)),
            row(None, None),
        ];
        assert_eq!(digest(&rows, true).as_deref(), Some("1 approved · 1 revise · 2 of 3 ruled · members split on 1"));
        assert_eq!(digest(&rows, false).as_deref(), Some("members split on 1"));
        assert_eq!(digest(&[], false), None);
    }

    #[test]
    fn readiness_explains_itself() {
        let store = Store::in_memory();
        let cfg = CouncilConfig::default();
        // No members: nothing needs OpenRouter, so the roster is what's missing.
        assert!(readiness(&store, &cfg, &|_| None).unwrap_err().contains("no arbiter"));
        let add = |label: &str, model: &str, role, runs_on| db::save_member(&store, None, label, model, role, None, runs_on).unwrap();
        add("Opus", "claude-opus-5-5", CouncilRole::Arbiter, RunsOn::Claude);
        add("Astra", "gpt-6-astra", CouncilRole::Member, RunsOn::Codex);
        add("Sonnet", "claude-sonnet-5-5", CouncilRole::Member, RunsOn::Claude);
        // Every participant local: no provider is needed, and the roster says where each runs.
        let (provider, roster) = readiness(&store, &cfg, &|_| None).unwrap();
        assert!(provider.is_none());
        assert_eq!(roster.iter().map(|r| r.runs_on).collect::<Vec<_>>(), [RunsOn::Codex, RunsOn::Claude, RunsOn::Claude]);
        assert_eq!(roster.last().map(|r| r.label.as_str()), Some("Opus"));
        // A local CLI that isn't found stops the run, by name.
        let missing = |r: RunsOn| (r == RunsOn::Codex).then(|| "Codex isn't available: not found".to_owned());
        assert_eq!(readiness(&store, &cfg, &missing).unwrap_err(), "Council member 'Astra': Codex isn't available: not found");
        // One OpenRouter member brings the provider back as a requirement.
        add("Grok", "x-ai/grok-4.7", CouncilRole::Member, RunsOn::Openrouter);
        assert!(readiness(&store, &cfg, &|_| None).unwrap_err().contains("Choose the Council's OpenRouter provider"));
    }

    #[test]
    fn a_run_reads_the_chosen_folder_or_the_projects_own() {
        use crate::store::projects::NewProject;
        let store = Store::in_memory();
        let root = std::env::temp_dir().join(format!("chorus-council-folder-{}", std::process::id()));
        let (own, copy, gone) = (root.join("app"), root.join("app-redacted"), root.join("moved"));
        std::fs::create_dir_all(&own).unwrap();
        std::fs::create_dir_all(&gone).unwrap();
        std::fs::create_dir_all(&copy).unwrap();
        let own_text = own.display().to_string();
        let with = projects::insert(&store, &NewProject { name: "Atlas", folder: Some(&own_text), colour: "#336699" }).unwrap();
        let gone_text = gone.display().to_string();
        let moved = projects::insert(&store, &NewProject { name: "Moved", folder: Some(&gone_text), colour: "#336699" }).unwrap();
        std::fs::remove_dir_all(&gone).unwrap();

        let same = |p: &Path, q: &Path| p.canonicalize().unwrap() == q.canonicalize().unwrap();
        assert!(same(&run_folder(&store, with, None).unwrap().1, &own));
        assert!(same(&run_folder(&store, with, Some("  ")).unwrap().1, &own));
        assert!(same(&run_folder(&store, with, copy.to_str()).unwrap().1, &copy));
        // A project whose folder has gone can still run on a chosen one.
        assert!(run_folder(&store, moved, None).unwrap_err().contains("doesn't exist"));
        assert!(same(&run_folder(&store, moved, copy.to_str()).unwrap().1, &copy));
        // A chosen folder must exist and be a full path.
        assert!(run_folder(&store, with, root.join("missing").to_str()).unwrap_err().starts_with("The folder to read"));
        assert!(run_folder(&store, with, Some("relative\\dir")).is_err());
        assert!(run_folder(&store, 9999, copy.to_str()).unwrap_err().contains("no longer exists"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn brief_paths_are_checked() {
        assert!(read_brief("relative.md").unwrap_err().contains("absolute"));
        assert!(read_brief("\\\\server\\share\\b.md").unwrap_err().contains("network"));
        assert!(read_brief("C:\\nowhere\\b.txt").unwrap_err().contains(".md"));
        assert!(read_brief("C:\\nowhere\\missing.md").unwrap_err().contains("doesn't exist"));
    }
}
