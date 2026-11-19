//! The hand-off note (P18, D128): `.chorus/handoff.md` in a project folder, kept by Chorus and
//! OpenMausBot. Front matter holds the baton; entries, newest first, say what was done, where
//! things stand and what's next.
//!
//! Chorus edits only the front matter and its own entries (K1). Every other entry, and the text
//! between the front matter and the first entry, is kept byte for byte. The front matter itself is
//! rewritten in one form: known keys in order, then unknown keys as they were.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::progress::PlanStep;

pub const NOTE_FILE: &str = ".chorus/handoff.md";
pub const ARCHIVE_FILE: &str = ".chorus/handoff-archive.md";
/// What `.git/info/exclude` must list: the two files, never `/.chorus/`, because P15 commits
/// `.chorus/run/` on its run branches (K5).
pub const EXCLUDE_LINES: [&str; 2] = ["/.chorus/handoff.md", "/.chorus/handoff-archive.md"];
pub const MAX_ENTRIES: usize = 20;
pub const MAX_NOTE_BYTES: usize = 64 * 1024;
/// A note bigger than this isn't read at all.
pub const MAX_READ_BYTES: usize = 1024 * 1024;
pub const LATEST_MARK: &str = "· Chorus · latest work";
const ARCHIVE_HEADING: &str = "# Hand-off archive";
const PREAMBLE: &str = "\n# Hand-off note\n\nKept by Chorus and MausBot. Newest entry first. Each entry has Done, State, Open and Next lines.\nAdd yours at the top; leave the others as they are.\n\n";
/// The longest value Chorus writes on one labelled line.
const LINE_CHARS: usize = 200;
/// Steps listed on one line before "and N more".
const LISTED_STEPS: usize = 6;

/// Who is working on the project now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Baton {
    Chorus,
    Mausbot,
    Nobody,
}

impl Baton {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chorus => "chorus",
            Self::Mausbot => "mausbot",
            Self::Nobody => "nobody",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "chorus" => Some(Self::Chorus),
            "mausbot" => Some(Self::Mausbot),
            "nobody" => Some(Self::Nobody),
            _ => None,
        }
    }
}

/// Who wrote an entry, from its heading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Author {
    Chorus,
    Mausbot,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    pub project: String,
    pub baton: Baton,
    pub branch: Option<String>,
    pub worktree: Option<String>,
    pub base: Option<String>,
    pub updated: Option<String>,
    /// Keys Chorus doesn't know, kept in their order.
    pub extra: Vec<(String, String)>,
}

/// One entry, as written: from its `## ` heading to just before the next one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub raw: String,
}

/// An entry's labelled lines, for display. Other non-empty lines are kept in `other`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fields {
    pub done: Option<String>,
    pub state: Option<String>,
    pub open: Option<String>,
    pub next: Option<String>,
    pub other: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub meta: Meta,
    /// Everything between the closing `---` and the first entry, as written.
    pub preamble: String,
    pub entries: Vec<Entry>,
}

#[derive(Clone, Copy)]
enum Label {
    Done,
    State,
    Open,
    Next,
}

fn label_of(line: &str) -> Option<(Label, &str)> {
    let (left, right) = line.split_once(':')?;
    let label = match left.trim().to_ascii_lowercase().as_str() {
        "done" => Label::Done,
        "state" => Label::State,
        "open" => Label::Open,
        "next" => Label::Next,
        _ => return None,
    };
    Some((label, right.trim()))
}

impl Entry {
    /// The heading line without its `## `.
    pub fn heading(&self) -> &str {
        let first = self.raw.lines().next().unwrap_or("");
        first.strip_prefix("## ").unwrap_or(first).trim()
    }

    pub fn is_latest_work(&self) -> bool {
        self.heading().ends_with(LATEST_MARK)
    }

