# Implementation spec 18-1: Hand-off note

## 1. The file

`<project folder>/.chorus/handoff.md`, UTF-8, LF:

````markdown
---
project: Atlas
baton: chorus
branch:
worktree:
base:
updated: 2026-11-18T14:05:00Z
---

# Hand-off note

Kept by Chorus and MausBot. Newest entry first. Each entry has Done, State, Open and Next lines.
Add yours at the top; leave the others as they are.

## 2026-11-18 14:05 · Chorus · latest work
Done: Read the spec; Add the migration (2 of 5 plan steps)
State: branch main @ 1a2b3c4, 3 uncommitted files
Open: Write the store; Wire the commands; Check in the app
Next: Write the store
Session: Vince (Claude Code)
Last reply (14:04): Migration added. Next I'll write the store and its tests.
````

### Front matter
- **Bounds:**
  - It starts at the file's first line, which must be exactly `---`, and ends at the next line that
    is exactly `---`.
  - Each line between is `key: value`. The value may be empty.
  - A value is cut at the first ` #` (space, then `#`), so a comment like
    `baton: mausbot   # chorus | mausbot | nobody` reads `mausbot`.
  - Blank lines are allowed.
- **Known keys,** written in this order:
  - `project`
  - `baton`: `chorus`, `mausbot` or `nobody`. Missing means `nobody`.
  - `branch`, `worktree` and `base`: empty means none.
  - `updated`
- **Unknown keys** are kept, in their order, after the known ones.
- **Errors,** each with its own sentence:
  - no first `---`
  - no closing `---` (a heading line before the closing `---` also reads as this)
  - a line without `:`
  - a key given twice
  - a `baton` value other than the three

### Body
- The **preamble** is everything after the front matter, up to the first line that starts with
  `## `.
- An **entry** runs from a `## ` line up to the next one. The text is kept as written.
- **Fields** are read for display only:
  - `Done:`, `State:`, `Open:` and `Next:` lines (case-insensitive label). A value continues
    only on the following *indented* lines, up to a blank line or an unindented line.
  - Every other non-empty line goes in `other`, so a bot's plain sentence after `Next:` isn't
    swallowed into it.
- **Author:**
  - `chorus`: the heading has `· Chorus ·` or `· Chorus →`.
  - `mausbot`: the heading has `· MausBot` before any `→`, and isn't `(from git)`.
  - `other`: everything else.
- **Latest work:** the heading ends with `· Chorus · latest work`.

## 2. Core: `crates/chorus-core/src/handoff.rs`

```rust
pub const NOTE_FILE: &str = ".chorus/handoff.md";
pub const ARCHIVE_FILE: &str = ".chorus/handoff-archive.md";
pub const EXCLUDE_LINES: [&str; 2] = ["/.chorus/handoff.md", "/.chorus/handoff-archive.md"];
pub const MAX_ENTRIES: usize = 20;
pub const MAX_NOTE_BYTES: usize = 64 * 1024;
pub const MAX_READ_BYTES: usize = 1024 * 1024;
pub const LATEST_MARK: &str = "· Chorus · latest work";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Baton { Chorus, Mausbot, Nobody }            // as_str / parse

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta { pub project: String, pub baton: Baton, pub branch: Option<String>,
                  pub worktree: Option<String>, pub base: Option<String>,
                  pub updated: Option<String>, pub extra: Vec<(String, String)> }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry { pub raw: String }                  // from "## " to just before the next
pub struct Fields { pub done, state, open, next: Option<String>, pub other: Vec<String> }
pub enum Author { Chorus, Mausbot, Other }            // Serialize as kebab-case for the view

pub struct Note { pub meta: Meta, pub preamble: String, pub entries: Vec<Entry> }

pub fn parse(text: &str) -> Result<Note, String>;
pub fn render(note: &Note) -> String;                 // parse(render(n)) == n
pub fn new_note(project: &str) -> Note;               // the preamble above, no entries
pub fn with_latest_work(note: Note, entry: String) -> Note;
pub fn prepend(note: Note, entries: Vec<String>) -> Note;
pub fn bound(note: Note) -> (Note, Vec<Entry>);       // the moved entries, newest first
pub fn archive(existing: Option<&str>, moved: &[Entry]) -> String;
pub fn with_exclude(existing: &str) -> Option<String>; // None when both lines are already there

pub struct WorkFacts<'a> {
    pub at_ms: i64, pub offset_minutes: i32,
    pub session: &'a str, pub agent: &'a str,
    pub plan: &'a [crate::progress::PlanStep],
    pub last_prompt: Option<&'a str>,
    pub last_reply: Option<(&'a str, i64)>,           // text, at_ms
    pub git: Option<GitLine<'a>>,                    // None: not a repository
}
pub struct GitLine<'a> { pub branch: Option<&'a str>, pub head: Option<&'a str>, pub dirty_files: u32 }
pub fn latest_work_entry(f: &WorkFacts) -> String;
pub fn stamp(at_ms: i64, offset_minutes: i32) -> String;   // "2026-11-18 14:05"
pub fn clock(at_ms: i64, offset_minutes: i32) -> String;   // "14:04"
pub fn one_line(s: &str, max_chars: usize) -> String;      // whitespace runs → one space, control chars dropped, "…" when cut
```

