//! P18: the hand-off note's files (spec 18-1 §3.1). Reads are bounded, writes go through a
//! temporary file and a rename, and the repository's `info/exclude` learns the note's two lines.

use std::io::Write;
use std::path::{Path, PathBuf};

use chorus_core::handoff::{self, Entry, MAX_READ_BYTES};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::git;

/// Whether git ignores the note in this folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum IgnoreState {
    Ignored,
    /// In a repository whose exclude file doesn't list the note yet (before the first write).
    NotIgnored,
    NotARepo,
}

pub fn note_path(folder: &Path) -> PathBuf {
    folder.join(".chorus").join("handoff.md")
}

pub fn archive_path(folder: &Path) -> PathBuf {
    folder.join(".chorus").join("handoff-archive.md")
}

/// The file's text, `None` when it doesn't exist. Too big, or not UTF-8, is an error with the reason.
pub fn read(path: &Path) -> Result<Option<String>, String> {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("reading {}: {e}", path.display())),
    };
    if meta.len() > MAX_READ_BYTES as u64 {
        return Err(format!("The note is larger than {} KiB, so Chorus doesn't read it.", MAX_READ_BYTES / 1024));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    String::from_utf8(bytes).map(Some).map_err(|_| "The note isn't UTF-8 text.".to_owned())
}

/// Writes `text` to a temporary file beside `path`, then renames it over `path` (Windows replaces
/// the old file). A failed write leaves the old file as it was.
pub fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let dir = path.parent().ok_or_else(|| format!("{} has no folder", path.display()))?;
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let written = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("writing {}: {e}", path.display()));
    }
    Ok(())
}

/// Adds the note's two lines to the repository's exclude file if they're missing (K5).
pub fn ensure_excluded(folder: &Path) -> Result<IgnoreState, String> {
    let Some(file) = git::exclude_file(folder)? else { return Ok(IgnoreState::NotARepo) };
    let existing = match std::fs::read_to_string(&file) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("reading {}: {e}", file.display())),
    };
    if let Some(text) = handoff::with_exclude(&existing) {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
        }
        std::fs::write(&file, text).map_err(|e| format!("writing {}: {e}", file.display()))?;
    }
    Ok(IgnoreState::Ignored)
}

/// Whether git ignores the note, without changing anything.
pub fn ignore_state(folder: &Path) -> Result<IgnoreState, String> {
    let Some(file) = git::exclude_file(folder)? else { return Ok(IgnoreState::NotARepo) };
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    Ok(if handoff::with_exclude(&existing).is_none() { IgnoreState::Ignored } else { IgnoreState::NotIgnored })
}

/// Puts moved entries on top of the archive.
pub fn append_archive(folder: &Path, moved: &[Entry]) -> Result<(), String> {
    let path = archive_path(folder);
    let existing = read(&path)?;
    write_atomic(&path, &handoff::archive(existing.as_deref(), moved))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("chorus-note-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_write_replaces_the_note_and_leaves_no_temporary_file() {
        let dir = scratch("write");
        let path = note_path(&dir);
        assert_eq!(read(&path).unwrap(), None);
        write_atomic(&path, "one\n").unwrap();
        write_atomic(&path, "two\n").unwrap();
        assert_eq!(read(&path).unwrap().as_deref(), Some("two\n"));
        assert!(!dir.join(".chorus").join("handoff.md.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn text_that_isnt_utf8_is_refused_with_a_reason() {
        let dir = scratch("utf8");
        let path = note_path(&dir);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert!(read(&path).unwrap_err().contains("UTF-8"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_folder_outside_any_repository_needs_no_ignore() {
        let dir = scratch("plain");
        assert_eq!(ignore_state(&dir).unwrap(), IgnoreState::NotARepo);
        assert_eq!(ensure_excluded(&dir).unwrap(), IgnoreState::NotARepo);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
