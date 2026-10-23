//! One bounded OpenRouter summary per completed turn, with persistent, visible fallback.
use crate::{
    sessions::SessionManager,
    store::{self, Store, narrator as rows},
    vault,
};
use chorus_core::narrator::{self as core, Config};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tauri::State;

const SETTINGS_KEY: &str = "narrator";

fn config(store: &Store) -> Result<Config, String> {
    store::settings::get(store, SETTINGS_KEY)?
        .map(|s| {
            serde_json::from_str(&s)
                .map_err(|_| "Narrator settings are unreadable; save them again.".into())
        })
        .unwrap_or_else(|| Ok(Config::default()))
}

fn unavailable(
    store: &Store,
    project: Option<u32>,
    config: &Config,
) -> Result<Option<String>, String> {
    if let Some(id) = project
        && store::settings::get(store, &format!("narrator.project.{id}"))?.as_deref() == Some("off")
    {
        return Ok(Some(
            "Narrator is off for this project. Continue and Review are generic drafts.".into(),
        ));
    }
    let Some(key) = config.credential_id else {
        return Ok(Some("No narrator key is configured. Choose an OpenRouter key and model in Settings → Narrator. Continue and Review are generic drafts.".into()));
    };
    if store::credentials::get(store, key)?.is_none() {
        return Ok(Some(
            "The narrator key was deleted. Choose a key in Settings → Narrator.".into(),
        ));
    }
    core::validate_config(config)?;
    Ok(None)
}

pub fn start(manager: Arc<SessionManager>) -> Result<(), String> {
    rows::abandon_pending(manager.store())?;
    let weak = Arc::downgrade(&manager);
    std::thread::Builder::new().name("narrator".into()).spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(750));
            let Some(manager)=weak.upgrade() else { break; };
            match rows::claim(manager.store()) {
                Ok(Some((session,turn))) => {
                    if let Err(reason)=summarise(&manager,session,turn) {
                        let _=rows::finish(manager.store(),session,turn,&core::fallback(),Some(&reason),None);
                    }
                }
                Ok(None) => {},
                Err(_) => { /* Reads report store errors through the panel; don't log private content. */ }
            }
        }
    }).map_err(|_|"Couldn't start the narrator worker.".to_owned())?;
    Ok(())
}

/// Only use a complete snapshot attributed to the turn being summarised.
fn plan_input(store: &Store, session: u32, turn: u32) -> Result<String, String> {
    let run: Option<u32> = store.with(|c| c.query_row(
        "SELECT run_id FROM turns WHERE id=?1 AND session_id=?2",
        rusqlite::params![turn, session], |r| r.get(0),
    ))?;
    let plan = store::progress::latest_plan(store, session)?;
    match plan {
        Some(plan) if Some(plan.run_id) == run && plan.turn_id == Some(turn) && !plan.incomplete =>
            serde_json::to_string(&plan.steps).map_err(|_| "The recorded plan could not be read.".into()),
        _ => Ok("No complete structured plan was captured for this turn. Do not infer a plan from earlier runs or turns.".into()),
    }
}

/// Recheck immediately before HTTP after Git/DB IO, including a project reassignment.
fn still_enabled(
    store: &Store,
    session: u32,
    turn: u32,
    original_project: Option<u32>,
    original_config: &Config,
) -> Result<(), String> {
    let current = store::sessions::get(store, session)?.ok_or("Session no longer exists.")?;
    if current.project_id != original_project || config(store)? != *original_config {
        return Err(
            "Narrator settings or the session's project changed. No request sent for this turn."
                .into(),
        );
    }
    if let Some(reason) = unavailable(store, current.project_id, original_config)? {
        return Err(reason);
    }
    let current_turn=store.with(|c|c.query_row("SELECT EXISTS(SELECT 1 FROM turns t WHERE t.id=?1 AND t.session_id=?2 AND t.end_reason='done' AND t.id=(SELECT MAX(id) FROM turns WHERE session_id=?2) AND t.run_id IS (SELECT MAX(id) FROM runs WHERE session_id=?2))",rusqlite::params![turn,session],|r|r.get::<_,bool>(0)))?;
    if !current_turn {
        return Err("A newer turn or run superseded this summary. No request sent.".into());
    }
    Ok(())
}

