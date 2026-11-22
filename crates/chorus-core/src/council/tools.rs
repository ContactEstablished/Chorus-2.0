//! The three read-only tools a participant may call (D137): their definitions for OpenRouter's
//! function calling, argument checks, the turn's budget, and how results are formatted. The
//! service reads the files; everything decidable from inputs is here.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use specta::Type;

use super::files;

/// Most rows `list_files` returns.
pub const MAX_LIST_ROWS: usize = 500;
/// Most bytes one `read_file` returns.
pub const MAX_READ_BYTES: usize = 64_000;
/// Most matching lines one `search_code` returns, and the longest line shown.
pub const MAX_SEARCH_HITS: usize = 200;
const MAX_HIT_CHARS: usize = 240;

/// The function definitions, in the OpenAI shape OpenRouter accepts.
pub fn definitions() -> Value {
    json!([
        {"type": "function", "function": {
            "name": "list_files",
            "description": "List the readable files and folders directly under a folder of the project, with file sizes and per-folder file counts. Omit `path` for the project's root.",
            "parameters": {"type": "object", "properties": {
                "path": {"type": "string", "description": "A folder, relative to the project root, e.g. \"src/council\". Empty for the root."}
            }, "additionalProperties": false}
        }},
        {"type": "function", "function": {
            "name": "read_file",
            "description": "Read one file of the project as numbered lines. Large files come back in parts: pass start_line to continue.",
            "parameters": {"type": "object", "properties": {
                "path": {"type": "string", "description": "The file, relative to the project root, e.g. \"src/main.rs\"."},
                "start_line": {"type": "integer", "minimum": 1, "description": "First line to read (1-based). Default 1."},
                "end_line": {"type": "integer", "minimum": 1, "description": "Last line to read, inclusive. Default: as far as the size limit allows."}
            }, "required": ["path"], "additionalProperties": false}
        }},
        {"type": "function", "function": {
            "name": "search_code",
            "description": "Find lines containing some text (a literal string, not a regular expression) across the project's readable files, optionally under one folder. Returns path:line: text for each match.",
            "parameters": {"type": "object", "properties": {
                "query": {"type": "string", "description": "The text to look for, at least 2 characters."},
                "path": {"type": "string", "description": "Only search under this folder. Default: the whole project."},
                "case_sensitive": {"type": "boolean", "description": "Default false."}
            }, "required": ["query"], "additionalProperties": false}
        }}
    ])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    ListFiles { dir: String },
    ReadFile { path: String, start: usize, end: Option<usize> },
    Search { query: String, dir: String, case_sensitive: bool },
}

impl Call {
    /// The tool call as one short line for the transcript and the live view.
    pub fn describe(&self) -> String {
        match self {
            Self::ListFiles { dir } => format!("list {}", if dir.is_empty() { "/" } else { dir }),
            Self::ReadFile { path, start, end } => match end {
                Some(e) => format!("read {path}:{start}-{e}"),
                None if *start > 1 => format!("read {path} from line {start}"),
                None => format!("read {path}"),
            },
            Self::Search { query, dir, .. } => {
                let q = super::verdict::clip(query, 60);
                if dir.is_empty() { format!("search \"{q}\"") } else { format!("search \"{q}\" in {dir}") }
            }
        }
    }
}

/// A tool call's arguments, checked. A model sends arguments as a JSON string.
pub fn parse(name: &str, arguments: &str) -> Result<Call, String> {
    let args: Value = if arguments.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(arguments).map_err(|_| "The tool arguments were not valid JSON.".to_owned())?
    };
    let text = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("");
    let line = |k: &str| -> Result<Option<usize>, String> {
        match args.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v
                .as_u64()
                .filter(|n| *n >= 1)
                .and_then(|n| usize::try_from(n).ok())
                .map(Some)
                .ok_or_else(|| format!("`{k}` must be a whole number of at least 1.")),
        }
    };
    match name {
        "list_files" => Ok(Call::ListFiles { dir: files::normalize(text("path"))? }),
        "read_file" => {
            let path = files::normalize(text("path"))?;
            if path.is_empty() {
                return Err("`read_file` needs a file's path.".into());
            }
            let start = line("start_line")?.unwrap_or(1);
            let end = line("end_line")?;
            if end.is_some_and(|e| e < start) {
                return Err("`end_line` is before `start_line`.".into());
            }
            Ok(Call::ReadFile { path, start, end })
        }
        "search_code" => {
            let query = text("query").to_owned();
            if query.trim().chars().count() < 2 || query.contains('\n') {
                return Err("`query` must be at least 2 characters on one line.".into());
            }
            Ok(Call::Search {
                query,
                dir: files::normalize(text("path"))?,
                case_sensitive: args.get("case_sensitive").and_then(Value::as_bool).unwrap_or(false),
            })
        }
        other => Err(format!("There is no tool named `{other}`. The tools are list_files, read_file and search_code.")),
    }
}

/// Whether `rel` is `dir` itself or inside it (the root contains everything).
pub fn under(rel: &str, dir: &str) -> bool {
    dir.is_empty() || rel == dir || rel.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
}

