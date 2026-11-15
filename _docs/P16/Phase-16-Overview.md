# Phase 16: Progress panel

Written 2026-11-15 at P16's kickoff. The roadmap is
[HANDOFF-P16-P17-progress-goals.md](../HANDOFF-P16-P17-progress-goals.md); the decisions are D115–D122
in [DECISIONS.md](../DECISIONS.md). Facts were re-checked against the code on 2026-11-15 at commit
`c5ee0ff` on branch `feature/progress-and-goals`. Re-check a line number before relying on it.

## 1. Goal

Friendly gets a panel pinned at its top that shows the whole session's plan: which steps and skills
are done, which is current and which come next, and roughly how long each took. Then the panel says
where the time went, Overview cards get a thin progress bar, and a skill far off its usual time gets a
quiet marker. A glance should be enough to see that something looks wrong.

P16 adds no new agent behaviour. It records what Chorus already receives, and shows it.

## 2. Tasks

Run them in order. Each ends seen working in the real app, and is committed before the next starts.

| Task | Title | Depends on | Files it owns (new files in bold) |
|---|---|---|---|
| [16-1](Tasks/Task-16-1.md) | The pinned panel | None | **`chorus-core/src/step_times.rs`**, `events.rs`, `codex_stream.rs`, `lib.rs`; **`src-tauri/src/store/progress.rs`**, **`sessions/progress.rs`**, `store/history.rs`, `store/mod.rs`, `events/recorder.rs`, `sessions/mod.rs`, `sessions/commands.rs`, `ipc.rs`; **`src/views/session/ProgressPanel.vue`**, **`progress.ts`**, **`progress.test.ts`**, `FriendlyTimeline.vue`, `conversation.test.ts`, `src/ipc/index.ts` |
| [16-2](Tasks/Task-16-2.md) | Where the time went | 16-1 | **`migrations/0026_activity_log.sql`**, **`store/activity_log.rs`**, **`chorus-core/src/time_split.rs`**, `activity.rs`, `recorder.rs`, `sessions/mod.rs`, `sessions/progress.rs`, `step_times.rs` (one field), `ProgressPanel.vue`, `progress.ts` |
| [16-3](Tasks/Task-16-3.md) | Overview card bar | 16-1 | `step_times.rs` (glance), `recorder.rs`, `sessions/mod.rs` (summary field and republish), `SessionCard.vue`, `progress.ts`, `src/stores/sessions.test.ts` (fixture) |
| [16-4](Tasks/Task-16-4.md) | Usual-time markers | 16-1, 16-2 (migration number) | **`migrations/0027_step_runs.sql`**, **`store/step_runs.rs`**, **`chorus-core/src/usual_time.rs`**, `recorder.rs` (turn close), `sessions/progress.rs`, `step_times.rs` (one field), `ProgressPanel.vue`, `progress.ts` |

Every task also regenerates `src/ipc/bindings.ts` with `npm run bindings`. Never edit it by hand.

Each task has a deeper spec in [ImplementationSpecs/](ImplementationSpecs/ImplementationSpec-16-1.md).

## 3. Gates (roadmap §4, briefly)

| Gate | Rule |
|---|---|
| G1 | Done means seen working in the real app. Compiled and unit-tested is not done |
| G2 | Rows come only from structured channels: the agent's checklist, skill calls and Chorus's own steps. Never screen text |
| G3 | Times come from Chorus's clock when each change arrives. A step that can't be timed says "not timed" |
| G4 | Theme tokens only, seen in Glacier and Dusk. The look is the approved mockup (D120) |
| G5 | Pure logic in `chorus-core` with unit tests: step times, the time split, the usual-time comparison |
| G6 | P17 only (the Stop-hook spike) |
| G7 | Before each commit: `cargo test`, `cargo clippy -- -D warnings`, `npm run typecheck`, `npm test`, the secret scan |

## 4. Decisions that govern P16

| Decision | In one line |
|---|---|
| D115 | A pinned panel for the whole session: status, skill chips, rounded durations from Chorus's clock, "not timed", skill time as an estimate, per-turn plan boxes kept |
| D116 | Then a time split, an Overview card bar and usual-time markers, in that order |
| D119 | The time split records each activity change in a new table; older sessions read "not split" |
| D120 | Under the current step, fold a summary of its tool calls with times, not checklist sub-items. The mockup is approved |
| D121 | Usual time: 5 timed runs, a marker under a third or over three times the median; free-text steps never; untimed never count |
| D122 | Codex: one progress row per plan update. `tool_target` reads `skill`. A skill is a chip on the step in progress, or its own row with no plan. No Overview bar without a plan |

**Choices these docs make** (K1–K10). Each is explained in its spec. Matthew accepted K1–K10 on
2026-11-15, with the changes to K6, K7 and K9 shown below; they are recorded as D123 in
[DECISIONS.md](../DECISIONS.md). Implementers don't edit DECISIONS.md for them.

