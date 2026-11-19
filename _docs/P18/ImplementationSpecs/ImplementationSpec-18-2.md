# Implementation spec 18-2: Baton and MausBot worktree

## 1. Migration `0028_p18_handoffs.sql`

```sql
-- P18 (D130): a project handed to OpenMausBot on its own branch, and back. The hand-off note
-- mirrors the active row; this table is the source of truth.
CREATE TABLE handoffs (
  id            INTEGER PRIMARY KEY,
  project_id    INTEGER NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
  state         TEXT    NOT NULL CHECK (state IN ('active', 'returned', 'closed')),
  task          TEXT    NOT NULL DEFAULT '',
  bot_id        TEXT,
  bot_name      TEXT,
  thread_id     TEXT,
  -- Why Chorus didn't start MausBot itself (no pairing, MausBot not running, a refusal).
  start_problem TEXT,
  branch        TEXT    NOT NULL,
  worktree      TEXT    NOT NULL,
  base_branch   TEXT,
  base_sha      TEXT    NOT NULL,
  started_at    INTEGER NOT NULL,
  returned_at   INTEGER,
  returned_by   TEXT CHECK (returned_by IN ('chorus', 'mausbot')),
  closed_at     INTEGER
) STRICT;

-- One hand-off at a time per project.
CREATE UNIQUE INDEX handoffs_one_active ON handoffs (project_id) WHERE state = 'active';
CREATE INDEX handoffs_project ON handoffs (project_id, id);
```

There is no MausBot settings table. The token is a credential, labelled `MausBot`. Its non-secret
facts (scopes, expiry, the MausBot session id) go in the existing `settings` key/value table, if
there is one; otherwise in a JSON file beside the database. Check `store` for a settings helper
(the prefs use one) and reuse it, under the key `mausbot.pairing`.

## 2. Core: `crates/chorus-core/src/maus.rs`

```rust
pub const API: &str = "http://127.0.0.1:8799";
pub const CREDENTIAL_LABEL: &str = "MausBot";

pub struct Bot { pub id: String, pub name: String, pub title: Option<String>,
                 pub cwd: Option<String>, pub busy: bool, pub conversations: Vec<Conversation> }
pub struct Conversation { pub thread_id: String, pub title: Option<String>, pub cwd: Option<String>,
                          pub busy: bool, pub updated_at_ms: Option<i64> }
pub fn parse_bots(v: &serde_json::Value) -> Result<Vec<Bot>, String>;
pub fn conversation<'a>(bots: &'a [Bot], thread_id: Option<&str>, folder: &str)
    -> Option<(&'a Bot, &'a Conversation)>;
pub fn default_bot(bots: &[Bot], project_folder: &str, last_bot: Option<&str>) -> Option<String>;
pub fn valid_id(id: &str) -> bool;                    // ^[\w-]{1,128}$
pub fn is_token(s: &str) -> bool;                     // omb_sess_ + 20..=200 of [A-Za-z0-9_-]
pub fn is_pairing_code(s: &str) -> bool;              // 12 [A-Z0-9] once '-' and spaces are dropped

pub struct Plan { pub branch: String, pub path: String }
pub fn slug(task: &str) -> String;                    // [a-z0-9-], ≤ 32, else "handoff"
pub fn plan(main_root: &str, date: &str, task: &str, taken: &dyn Fn(&str, &str) -> bool)
    -> Result<Plan, String>;

pub struct HandoffFacts<'a> { pub at_ms: i64, pub offset_minutes: i32, pub project: &'a str,
    pub branch: &'a str, pub worktree: &'a str, pub base: &'a str, pub task: &'a str,
    pub bot: Option<&'a str>, pub latest: Option<&'a crate::handoff::Fields> }
pub fn handoff_heading(at_ms: i64, offset_minutes: i32) -> String;   // "## {stamp} · Chorus → MausBot"
pub fn handoff_entry(f: &HandoffFacts) -> String;
pub fn message(f: &HandoffFacts) -> String;
pub fn task_title(project: &str, task: &str) -> String;              // ≤ 80 chars
pub fn send_id(handoff_id: u32, nonce_hex: &str) -> String;          // "chorus-handoff-{id}-{nonce}"

pub fn new_entries(worktree_note: &crate::handoff::Note, handoff_heading: &str) -> Vec<crate::handoff::Entry>;
pub struct Commit { pub sha: String, pub subject: String }
pub struct FileChange { pub path: String, pub added: Option<u32>, pub removed: Option<u32> }
pub fn from_git_entry(at_ms: i64, offset_minutes: i32, branch: &str, base: &str,
                      commits: &[Commit], more_commits: u32, files: &[FileChange], more_files: u32) -> String;
pub fn continue_text(project: &str, branch: &str, worktree: &str, ahead: u32, merged: bool,
                     entries: &[crate::handoff::Fields]) -> String;
```

