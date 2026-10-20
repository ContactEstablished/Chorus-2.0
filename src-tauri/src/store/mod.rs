//! The SQLite store (ARCHITECTURE §7): one database in the app's data folder, WAL mode,
//! `synchronous=FULL` (history must survive power loss; write volume is low), foreign keys on, and
//! numbered forward-only migrations from `src-tauri/migrations/` (planned by
//! `chorus_core::migrate`). Typed queries for each area live in the submodules.

pub mod credentials;
pub mod history;
pub mod profiles;
pub mod providers;
pub mod sessions;
pub mod settings;

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
fn migrate(conn: &mut Connection) -> Result<u32, String> {
    use chorus_core::migrate::{from_files, pending};
    let all = from_files(MIGRATION_FILES).map_err(|e| e.to_string())?;
    let current: u32 =
        conn.pragma_query_value(None, "user_version", |row| row.get(0)).map_err(|e| format!("reading schema version: {e}"))?;
    let mut version = current;
    for m in pending(current, &all).map_err(|e| e.to_string())? {
        let apply = |conn: &mut Connection| -> rusqlite::Result<()> {
            let tx = conn.transaction()?;
            tx.execute_batch(m.sql)?;
            tx.pragma_update(None, "user_version", m.version)?;
            tx.commit()
        };
        apply(conn).map_err(|e| format!("migration {:04}_{}: {e}", m.version, m.name))?;
        version = m.version;
    }
    Ok(version)
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
}