| # | Choice |
|---|---|
| K1 | A step is matched across snapshots by its text (and its place among equal texts), not its id. TodoWrite and Codex items have no id, so `plan()` uses the index (`progress.rs:34`), which shifts when a step is inserted |
| K2 | A snapshot that shares no step text with the one before starts a new plan; earlier times are dropped. A step that goes back to pending after completed starts over |
| K3 | A skill called between steps goes to the next step that starts; with none, it is its own row. Its estimate ends at the first of: the next step starting, the next skill call, the end of its turn |
| K4 | A step's time is the sum of its in-progress intervals. An interval also ends when its run ends, so time while the agent or Chorus was down never counts |
| K5 | Duration wording lives in TypeScript (`progress.ts`), because the current step's clock ticks between reads. Every number comes from `chorus-core` |
| K6 | Time split: the newest run only, in four parts: agent working, waiting on you, waiting on approval and Idle (Matthew, 2026-11-15). A waiting stretch (Ready, Failed, or Unknown before the first event) counts as waiting on you for its first 15 minutes (`IDLE_AFTER_MS`) and as Idle after that. NeedsAnswer is always waiting on you, however long; AwaitingApproval is always waiting on approval. Nothing after exit counts |
| K7 | Overview: a live session's glance comes from the recorder's memory; a stopped one's from one bounded query. Cards with an event channel reserve the bar's height, so nothing jumps. "4 of 9 done" sits on the "Now:" line and the mockup's run time beside the bar is dropped, since the footer shows it (approved by Matthew, 2026-11-15) |
| K8 | Usual times are stored as they finish, in `step_runs` (migration 0027), written when a turn closes |
| K9 | Real-app checks run in an isolated dev instance, label `p16`, port 1436 (§7), so the new migrations stay out of the shared dev database (approved by Matthew, 2026-11-15) |
| K10 | No plan yet: the panel says so in one line. For opencode it says Chorus doesn't read its checklist yet (fail visible) |

## 5. The tree at kickoff (leave it alone)

`git status --short` on 2026-11-15:

```
 M _docs/DECISIONS.md
 M _docs/HANDOFF-P16-P17-progress-goals.md
?? Python/
```

The two doc edits are the coordinator's kickoff edits. `Python/` is untracked and of unknown origin.
Don't read into it, change it, delete it or commit it. Stage files by name (`git add <path>`), never
`git add -A` or `git add .`. Don't revert or commit anything outside your task's files.

## 6. Checks for every task

Run from the repo root, after the task's own tests:

```
npm run bindings
npm run check
git status --short
```

`npm run check` runs `npm run typecheck`, `npm test`, `cargo test --workspace`, `cargo clippy
--workspace --all-targets -- -D warnings` and the secret scan. `cargo test` also fails if the bindings
are stale (`ipc::tests::bindings_are_current`) or a migration isn't listed. `git status` should show
only the task's files, plus `?? Python/`. Files are LF (`.gitattributes`).

## 7. Running the real app

A build from this branch adds migrations 0026 and 0027. A database that has them is refused by an
older build ("newer Chorus"; the test `a_database_from_a_newer_chorus_is_refused`,
`store/mod.rs:204`). So `npm run dev`, which uses the shared `.dev` data folder, would leave
Matthew's dev database unusable on `main` until P16 merges. Use the isolated instance Matthew
approved on 2026-11-15 (K9), as [HANDOFF-COMMON.md](../HANDOFF-COMMON.md) §1 describes: label `p16`,
port 1436 (after P13's 1433 and P14's 1434). Don't use `npm run dev` for P16. Create
`_verify/tauri.p16.conf.json` (`_verify/` is gitignored):

```json
{
  "identifier": "com.contactestablished.chorus2.dev.p16",
  "build": {
    "beforeDevCommand": "cargo build -p chorus-hook && npx vite --port 1436 --strictPort",
    "devUrl": "http://localhost:1436"
  }
}
```

Run it with `npx tauri dev --config _verify/tauri.p16.conf.json`. Probe agents in a scratch folder
outside any repository. Evidence goes in `_verify/` and is never committed. Switch themes in
Settings, Appearance, or with the command palette's "Switch to Dusk". Claude Code is 2.1.316 here;
Codex is not on PATH, so Codex checks run only if `codex --version` works.

## 8. Exit criteria (roadmap §5)

- **16.1:** in a real Claude Code session with a multi-step plan and a skill call, the panel shows
  done, current and next steps with rounded durations, a skill chip with its time marked as an
  estimate, and "not timed" for a step that went straight to completed. It reads the same after
  Chorus restarts, and is seen in Glacier and Dusk. A Codex session shows its plan too, if Codex is
  installed.
- **16.2:** a real session that had a question and an approval shows its four totals (agent working,
  waiting on you, waiting on approval, Idle) adding up to its elapsed time, give or take rounding,
  and each wait matches what happened. A Ready stretch longer than 15 minutes shows its excess as
  Idle; an open question never does.
- **16.3:** Overview shows bars that move as steps complete, in both themes, and a card's layout
  doesn't jump when its bar appears.
- **16.4:** after 5 timed runs of one skill, a much shorter or longer run shows the marker, a name
  with fewer runs shows none, and both are seen in the app.

When all four pass, update the roadmap's P16 status. K1–K10 are already recorded (D123).
