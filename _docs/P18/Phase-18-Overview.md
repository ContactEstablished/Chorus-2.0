# Phase 18: Working alongside OpenMausBot

Written 2026-11-18 at P18's kickoff. The roadmap is
[HANDOFF-P18-mausbot.md](../HANDOFF-P18-mausbot.md); the decisions are D128–D132 in
[DECISIONS.md](../DECISIONS.md). The spike results are in [spikes/P18.md](../spikes/P18.md).

Facts were checked against the code on 2026-11-18, on branch `feature/mausbot-handoff` from `main` at
`b12ab12`, and against the OpenMausBot desktop app 0.1.169. Re-check a line number before relying on
it.

## 1. Goal

Chorus and OpenMausBot (MausBot) hand work to each other without either swallowing the other.

- Chorus keeps a **hand-off note** in each project folder.
- **Hand to MausBot** gives MausBot its own branch, in a worktree Chorus makes, and starts the hand-off.
- When the baton comes back, Chorus shows what MausBot did, and Matthew carries on from there.
- **Branches don't pile up:** Chorus names MausBot's branch, and flags any branch unused for 30 days.
  Deleting a branch is always Matthew's click.
- The **Telegram relay is switched off,** because the phone route is now MausBot.

MausBot reading and prompting Chorus sessions (18.4, an MCP server) is deferred (D132). No task
builds it.

## 2. Tasks

Run them in order. Each ends seen working in the real app, and is committed before the next starts.

| Task | Title | Depends on | Files it owns (new files in bold) |
|---|---|---|---|
| [18-0](Tasks/Task-18-0.md) | Spikes | None | **`_docs/spikes/P18.md`**, `_docs/spikes/README.md` (done at kickoff; live runs wait for a MausBot token) |
| [18-1](Tasks/Task-18-1.md) | Hand-off note | 18-0 | **`chorus-core/src/handoff.rs`**, `chorus-core/src/lib.rs`; **`src-tauri/src/handoff/mod.rs`**, **`handoff/note.rs`**, **`handoff/refresh.rs`**, **`src-tauri/src/store/handoff.rs`**, `store/mod.rs` (one `mod` line), `git.rs` (one function), `lib.rs` (start and observe), `ipc.rs`; **`src/views/handoff/ProjectHandoff.vue`**, **`HandoffNote.vue`**, **`handoff.ts`**, **`handoff.test.ts`**, **`src/stores/handoff.ts`**, `OverviewView.vue` (one tag), `src/ipc/index.ts` |
| [18-2](Tasks/Task-18-2.md) | Baton and MausBot worktree | 18-1 | **`migrations/0028_p18_handoffs.sql`**, `store/mod.rs`, `store/handoff.rs`; **`chorus-core/src/maus.rs`**; **`handoff/baton.rs`**, **`handoff/maus_client.rs`**, `handoff/mod.rs`, `git.rs` (range log and numstat); `crates/chorus-core/secret-patterns.json` (one entry); **`src/views/handoff/BatonBanner.vue`**, **`ReturnCard.vue`**, **`HandToMausBot.vue`**, **`src/views/settings/MausBotSettings.vue`**, `sections.ts`, `SettingsView.vue`, `src/stores/view.ts` (one union member), `Rail.vue` (one menu item) |
| [18-3](Tasks/Task-18-3.md) | Branch hygiene | 18-2 | **`migrations/0029_p18_project_notices.sql`**, `store/notifications.rs`, `attention.rs` (one function); **`chorus-core/src/branch_hygiene.rs`**; **`handoff/branches.rs`**, `git.rs` (branch listing and forced delete); `NotificationCentre.vue`, `src/stores/notifications.ts`; **`src/views/handoff/BranchesCard.vue`** |
| [18-5](Tasks/Task-18-5.md) | Telegram off | 18-3 (migration number) | **`migrations/0030_p18_telegram_off.sql`**, `store/mod.rs`, `PhoneSettings.vue`; docs: `DECISIONS.md` (D16 status), `HANDOFF-P15-loop.md` (a note at the top), `PLAN.md` (P13 and P15 rows) |

Every task that changes a command or event regenerates `src/ipc/bindings.ts`. Never edit it by
hand. Each task has a deeper spec in [ImplementationSpecs/](ImplementationSpecs/).

## 3. Gates

| Gate | Rule |
|---|---|
| G1 | Done means seen working in the real app. Compiled and unit-tested is not done |
| G2 | Pure logic in `chorus-core` with unit tests: note parsing and rendering, entries, branch and folder names, the bot list, the stale-branch rules, delete checks. `tests/purity.rs` enforces no IO |
| G3 | Fail visible: a broken note, a missing folder, MausBot not running, no token and a refused delete all say why on screen |
| G4 | Secrets: the MausBot token is a DPAPI credential, read at use, and sent only in an `Authorization` header to `127.0.0.1`. Never argv, a log, the note, an event or an export. The scrubber learns its shape |
| G5 | Chorus reads MausBot only through its API, never its files (plan §3) |
| G6 | Nothing is merged or deleted without Matthew's click. An unmerged branch needs a second confirmation |
| G7 | Theme tokens only, seen in Glacier and Dusk |
| G8 | Before each commit, `npm run check` from PowerShell passes (typecheck, vitest, cargo test, clippy `-D warnings`, secret scan) |

