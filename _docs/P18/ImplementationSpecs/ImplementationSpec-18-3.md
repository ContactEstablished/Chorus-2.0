# Implementation spec 18-3: Branch hygiene

## 1. Migration `0029_p18_project_notices.sql`

Copy the header directive exactly as `0011_p14_agent_check.sql` writes it.

```sql
-- chorus: foreign_keys off
-- P18 (K13): a notice can belong to a project instead of a session (branch hygiene, the baton).
CREATE TABLE notifications_p18 (
    id INTEGER PRIMARY KEY,
    session_id INTEGER REFERENCES sessions(id) ON DELETE CASCADE,
    project_id INTEGER REFERENCES projects(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    read_at INTEGER,
    dedupe_key TEXT NOT NULL UNIQUE,
    payload TEXT,
    CHECK (session_id IS NOT NULL OR project_id IS NOT NULL)
);
INSERT INTO notifications_p18 (id, session_id, kind, title, body, created_at, read_at, dedupe_key)
  SELECT id, session_id, kind, title, body, created_at, read_at, dedupe_key FROM notifications;
DROP TABLE notifications;
ALTER TABLE notifications_p18 RENAME TO notifications;
CREATE INDEX notifications_created ON notifications(created_at);

-- D131: Keep hides a stale branch for 30 days.
CREATE TABLE branch_snoozes (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    branch     TEXT    NOT NULL,
    until_at   INTEGER NOT NULL,
    PRIMARY KEY (project_id, branch)
) STRICT;
```

**The store test:** a database at 0028 with one session notice reaches the latest schema. The
notice keeps its id and columns, gains a NULL `project_id`, and `PRAGMA foreign_key_check` returns
nothing.

## 2. Core: `crates/chorus-core/src/branch_hygiene.rs`

```rust
pub const DAY_MS: i64 = 86_400_000;
pub const STALE_AFTER_MS: i64 = 30 * DAY_MS;
pub const KEEP_MS: i64 = 30 * DAY_MS;
pub const FIRST_SWEEP_MS: i64 = 2 * 60_000;
pub const SWEEP_EVERY_MS: i64 = DAY_MS;

pub struct BranchFacts { pub name: String, pub last_commit_ms: i64, pub merged: bool, pub ahead: u32,
                         pub checked_out: bool, pub in_use: bool }   // in_use: an open session's worktree or an active hand-off
pub struct Stale { pub name: String, pub days: u32, pub merged: bool, pub ahead: u32, pub last_commit_ms: i64 }

pub fn default_branch(origin_head: Option<&str>, exists: &dyn Fn(&str) -> bool, current: Option<&str>) -> Option<String>;
pub fn stale(branches: &[BranchFacts], default: Option<&str>, now_ms: i64, snoozed: &[(String, i64)]) -> Vec<Stale>;
pub fn stale_notice(project_id: u32, project: &str, base: &str, s: &Stale) -> crate::inbox::Notice;
pub enum HandoffEvent<'a> { Started { branch: &'a str }, Returned { branch: &'a str, commits: u32, by_bot: bool } }
pub fn handoff_notice(handoff_id: u32, project: &str, e: HandoffEvent) -> crate::inbox::Notice;
pub enum DeleteMode { Merged, Unmerged }
pub fn delete_mode(branch: &str, merged: bool, ahead: u32, confirmed: bool) -> Result<DeleteMode, String>;
pub fn valid_branch(name: &str) -> bool;
```

### Rules
- **`default_branch`:** `origin_head` with its `origin/` stripped, else `main` if it exists, else
  `master` if it exists, else `current`.
- **`stale`:** keep a branch when all of these hold:
  - it isn't the default
  - it isn't checked out
  - it isn't `in_use`
  - it isn't snoozed until after `now`
  - `now − last_commit_ms ≥ STALE_AFTER_MS`

  `days` is floored. The result is sorted oldest first.
- **`stale_notice`:**
  - kind `"branch"`
  - title `{name} unused for {days} days`
  - body `{project}: merged into {base}. Open the project to delete it or keep it.`, or
    `{project}: {ahead} commit(s) not merged into {base}. …`
  - dedupe key `branch:{project_id}:{name}:{last_commit_ms}`
- **`handoff_notice`:**
  - kind `"handoff"`
  - Started: title `{project} is with MausBot`, body `On {branch}. Chorus is watching for its
    return.`, key `handoff:{id}:start`
  - Returned: title `MausBot handed {project} back` (or `You took {project} back`), body `{commits}
    commit(s) on {branch}.`, key `handoff:{id}:return`
- **`delete_mode`:** merged gives `Merged`. Unmerged and unconfirmed is
  `Err("{ahead} commit(s) on {branch} aren't merged into the default branch. Delete them for
  good?")`. Unmerged and confirmed gives `Unmerged`.
