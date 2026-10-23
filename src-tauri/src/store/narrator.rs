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
        // A completed summary is history once another turn starts or a run restarts,
        // including when its HTTP response arrives after that change. Keep its cost and
        // resolved actions, but never offer the old model's instructions as current work.
        let applicable = match turn_id {
            Some(turn) => c.query_row(
                "SELECT EXISTS(SELECT 1 FROM turns t WHERE t.id=?1 AND t.session_id=?2
                 AND t.end_reason='done' AND t.id=(SELECT MAX(id) FROM turns WHERE session_id=?2)
                 AND t.run_id IS (SELECT MAX(id) FROM runs WHERE session_id=?2))",
                params![turn,session_id], |r| r.get::<_,bool>(0),
            )?,
            None => true,
        };
        let historical = !applicable;
        let historical_reason = historical.then(|| "The previous summary is historical: a newer turn or run superseded it. Continue and Review are generic drafts.".to_owned());
        let fallback = unavailable.is_some() || historical || status != "ready";
        let summary = if fallback { chorus_core::narrator::fallback() } else { summary };
        let action_turn = if historical { None } else { turn_id };
        if fallback { put_actions(c,session_id,action_turn,&summary.actions)?; }
        let mut stmt = c.prepare("SELECT id,suggestion,status FROM narrator_actions WHERE session_id=?1 AND (status!='open' OR turn_id IS ?2) ORDER BY status='open' DESC,updated_at DESC,id DESC LIMIT 40")?;
        let mut actions = stmt.query_map(params![session_id,action_turn], |r| {
            let data: String = r.get(1)?;
            let suggestion = serde_json::from_str(&data).map_err(|e|rusqlite::Error::FromSqlConversionFailure(1,rusqlite::types::Type::Text,Box::new(e)))?;
            Ok(Action{id:r.get(0)?,suggestion,status:r.get(2)?})
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        // Disabled/unavailable mode must never keep old model suggestions on screen.
        let allowed: Vec<String> = summary.actions.iter().map(|a|signature(&a.sends)).collect();
        actions.retain(|a|a.status!="open" || allowed.contains(&signature(&a.suggestion.sends)));
        let (known_cost_usd,unknown_cost_calls) = c.query_row("SELECT COALESCE(SUM(cost_usd),0),COALESCE(SUM(requested=1 AND cost_usd IS NULL),0) FROM narrator_jobs WHERE session_id=?1", [session_id],|r|Ok((r.get(0)?,r.get(1)?)))?;
        Ok(Panel{turn_id,summary,status:if unavailable.is_some() || historical {"fallback".into()}else{status},reason:unavailable.or(historical_reason).or(reason),actions,known_cost_usd,unknown_cost_calls})
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Store, u32, u32) {
        let s = Store::in_memory();
        s.with(|c|c.execute("INSERT INTO sessions(id,name,agent,cwd,state,cols,rows,created_at) VALUES(1,'test','claude','scratch','exited',80,24,1)",[])).unwrap();
        let turn = super::super::history::open_turn(&s, 1, None, None, None).unwrap() as u32;
        super::super::history::close_turn(&s, turn.into(), "done", Some("Done.")).unwrap();
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

    fn model_summary() -> Summary {
        let mut summary = chorus_core::narrator::fallback();
        summary.now = "An old model summary".into();
        summary.actions[0].sends = "A previous model instruction".into();
        summary.actions[1].sends = "A resolved model instruction".into();
        summary
    }

    fn assert_historical(panel: &Panel, cost: f64) {
        assert_eq!(panel.status, "fallback");
        assert!(panel.reason.as_deref().unwrap().contains("historical"));
        assert_ne!(panel.summary.now, "An old model summary");
        let generic = chorus_core::narrator::fallback();
        assert!(panel.actions.iter().filter(|a| a.status == "open")
            .all(|a| generic.actions.iter().any(|g| g.sends == a.suggestion.sends)));
        assert_eq!(panel.known_cost_usd, cost);
    }

    #[test]
    fn newer_turn_hides_old_summary_and_keeps_cost_and_resolved_history() {
        let (s, id, turn) = fixture();
        enqueue(&s, id, turn.into()).unwrap();
        finish(&s, id, turn, &model_summary(), None, Some(0.002)).unwrap();
        let current = panel(&s, id, None).unwrap();
        let resolved = current.actions.iter().find(|a| a.suggestion.sends == "A resolved model instruction").unwrap().id;
        resolve(&s, id, resolved, Disposition::Done).unwrap();
        super::super::history::open_turn(&s, id, None, None, None).unwrap();
        let historical = panel(&s, id, None).unwrap();
        assert_historical(&historical, 0.002);
        assert!(historical.actions.iter().any(|a| a.id == resolved && a.status == "done"));
        let stored: String = s.with(|c| c.query_row("SELECT summary FROM narrator_jobs WHERE turn_id=?1", [turn], |r| r.get(0))).unwrap();
        assert!(stored.contains("An old model summary"), "historical evidence remains stored");
    }

    #[test]
    fn late_summary_completion_is_inapplicable_after_new_turn() {
        let (s, id, turn) = fixture();
        enqueue(&s, id, turn.into()).unwrap();
        claim(&s).unwrap();
        requested(&s, turn, "fixture/model").unwrap();
        // A transport can finish after another turn starts; that response still records cost.
        super::super::history::open_turn(&s, id, None, None, None).unwrap();
        finish(&s, id, turn, &model_summary(), None, Some(0.003)).unwrap();
        assert_historical(&panel(&s, id, None).unwrap(), 0.003);
    }

    #[test]
    fn restarted_run_hides_old_summary_before_any_new_turn() {
        let (s, id, turn) = fixture();
        s.with(|c| {
            c.execute("INSERT INTO runs(id,session_id,program,args,started_at) VALUES(1,?1,'fixture','[]',1)", [id])?;
            c.execute("UPDATE turns SET run_id=1 WHERE id=?1", [turn])?;
            Ok(())
        }).unwrap();
        enqueue(&s, id, turn.into()).unwrap();
        finish(&s, id, turn, &model_summary(), None, Some(0.004)).unwrap();
        assert_eq!(panel(&s, id, None).unwrap().status, "ready");
        s.with(|c| c.execute("INSERT INTO runs(id,session_id,program,args,started_at) VALUES(2,?1,'fixture','[]',2)", [id])).unwrap();
        assert_historical(&panel(&s, id, None).unwrap(), 0.004);
    }

    #[test]
    fn distinct_case_and_literal_whitespace_actions_are_not_suppressed() {
        for (first, second) in [
            ("Rename foo to Foo", "Rename Foo to foo"),
            ("Replace \"a  b\"", "Replace \"a b\""),
        ] {
            let (s, id, turn) = fixture();
            enqueue(&s, id, turn.into()).unwrap();
            let mut summary = model_summary();
            summary.actions.truncate(1);
            summary.actions[0].sends = first.into();
            finish(&s, id, turn, &summary, None, None).unwrap();
            let first_id = panel(&s, id, None).unwrap().actions[0].id;
            resolve(&s, id, first_id, Disposition::Done).unwrap();
            summary.actions[0].sends = second.into();
            finish(&s, id, turn, &summary, None, None).unwrap();
            let shown = panel(&s, id, None).unwrap();
            assert!(shown.actions.iter().any(|a| a.suggestion.sends == second && a.status == "open"));
            assert!(shown.actions.iter().any(|a| a.id == first_id && a.status == "done"));
        }
    }

}
