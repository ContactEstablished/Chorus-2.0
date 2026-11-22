//! The project's code as Council participants see it (D137): the readable file set, taken once at
//! run start, and the three read-only tools run against it. Every result is scrubbed before a
//! model sees it.

use std::path::{Path, PathBuf};

use chorus_core::council::{files, tools};
use chorus_core::scrub::{self, Pattern};

/// More files than this and the project is refused: a Council can't usefully browse it.
const MAX_FILES: usize = 200_000;

pub struct Code {
    root: PathBuf,
    /// Project-relative paths and sizes, sorted.
    allowed: Vec<(String, u64)>,
    secrets: Vec<Vec<u8>>,
    patterns: Vec<Pattern>,
}

/// What a tool call returned: the text for the model, or why it was refused (also for the model).
pub struct Ran {
    pub ok: bool,
    pub text: String,
}

impl Code {
    /// The readable files under `root`: git's tracked and unignored files when `root` is in a
    /// repository, otherwise a walk without build output. Blocked names and `exclude` (the run's
    /// own findings file) are left out.
    pub fn load(root: &Path, exclude: &[String], secrets: Vec<Vec<u8>>, patterns: Vec<Pattern>) -> Result<Self, String> {
        if !root.is_dir() {
            return Err(format!("The project's folder ({}) doesn't exist.", root.display()));
        }
        let candidates = match crate::git::ls_files(root) {
            Ok(Some(files)) => files,
            Ok(None) => walk(root)?,
            Err(e) => return Err(format!("Couldn't list the project's files with git: {e}")),
        };
        let mut allowed = Vec::new();
        for rel in candidates {
            let Ok(rel) = files::normalize(&rel) else { continue };
            if rel.is_empty() || files::blocked(&rel).is_some() || exclude.iter().any(|e| e.eq_ignore_ascii_case(&rel)) {
                continue;
            }
            // Listed by git but deleted in the working tree: not readable.
            let Ok(meta) = std::fs::metadata(root.join(&rel)) else { continue };
            if meta.is_file() {
                allowed.push((rel, meta.len()));
            }
            if allowed.len() > MAX_FILES {
                return Err(format!("The project has more than {MAX_FILES} readable files; a council can't browse it."));
            }
        }
        allowed.sort();
        Ok(Self { root: root.to_owned(), allowed, secrets, patterns })
    }

    pub fn file_count(&self) -> usize {
        self.allowed.len()
    }

    fn size_of(&self, rel: &str) -> Option<u64> {
        self.allowed.binary_search_by(|(p, _)| p.as_str().cmp(rel)).ok().map(|i| self.allowed[i].1)
    }

    /// A file's text, or why it can't be read.
    fn text(&self, rel: &str) -> Result<String, String> {
        if let Some(reason) = files::blocked(rel) {
            return Err(reason.into());
        }
        let size = self.size_of(rel).ok_or_else(|| {
            format!("`{rel}` isn't one of the project's readable files. Use list_files or search_code to find the right path.")
        })?;
        if size > files::MAX_FILE_BYTES {
            return Err(format!("`{rel}` is {}, over the {} a council reads.", tools::bytes(size), tools::bytes(files::MAX_FILE_BYTES)));
        }
        let bytes = std::fs::read(self.root.join(rel)).map_err(|_| format!("`{rel}` couldn't be read."))?;
        if files::looks_binary(&bytes) {
            return Err(format!("`{rel}` is a binary file."));
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn scrubbed(&self, s: &str) -> String {
        String::from_utf8_lossy(&scrub::scrub_all(s.as_bytes(), &self.secrets, &self.patterns)).into_owned()
    }

    pub fn run(&self, call: &tools::Call) -> Ran {
        let result = match call {
            tools::Call::ListFiles { dir } => tools::list(&self.allowed, dir),
            tools::Call::ReadFile { path, start, end } => {
                self.text(path).and_then(|t| tools::number_lines(path, &t, *start, *end))
            }
            tools::Call::Search { query, dir, case_sensitive } => {
                let mut hits = Vec::new();
                let mut complete = true;
                for (rel, size) in self.allowed.iter().filter(|(p, _)| tools::under(p, dir)) {
                    if *size > files::MAX_FILE_BYTES {
                        continue;
                    }
                    let Ok(text) = self.text(rel) else { continue };
                    if !tools::search_into(rel, &text, query, *case_sensitive, &mut hits, tools::MAX_SEARCH_HITS) {
                        complete = false;
                        break;
                    }
                }
                Ok(tools::search_result(query, &hits, complete))
            }
        };
        match result {
            Ok(text) => Ran { ok: true, text: self.scrubbed(&text) },
            Err(reason) => Ran { ok: false, text: self.scrubbed(&reason) },
        }
    }
}

/// A folder that isn't a git repository: every file, skipping build output and dependencies.
fn walk(root: &Path) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(rel) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(root.join(&rel)) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else { continue };
            let child = rel.join(&name);
            if kind.is_dir() {
                if !files::skip_dir(&name) {
                    stack.push(child);
                }
            } else if kind.is_file() {
                out.push(child.to_string_lossy().replace('\\', "/"));
                if out.len() > MAX_FILES {
                    return Err(format!("The project has more than {MAX_FILES} files; a council can't browse it."));
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("chorus-council-code-{}-{}", std::process::id(), crate::store::now_ms()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules/x")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {\n    let token = \"SECRET-VALUE-123\";\n}\n").unwrap();
        std::fs::write(dir.join(".env"), "KEY=1").unwrap();
        std::fs::write(dir.join("node_modules/x/index.js"), "x").unwrap();
        std::fs::write(dir.join("brief-Findings.md"), "old findings").unwrap();
        std::fs::write(dir.join("logo.bin"), b"PK\0\0").unwrap();
        dir
    }

    #[test]
    fn a_plain_folder_is_walked_and_secrets_never_reach_a_model() {
        let dir = fixture();
        let code = Code::load(&dir, &["brief-Findings.md".into()], vec![b"SECRET-VALUE-123".to_vec()], vec![]).unwrap();
        let listed: Vec<&str> = code.allowed.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(listed, vec!["logo.bin", "src/main.rs"]);
        let read = code.run(&tools::Call::ReadFile { path: "src/main.rs".into(), start: 1, end: None });
        assert!(read.ok && !read.text.contains("SECRET-VALUE-123") && read.text.contains("fn main()"), "{}", read.text);
        let env = code.run(&tools::Call::ReadFile { path: ".env".into(), start: 1, end: None });
        assert!(!env.ok && env.text.contains("Environment files"));
        let bin = code.run(&tools::Call::ReadFile { path: "logo.bin".into(), start: 1, end: None });
        assert!(!bin.ok && bin.text.contains("binary"));
        let missing = code.run(&tools::Call::ReadFile { path: "node_modules/x/index.js".into(), start: 1, end: None });
        assert!(!missing.ok);
        let search = code.run(&tools::Call::Search { query: "token".into(), dir: String::new(), case_sensitive: false });
        assert!(search.ok && search.text.starts_with("src/main.rs:2:") && !search.text.contains("SECRET"), "{}", search.text);
        let _ = std::fs::remove_dir_all(dir);
    }
}
