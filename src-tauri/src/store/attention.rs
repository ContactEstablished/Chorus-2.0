//! Read the current run's durable waiting evidence. Old runs never lend an age to a new one.

use super::{Store, sessions::RunRow};
use chorus_core::{
    activity::looks_like_question,
    attention::Attention,
    inbox::{WaitingFacts, needs_you_since},
};
use rusqlite::{OptionalExtension, params};

/// The newest unanswered ask of `kind` ("approval" or "question") in the session: what it is
/// waiting on, already scrubbed, for a notification's body.
pub fn open_ask_subject(store: &Store, session: u32, kind: &str) -> Result<Option<String>, String> {
    store.with(|c| {
        c.query_row(
            "SELECT subject FROM asks WHERE session_id=?1 AND kind=?2 AND answered_at IS NULL ORDER BY asked_at DESC, id DESC LIMIT 1",
            params![session, kind],
            |r| r.get(0),
        )
        .optional()
    })
}

pub fn waiting_since(
    store: &Store,
    session: u32,
    attention: Attention,
    run: Option<&RunRow>,
) -> Result<Option<i64>, String> {
    if !attention.needs_you() {
        return Ok(None);
    }
    let Some(run) = run else {
        return Ok(None);
    };
    store.with(|c| {
        let mut facts = WaitingFacts { run_ended_at: run.ended_at, ..Default::default() };
        // Asks without a turn occur at startup. Bound them to this run's lifetime too.
        let ask_at = |kind: &str| c.query_row(
            "SELECT min(a.asked_at) FROM asks a LEFT JOIN turns t ON t.id=a.turn_id
             WHERE a.session_id=?1 AND a.kind=?2 AND a.answered_at IS NULL
               AND ((t.run_id=?3 AND t.ended_at IS NULL)
                 OR (a.turn_id IS NULL AND a.asked_at>=?4))",
            params![session, kind, run.id, run.started_at], |r| r.get::<_, Option<i64>>(0));
        if run.ended_at.is_none() {
            facts.approval_at = ask_at("approval")?;
            facts.question_at = ask_at("question")?;
        }
        let turn: Option<(Option<i64>, Option<String>, Option<String>)> = c.query_row(
            "SELECT ended_at,end_reason,final_message FROM turns WHERE session_id=?1 AND run_id=?2 ORDER BY id DESC LIMIT 1",
            params![session, run.id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional()?;
        if let Some((ended, reason, message)) = turn {
            if reason.as_deref() == Some("done") && message.as_deref().is_some_and(looks_like_question) {
                facts.question_turn_ended_at = ended;
            }
            if reason.as_deref() == Some("failed") { facts.failed_turn_ended_at = ended; }
        }
        Ok(needs_you_since(attention, &facts))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_open_ask_names_what_it_waits_on() {
        let store = Store::in_memory();
        store.with(|c| c.execute_batch("INSERT INTO sessions(id,name,agent,cwd,state,cols,rows,created_at) VALUES(1,'n','shell','.','running',80,24,1);
            INSERT INTO asks(session_id,turn_id,kind,subject,asked_at,answered_at) VALUES
                (1,NULL,'approval','Write old.txt',10,20),(1,NULL,'approval','Write p7.txt',30,NULL),(1,NULL,'question','Tea or coffee?',40,NULL);")).unwrap();
        assert_eq!(open_ask_subject(&store, 1, "approval").unwrap().as_deref(), Some("Write p7.txt"));
        assert_eq!(open_ask_subject(&store, 1, "question").unwrap().as_deref(), Some("Tea or coffee?"));
        assert_eq!(open_ask_subject(&store, 2, "approval").unwrap(), None);
    }

    #[test]
    fn persisted_ages_ignore_old_runs_answered_asks_and_unrelated_turns() {
        let store = Store::in_memory();
        store.with(|c| c.execute_batch("INSERT INTO sessions(id,name,agent,cwd,state,cols,rows,created_at) VALUES(1,'inbox','shell','.', 'running',80,24,1);
            INSERT INTO runs(id,session_id,program,args,started_at) VALUES(1,1,'shell','[]',100),(2,1,'shell','[]',1000);
            INSERT INTO turns(id,session_id,run_id,started_at) VALUES(1,1,1,100),(2,1,2,1000);
            INSERT INTO asks(session_id,turn_id,kind,subject,asked_at,answered_at) VALUES
                (1,1,'approval','old run',200,NULL),(1,2,'approval','answered',1100,1200),
                (1,2,'approval','current',1300,NULL),(1,2,'approval','newer',1400,NULL);")).unwrap();
        let run = super::super::sessions::last_run(&store, 1)
            .unwrap()
            .unwrap();
        assert_eq!(
            waiting_since(&store, 1, Attention::Approval, Some(&run)).unwrap(),
            Some(1300)
        );
        // Re-reading persistent facts cannot reset the age.
        assert_eq!(
            waiting_since(&store, 1, Attention::Approval, Some(&run)).unwrap(),
            Some(1300)
        );
        store.with(|c| c.execute_batch("UPDATE asks SET answered_at=1500 WHERE turn_id=2;
            UPDATE turns SET ended_at=1600,end_reason='done',final_message='Which colour?' WHERE id=2;")).unwrap();
        assert_eq!(
            waiting_since(&store, 1, Attention::Approval, Some(&run)).unwrap(),
            None
        );
        assert_eq!(
            waiting_since(&store, 1, Attention::Input, Some(&run)).unwrap(),
            Some(1600)
        );
        store
            .with(|c| {
                c.execute_batch(
                    "INSERT INTO turns(id,session_id,run_id,started_at) VALUES(3,1,2,1700);",
                )
            })
            .unwrap();
        assert_eq!(
            waiting_since(&store, 1, Attention::Input, Some(&run)).unwrap(),
            None
        );
        store
            .with(|c| {
                c.execute_batch("UPDATE turns SET ended_at=1800,end_reason='failed' WHERE id=3;")
            })
            .unwrap();
        assert_eq!(
            waiting_since(&store, 1, Attention::Failed, Some(&run)).unwrap(),
            Some(1800)
        );
        let ended = RunRow {
            ended_at: Some(1900),
            ..run
        };
        assert_eq!(
            waiting_since(&store, 1, Attention::Failed, Some(&ended)).unwrap(),
            Some(1900)
        );
        assert_eq!(
            waiting_since(&store, 1, Attention::Done, Some(&ended)).unwrap(),
            None
        );
    }
}
