//! Tokens and cost per turn (migration 0025, D107). The figures are the agent's own.

use chorus_core::measure::ledger::{TokenUsage, UsageSource};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use specta::Type;

use super::{Store, now_ms};

/// Records `usage` for the turn the agent calls `agent_turn` in run `run_id`. Returns whether a
/// turn was found.
pub fn set_for_agent_turn(store: &Store, run_id: u32, agent_turn: &str, usage: &TokenUsage, source: UsageSource) -> Result<bool, String> {
    store.with(|c| {
        let turn: Option<i64> = c
            .query_row("SELECT id FROM turns WHERE run_id = ?1 AND agent_turn = ?2 ORDER BY id DESC LIMIT 1", params![run_id, agent_turn], |r| r.get(0))
            .optional()?;
        let Some(turn) = turn else { return Ok(false) };
        c.execute(
            "INSERT INTO turn_usage (turn_id, input, cached, cache_write, output, reasoning, cost_usd, cost_kind, source, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT (turn_id) DO UPDATE SET input = ?2, cached = ?3, cache_write = ?4, output = ?5, reasoning = ?6,
               cost_usd = ?7, cost_kind = ?8, source = ?9, updated_at = ?10",
            params![
                turn,
                usage.input,
                usage.cached,
                usage.cache_write,
                usage.output,
                usage.reasoning,
                usage.cost_usd,
                usage.cost_usd.map(|_| "reported"),
                source.as_str(),
                now_ms()
            ],
        )?;
        Ok(true)
    })
}

/// A session's usage: the sum over its turns that have any, and its newest such turn.
#[derive(Debug, Clone, Default, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionUsage {
    /// Turns with figures.
    pub turns: u32,
    pub total: TokenUsage,
    /// Turns with figures but no cost (a model the agent can't price), so `total`'s cost leaves
    /// them out.
    pub cost_missing: u32,
    pub last: Option<TokenUsage>,
    /// Where they came from, when any did.
    pub source: Option<String>,
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<TokenUsage> {
    Ok(TokenUsage {
        input: r.get(0)?,
        cached: r.get(1)?,
        cache_write: r.get(2)?,
        output: r.get(3)?,
        reasoning: r.get(4)?,
        cost_usd: r.get(5)?,
    })
}

pub fn for_session(store: &Store, session_id: u32) -> Result<SessionUsage, String> {
    store.with(|c| {
        let mut stmt = c.prepare(
            "SELECT u.input, u.cached, u.cache_write, u.output, u.reasoning, u.cost_usd, u.source
             FROM turn_usage u JOIN turns t ON t.id = u.turn_id WHERE t.session_id = ?1 ORDER BY t.id",
        )?;
        let mut out = SessionUsage::default();
        let rows = stmt.query_map([session_id], |r| Ok((row(r)?, r.get::<_, String>(6)?)))?;
        for item in rows {
            let (usage, source) = item?;
            out.turns += 1;
            out.cost_missing += u32::from(usage.cost_usd.is_none());
            out.total.add(&usage);
            out.source = Some(source);
            out.last = Some(usage);
        }
        Ok(out)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::history;
    use crate::store::sessions::{self as rows, NewRun, NewSession};
    use chorus_core::agent::{AgentKind, LaunchOptions};

    #[test]
    fn usage_lands_on_the_agents_turn_and_sums_per_session() {
        let store = Store::in_memory();
        let new = NewSession {
            name: "t",
            agent: AgentKind::Opencode,
            cwd: r"C:\w",
            credential_id: None,
            credential_env: None,
            cols: 80,
            rows: 24,
            options: &LaunchOptions::default(),
            agent_session_id: None,
            provider_id: None,
            description: None,
            project_id: None,
            worktree_id: None,
            routing: None,
        };
        let session = rows::insert(&store, &new).unwrap();
        let run = rows::start_run(&store, &NewRun { session_id: session, pid: None, program: "opencode.exe", args: &[], cli_version: None, resumed: false }).unwrap();
        history::open_turn(&store, session, Some(run), Some("msg_1"), None).unwrap();
        history::open_turn(&store, session, Some(run), Some("msg_2"), None).unwrap();
        let a = TokenUsage { input: Some(100), cached: Some(4000), output: Some(20), cost_usd: Some(0.001), ..Default::default() };
        let b = TokenUsage { input: Some(50), output: Some(5), ..Default::default() };
        assert!(set_for_agent_turn(&store, run, "msg_1", &a, UsageSource::OpencodeServer).unwrap());
        assert!(set_for_agent_turn(&store, run, "msg_2", &b, UsageSource::OpencodeServer).unwrap());
        assert!(!set_for_agent_turn(&store, run, "msg_9", &b, UsageSource::OpencodeServer).unwrap(), "no such turn");
        // Written again, it replaces.
        assert!(set_for_agent_turn(&store, run, "msg_2", &b, UsageSource::OpencodeServer).unwrap());
        let s = for_session(&store, session).unwrap();
        assert_eq!(s.turns, 2);
        assert_eq!((s.total.input, s.total.cached, s.total.output), (Some(150), Some(4000), Some(25)));
        assert_eq!(s.total.cost_usd, Some(0.001));
        assert_eq!(s.cost_missing, 1, "the second turn has no cost");
        assert_eq!(s.last, Some(b));
        assert_eq!(s.source.as_deref(), Some("opencode-server"));
    }
}