### Rules
- **`render`:**
  - Known keys in order, then the extras, with an empty value written as `key:`.
  - Then `---`, a blank line, the preamble as kept, then the entries' raw text in order.
  - Entries keep their own trailing newlines.
  - The note ends with exactly one `\n`.
- **`with_latest_work`:** if `entries[0]` is a latest-work entry it is replaced, otherwise the new
  entry goes first (K2). The new entry's raw text ends `\n\n`.
- **`bound`:**
  - While `entries.len() > MAX_ENTRIES` or the rendered size is over `MAX_NOTE_BYTES`, move the last
    entry out.
  - The newest entry is never moved.
- **`archive`:**
  - The file is `# Hand-off archive\n\n` followed by entries, newest first.
  - Moved entries go above the archive's existing entries.
  - An existing archive that doesn't start with the heading is kept below the moved entries,
    unchanged.
- **`latest_work_entry`:** heading `## {stamp} {LATEST_MARK}`, then these lines:
  - **`Done:`** the completed steps, joined with `; `, at most 6, then `and N more`, followed by
    `(k of n plan steps)`. With no plan: `Last turn: {one_line(prompt, 160)}`, or
    `Nothing recorded yet`.
  - **`State:`** `branch {b} @ {head}`, plus `, clean` or `, {n} uncommitted file(s)`. With no git:
    `not a git repository`. A detached head reads `detached @ {head}`.
  - **`Open:`** the in-progress steps, then the pending ones (at most 6, then `and N more`). With no
    plan: `No plan in this session`. With every step done: `Nothing open in the plan`.
  - **`Next:`** the first in-progress step, else the first pending one, else
    `All {n} plan steps are done`. With no plan: `See the last reply`.
  - **`Session:`** `{session} ({agent})`.
  - **`Last reply ({clock}):`** `{one_line(reply, 240)}`, only when there is one.
- **Step status:** `completed` and `done` count as done; `in_progress`, `in-progress`,
  `inProgress` and `active` count as current; anything else is pending.
- **Text:** every value goes through `one_line(…, 200)` before it lands in a line, so a newline can
  never start a fake heading.
- **`stamp` and `clock`:** UTC milliseconds plus the offset, through a civil-from-days conversion,
  the same arithmetic as `providers::iso_utc_secs`. `updated` uses `providers::iso_utc_secs`.

### Tests (`#[cfg(test)]` in the module)
- Round trip:
  - A note already in Chorus's form, with unknown keys and a MausBot entry, renders exactly as it
    was read.
  - A note with a `# comment` or blank lines in its front matter keeps its body byte for byte, and
    gets its front matter in Chorus's form.
  - Rendering what was read changes nothing.
- Errors: each malformed case gives its reason.
- Latest work: twice in a row gives one entry. After a hand-off entry, a new one goes on top.
- Bound: 25 entries keep 20, and move 5 in order. A 70 KiB note moves the oldest until it fits,
  and never moves the newest.
- Archive: an empty archive, an existing one, and a foreign file.
- Exclude: an empty file, a file with one line present, a file with both lines, and a file with no
  trailing newline (gets one).
- Entry: a plan with done, current and pending steps; no plan; detached; not a repository; a reply
  with newlines and a `## ` inside stays one line.
- Stamp: `stamp(1_792_713_600_000, 60)`, a negative offset across midnight, and a leap day.

## 3. IO

### 3.1 `src-tauri/src/handoff/note.rs`
- `note_path(folder) -> PathBuf`, `archive_path(folder) -> PathBuf`.
- `read(path) -> Result<Option<String>, String>`. A missing file reads `None`. A file over
  `MAX_READ_BYTES`, or not UTF-8, is an error with the reason.
