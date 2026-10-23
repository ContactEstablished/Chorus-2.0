//! Durable narrator attempts, suggestions and their explicit user dispositions.
use super::{Store, now_ms};
use chorus_core::narrator::{Suggestion, Summary, signature};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Disposition {
    Done,
    Dismissed,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub id: u32,
    pub suggestion: Suggestion,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Panel {
    pub turn_id: Option<u32>,
    pub summary: Summary,
    pub status: String,
    pub reason: Option<String>,
    pub actions: Vec<Action>,
    pub known_cost_usd: f64,
    pub unknown_cost_calls: u32,
}

pub fn enqueue(store: &Store, session_id: u32, turn_id: i64) -> Result<(), String> {
    store.with(|c| c.execute("INSERT OR IGNORE INTO narrator_jobs(turn_id,session_id,status,created_at) VALUES(?1,?2,'pending',?3)", params![turn_id,session_id,now_ms()]).map(|_| ()))
}

/// Runs before launch/restore: historical or uncertain attempts must never spend on startup.
pub fn abandon_pending(store: &Store) -> Result<(), String> {
    store.with(|c| c.execute("UPDATE narrator_jobs SET status='fallback',reason='Chorus restarted before the summary completed. This turn will not be retried.' WHERE status IN ('pending','running')", []).map(|_| ()))
}

pub fn claim(store: &Store) -> Result<Option<(u32, u32)>, String> {
    store.with(|c| {
        let tx = c.unchecked_transaction()?;
        let job = tx.query_row("SELECT session_id,turn_id FROM narrator_jobs WHERE status='pending' ORDER BY turn_id LIMIT 1", [], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((_,turn)) = job { tx.execute("UPDATE narrator_jobs SET status='running' WHERE turn_id=?1", [turn])?; }
        tx.commit()?; Ok(job)
    })
}

pub fn requested(store: &Store, turn_id: u32, model: &str) -> Result<(), String> {
    store.with(|c| {
        c.execute(
            "UPDATE narrator_jobs SET requested=1,model=?2 WHERE turn_id=?1",
            params![turn_id, model],
        )
        .map(|_| ())
    })
}

fn put_actions(
    c: &rusqlite::Connection,
    session_id: u32,
    turn_id: Option<u32>,
    actions: &[Suggestion],
) -> rusqlite::Result<()> {
    for action in actions {
        // A resolved suggestion is never reopened, even when the model returns it again.
        c.execute("INSERT INTO narrator_actions(session_id,turn_id,signature,suggestion,updated_at) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(session_id,signature) DO UPDATE SET turn_id=excluded.turn_id,suggestion=excluded.suggestion,updated_at=excluded.updated_at WHERE narrator_actions.status='open'",
            params![session_id,turn_id,signature(&action.sends),serde_json::to_string(action).expect("suggestion JSON"),now_ms()])?;
    }
    Ok(())
}

pub fn finish(
    store: &Store,
    session_id: u32,
    turn_id: u32,
    summary: &Summary,
    reason: Option<&str>,
    cost: Option<f64>,
) -> Result<(), String> {
    store.with(|c| {
        let tx = c.unchecked_transaction()?;
        tx.execute(
            "UPDATE narrator_jobs SET status=?2,reason=?3,summary=?4,cost_usd=?5 WHERE turn_id=?1",
            params![
                turn_id,
                if reason.is_some() {
                    "fallback"
                } else {
                    "ready"
                },
                reason,
                serde_json::to_string(summary).expect("summary JSON"),
                cost
            ],
        )?;
        put_actions(&tx, session_id, Some(turn_id), &summary.actions)?;
        tx.commit()
    })
}

pub fn resolve(
    store: &Store,
    session_id: u32,
    action_id: u32,
    disposition: Disposition,
) -> Result<(), String> {
    let status = match disposition {
        Disposition::Done => "done",
        Disposition::Dismissed => "dismissed",
    };
    let count = store.with(|c| c.execute("UPDATE narrator_actions SET status=?3,updated_at=?4 WHERE id=?1 AND session_id=?2 AND status='open'", params![action_id,session_id,status,now_ms()]))?;
    if count == 0 {
        return Err("That step is no longer open in this session. Refresh its Next steps.".into());
    }
    Ok(())
}

pub fn resolved(store: &Store, session_id: u32) -> Result<String, String> {
    store.with(|c| {
        let mut stmt = c.prepare("SELECT suggestion FROM narrator_actions WHERE session_id=?1 AND status!='open' ORDER BY updated_at DESC LIMIT 32")?;
        let rows = stmt.query_map([session_id], |r| r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(format!("[{}]",rows.join(",")))
    })
}

pub fn panel(store: &Store, session_id: u32, unavailable: Option<String>) -> Result<Panel, String> {
    store.with(|c| {
        let row = c.query_row("SELECT turn_id,status,reason,summary FROM narrator_jobs WHERE session_id=?1 ORDER BY turn_id DESC LIMIT 1", [session_id], |r| Ok((r.get::<_,u32>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,Option<String>>(3)?))).optional()?;
        let (turn_id,status,reason,summary) = match row {
            Some((id,status,reason,summary)) => (Some(id), status, reason, summary.and_then(|s|serde_json::from_str(&s).ok()).unwrap_or_else(chorus_core::narrator::fallback)),
            None => (None,"fallback".into(),Some("No completed turn has been summarised yet.".into()),chorus_core::narrator::fallback()),
        };
        let fallback = unavailable.is_some() || status != "ready";
        let summary = if fallback { chorus_core::narrator::fallback() } else { summary };
        if fallback { put_actions(c,session_id,turn_id,&summary.actions)?; }
        let mut stmt = c.prepare("SELECT id,suggestion,status FROM narrator_actions WHERE session_id=?1 AND (status!='open' OR turn_id IS ?2) ORDER BY status='open' DESC,updated_at DESC,id DESC LIMIT 40")?;
        let mut actions = stmt.query_map(params![session_id,turn_id], |r| {
            let data: String = r.get(1)?;
            let suggestion = serde_json::from_str(&data).map_err(|e|rusqlite::Error::FromSqlConversionFailure(1,rusqlite::types::Type::Text,Box::new(e)))?;
            Ok(Action{id:r.get(0)?,suggestion,status:r.get(2)?})
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        // Disabled/unavailable mode must never keep old model suggestions on screen.
        let allowed: Vec<String> = summary.actions.iter().map(|a|signature(&a.sends)).collect();
        actions.retain(|a|a.status!="open" || allowed.contains(&signature(&a.suggestion.sends)));
        let (known_cost_usd,unknown_cost_calls) = c.query_row("SELECT COALESCE(SUM(cost_usd),0),COALESCE(SUM(requested=1 AND cost_usd IS NULL),0) FROM narrator_jobs WHERE session_id=?1", [session_id],|r|Ok((r.get(0)?,r.get(1)?)))?;
        Ok(Panel{turn_id,summary,status:if unavailable.is_some(){"fallback".into()}else{status},reason:unavailable.or(reason),actions,known_cost_usd,unknown_cost_calls})
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Store, u32, u32) {
        let s = Store::in_memory();
        s.with(|c|c.execute("INSERT INTO sessions(id,name,agent,cwd,state,cols,rows,created_at) VALUES(1,'test','claude','scratch','exited',80,24,1)",[])).unwrap();
        let turn = super::super::history::open_turn(&s, 1, None, None, None).unwrap() as u32;
        (s, 1, turn)
    }
    #[test]
    fn attempts_are_claimed_once_and_restart_never_replays() {
        let (s, id, turn) = fixture();
        enqueue(&s, id, turn.into()).unwrap();
        enqueue(&s, id, turn.into()).unwrap();
        assert_eq!(claim(&s).unwrap(), Some((id, turn)));
        assert_eq!(claim(&s).unwrap(), None);
        requested(&s, turn, "fixture/model").unwrap();
        abandon_pending(&s).unwrap();
        assert_eq!(claim(&s).unwrap(), None);
        let p = panel(&s, id, None).unwrap();
        assert_eq!(p.unknown_cost_calls, 1);
        assert_eq!(p.status, "fallback");
    }
    #[test]
    fn done_and_dismissed_stay_resolved_when_repeated() {
        let (s, id, turn) = fixture();
        enqueue(&s, id, turn.into()).unwrap();
        let summary = chorus_core::narrator::fallback();
        finish(&s, id, turn, &summary, None, Some(0.0)).unwrap();
        let p = panel(&s, id, None).unwrap();
        resolve(&s, id, p.actions[0].id, Disposition::Done).unwrap();
        resolve(&s, id, p.actions[1].id, Disposition::Dismissed).unwrap();
        finish(&s, id, turn, &summary, None, None).unwrap();
        let p = panel(&s, id, None).unwrap();
        assert!(p.actions.iter().all(|a| a.status != "open"));
        assert!(resolve(&s, id + 1, p.actions[0].id, Disposition::Done).is_err());
    }
    #[test]
    fn disabled_or_missing_key_hides_model_actions_without_losing_them() {
        let (s, id, turn) = fixture();
        enqueue(&s, id, turn.into()).unwrap();
        let mut summary = chorus_core::narrator::fallback();
        summary.actions[0].sends = "A specific model suggestion".into();
        finish(&s, id, turn, &summary, None, Some(0.001)).unwrap();
        let shown = panel(&s, id, Some("No key".into())).unwrap();
        assert!(
            shown
                .actions
                .iter()
                .all(|a| a.suggestion.sends != "A specific model suggestion")
        );
        assert_eq!(shown.status, "fallback");
        let shown = panel(&s, id, None).unwrap();
        assert!(
            shown
                .actions
                .iter()
                .any(|a| a.suggestion.sends == "A specific model suggestion")
        );
    }
}
