//! A run's instructions (P8, D17, D31): the session's copy of its Bot's instructions, the notes
//! that apply to it, and an index of the Bot's skill folders, composed into one text and capped.
//!
//! The text is composed here from inputs; reading notes and `SKILL.md` files, scrubbing and
//! writing the per-run file are the back end's. How the text reaches each agent is
//! [`carrier`]: an agent without a verified mechanism gets none, and the session says why (D19).
//! Chorus never writes a repository's or the user's own agent configuration to deliver it.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::agent::AgentKind;

/// The whole text, Bot, notes and skill index included.
pub const MAX_INSTRUCTION_BYTES: usize = 24 * 1024;
pub const MAX_NOTE_TITLE_CHARS: usize = 120;
pub const MAX_NOTE_BODY_BYTES: usize = 16 * 1024;
pub const MAX_BOT_INSTRUCTION_BYTES: usize = 16 * 1024;
pub const MAX_SKILLS: usize = 16;

/// How the instruction text reaches an agent, as verified against its CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Carrier {
    /// `flag <file>`: the text in a per-run file, never in argv (Claude Code's
    /// `--append-system-prompt-file`).
    AppendFile(&'static str),
    /// Codex's private app-server (0.163.0): a developer message injected into the run's thread
    /// (`thread/inject_items`) by Chorus's bridge, so the text stays out of argv.
    CodexDeveloperInstructions,
    /// Nothing verified: the reason is shown on the session and in Settings.
    Unsupported(&'static str),
}

/// Each agent's mechanism. A new agent kind falls to the last arm, unsupported, until one is
/// verified.
pub fn carrier(agent: AgentKind) -> Carrier {
    match agent {
        // Claude Code 2.1.308 `--help`; canary in P8's review file.
        AgentKind::Claude => Carrier::AppendFile("--append-system-prompt-file"),
        AgentKind::Shell => Carrier::Unsupported("A shell has no agent instructions."),
        // Codex 0.163.0's app-server; canary in P8's review file. Without it, CODEX_UNSUPPORTED.
        AgentKind::Codex => Carrier::CodexDeveloperInstructions,
        AgentKind::Opencode => Carrier::Unsupported(
            "opencode instructions aren't verified yet: its instructions setting hasn't been measured, and opencode quietly ignores settings it rejects.",
        ),
        AgentKind::Grok => Carrier::Unsupported("Grok has no verified way to take instructions from Chorus."),
        _ => Carrier::Unsupported("This agent has no verified way to take instructions from Chorus."),
    }
}

/// A Codex run without Chorus's private app-server. `-c developer_instructions` would put the text
/// in argv and `runs.args`, and `model_instructions_file` replaces Codex's own instructions, so
/// neither is used (P8 review).
pub const CODEX_UNSUPPORTED: &str = "Codex gets instructions only through Chorus's private Codex server, which needs Codex 0.163.0. The other ways put them on the command line, where other programs can read them.";

/// Starts a Codex developer message that replaces one an earlier run of the conversation got: an
/// injected message stays in the conversation, so a second one adds to it.
pub const CODEX_REPLACES: &str = "These instructions from Chorus replace any earlier instructions from Chorus in this conversation.\n\n";

/// A note as composition needs it. `project` is false for a note that applies everywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteInput {
    pub id: u32,
    pub title: String,
    pub body: String,
    pub pinned: bool,
    pub project: bool,
    pub updated_at: i64,
}

/// A skill folder, read from its `SKILL.md` front matter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// The folder, absolute.
    pub path: String,
}

/// A note a run received, for session facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NoteRef {
    pub id: u32,
    pub title: String,
    pub pinned: bool,
    /// Tagged with the session's project, rather than applying everywhere.
    pub project: bool,
}