- `write_atomic(path, text) -> Result<(), String>`:
  - creates `.chorus/`
  - writes `handoff.md.tmp`
  - flushes, then `std::fs::rename` over the target (Windows replaces an existing file)
  - deletes the temporary file on any error
- `ensure_excluded(folder) -> Result<Ignore, String>`:
  - `Ignore::NotARepo` when `git::exclude_file` is `None`.
  - Otherwise it reads the file (missing is empty). If `handoff::with_exclude` returns new text, it
    creates the `info` folder and writes it. Then `Ignore::Ignored`.

### 3.2 `git::exclude_file(dir: &Path) -> Result<Option<PathBuf>, String>`
- Runs `git -C <dir> rev-parse --git-path info/exclude` with the `QUICK` limit.
- A folder outside any repository returns `None`; that is git's "not a git repository" complaint.
  Any other failure is an error.
- A relative answer is joined to `dir`. The answer is the common directory's file, so it is
  shared by every worktree.

### 3.3 `src-tauri/src/handoff/refresh.rs`
`refresh(m: &SessionManager, project_id: u32, session_id: Option<u32>) -> Result<(), String>`:

1. **The project.** Get it. With no folder, or a folder that doesn't exist, return `Ok(())`: there
   is nothing to write.
2. **The current note.** Read it. If it doesn't parse, record the problem (§3.4), emit, and return
   `Ok(())` without writing (K4).
3. **The session.** `session_id`, or else `store::handoff::newest_session(project_id)`. With no
   session, record "No session in this project has finished a turn yet." and return.
4. **The facts:**
   - the session row's `name` and `agent` (its label from `raw/labels` naming, kept in Rust as
     `AgentKind::label()` if it exists, else the kebab name)
   - `store::progress::latest_plan`
   - `store::handoff::latest_prompt` and `latest_reply`
   - git from the session's `cwd`: `git::facts` for the branch, `git::rev(cwd, "HEAD")` cut to 7
     characters for the head, and `git::snapshot(cwd).changes.files` for the dirty count. A git
     error is the "not a repository" line.
5. **Scrub** every string with `vault::all_for_scrubbing` plus `builtin_patterns`. If a vault key
   can't be read, stop and record the reason, as `composer::save` does.
6. **Build:** `with_latest_work`, set `meta.updated` (UTC now) and `meta.project` (the project's
   current name), then `bound`. Moved entries go to the archive first.
7. **Write:** `ensure_excluded`, then `write_atomic`. Clear the project's problem, record its ignore
   state, and emit `HandoffChanged { project_id }`.

### 3.4 `src-tauri/src/handoff/mod.rs`
- **State:**
  - `static QUEUE: OnceLock<Mutex<Sender<(u32, u32)>>>`, holding (project, session).
  - `static STATUS: Mutex<HashMap<u32, Status>>`, where `Status { problem: Option<String>, ignore:
    Option<Ignore> }`.
- **`observe(s: &SessionSummary)`.** Never blocks and does no IO. It keeps
  `static LAST: Mutex<HashMap<u32, (Activity, bool)>>` (activity, closed). It queues
  `(project_id, s.id)` when all of these hold:
  - the session has a `project_id`
  - it isn't tagged `loop`
  - either its activity changed from `Working` to anything else, or its status became `Closed`
- **`start(m)`.** Spawns thread `handoff`:
  - It keeps `pending: HashMap<project, (session, due)>`, where `due = now + 3 s` and a later
    trigger pushes the time back.
  - Each pass, `recv_timeout` until the soonest `due`, then runs `refresh` for each project that is
    due. An error goes into `STATUS[project].problem`, and is also emitted.
  - It stops when `m.is_quitting()`.
- **Event:** `HandoffChanged { project_id: u32 }`. Derive like `run_loop::LoopUpdated`, and register
  it in `collect_events!` under `// p18`.
- **Commands** (`// p18` block, end of `collect_commands!`):
  - `handoff_view(project_id: u32) -> Result<HandoffView, String>`
  - `handoff_refresh(project_id: u32) -> Result<HandoffView, String>`: runs `refresh` now, on a
    blocking thread
  - `handoff_open_note(app: AppHandle, project_id: u32) -> Result<(), String>`:
    `app.opener().open_path(...)` as `documents.rs:209-211` does. A missing note is an error.

