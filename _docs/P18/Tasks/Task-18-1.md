# Task 18-1: Hand-off note

## Source Of Truth
- D128, and [HANDOFF-P18-mausbot.md](../../HANDOFF-P18-mausbot.md) §4.1.
- [Phase-18-Overview.md](../Phase-18-Overview.md) K1–K7.
- [ImplementationSpec-18-1.md](../ImplementationSpecs/ImplementationSpec-18-1.md).

## Initial Starting Point (verified 2026-11-18 at `b12ab12`)
- **Projects** have `folder: Option<String>`.
  - Store: `store/projects.rs:106` `get(store, id) -> Result<Option<ProjectRow>,String>`.
  - Front end: `stores/projects.ts` `byId`.
- **Session summaries** reach `lib.rs`'s notify closure (`lib.rs:91-96`), which already feeds two
  observers: `attention.observe` and `relay::sessions::observe`. The relay's observer only queues
  the summary for its own thread (`relay/sessions.rs:43-47`).
  - `SessionSummary` (`sessions/mod.rs:112`) has `project_id`, `activity`, `status`, `cwd`, `tags`
    and `name`.
- **Data for an entry:**
  - The plan: `store/progress.rs:110` `latest_plan(store, session_id)`.
  - The last reply: `turns.final_message`. Use it, not `history::latest`, which falls back to the
    prompt (`store/history.rs:279-291`).
  - Git: `git::facts` (`git.rs:95`), `git::rev` (`:144`) and `git::snapshot` (`:203`).
- **Local time:** `automations::offset_at(store, utc_ms) -> i32` (`automations.rs:156`).
  - Core time helpers exist: `providers::iso_utc_secs` (`chorus-core/src/providers.rs:466`), and
    `routing::time` for parsing.
- **Scrubbing:** `vault::all_for_scrubbing` (`vault/mod.rs:127`), plus
  `chorus_core::scrub::{builtin_patterns, scrub_all}`. Example at `attention.rs:147-151`.
- **Background work** is a named `std::thread` that loops while `!m.is_quitting()`. Example:
  `documents.rs:123-134`.
- **The Overview page for one project** is `OverviewView.vue` with `chosenProject` (`:80`) and
  `<header class="head">` (`:112`).

## Goal
Every project with a folder gets `.chorus/handoff.md`, which says what was done last, where things
stand and what's next. Chorus refreshes it after a session's turn ends or the session closes. Git
ignores it through `.git/info/exclude`. The project page shows it. A note Chorus can't read is shown
with the reason, and is never overwritten.

## Exact Scope
- **Create:**
  - `crates/chorus-core/src/handoff.rs`
  - `src-tauri/src/handoff/mod.rs`, `src-tauri/src/handoff/note.rs`, `src-tauri/src/handoff/refresh.rs`
  - `src-tauri/src/store/handoff.rs`
  - `src/views/handoff/ProjectHandoff.vue`, `src/views/handoff/HandoffNote.vue`
  - `src/views/handoff/handoff.ts`, `src/views/handoff/handoff.test.ts`
  - `src/stores/handoff.ts`
- **Edit:**
  - `crates/chorus-core/src/lib.rs`: one `pub mod`
  - `src-tauri/src/lib.rs`: `mod handoff;`, one `handoff::observe(&session)` in the notify closure, and
    one `handoff::start(&manager)?`
  - `src-tauri/src/store/mod.rs`: one `pub mod handoff;`
  - `src-tauri/src/git.rs`: add `exclude_file`
  - `src-tauri/src/ipc.rs`: a `// p18` block at the end of the commands and of the events
  - `src/views/OverviewView.vue`: one `<ProjectHandoff>` tag
  - `src/ipc/index.ts`: re-export the new types
  - `src/ipc/bindings.ts`: regenerated, never hand-edited

## Non-Goals
- The baton, MausBot and worktrees (18-2). Branch notices (18-3).
- An LLM summary. The entry is deterministic.
- Writing anywhere but `<project folder>/.chorus/` and the repository's `info/exclude`.
- Ignoring `/.chorus/` as a whole (K5).
- Do not revert or commit unrelated changes. `Python/` is not yours.

## Dependencies
18-0.

## Step-by-step Work
1. **Core:** write `handoff.rs` (spec §2): the parse and render round trip, the "latest work"
   entry, bounding and archiving, the exclude lines, and the time stamps. Unit tests first.
2. **Store:** write `store/handoff.rs` with `latest_reply(store, session_id)`,
   `latest_prompt(store, session_id)` and `newest_session(store, project_id)`.
3. **Git:** add `git::exclude_file(dir) -> Result<Option<PathBuf>,String>` (spec §3.2).
4. **Note IO:** write `handoff/note.rs` (spec §3.1): read with a 1 MiB cap, write atomically, append
   to the archive, keep the exclude file up to date.
5. **Refresh:** write `handoff/refresh.rs` (spec §3.3): gather the facts, scrub them, render, write.
   Keep a per-project problem map.
6. **Module:** write `handoff/mod.rs`:
   - `observe` sees a turn end or a session close, and queues the session.
   - The `handoff` worker thread debounces each project 3 s, then refreshes.
   - The commands, and the event `HandoffChanged`.
7. **Wire it** in `lib.rs` and `ipc.rs`, then regenerate the bindings.
8. **UI:** write the store, `ProjectHandoff.vue` and `HandoffNote.vue`, plus `handoff.ts` and its
   tests. Add the tag to `OverviewView.vue`.
9. **Real app:** `p18` instance, a scratch repository and a Claude Code session (spec §6).

## Test Expectations
- **Core:**
  - parse and render round-trip byte for byte, including unknown keys and other authors' entries
  - each kind of malformed front matter gives a reason
  - the latest-work entry is replaced, not stacked
  - a hand-off entry stops the replacing
  - bounding moves the oldest entries
  - the exclude lines are added only when missing, and keep the file's own lines
  - time stamps are right, including at a date line
  - one-line capping keeps whole characters
- **Store:** `latest_reply` ignores turns with no final message, and never returns a prompt.
- **Front end:** the labels in `handoff.ts`.

## Verification Commands
```
cargo test -p chorus-core handoff
cargo test -p chorus --lib handoff
npm run bindings
npm run check            # PowerShell
git status --short
```

## Acceptance Criteria
- In the real app:
  - a turn's end writes `.chorus/handoff.md` within about 5 s, with a latest-work entry
  - a second turn replaces that entry
  - `git status` in the scratch repository lists no note
  - the project page shows the note
- A note with an unclosed front matter keeps its exact bytes, and the project page shows the reason
  and the path.
- Sessions tagged `loop` never trigger a refresh.
- No text reaches the note unscrubbed.

## Review Checklist
- `observe` never blocks, and never does IO.
- The note is written by rename. A failed write leaves the old note.
- Only the two exclude lines are ever added. `info/exclude` keeps its own lines.
- An entry Chorus didn't write is never changed.
- No local path or username is committed, in code, tests or docs.
