//! Which SQLite migrations to run (ARCHITECTURE §7). Migrations are numbered, forward-only files
//! in `src-tauri/migrations/` named `NNNN_name.sql`; the database records the last one applied in
//! `PRAGMA user_version`. The IO (reading the version, running the SQL) is in `src-tauri::store`.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration<'a> {
    pub version: u32,
    pub name: &'a str,
    pub sql: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrateError {
    /// A file name isn't `NNNN_name.sql`.
    BadFileName(String),
    /// Versions must run 1, 2, 3… with no gaps or repeats, so two branches can't both add 0004.
    NotContiguous { expected: u32, found: u32 },
    /// The database was written by a newer Chorus. Running against it could lose data.
    DatabaseIsNewer { database: u32, latest: u32 },
    /// A migration asks the runner for something this build doesn't know. Refused rather than run
    /// half-applied: a newer branch may have relied on it for a table rebuild.
    UnknownDirective(String),
}

impl fmt::Display for MigrateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadFileName(name) => write!(f, "migration file {name:?} is not named NNNN_name.sql"),
            Self::NotContiguous { expected, found } => {
                write!(f, "migrations must be numbered without gaps: expected {expected:04}, found {found:04}")
            }
            Self::DatabaseIsNewer { database, latest } => write!(
                f,
                "the database is at schema {database}, but this build only knows up to {latest}; \
                 it was written by a newer Chorus"
            ),
            Self::UnknownDirective(directive) => {
                write!(f, "a migration asks for {directive:?}, which this build doesn't know")
            }
        }
    }
}

impl std::error::Error for MigrateError {}

/// Parses `0003_add_runs.sql` into `(3, "add_runs")`.
pub fn parse_file_name(file_name: &str) -> Result<(u32, &str), MigrateError> {
    let bad = || MigrateError::BadFileName(file_name.to_owned());
    let stem = file_name.strip_suffix(".sql").ok_or_else(bad)?;
    let (digits, name) = stem.split_once('_').ok_or_else(bad)?;
    let name_ok = !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if digits.len() != 4 || !digits.chars().all(|c| c.is_ascii_digit()) || !name_ok {
        return Err(bad());
    }
    let version: u32 = digits.parse().map_err(|_| bad())?;
    if version == 0 {
        return Err(bad());
    }
    Ok((version, name))
}

/// What a migration asks the runner to do around its transaction, from `-- chorus: <directive>`
/// comment lines in its header (the comment lines before its first statement).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Directives {
    /// Run this file with `PRAGMA foreign_keys=OFF`, then refuse to commit if
    /// `PRAGMA foreign_key_check` finds anything. That is SQLite's documented 12-step table
    /// rebuild, and the only way to change a CHECK: with foreign keys on, dropping the old table
    /// would cascade-delete the rows other tables point at.
    pub foreign_keys_off: bool,
}

/// Reads the header directives of one migration. A directive this build doesn't know is an error,
/// not a warning: running the file without it could apply half a rebuild.
pub fn directives(sql: &str) -> Result<Directives, MigrateError> {
    let mut out = Directives::default();
    for line in sql.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(comment) = line.strip_prefix("--") else { break };
        let Some(directive) = comment.trim().strip_prefix("chorus:") else { continue };
        match directive.trim() {
            "foreign_keys off" => out.foreign_keys_off = true,
            other => return Err(MigrateError::UnknownDirective(other.to_owned())),
        }
    }
    Ok(out)
}

/// Builds the ordered migration list from `(file name, sql)` pairs, checking the numbering.
pub fn from_files<'a>(files: &[(&'a str, &'a str)]) -> Result<Vec<Migration<'a>>, MigrateError> {
    let mut all = files
        .iter()
        .map(|&(file, sql)| parse_file_name(file).map(|(version, name)| Migration { version, name, sql }))
        .collect::<Result<Vec<_>, _>>()?;
    all.sort_by_key(|m| m.version);
    for (i, m) in all.iter().enumerate() {
        let expected = i as u32 + 1;
        if m.version != expected {
            return Err(MigrateError::NotContiguous { expected, found: m.version });
        }
    }
    Ok(all)
}

/// The migrations still to apply to a database at `current`, in order.
pub fn pending<'a>(current: u32, all: &[Migration<'a>]) -> Result<Vec<Migration<'a>>, MigrateError> {
    let latest = all.last().map_or(0, |m| m.version);
    if current > latest {
        return Err(MigrateError::DatabaseIsNewer { database: current, latest });
    }
    Ok(all.iter().filter(|m| m.version > current).copied().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_well_formed_names_and_refuses_the_rest() {
        assert_eq!(parse_file_name("0001_initial.sql"), Ok((1, "initial")));
        assert_eq!(parse_file_name("0012_add_runs_2.sql"), Ok((12, "add_runs_2")));
        for bad in ["1_initial.sql", "0001initial.sql", "0001_.sql", "0001_Initial.sql", "0000_zero.sql", "0001_x.txt", "abcd_x.sql"] {
            assert!(parse_file_name(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn orders_by_number_and_refuses_gaps_and_repeats() {
        let ok = from_files(&[("0002_b.sql", "B"), ("0001_a.sql", "A")]).unwrap();
        assert_eq!(ok.iter().map(|m| m.version).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(
            from_files(&[("0001_a.sql", ""), ("0003_c.sql", "")]),
            Err(MigrateError::NotContiguous { expected: 2, found: 3 })
        );
        assert_eq!(
            from_files(&[("0001_a.sql", ""), ("0001_b.sql", "")]),
            Err(MigrateError::NotContiguous { expected: 2, found: 1 })
        );
    }

    #[test]
    fn reads_header_directives_and_refuses_unknown_ones() {
        assert_eq!(directives("CREATE TABLE t (x);").unwrap(), Directives::default());
        assert_eq!(directives("-- P14: a rebuild.\n\nCREATE TABLE t (x);").unwrap(), Directives::default());
        let rebuild = directives("-- P14: a rebuild.\n-- chorus: foreign_keys off\n\nCREATE TABLE t (x);").unwrap();
        assert!(rebuild.foreign_keys_off);
        // Only the header counts: the same words after the first statement are not a directive.
        assert_eq!(directives("CREATE TABLE t (x);\n-- chorus: foreign_keys off\n").unwrap(), Directives::default());
        assert_eq!(
            directives("-- chorus: legacy_alter_table on\nCREATE TABLE t (x);"),
            Err(MigrateError::UnknownDirective("legacy_alter_table on".into()))
        );
        assert_eq!(
            directives("-- chorus: foreign_keys off\n-- chorus: something else\n").unwrap_err().to_string(),
            "a migration asks for \"something else\", which this build doesn't know"
        );
    }

    #[test]
    fn pending_is_everything_after_the_current_version() {
        let all = from_files(&[("0001_a.sql", "A"), ("0002_b.sql", "B"), ("0003_c.sql", "C")]).unwrap();
        assert_eq!(pending(0, &all).unwrap().len(), 3);
        assert_eq!(pending(2, &all).unwrap().iter().map(|m| m.sql).collect::<Vec<_>>(), vec!["C"]);
        assert!(pending(3, &all).unwrap().is_empty());
        assert_eq!(pending(4, &all), Err(MigrateError::DatabaseIsNewer { database: 4, latest: 3 }));
    }
}
