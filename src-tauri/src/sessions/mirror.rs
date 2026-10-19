//! A session's scrollback file (ARCHITECTURE §4): the scrubbed bytes its views were sent, capped to
//! the newest few megabytes by the rules in `chorus_core::scrollback`.
//!
//! It lives in the app's data folder (never a project folder, where an agent could commit it, and
//! never TEMP). Failures cost history, never the session: every error is kept as a message the UI
//! shows, and the session carries on.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chorus_core::scrollback::{MAX_BYTES, cap_tail, exceeds_slack, prepare_history};

pub struct Mirror {
    path: PathBuf,
    file: Option<File>,
    len: usize,
    max: usize,
    /// The last failure, shown in the session header ("scrollback isn't being saved").
    pub error: Option<String>,
}

impl Mirror {
    pub fn open(dir: &Path, session_id: u32) -> Self {
        Self::with_cap(dir, session_id, MAX_BYTES)
    }

    pub fn with_cap(dir: &Path, session_id: u32, max: usize) -> Self {
        let path = dir.join(format!("{session_id}.log"));
        let mut mirror = Self { path, file: None, len: 0, max, error: None };
        if let Err(e) = fs::create_dir_all(dir).and_then(|()| mirror.reopen()) {
            mirror.fail("opening", &e);
        }
        mirror
    }

    fn reopen(&mut self) -> std::io::Result<()> {
        let file = OpenOptions::new().create(true).append(true).open(&self.path)?;
        self.len = file.metadata()?.len() as usize;
        self.file = Some(file);
        Ok(())
    }

    fn fail(&mut self, doing: &str, e: &std::io::Error) {
        self.error = Some(format!("Scrollback isn't being saved ({doing} {}: {e}).", self.path.display()));
    }

    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends bytes that have already been scrubbed.
    pub fn append(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        let Some(file) = self.file.as_mut() else { return };
        if let Err(e) = file.write_all(bytes) {
            self.fail("writing", &e);
            return;
        }
        self.len += bytes.len();
        if exceeds_slack(self.len, self.max) {
            let capped = self.read_raw().map(|old| cap_tail(&old, self.max).to_vec());
            match capped {
                Ok(capped) => self.replace(&capped, "trimming"),
                Err(e) => self.fail("trimming", &e),
            }
        }
    }

    /// Everything a newly attached view should be sent first.
    pub fn read_all(&mut self) -> Vec<u8> {
        match self.read_raw() {
            Ok(bytes) => cap_tail(&bytes, self.max).to_vec(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                self.fail("reading", &e);
                Vec::new()
            }
        }
    }

    /// Rewrites the file for a new run: the old output kept in scrollback above `boundary`.
    pub fn begin_new_run(&mut self, boundary: &str) {
        let old = self.read_raw().unwrap_or_default();
        let prepared = prepare_history(&old, self.max, boundary);
        self.replace(&prepared, "preparing history");
    }

    fn read_raw(&mut self) -> std::io::Result<Vec<u8>> {
        if let Some(file) = self.file.as_mut() {
            file.flush()?;
        }
        fs::read(&self.path)
    }

    /// Replaces the contents through a temporary file and a rename, so a crash mid-way leaves the
    /// old file rather than half a new one.
    fn replace(&mut self, bytes: &[u8], doing: &str) {
        let tmp = self.path.with_extension("log.tmp");
        self.file = None;
        let result = fs::write(&tmp, bytes).and_then(|()| fs::rename(&tmp, &self.path)).and_then(|()| self.reopen());
        if let Err(e) = result {
            self.fail(doing, &e);
            let _ = self.reopen();
        }
    }

    /// Deletes the file (on Close: an orphan would be a plaintext record of work with nothing
    /// pointing at it).
    pub fn remove(mut self) {
        self.file = None;
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(self.path.with_extension("log.tmp"));
    }
}

/// Deletes scrollback files whose session is no longer open. Returns how many were removed.
pub fn prune(dir: &Path, open_sessions: &[u32]) -> usize {
    let Ok(entries) = fs::read_dir(dir) else { return 0 };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let id = name.strip_suffix(".log.tmp").or_else(|| name.strip_suffix(".log")).and_then(|s| s.parse::<u32>().ok());
        let keep = name.ends_with(".log") && id.is_some_and(|id| open_sessions.contains(&id));
        if id.is_some() && !keep && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("chorus-mirror-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn appends_survive_reopening_and_are_capped() {
        let dir = temp_dir("cap");
        {
            let mut m = Mirror::with_cap(&dir, 7, 100);
            m.append(b"hello\n");
            assert_eq!(m.read_all(), b"hello\n");
        }
        let mut m = Mirror::with_cap(&dir, 7, 100);
        for i in 0..100 {
            m.append(format!("line {i}\n").as_bytes());
        }
        let all = m.read_all();
        assert!(all.len() <= 100);
        assert!(all.ends_with(b"line 99\n"));
        assert!(fs::metadata(m.path()).unwrap().len() <= 125, "trimmed within the slack");
        assert!(m.error.is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_run_keeps_history_above_the_boundary() {
        let dir = temp_dir("run");
        let mut m = Mirror::with_cap(&dir, 1, 4096);
        m.append(b"\x1b[?1049hold screen");
        m.begin_new_run("-- Restarted --");
        m.append(b"new output");
        let all = String::from_utf8(m.read_all()).unwrap();
        let old = all.find("old screen").unwrap();
        let boundary = all.find("-- Restarted --").unwrap();
        let new = all.find("new output").unwrap();
        assert!(old < boundary && boundary < new);
        assert!(!all.contains("\x1b[?1049h"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_removes_closed_sessions_and_debris() {
        let dir = temp_dir("prune");
        fs::create_dir_all(&dir).unwrap();
        for name in ["1.log", "2.log", "2.log.tmp", "notes.txt"] {
            fs::write(dir.join(name), b"x").unwrap();
        }
        assert_eq!(prune(&dir, &[1]), 2);
        assert!(dir.join("1.log").exists() && dir.join("notes.txt").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