    pub fn author(&self) -> Author {
        let h = self.heading();
        if h.contains("(from git)") {
            return Author::Chorus;
        }
        let before = h.split('→').next().unwrap_or(h);
        if before.contains("· Chorus") {
            Author::Chorus
        } else if before.contains("MausBot") {
            Author::Mausbot
        } else {
            Author::Other
        }
    }

    pub fn fields(&self) -> Fields {
        let mut f = Fields::default();
        let mut current: Option<Label> = None;
        for line in self.raw.lines().skip(1) {
            let t = line.trim();
            if t.is_empty() {
                current = None;
                continue;
            }
            // Only an indented line continues a labelled value; any other line stands alone.
            if !line.starts_with([' ', '\t']) {
                current = None;
            }
            if let Some((label, value)) = label_of(t) {
                let slot = match label {
                    Label::Done => &mut f.done,
                    Label::State => &mut f.state,
                    Label::Open => &mut f.open,
                    Label::Next => &mut f.next,
                };
                *slot = Some(value.to_owned());
                current = Some(label);
            } else if let Some(label) = current {
                let slot = match label {
                    Label::Done => &mut f.done,
                    Label::State => &mut f.state,
                    Label::Open => &mut f.open,
                    Label::Next => &mut f.next,
                };
                if let Some(v) = slot {
                    v.push(' ');
                    v.push_str(t);
                }
            } else {
                f.other.push(t.to_owned());
            }
        }
        f
    }
}

fn line_body(line: &str) -> &str {
    line.trim_end_matches(['\n', '\r'])
}

/// A front matter value with its trailing ` # comment` cut off.
fn strip_comment(v: &str) -> &str {
    match v.find(" #") {
        Some(i) => &v[..i],
        None => v,
    }
}

fn optional(v: &str) -> Option<String> {
    (!v.is_empty()).then(|| v.to_owned())
}

/// Reads a note. Each way it can be malformed has its own sentence.
pub fn parse(text: &str) -> Result<Note, String> {
    if text.len() > MAX_READ_BYTES {
        return Err(format!("The note is larger than {} KiB.", MAX_READ_BYTES / 1024));
    }
    let mut lines = text.split_inclusive('\n');
    let first = lines.next().ok_or("The note is empty.")?;
    if line_body(first) != "---" {
        return Err("The note doesn't start with a front matter line (---).".into());
    }
    let mut meta = Meta {
        project: String::new(),
        baton: Baton::Nobody,
        branch: None,
        worktree: None,
        base: None,
        updated: None,
        extra: Vec::new(),
    };
    let mut seen: Vec<String> = Vec::new();
    let mut consumed = first.len();
    let mut closed = false;
    for line in lines {
        consumed += line.len();
        let body = line_body(line);
        if body == "---" {
            closed = true;
            break;
        }
        if body.trim().is_empty() {
            continue;
        }
        // A heading here means the front matter was never closed.
        if body.starts_with('#') {
            break;
        }
        let (k, v) = body
            .split_once(':')
            .ok_or_else(|| format!("The front matter line \"{}\" has no colon.", one_line(body, 60)))?;
        let key = k.trim();
        if key.is_empty() {
            return Err(format!("The front matter line \"{}\" has no key.", one_line(body, 60)));
        }
        let value = strip_comment(v).trim();
        if seen.iter().any(|s| s == key) {
            return Err(format!("The front matter gives {key} twice."));
        }
        seen.push(key.to_owned());
        match key {
            "project" => meta.project = value.to_owned(),
            "baton" => {
                meta.baton = if value.is_empty() {
                    Baton::Nobody
                } else {
                    Baton::parse(value).ok_or_else(|| {
                        format!("The baton must be chorus, mausbot or nobody, not \"{}\".", one_line(value, 40))
                    })?
                }
            }
            "branch" => meta.branch = optional(value),
            "worktree" => meta.worktree = optional(value),
            "base" => meta.base = optional(value),
            "updated" => meta.updated = optional(value),
            _ => meta.extra.push((key.to_owned(), value.to_owned())),
        }
    }
    if !closed {
        return Err("The front matter has no closing line (---).".into());
    }
    let body = &text[consumed..];
    let mut starts: Vec<usize> = Vec::new();
    let mut at = 0;
    for line in body.split_inclusive('\n') {
        if line.starts_with("## ") {
            starts.push(at);
        }
        at += line.len();
    }
    let preamble = body[..starts.first().copied().unwrap_or(body.len())].to_owned();
    let entries = starts
        .iter()
        .enumerate()
        .map(|(i, &s)| Entry { raw: body[s..starts.get(i + 1).copied().unwrap_or(body.len())].to_owned() })
        .collect();
    Ok(Note { meta, preamble, entries })
}