fn summarise(manager: &SessionManager, session: u32, turn: u32) -> Result<(), String> {
    summarise_with(manager, session, turn, call)
}

fn summarise_with(
    manager: &SessionManager,
    session: u32,
    turn: u32,
    transport: impl FnOnce(&str, String) -> Result<String, String>,
) -> Result<(), String> {
    let store = manager.store();
    let row = store::sessions::get(store, session)?.ok_or("Session no longer exists.")?;
    let config = config(store)?;
    if let Some(reason) = unavailable(store, row.project_id, &config)? {
        return Err(reason);
    }
    // A newer turn makes a delayed result stale. Never charge for stale backlog.
    let history = store::history::recent_turns(store, session, 1)?;
    let last = history
        .last()
        .filter(|t| t.id == turn && t.end_reason.as_deref() == Some("done"))
        .ok_or(
            "A newer or incomplete turn superseded this summary. No narrator request was sent.",
        )?;
    let keys = store::credentials::all_blobs(store)?
        .into_iter()
        .map(|(_, sealed)| {
            vault::unprotect(&sealed).map_err(|_| {
                "Narrator paused: a stored key could not be loaded for scrubbing.".to_owned()
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let patterns = chorus_core::scrub::builtin_patterns();
    let clean = |text: &str| {
        String::from_utf8_lossy(&chorus_core::scrub::scrub_all(
            text.as_bytes(),
            &keys,
            &patterns,
        ))
        .into_owned()
    };
    let tool_text = last
        .tools
        .iter()
        .rev()
        .take(32)
        .map(|t| {
            format!(
                "{} {}: {}",
                t.tool,
                t.target.as_deref().unwrap_or(""),
                if !t.finished {
                    "unfinished"
                } else if t.failed {
                    "failed"
                } else {
                    "finished"
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let diff = match manager.session_git_status(session) {
        Ok(Some(diff)) => {
            serde_json::to_string(&diff).unwrap_or_else(|_| "Diff unavailable".into())
        }
        Ok(None) => "Not a Git repository".into(),
        Err(_) => "Diff unavailable".into(),
    };
    let clean_json = |text: &str| -> String {
        match serde_json::from_str::<Value>(text) {
            Ok(value) => chorus_core::scrub::scrub_json(&value, &keys, &patterns).to_string(),
            Err(_) => clean(text),
        }
    };
    let body = core::request(
        &config.model,
        core::Input {
            prompt: &clean(last.prompt.as_deref().unwrap_or("Prompt unavailable")),
            final_message: &clean(
                last.final_message
                    .as_deref()
                    .unwrap_or("Final message unavailable"),
            ),
            plan: &clean_json(&plan_input(store, session, turn)?),
            tools: &clean(&tool_text),
            diff: &clean_json(&diff),
            resolved: &clean_json(&rows::resolved(store, session)?),
        },
    );
    let key = vault::decrypt(
        store,
        config.credential_id.ok_or("No narrator key configured.")?,
    )?;
    still_enabled(store, session, turn, row.project_id, &config)?;
    // Commit uncertainty before HTTP. Neither a timeout nor a crash can retry this turn.
    rows::requested(store, turn, &config.model)?;
    let response = transport(&key, body)?;
    let value: Value =
        serde_json::from_str(&response).map_err(|_| "Narrator response was not JSON.")?;
    let cost = core::cost(&value);
    if still_enabled(store, session, turn, row.project_id, &config).is_err() {
        return rows::finish(store, session, turn, &core::fallback(),
            Some("The session or narrator settings changed while the summary was being generated. Its cost is recorded; its suggestions are not current."), cost);
    }
    let summary = value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| "Narrator returned no summary text.".to_owned())
        .and_then(|text| core::parse_summary(&clean_json(text)));
    match summary {
        Ok(summary) => rows::finish(store, session, turn, &summary, None, cost),
        Err(reason) => rows::finish(store, session, turn, &core::fallback(), Some(&reason), cost),
    }
}

fn call(key: &str, body: String) -> Result<String, String> {
    tauri::async_runtime::block_on(async {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(25))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "Couldn't create narrator HTTP client.")?;
        let mut response = client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .bearer_auth(key)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(
                |_| "Narrator request failed or timed out; it will not be retried for this turn.",
            )?;
        if !response.status().is_success() {
            return Err(format!(
                "OpenRouter narrator returned HTTP {}. No retry for this turn.",
                response.status().as_u16()
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Narrator response was interrupted.")?
        {
            if bytes.len() + chunk.len() > 64 * 1024 {
                return Err("Narrator response exceeded 64 KiB.".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        String::from_utf8(bytes).map_err(|_| "Narrator response was not UTF-8.".into())
    })
}

#[tauri::command]
#[specta::specta]
pub fn narrator_config(manager: State<'_, Arc<SessionManager>>) -> Result<Config, String> {
    config(manager.store())
}

#[tauri::command]
#[specta::specta]
pub fn narrator_config_save(
    manager: State<'_, Arc<SessionManager>>,
    input: Config,
) -> Result<(), String> {
    core::validate_config(&input)?;
    if let Some(key) = input.credential_id
        && store::credentials::get(manager.store(), key)?.is_none()
    {
        return Err("That key no longer exists.".into());
    }
    store::settings::set(
        manager.store(),
        SETTINGS_KEY,
        &serde_json::to_string(&input).map_err(|_| "Invalid narrator settings.")?,
    )
}

#[tauri::command]
#[specta::specta]
pub fn narrator_project_enabled(
    manager: State<'_, Arc<SessionManager>>,
    project_id: u32,
) -> Result<bool, String> {
    if store::projects::get(manager.store(), project_id)?.is_none() {
        return Err("Project no longer exists.".into());
    }
    Ok(
        store::settings::get(manager.store(), &format!("narrator.project.{project_id}"))?
            .as_deref()
            != Some("off"),
    )
}

#[tauri::command]
#[specta::specta]
pub fn narrator_project_set(
    manager: State<'_, Arc<SessionManager>>,
    project_id: u32,
    enabled: bool,
) -> Result<(), String> {
    if store::projects::get(manager.store(), project_id)?.is_none() {
        return Err("Project no longer exists.".into());
    }
    store::settings::set(
        manager.store(),
        &format!("narrator.project.{project_id}"),
        if enabled { "on" } else { "off" },
    )
}

#[tauri::command]
#[specta::specta]
pub fn narrator_panel(
    manager: State<'_, Arc<SessionManager>>,
    session_id: u32,
) -> Result<rows::Panel, String> {
    let row =
        store::sessions::get(manager.store(), session_id)?.ok_or("Session no longer exists.")?;
    rows::panel(
        manager.store(),
        session_id,
        unavailable(manager.store(), row.project_id, &config(manager.store())?)?,
    )
}

#[tauri::command]
#[specta::specta]
pub fn narrator_resolve(
    manager: State<'_, Arc<SessionManager>>,
    session_id: u32,
    action_id: u32,
    disposition: rows::Disposition,
) -> Result<(), String> {
    if store::sessions::get(manager.store(), session_id)?.is_none() {
        return Err("Session no longer exists.".into());
    }
    rows::resolve(manager.store(), session_id, action_id, disposition)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn narrator_uses_only_complete_same_turn_plan() {
        use chorus_core::progress::{PlanChange, PlanStatus};
        let store = Store::in_memory();
        store.with(|c| {
            c.execute("INSERT INTO sessions(id,name,agent,cwd,state,cols,rows,created_at) VALUES(1,'test','claude','scratch','exited',80,24,1)", [])?;
            c.execute("INSERT INTO runs(id,session_id,program,args,started_at) VALUES(1,1,'test','[]',1)", [])?;
            Ok(())
        }).unwrap();
        let turn = store::history::open_turn(&store, 1, Some(1), None, None).unwrap() as u32;
        assert!(plan_input(&store, 1, turn).unwrap().starts_with("No complete"));
        store::progress::record_plan(&store, 1, 1, Some(turn.into()), "task-1", PlanChange::Upsert {
            id: "1".into(), title: Some("Verify the greeting".into()), status: Some(PlanStatus::Completed),
        }).unwrap();
        assert!(plan_input(&store, 1, turn).unwrap().contains("Verify the greeting"));
        let next = store::history::open_turn(&store, 1, Some(1), None, None).unwrap() as u32;
        assert!(plan_input(&store, 1, next).unwrap().starts_with("No complete"));
        store::progress::mark_plan_incomplete(&store, 1).unwrap();
        assert!(plan_input(&store, 1, turn).unwrap().starts_with("No complete"));
    }
    #[test]
    fn last_moment_gate_refuses_project_change_disable_and_new_turn() {
        let store = Store::in_memory();
        store.with(|c|c.execute("INSERT INTO sessions(id,name,agent,cwd,state,cols,rows,created_at) VALUES(1,'test','claude','scratch','exited',80,24,1)",[])).unwrap();
        let turn = store::history::open_turn(&store, 1, None, None, None).unwrap() as u32;
        store::history::close_turn(&store, turn.into(), "done", Some("Done")).unwrap();
        let key = store::credentials::insert(&store, "fixture", b"sealed", "fingerprint").unwrap();
        let settings = Config {
            credential_id: Some(key),
            model: "fixture/model".into(),
        };
        store::settings::set(
            &store,
            SETTINGS_KEY,
            &serde_json::to_string(&settings).unwrap(),
        )
        .unwrap();
        assert!(still_enabled(&store, 1, turn, None, &settings).is_ok());
        assert!(
            still_enabled(&store, 1, turn, Some(99), &settings)
                .unwrap_err()
                .contains("project changed")
        );
        store::settings::set(
            &store,
            SETTINGS_KEY,
            &serde_json::to_string(&Config::default()).unwrap(),
        )
        .unwrap();
        assert!(still_enabled(&store, 1, turn, None, &settings).is_err());
        store::settings::set(
            &store,
            SETTINGS_KEY,
            &serde_json::to_string(&settings).unwrap(),
        )
        .unwrap();
        store.with(|c|c.execute("INSERT INTO projects(id,name,colour,position,created_at) VALUES(1,'fixture','#ffffff',0,1)",[])).unwrap();
        store
            .with(|c| c.execute("UPDATE sessions SET project_id=1 WHERE id=1", []))
            .unwrap();
        store::settings::set(&store, "narrator.project.1", "off").unwrap();
        assert!(
            still_enabled(&store, 1, turn, Some(1), &settings)
                .unwrap_err()
                .contains("off for this project")
        );
        store::settings::set(&store, "narrator.project.1", "on").unwrap();
        assert!(still_enabled(&store, 1, turn, Some(1), &settings).is_ok());
        store::history::open_turn(&store, 1, None, None, None).unwrap();
        assert!(
            still_enabled(&store, 1, turn, Some(1), &settings)
                .unwrap_err()
                .contains("superseded")
        );
    }

    #[test]
    fn fixture_transport_scrubs_escaped_secrets_and_records_usage_without_network() {
        let store = Store::in_memory();
        store.with(|c|c.execute("INSERT INTO sessions(id,name,agent,cwd,state,cols,rows,created_at) VALUES(1,'test','claude','scratch','exited',80,24,1)",[])).unwrap();
        let secret = "private\\fixture-token";
        let key = vault::add(&store, "fixture", secret).unwrap();
        let settings = Config {
            credential_id: Some(key),
            model: "fixture/model".into(),
        };
        store::settings::set(
            &store,
            SETTINGS_KEY,
            &serde_json::to_string(&settings).unwrap(),
        )
        .unwrap();
        let prompt = store::history::insert_prompt(&store, 1, None, "hook", secret).unwrap();
        let turn = store::history::open_turn(&store, 1, None, None, Some(prompt)).unwrap() as u32;
        store::history::close_turn(&store, turn.into(), "done", Some(secret)).unwrap();
        rows::enqueue(&store, 1, turn.into()).unwrap();
        rows::claim(&store).unwrap();
        let manager = SessionManager::new(store, std::env::temp_dir(), Box::new(|_| {}));
        summarise_with(&manager,1,turn,|passed,body| {
            assert_eq!(passed.len(),secret.len());
            assert!(!body.contains("fixture-token"));
            let mut answer=core::fallback(); answer.now=secret.into();
            Ok(serde_json::json!({"choices":[{"message":{"content":serde_json::to_string(&answer).unwrap()}}],"usage":{"cost":0.002}}).to_string())
        }).unwrap();
        let panel = rows::panel(manager.store(), 1, None).unwrap();
        assert_eq!(panel.status, "ready");
        assert!(!panel.summary.now.contains("fixture-token"));
        assert_eq!(panel.known_cost_usd, 0.002);
        assert_eq!(panel.unknown_cost_calls, 0);
    }

    #[test]
    fn no_key_never_calls_transport() {
        let store = Store::in_memory();
        store.with(|c|c.execute("INSERT INTO sessions(id,name,agent,cwd,state,cols,rows,created_at) VALUES(1,'test','claude','scratch','exited',80,24,1)",[])).unwrap();
        let manager = SessionManager::new(store, std::env::temp_dir(), Box::new(|_| {}));
        let result = summarise_with(&manager, 1, 1, |_, _| {
            panic!("No key must not send a request")
        });
        assert!(result.unwrap_err().contains("No narrator key"));
    }

    #[test]
    fn response_after_new_turn_preserves_cost_without_old_suggestions() {
        let store = Store::in_memory();
        store.with(|c| c.execute("INSERT INTO sessions(id,name,agent,cwd,state,cols,rows,created_at) VALUES(1,'test','claude','scratch','exited',80,24,1)", [])).unwrap();
        let key = vault::add(&store, "fixture", "test-only-narrator-credential").unwrap();
        let settings = Config { credential_id: Some(key), model: "fixture/model".into() };
        store::settings::set(&store, SETTINGS_KEY, &serde_json::to_string(&settings).unwrap()).unwrap();
        let turn = store::history::open_turn(&store, 1, None, None, None).unwrap() as u32;
        store::history::close_turn(&store, turn.into(), "done", Some("Finished")).unwrap();
        rows::enqueue(&store, 1, turn.into()).unwrap();
        rows::claim(&store).unwrap();
        let manager = SessionManager::new(store, std::env::temp_dir(), Box::new(|_| {}));
        summarise_with(&manager, 1, turn, |_, _| {
            store::history::open_turn(manager.store(), 1, None, None, None).unwrap();
            let mut answer = core::fallback();
            answer.actions[0].sends = "Stale suggestion from the previous task".into();
            Ok(serde_json::json!({"choices":[{"message":{"content":serde_json::to_string(&answer).unwrap()}}],"usage":{"cost":0.003}}).to_string())
        }).unwrap();
        let panel = rows::panel(manager.store(), 1, None).unwrap();
        assert_ne!(panel.status, "ready");
        assert_eq!(panel.known_cost_usd, 0.003);
        assert!(panel.actions.iter().all(|a| !a.suggestion.sends.contains("Stale suggestion")));
    }
}