/// The folder listing, from the allowed files (sorted, `(path, size)`).
pub fn list(allowed: &[(String, u64)], dir: &str) -> Result<String, String> {
    use std::collections::BTreeMap;
    let mut folders: BTreeMap<&str, (usize, u64)> = BTreeMap::new();
    let mut entries: Vec<(&str, u64)> = Vec::new();
    for (path, size) in allowed {
        let rest = if dir.is_empty() {
            path.as_str()
        } else {
            match path.strip_prefix(dir).and_then(|r| r.strip_prefix('/')) {
                Some(r) => r,
                None => continue,
            }
        };
        match rest.split_once('/') {
            Some((folder, _)) => {
                let f = folders.entry(folder).or_default();
                f.0 += 1;
                f.1 += size;
            }
            None => entries.push((rest, *size)),
        }
    }
    if folders.is_empty() && entries.is_empty() {
        return Err(if dir.is_empty() {
            "The project has no readable files.".into()
        } else {
            format!("There is no readable folder `{dir}`. Call list_files on its parent to see what exists.")
        });
    }
    let mut rows: Vec<String> = folders
        .iter()
        .map(|(name, (n, size))| format!("{name}/  ({n} file{}, {})", if *n == 1 { "" } else { "s" }, bytes(*size)))
        .chain(entries.iter().map(|(name, size)| format!("{name}  ({})", bytes(*size))))
        .collect();
    let total = rows.len();
    rows.truncate(MAX_LIST_ROWS);
    let shown = if dir.is_empty() { "the project root".to_owned() } else { format!("`{dir}`") };
    let mut out = format!("Contents of {shown}:\n{}", rows.join("\n"));
    if total > MAX_LIST_ROWS {
        out.push_str(&format!("\n[{} more not shown; list a subfolder]", total - MAX_LIST_ROWS));
    }
    Ok(out)
}

pub fn bytes(n: u64) -> String {
    match n {
        0..1_000 => format!("{n} B"),
        1_000..1_000_000 => format!("{:.1} KB", n as f64 / 1e3),
        _ => format!("{:.1} MB", n as f64 / 1e6),
    }
}

/// Numbered lines `start..=end` of a file's text, stopping at [`MAX_READ_BYTES`] with a note on
/// where to continue.
pub fn number_lines(path: &str, text: &str, start: usize, end: Option<usize>) -> Result<String, String> {
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let total = if text.ends_with('\n') { lines.len() - 1 } else { lines.len() };
    if total == 0 {
        return Ok(format!("{path} is empty."));
    }
    if start > total {
        return Err(format!("{path} has {total} lines; start_line {start} is past the end."));
    }
    let last = end.unwrap_or(total).min(total);
    let width = last.to_string().len();
    let mut out = String::new();
    let mut shown_to = start - 1;
    for (i, line) in lines.iter().enumerate().take(last).skip(start - 1) {
        let row = format!("{:>width$}  {line}\n", i + 1);
        if out.len() + row.len() > MAX_READ_BYTES && shown_to >= start {
            break;
        }
        out.push_str(&row);
        shown_to = i + 1;
    }
    let mut head = format!("{path} — lines {start}–{shown_to} of {total}\n");
    if shown_to < last {
        out.push_str(&format!("[stopped at the size limit; call read_file with start_line={} to continue]\n", shown_to + 1));
    }
    head.push_str(&out);
    Ok(head)
}

/// Appends `path:line: text` for each line of `text` containing `query`, up to `limit` in all.
/// Returns false once the limit is reached.
pub fn search_into(path: &str, text: &str, query: &str, case_sensitive: bool, hits: &mut Vec<String>, limit: usize) -> bool {
    let needle = if case_sensitive { query.to_owned() } else { query.to_lowercase() };
    for (i, line) in text.lines().enumerate() {
        let found = if case_sensitive { line.contains(&needle) } else { line.to_lowercase().contains(&needle) };
        if found {
            if hits.len() >= limit {
                return false;
            }
            hits.push(format!("{path}:{}: {}", i + 1, super::verdict::clip(line.trim(), MAX_HIT_CHARS)));
        }
    }
    hits.len() < limit
}

pub fn search_result(query: &str, hits: &[String], complete: bool) -> String {
    if hits.is_empty() {
        return format!("No readable file contains \"{query}\".");
    }
    let mut out = hits.join("\n");
    if !complete {
        out.push_str(&format!("\n[stopped at {MAX_SEARCH_HITS} matches; narrow the query or give a folder]"));
    }
    out
}

/// How much one participant turn may read (Settings → Council; D137's defaults).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    pub max_calls: u32,
    pub max_bytes: u32,
    /// Stop calling tools once the conversation reaches this share of the model's context.
    pub context_share: f64,
    /// The run key's spend cap.
    pub cap_usd: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_calls: 40, max_bytes: 1_500_000, context_share: 0.7, cap_usd: 10.0 }
    }
}