/// The note as text: the front matter in Chorus's form, then the preamble and the entries as
/// written. Ends with exactly one newline.
pub fn render(note: &Note) -> String {
    let m = &note.meta;
    let mut s = String::from("---\n");
    let mut key = |k: &str, v: Option<&str>| match v.filter(|v| !v.is_empty()) {
        Some(v) => s.push_str(&format!("{k}: {v}\n")),
        None => s.push_str(&format!("{k}:\n")),
    };
    key("project", Some(&m.project));
    key("baton", Some(m.baton.as_str()));
    key("branch", m.branch.as_deref());
    key("worktree", m.worktree.as_deref());
    key("base", m.base.as_deref());
    key("updated", m.updated.as_deref());
    for (k, v) in &m.extra {
        key(k, Some(v));
    }
    s.push_str("---\n");
    s.push_str(&note.preamble);
    for e in &note.entries {
        s.push_str(&e.raw);
    }
    let trimmed = s.trim_end_matches(['\n', '\r']).len();
    s.truncate(trimmed);
    s.push('\n');
    s
}

/// A new note for a project, with no entries.
pub fn new_note(project: &str) -> Note {
    Note {
        meta: Meta {
            project: project.to_owned(),
            baton: Baton::Chorus,
            branch: None,
            worktree: None,
            base: None,
            updated: None,
            extra: Vec::new(),
        },
        preamble: PREAMBLE.to_owned(),
        entries: Vec::new(),
    }
}

/// An entry's text always ends with a blank line, so the next heading starts cleanly.
fn spaced(mut raw: String) -> String {
    let trimmed = raw.trim_end_matches(['\n', '\r']).len();
    raw.truncate(trimmed);
    raw.push_str("\n\n");
    raw
}

/// Replaces the newest entry if it is Chorus's latest work; otherwise puts the new one on top (K2).
pub fn with_latest_work(mut note: Note, entry: String) -> Note {
    let entry = Entry { raw: spaced(entry) };
    if note.entries.first().is_some_and(Entry::is_latest_work) {
        note.entries[0] = entry;
    } else {
        note.entries.insert(0, entry);
    }
    note
}

/// Puts entries on top, in the order given (the first given ends up first).
pub fn prepend(mut note: Note, entries: Vec<String>) -> Note {
    let mut new: Vec<Entry> = entries.into_iter().map(|raw| Entry { raw: spaced(raw) }).collect();
    new.append(&mut note.entries);
    note.entries = new;
    note
}

/// Keeps at most `MAX_ENTRIES` entries and `MAX_NOTE_BYTES`, moving the oldest out. The newest is
/// never moved. Returns the moved entries, newest first.
pub fn bound(mut note: Note) -> (Note, Vec<Entry>) {
    let mut moved = Vec::new();
    while note.entries.len() > 1 && (note.entries.len() > MAX_ENTRIES || render(&note).len() > MAX_NOTE_BYTES) {
        if let Some(e) = note.entries.pop() {
            moved.push(e);
        }
    }
    moved.reverse();
    (note, moved)
}

