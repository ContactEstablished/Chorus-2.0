//! Durable at-most-once narrator attempts, reported costs and session-scoped dismissals.
use super::{Store, now_ms};
use chorus_core::narrator::{Narration, Suggestion, action_key};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use specta::Type;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NarratorJob {
    pub turn_id: u32,
    pub status: String,
    pub note: Option<String>,
    pub model: Option<String>,
    pub summary: Option<Narration>,
    pub cost_usd: Option<f64>,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub finished_at_secs: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NarratorAction {
    pub id: u32,
    pub turn_id: Option<u32>,
    pub suggestion: Suggestion,
    pub disposition: String,
    pub restored: bool,
}

#[derive(Debug, Clone, Default, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NarratorCosts {
    pub reported_usd: f64,
    pub calls: u32,
    pub unknown_cost_calls: u32,
}

pub fn latest(store: &Store, session: u32) -> Result<Option<NarratorJob>, String> {
    store.with(|c| c.query_row("SELECT turn_id,status,note,model,body,cost_usd,prompt_tokens,completion_tokens,finished_at FROM narrator_jobs WHERE session_id=?1 ORDER BY turn_id DESC LIMIT 1",[session],|r| Ok(NarratorJob {
        turn_id:r.get(0)?, status:r.get(1)?,note:r.get(2)?,model:r.get(3)?,
        summary:r.get::<_,Option<String>>(4)?.and_then(|s| serde_json::from_str(&s).ok()), cost_usd:r.get(5)?,prompt_tokens:r.get(6)?,completion_tokens:r.get(7)?,
        finished_at_secs:r.get::<_,Option<i64>>(8)?.map(|m| (m/1000) as u32),
    })).optional())
}

pub fn costs(store: &Store, session: Option<u32>) -> Result<NarratorCosts, String> {
    store.with(|c| c.query_row("SELECT COALESCE(SUM(cost_usd),0),COUNT(*),COALESCE(SUM(cost_usd IS NULL),0) FROM narrator_jobs WHERE model IS NOT NULL AND (?1 IS NULL OR session_id=?1)",[session],|r| Ok(NarratorCosts { reported_usd:r.get(0)?,calls:r.get(1)?,unknown_cost_calls:r.get(2)? })))
}

pub fn actions(store: &Store, session: u32) -> Result<Vec<NarratorAction>, String> {
    store.with(|c| c.prepare("SELECT id,turn_id,body,disposition,restored FROM narrator_actions WHERE session_id=?1 ORDER BY updated_at DESC,id DESC")?
        .query_map([session],|r| {
            let body:String=r.get(2)?;
            let suggestion=serde_json::from_str(&body).map_err(|e| rusqlite::Error::FromSqlConversionFailure(2,rusqlite::types::Type::Text,Box::new(e)))?;
            Ok(NarratorAction { id:r.get(0)?,turn_id:r.get(1)?,suggestion,disposition:r.get(3)?,restored:r.get(4)? })
        })?.collect())
}

