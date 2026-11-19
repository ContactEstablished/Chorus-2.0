# Task 18-2: Baton and MausBot worktree

## Source Of Truth
- D129 and D130, and [HANDOFF-P18-mausbot.md](../../HANDOFF-P18-mausbot.md) §4.2.
- [Phase-18-Overview.md](../Phase-18-Overview.md) K8–K12.
- [spikes/P18.md](../../spikes/P18.md) S1.
- [ImplementationSpec-18-2.md](../ImplementationSpecs/ImplementationSpec-18-2.md).

## Initial Starting Point (verified 2026-11-18)
- **18-1 is in:** the note module, the `handoff` worker thread, `HandoffView`, `HandoffChanged`
  and the project card.
- **Git:**
  - `git::add_worktree(main_root, path, branch, base)` (`git.rs:106`) runs
    `worktree add -b <branch> <path> <base>`.
  - `git::facts` gives `top_level`, `branch` and `main_root`. `git::rev` (`:144`).
  - No range log or numstat helper exists.
- **Worktree placement:** the core planner `worktree::plan` puts session worktrees at
  `<parent>\.chorus2\<slug>\<id>` (`chorus-core/src/worktree.rs:73-86`); its `slug` is at `:45-56`.
  It can't name `maus/…` branches.
- **Vault:**
  - `vault::add(store, label, secret)` (`vault/mod.rs:112`) and `vault::decrypt(store, id)` (`:119`).
  - `credentials.label` is UNIQUE (`0001_initial.sql:13`).
  - The Telegram token's label is `"Telegram bot"` (`relay/mod.rs:29`).
- **Settings:**
  - Sections are listed in `src/views/settings/sections.ts:5-14`.
  - The union is in `src/stores/view.ts:7`.
  - Each section is rendered in `SettingsView.vue:35-42`.
- **The composer:** `useComposerStore().addSuggestion(id, text)` appends to a session's draft and
  never sends (`src/stores/composer.ts:137-146`).
- **The rail's project menu:** `src/shell/Rail.vue:255-270`.
- **reqwest** 0.13 (json, native-tls). The provider client pattern is in `providers/mod.rs:13-62`.
- **MausBot 0.1.169:** `GET /api/bots`, `POST /api/bots/<id>/tasks` (`cwd` needs admin),
  `POST /api/bots/<id>/messages`, `POST /api/auth/pair` and `GET /api/health` (spikes §S1).

## Goal
**Hand to MausBot** makes a `maus/<date>-<slug>` worktree from the project's checked-out branch. It
writes the hand-off entry to the project's note and to the worktree's copy, then starts the
conversation in MausBot, or hands Matthew the message to start it himself.

While MausBot has the baton, the project shows a banner.

The baton comes back in either of two ways:
- the worktree note's `baton:` line changes, within 5 s
- Matthew clicks Take back

The return card then shows the entries, commits and files, with Continue from here.

## Exact Scope
- **Create:**
  - `src-tauri/migrations/0028_p18_handoffs.sql`
  - `crates/chorus-core/src/maus.rs`
  - `src-tauri/src/handoff/baton.rs`, `src-tauri/src/handoff/maus_client.rs`
  - `src/views/handoff/BatonBanner.vue`, `src/views/handoff/ReturnCard.vue`,
    `src/views/handoff/HandToMausBot.vue`
  - `src/views/settings/MausBotSettings.vue`
- **Edit:**
  - `crates/chorus-core/src/lib.rs`: one line
  - `crates/chorus-core/secret-patterns.json`: append one entry
  - `src-tauri/src/store/mod.rs`: the migration list
  - `src-tauri/src/store/handoff.rs`: the `handoffs` queries
  - `src-tauri/src/handoff/mod.rs`: the watch, the commands and the view fields
  - `src-tauri/src/git.rs`: `log_range`, `numstat`, `is_ancestor` and `count_range`
  - `src-tauri/src/ipc.rs`: the p18 block
  - `src/views/handoff/ProjectHandoff.vue`, `src/views/handoff/handoff.ts`,
    `src/views/handoff/handoff.test.ts`, `src/stores/handoff.ts`
  - `src/views/settings/sections.ts`, `src/views/SettingsView.vue`, `src/stores/view.ts`: one
    member each
  - `src/shell/Rail.vue`: one menu item
  - `src/ipc/index.ts`, and `src/ipc/bindings.ts` (regenerated)

