//! The SQLite store (ARCHITECTURE §7): one database in the app's data folder, WAL mode,
//! `synchronous=FULL` (history must survive power loss; write volume is low), foreign keys on, and
//! numbered forward-only migrations from `src-tauri/migrations/` (planned by
//! `chorus_core::migrate`). Typed queries for each area live in the submodules.

pub mod credentials;
pub mod attention;
pub mod narrator;
pub mod delivery;
pub mod composer;
pub mod images;
pub mod replies;
pub mod history;
pub mod profiles;
pub mod projects;
pub mod providers;
pub mod routing;
pub mod sessions;
pub mod settings;
pub mod tags;
pub mod worktrees;

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

/// Every migration, in order. `migrations_are_all_listed` fails if a file is missing here.
const MIGRATION_FILES: &[(&str, &str)] = &[
    ("0001_initial.sql", include_str!("../../migrations/0001_initial.sql")),
    ("0002_agent_options.sql", include_str!("../../migrations/0002_agent_options.sql")),
    ("0003_providers.sql", include_str!("../../migrations/0003_providers.sql")),
    ("0004_launch.sql", include_str!("../../migrations/0004_launch.sql")),
    ("0005_history.sql", include_str!("../../migrations/0005_history.sql")),
    ("0006_transcripts.sql", include_str!("../../migrations/0006_transcripts.sql")),
    ("0007_projects.sql", include_str!("../../migrations/0007_projects.sql")),
    ("0008_projects_no_import.sql", include_str!("../../migrations/0008_projects_no_import.sql")),
    ("0009_tags_worktrees.sql", include_str!("../../migrations/0009_tags_worktrees.sql")),
    ("0010_routing.sql", include_str!("../../migrations/0010_routing.sql")),
    ("0011_p14_agent_check.sql", include_str!("../../migrations/0011_p14_agent_check.sql")),
    ("0012_aider_permissions.sql", include_str!("../../migrations/0012_aider_permissions.sql")),
    ("0013_p6_delivery.sql", include_str!("../../migrations/0013_p6_delivery.sql")),
    ("0014_p6_replies.sql", include_str!("../../migrations/0014_p6_replies.sql")),
    ("0015_p6_composer.sql", include_str!("../../migrations/0015_p6_composer.sql")),
    ("0016_p6_images.sql", include_str!("../../migrations/0016_p6_images.sql")),
    ("0017_p6_progress.sql", include_str!("../../migrations/0017_p6_progress.sql")),
    ("0018_p6_narrator.sql", include_str!("../../migrations/0018_p6_narrator.sql")),
];

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| format!("opening {}: {e}", path.display()))?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self::init(Connection::open_in_memory().expect("in-memory database")).expect("init in-memory store")
    }

    fn init(mut conn: Connection) -> Result<Self, String> {
        let setup = || -> rusqlite::Result<()> {
            // An in-memory database answers "memory"; a file answers "wal".
            let _mode: String = conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
            conn.pragma_update(None, "synchronous", "FULL")?;
            conn.pragma_update(None, "foreign_keys", "ON")?;
            conn.busy_timeout(Duration::from_secs(5))
        };
        setup().map_err(|e| format!("configuring the database: {e}"))?;
        migrate(&mut conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Runs `f` with the connection. Errors become strings, ready for the UI.
    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T, String> {
        let conn = self.conn.lock().expect("store lock");
        f(&conn).map_err(|e| format!("database: {e}"))
    }

}

/// Applies every migration newer than the database's `user_version`, each in its own transaction
/// together with the version bump, so a failure leaves the database at the previous version.
///
/// A migration whose header asks for it (`-- chorus: foreign_keys off`) runs its transaction with
/// foreign keys off: that is a table rebuild, which SQLite can't do with them on (dropping the old
/// table would cascade). The keys are turned back on afterwards whether it succeeded or not, and a
/// rebuild that leaves them dangling is refused before its version bump commits.
fn migrate(conn: &mut Connection) -> Result<u32, String> {
    use chorus_core::migrate::{directives, from_files, pending};
    let all = from_files(MIGRATION_FILES).map_err(|e| e.to_string())?;
    let current: u32 =
        conn.pragma_query_value(None, "user_version", |row| row.get(0)).map_err(|e| format!("reading schema version: {e}"))?;
    let mut version = current;
    for m in pending(current, &all).map_err(|e| e.to_string())? {
        let d = directives(m.sql).map_err(|e| e.to_string())?;
        if d.foreign_keys_off {
            set_foreign_keys(conn, false)?;
        }
        let applied = apply(conn, &m, d.foreign_keys_off);
        if d.foreign_keys_off {
            // Restored even on failure: every later query in this connection relies on it.
            set_foreign_keys(conn, true)?;
        }
        applied.map_err(|e| format!("migration {:04}_{}: {e}", m.version, m.name))?;
        version = m.version;
    }
    Ok(version)
}

fn set_foreign_keys(conn: &Connection, on: bool) -> Result<(), String> {
    conn.pragma_update(None, "foreign_keys", if on { "ON" } else { "OFF" })
        .map_err(|e| format!("turning foreign keys {}: {e}", if on { "on" } else { "off" }))
}

/// One migration in its transaction, with the version bump. A rebuild is checked for dangling
/// references first (SQLite's step 10), while the rollback is still possible.
fn apply(conn: &mut Connection, m: &chorus_core::migrate::Migration<'_>, check_foreign_keys: bool) -> Result<(), String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    tx.execute_batch(m.sql).map_err(|e| e.to_string())?;
    if check_foreign_keys {
        let broken = foreign_key_violations(&tx)?;
        if !broken.is_empty() {
            return Err(format!("the rebuild left foreign keys pointing nowhere: {}", broken.join(", ")));
        }
    }
    tx.pragma_update(None, "user_version", m.version).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

/// `PRAGMA foreign_key_check` as `child table → missing parent table`, empty when every reference
/// resolves.
fn foreign_key_violations(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn.prepare("PRAGMA foreign_key_check").map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let child: String = row.get(0).map_err(|e| e.to_string())?;
        let parent: String = row.get(2).map_err(|e| e.to_string())?;
        out.push(format!("{child} → {parent}"));
    }
    Ok(out)
}

/// Unix milliseconds, the unit every table uses.
pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_database_reaches_the_latest_schema() {
        let store = Store::in_memory();
        let (version, tables): (u32, Vec<String>) = store
            .with(|c| {
                let v = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
                let mut stmt = c.prepare("SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name")?;
                let names = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
                Ok((v, names))
            })
            .unwrap();
        assert_eq!(version as usize, MIGRATION_FILES.len());
        for t in ["credentials", "runs", "sessions", "settings"] {
            assert!(tables.iter().any(|n| n == t), "missing table {t}: {tables:?}");
        }
    }

    #[test]
    fn reopening_a_file_database_keeps_data_and_runs_nothing_twice() {
        let dir = std::env::temp_dir().join(format!("chorus-store-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.db");
        let _ = std::fs::remove_file(&path);
        {
            let store = Store::open(&path).unwrap();
            settings::set(&store, "probe", "1").unwrap();
            let mode: String = store.with(|c| c.pragma_query_value(None, "journal_mode", |r| r.get(0))).unwrap();
            assert_eq!(mode, "wal");
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(settings::get(&store, "probe").unwrap().as_deref(), Some("1"));
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_database_from_a_newer_chorus_is_refused() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
        let err = Store::init(conn).err().expect("should refuse");
        assert!(err.contains("newer Chorus"), "{err}");
    }

    /// A database as P4 leaves it: every migration up to 10 applied, foreign keys on.
    fn database_at_version_10() -> Connection {
        use chorus_core::migrate::{from_files, pending};
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        let all = from_files(MIGRATION_FILES).unwrap();
        for m in pending(0, &all).unwrap().into_iter().filter(|m| m.version <= 10) {
            conn.execute_batch(m.sql).unwrap();
            conn.pragma_update(None, "user_version", m.version).unwrap();
        }
        conn
    }

    #[test]
    fn widening_the_agent_check_rebuilds_the_table_and_keeps_every_row() {
        let conn = database_at_version_10();
        conn.execute_batch(
            r#"
            INSERT INTO sessions (id, name, agent, cwd, state, cols, rows, locked, note, created_at, options, agent_session_id, description, routing)
            VALUES (7, 'Fix login', 'opencode', 'C:\w', 'exited', 120, 30, 0, 'a note', 1000, '{"effort":"deep"}', 'ses_1', 'the login fix', '{"routing":"tier"}');
            INSERT INTO runs (id, session_id, program, args, started_at, ended_at, exit_code, end_reason)
            VALUES (3, 7, 'opencode.exe', '["serve"]', 1001, 1002, 0, 'exited');
            INSERT INTO prompts (id, session_id, run_id, source, text, created_at) VALUES (5, 7, 3, 'hook', 'hello', 1003);
            INSERT INTO turns (id, session_id, run_id, prompt_id, started_at, ended_at, end_reason, final_message)
            VALUES (2, 7, 3, 5, 1004, 1005, 'done', 'ok');
            INSERT INTO tool_calls (id, session_id, turn_id, tool, target, started_at) VALUES (1, 7, 2, 'edit', 'src/app.ts', 1006);
            INSERT INTO asks (id, session_id, turn_id, kind, subject, asked_at, answer) VALUES (1, 7, 2, 'approval', 'edit', 1007, 'yes');
            INSERT INTO agent_events (id, session_id, run_id, at, source, name, body) VALUES (1, 7, 3, 1008, 'hook', 'Stop', 'done');
            INSERT INTO session_tags (session_id, tag, position) VALUES (7, 'fix', 0);
            INSERT INTO providers (id, kind, name, created_at) VALUES (4, 'openrouter', 'OpenRouter', 1009);
            INSERT INTO model_catalog (provider_id, model_id, data, first_seen_at) VALUES (4, 'x/y', '{}', 1010);
            INSERT INTO shortlist (provider_id, model_id, added_at) VALUES (4, 'x/y', 1011);
            "#,
        )
        .unwrap();

        let store = Store::init(conn).unwrap();
        let (version, counts, violations): (u32, Vec<i64>, Vec<String>) = store
            .with(|c| {
                let version = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
                let counts = [
                    "sessions",
                    "runs",
                    "prompts",
                    "turns",
                    "tool_calls",
                    "asks",
                    "agent_events",
                    "session_tags",
                    "providers",
                    "model_catalog",
                    "shortlist",
                ]
                    .iter()
                    .map(|t| c.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0)))
                    .collect::<rusqlite::Result<Vec<i64>>>()?;
                Ok((version, counts, super::foreign_key_violations(c).unwrap()))
            })
            .unwrap();
        assert_eq!(version, MIGRATION_FILES.len() as u32);
        assert_eq!(counts, [1; 11], "every row survives the rebuild");
        assert!(violations.is_empty(), "{violations:?}");

        let kept: (String, String, String, String, String) = store
            .with(|c| {
                c.query_row(
                    "SELECT agent, note, options, description, routing FROM sessions WHERE id = 7",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
            })
            .unwrap();
        assert_eq!(
            kept,
            (
                "opencode".to_owned(),
                "a note".to_owned(),
                r#"{"effort":"deep"}"#.to_owned(),
                "the login fix".to_owned(),
                r#"{"routing":"tier"}"#.to_owned()
            )
        );
    }

    #[test]
    fn the_rebuilt_sessions_table_takes_the_new_agents_and_keeps_its_rules() {
        let conn = database_at_version_10();
        let store = Store::init(conn).unwrap();
        let insert = |agent: &str| {
            store.with(|c| {
                c.execute(
                    "INSERT INTO sessions (name, agent, cwd, state, cols, rows, created_at) VALUES ('x', ?1, 'C:\\w', 'exited', 80, 24, 1)",
                    [agent],
                )
            })
        };
        assert!(insert("gemini").is_ok());
        assert!(insert("aider").is_ok());
        assert!(insert("kimi").is_err(), "an agent kind Rust doesn't know is still refused");
        // The providers table takes Ollama, and still refuses a kind Chorus doesn't know.
        let provider = |kind: &str| {
            store.with(|c| c.execute("INSERT INTO providers (kind, name, created_at) VALUES (?1, ?1, 1)", [kind]))
        };
        assert!(provider("ollama").is_ok());
        assert!(provider("nobody").is_err());
        // Foreign keys are back on, and the other tables still point at `sessions`.
        assert!(store
            .with(|c| c.execute(
                "INSERT INTO runs (session_id, program, args, started_at) VALUES (999, 'x.exe', '[]', 1)",
                [],
            ))
            .is_err());
        let prompt_sql: String = store
            .with(|c| c.query_row("SELECT sql FROM sqlite_schema WHERE name = 'prompts'", [], |r| r.get(0)))
            .unwrap();
        assert!(prompt_sql.contains("REFERENCES sessions (id)"), "{prompt_sql}");
    }

    #[test]
    fn migrations_are_all_listed() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".sql"))
            .collect();
        on_disk.sort();
        let listed: Vec<String> = MIGRATION_FILES.iter().map(|(n, _)| (*n).to_owned()).collect();
        assert_eq!(on_disk, listed, "every file in src-tauri/migrations must be listed in MIGRATION_FILES");
    }

    #[test]
    fn aider_permission_migration_keeps_profiles_and_other_agents_choices() {
        let conn = database_at_version_10();
        conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
        conn.execute_batch(MIGRATION_FILES[10].1).unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn.pragma_update(None, "user_version", 11).unwrap();
        for (id, agent, options) in [
            (1, "aider", r#"{"permission":"manual","effort":"deep","model":"test"}"#),
            (2, "claude", r#"{"permission":"manual"}"#),
            (3, "aider", r#"{"permission":"full-access"}"#),
            (4, "aider", "{}"),
            (5, "aider", "unreadable"),
        ] {
            conn.execute("INSERT INTO sessions (id,name,agent,cwd,state,cols,rows,created_at,options) VALUES (?1,'x',?2,'C:\\w','exited',80,24,1,?3)",
                rusqlite::params![id, agent, options]).unwrap();
            conn.execute("INSERT INTO launch_profiles (id,name,agent,options,created_at,updated_at) VALUES (?1,?1,?2,?3,1,1)",
                rusqlite::params![id, agent, options]).unwrap();
        }
        let store = Store::init(conn).unwrap();
        store.with(|c| {
            for table in ["sessions", "launch_profiles"] {
                let values: Vec<String> = c.prepare(&format!("SELECT options FROM {table} ORDER BY id"))?
                    .query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
                let first: serde_json::Value = serde_json::from_str(&values[0]).unwrap();
                assert_eq!(first, serde_json::json!({"permission":"accept-edits", "effort":"deep", "model":"test"}));
                assert_eq!(values[1..], [r#"{"permission":"manual"}"#, r#"{"permission":"full-access"}"#, "{}", "unreadable"]);
            }
            Ok(())
        }).unwrap();
    }
}
