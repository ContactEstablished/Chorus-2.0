//! OpenMausBot as Chorus sees it through its local API (P18, `_docs/spikes/P18.md` S1). Pure:
//! reading its bot list, choosing a bot for a project, naming the hand-off's branch and folder,
//! and writing what Chorus puts in the note and sends the bot.

use serde_json::Value;

use crate::handoff::{self, Author, Entry, Fields, Note, one_line, stamp};
use crate::projects::same_folder;
use crate::worktree;

/// OpenMausBot's API, on loopback only (it listens on 127.0.0.1 unless told otherwise).
pub const API: &str = "http://127.0.0.1:8799";
/// The vault label of the token Chorus keeps for it.
pub const CREDENTIAL_LABEL: &str = "MausBot";
/// How Chorus names itself when it pairs.
pub const PAIRING_LABEL: &str = "Chorus";
/// MausBot's limit on a conversation title.
const TITLE_CHARS: usize = 80;
const LINE_CHARS: usize = 200;
/// Commits and files named in a from-git entry before "and N more".
const NAMED_COMMITS: usize = 5;
const NAMED_FILES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversation {
    pub thread_id: String,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub busy: bool,
    pub updated_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bot {
    pub id: String,
    pub name: String,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub busy: bool,
    pub conversations: Vec<Conversation>,
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

/// The bots in `GET /api/bots`'s answer (`{"bots": [...]}`). A bot or conversation without an id
/// is skipped; a missing list is an error.
pub fn parse_bots(v: &Value) -> Result<Vec<Bot>, String> {
    let list = v.get("bots").and_then(Value::as_array).ok_or("MausBot answered without a bot list.")?;
    Ok(list
        .iter()
        .filter_map(|b| {
            let id = text(b, "id").filter(|id| valid_id(id))?;
            let conversations = b
                .get("tasks")
                .and_then(Value::as_array)
                .map(|ts| {
                    ts.iter()
                        .filter_map(|t| {
                            Some(Conversation {
                                thread_id: text(t, "threadId").filter(|id| valid_id(id))?,
                                title: text(t, "title"),
                                cwd: text(t, "cwd"),
                                busy: t.get("busy").and_then(Value::as_bool).unwrap_or(false),
                                updated_at_ms: t.get("updatedAt").and_then(Value::as_i64),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(Bot {
                name: text(b, "name").unwrap_or_else(|| id.clone()),
                id,
                title: text(b, "title"),
                cwd: text(b, "cwd"),
                busy: b.get("busy").and_then(Value::as_bool).unwrap_or(false),
                conversations,
            })
        })
        .collect())
}

/// The hand-off's conversation: by its thread when Chorus started it, else the newest one in the
/// hand-off's folder (Matthew started it by hand there).
pub fn conversation<'a>(bots: &'a [Bot], thread_id: Option<&str>, folder: &str) -> Option<(&'a Bot, &'a Conversation)> {
    let all = bots.iter().flat_map(|b| b.conversations.iter().map(move |c| (b, c)));
    match thread_id {
        Some(t) => all.into_iter().find(|(_, c)| c.thread_id == t),
        None => all
            .filter(|(_, c)| c.cwd.as_deref().is_some_and(|cwd| same_folder(cwd, folder)))
            .max_by_key(|(_, c)| c.updated_at_ms.unwrap_or(0)),
    }
}

/// The bot to offer first: the last one used for the project if it's still there, else the first
/// whose own folder is the project's.
pub fn default_bot(bots: &[Bot], project_folder: &str, last_bot: Option<&str>) -> Option<String> {
    if let Some(last) = last_bot.filter(|l| bots.iter().any(|b| b.id == *l)) {
        return Some(last.to_owned());
    }
    bots.iter().find(|b| b.cwd.as_deref().is_some_and(|c| same_folder(c, project_folder))).map(|b| b.id.clone())
}

/// A bot or thread id safe in a URL path: 1–128 letters, digits, `_` or `-`.
pub fn valid_id(id: &str) -> bool {
    (1..=128).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// A MausBot access token: `omb_sess_` and URL-safe base64 (43 characters in 0.1.169). The same
/// shape the scrubber's `openmausbot-session-token` pattern redacts.
pub fn is_token(s: &str) -> bool {
    s.strip_prefix("omb_sess_")
        .is_some_and(|rest| (40..=200).contains(&rest.len()) && rest.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
}

/// A pairing code from MausBot's Settings → Phone & devices: 12 letters or digits once dashes and
/// spaces are dropped (`7KQ2-M9XD-H4TP`).
pub fn is_pairing_code(s: &str) -> bool {
    let kept: Vec<char> = s.chars().filter(|c| *c != '-' && !c.is_whitespace()).collect();
    kept.len() == 12 && kept.iter().all(char::is_ascii_alphanumeric)
}

/// The task made safe for a branch: lowercase letters and digits, `-` between words, at most 32
/// characters. Nothing usable gives "handoff".
pub fn slug(task: &str) -> String {
    let mut s = String::new();
    for c in task.trim().to_lowercase().chars() {
        let c = if c.is_ascii_alphanumeric() { c } else { '-' };
        if !(c == '-' && (s.is_empty() || s.ends_with('-'))) {
            s.push(c);
        }
    }
    let s: String = s.chars().take(32).collect();
    let s = s.trim_matches('-');
    if s.is_empty() { "handoff".to_owned() } else { s.to_owned() }
}

/// Where the hand-off's worktree goes and its branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub branch: String,
    pub path: String,
}

/// Branch `maus/<date>-<slug>` in `<parent>\.chorus2\<repo>\maus-<date>-<slug>`, beside the
/// repository like a session's worktree (K10). `-2`…`-20` when the name is taken.
pub fn plan(main_root: &str, date: &str, task: &str, taken: &dyn Fn(&str, &str) -> bool) -> Result<Plan, String> {
    let slug = slug(task);
    for n in 1..=20 {
        let suffix = if n == 1 { String::new() } else { format!("-{n}") };
        let name = format!("{date}-{slug}{suffix}");
        let path = worktree::plan(main_root, &format!("maus-{name}"))?.path;
        let branch = format!("maus/{name}");
        if !taken(&branch, &path) {
            return Ok(Plan { branch, path });
        }
    }
    Err("There are already 20 hand-off branches like this today. Delete some first.".into())
}

/// What Chorus knows when it hands a project to MausBot.
pub struct HandoffFacts<'a> {
    pub at_ms: i64,
    pub offset_minutes: i32,
    pub project: &'a str,
    pub branch: &'a str,
    pub worktree: &'a str,
    /// `main @ 1a2b3c4`.
    pub base: &'a str,
    pub task: &'a str,
    pub bot: Option<&'a str>,
    /// The note's newest entry, if any, to carry its lines forward.
    pub latest: Option<&'a Fields>,
}

/// The hand-off entry's heading, from when it was written. Recomputed on return to find the
/// entries MausBot added above it.
pub fn handoff_heading(at_ms: i64, offset_minutes: i32) -> String {
    format!("## {} · Chorus → MausBot", stamp(at_ms, offset_minutes))
}

pub fn handoff_entry(f: &HandoffFacts) -> String {
    let task = f.task.trim();
    let latest = |pick: fn(&Fields) -> &Option<String>| f.latest.and_then(|l| pick(l).clone());
    let done = latest(|l| &l.done).unwrap_or_else(|| "See the entry below".into());
    let open = if task.is_empty() { latest(|l| &l.open).unwrap_or_else(|| "Nothing named".into()) } else { task.to_owned() };
    let next = if task.is_empty() { latest(|l| &l.next).unwrap_or_else(|| "Pick up from the entry below".into()) } else { task.to_owned() };
    let mut s = format!("{}\n", handoff_heading(f.at_ms, f.offset_minutes));
    s.push_str(&format!("Done: {}\n", one_line(&done, LINE_CHARS)));
    s.push_str(&format!("State: {}\n", one_line(&format!("{} from {}, in {}", f.branch, f.base, f.worktree), 400)));
    s.push_str(&format!("Open: {}\n", one_line(&open, LINE_CHARS)));
    s.push_str(&format!("Next: {}\n", one_line(&next, LINE_CHARS)));
    s.push_str(&format!("Bot: {}\n\n", one_line(f.bot.unwrap_or("started by hand"), 80)));
    s
}

/// What MausBot is sent (or what Matthew pastes into it): where to work, the rules, and how to
/// hand back.
pub fn message(f: &HandoffFacts) -> String {
    let task = one_line(f.task, 400);
    let task_line = if task.is_empty() { String::new() } else { format!("\nTask: {task}\n") };
    format!(
        "You're picking up {project} from Chorus.\n\n\
         Work only in this folder: {worktree}\n\
         It's a git worktree on branch {branch}, made from {base}. Commit your work there. Don't merge, \
         rebase or push, and don't change any other checkout.\n\n\
         First read .chorus/handoff.md in that folder. Its newest entry says what was done, where things \
         stand and what's next.\n{task_line}\n\
         When you stop, add an entry at the top of .chorus/handoff.md, under the \"# Hand-off note\" text, \
         headed \"## <date> <time> · MausBot → Chorus\", with Done:, State:, Open: and Next: lines. Then \
         change \"baton: mausbot\" to \"baton: chorus\" in its front matter. Chorus watches that line.\n",
        project = one_line(f.project, 120),
        worktree = f.worktree,
        branch = f.branch,
        base = f.base,
    )
}

/// The MausBot conversation's title.
pub fn task_title(project: &str, task: &str) -> String {
    let task = task.trim();
    let title = if task.is_empty() { format!("{project}: hand-off from Chorus") } else { format!("{project}: {task}") };
    one_line(&title, TITLE_CHARS)
}

/// MausBot's send id, so a retried send never sends twice: 16–80 letters, digits, `-` or `_`.
pub fn send_id(handoff_id: u32, nonce_hex: &str) -> String {
    let nonce: String = nonce_hex.chars().filter(char::is_ascii_hexdigit).take(16).collect();
    format!("chorus-handoff-{handoff_id}-{nonce}")
}

/// The entries MausBot added above the hand-off entry in its copy of the note. With that heading
/// changed, the entries above the first `→ MausBot` heading, else above the first of Chorus's own;
/// with none of those, every entry is MausBot's.
pub fn new_entries(worktree_note: &Note, handoff_heading: &str) -> Vec<Entry> {
    let wanted = handoff_heading.trim_start_matches("## ").trim();
    let entries = &worktree_note.entries;
    let stop = entries
        .iter()
        .position(|e| e.heading() == wanted)
        .or_else(|| entries.iter().position(|e| e.heading().ends_with("→ MausBot")))
        .or_else(|| entries.iter().position(|e| e.author() == Author::Chorus))
        .unwrap_or(entries.len());
    entries[..stop].to_vec()
}

/// Whether any of the entries is MausBot's own.
pub fn has_bot_entry(entries: &[Entry]) -> bool {
    entries.iter().any(|e| e.author() == Author::Mausbot)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub sha: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub added: Option<u32>,
    pub removed: Option<u32>,
}

/// Chorus's entry when it couldn't read the branch at all: it says so, rather than "no commits".
pub fn from_git_failure_entry(at_ms: i64, offset_minutes: i32, branch: &str, problem: &str) -> String {
    let mut s = format!("## {} · MausBot → Chorus (from git)\n", stamp(at_ms, offset_minutes));
    s.push_str(&format!("Done: Chorus couldn't read {branch}: {}\n", one_line(problem, 200)));
    s.push_str("State: Unknown: check the branch by hand\n");
    s.push_str("Open: MausBot didn't write an entry\n");
    s.push_str(&format!("Next: Review {branch} by hand\n\n"));
    s
}

/// Chorus's entry for MausBot's work when the bot wrote none: its commits and the files they changed.
#[allow(clippy::too_many_arguments, reason = "each argument is one fact of the entry")]
pub fn from_git_entry(at_ms: i64, offset_minutes: i32, branch: &str, base: &str, commits: &[Commit], more_commits: u32, files: &[FileChange], more_files: u32) -> String {
    let total_commits = commits.len() as u32 + more_commits;
    let done = if total_commits == 0 {
        format!("No commits on {branch}")
    } else {
        let mut named: Vec<String> = commits.iter().take(NAMED_COMMITS).map(|c| one_line(&c.subject, 80)).collect();
        let beyond = total_commits.saturating_sub(named.len() as u32);
        if beyond > 0 {
            named.push(format!("and {beyond} more"));
        }
        format!("{total_commits} commit{} on {branch}: {}", if total_commits == 1 { "" } else { "s" }, named.join("; "))
    };
    let total_files = files.len() as u32 + more_files;
    let state = if total_files == 0 {
        format!("No files changed against {base}")
    } else {
        let mut named: Vec<String> = files.iter().take(NAMED_FILES).map(|f| f.path.clone()).collect();
        let beyond = total_files.saturating_sub(named.len() as u32);
        if beyond > 0 {
            named.push(format!("and {beyond} more"));
        }
        format!("{total_files} file{} changed against {base}: {}", if total_files == 1 { "" } else { "s" }, named.join(", "))
    };
    let mut s = format!("## {} · MausBot → Chorus (from git)\n", stamp(at_ms, offset_minutes));
    s.push_str(&format!("Done: {}\n", one_line(&done, 400)));
    s.push_str(&format!("State: {}\n", one_line(&state, 400)));
    s.push_str("Open: MausBot didn't write an entry\n");
    s.push_str(&format!("Next: Review {branch} and merge it when it's ready\n\n"));
    s
}

/// What "Continue from here" puts in a session's composer. Never sent by itself.
pub fn continue_text(project: &str, branch: &str, worktree: &str, base: &str, ahead: u32, merged: bool, entries: &[Fields]) -> String {
    let state = if merged {
        format!("{branch} is merged into {base}")
    } else {
        format!("{branch} is {ahead} commit{} ahead of {base} and not merged yet", if ahead == 1 { "" } else { "s" })
    };
    let mut s = format!("Picking up {} from MausBot's work. {state}. It's checked out in {worktree}.\n", one_line(project, 120));
    for f in entries {
        for (label, v) in [("Done", &f.done), ("State", &f.state), ("Open", &f.open), ("Next", &f.next)] {
            if let Some(v) = v {
                s.push_str(&format!("{label}: {}\n", one_line(v, 400)));
            }
        }
    }
    s.push_str("\nReview the branch, then decide how to carry on.");
    s
}

/// Whether the bot's copy of the note hands the baton back: `baton: chorus`, exactly. A missing or
/// blank baton (read as nobody) is MausBot still working on it.
pub fn handed_back(note: &Note) -> bool {
    note.meta.baton == handoff::Baton::Chorus
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        json!({
            "bots": [
                { "id": "bot-a", "name": "Atlas Dev", "title": "Atlas developer", "cwd": "C:\\work\\atlas", "busy": false, "createdAt": 1,
                  "tasks": [
                    { "threadId": "t-1", "title": "Old", "cwd": "C:\\work\\atlas", "busy": false, "updatedAt": 100 },
                    { "threadId": "t-2", "title": "Hand-off", "cwd": "D:\\w\\.chorus2\\atlas\\maus-2026-11-18-x", "busy": true, "updatedAt": 300 },
                    { "title": "no id" }
                  ] },
                { "id": "bot-b", "name": "Basil", "busy": true },
                { "name": "no id" },
                { "id": "bad id!", "name": "x" }
            ]
        })
    }

    #[test]
    fn the_bot_list_reads_every_field_type_and_skips_junk() {
        let bots = parse_bots(&fixture()).unwrap();
        assert_eq!(bots.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(), ["bot-a", "bot-b"]);
        let a = &bots[0];
        assert_eq!((a.name.as_str(), a.title.as_deref(), a.cwd.as_deref(), a.busy), ("Atlas Dev", Some("Atlas developer"), Some("C:\\work\\atlas"), false));
        assert_eq!(a.conversations.len(), 2);
        assert_eq!(a.conversations[1].updated_at_ms, Some(300));
        assert!(a.conversations[1].busy);
        assert_eq!((bots[1].cwd.as_deref(), bots[1].conversations.len(), bots[1].busy), (None, 0, true));
        assert!(parse_bots(&json!({})).unwrap_err().contains("bot list"));
    }

    #[test]
    fn the_conversation_is_found_by_thread_else_by_folder() {
        let bots = parse_bots(&fixture()).unwrap();
        assert_eq!(conversation(&bots, Some("t-1"), "x").map(|(b, c)| (b.id.as_str(), c.thread_id.as_str())), Some(("bot-a", "t-1")));
        assert_eq!(conversation(&bots, None, "d:/w/.chorus2/atlas/maus-2026-11-18-x").map(|(_, c)| c.thread_id.as_str()), Some("t-2"));
        assert_eq!(conversation(&bots, Some("nope"), "x"), None);
    }

    #[test]
    fn the_default_bot_is_the_last_used_else_the_one_in_the_project_folder() {
        let bots = parse_bots(&fixture()).unwrap();
        assert_eq!(default_bot(&bots, "c:/work/atlas", None).as_deref(), Some("bot-a"));
        assert_eq!(default_bot(&bots, "c:/work/atlas", Some("bot-b")).as_deref(), Some("bot-b"));
        assert_eq!(default_bot(&bots, "c:/work/atlas", Some("gone")).as_deref(), Some("bot-a"));
        assert_eq!(default_bot(&bots, "c:/elsewhere", None), None);
    }

    #[test]
    fn ids_tokens_and_codes_are_checked() {
        assert!(valid_id("7db98c8f-7785-4493-8816-a65af8879da7"));
        assert!(!valid_id("") && !valid_id("a/b") && !valid_id(&"a".repeat(129)));
        let token = format!("omb_sess_{}", "Ab9_-".repeat(9));
        assert!(is_token(&token));
        assert!(!is_token("omb_sess_short") && !is_token(&format!("omb_sess_{}", "a b".repeat(10))));
        assert!(is_pairing_code("7KQ2-M9XD-H4TP") && is_pairing_code("7kq2 m9xd h4tp"));
        assert!(!is_pairing_code("7KQ2-M9XD") && !is_pairing_code("7KQ2-M9XD-H4T!"));
    }

    #[test]
    fn slugs_are_short_and_safe() {
        assert_eq!(slug("Fix the checkout, again!"), "fix-the-checkout-again");
        assert_eq!(slug(""), "handoff");
        assert_eq!(slug("¿¿¿"), "handoff");
        assert_eq!(slug("a very long task name that keeps going and going"), "a-very-long-task-name-that-keeps");
        assert!(slug("a very long task name that keeps going and going").len() <= 32);
    }

    #[test]
    fn the_plan_sits_beside_the_repository_and_skips_taken_names() {
        let p = plan("D:\\work\\atlas", "2026-11-18", "Fix it", &|_, _| false).unwrap();
        assert_eq!(p.branch, "maus/2026-11-18-fix-it");
        assert_eq!(p.path, "D:\\work\\.chorus2\\atlas\\maus-2026-11-18-fix-it");
        let taken = |b: &str, _: &str| b == "maus/2026-11-18-fix-it" || b == "maus/2026-11-18-fix-it-2";
        assert_eq!(plan("D:\\work\\atlas", "2026-11-18", "Fix it", &taken).unwrap().branch, "maus/2026-11-18-fix-it-3");
        assert!(plan("D:\\work\\atlas", "2026-11-18", "x", &|_, _| true).unwrap_err().contains("20"));
        assert!(plan("D:\\", "2026-11-18", "x", &|_, _| false).is_err(), "a drive's root has no folder beside it");
    }

    fn facts<'a>(task: &'a str, latest: Option<&'a Fields>) -> HandoffFacts<'a> {
        HandoffFacts {
            at_ms: 1_794_960_000_000,
            offset_minutes: 0,
            project: "Atlas",
            branch: "maus/2026-11-18-fix-it",
            worktree: "D:\\work\\.chorus2\\atlas\\maus-2026-11-18-fix-it",
            base: "main @ 1a2b3c4",
            task,
            bot: Some("Atlas Dev"),
            latest,
        }
    }

    #[test]
    fn the_entry_and_the_message_name_the_folder_branch_and_base() {
        let latest = Fields { done: Some("Built the store".into()), open: Some("Tests".into()), next: Some("Write tests".into()), ..Fields::default() };
        let e = handoff_entry(&facts("", Some(&latest)));
        assert!(e.starts_with("## 2026-11-18 00:00 · Chorus → MausBot\n"));
        assert!(e.contains("Done: Built the store\n") && e.contains("Open: Tests\n") && e.contains("Next: Write tests\n"));
        assert!(e.contains("State: maus/2026-11-18-fix-it from main @ 1a2b3c4, in D:\\work\\.chorus2\\atlas\\maus-2026-11-18-fix-it\n"));
        assert!(e.contains("Bot: Atlas Dev\n"));
        let with_task = handoff_entry(&facts("Fix it", None));
        assert!(with_task.contains("Open: Fix it\n") && with_task.contains("Next: Fix it\n") && with_task.contains("Done: See the entry below\n"));
        let m = message(&facts("Fix it", None));
        for needle in ["Work only in this folder: D:\\work\\.chorus2\\atlas\\maus-2026-11-18-fix-it", "branch maus/2026-11-18-fix-it", "made from main @ 1a2b3c4", "Task: Fix it", "baton: chorus", "Don't merge"] {
            assert!(m.contains(needle), "{needle} in {m}");
        }
        assert!(!message(&facts("", None)).contains("Task:"));
    }

    #[test]
    fn titles_and_send_ids_fit_mausbots_limits() {
        assert_eq!(task_title("Atlas", ""), "Atlas: hand-off from Chorus");
        assert!(task_title("Atlas", &"x".repeat(200)).chars().count() <= TITLE_CHARS);
        let id = send_id(7, "00ff11ee22dd33cc44bb");
        assert_eq!(id, "chorus-handoff-7-00ff11ee22dd33cc");
        assert!((16..=80).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'));
    }

    #[test]
    fn new_entries_stop_at_the_handoff_entry() {
        let heading = handoff_heading(1_794_960_000_000, 0);
        let text = format!("---\nbaton: chorus\n---\n\n## 2026-11-18 03:00 · MausBot → Chorus\nDone: x\n\n{heading}\nDone: y\n\n## 2026-11-17 · Chorus · latest work\nDone: z\n");
        let note = handoff::parse(&text).unwrap();
        let new = new_entries(&note, &heading);
        assert_eq!(new.len(), 1);
        assert!(has_bot_entry(&new));
        assert!(handed_back(&note));
        let renamed = text.replace("2026-11-18 00:00 · Chorus → MausBot", "edited · Chorus → MausBot");
        assert_eq!(new_entries(&handoff::parse(&renamed).unwrap(), &heading).len(), 1, "falls back to the first → MausBot heading");
        let untouched = format!("---\nbaton: mausbot\n---\n\n{heading}\nDone: y\n");
        let note = handoff::parse(&untouched).unwrap();
        assert!(new_entries(&note, &heading).is_empty() && !handed_back(&note));
        let blank = handoff::parse("---\nbaton:\n---\n").unwrap();
        assert!(!handed_back(&blank), "a blank baton is MausBot still working");
        let ascii = text.replace("· Chorus → MausBot", "· Chorus -> MausBot");
        assert_eq!(new_entries(&handoff::parse(&ascii).unwrap(), &heading).len(), 1, "stops at Chorus's own entry");
        let rewritten = handoff::parse("---\nbaton: chorus\n---\n\n## a · MausBot → Chorus\nDone: x\n\n## b\nDone: y\n").unwrap();
        assert_eq!(new_entries(&rewritten, &heading).len(), 2, "a note MausBot rewrote is all its own");
    }

    #[test]
    fn a_branch_git_couldnt_read_says_so() {
        let e = from_git_failure_entry(1_794_960_000_000, 0, "maus/x", "git log: unknown revision");
        assert!(e.contains("Done: Chorus couldn't read maus/x: git log: unknown revision\n"));
        assert!(!e.contains("No commits"));
    }

    #[test]
    fn the_from_git_entry_names_commits_and_files_with_caps() {
        let commits: Vec<Commit> = (0..7).map(|i| Commit { sha: format!("{i}"), subject: format!("c{i}") }).collect();
        let files: Vec<FileChange> = (0..10).map(|i| FileChange { path: format!("f{i}.rs"), added: Some(1), removed: None }).collect();
        let e = from_git_entry(1_794_960_000_000, 0, "maus/x", "main", &commits, 3, &files, 0);
        assert!(e.starts_with("## 2026-11-18 00:00 · MausBot → Chorus (from git)\n"));
        assert!(e.contains("Done: 10 commits on maus/x: c0; c1; c2; c3; c4; and 5 more\n"), "{e}");
        assert!(e.contains("State: 10 files changed against main: f0.rs, f1.rs, f2.rs, f3.rs, f4.rs, f5.rs, f6.rs, f7.rs, and 2 more\n"), "{e}");
        assert!(e.contains("Open: MausBot didn't write an entry\n"));
        let none = from_git_entry(0, 0, "maus/x", "main", &[], 0, &[], 0);
        assert!(none.contains("Done: No commits on maus/x\n") && none.contains("State: No files changed against main\n"));
        let parsed = handoff::parse(&format!("---\n---\n{e}")).unwrap();
        assert_eq!(parsed.entries[0].author(), Author::Chorus, "Chorus wrote it, on the bot's behalf");
    }

    #[test]
    fn the_continue_text_says_where_the_branch_stands() {
        let f = Fields { done: Some("Fixed it".into()), next: Some("Merge".into()), ..Fields::default() };
        let t = continue_text("Atlas", "maus/x", "D:\\w", "main", 2, false, &[f]);
        assert!(t.contains("maus/x is 2 commits ahead of main and not merged yet"));
        assert!(t.contains("Done: Fixed it\nNext: Merge\n"));
        assert!(continue_text("Atlas", "maus/x", "D:\\w", "main", 0, true, &[]).contains("maus/x is merged into main"));
    }
}