/// The archive's text with `moved` (newest first) above its existing entries. An archive Chorus
/// didn't write is kept, below, unchanged.
pub fn archive(existing: Option<&str>, moved: &[Entry]) -> String {
    let header = format!("{ARCHIVE_HEADING}\n\nOlder hand-off entries, newest first. Chorus moves them here from handoff.md.\n\n");
    let mut s = String::new();
    let rest = match existing {
        Some(e) if e.starts_with(ARCHIVE_HEADING) => {
            let first_entry = e.find("\n## ").map_or(e.len(), |i| i + 1);
            s.push_str(&e[..first_entry]);
            if !s.ends_with("\n\n") {
                s.push('\n');
            }
            &e[first_entry..]
        }
        Some(e) => {
            s.push_str(&header);
            e
        }
        None => {
            s.push_str(&header);
            ""
        }
    };
    for m in moved {
        s.push_str(&spaced(m.raw.clone()));
    }
    s.push_str(rest);
    let trimmed = s.trim_end_matches(['\n', '\r']).len();
    s.truncate(trimmed);
    s.push('\n');
    s
}

/// `.git/info/exclude` with the note's two lines added, or `None` when both are already there.
/// The file's own lines are kept.
pub fn with_exclude(existing: &str) -> Option<String> {
    let has = |want: &str| existing.lines().any(|l| l.trim() == want);
    let missing: Vec<&str> = EXCLUDE_LINES.iter().copied().filter(|l| !has(l)).collect();
    if missing.is_empty() {
        return None;
    }
    let mut s = existing.to_owned();
    if !s.is_empty() && !s.ends_with('\n') {
        s.push('\n');
    }
    s.push_str("# Chorus's hand-off note (P18)\n");
    for l in missing {
        s.push_str(l);
        s.push('\n');
    }
    Some(s)
}

/// One line of text: whitespace runs become one space, control characters go, and a value longer
/// than `max_chars` is cut with "…". A newline can never start a fake heading.
pub fn one_line(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            space = true;
            continue;
        }
        if c.is_control() {
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.push(c);
    }
    if out.chars().count() > max_chars {
        let mut cut: String = out.chars().take(max_chars.saturating_sub(1)).collect();
        cut.truncate(cut.trim_end().len());
        cut.push('…');
        return cut;
    }
    out
}

/// Local calendar fields of a UTC instant shifted by `offset_minutes` (Howard Hinnant's
/// civil_from_days).
fn civil(at_ms: i64, offset_minutes: i32) -> (i64, i64, i64, i64, i64) {
    let secs = at_ms.div_euclid(1000) + i64::from(offset_minutes) * 60;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, rem / 3600, rem % 3600 / 60)
}

/// `2026-11-18 14:05`, local.
pub fn stamp(at_ms: i64, offset_minutes: i32) -> String {
    let (y, m, d, hh, mm) = civil(at_ms, offset_minutes);
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}")
}

/// `2026-11-18`, local.
pub fn day(at_ms: i64, offset_minutes: i32) -> String {
    let (y, m, d, _, _) = civil(at_ms, offset_minutes);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `14:05`, local.
pub fn clock(at_ms: i64, offset_minutes: i32) -> String {
    let (_, _, _, hh, mm) = civil(at_ms, offset_minutes);
    format!("{hh:02}:{mm:02}")
}

/// Where the session's checkout stands, for the State line. `None` in a folder that isn't a
/// repository.
pub struct GitLine<'a> {
    pub branch: Option<&'a str>,
    /// The short commit, or `None` before the first commit.
    pub head: Option<&'a str>,
    pub dirty_files: u32,
}

/// What Chorus knows about a session's latest work.
pub struct WorkFacts<'a> {
    pub at_ms: i64,
    pub offset_minutes: i32,
    pub session: &'a str,
    pub agent: &'a str,
    pub plan: &'a [PlanStep],
    pub last_prompt: Option<&'a str>,
    /// The final message and when it came.
    pub last_reply: Option<(&'a str, i64)>,
    pub git: Option<GitLine<'a>>,
}