### Rules
- **`parse_bots`** reads `{"bots":[…]}`:
  - **Required:** `id`, `name`.
  - **Optional:** `title`, `cwd`, `busy` (bool, default false), and `tasks` → `conversations`,
    each with `threadId` (required), `title`, `cwd`, `busy`, and `updatedAt` as milliseconds
    (number).
  - Bad entries are skipped. A missing `bots` is an error ("MausBot answered without a bot list").
- **`conversation`:** by `thread_id` when given, else the newest one whose `cwd` is the same folder
  (`projects::same_folder`).
- **`default_bot`:** `last_bot` if it is still in the list, else the first bot whose `cwd` is the
  project's folder, else `None`.
- **`plan`:**
  - Branch: `maus/{date}-{slug}`.
  - Path: the same parent as `worktree::plan` (`<parent of main_root>\.chorus2\<worktree::slug(repo
    name)>\maus-{date}-{slug}`).
  - If `taken(branch, path)`, try `-2` up to `-20`, then give up with "Too many hand-off branches
    today; delete some first".
- **`handoff_entry`:** under `handoff_heading`:
  - `Done: {latest.done or "See the entry below"}`
  - `State: {branch} from {base}, in {worktree}`
  - `Open: {task or latest.open or "Nothing named"}`
  - `Next: {task or latest.next or "Pick up from the entry below"}`
  - `Bot: {bot or "started by hand"}`
  - Every value goes through `handoff::one_line(…, 200)`.
- **`message`,** plain text:

```
You're picking up {project} from Chorus.

Work only in this folder: {worktree}
It's a git worktree on branch {branch}, made from {base}. Commit your work there. Don't merge,
rebase or push, and don't change any other checkout.

First read .chorus/handoff.md in that folder. Its newest entry says what was done, where things
stand and what's next.
{Task: {task}}

When you stop, add an entry at the top of .chorus/handoff.md, under the "# Hand-off note" text,
headed "## <date> <time> · MausBot → Chorus", with Done:, State:, Open: and Next: lines. Then
change "baton: mausbot" to "baton: chorus" in its front matter. Chorus watches that line.
```

- **`new_entries`:** the entries before the first entry whose heading line equals
  `handoff_heading`. If there is none, every entry before the first `→ MausBot` heading.
- **`from_git_entry`:** heading `## {stamp} · MausBot → Chorus (from git)`, then:
  - `Done: {n} commit(s) on {branch}: {subjects, "; "-joined, ≤ 5, "and N more"}`, or
    `No commits on {branch}`
  - `State: {files} file(s) changed against {base}: {paths ≤ 8, "and N more"}`
  - `Open: MausBot didn't write an entry`
  - `Next: Review {branch} and merge it when it's ready`

### Tests
- A bot list fixture with every field type seen, plus junk entries.
- `default_bot` order.
- Slug: a long name, punctuation, an empty task, and a non-ASCII task (`handoff`).
- Plan collisions.
- The entry and the message contain the folder, the branch and the base.
- `new_entries` with and without the bot's entry.
- From-git caps.
- Send id shape.
- `is_pairing_code("7KQ2-M9XD-H4TP")`.
- `is_token`, on a token built at run time.

## 3. IO

