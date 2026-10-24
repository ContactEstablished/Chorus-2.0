//! One small OpenRouter call per completed turn. Never changes agent state or sends a prompt.
use crate::{
    sessions::SessionManager,
    store::{Store, narrator as db, providers, sessions, settings},
    vault,
};
use chorus_core::{
    narrator::{self as core, NarratorConfig},
    providers::ProviderKind,
    scrub,
};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use serde_json::{Value, json};
use specta::Type;
use std::{sync::Arc, time::Duration};
use tauri::State;

type Manager<'a> = State<'a, Arc<SessionManager>>;
const CONFIG: &str = "narrator.config";

fn config(store: &Store) -> Result<NarratorConfig, String> {
    settings::get(store, CONFIG)?
        .map(|s| {
            serde_json::from_str(&s).map_err(|_| "Narrator settings could not be read.".into())
        })
        .unwrap_or(Ok(NarratorConfig::default()))
}

fn available(store: &Store, cfg: &NarratorConfig, project: Option<u32>) -> Result<u32, String> {
    if !db::enabled(store, project)? {
        return Err("Narrator is off for this project.".into());
    }
    let id = cfg
        .provider_id
        .ok_or("Choose an OpenRouter provider and narrator model in Settings → Narrator.")?;
    if cfg.model.is_empty() {
        return Err("Choose a narrator model in Settings → Narrator.".into());
    }
    let p = providers::get(store, id)?
        .ok_or("The narrator provider was deleted. Choose another in Settings → Narrator.")?;
    if p.kind != ProviderKind::Openrouter {
        return Err("Narrator needs an OpenRouter provider.".into());
    }
    let key=p.credential_id.ok_or("Add an API key to the narrator's OpenRouter provider. A management key cannot generate summaries.")?;
    if crate::store::credentials::blob(store, key)?.is_none() {
        return Err("The narrator API key was deleted.".into());
    }
    Ok(key)
}

#[derive(Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NarratorSettings {
    pub config: NarratorConfig,
    pub reason: Option<String>,
    pub costs: db::NarratorCosts,
    pub projects: Vec<NarratorProject>,
}

#[derive(Serialize, Type)]
pub struct NarratorProject {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
}

#[derive(Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NarratorView {
    pub enabled: bool,
    pub reason: Option<String>,
    pub job: Option<db::NarratorJob>,
    pub actions: Vec<db::NarratorAction>,
    pub costs: db::NarratorCosts,
    pub latest_turn: Option<u32>,
    pub can_summarize: bool,
}

