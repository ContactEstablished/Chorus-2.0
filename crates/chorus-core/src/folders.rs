//! Folders as New session handles them: comparing Windows paths, and the recent-folders list
//! (newest first, as in Chorus 1, capped at [`MAX_RECENT`]).

pub const MAX_RECENT: usize = 10;

/// A path in a form two spellings of the same folder share: backslashes, no trailing separator,
/// lowercase (Windows paths are case-insensitive).
pub fn key(path: &str) -> String {
    let p = path.trim().replace('/', "\\");
    let p = p.trim_end_matches('\\');
    // A drive root keeps its backslash: `C:\`, not `C:`.
    let p = if p.len() == 2 && p.ends_with(':') { format!("{p}\\") } else { p.to_owned() };
    p.to_lowercase()
}

pub fn same(a: &str, b: &str) -> bool {
    key(a) == key(b)
}

/// Whether `path` is `folder` or inside it.
pub fn inside(path: &str, folder: &str) -> bool {
    let (p, f) = (key(path), key(folder));
    p == f || p.starts_with(&if f.ends_with('\\') { f } else { format!("{f}\\") })
}

/// Joins a path an agent wrote (absolute, or relative to the session folder) onto `folder`, resolving
/// `.` and `..` by text. `None` when it is empty, has characters Windows forbids in a name, or climbs out
/// of its own root. Whether it is inside the folder is a separate check ([`inside`]).
pub fn resolve_in(raw: &str, folder: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.chars().any(|c| c.is_control() || matches!(c, '<' | '>' | '"' | '|' | '?' | '*')) {
        return None;
    }
    let raw = raw.replace('/', "\\");
    let bytes = raw.as_bytes();
    let absolute = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    let joined = if absolute { raw } else { format!("{}\\{}", folder.trim().replace('/', "\\").trim_end_matches('\\'), raw) };
    let mut parts: Vec<&str> = Vec::new();
    let mut segments = joined.split('\\');
    let drive = segments.next()?;
    if drive.len() != 2 || !drive.ends_with(':') {
        return None;
    }
    for seg in segments {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(if parts.is_empty() { format!("{drive}\\") } else { format!("{drive}\\{}", parts.join("\\")) })
}

/// Files that run code when opened. Chorus shows these in Explorer instead of launching them.
pub fn runs_code(path: &str) -> bool {
    let name = path.rsplit(['\\', '/']).next().unwrap_or("");
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e).to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "exe" | "com" | "bat" | "cmd" | "ps1" | "psm1" | "vbs" | "vbe" | "js" | "jse" | "wsf" | "wsh" | "msi" | "msp"
            | "scr" | "lnk" | "url" | "reg" | "hta" | "cpl" | "jar" | "appx" | "msix" | "dll" | "sys" | "pif" | "gadget"
            | "msc" | "chm" | "scf" | "ws" | "wsc" | "inf" | "py" | "pyw" | "ps1xml" | "appref-ms" | "application"
            | "library-ms" | "settingcontent-ms"
    )
}

/// How a click on an agent's path is treated, once symlinks and `..` have been resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathAccess {
    /// Inside the session's folder: opens straight away.
    Inside,
    /// Outside it, on this machine: opens only after the person approves the real path.
    AskFirst,
    /// Outside it, on a network share: never opened, because reaching a share can send it the
    /// person's Windows credentials.
    Refused,
}

/// `real` and `root` are canonical paths, which Windows writes as `\\?\C:\…` or `\\?\UNC\server\…`.
pub fn path_access(real: &str, root: &str) -> PathAccess {
    if inside(real, root) {
        PathAccess::Inside
    } else if is_local_drive(real) {
        PathAccess::AskFirst
    } else {
        PathAccess::Refused
    }
}

fn is_local_drive(path: &str) -> bool {
    let bytes = path.strip_prefix(r"\\?\").unwrap_or(path).as_bytes();
    bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\'
}

/// What opening a path does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAction {
    OpenFolder,
    OpenFile,
    /// The file can run code, so it is selected in Explorer and never launched.
    ShowInExplorer,
}

pub fn open_action(path: &str, is_dir: bool) -> OpenAction {
    if is_dir {
        OpenAction::OpenFolder
    } else if runs_code(path) {
        OpenAction::ShowInExplorer
    } else {
        OpenAction::OpenFile
    }
}

/// The dialog that approves a path outside the session's folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalPrompt {
    pub title: &'static str,
    pub message: String,
    pub confirm: &'static str,
}