### 3.1 `git.rs` additions (the `QUICK` limit, `run` as the others use it)
- `log_range(dir, from, to, limit) -> Result<(Vec<(String, String)>, u32), String>`:
  `log --format=%h%x00%s {from}..{to}`. Returns the first `limit` commits and the count beyond.
- `numstat(dir, from, to) -> Result<Vec<(String, Option<u32>, Option<u32>)>, String>`:
  `diff --numstat {from}...{to}`. A `-` means binary: `None`.
- `is_ancestor(dir, a, b) -> Result<bool, String>`: `merge-base --is-ancestor a b`. Exit 1 is
  `false`; any other failure is an error.
- `count_range(dir, from, to) -> Result<u32, String>`: `rev-list --count {from}..{to}`.

Revisions come from Chorus, never from MausBot. Each is checked with `git rev-parse --verify`
through `git::rev` before use.

### 3.2 `src-tauri/src/handoff/maus_client.rs`
- **The client:** one `reqwest::Client` in a `OnceLock`, with a 10 s timeout and
  `redirect(Policy::none())`.
  - Calls block on `tauri::async_runtime::block_on`, as `relay/client.rs` does, so they must run
    on the handoff thread or a blocking command thread, never on the async runtime.
- **Calls**, all to `maus::API`:
  - `health() -> Result<String /*version*/, Failure>`: `GET /api/health`, with
    `app == "openmausbot"`.
  - `bots() -> Result<Vec<Bot>, Failure>`: `GET /api/bots`, no token.
  - `pair(code) -> Result<(String /*token*/, PairingFacts), Failure>`: `POST /api/auth/pair`,
    `{"code", "label": "Chorus"}`. `PairingFacts { session_id, scopes: Vec<String>,
    expires_at_ms: Option<i64> }`.
  - `check(token) -> Result<PairingFacts, Failure>`: `GET /api/auth/session` with the bearer.
    Read `scopes` and `expiresAt` if present. Any 2xx is "works".
  - `create_conversation(token, bot_id, cwd, title) -> Result<String /*threadId*/, Failure>`:
    `POST /api/bots/{id}/tasks`, `{"title", "cwd", "select": false}`.
  - `send(token, bot_id, thread_id, text, send_id) -> Result<(), Failure>`:
    `POST /api/bots/{id}/messages`, `{"text", "threadId", "sendId"}`.
- **`Failure`:**
  - `NotRunning`: refused, timed out or reset.
  - `Refused(String)`: MausBot's `error` field, cut to 200 characters and scrubbed.
  - `Other(String)`: fixed words.
  - Never format a reqwest error with its URL (`.without_url()`); the URL is constant anyway.
  - The token is only ever in `bearer_auth`.

### 3.3 `src-tauri/src/handoff/baton.rs`
- **`start(m, project_id, bot_id, task)`:**
  1. **Checks.** Any failure here returns an `Err`, and nothing is done:
     - the project and its folder
     - `git::facts(folder)` is a repository
     - no active hand-off (`store::handoff::active`)
     - the project note parses, or is missing
     - `bot_id`, if given, passes `valid_id`
     - `task` is at most 200 characters after trimming
  2. **Base:**
     - `base_branch` is `facts.branch`, or `None` when the head is detached.
     - `base_sha` is `git::rev(folder, "HEAD")`.
     - `today` is the local date from `handoff::stamp`'s date part.
  3. **Plan:**
     - `taken` is true when `git::rev(main_root, "refs/heads/{branch}")` is `Some`, or the path
       exists.
     - `git::add_worktree(main_root, path, branch, Some(&base_sha))`.
  4. **Insert** the `handoffs` row. If that fails, run `git worktree remove` and
     `git branch -d {branch}` (the branch was just made, so it is merged), then return the error.
  5. **The project note:** prepend `handoff_entry`. Front matter: `baton: mausbot`, `branch`,
     `worktree`, `base: {base_branch or "detached"} @ {sha7}`, `updated`. Then write it, with
     `ensure_excluded`.
  6. **The worktree's note:** the same rendered text to `<worktree>/.chorus/handoff.md`. The common
     exclude already covers it.
  7. **Start MausBot,** only if a token decrypts and a bot was chosen:
     - `create_conversation` with `cwd = worktree`, then `send(message)`.
     - Store `thread_id`, `bot_name` and `bot_id`.
     - On any failure, store `start_problem` (the reason). The hand-off stays active; the manual
       path applies.
     - With no token: `start_problem` is "Chorus isn't paired with MausBot (Settings → MausBot)".
     - With no bot: "No bot chosen".
  8. **Emit** `HandoffChanged`.