#[tauri::command]
#[specta::specta]
pub fn narrator_settings(state: Manager<'_>) -> Result<NarratorSettings, String> {
    let cfg = config(state.store())?;
    let projects = crate::store::projects::list(state.store())?
        .into_iter()
        .map(|p| {
            Ok(NarratorProject {
                id: p.id,
                name: p.name,
                enabled: db::enabled(state.store(), Some(p.id))?,
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(NarratorSettings {
        reason: available(state.store(), &cfg, None).err(),
        config: cfg,
        costs: db::costs(state.store(), None)?,
        projects,
    })
}

#[tauri::command]
#[specta::specta]
pub fn narrator_configure(state: Manager<'_>, config: NarratorConfig) -> Result<(), String> {
    if !config.model.is_empty() && !chorus_core::agent::valid_model(&config.model) {
        return Err("Invalid narrator model ID.".into());
    }
    if let Some(id) = config.provider_id {
        let p = providers::get(state.store(), id)?.ok_or("Provider no longer exists.")?;
        if p.kind != ProviderKind::Openrouter {
            return Err("Choose an OpenRouter provider.".into());
        }
    }
    settings::set(
        state.store(),
        CONFIG,
        &serde_json::to_string(&config).map_err(|e| e.to_string())?,
    )
}

#[tauri::command]
#[specta::specta]
pub fn narrator_project_set(
    state: Manager<'_>,
    project_id: u32,
    enabled: bool,
) -> Result<(), String> {
    if crate::store::projects::get(state.store(), project_id)?.is_none() {
        return Err("Project no longer exists.".into());
    }
    state.store().with(|c|c.execute("INSERT INTO narrator_projects(project_id,enabled) VALUES(?1,?2) ON CONFLICT(project_id) DO UPDATE SET enabled=excluded.enabled",params![project_id,enabled]).map(|_|()))
}

pub fn view(store: &Store, id: u32) -> Result<NarratorView, String> {
    let s = sessions::get(store, id)?.ok_or("Session no longer exists.")?;
    let reason = available(store, &config(store)?, s.project_id).err();
    let latest=store.with(|c|c.query_row("SELECT id,ended_at IS NOT NULL AND end_reason IN ('done','failed') FROM turns WHERE session_id=?1 ORDER BY id DESC LIMIT 1",[id],|r|Ok((r.get::<_,u32>(0)?,r.get::<_,bool>(1)?))).optional())?;
    let job = db::latest(store, id)?;
    let can_summarize = reason.is_none()
        && latest.is_some_and(|(t, ended)| {
            ended
                && job
                    .as_ref()
                    .is_none_or(|j| j.turn_id != t || j.status == "skipped")
        });
    // Generic next steps only make sense once a turn has ended (D82: turn-scoped); a new session
    // has nothing to continue or review yet.
    if latest.is_some_and(|(_, ended)| ended) {
        db::ensure_fallback(store, id)?;
    }
    Ok(NarratorView {
        enabled: db::enabled(store, s.project_id)?,
        reason,
        job,
        actions: db::actions(store, id)?,
        costs: db::costs(store, Some(id))?,
        latest_turn: latest.map(|t| t.0),
        can_summarize,
    })
}

#[tauri::command]
#[specta::specta]
pub fn session_narrator(state: Manager<'_>, id: u32) -> Result<NarratorView, String> {
    view(state.store(), id)
}

#[tauri::command]
#[specta::specta]
pub fn narrator_summarize(state: Manager<'_>, id: u32, turn_id: u32) -> Result<(), String> {
    let v = view(state.store(), id)?;
    if !v.can_summarize || v.latest_turn != Some(turn_id) {
        return Err(
            "This turn cannot be summarized again. Wait for the next completed turn.".into(),
        );
    }
    db::queue(state.store(), id, turn_id)
}

#[tauri::command]
#[specta::specta]
pub fn narrator_action_set(
    state: Manager<'_>,
    id: u32,
    action_id: u32,
    disposition: String,
) -> Result<(), String> {
    db::disposition(state.store(), id, action_id, &disposition)
}

fn evidence(
    store: &Store,
    session: u32,
    turn: u32,
    secrets: &[Vec<u8>],
    patterns: &[scrub::Pattern],
) -> Result<Value, String> {
    // Redact BEFORE clipping: a secret crossing the excerpt boundary must not leak a prefix.
    let clip = |s: &str, n| core::scrubbed_excerpt(s, n, secrets, patterns);
    let t = crate::store::history::recent_turns(store, session, 100)?
        .into_iter()
        .find(|t| t.id == turn)
        .ok_or("Turn is outside the narrator's recent history window.")?;
    let row = sessions::get(store, session)?.ok_or("Session no longer exists.")?;
    let plan = t.progress.iter().rev().find_map(|p| match &p.content {
        chorus_core::progress::Progress::Plan { steps } => Some(
            steps
                .iter()
                .take(20)
                .map(|s| json!({"status":s.status,"text":clip(&s.text,200)}))
                .collect::<Vec<_>>(),
        ),
        _ => None,
    });
    let tools=t.tools.iter().rev().take(25).map(|t|json!({"tool":clip(&t.tool,80),"target":t.target.as_deref().map(|s|clip(s,160)),"failed":t.failed,"finished":t.finished})).collect::<Vec<_>>();
    let feedback=db::actions(store,session)?.into_iter().filter(|a|a.disposition!="open").take(20).map(|a|json!({"status":a.disposition,"what":clip(&a.suggestion.what,160),"sends":clip(&a.suggestion.sends,200)})).collect::<Vec<_>>();
    let diff = crate::git::snapshot(std::path::Path::new(&row.cwd))
        .ok()
        .map(|s| json!({"changes":s.changes,"diff":s.diff}));
    Ok(
        json!({"prompt":t.prompt.as_deref().map(|s|clip(s,3000)),"final_message":t.final_message.as_deref().map(|s|clip(s,6000)),"end_reason":t.end_reason,"plan":plan,"tools":tools,"current_diff_stat":diff,"feedback":feedback}),
    )
}

fn call(key: &str, body: Value) -> Result<Value, String> {
    // No redirects, no provider-error bodies in logs/UI, bounded streaming read, no HTTP retries.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(40))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "Could not create narrator connection.")?;
    tauri::async_runtime::block_on(async {
        let mut r=client.post(format!("{}/chat/completions",chorus_core::providers::OPENROUTER_API)).bearer_auth(key).json(&body).send().await.map_err(|_|"Narrator connection failed or timed out. This turn will not be retried automatically.")?;
        if !r.status().is_success() {
            return Err(core::provider_failure(r.status().as_u16()));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = r
            .chunk()
            .await
            .map_err(|_| "Narrator response was interrupted.")?
        {
            if bytes.len() + chunk.len() > 128 * 1024 {
                return Err("Narrator response exceeded its size limit.".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes)
            .map_err(|_| "OpenRouter returned an unreadable response.".into())
    })
}

fn process(store: &Store, session: u32, turn: u32) -> Result<(), String> {
    let cfg = config(store)?;
    let row = sessions::get(store, session)?.ok_or("Session no longer exists.")?;
    let preparation = (|| {
        let credential = available(store, &cfg, row.project_id)?;
        let key = vault::decrypt(store, credential)?;
        let (secrets, failed) = vault::all_for_scrubbing(store);
        if failed > 0 {
            return Err::<_, String>(
                "Narrator paused: some saved keys could not be loaded for redaction.".into(),
            );
        }
        let patterns = scrub::builtin_patterns();
        let evidence = scrub::scrub_json(
            &evidence(store, session, turn, &secrets, &patterns)?,
            &secrets,
            &patterns,
        );
        Ok((key, secrets, patterns, evidence))
    })();
    let (key, secrets, patterns, evidence) = match preparation {
        Ok(p) => p,
        Err(e) => {
            store.with(|c|c.execute("UPDATE narrator_jobs SET status='skipped',note=?2 WHERE turn_id=?1 AND status='queued'",params![turn,e]))?;
            return Ok(());
        }
    };
    // Evidence preparation can take time. Recheck immediately before the paid attempt.
    let current_project = sessions::get(store, session)?
        .ok_or("Session no longer exists.")?
        .project_id;
    if config(store)? != cfg || available(store, &cfg, current_project).is_err() {
        store.with(|c| c.execute("UPDATE narrator_jobs SET status='skipped',note='Narrator settings changed before this call. Summarize the latest turn when ready.' WHERE turn_id=?1 AND status='queued'",[turn]))?;
        return Ok(());
    }
    if !db::claim(store, turn, &cfg.model)? {
        return Ok(());
    }
    let response = call(&key, core::request(&cfg.model, evidence));
    let (result, cost, tokens) = match response {
        Ok(v) => {
            let cost = core::reported_cost(&v);
            let num = |p: &str| {
                v.pointer(p)
                    .and_then(Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
            };
            let result = v
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
                .ok_or_else(|| "Narrator returned no summary.".to_owned())
                .and_then(core::parse)
                .and_then(|n| {
                    let clean =
                        scrub::scrub_json(&serde_json::to_value(n).unwrap(), &secrets, &patterns);
                    core::parse(&clean.to_string())
                });
            (
                result,
                cost,
                (num("/usage/prompt_tokens"), num("/usage/completion_tokens")),
            )
        }
        Err(e) => (Err(e), None, (None, None)),
    };
    db::finish(
        store,
        turn,
        session,
        result.as_ref().map_err(String::as_str),
        cost,
        tokens,
    )
}

pub fn start(manager: &Arc<SessionManager>) -> Result<(), String> {
    db::recover(manager.store())?;
    let weak = Arc::downgrade(manager);
    std::thread::Builder::new().name("narrator".into()).spawn(move || loop {
        std::thread::sleep(Duration::from_millis(1000));
        let Some(m)=weak.upgrade() else {break};
        if m.is_quitting() {break;}
        let pending=m.store().with(|c|c.query_row("SELECT session_id,turn_id FROM narrator_jobs WHERE status='queued' AND created_at<=?1 ORDER BY turn_id LIMIT 1",[crate::store::now_ms()-1500],|r|Ok((r.get::<_,u32>(0)?,r.get::<_,u32>(1)?))).optional());
        if let Ok(Some((s,t)))=pending && process(m.store(),s,t).is_err() {
            let _=m.store().with(|c|c.execute("UPDATE narrator_jobs SET status='failed',note='Narrator could not store or prepare this summary.' WHERE turn_id=?1 AND status IN ('queued','attempting')",[t]));
        }
    }).map(|_|()).map_err(|e|format!("Could not start narrator: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_next_steps_wait_for_an_ended_turn() {
        let store = Store::in_memory();
        store
            .with(|c| c.execute_batch("INSERT INTO sessions(id,name,agent,cwd,state,cols,rows,created_at) VALUES(1,'n','claude','.','running',80,24,1);
                INSERT INTO runs(id,session_id,program,args,started_at) VALUES(1,1,'claude','[]',1);"))
            .unwrap();
        assert!(view(&store, 1).unwrap().actions.is_empty(), "a new session has nothing to continue");
        store.with(|c| c.execute_batch("INSERT INTO turns(id,session_id,run_id,started_at) VALUES(1,1,1,2);")).unwrap();
        assert!(view(&store, 1).unwrap().actions.is_empty(), "a turn still running has no next steps yet");
        store.with(|c| c.execute_batch("UPDATE turns SET ended_at=3,end_reason='done' WHERE id=1;")).unwrap();
        assert!(!view(&store, 1).unwrap().actions.is_empty());
    }
}
