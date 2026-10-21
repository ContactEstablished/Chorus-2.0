//! Resolve an npm `.cmd` shim to the file it really launches (LESSONS L14).
//!
//! Ported from Chorus 1's `cliShimCore.ts`. Launching a shim through `cmd.exe /c` lets cmd re-parse
//! the command line: an argument holding a quote plus `>`, `|` or `&` turns into redirection
//! (Chorus 1 F96). So 2.0 reads the shim and launches its target directly. npm's template is a
//! convention, not a contract: anything unrecognised returns `None`, and the caller must report it
//! rather than guess.

/// What an npm shim launches, with `%dp0%` resolved against the shim's folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShimTarget {
    pub file: String,
    pub kind: ShimKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShimKind {
    /// Spawn directly.
    Executable,
    /// Run with `node`.
    NodeScript,
}

/// npm's shims are a few hundred bytes; anything bigger is not one.
pub const MAX_SHIM_BYTES: usize = 8192;

/// Parses `shim_text` (the contents of `shim_path`). The invocation line is the single line ending
/// in `%*`; the target is its single `%dp0%`-relative quoted token.
pub fn parse_npm_shim(shim_text: &str, shim_path: &str) -> Option<ShimTarget> {
    if shim_text.is_empty() || shim_text.len() > MAX_SHIM_BYTES {
        return None;
    }
    let mut invocations = shim_text.lines().map(str::trim).filter(|l| l.ends_with("%*"));
    let line = invocations.next()?;
    if invocations.next().is_some() {
        return None;
    }

    let mut relative = quoted_tokens(line).filter_map(strip_dp0);
    let rest = relative.next()?;
    if relative.next().is_some() || rest.contains('%') {
        return None;
    }

    let dir = windows_dirname(shim_path)?;
    let file = format!("{}\\{}", dir.trim_end_matches(['\\', '/']), collapse_separators(rest));
    let kind = classify(&file)?;
    Some(ShimTarget { file, kind })
}

fn quoted_tokens(line: &str) -> impl Iterator<Item = &str> {
    line.split('"').skip(1).step_by(2)
}

/// `%dp0%\rest` or `%~dp0\rest` → `rest`.
fn strip_dp0(token: &str) -> Option<&str> {
    let lower = token.to_ascii_lowercase();
    if !(lower.starts_with("%dp0%") || lower.starts_with("%~dp0")) {
        return None;
    }
    let rest = token[5..].trim_start_matches(['\\', '/']);
    (!rest.is_empty()).then_some(rest)
}

fn classify(file: &str) -> Option<ShimKind> {
    let lower = file.to_ascii_lowercase();
    if lower.ends_with(".exe") {
        Some(ShimKind::Executable)
    } else if [".js", ".mjs", ".cjs"].iter().any(|e| lower.ends_with(e)) {
        Some(ShimKind::NodeScript)
    } else {
        None
    }
}

fn windows_dirname(path: &str) -> Option<&str> {
    let cut = path.rfind(['\\', '/'])?;
    (cut > 0).then(|| &path[..cut])
}

fn collapse_separators(rest: &str) -> String {
    let mut out = String::with_capacity(rest.len());
    let mut last_sep = false;
    for c in rest.chars() {
        let sep = c == '\\' || c == '/';
        if !(sep && last_sep) {
            out.push(if sep { '\\' } else { c });
        }
        last_sep = sep;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // codex-cli 0.162.1's shim, as installed by npm 11 (2026-10-18).
    const CODEX_SHIM: &str = "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\r\nIF EXIST \"%dp0%\\node.exe\" (\r\n  SET \"_prog=%dp0%\\node.exe\"\r\n) ELSE (\r\n  SET \"_prog=node\"\r\n  SET PATHEXT=%PATHEXT:;.JS;=;%\r\n)\r\n\r\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  \"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*\r\n";

    #[test]
    fn resolves_codex_to_its_node_script() {
        let t = parse_npm_shim(CODEX_SHIM, "C:\\npm\\codex.cmd").expect("recognised");
        assert_eq!(t.file, "C:\\npm\\node_modules\\@openai\\codex\\bin\\codex.js");
        assert_eq!(t.kind, ShimKind::NodeScript);
    }

    #[test]
    fn resolves_an_exe_target() {
        // opencode 1.18.34's shim.
        let shim = "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\"%dp0%\\node_modules\\opencode-ai\\bin\\opencode.exe\"   %*\r\n";
        let t = parse_npm_shim(shim, "C:\\npm\\opencode.cmd").expect("recognised");
        assert_eq!(t.file, "C:\\npm\\node_modules\\opencode-ai\\bin\\opencode.exe");
        assert_eq!(t.kind, ShimKind::Executable);
    }

    #[test]
    fn resolves_gemini_to_its_bundle() {
        // Gemini CLI 0.64.0's shim, as installed by npm 11 (2026-10-20).
        const GEMINI_SHIM: &str = "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\r\nIF EXIST \"%dp0%\\node.exe\" (\r\n  SET \"_prog=%dp0%\\node.exe\"\r\n) ELSE (\r\n  SET \"_prog=node\"\r\n  SET PATHEXT=%PATHEXT:;.JS;=;%\r\n)\r\n\r\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  \"%dp0%\\node_modules\\@google\\gemini-cli\\bundle\\gemini.js\" %*\r\n";
        let t = parse_npm_shim(GEMINI_SHIM, "C:\\npm\\gemini.cmd").expect("recognised");
        assert_eq!(t.file, "C:\\npm\\node_modules\\@google\\gemini-cli\\bundle\\gemini.js");
        assert_eq!(t.kind, ShimKind::NodeScript);
    }

    #[test]
    fn refuses_shapes_it_has_not_seen() {
        assert_eq!(parse_npm_shim("", "C:\\npm\\x.cmd"), None);
        assert_eq!(parse_npm_shim("\"%dp0%\\a.js\" %*\n\"%dp0%\\b.js\" %*\n", "C:\\npm\\x.cmd"), None);
        assert_eq!(parse_npm_shim("\"%dp0%\\%FOO%\\a.js\" %*\n", "C:\\npm\\x.cmd"), None);
        assert_eq!(parse_npm_shim("\"%dp0%\\a.ps1\" %*\n", "C:\\npm\\x.cmd"), None);
        assert_eq!(parse_npm_shim("\"C:\\abs\\a.js\" %*\n", "C:\\npm\\x.cmd"), None);
    }
}