## Non-Goals
- Merging, rebasing or pushing anything.
- Deleting branches or worktrees (18-3).
- An MCP server (18-4, deferred).
- Reading MausBot's files: use the API only.
- Changing a MausBot bot's own settings or folder.
- Do not revert or commit unrelated changes.

## Dependencies
18-1.

## Step-by-step Work
1. **Migration** 0028 (spec §1), listed in `MIGRATION_FILES`.
2. **Core** `maus.rs` (spec §2), unit tests first:
   - the bot list parser
   - the default bot
   - the branch and folder plan
   - the hand-off entry, the message, the task title and the send id
   - the new entries since the hand-off
   - the from-git entry
   - the continue text
3. **Git helpers** (spec §3.1).
4. **MausBot client** (spec §3.2):
   - health, bots, pair, the token check, create a conversation, send
   - fixed error words, and a token that never appears in an error
5. **Baton** (spec §3.3): start, the watch pass, return, take back, dismiss and continue. Then the
   commands and the view (spec §3.4).
6. **Scrubber:** append the `omb_sess_` pattern to `secret-patterns.json` (spec §3.5).
7. **UI:** the settings section, the Hand to MausBot panel, the banner, the return card and the
   rail menu item (spec §4).
8. **Real app:** the manual path end to end, plus the refusals (spec §5). The automatic path waits
   for the pairing.

## Test Expectations
- **Core:**
  - the bot list from a fixture with the observed field types (`busy` bool, `updatedAt` number,
    `cwd` string or missing)
  - default bot order
  - slug and collision suffixes
  - the message has the folder, the branch and the rules
  - new entries stop at the hand-off heading
  - from-git caps commits and files
  - the send id fits `^[A-Za-z0-9_-]{16,80}$`
- **IO:**
  - `store::handoff` refuses a second active hand-off for a project
  - a worktree note that doesn't parse leaves the hand-off active, with the problem shown
- **Front end:** the banner wording, and the return summary helpers in `handoff.ts`.
- **No literal `omb_sess_` token** may appear in a source file. Tests build one at run time, or the
  secret scan fails.

## Verification Commands
```
cargo test -p chorus-core maus
cargo test -p chorus --lib handoff
npm run bindings
npm run check            # PowerShell
git status --short
```

## Acceptance Criteria
- **Hand to MausBot, without a token:**
  - In a scratch repository project, it makes `maus/<today>-<slug>` at
    `<parent>\.chorus2\<repo>\maus-<today>-<slug>`.
  - Both notes get the hand-off entry, with `baton: mausbot`.
  - The banner shows the folder and the message to copy, with the reason.
- **A commit, then `baton: chorus` in the worktree's note,** gives a return card within 5 s, with:
  - the commit
  - the files with +/−
  - the entries, or "from git"
  - "not merged yet"
- **Continue from here** puts the summary in the chosen session's composer, unsent.
- **Take back** returns the baton the same way.
- **Refusals,** each with its reason:
  - a second Hand to MausBot while one is active
  - a project folder that isn't a repository
  - a broken project note
- **With MausBot running** but no pairing, the panel lists the bots, read with no token.

## Review Checklist
- The token is read with `vault::decrypt` just before each authenticated call, and never cached
  or logged. Errors carry no token, and no URL with a token.
- Every MausBot reply, bot name and entry is scrubbed before it is shown or stored.
- Bot ids and thread ids are checked against `^[\w-]{1,128}$` before they go into a URL path.
- The worktree is made from a commit (the base's sha). The project's uncommitted files are named in
  the panel, as staying behind.
- No local path or username is committed.
