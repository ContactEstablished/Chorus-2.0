//! Mechanical enforcement of the pure-core rule (ARCHITECTURE §2, D11).
//!
//! The library in `src/` may not name `std::fs`, `std::process`, `std::net` or `std::env`, may not
//! read the clock, and may depend only on pure crates. This test reads the crate's own sources (a
//! test may do IO; the library may not).

// clippy.toml forbids IO across this crate; this test is the one place that must read files.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::fs;
use std::path::{Path, PathBuf};

const FORBIDDEN_MODULES: &[&str] = &["fs", "process", "net", "env"];
const FORBIDDEN_PATHS: &[&str] = &["std::time::SystemTime", "std::time::Instant"];
/// Dependencies that are known to be pure. Add to this list only with a reason.
const ALLOWED_DEPENDENCIES: &[&str] = &["serde", "serde_json", "thiserror", "specta"];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read src dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Source with `//` comments removed, so prose about the rule doesn't trip it.
fn code_only(source: &str) -> String {
    source
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `code` with runs of whitespace collapsed and none left around `::`, braces or commas, so
/// `use std :: { fs }` reads as `use std::{fs}`.
fn normalise(code: &str) -> String {
    let mut s = code.split_whitespace().collect::<Vec<_>>().join(" ");
    for (from, to) in [(" ::", "::"), (":: ", "::"), ("{ ", "{"), (" }", "}"), (", ", ","), (" ,", ",")] {
        while s.contains(from) {
            s = s.replace(from, to);
        }
    }
    s
}

/// Every forbidden use in `code`, as human-readable strings.
fn violations(code: &str) -> Vec<String> {
    let compact = normalise(code);
    let mut found = Vec::new();
    for module in FORBIDDEN_MODULES {
        for prefix in ["std::", "core::"] {
            let path = format!("{prefix}{module}");
            let mut rest = compact.as_str();
            while let Some(i) = rest.find(&path) {
                let after = &rest[i + path.len()..];
                let before_ok = i == 0 || !rest[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_');
                let after_ok = !after.starts_with(|c: char| c.is_alphanumeric() || c == '_');
                if before_ok && after_ok {
                    found.push(path.clone());
                }
                rest = after;
            }
        }
    }
    // Grouped imports: `use std::{fs, io};`
    let mut rest = compact.as_str();
    while let Some(i) = rest.find("std::{") {
        let group = &rest[i + "std::{".len()..];
        let end = group.find('}').unwrap_or(group.len());
        for item in group[..end].split(',') {
            let head = item.split("::").next().unwrap_or("");
            if FORBIDDEN_MODULES.contains(&head) {
                found.push(format!("std::{{{head}}}"));
            }
        }
        rest = group;
    }
    for path in FORBIDDEN_PATHS {
        if compact.contains(path) {
            found.push((*path).to_owned());
        }
    }
    found
}

#[test]
fn library_sources_do_no_io() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(!files.is_empty(), "no sources found under {}", src.display());

    let mut problems = Vec::new();
    for file in &files {
        let code = code_only(&fs::read_to_string(file).expect("read source"));
        for v in violations(&code) {
            problems.push(format!("{}: uses {v}", file.strip_prefix(&src).unwrap_or(file).display()));
        }
    }
    assert!(problems.is_empty(), "chorus-core must stay pure; move IO to src-tauri:\n{}", problems.join("\n"));
}

#[test]
fn dependencies_are_pure() {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).expect("read Cargo.toml");
    let mut section = String::new();
    let mut problems = Vec::new();
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            section = line.trim_matches(|c| c == '[' || c == ']').to_owned();
            continue;
        }
        // Normal and build dependencies ship in the library; dev-dependencies don't.
        let in_shipped_deps = section == "dependencies"
            || section == "build-dependencies"
            || (section.starts_with("target.") && !section.ends_with("dev-dependencies"));
        if !in_shipped_deps || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let name = line.split(['=', '.']).next().unwrap_or("").trim();
        if !name.is_empty() && !ALLOWED_DEPENDENCIES.contains(&name) {
            problems.push(name.to_owned());
        }
    }
    assert!(problems.is_empty(), "chorus-core may depend only on pure crates; not allowed: {problems:?}");
}

#[test]
fn the_checker_catches_what_it_should() {
    assert_eq!(violations("use std::fs;"), vec!["std::fs"]);
    assert_eq!(violations("let _ = std::process::Command::new(\"x\");"), vec!["std::process"]);
    assert_eq!(violations("use std::{collections::HashMap, net::TcpStream};"), vec!["std::{net}"]);
    assert_eq!(violations("use std :: env ;"), vec!["std::env"]);
    assert_eq!(violations("let t = std::time::Instant::now();"), vec!["std::time::Instant"]);
    assert!(violations("use std::fmt; use std::collections::HashMap; let fsx = 1;").is_empty());
    assert!(violations(&code_only("// std::fs is forbidden here")).is_empty());
}