fn listed(texts: &[&str]) -> String {
    let shown: Vec<String> = texts.iter().take(LISTED_STEPS).map(|t| one_line(t, 120)).collect();
    let mut s = shown.join("; ");
    if texts.len() > LISTED_STEPS {
        s.push_str(&format!(" and {} more", texts.len() - LISTED_STEPS));
    }
    s
}

/// The State line.
pub fn state_line(git: Option<&GitLine>) -> String {
    let Some(g) = git else { return "not a git repository".into() };
    let head = g.head.unwrap_or("no commits");
    let place = match g.branch {
        Some(b) => format!("branch {b} @ {head}"),
        None => format!("detached @ {head}"),
    };
    if g.dirty_files == 0 {
        format!("{place}, clean")
    } else {
        format!("{place}, {} uncommitted {}", g.dirty_files, if g.dirty_files == 1 { "file" } else { "files" })
    }
}

/// Chorus's "latest work" entry, from data it already has. Deterministic: no model.
pub fn latest_work_entry(f: &WorkFacts) -> String {
    let done: Vec<&str> = f.plan.iter().filter(|s| s.status == "completed").map(|s| s.text.as_str()).collect();
    let current: Vec<&str> = f.plan.iter().filter(|s| s.status == "in-progress").map(|s| s.text.as_str()).collect();
    let pending: Vec<&str> = f
        .plan
        .iter()
        .filter(|s| s.status != "completed" && s.status != "in-progress")
        .map(|s| s.text.as_str())
        .collect();
    let n = f.plan.len();
    let (done_line, open_line, next_line) = if n == 0 {
        let done = match f.last_prompt.map(str::trim).filter(|p| !p.is_empty()) {
            Some(p) => format!("Last turn: {}", one_line(p, 160)),
            None => "Nothing recorded yet".into(),
        };
        (done, "No plan in this session".to_owned(), "See the last reply".to_owned())
    } else {
        let done_line = if done.is_empty() {
            format!("None of the {n} plan steps yet")
        } else {
            format!("{} ({} of {n} plan steps)", listed(&done), done.len())
        };
        let open: Vec<&str> = current.iter().chain(pending.iter()).copied().collect();
        let open_line = if open.is_empty() { "Nothing open in the plan".to_owned() } else { listed(&open) };
        let next_line = match current.first().or(pending.first()) {
            Some(t) => one_line(t, 160),
            None => format!("All {n} plan steps are done"),
        };
        (done_line, open_line, next_line)
    };
    let mut s = format!("## {} {LATEST_MARK}\n", stamp(f.at_ms, f.offset_minutes));
    s.push_str(&format!("Done: {}\n", one_line(&done_line, LINE_CHARS)));
    s.push_str(&format!("State: {}\n", one_line(&state_line(f.git.as_ref()), LINE_CHARS)));
    s.push_str(&format!("Open: {}\n", one_line(&open_line, LINE_CHARS)));
    s.push_str(&format!("Next: {}\n", one_line(&next_line, LINE_CHARS)));
    s.push_str(&format!("Session: {}\n", one_line(&format!("{} ({})", f.session, f.agent), LINE_CHARS)));
    if let Some((reply, at)) = f.last_reply.filter(|(r, _)| !r.trim().is_empty()) {
        s.push_str(&format!("Last reply ({}): {}\n", clock(at, f.offset_minutes), one_line(reply, 240)));
    }
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTE: &str = "---\nproject: Atlas\nbaton: mausbot\nbranch: maus/2026-11-18-fix\nworktree: D:\\w\\.chorus2\\atlas\\maus-2026-11-18-fix\nbase: main @ 1a2b3c4\nupdated: 2026-11-18T14:05:00Z\nowner: someone\n---\n\n# Hand-off note\n\nIntro.\n\n## 2026-11-18 16:40 · MausBot → Chorus\nDone: Fixed the checkout\n  and its test\nState: maus/2026-11-18-fix, 2 commits\nOpen: Nothing\nNext: Merge it\nA note from the bot.\n\n## 2026-11-18 14:05 · Chorus → MausBot\nDone: A\nState: B\nOpen: C\nNext: D\n";

    /// 2026-11-18T00:00:00Z.
    const NOV_18: i64 = 1_794_960_000_000;

    fn step(text: &str, status: &str) -> PlanStep {
        PlanStep { id: String::new(), text: text.into(), status: status.into() }
    }

    fn facts<'a>(plan: &'a [PlanStep], git: Option<GitLine<'a>>) -> WorkFacts<'a> {
        WorkFacts {
            at_ms: NOV_18,
            offset_minutes: 0,
            session: "Vince",
            agent: "Claude Code",
            plan,
            last_prompt: Some("Build the store"),
            last_reply: Some(("Done.\n## not a heading\nNext I'll test it.", NOV_18 - 60_000)),
            git,
        }
    }

    #[test]
    fn a_note_in_chorus_form_renders_exactly_as_read() {
        let note = parse(NOTE).unwrap();
        assert_eq!(render(&note), NOTE);
        assert_eq!(note.meta.baton, Baton::Mausbot);
        assert_eq!(note.meta.extra, [("owner".to_owned(), "someone".to_owned())]);
        assert_eq!(note.entries.len(), 2);
        assert_eq!(note.preamble, "\n# Hand-off note\n\nIntro.\n\n");
    }

    #[test]
    fn comments_and_spacing_in_the_front_matter_are_normalised_and_the_body_kept() {
        let text = "---\nproject: Atlas\n\nbaton: mausbot   # chorus | mausbot | nobody\nbranch:\n---\nbody text\n## x · MausBot → Chorus\nDone: y\r\n";
        let note = parse(text).unwrap();
        assert_eq!(note.meta.baton, Baton::Mausbot);
        assert_eq!(note.meta.branch, None);
        let out = render(&note);
        assert!(out.starts_with("---\nproject: Atlas\nbaton: mausbot\nbranch:\nworktree:\nbase:\nupdated:\n---\n"));
        assert!(out.ends_with("body text\n## x · MausBot → Chorus\nDone: y\n"), "{out:?}");
        assert_eq!(parse(&out).unwrap().entries[0].fields().done.as_deref(), Some("y"));
    }

    #[test]
    fn each_malformed_note_says_why() {
        let cases = [
            ("", "empty"),
            ("project: x\n", "doesn't start"),
            ("---\nproject: x\n", "no closing"),
            ("---\nproject: x\n\n## an entry\nDone: y\n", "no closing"),
            ("---\nproject x\n---\n", "no colon"),
            ("---\n: x\n---\n", "no key"),
            ("---\nproject: a\nproject: b\n---\n", "twice"),
            ("---\nbaton: maybe\n---\n", "chorus, mausbot or nobody"),
        ];
        for (text, why) in cases {
            let e = parse(text).unwrap_err();
            assert!(e.contains(why), "{text:?} → {e}");
        }
        assert!(parse(&"x".repeat(MAX_READ_BYTES + 1)).unwrap_err().contains("larger"));
    }

    #[test]
    fn a_missing_or_empty_baton_is_nobody() {
        assert_eq!(parse("---\nproject: a\n---\n").unwrap().meta.baton, Baton::Nobody);
        assert_eq!(parse("---\nbaton:\n---\n").unwrap().meta.baton, Baton::Nobody);
    }

    #[test]
    fn entries_say_who_wrote_them_and_what_they_say() {
        let note = parse(NOTE).unwrap();
        let bot = &note.entries[0];
        assert_eq!(bot.author(), Author::Mausbot);
        let f = bot.fields();
        assert_eq!(f.done.as_deref(), Some("Fixed the checkout and its test"));
        assert_eq!(f.next.as_deref(), Some("Merge it"));
        assert_eq!(f.other, ["A note from the bot."]);
        assert_eq!(note.entries[1].author(), Author::Chorus);
        let git = Entry { raw: "## 2026-11-18 16:40 · MausBot → Chorus (from git)\n".into() };
        assert_eq!(git.author(), Author::Chorus);
        let other = Entry { raw: "## Some notes\n".into() };
        assert_eq!(other.author(), Author::Other);
        assert!(Entry { raw: format!("## 2026-11-18 14:05 {LATEST_MARK}\n") }.is_latest_work());
    }

    #[test]
    fn latest_work_is_replaced_until_something_else_is_written() {
        let plan = [step("A", "completed")];
        let first = latest_work_entry(&facts(&plan, None));
        let note = with_latest_work(new_note("Atlas"), first.clone());
        let note = with_latest_work(note, first.clone());
        assert_eq!(note.entries.len(), 1);
        let note = prepend(note, vec!["## 2026-11-18 15:00 · Chorus → MausBot\nDone: x".into()]);
        let note = with_latest_work(note, first);
        assert_eq!(note.entries.len(), 3);
        assert!(note.entries[0].is_latest_work());
        assert!(note.entries[1].heading().ends_with("Chorus → MausBot"));
        assert!(note.entries[1].raw.ends_with("\n\n"), "a prepended entry is spaced");
        let text = render(&note);
        assert_eq!(render(&parse(&text).unwrap()), text, "rendering what was read changes nothing");
    }

    #[test]
    fn bounding_moves_the_oldest_and_never_the_newest() {
        let mut note = new_note("Atlas");
        for i in 0..25 {
            note = prepend(note, vec![format!("## e{i}\nDone: {i}")]);
        }
        let (kept, moved) = bound(note);
        assert_eq!(kept.entries.len(), MAX_ENTRIES);
        assert_eq!(kept.entries[0].heading(), "e24");
        assert_eq!(moved.iter().map(Entry::heading).collect::<Vec<_>>(), ["e4", "e3", "e2", "e1", "e0"]);

        let big = "x".repeat(10 * 1024);
        let mut note = new_note("Atlas");
        for i in 0..7 {
            note = prepend(note, vec![format!("## b{i}\nDone: {big}")]);
        }
        let (kept, moved) = bound(note);
        assert!(render(&kept).len() <= MAX_NOTE_BYTES);
        assert_eq!(kept.entries[0].heading(), "b6");
        assert!(!moved.is_empty());

        let huge = new_note("Atlas");
        let huge = prepend(huge, vec![format!("## only\nDone: {}", "y".repeat(MAX_NOTE_BYTES))]);
        let (kept, moved) = bound(huge);
        assert_eq!((kept.entries.len(), moved.len()), (1, 0), "the newest stays even when too big");
    }

    #[test]
    fn the_archive_takes_moved_entries_on_top() {
        let moved = [Entry { raw: "## new\nDone: 2\n\n".into() }, Entry { raw: "## older\nDone: 1".into() }];
        let fresh = archive(None, &moved);
        assert!(fresh.starts_with("# Hand-off archive\n\n"));
        assert!(fresh.find("## new").unwrap() < fresh.find("## older").unwrap());
        let again = archive(Some(&fresh), &[Entry { raw: "## newest\n".into() }]);
        assert!(again.find("## newest").unwrap() < again.find("## new\n").unwrap());
        assert_eq!(again.matches("# Hand-off archive").count(), 1);
        let foreign = archive(Some("my own notes\n"), &moved);
        assert!(foreign.starts_with("# Hand-off archive") && foreign.trim_end().ends_with("my own notes"));
    }

    #[test]
    fn the_exclude_lines_are_added_once_and_the_file_kept() {
        let empty = with_exclude("").unwrap();
        assert!(empty.contains("/.chorus/handoff.md\n") && empty.contains("/.chorus/handoff-archive.md\n"));
        assert_eq!(with_exclude(&empty), None);
        let one = with_exclude("*.log\n/.chorus/handoff.md\n").unwrap();
        assert!(one.starts_with("*.log\n/.chorus/handoff.md\n"));
        assert_eq!(one.matches("/.chorus/handoff.md\n").count(), 1);
        assert!(one.ends_with("/.chorus/handoff-archive.md\n"));
        assert!(with_exclude("# git's own\n*.tmp").unwrap().starts_with("# git's own\n*.tmp\n"));
        assert!(!empty.contains("/.chorus/\n"), "never the whole folder (K5)");
    }

    #[test]
    fn the_latest_work_entry_reads_the_plan_and_git() {
        let plan = [step("Read", "completed"), step("Build", "in-progress"), step("Check", "pending")];
        let e = latest_work_entry(&facts(&plan, Some(GitLine { branch: Some("main"), head: Some("1a2b3c4"), dirty_files: 3 })));
        assert!(e.starts_with("## 2026-11-18 00:00 · Chorus · latest work\n"), "{e}");
        assert!(e.contains("Done: Read (1 of 3 plan steps)\n"));
        assert!(e.contains("State: branch main @ 1a2b3c4, 3 uncommitted files\n"));
        assert!(e.contains("Open: Build; Check\n"));
        assert!(e.contains("Next: Build\n"));
        assert!(e.contains("Session: Vince (Claude Code)\n"));
        assert!(e.contains("Last reply (23:59): Done. ## not a heading Next I'll test it.\n"));
        assert_eq!(e.lines().filter(|l| l.starts_with("## ")).count(), 1, "a reply never starts a heading");
        assert!(e.ends_with("\n\n"));
    }

    #[test]
    fn the_latest_work_entry_without_a_plan_or_a_repository() {
        let e = latest_work_entry(&facts(&[], None));
        assert!(e.contains("Done: Last turn: Build the store\n"));
        assert!(e.contains("State: not a git repository\n"));
        assert!(e.contains("Open: No plan in this session\n"));
        assert!(e.contains("Next: See the last reply\n"));
        let done = [step("A", "completed"), step("B", "completed")];
        let e = latest_work_entry(&facts(&done, Some(GitLine { branch: None, head: Some("abc1234"), dirty_files: 0 })));
        assert!(e.contains("State: detached @ abc1234, clean\n"));
        assert!(e.contains("Open: Nothing open in the plan\n"));
        assert!(e.contains("Next: All 2 plan steps are done\n"));
        let none_done = [step("A", "pending")];
        assert!(latest_work_entry(&facts(&none_done, None)).contains("Done: None of the 1 plan steps yet\n"));
        let many: Vec<PlanStep> = (0..9).map(|i| step(&format!("s{i}"), "completed")).collect();
        assert!(latest_work_entry(&facts(&many, None)).contains("Done: s0; s1; s2; s3; s4; s5 and 3 more (9 of 9 plan steps)\n"));
        let new_repo = state_line(Some(&GitLine { branch: Some("main"), head: None, dirty_files: 1 }));
        assert_eq!(new_repo, "branch main @ no commits, 1 uncommitted file");
    }

    #[test]
    fn stamps_are_local_and_cross_midnight() {
        let t = NOV_18;
        assert_eq!(stamp(t, 0), "2026-11-18 00:00");
        assert_eq!(stamp(t, 60), "2026-11-18 01:00");
        assert_eq!(stamp(t, -300), "2026-11-17 19:00");
        assert_eq!(day(t, -300), "2026-11-17");
        assert_eq!(clock(t + 65 * 60_000, 0), "01:05");
        let leap = 1_709_164_800_000; // 2024-02-29T00:00:00Z
        assert_eq!(stamp(leap, 0), "2024-02-29 00:00");
    }

    #[test]
    fn one_line_collapses_and_cuts_on_characters() {
        assert_eq!(one_line("  a\n\n b\tc \u{7}d ", 50), "a b c d");
        assert_eq!(one_line("ééééé", 3), "éé…");
        assert_eq!(one_line("abc", 3), "abc");
        assert_eq!(one_line("ab cd", 4), "ab…");
    }
}