- **The watch pass,** called by the handoff thread every 5 s:
  - For each active hand-off, read `<worktree>/.chorus/handoff.md`.
    - A missing file or folder: record "The worktree's note is missing" against the hand-off, and
      keep it active.
    - A file that doesn't parse: record the reason, and keep it active.
    - `baton` not `mausbot`: `return_(by: mausbot)`.
  - Every 15 s, for active hand-offs, `bots()`. Find the conversation (by thread, else by folder),
    and keep `ACTIVITY[handoff_id] = { busy, updated_at_ms, problem }` in memory. Emit only when
    it changes.
- **`return_(m, handoff, by)`:**
  1. Read the worktree note. `entries = new_entries(note, heading)`, where the heading is
     recomputed from `started_at`. With no note, `entries` is empty.
  2. If no entry is the bot's (`Author::Mausbot`), add `from_git_entry`. It uses `log_range(base_sha,
     branch, 20)` and `numstat(base_sha, branch)`, capped at 200 files with the count beyond.
  3. **The project note:** prepend the scrubbed entries, and set `baton: chorus`, keeping
     `branch`, `worktree` and `base` until dismissed. Write it.
     - If the project note doesn't parse, skip the write and record the problem. The return still
       happens.
  4. Update the row: `state = 'returned'`, `returned_at`, `returned_by`. Emit.
- **`take_back(m, project_id)`:** `return_(by: chorus)` on the active hand-off. It is an error if
  there is none.
- **`dismiss(m, project_id)`:** sets the newest returned row to `closed`, and clears `branch`,
  `worktree` and `base` from the project note's front matter. Emit.
- **`continue_text(m, project_id) -> String`:** from the newest returned row: `ahead =
  count_range(base_sha, branch)`, `merged = is_ancestor(branch, base_branch)` (false with no base
  branch), and the returned entries' fields. Scrubbed.

### 3.4 Commands and view
```rust
mausbot_status() -> MausStatus
mausbot_pair(input: String) -> MausStatus      // a pairing code, or an omb_sess_ token
mausbot_forget() -> MausStatus                 // deletes the credential and the pairing facts
mausbot_bots(project_id: u32) -> BotsView
handoff_start(project_id: u32, bot_id: Option<String>, task: String) -> HandoffView
handoff_take_back(project_id: u32) -> HandoffView
handoff_dismiss(project_id: u32) -> HandoffView
handoff_continue_text(project_id: u32) -> String
```

- **`MausStatus`:**
  - `{ running: bool, version: Option<String>, problem: Option<String>, paired: bool,`
  - `scopes: Vec<String>, can_choose_folder: bool, expires_at_ms: Option<f64>,`
  - `token_problem: Option<String> }`
  - `can_choose_folder` means the scopes include `admin`.
- **`mausbot_pair`:**
  1. Trim the input.
  2. If `is_token`, `check(token)`.
  3. Else if `is_pairing_code`, `pair(code)`.
  4. Else refuse: "Enter the pairing code from MausBot (Settings → Phone & devices)".
  5. Store with `vault::add(store, "MausBot", token)`, after deleting an older `MausBot`
     credential by id, then save the facts.
  - The token never goes back to the front end.
- **`BotsView`:**
  - `{ running, problem: Option<String>, bots: Vec<BotView>, default_id: Option<String> }`
  - `BotView { id, name, title, busy, folder_matches: bool }`, scrubbed.
- **`HandoffView` gains:**
  - `active: Option<ActiveView>`:
    - `{ id, branch, worktree, base, task, bot_name, started_at_ms: f64, started_by_chorus: bool,`
    - `start_problem: Option<String>, message: Option<String>, note_problem: Option<String>,`
    - `activity: Option<ActivityView { busy, updated_at_ms: Option<f64>, problem: Option<String> }> }`
    - `message` is filled only when `start_problem` is set, for the copy button.
  - `returned: Option<ReturnView>`:
    - `{ id, branch, worktree, worktree_exists, base, returned_by, returned_at_ms: f64,`
    - `entries: Vec<EntryView>, from_git: bool, commits: Vec<CommitView>, more_commits: u32,`
    - `files: Vec<FileView>, more_files: u32, ahead: u32, merged: bool, problem: Option<String> }`
  - Timestamps are `f64`, as elsewhere in the bindings.

### 3.5 Scrubber
Append to `crates/chorus-core/secret-patterns.json`, as the last entry:
`{ "name": "openmausbot-session-token", "source": "omb_sess_[A-Za-z0-9_-]{43}" }`.

Check `scripts/secret-grep.mjs` reads the same file, and keep every committed file free of a
43-character example.

## 4. UI

### `MausBotSettings.vue`
Section `mausbot`, label "MausBot", icon `agents`. It goes after Phone in `sections.ts` and the
union.

- **Status line:** "MausBot 0.1.169 is running", or "MausBot isn't running (it answers on
  127.0.0.1:8799)".
- **Paired:**
  - "Paired · full access · until 17 May 2027", with Forget.
  - If not full access: "Chat-only access: Chorus can send the hand-off message but can't choose
    the folder, so each hand-off starts by hand."
- **Not paired:** a field "Pairing code from MausBot" plus Pair. The help line: "In MausBot,
  Settings → Phone & devices → make a pairing code, with full access if it asks, then enter it
  here. Chorus keeps the key it gets, encrypted; you can also paste an `omb_sess_…` token." The
  field is `type="password"`, `autocomplete="off"`, and is cleared after Pair.

### `HandToMausBot.vue`
Shown on the project page, from a "Hand to MausBot" button on the hand-off card, or from the rail
menu with the page opened and the panel expanded.

- **Bot:** a select from `mausbot_bots`, with "Start it by hand" as the last option. When MausBot
  isn't running: "MausBot isn't running, so Chorus will make the branch and give you the message
  to start it by hand."
- **Task:** optional, one line, at most 200 characters.
- **Facts:**
  - "From main @ 1a2b3c4."
  - "3 uncommitted files in your checkout stay with you." (from `git::snapshot`, when non-zero)
  - "Chorus isn't paired with MausBot, so you'll start it by hand." (when not paired)
- **Buttons:** Start and Cancel. Errors show inline.

### `BatonBanner.vue`
For an active hand-off.

- **The line:** "With MausBot on `maus/…` since 14:05" plus an activity phrase: "MausBot is working",
  "MausBot last worked 5 min ago", "MausBot isn't running" or "Chorus can't see the conversation".
- **Button:** Take back.
- **When `start_problem` is set:**
  - the reason
  - "Copy folder" and "Copy message", using `navigator.clipboard.writeText` (the composer already
    uses the clipboard; reuse its notice)
  - one line of steps: "In MausBot, start a new conversation with your bot in this folder, then
    paste the message."
- **A worktree-note problem** shows as a warning line.

### `ReturnCard.vue`
For the newest returned (not closed) hand-off.

- **Title:** "MausBot handed {project} back" or "You took {project} back", with the time.
- **The new entries,** as in `HandoffNote`. "From git" is marked with a muted badge.
- **Commits:** sha in mono, then the subject, with "and N more".
- **Files:** path, then +a −r (mono), with "and N more".
- **State:** "Not merged yet: 4 commits ahead of main" or "Merged into main".
- **Actions:**
  - **Continue from here** opens a small select of the project's open sessions, with the
    newest-active first. It calls `handoff_continue_text`, then `composer.addSuggestion(sessionId,
    text)`, then `view.openSession(sessionId)`. With no session: "Start a session in this project
    first."
  - **Dismiss.**

### Wiring
- **`ProjectHandoff.vue`** order: ReturnCard (if any), BatonBanner (if active), the hand-off note
  card with its new "Hand to MausBot" button (hidden while one is active), then HandToMausBot when
  opened.
- **`Rail.vue`:** one menu item, "Hand to MausBot…", after "Edit project…". It calls
  `view.openProject(id)` and sets `useHandoffStore().openHandTo = id`.

## 5. Verification in the app (`p18`)
1. **A scratch repository** `%TEMP%\chorus-p18\baton-repo` with two commits, and a project on it.
   Make one session in the project (a shell is enough), for Continue.
2. **Settings → MausBot** shows MausBot running 0.1.169, not paired. Entering a nonsense code is
   refused, with the reason.
3. **Hand to MausBot** with the task "Add a line", and the bot list shown. Choose "Start it by
   hand". Expect:
   - the branch `maus/{today}-add-a-line`, and the folder under `.chorus2\baton-repo\`
   - both notes with the entry and `baton: mausbot`
   - the banner with Copy message and Copy folder
4. **Simulate MausBot:** in the worktree, commit a change, then edit the worktree note: add a
   `## … · MausBot → Chorus` entry and set `baton: chorus`. Within 5 s:
   - the return card shows the commit, the file and the entry (not from git)
   - the project note has the entry, and `baton: chorus`