impl Limits {
    pub fn check(&self) -> Result<(), String> {
        if !(1..=200).contains(&self.max_calls) {
            return Err("Tool calls per turn must be between 1 and 200.".into());
        }
        if !(10_000..=8_000_000).contains(&self.max_bytes) {
            return Err("Code read per turn must be between 10 KB and 8 MB.".into());
        }
        if !(0.2..=0.95).contains(&self.context_share) {
            return Err("The context share must be between 20 % and 95 %.".into());
        }
        if !(0.5..=100.0).contains(&self.cap_usd) {
            return Err("The spend cap must be between $0.50 and $100.".into());
        }
        Ok(())
    }
}

/// One participant turn's use of the tools.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Budget {
    pub calls: u32,
    pub bytes: u64,
}

impl Budget {
    /// Why the turn may not call another tool, if it may not.
    pub fn spent(&self, limits: &Limits, prompt_tokens: Option<u64>, context_length: Option<u64>) -> Option<String> {
        if self.calls >= limits.max_calls {
            return Some(format!("You have used all {} tool calls for this turn.", limits.max_calls));
        }
        if self.bytes >= u64::from(limits.max_bytes) {
            return Some(format!("You have read the {} of code allowed for this turn.", bytes(u64::from(limits.max_bytes))));
        }
        if let (Some(used), Some(ctx)) = (prompt_tokens, context_length)
            && ctx > 0
            && used as f64 >= ctx as f64 * limits.context_share
        {
            return Some("This conversation is near your context limit.".into());
        }
        None
    }
}

/// The note added when the budget is spent; the next request has `tool_choice: "none"`.
pub fn answer_now(reason: &str) -> String {
    format!("{reason} You cannot call any more tools. Answer now, in the required format, with what you have read; say what you could not verify.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed() -> Vec<(String, u64)> {
        vec![
            ("README.md".into(), 1200),
            ("src/council/mod.rs".into(), 4000),
            ("src/council/files.rs".into(), 3000),
            ("src/main.rs".into(), 500),
        ]
    }

    #[test]
    fn parse_checks_arguments() {
        assert_eq!(parse("list_files", "").unwrap(), Call::ListFiles { dir: String::new() });
        assert_eq!(parse("read_file", r#"{"path":"/src/main.rs","start_line":5}"#).unwrap(), Call::ReadFile {
            path: "src/main.rs".into(),
            start: 5,
            end: None
        });
        assert!(parse("read_file", r#"{"path":"../x"}"#).is_err());
        assert!(parse("read_file", r#"{"path":"a","start_line":0}"#).is_err());
        assert!(parse("read_file", r#"{"path":"a","start_line":9,"end_line":3}"#).is_err());
        assert!(parse("search_code", r#"{"query":"a"}"#).is_err());
        assert!(parse("write_file", "{}").unwrap_err().contains("no tool named"));
        assert!(parse("read_file", "{not json").is_err());
    }

    #[test]
    fn list_groups_folders() {
        let root = list(&allowed(), "").unwrap();
        assert!(root.contains("src/  (3 files, 7.5 KB)") && root.contains("README.md  (1.2 KB)"), "{root}");
        let sub = list(&allowed(), "src").unwrap();
        assert!(sub.contains("council/  (2 files") && sub.contains("main.rs  (500 B)"));
        assert!(list(&allowed(), "nope").is_err());
        assert!(!list(&allowed(), "src/council").unwrap().contains("main.rs"));
    }

    #[test]
    fn number_lines_windows_and_limits() {
        let text = "a\r\nb\nc\n";
        assert_eq!(number_lines("f", text, 2, None).unwrap(), "f — lines 2–3 of 3\n2  b\n3  c\n");
        assert!(number_lines("f", text, 9, None).is_err());
        let big = "x".repeat(1000) + "\n";
        let long = big.repeat(200);
        let out = number_lines("big", &long, 1, None).unwrap();
        assert!(out.len() < MAX_READ_BYTES + 200 && out.contains("start_line="));
    }

    #[test]
    fn search_and_budget() {
        let mut hits = Vec::new();
        assert!(search_into("a.rs", "fn Main() {}\nlet main = 1;", "main", false, &mut hits, 10));
        assert_eq!(hits, vec!["a.rs:1: fn Main() {}", "a.rs:2: let main = 1;"]);
        let mut few = Vec::new();
        assert!(!search_into("a.rs", "x\nx\nx", "x", true, &mut few, 2));
        assert!(under("src/a.rs", "src") && !under("srcx/a.rs", "src") && under("a", ""));
        let l = Limits::default();
        assert!(Budget { calls: 39, bytes: 0 }.spent(&l, Some(10), Some(100)).is_none());
        assert!(Budget { calls: 40, bytes: 0 }.spent(&l, None, None).is_some());
        assert!(Budget { calls: 1, bytes: 1_500_000 }.spent(&l, None, None).is_some());
        assert!(Budget::default().spent(&l, Some(71), Some(100)).is_some());
        assert!(l.check().is_ok() && Limits { cap_usd: 0.1, ..l }.check().is_err());
    }
}
