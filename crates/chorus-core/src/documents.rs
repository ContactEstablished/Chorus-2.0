//! Documents (P8): the plans, specs and reports sessions write. A file a session wrote counts when
//! it is Markdown, sits at the top of the session's folder or under `_docs/` or `docs/`, and is
//! named like one of the kinds below. Its title is its first `# heading`.

use serde::{Deserialize, Serialize};
use specta::Type;

pub const MAX_TITLE_CHARS: usize = 200;
/// How much of a file is read for its title.
pub const TITLE_SCAN_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum DocKind {
    Plan,
    Spec,
    Report,
    Review,
    Decision,
}

impl DocKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Spec => "spec",
            Self::Report => "report",
            Self::Review => "review",
            Self::Decision => "decision",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Plan, Self::Spec, Self::Report, Self::Review, Self::Decision].into_iter().find(|k| k.as_str() == s)
    }
}

/// Folders whose Markdown files can be documents, at the top of the session's folder.
const DOC_FOLDERS: &[&str] = &["_docs", "docs", "doc"];

/// The words in a file name that make it a document, checked in this order (a "plan review" is a
/// review). `adr` must start the name.
const KINDS: &[(&str, DocKind)] = &[
    ("review", DocKind::Review),
    ("decision", DocKind::Decision),
    ("findings", DocKind::Report),
    ("report", DocKind::Report),
    ("postmortem", DocKind::Report),
    ("spec", DocKind::Spec),
    ("design", DocKind::Spec),
    ("rfc", DocKind::Spec),
    ("proposal", DocKind::Spec),
    ("plan", DocKind::Plan),
    ("roadmap", DocKind::Plan),
    ("handoff", DocKind::Plan),
];

/// Whether a path, relative to the session's folder (`/` or `\`), is a document, and which kind.
pub fn classify(relative: &str) -> Option<DocKind> {
    let parts: Vec<&str> = relative.split(['/', '\\']).filter(|p| !p.is_empty() && *p != ".").collect();
    let (file, dirs) = parts.split_last()?;
    if parts.contains(&"..") {
        return None;
    }
    let lower = file.to_lowercase();
    let stem = lower.strip_suffix(".md").or_else(|| lower.strip_suffix(".markdown"))?;
    let placed = dirs.is_empty() || DOC_FOLDERS.contains(&dirs[0].to_lowercase().as_str());
    if !placed {
        return None;
    }
    if stem.starts_with("adr-") || stem.starts_with("adr_") || stem == "adr" {
        return Some(DocKind::Decision);
    }
    let words: Vec<&str> = stem.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).collect();
    KINDS.iter().find(|(word, _)| words.iter().any(|w| w == word || w.strip_suffix('s') == Some(word))).map(|(_, k)| *k)
}

/// The first `# heading` in the first lines of a Markdown file, if any.
pub fn title(markdown: &str) -> Option<String> {
    let mut fenced = false;
    for line in markdown.lines().take(80) {
        let t = line.trim_start();
        if t.starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        if let Some(rest) = t.strip_prefix("# ") {
            let heading = rest.trim().trim_end_matches('#').trim();
            if heading.is_empty() {
                continue;
            }
            return Some(heading.chars().take(MAX_TITLE_CHARS).collect());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_plans_specs_and_reports_are_documents() {
        assert_eq!(classify("_docs/PLAN-test.md"), Some(DocKind::Plan));
        assert_eq!(classify("_docs\\HANDOFF-P8-REVIEW.md"), Some(DocKind::Review));
        assert_eq!(classify("docs/adr/ADR-001-storage.md"), Some(DocKind::Decision));
        assert_eq!(classify("docs/api-spec.md"), Some(DocKind::Spec));
        assert_eq!(classify("findings.md"), Some(DocKind::Report));
        assert_eq!(classify("DECISIONS.md"), Some(DocKind::Decision));
        assert_eq!(classify("./plans.md"), Some(DocKind::Plan));
        assert_eq!(classify("_docs/design/DESIGN.md"), Some(DocKind::Spec));
    }

    #[test]
    fn other_files_are_not() {
        assert_eq!(classify("README.md"), None);
        assert_eq!(classify("src/plan.md"), None);
        assert_eq!(classify("_docs/PLAN.txt"), None);
        assert_eq!(classify("_docs/notes.md"), None);
        assert_eq!(classify("../_docs/PLAN.md"), None);
        assert_eq!(classify("planet.md"), None);
        assert_eq!(classify(""), None);
    }

    #[test]
    fn the_title_is_the_first_heading_outside_code() {
        assert_eq!(title("Intro\n\n# Plan: storage #\n## Later"), Some("Plan: storage".into()));
        assert_eq!(title("```\n# not this\n```\n# This"), Some("This".into()));
        assert_eq!(title("## Only a second level"), None);
        assert_eq!(title(&format!("# {}", "t".repeat(500))).unwrap().chars().count(), MAX_TITLE_CHARS);
    }
}