pub fn put_actions(
    c: &rusqlite::Connection,
    session: u32,
    turn: Option<u32>,
    actions: &[Suggestion],
) -> rusqlite::Result<()> {
    let closed: Vec<Suggestion> = c
        .prepare("SELECT body FROM narrator_actions WHERE session_id=?1 AND disposition<>'open'")?
        .query_map([session], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .filter_map(|s| serde_json::from_str(&s).ok())
        .collect();
    for a in actions {
        if closed
            .iter()
            .any(|old| chorus_core::narrator::same_action(old, a))
        {
            continue;
        }
        let body = serde_json::to_string(a)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        c.execute("INSERT INTO narrator_actions(session_id,turn_id,action_key,body,updated_at) VALUES(?1,?2,?3,?4,?5)
            ON CONFLICT(session_id,action_key) DO UPDATE SET turn_id=excluded.turn_id,body=excluded.body,updated_at=excluded.updated_at WHERE narrator_actions.disposition='open'",
            params![session,turn,action_key(a),body,now_ms()])?;
    }
    Ok(())
}

pub fn ensure_fallback(store: &Store, session: u32) -> Result<(), String> {
    // Stable rows: do not continually reorder or resurrect dismissed generic actions.
    store.with(|c| {
        for a in chorus_core::narrator::fallback() {
            c.execute("INSERT OR IGNORE INTO narrator_actions(session_id,action_key,body,updated_at) VALUES(?1,?2,?3,?4)",params![session,action_key(&a),serde_json::to_string(&a).unwrap(),now_ms()])?;
        }
        Ok(())
    })
}

pub fn disposition(store: &Store, session: u32, id: u32, value: &str) -> Result<(), String> {
    if !matches!(value, "done" | "dismissed" | "open") {
        return Err("Unknown next-step action.".into());
    }
    let n=store.with(|c| c.execute("UPDATE narrator_actions SET disposition=?3,updated_at=?4,restored=(?3='open') WHERE session_id=?1 AND id=?2",params![session,id,value,now_ms()]))?;
    if n == 0 {
        return Err("This next step no longer exists in this session.".into());
    }
    Ok(())
}

pub fn enabled(store: &Store, project: Option<u32>) -> Result<bool, String> {
    match project {
        None => Ok(true),
        Some(id) => store
            .with(|c| {
                c.query_row(
                    "SELECT enabled FROM narrator_projects WHERE project_id=?1",
                    [id],
                    |r| r.get(0),
                )
                .optional()
            })
            .map(|v| v.unwrap_or(true)),
    }
}

pub fn queue(store: &Store, session: u32, turn: u32) -> Result<(), String> {
    store.with(|c| c.execute("INSERT INTO narrator_jobs(turn_id,session_id,created_at) SELECT id,session_id,?3 FROM turns WHERE id=?1 AND session_id=?2 AND ended_at IS NOT NULL AND end_reason IN ('done','failed')
        ON CONFLICT(turn_id) DO UPDATE SET status='queued',note=NULL WHERE narrator_jobs.status='skipped'",params![turn,session,now_ms()]).map(|_|()))
}

/// Committed before HTTP. A crash or timeout can never trigger a second paid call.
pub fn claim(store: &Store, turn: u32, model: &str) -> Result<bool, String> {
    store.with(|c|c.execute("UPDATE narrator_jobs SET status='attempting',model=?2 WHERE turn_id=?1 AND status='queued'",params![turn,model]).map(|n|n==1))
}

pub fn finish(
    store: &Store,
    turn: u32,
    session: u32,
    result: Result<&Narration, &str>,
    cost: Option<f64>,
    tokens: (Option<u32>, Option<u32>),
) -> Result<(), String> {
    store.with(|c|{
        let tx=c.unchecked_transaction()?;
        let (status,note,body)=match result {
            Ok(n)=>("ready",None,Some(serde_json::to_string(n).unwrap())),
            Err(e)=>("failed",Some(e),None),
        };
        tx.execute("UPDATE narrator_jobs SET status=?2,note=?3,body=?4,cost_usd=?5,prompt_tokens=?6,completion_tokens=?7,finished_at=?8 WHERE turn_id=?1 AND status='attempting'",params![turn,status,note,body,cost,tokens.0,tokens.1,now_ms()])?;
        if let Ok(n)=result { put_actions(&tx,session,Some(turn),&n.actions)?; }
        tx.commit()
    })
}

pub fn recover(store: &Store) -> Result<(), String> {
    store.with(|c|c.execute("UPDATE narrator_jobs SET status='failed',note='Chorus closed during this attempt. Its cost may be unknown; it will not be retried.' WHERE status='attempting'",[]).map(|_|()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Store, u32, u32) {
        let s = Store::in_memory();
        s.with(|c|c.execute("INSERT INTO sessions(name,agent,cwd,state,cols,rows,created_at) VALUES('n','shell','C:/scratch','closed',80,24,0)",[])).unwrap();
        let id = s.with(|c| Ok(c.last_insert_rowid() as u32)).unwrap();
        let t = super::super::history::open_turn(&s, id, None, None, None).unwrap() as u32;
        super::super::history::close_turn(&s, t.into(), "done", Some("Finished")).unwrap();
        (s, id, t)
    }
    #[test]
    fn interrupted_attempts_are_unknown_cost_and_never_replayed() {
        let (s, id, t) = fixture();
        assert!(claim(&s, t, "test/model").unwrap());
        recover(&s).unwrap();
        assert_eq!(latest(&s, id).unwrap().unwrap().status, "failed");
        assert_eq!(costs(&s, Some(id)).unwrap().unknown_cost_calls, 1);
        queue(&s, id, t).unwrap();
        assert!(!claim(&s, t, "test/model").unwrap());
        ensure_fallback(&s, id).unwrap();
        let a = actions(&s, id).unwrap().remove(0);
        disposition(&s, id, a.id, "dismissed").unwrap();
        recover(&s).unwrap();
        ensure_fallback(&s, id).unwrap();
        assert_eq!(
            actions(&s, id)
                .unwrap()
                .iter()
                .find(|x| x.id == a.id)
                .unwrap()
                .disposition,
            "dismissed"
        );
        disposition(&s, id, a.id, "open").unwrap();
        assert!(
            actions(&s, id)
                .unwrap()
                .iter()
                .find(|x| x.id == a.id)
                .unwrap()
                .restored
        );
    }

    #[test]
    fn one_claim_and_dismissals_survive_new_titles_and_turns() {
        let (s, id, t) = fixture();
        queue(&s, id, t).unwrap();
        assert!(claim(&s, t, "test/model").unwrap());
        assert!(!claim(&s, t, "test/model").unwrap());
        ensure_fallback(&s, id).unwrap();
        let a = actions(&s, id).unwrap().remove(0);
        disposition(&s, id, a.id, "done").unwrap();
        let mut changed = a.suggestion;
        changed.title = "Different title".into();
        s.with(|c| put_actions(c, id, Some(t), &[changed])).unwrap();
        assert_eq!(
            actions(&s, id)
                .unwrap()
                .iter()
                .find(|x| x.id == a.id)
                .unwrap()
                .disposition,
            "done"
        );
        assert!(disposition(&s, id + 1, a.id, "open").is_err());
        let mut repeated = actions(&s, id)
            .unwrap()
            .into_iter()
            .find(|x| x.id == a.id)
            .unwrap()
            .suggestion;
        repeated.sends = "Different wording, same dismissed title".into();
        s.with(|c| put_actions(c, id, Some(t), &[repeated]))
            .unwrap();
        assert_eq!(actions(&s, id).unwrap().len(), 2);
        finish(
            &s,
            t,
            id,
            Err("Invalid summary"),
            Some(0.01),
            (Some(12), Some(4)),
        )
        .unwrap();
        assert_eq!(costs(&s, Some(id)).unwrap().reported_usd, 0.01);
        queue(&s, id, t).unwrap();
        assert!(!claim(&s, t, "test/model").unwrap());
    }
}
