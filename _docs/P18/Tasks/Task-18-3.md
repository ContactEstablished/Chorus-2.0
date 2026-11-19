# Task 18-3: Branch hygiene

## Source Of Truth
- D131, and [HANDOFF-P18-mausbot.md](../../HANDOFF-P18-mausbot.md) §4.3.
- [Phase-18-Overview.md](../Phase-18-Overview.md) K13–K15.
- [ImplementationSpec-18-3.md](../ImplementationSpecs/ImplementationSpec-18-3.md).

## Initial Starting Point (verified 2026-11-18)
- **Notices are tied to a session.**
  - `notifications.session_id INTEGER NOT NULL` (`0019_p7_notifications.sql:4`), and
    `dedupe_key TEXT NOT NULL UNIQUE` (`:10`).
  - Rust: `Notification { session_id: u32, … }` (`store/notifications.rs:12-22`) and
    `insert(store, session, notice, now)` (`:39`, INSERT OR IGNORE).
  - Core: `inbox::Notice { kind: &'static str, title, body, dedupe_key }` (`chorus-core/src/inbox.rs:52-60`).
  - The event: `attention.rs` scrubs the notice, inserts it and emits `NotificationAdded` (`:147-155`).
  - Notices are pruned after 30 days (`attention.rs:76`, `NOTICE_KEEP_MS`).
- **A table rebuild has a precedent:** `0011_p14_agent_check.sql`, with its `-- chorus:
  foreign_keys off` header, honoured at `store/mod.rs:104-106` and `:115-117`.
- **The notification centre** (`src/shell/NotificationCentre.vue`): `choose(n)` opens
  `n.sessionId` (`:37-46`). Items have no action buttons.
- **Git:**
  - Only `git branch -d` runs today (`git.rs:121-124`).
  - `git::worktrees(root)` (`:186`) lists each worktree's path and branch.
  - `remove_worktree` (`:129`) has no `--force`.
  - Nothing lists branches, dates or merged state.
- **Worktrees in use:** a session worktree counts while a non-closed session holds it
  (`store/worktrees.rs:73-81`).
- **18-2 is in:** `handoffs`, `git::count_range` and `git::is_ancestor`.

## Goal
Branches don't pile up:
- **The baton** posts a notice naming MausBot's branch.
- **Its return** posts one saying how many commits came back.
- **A daily sweep** flags each local branch with no commit for 30 days, once, in the bell and on
  the project's Branches card. There, Keep snoozes it for 30 days, and Delete removes it, and its
  clean worktree.
- **An unmerged branch** needs a second confirmation that names the commits that will be lost.

## Exact Scope
- **Create:**
  - `src-tauri/migrations/0029_p18_project_notices.sql`
  - `crates/chorus-core/src/branch_hygiene.rs`
  - `src-tauri/src/handoff/branches.rs`
  - `src/views/handoff/BranchesCard.vue`
- **Edit:**
  - `crates/chorus-core/src/lib.rs`
  - `src-tauri/src/store/mod.rs` (the list)
  - `src-tauri/src/store/notifications.rs` (nullable session, project and payload)
  - `src-tauri/src/attention.rs`: one `pub fn add_project_notice`
  - `src-tauri/src/git.rs`: `branches`, `origin_head`, `merged_into`, `delete_branch` (forced
    variant) and `prune_worktrees`
  - `src-tauri/src/handoff/mod.rs` and `baton.rs`: the start and return notices, the sweep thread,
    and the commands
  - `src-tauri/src/lib.rs`: pass the app handle to `handoff::start`
  - `src-tauri/src/ipc.rs`
  - `src/shell/NotificationCentre.vue` and `src/stores/notifications.ts`: a project notice opens
    the project
  - `src/views/handoff/ProjectHandoff.vue`, `ReturnCard.vue` (Delete branch when merged),
    `handoff.ts` and `handoff.test.ts`
  - `src/ipc/index.ts`, and `src/ipc/bindings.ts` (regenerated)

## Non-Goals
- Remote branches. Pruning remotes.
- Deleting anything without a click.
- Notification-centre action buttons: the actions live on the project page (K13).
- Do not revert or commit unrelated changes.

## Dependencies
18-2.

## Step-by-step Work
1. **Migration** 0029 (spec §1): rebuild `notifications`, and add `branch_snoozes`. Add a store test
   that a P7 notice survives the rebuild.
2. **Store and attention:** `Notification` gains `session_id: Option<u32>`, `project_id:
   Option<u32>` and `payload: Option<String>`. Add `insert_for_project`, and
   `Attention::add_project_notice`.
3. **Core** `branch_hygiene.rs` (spec §2), tests first.
4. **Git helpers** (spec §3.1).
5. **`handoff/branches.rs`:** the report, keep, delete (with the worktree first), the sweep and the
   notices (spec §3.2).
6. **Baton notices** in `baton.rs` start and return (spec §3.3).
7. **UI:**
   - the Branches card
   - Delete branch on the return card
   - project notices in the centre: a muted "project" line, and a click opens the project
8. **Real app** (spec §5).

## Test Expectations
- **Core:**
  - the stale rule at 29 and 31 days
  - default, checked-out, in-use, active hand-off and snoozed branches are skipped
  - the default branch order
  - the notice dedupe key includes the last commit
  - the delete mode: merged; unmerged unconfirmed refused, with the commit count; unmerged confirmed
  - branch-name validation rejects `-x`, `a..b`, control characters and over 255 bytes
- **Store:** the rebuild keeps rows. A project notice needs a project or a session. Deleting a
  project deletes its notices.
- **Front end:** the card's labels in `handoff.ts`.

## Verification Commands
```
cargo test -p chorus-core branch_hygiene
cargo test -p chorus --lib store::tests
cargo test -p chorus --lib handoff
npm run bindings
npm run check            # PowerShell
git status --short
```

## Acceptance Criteria
- **In a scratch repository,** with `old-merged` (merged, last commit 45 days ago) and
  `old-open` (two unmerged commits, 60 days ago):
  - "Check now" flags both, once each.
  - The bell shows two project notices, and a click opens the project.
  - Running the check again adds none.
- **Keep** on `old-open` hides it, and stores a snooze until 30 days later.
- **Delete** on `old-merged` removes it.
- **Delete** on `old-open` asks again, naming "2 commits". A second confirm removes it.
- **Delete on a branch with a dirty worktree** is refused, with the reason.
- **The baton:** Hand to MausBot posts "… is with MausBot on maus/…". Its return posts the commit
  count. A merged `maus/…` branch gets Delete branch on the return card, which also removes its
  worktree.
- **A P7 session notice** still opens its session.

## Review Checklist
- The migration copies every column and keeps the ids. `PRAGMA foreign_key_check` is clean after
  it (the test covers this).
- Branch names reaching git are validated, and passed after `--` where git allows.
- Delete never uses `--force` on a worktree. `branch -D` only after the confirmation flag.
- The sweep never runs git on the UI thread. A failure in one project doesn't stop the others.
