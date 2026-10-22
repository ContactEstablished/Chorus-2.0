//! NUL-delimited Git output. Paths may contain tabs and newlines; never split them into lines.
use serde::Serialize;
use specta::Type;

#[derive(Debug, PartialEq, Eq)]
pub struct Tree {
    pub path: String,
    pub branch: Option<String>,
}

pub fn worktrees(out: &str) -> Result<Vec<Tree>, String> {
    let mut trees = Vec::new();
    if !out.ends_with("\0\0") {
        return Err("Incomplete git worktree list.".into());
    }
    for record in out.split("\0\0").filter(|r| !r.is_empty()) {
        let mut fields = record.split('\0');
        let path = fields
            .next()
            .and_then(|s| s.strip_prefix("worktree "))
            .filter(|s| !s.is_empty())
            .ok_or("Unreadable git worktree list.")?;
        let branch = fields
            .find_map(|s| s.strip_prefix("branch refs/heads/"))
            .map(str::to_owned);
        trees.push(Tree {
            path: path.replace('/', "\\"),
            branch,
        });
    }
    Ok(trees)
}

#[derive(Debug, Default, Clone, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Changes {
    pub branch: Option<String>,
    pub upstream: Option<String>,
    /// None when there is no usable upstream comparison, rather than zero.
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub files: u32,
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    pub conflicts: u32,
}

/// `status --porcelain=v2 --branch --untracked-files=all -z` (Git 2.50).
pub fn changes(out: &str) -> Result<Changes, String> {
    if !out.is_empty() && !out.ends_with('\0') {
        return Err("Incomplete git status.".into());
    }
    let mut s = Changes::default();
    let mut fields = out.split_terminator('\0');
    let bad = || "Unreadable git status.".to_owned();
    for_record(&mut fields, &mut s, &bad)?;
    Ok(s)
}

fn for_record(
    fields: &mut std::str::SplitTerminator<'_, char>,
    s: &mut Changes,
    bad: &impl Fn() -> String,
) -> Result<(), String> {
    while let Some(record) = fields.next() {
        if let Some(b) = record.strip_prefix("# branch.head ") {
            s.branch = (b != "(detached)").then(|| b.to_owned());
        } else if let Some(u) = record.strip_prefix("# branch.upstream ") {
            s.upstream = Some(u.to_owned());
        } else if let Some(ab) = record.strip_prefix("# branch.ab ") {
            let (a, b) = ab.split_once(' ').ok_or_else(bad)?;
            s.ahead = Some(
                a.strip_prefix('+')
                    .ok_or_else(bad)?
                    .parse()
                    .map_err(|_| bad())?,
            );
            s.behind = Some(
                b.strip_prefix('-')
                    .ok_or_else(bad)?
                    .parse()
                    .map_err(|_| bad())?,
            );
        } else if record.starts_with("# ") || record.starts_with("! ") {
            // Other headers and ignored paths are not changes.
        } else if record.starts_with("? ") {
            s.files += 1;
            s.untracked += 1;
        } else {
            let count = match record.as_bytes().first() {
                Some(b'1') => 9,
                Some(b'2') => 10,
                Some(b'u') => 11,
                _ => return Err(bad()),
            };
            let parts: Vec<&str> = record.splitn(count, ' ').collect();
            if parts.len() != count || parts[1].len() != 2 || parts[count - 1].is_empty() {
                return Err(bad());
            }
            s.files += 1;
            s.staged += u32::from(parts[1].as_bytes()[0] != b'.');
            s.unstaged += u32::from(
                parts[1].as_bytes()[1] != b'.' || parts[2].bytes().skip(1).any(|b| b != b'.'),
            );
            if parts[0] == "u" {
                s.conflicts += 1;
            }
            if parts[0] == "2" && fields.next().filter(|p| !p.is_empty()).is_none() {
                return Err(bad());
            }
        }
    }
    Ok(())
}

#[derive(Debug, Default, Clone, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Diff {
    pub additions: u32,
    pub deletions: u32,
    pub binary: u32,
}

/// `diff --numstat -z`: renames have an empty path followed by old and new NUL-delimited paths.
pub fn numstat(out: &str) -> Result<Diff, String> {
    let mut d = Diff::default();
    let bad = || "Incomplete or unreadable git diff summary.".to_owned();
    if !out.is_empty() && !out.ends_with('\0') {
        return Err(bad());
    }
    let mut records = out.split_terminator('\0');
    while let Some(record) = records.next() {
        let mut fields = record.splitn(3, '\t');
        let a = fields.next().ok_or_else(bad)?;
        let b = fields.next().ok_or_else(bad)?;
        let path = fields.next().ok_or_else(bad)?;
        if a == "-" && b == "-" {
            d.binary += 1;
        } else {
            d.additions = d
                .additions
                .checked_add(a.parse().map_err(|_| bad())?)
                .ok_or_else(bad)?;
            d.deletions = d
                .deletions
                .checked_add(b.parse().map_err(|_| bad())?)
                .ok_or_else(bad)?;
        }
        if path.is_empty() {
            for _ in 0..2 {
                records.next().filter(|p| !p.is_empty()).ok_or_else(bad)?;
            }
        }
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trees_preserve_unusual_paths_and_detached_heads() {
        let t = worktrees("worktree C:/a\n b\0HEAD abc\0branch refs/heads/main\0\0worktree C:/other\0HEAD def\0detached\0locked reason\0\0").unwrap();
        assert_eq!(
            t,
            vec![
                Tree {
                    path: "C:\\a\n b".into(),
                    branch: Some("main".into())
                },
                Tree {
                    path: "C:\\other".into(),
                    branch: None
                }
            ]
        );
        assert!(worktrees("worktree C:/cut\0HEAD abc\0").is_err());
    }

    #[test]
    fn status_counts_paths_once_and_distinguishes_no_upstream() {
        let s = changes(
            "# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -3\0\
            1 MM N... 100644 100644 100644 abc def file\nname\0\
            2 R. N... 100644 100644 100644 abc def R100 new name\0old\nname\0\
            ? untracked\0u UU N... 100644 100644 100644 100644 a b c conflict\0",
        )
        .unwrap();
        assert_eq!(
            (s.files, s.staged, s.unstaged, s.untracked, s.conflicts),
            (4, 3, 2, 1, 1)
        );
        assert_eq!((s.ahead, s.behind), (Some(2), Some(3)));
        let detached = changes("# branch.head (detached)\0").unwrap();
        assert_eq!(
            (detached.branch, detached.ahead, detached.behind),
            (None, None, None)
        );
        assert!(changes("? truncated").is_err());
        assert!(changes("2 R. N... 100644 100644 100644 a b R100 new\0").is_err());
        let sub = changes("1 .M S.MU 160000 160000 160000 a b submodule\0").unwrap();
        assert_eq!(sub.unstaged, 1);
    }

    #[test]
    fn diff_handles_renames_binary_tabs_and_overflow() {
        let d = numstat(
            "3\t2\tfile\twith\nwhitespace\0-\t-\timage\0\
            0\t0\t\0old\0new\0",
        )
        .unwrap();
        assert_eq!(
            d,
            Diff {
                additions: 3,
                deletions: 2,
                binary: 1
            }
        );
        for bad in [
            "1\t2\tpath",
            "0\t0\t\0old\0",
            "-\t1\tfile\0",
            "4294967295\t0\ta\x001\t0\tb\0",
        ] {
            assert!(numstat(bad).is_err(), "{bad:?}");
        }
    }
}
