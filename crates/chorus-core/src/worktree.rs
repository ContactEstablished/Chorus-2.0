//! Worktrees made at launch (D53, brought forward from PLAN P5): where one goes, what its branch is
//! called, and reading git's answers. 2.0 marks its own with `chorus2` (folder `.chorus2`, branches
//! `chorus2/…`), so it never mistakes one of Chorus 1's (`.chorus`, `chorus/…`) for its own, or the
//! other way round.

use crate::folders;

pub const DIR: &str = ".chorus2";
pub const BRANCH_PREFIX: &str = "chorus2/";
const MAX_SLUG_CHARS: usize = 40;

/// The arguments that ask git about a folder; [`parse_facts`] reads the answer.
pub const FACTS_ARGS: [&str; 5] = ["rev-parse", "--path-format=absolute", "--show-toplevel", "--git-common-dir", "--abbrev-ref"];
/// Follows [`FACTS_ARGS`].
pub const FACTS_LAST_ARG: &str = "HEAD";

/// What git says about a folder inside a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// The working tree the folder is in (the repository, or one of its worktrees).
    pub top_level: String,
    /// The checked-out branch; none on a detached HEAD.
    pub branch: Option<String>,
    /// The repository's main working tree, where its `.git` folder is.
    pub main_root: String,
}

/// Reads `git rev-parse --path-format=absolute --show-toplevel --git-common-dir --abbrev-ref HEAD`.
/// Paths come back with backslashes.
pub fn parse_facts(out: &str) -> Option<Facts> {
    let mut lines = out.lines().map(str::trim).filter(|l| !l.is_empty());
    let top_level = lines.next()?.replace('/', "\\");
    let common = lines.next()?.replace('/', "\\");
    let branch = lines.next()?;
    let main_root = match common.trim_end_matches('\\').strip_suffix("\\.git") {
        Some(root) => root.to_owned(),
        // A repository kept elsewhere (`--separate-git-dir`): the working tree is the best anchor.
        None => top_level.clone(),
    };
    Some(Facts { top_level, branch: (branch != "HEAD").then(|| branch.to_owned()), main_root })
}

/// A repository's name made safe for a branch and a folder: lowercase letters, digits and `_`,
/// with anything else turned into `-`.
pub fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.trim().to_lowercase().chars() {
        let c = if c.is_ascii_alphanumeric() || c == '_' { c } else { '-' };
        if !(c == '-' && s.ends_with('-')) {
            s.push(c);
        }
    }
    let s: String = s.trim_matches('-').chars().take(MAX_SLUG_CHARS).collect();
    let s = s.trim_end_matches('-');
    if s.is_empty() { "repo".to_owned() } else { s.to_owned() }
}

/// Eight hex digits naming one worktree.
pub fn short_id(bytes: [u8; 4]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Where a new worktree goes and its branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub path: String,
    pub branch: String,
}

/// A new worktree of the repository whose main working tree is `main_root`: beside it, in
/// `<parent>\.chorus2\<repo>\<id>`, on branch `chorus2/<repo>/<id>`. Beside rather than inside, so
/// the repository never sees it as untracked files.
pub fn plan(main_root: &str, id: &str) -> Result<Plan, String> {
    let root = main_root.trim().replace('/', "\\");
    let root = root.trim_end_matches('\\');
    let (parent, name) = root
        .rsplit_once('\\')
        .filter(|(parent, name)| !parent.is_empty() && !name.is_empty())
        .ok_or_else(|| format!("{root} is a drive's root, so there's no folder beside it for a worktree."))?;
    let parent = if parent.ends_with(':') { format!("{parent}\\") } else { parent.to_owned() };
    let slug = slug(name);
    Ok(Plan {
        path: format!("{}\\{DIR}\\{slug}\\{id}", parent.trim_end_matches('\\')),
        branch: format!("{BRANCH_PREFIX}{slug}/{id}"),
    })
}

/// Where a session that asked for `cwd` works inside a new worktree of the tree `top_level`: the
/// same subfolder there.
pub fn cwd_in(worktree: &str, top_level: &str, cwd: &str) -> String {
    match folders::relative(cwd, top_level) {
        Some(rel) if !rel.is_empty() => format!("{}\\{rel}", worktree.trim_end_matches('\\')),
        _ => worktree.to_owned(),
    }
}

/// Whether `git status --porcelain` found nothing: no changes and no untracked files. Anything
/// else is work that removing the worktree would lose.
pub fn is_clean(porcelain: &str) -> bool {
    porcelain.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facts_come_from_rev_parse() {
        let f = parse_facts("C:/work/ledger\nC:/work/ledger/.git\nmain\n").unwrap();
        assert_eq!((f.top_level.as_str(), f.branch.as_deref(), f.main_root.as_str()), (r"C:\work\ledger", Some("main"), r"C:\work\ledger"));
        let wt = parse_facts("C:/work/.chorus2/ledger/1a2b3c4d\nC:/work/ledger/.git\nchorus2/ledger/1a2b3c4d").unwrap();
        assert_eq!(wt.main_root, r"C:\work\ledger", "a worktree's repository is the main tree");
        assert_eq!(parse_facts("C:/w\nC:/w/.git\nHEAD").unwrap().branch, None, "detached");
        assert_eq!(parse_facts("C:/w\nD:/store/w.git\nmain").unwrap().main_root, r"C:\w");
        assert_eq!(parse_facts("C:/w\n"), None);
    }

    #[test]
    fn a_worktree_goes_beside_its_repository() {
        let p = plan(r"C:\work\My Ledger\", "1a2b3c4d").unwrap();
        assert_eq!(p.path, r"C:\work\.chorus2\my-ledger\1a2b3c4d");
        assert_eq!(p.branch, "chorus2/my-ledger/1a2b3c4d");
        assert_eq!(plan(r"D:\ledger", "00ff00ff").unwrap().path, r"D:\.chorus2\ledger\00ff00ff");
        assert!(plan(r"D:\", "00ff00ff").is_err());
        assert_eq!(slug("..Ünïcode repo!!"), "n-code-repo");
        assert_eq!(slug("..."), "repo");
        assert_eq!(short_id([0x1a, 0x2b, 0x3c, 0x4d]), "1a2b3c4d");
    }

    #[test]
    fn a_subfolder_maps_into_the_worktree() {
        let wt = r"C:\work\.chorus2\ledger\1a2b3c4d";
        assert_eq!(cwd_in(wt, r"C:\work\ledger", r"c:\work\ledger\web"), format!(r"{wt}\web"));
        assert_eq!(cwd_in(wt, r"C:\work\ledger", r"C:\work\ledger"), wt);
        assert!(is_clean("\n") && !is_clean("?? notes.txt\n"));
    }
}