5. **Continue from here** into the session. Its composer holds the text, and nothing was sent.
6. **Dismiss.** The card goes, and the front matter's branch and worktree are cleared.
7. **Hand to MausBot again, then Take back** with no edits. The return card shows "from git", with
   no commits.
8. **Refusals:** a second Hand to MausBot while one is active; a project on a plain folder; a
   broken project note.
9. **Themes:** screenshots of the banner and the card, in Glacier and Dusk.
10. **Waits on the pairing:** the automatic start with "Chorus test" (ImplementationSpec-18-0).

## 6. Changes after the code review (2026-11-18)

A review of this slice before it was committed found these gaps; the code does the following, and
the sections above should be read with it.

- **`handoffs.returned_entries`.** Migration 0028 has this extra column: the scrubbed entries a
  return brought back, as a JSON array.
  - The return card and Continue from here read the entries from the row, not the project's note.
  - So MausBot's words survive a project folder that's gone, or a note that couldn't take them.
    The card then says why.
- **The return claims first.**
  - `set_returned` only updates an active row, and reports whether it did. The watch and Take back
    can't both return a hand-off; a second Take back says "MausBot handed it back already".
  - The entries are stored on the row, then added to the note.
- **One note writer at a time.** Every read-change-write of a project's note (the refresh, start,
  return and Dismiss) goes through `refresh::update_note`, under one lock. The refresh gathers its
  git facts before taking it.