/// `real` is the resolved path, so the person approves where it really leads rather than what the
/// agent wrote.
pub fn approval_prompt(action: OpenAction, real: &str) -> ApprovalPrompt {
    let (noun, confirm) = match action {
        OpenAction::OpenFolder => ("folder", "Open folder"),
        OpenAction::OpenFile => ("file", "Open file"),
        OpenAction::ShowInExplorer => ("file, which can run code, so it will only be shown in Explorer", "Show in Explorer"),
    };
    ApprovalPrompt {
        title: "Outside this session's folder",
        message: format!(
            "An agent's message points to this {noun}:\n\n{}\n\nContinue only if you expected this path.",
            wrapped_path(&readable_path(real))
        ),
        confirm,
    }
}

/// The longest line of a path in the approval dialog.
const PATH_LINE: usize = 60;

/// Windows' task dialog replaces the middle of a long path with `...`, which would hide the part the
/// person is approving, but it leaves short lines whole. Breaks after a separator where it can.
fn wrapped_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len() + 8);
    let mut line = 0;
    for segment in path.split_inclusive('\\') {
        let length = segment.chars().count();
        if line > 0 && line + length > PATH_LINE {
            out.push('\n');
            line = 0;
        }
        for c in segment.chars() {
            if line == PATH_LINE {
                out.push('\n');
                line = 0;
            }
            out.push(c);
            line += 1;
        }
    }
    out
}