/// What a run's instructions held, kept on the run and shown in session facts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RunInstructions {
    /// The text reached the agent's launch (it was written and passed on).
    pub given: bool,
    /// Why nothing was given, or what went wrong, in words to show as is.
    pub reason: Option<String>,
    /// The Bot whose instructions were included.
    pub bot: Option<String>,
    pub notes: Vec<NoteRef>,
    /// Notes that applied but didn't fit under the cap.
    pub left_out: u32,
    pub skills: Vec<String>,
    /// Skill folders that couldn't be read, with why.
    pub skill_problems: Vec<String>,
    pub bytes: u32,
    /// [`text_hash`] of the text given, to tell whether a resumed conversation has it already.
    pub hash: Option<String>,
    /// Claude Code was told to render its system prompt afresh (`--system-prompt-snapshot off`),
    /// because the conversation recorded different instructions when it started.
    pub fresh_prompt: bool,
}

/// The composed text and what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composed {
    pub text: String,
    pub notes: Vec<NoteRef>,
    pub left_out: u32,
}

/// Notes in the order they are given: pinned first, then the most recently updated.
pub fn order_notes(notes: &mut [NoteInput]) {
    notes.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.updated_at.cmp(&a.updated_at)).then(b.id.cmp(&a.id)));
}

fn note_section(n: &NoteInput) -> String {
    format!("### {}\n\n{}\n", n.title.trim(), n.body.trim())
}

fn skill_index(skills: &[Skill]) -> String {
    let mut s = String::from("## Skills\n\nThese skill folders are available. Read a skill's SKILL.md before using it.\n\n");
    for k in skills {
        s.push_str(&format!("- **{}**: {} (`{}`)\n", k.name.trim(), k.description.trim(), k.path));
    }
    s
}

/// The text a run's agent gets, or `None` when there is nothing to give. Pinned notes come first,
/// then the most recently updated. Over [`MAX_INSTRUCTION_BYTES`], notes are left out from the
/// end of that order (the oldest unpinned first), and the text says how many.
pub fn compose(bot: Option<&str>, notes: &[NoteInput], skills: &[Skill]) -> Option<Composed> {
    compose_capped(bot, notes, skills, MAX_INSTRUCTION_BYTES)
}

pub fn compose_capped(bot: Option<&str>, notes: &[NoteInput], skills: &[Skill], cap: usize) -> Option<Composed> {
    let bot = bot.map(str::trim).filter(|b| !b.is_empty());
    let mut ordered: Vec<NoteInput> = notes.iter().filter(|n| !n.body.trim().is_empty() || !n.title.trim().is_empty()).cloned().collect();
    order_notes(&mut ordered);
    if bot.is_none() && ordered.is_empty() && skills.is_empty() {
        return None;
    }
    let head = "# Instructions from Chorus\n\nThe user set these up in Chorus for this session. They add to your usual instructions.\n";
    let bot_part = bot.map(|b| format!("\n## From the Bot\n\n{}\n", truncate(b, cap / 2)));
    let skills_part = (!skills.is_empty()).then(|| format!("\n{}", skill_index(skills)));
    let fixed = head.len() + bot_part.as_ref().map_or(0, String::len) + skills_part.as_ref().map_or(0, String::len);
    // Room for the notes heading and the "left out" line.
    let reserve = 120;
    let mut budget = cap.saturating_sub(fixed + reserve);
    let mut kept: Vec<&NoteInput> = Vec::new();
    let mut left_out = 0u32;
    for n in &ordered {
        let len = note_section(n).len() + 1;
        if len <= budget {
            budget -= len;
            kept.push(n);
        } else {
            left_out += 1;
        }
    }
    let mut text = String::from(head);
    if let Some(b) = &bot_part {
        text.push_str(b);
    }
    if !kept.is_empty() || left_out > 0 {
        text.push_str("\n## Notes\n");
        for n in &kept {
            text.push('\n');
            text.push_str(&note_section(n));
        }
        if left_out > 0 {
            text.push_str(&format!("\n({left_out} more note{} left out: they didn't fit.)\n", if left_out == 1 { " was" } else { "s were" }));
        }
    }
    if let Some(s) = &skills_part {
        text.push_str(s);
    }
    let text = truncate(&text, cap);
    Some(Composed {
        text,
        notes: kept.iter().map(|n| NoteRef { id: n.id, title: n.title.clone(), pinned: n.pinned, project: n.project }).collect(),
        left_out,
    })
}

