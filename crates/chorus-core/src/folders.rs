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
    )
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