```rust
#[derive(Serialize, Type)] #[serde(rename_all = "camelCase")]
pub struct HandoffView {
    pub project_id: u32,
    pub has_folder: bool,
    pub note_path: Option<String>,                    // absolute, for the tooltip only
    pub exists: bool,
    pub problem: Option<String>,
    pub ignored: Option<IgnoreState>,                 // ignored | not-a-repo
    pub baton: Baton,
    pub updated: Option<String>,
    pub entries: Vec<EntryView>,                      // scrubbed; at most MAX_ENTRIES
}
pub struct EntryView { pub heading: String, pub author: Author, pub latest_work: bool,
                       pub done: Option<String>, pub state: Option<String>, pub open: Option<String>,
                       pub next: Option<String>, pub other: Vec<String> }
```

`handoff_view` reads the file fresh each time, so a hand edit shows at once, and scrubs every
string.

### 3.5 `src-tauri/src/store/handoff.rs`
- `latest_reply(store, session) -> Result<Option<(String, i64)>, String>`: `SELECT final_message,
  COALESCE(ended_at, started_at) FROM turns WHERE session_id = ?1 AND final_message IS NOT NULL
  AND final_message != '' ORDER BY id DESC LIMIT 1`.
- `latest_prompt(store, session) -> Result<Option<String>, String>`: the newest prompt's text for
  the session. Use the existing prompts table and its columns exactly; see `store/history.rs` for the
  query that `prompt_of_turn` uses.
- `newest_session(store, project) -> Result<Option<u32>, String>`: the session in the project
  with the newest turn, else the newest session.

## 4. Wiring
- **`lib.rs`:**
  - `mod handoff;`
  - In the notify closure, after `relay::sessions::observe(&session);`, add
    `handoff::observe(&session);` with the comment `// P18: the hand-off note's own subscription (it
    only queues).`
  - After `relay::start(&manager)?;`, add `handoff::start(&manager)?;`.
- **`ipc.rs`:** the `// p18: the hand-off note, the baton and branch hygiene.` block.

## 5. UI
- **`src/stores/handoff.ts`** (Pinia `useHandoffStore`):
  - `views: Map<number, HandoffView>` and `errors: Map<number, string>`
  - `load(projectId)` and `refresh(projectId)`
  - `init()` listens to `events.handoffChanged` and reloads that project if it is loaded
- **`ProjectHandoff.vue`** takes a `project: Project`. It renders `HandoffNote` (18-2 and 18-3 add
  their cards here). It loads on mount and when the project changes.
- **`HandoffNote.vue`:** a card, with theme tokens only.
  - Header: "Hand-off note", plus a muted line `Updated 3 min ago · ignored by git` (or `not a git
    repository`).
  - Buttons: Refresh and Open file. Open file is disabled with a reason when there is no note.
  - Body: the newest entry's heading, then Done, State, Open and Next as label and value rows.
  - "Show {n} earlier entries" expands the rest.
  - A problem shows as a warning line with the path in the tooltip.
  - With no folder: "Set a folder for this project to keep a hand-off note."
- **`OverviewView.vue`:** `<ProjectHandoff v-if="chosenProject" :project="chosenProject" />`, right
  after `</header>`, with the import. Nothing else changes.
- **`handoff.ts`:** pure helpers, with tests in `handoff.test.ts`.
  - `updatedLabel(iso, nowMs)`: "just now", "3 min ago", "2 h ago", or the date
  - `ignoreLabel(state)`
  - `entryRows(entry)`: the labelled rows that exist, in order

## 6. Verification in the app
1. Launch `p18` (overview §7).
2. **Scratch repository:** `git init` `%TEMP%\chorus-p18\note-repo`, with one commit. Make a project
   on that folder through the UI or `commands.projectSave`.
3. **First turn:** start a Claude Code session in the folder, with permissions auto. Send a tiny
   prompt, then Enter as a separate write. When the turn ends, within about 5 s:
   - `.chorus/handoff.md` exists, with front matter and one latest-work entry
   - the project page shows it
   - `git status --porcelain` in the repository prints nothing for `.chorus/`
   - `.git/info/exclude` has the two lines, and only them added
4. **Second turn:** send a second prompt. The latest-work entry is replaced; the file still has one
   entry.
5. **Broken note:** delete the closing `---` by hand. The card shows the reason. A third turn
   leaves the file's bytes unchanged (compare hashes). Restore the line, and the next refresh
   works.
6. **Themes:** screenshot the card in Glacier and Dusk.
7. Evidence goes in `_verify/p18-note-*`.