/// At most `max` bytes, cut at a character boundary.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut end = max.saturating_sub(4);
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…\n", &s[..end])
}

/// A short, stable fingerprint of a text (FNV-1a, 64 bits), to notice a change. Not a secret.
pub fn text_hash(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// Longest skill name and description kept for the index, in characters, so one skill can't use
/// up the instructions' size limit.
pub const MAX_SKILL_NAME_CHARS: usize = 80;
pub const MAX_SKILL_DESCRIPTION_CHARS: usize = 300;

fn bounded(text: &str, max: usize) -> Option<String> {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = one_line.chars().take(max).collect();
    if one_line.chars().count() > max {
        out.push('…');
    }
    (!out.is_empty()).then_some(out)
}

/// A skill's name and description from its `SKILL.md` front matter (`---` … `---`, with `name:`
/// and `description:` lines; a description may be a `|` or `>` block of indented lines). Without
/// a name, the folder's own name is used by the caller. Both are bounded and kept on one line.
pub fn parse_skill(text: &str) -> Option<(Option<String>, Option<String>)> {
    let mut lines = text.lines().peekable();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let (mut name, mut description) = (None, None);
    let unquote = |v: &str| v.trim().trim_matches('"').trim_matches('\'').trim().to_owned();
    while let Some(raw) = lines.next() {
        let line = raw.trim();
        if line == "---" {
            return Some((name, description));
        }
        if let Some(v) = line.strip_prefix("name:") {
            name = bounded(&unquote(v), MAX_SKILL_NAME_CHARS);
        } else if let Some(v) = line.strip_prefix("description:") {
            let v = v.trim();
            let text = if matches!(v, "|" | ">" | "|-" | ">-" | "|+" | ">+") {
                // A block: the indented lines that follow.
                let mut block = Vec::new();
                while let Some(next) = lines.peek() {
                    if next.trim() == "---" || !(next.starts_with(' ') || next.starts_with('\t') || next.trim().is_empty()) {
                        break;
                    }
                    block.push(next.trim().to_owned());
                    lines.next();
                }
                block.join(" ")
            } else {
                unquote(v)
            };
            description = bounded(&text, MAX_SKILL_DESCRIPTION_CHARS);
        }
    }
    None
}

/// Checks a note before it is saved.
pub fn check_note(title: &str, body: &str) -> Result<(String, String), String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("A note needs a title.".into());
    }
    if title.chars().count() > MAX_NOTE_TITLE_CHARS || title.chars().any(char::is_control) {
        return Err(format!("A note's title is one line of up to {MAX_NOTE_TITLE_CHARS} characters."));
    }
    let body = body.replace("\r\n", "\n");
    if body.len() > MAX_NOTE_BODY_BYTES {
        return Err(format!("A note is up to {} KiB.", MAX_NOTE_BODY_BYTES / 1024));
    }
    if body.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
        return Err("A note can't hold control characters.".into());
    }
    Ok((title.to_owned(), body))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: u32, pinned: bool, updated_at: i64, body: &str) -> NoteInput {
        NoteInput { id, title: format!("Note {id}"), body: body.into(), pinned, project: id.is_multiple_of(2), updated_at }
    }

    #[test]
    fn nothing_to_give_is_none() {
        assert_eq!(compose(None, &[], &[]), None);
        assert_eq!(compose(Some("   "), &[], &[]), None);
    }

    #[test]
    fn bot_then_pinned_then_newest_notes_then_skills() {
        let notes = [note(1, false, 10, "old"), note(2, true, 1, "pinned"), note(3, false, 20, "new")];
        let skills = [Skill { name: "deploy".into(), description: "Ship it".into(), path: "C:\\skills\\deploy".into() }];
        let c = compose(Some("Be brief."), &notes, &skills).unwrap();
        let at = |s: &str| c.text.find(s).unwrap_or_else(|| panic!("{s} missing from {}", c.text));
        assert!(at("Be brief.") < at("pinned"));
        assert!(at("pinned") < at("new") && at("new") < at("old"));
        assert!(at("old") < at("## Skills") && at("deploy") > at("## Skills"));
        assert!(c.text.contains("Read a skill's SKILL.md before using it"));
        assert_eq!(c.notes.iter().map(|n| n.id).collect::<Vec<_>>(), [2, 3, 1]);
        assert_eq!(c.left_out, 0);
    }

    #[test]
    fn the_cap_leaves_out_the_oldest_unpinned_notes_first_and_says_so() {
        let big = "x".repeat(900);
        let notes = [note(1, false, 1, &big), note(2, true, 0, &big), note(3, false, 3, &big), note(4, false, 2, &big)];
        let c = compose_capped(None, &notes, &[], 2600).unwrap();
        // Room for two notes: the pinned one and the newest.
        assert_eq!(c.notes.iter().map(|n| n.id).collect::<Vec<_>>(), [2, 3]);
        assert_eq!(c.left_out, 2);
        assert!(c.text.contains("2 more notes were left out"));
        assert!(c.text.len() <= 2600);
    }

    #[test]
    fn the_whole_text_never_passes_the_cap() {
        let c = compose_capped(Some(&"é".repeat(20_000)), &[], &[], 4096).unwrap();
        assert!(c.text.len() <= 4096);
        assert!(c.text.is_char_boundary(c.text.len()));
        let c = compose(Some(&"b".repeat(40_000)), &[note(1, true, 1, &"n".repeat(16_000))], &[]).unwrap();
        assert!(c.text.len() <= MAX_INSTRUCTION_BYTES);
    }

    #[test]
    fn every_agent_declares_a_carrier_and_only_verified_ones_carry() {
        for kind in AgentKind::ALL {
            match carrier(kind) {
                Carrier::AppendFile(flag) => {
                    assert_eq!(kind, AgentKind::Claude);
                    assert!(flag.starts_with("--") && flag.ends_with("-file"), "a file flag keeps the text out of argv");
                }
                Carrier::CodexDeveloperInstructions => assert_eq!(kind, AgentKind::Codex),
                Carrier::Unsupported(reason) => assert!(reason.ends_with('.') && reason.len() > 20),
            }
        }
    }

    #[test]
    fn skill_front_matter_is_read() {
        let text = "---\nname: deploy\ndescription: \"Ship the app\"\nother: x\n---\n# Deploy\n";
        assert_eq!(parse_skill(text), Some((Some("deploy".into()), Some("Ship the app".into()))));
        assert_eq!(parse_skill("# No front matter"), None);
        assert_eq!(parse_skill("---\nname: open\n"), None);
    }

    #[test]
    fn block_descriptions_are_read_and_long_ones_are_bounded() {
        let block = "---\nname: review\ndescription: >\n  Reviews a change\n  for bugs.\nother: x\n---\n";
        assert_eq!(parse_skill(block), Some((Some("review".into()), Some("Reviews a change for bugs.".into()))));
        let long = format!("---\nname: big\ndescription: {}\n---\n", "word ".repeat(10_000));
        let (_, d) = parse_skill(&long).unwrap();
        assert_eq!(d.unwrap().chars().count(), MAX_SKILL_DESCRIPTION_CHARS + 1);
    }

    #[test]
    fn the_hash_tells_texts_apart() {
        assert_eq!(text_hash("a"), text_hash("a"));
        assert_ne!(text_hash("a"), text_hash("b"));
        assert_eq!(text_hash("").len(), 16);
    }

    #[test]
    fn notes_are_checked() {
        assert!(check_note("", "x").is_err());
        assert!(check_note("a\nb", "x").is_err());
        assert!(check_note("ok", &"x".repeat(MAX_NOTE_BODY_BYTES + 1)).is_err());
        assert!(check_note("ok", "bell\u{7}").is_err());
        assert_eq!(check_note(" Title ", "a\r\nb\tc").unwrap(), ("Title".into(), "a\nb\tc".into()));
    }
}
