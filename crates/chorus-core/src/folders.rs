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

#[cfg(test)]
mod tests {
    use super::*;

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