- **`valid_branch`:** 1–255 bytes. No leading `-`, no `..`, no ASCII control characters, none of
  space `~^:?*[\`. It may not end with `/` or `.lock`, and may not be `@`.

### Tests
- The 29 and 31 day edges.
- Each skip reason.
- Snooze expiry.
- The default branch order.
- Notice keys and text.
- Delete modes.
- Name validation.

## 3. IO

### 3.1 `git.rs`
- `branches(dir) -> Result<Vec<(String, i64, String)>, String>`:
  `for-each-ref --format=%(refname:short)%00%(committerdate:unix)%00%(objectname:short) refs/heads`.
  Seconds are turned into milliseconds by the caller.
- `origin_head(dir) -> Result<Option<String>, String>`:
  `symbolic-ref --quiet --short refs/remotes/origin/HEAD`. Exit 1 gives `None`.
- `merged_into(dir, base) -> Result<Vec<String>, String>`:
  `branch --format=%(refname:short) --merged {base}`.
- `delete_branch(dir, branch, force: bool) -> Result<(), String>`: `branch -d|-D -- {branch}`,
  where git accepts `--`; otherwise validate first.
- `prune_worktrees(dir)`: `worktree prune`, for worktree folders that are already gone.

### 3.2 `src-tauri/src/handoff/branches.rs`
- **`report(m, project_id) -> Result<BranchReport, String>`:**
  1. Read the project folder's `git::facts`, then `branches`, `origin_head`, `default_branch` and
     `merged_into(default)`.
  2. `count_range(default, b)` for unmerged branches only.
  3. Checked out: from `git::worktrees(main_root)`.
  4. In use, either of:
     - a worktree row in `store::worktrees` whose `other_open_sessions(…, 0) > 0`
     - an active hand-off's branch
  5. Snoozes from `branch_snoozes`.
- **`BranchReport`:**
  - `{ problem: Option<String>, base: Option<String>, branches: Vec<BranchView> }`
  - `BranchView { name, last_commit_ms: f64, days: u32, merged, ahead, stale, snoozed_until_ms:
    Option<f64>, checked_out, in_use, worktree: Option<String>, maus: bool }`
  - It lists every stale-and-not-snoozed branch plus every `maus/…` branch, in that order.
- **`keep(m, project_id, branch)`:** upsert a snooze until `now + KEEP_MS`. Mark that branch's
  unread notices read.
- **`delete(m, project_id, branch, confirmed)`:**
  1. Refuse, each with a sentence, if:
     - `valid_branch` fails
     - the branch is the default, or checked out in the main checkout
     - it is in use
  2. Find its worktree from `git::worktrees`. If the folder is missing, `prune_worktrees`. If it is
     dirty (`git::is_clean` is false), refuse: "The worktree at {path} has uncommitted changes."
  3. Run `delete_mode`, then remove the clean worktree, then `delete_branch(force = Unmerged)`.
  4. Remove its snooze. Close a returned or active hand-off on that branch (`state = 'closed'`).
  5. Emit `HandoffChanged`.
- **`sweep(m, app)`:** for every usable project with a folder in a repository, deduplicated by
  `main_root` (the first project wins), run `report`. For each stale, unsnoozed branch, call
  `add_project_notice` with `stale_notice`; the dedupe key keeps it to once. A failing project is
  logged with `eprintln!` and skipped.
- **The thread `branch-sweep`:** sleep `FIRST_SWEEP_MS` in 1 s steps while not quitting, then
  sweep. Repeat every `SWEEP_EVERY_MS`.
- **Commands:**
  - `branch_report(project_id) -> BranchReport`
  - `branch_keep(project_id, branch) -> BranchReport`
  - `branch_delete(project_id, branch, confirmed: bool) -> BranchReport`
  - `branch_check_now(project_id) -> BranchReport`: runs that project's part of the sweep now.

### 3.3 `attention.rs`
```rust
pub fn add_project_notice(&self, store: &Store, project_id: u32, notice: &Notice) -> Result<(), String>
```
It scrubs exactly as `notify` does (`:147-151`), then `notifications::insert_for_project`. If a
row was inserted, it emits `NotificationAdded { notification, unread }`.

Baton calls go in `baton.rs`: on `start` after the row is inserted, and in `return_` after the
update.

## 4. UI
- **`BranchesCard.vue`,** on the project page after the note card.
  - Title "Branches", with Check now.
  - **Rows:** name (mono), then `45 days unused · merged into main` or `2 commits not merged`, then
    the worktree path muted, if any.
  - **Actions:** Keep 30 days and Delete.
  - **Unmerged Delete** turns the row into a confirm: the question from `delete_mode`'s error,
    then "Delete them" (danger) and Cancel.
  - **Empty:** "No branches unused for 30 days."
  - **Problems** show inline.
- **`ReturnCard.vue`:** when `merged`, a "Delete branch" button calls `branch_delete(…, false)`.
- **`NotificationCentre.vue`:**
  - `choose(n)`: `n.sessionId` opens the session, as today. `n.projectId` closes the panel and
    calls `view.openProject(n.projectId)`, or shows "That project has been deleted."
  - An item with a project shows the project's name in its meta line.
- **`stores/notifications.ts`:** no change beyond types.

## 5. Verification in the app (`p18`)
1. **Scratch repository** `%TEMP%\chorus-p18\branch-repo`, with a commit on `main`, and these
   branches:
   - `old-merged`: one commit with `GIT_AUTHOR_DATE` and `GIT_COMMITTER_DATE` set 45 days back,
     then merged into `main` with `--ff-only` from before main moved on, so it is merged.
   - `old-open`: two commits 60 days back, not merged.
   - `fresh`: one commit today.
2. **The project page:** Check now. Expect `old-merged` and `old-open` flagged, `fresh` not, two
   project notices in the bell, and a second Check now adds none.
3. **Keep on `old-open`:** it leaves the card. The DB has a snooze 30 days out.
4. **Delete on `old-merged`:** it is gone (`git branch --list`).
5. **Unmerged:** `branch_snooze` deleted by hand to bring `old-open` back, or a fresh `old-open2`.
   Delete asks with "2 commits". Confirming removes it.
6. **A worktree with a dirty file** on `old-wt`: Delete is refused with the reason. Cleaned, Delete
   removes the worktree folder and the branch.
7. **The baton:** Hand to MausBot gives the start notice. Take back gives the return notice.
   Merging the branch by hand into `main` gives Delete branch on the return card, which removes
   the branch and its worktree.
8. **A session notice** (an approval in a Claude session) still opens its session.
9. **Themes:** screenshots in Glacier and Dusk.