- **Start writes the worktree's copy first,** then the project's note, inside that lock. On any
  failure the row, the worktree and its branch are removed, and the project's note is untouched.
  A new hand-off also puts away the project's older return cards.
- **`baton: chorus` exactly.** A blank or missing `baton:` reads as nobody, which is MausBot still
  working; only `chorus` returns it.
- **Entries after a changed heading.** `new_entries` falls back to the first `→ MausBot` heading,
  then to Chorus's own first entry, then to every entry. At most 20 come back.
- **Git that can't read the branch** gives an entry that says so (`from_git_failure_entry`), never
  "No commits". The card shows git's complaint.
- **Errors are shown:**
  - A failed return is on the banner, and tried again on the next pass.
  - Merged/ahead errors are on the card.
  - A vault problem shows on the card instead of failing it.
- **Timeouts are their own failure.** A conversation or message that timed out "may have been made
  anyway", so the banner says to check MausBot before starting or pasting again.
- **The listener is checked.** Before a pairing code or token is sent, `GET /api/health` must answer
  as OpenMausBot.
- **The task is scrubbed** before it reaches the note, the database or MausBot.
- **Small UI fixes:**
  - one event listener however many cards mount
  - a busy count per project
  - the panel closes when the project page is left, or once a hand-off starts
- **`connection.rs`** (new) holds the pairing, status and bot list, apart from `baton.rs`.