/// A canonical path as a person reads it: no `\\?\` prefix, and no invisible direction marks, which
/// could make a file name appear to be something it isn't.
pub fn readable_path(path: &str) -> String {
    path.strip_prefix(r"\\?\")
        .unwrap_or(path)
        .chars()
        .map(|c| if matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}') { '\u{FFFD}' } else { c })
        .collect()
}

/// `path`'s place inside `folder`, as a relative path with backslashes; empty for `folder` itself.
pub fn relative(path: &str, folder: &str) -> Option<String> {
    if !inside(path, folder) {
        return None;
    }
    let p = path.trim().replace('/', "\\");
    let p = p.trim_end_matches('\\');
    let f = folder.trim().replace('/', "\\");
    let skip = f.trim_end_matches('\\').chars().count();
    Some(p.chars().skip(skip).collect::<String>().trim_start_matches('\\').to_owned())
}

/// The list with `folder` put first, any other spelling of it removed.
pub fn remember(recent: &[String], folder: &str) -> Vec<String> {
    let folder = folder.trim().trim_end_matches(['\\', '/']).to_owned();
    std::iter::once(folder.clone()).chain(recent.iter().filter(|r| !same(r, &folder)).cloned()).take(MAX_RECENT).collect()
}

/// A folder without trailing separators, except a drive root, which keeps its own (`D:\`): `D:`
/// alone means the current folder on drive D.
pub fn trim_trailing_separators(path: &str) -> String {
    let t = path.trim_end_matches(['\\', '/']);
    if t.len() == 2 && t.ends_with(':') && t.as_bytes()[0].is_ascii_alphabetic() {
        format!("{t}\\")
    } else {
        t.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_outside_the_folder_asks_first_and_a_network_one_is_refused() {
        let root = r"\\?\C:\Projects\App";
        assert_eq!(path_access(r"\\?\C:\Projects\App\_docs\a.md", root), PathAccess::Inside);
        assert_eq!(path_access(r"\\?\c:\projects\app", root), PathAccess::Inside);
        assert_eq!(path_access(r"\\?\C:\Projects\App2\a.md", root), PathAccess::AskFirst);
        assert_eq!(path_access(r"\\?\C:\Users\me\AppData\Local\Temp\a.md", root), PathAccess::AskFirst);
        assert_eq!(path_access(r"\\?\D:\other", root), PathAccess::AskFirst);
        assert_eq!(path_access(r"\\?\UNC\server\share\a.md", root), PathAccess::Refused);
        assert_eq!(path_access(r"\\server\share\a.md", root), PathAccess::Refused);
        assert_eq!(path_access(r"\\?\Volume{1234}\a.md", root), PathAccess::Refused);
        // A session whose own folder is on a share keeps working inside that folder.
        assert_eq!(path_access(r"\\?\UNC\server\share\app\a.md", r"\\?\UNC\server\share\app"), PathAccess::Inside);
    }

    #[test]
    fn code_is_only_ever_shown_whether_or_not_it_is_inside_the_folder() {
        assert_eq!(open_action(r"C:\x\setup.exe", false), OpenAction::ShowInExplorer);
        assert_eq!(open_action(r"C:\x\console.msc", false), OpenAction::ShowInExplorer);
        assert_eq!(open_action(r"C:\x\app.appref-ms", false), OpenAction::ShowInExplorer);
        assert_eq!(open_action(r"C:\x\notes.md", false), OpenAction::OpenFile);
        assert_eq!(open_action(r"C:\x\folder.exe", true), OpenAction::OpenFolder);
    }

    #[test]
    fn the_approval_shows_the_real_path_and_says_what_will_happen() {
        let file = approval_prompt(OpenAction::OpenFile, r"\\?\C:\Temp\report.md");
        assert!(file.message.contains(r"C:\Temp\report.md") && !file.message.contains(r"\\?\"));
        assert_eq!(file.confirm, "Open file");
        assert_eq!(approval_prompt(OpenAction::OpenFolder, r"\\?\C:\Temp").confirm, "Open folder");
        let code = approval_prompt(OpenAction::ShowInExplorer, r"\\?\C:\Temp\a.exe");
        assert!(code.message.contains("only be shown in Explorer"));
        assert_eq!(code.confirm, "Show in Explorer");
    }

    #[test]
    fn a_long_path_is_wrapped_so_windows_cannot_hide_its_middle() {
        let long = format!(r"C:\Users\me\AppData\Local\Temp\claude\{}\scratchpad\report.md", "x".repeat(150));
        let wrapped = wrapped_path(&long);
        assert!(wrapped.lines().all(|l| l.chars().count() <= PATH_LINE), "{wrapped}");
        assert_eq!(wrapped.replace('\n', ""), long);
        assert!(wrapped.lines().next().is_some_and(|l| l.ends_with('\\')));
        assert_eq!(wrapped_path(r"C:\short\a.md"), r"C:\short\a.md");
        let prompt = approval_prompt(OpenAction::OpenFile, &format!(r"\\?\{long}"));
        assert_eq!(prompt.message.replace('\n', "").matches(&long).count(), 1);
    }

    #[test]
    fn a_file_name_cannot_disguise_itself_with_direction_marks() {
        assert_eq!(readable_path("\\\\?\\C:\\x\\a\u{202E}dm.exe"), "C:\\x\\a\u{FFFD}dm.exe");
        assert_eq!(readable_path(r"\\?\UNC\server\share"), r"UNC\server\share");
        assert_eq!(readable_path(r"C:\plain path\é.md"), r"C:\plain path\é.md");
    }

    #[test]
    fn agent_paths_resolve_by_text_and_cannot_leave_the_folder_unnoticed() {
        let f = r"C:\Projects\App";
        assert_eq!(resolve_in(r"C:\Projects\App\_docs\a b.md", f).as_deref(), Some(r"C:\Projects\App\_docs\a b.md"));
        assert_eq!(resolve_in("_docs/x/", f).as_deref(), Some(r"C:\Projects\App\_docs\x"));
        assert_eq!(resolve_in("./src/../README.md", f).as_deref(), Some(r"C:\Projects\App\README.md"));
        let escaped = resolve_in(r"..\Other\x.txt", f).unwrap();
        assert!(!inside(&escaped, f));
        assert_eq!(resolve_in(r"..\..\..\x", f), None);
        assert_eq!(resolve_in("a|b", f), None);
        assert_eq!(resolve_in("", f), None);
    }

    #[test]
    fn code_running_files_are_not_launched() {
        assert!(runs_code(r"C:\x\Setup.EXE") && runs_code("a.ps1") && runs_code("b.lnk"));
        assert!(!runs_code("notes.md") && !runs_code(r"C:\x\folder.d\readme"));
    }

    #[test]
    fn trailing_separators_go_but_a_drive_root_keeps_its_own() {
        assert_eq!(trim_trailing_separators(r"C:\work\"), r"C:\work");
        assert_eq!(trim_trailing_separators("C:/work//"), "C:/work");
        assert_eq!(trim_trailing_separators(r"D:\"), r"D:\");
        assert_eq!(trim_trailing_separators("D:/"), r"D:\");
        assert_eq!(trim_trailing_separators("D:"), r"D:\");
    }

    #[test]
    fn paths_compare_as_windows_does() {
        assert!(same(r"C:\Work\Ledger\", "c:/work/ledger"));
        assert!(inside(r"C:\work\ledger\web", r"C:\Work\Ledger"));
        assert!(!inside(r"C:\work\ledger-old", r"C:\work\ledger"));
        assert!(inside(r"D:\x", "D:"), "a drive root holds everything on it");
        assert_eq!(relative(r"C:\work\ledger\web\src", r"c:\work\ledger").as_deref(), Some(r"web\src"));
        assert_eq!(relative(r"C:\work\ledger", r"C:\work\ledger\").as_deref(), Some(""));
        assert_eq!(relative(r"C:\elsewhere", r"C:\work"), None);
    }

    #[test]
    fn the_newest_folder_goes_first_once() {
        let recent = vec![r"C:\a".to_owned(), r"C:\b".to_owned()];
        assert_eq!(remember(&recent, r"c:\B\"), [r"c:\B", r"C:\a"]);
        let many: Vec<String> = (0..MAX_RECENT).map(|i| format!(r"C:\f{i}")).collect();
        let next = remember(&many, r"C:\new");
        assert_eq!((next.len(), next[0].as_str()), (MAX_RECENT, r"C:\new"));
    }
}