## 4. Decisions that govern P18

| Decision | In one line |
|---|---|
| D128 | The note is `.chorus/handoff.md` in the project folder, ignored through the repository's local `.git/info/exclude` |
| D129 | MausBot is the phone route. The Telegram relay is switched off, not removed. Chorus stays loopback-only |
| D130 | Chorus makes MausBot's branch: a worktree on `maus/…`. Chorus's sessions keep the main checkout |
| D131 | Branch hygiene: name MausBot's branch, flag branches unused for 30 days, delete only on a click, and double-confirm an unmerged one |
| D132 | P18 first pass is 18.0–18.3 and 18.5. 18.4 (MCP) is deferred. P15's remote channel becomes MausBot |

**Choices these docs make (K1–K17).** Each is explained in its spec. They are settled by Claude at
kickoff as implementation choices, recorded as D133 and told to Matthew.

| # | Choice |
|---|---|
| K1 | **What Chorus may edit in the note.** The front matter (`project`, `baton`, `branch`, `worktree`, `base`, `updated`) and its own entries. Every other entry, and any unknown front-matter key, is kept byte for byte |
| K2 | **One "latest work" entry.** It is refreshed in place while it is the newest. A new one starts after any other entry (a hand-off or a return) |
| K3 | **When the note refreshes.** After a session in the project ends a turn (its activity leaves Working) or closes, seen through the summary observer in `lib.rs`. Debounced 3 s per project, on a `handoff` worker thread, never on the recorder's thread. The project card also has a Refresh button |
| K4 | **A broken note is never overwritten.** The card shows the reason and the path, and refreshing resumes once it parses |
| K5 | **What git ignores.** `.git/info/exclude` (the common git directory's) gets `/.chorus/handoff.md` and `/.chorus/handoff-archive.md`, never `/.chorus/`, because P15 commits `.chorus/run/` on its run branches. In a folder that isn't a repository the note is still written, and the card says nothing needs ignoring |
| K6 | **The note's size.** At most 20 entries and 64 KiB. Older entries move to `.chorus/handoff-archive.md`, newest first |
| K7 | **Times.** Headings use local time (`2026-11-18 14:05`, from `automations::offset_at`); `updated:` is UTC (`2026-11-18T14:05:00Z`) |
| K8 | **MausBot works on its own copy of the note.** At hand-off Chorus writes `<worktree>/.chorus/handoff.md` and watches it every 5 s. On return, the entries above the hand-off entry are copied into the project's note. MausBot stays inside its folder, and the project's note can't be overwritten by the bot |
| K9 | **Where the baton lives.** A `handoffs` table (migration 0028), with at most one active per project (a partial unique index). The note mirrors it |
| K10 | **Branch and folder.** Branch `maus/<YYYY-MM-DD>-<slug>`, folder `<parent>\.chorus2\<repo-slug>\maus-<YYYY-MM-DD>-<slug>`, with `-2`, `-3` if taken. The slug comes from the optional task line, else `handoff`. Made from the project folder's checked-out branch. It is not a session's worktree, so it isn't in the `worktrees` table |
| K11 | **How Chorus reaches MausBot.** Its loopback API at `127.0.0.1:8799`. Reads need no token. Starting a conversation in the worktree and sending the hand-off message need a token (credential label `MausBot`; admin scope to pin the folder, spikes §S1). Chorus gets the token by redeeming a pairing code Matthew makes in MausBot (`POST /api/auth/pair`), or takes an `omb_sess_` token pasted in the same field. With no token, or on any failure, Matthew gets the folder and the message to copy, plus the reason. The bot picker defaults to the last bot used for the project, else the bot whose folder is the project's |
| K12 | **The return entry, and the return card.** If MausBot wrote no entry, Chorus writes "MausBot → Chorus (from git)" with its commits and changed files. The card lists commits (up to 20) and files with +/− (up to 200), in place of the plan's "Open in Raw", since Raw has no diff view to open |
| K13 | **Project-level notices.** Migration 0029 rebuilds `notifications` so `session_id` may be NULL, and adds `project_id` and `payload`. Clicking one opens the project page. Branch actions (Delete, Keep) live on the project page's Branches card, not in the notification centre |
| K14 | **The stale sweep.** Thread `branch-sweep`, 2 min after start, then every 24 h. It covers local branches only. Skipped: the default branch (`origin/HEAD`, else `main`, else `master`), the branch checked out in the main checkout or in any project's folder, branches checked out in worktrees Chorus didn't make, any branch an open session, an active hand-off (any project's) or a loop run that isn't done or stopped is using, and any branch snoozed in `branch_snoozes`. A branch in one of Chorus's own worktrees (`.chorus2`) is otherwise flagged, so MausBot's branches don't escape the sweep by staying checked out. A spell is flagged once: `branch_flags` keeps the notice's dedupe key (the last commit, and the end of a Keep that ran out), so a pruned notice isn't raised again. (Changed at 18-3, after two reviews) |
| K15 | **Delete.** Only a branch the card lists (stale, or MausBot's), and only while it still points at the commit the card showed (`update-ref -d` with that commit). A merged branch goes at once. An unmerged one is refused until a second confirmation names the commits, and one whose commits couldn't be counted is never deleted. Its worktree goes first, and only when `git status --ignored` shows nothing there that would be lost (ignored files such as `.env` count; Chorus's own copy of the note doesn't); otherwise the refusal names what's there. A worktree that holds a project's folder is never removed. (Changed at 18-3, after two reviews) |
| K16 | **Telegram off.** Migration 0030 sets `relay_config.enabled = 0` once. The switch stays in Settings → Phone, whose copy says why it's off (D129) |
| K17 | **Real-app checks** run in an isolated dev instance, label `p18`, Vite port 1438 and debug port 9438 (§7) |

**Waiting on Matthew (spikes §4):** how Chorus gets its MausBot token. Until then, 18.2 is built
and checked with the manual start, and the automatic start is checked only as far as the API
refuses without a token.

## 5. The tree at kickoff (leave it alone)

`git status --short` on 2026-11-18:

```
?? Python/
```

`Python/` is untracked and of unknown origin. Don't read into it, change it, delete it or commit it.
Stage files by name (`git add <path>`), never `git add -A` or `git add .`.

## 6. Checks for every task

Run from the repo root, after the task's own tests:

```
npm run bindings
npm run check            # from PowerShell; in Git Bash it can exit 1 silently (LESSONS L77)
git status --short
```

`npm run check` runs typecheck, vitest, `cargo test --workspace`, `cargo clippy --workspace
--all-targets -- -D warnings` and the secret scan. `cargo test` also fails if the bindings are
stale (`ipc::tests::bindings_are_current`) or a migration isn't listed
(`store::tests::migrations_are_all_listed`). Files are LF.

## 7. Running the real app

A build from this branch adds migrations 0028–0030, and an older build refuses a database that has
them. So never use `npm run dev`, which uses the shared `.dev` data folder. Use the isolated instance
(K17), as [HANDOFF-COMMON.md](../HANDOFF-COMMON.md) §1 describes. `_verify/tauri.p18.conf.json`
(`_verify/` is gitignored):

```json
{
  "identifier": "com.contactestablished.chorus2.dev.p18",
  "build": {
    "beforeDevCommand": "cargo build -p chorus-hook && npx vite --port 1438 --strictPort",
    "devUrl": "http://localhost:1438"
  }
}
```

Run it from PowerShell with
`$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --remote-debugging-port=9438'; npx tauri dev --config _verify/tauri.p18.conf.json`,
and drive it with `node _verify/p18-runtime.mjs <mode>`.

- **Don't edit Rust while it runs.** Editing Rust rebuilds and restarts the app, which ends every
  live run (LESSONS L78).
- **Scratch repositories** live under `%TEMP%\chorus-p18\`.
- **MausBot:** use only a bot named "Chorus test", and change nothing else in MausBot. Stop only the
  processes the check launched.
- **Evidence** goes in `_verify/`, never in a commit.

## 8. Exit criteria (roadmap §5)

- **18.0:** a written result for each spike (done: [spikes/P18.md](../spikes/P18.md)). The live S1
  and S3 runs follow the token.
- **18.1:**
  - In a scratch repository with a project, a Claude Code turn ends and the note appears, with a
    "latest work" entry.
  - A second turn refreshes that entry in place.
  - `git status` doesn't list the note.
  - A note with broken front matter shows why, and is left untouched.
- **18.2:**
  - Hand to MausBot makes `maus/<date>-<slug>` in its worktree, writes the hand-off entry to both
    notes, and shows the banner.
  - With no token, the card offers the folder and the message to copy, and says why.
  - A commit on the branch, followed by `baton: chorus` in the worktree's note, is seen within
    5 s. The return card then shows the commit, the files and the entry, and Continue from here
    fills the chosen session's composer.
  - Take back works the same way.
  - With a token, a real MausBot bot does the work. That run waits on Matthew.
- **18.3:**
  - A scratch branch backdated past 30 days is flagged once, in the bell, and on the project's
    Branches card.
  - Keep hides it for 30 days.
  - Delete removes a merged branch and its clean worktree.
  - An unmerged branch needs the second confirmation.
- **18.5:** Settings → Phone shows the relay off, with the reason. The docs say so.

When all pass, update the roadmap's P18 status and PLAN.md's P18 row.
