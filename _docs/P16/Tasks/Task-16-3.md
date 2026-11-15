# Task 16-3: Overview card bar

## Source Of Truth

- D116, D120 (the card mockup is approved) and D122 (no bar without a plan) in
  [DECISIONS.md](../../DECISIONS.md); the roadmap's slice 16.3.
- [Phase-16-Overview.md](../Phase-16-Overview.md): gates, K7 and K9.
- [ImplementationSpec-16-3.md](../ImplementationSpecs/ImplementationSpec-16-3.md).
- The approved mockup `Card.dc.html` (outside the repo); spec §4 records its layout and colours.

## Initial Starting Point (verified 2026-11-15, commit `c5ee0ff`, before 16-1)

- `src-tauri/src/sessions/mod.rs:112` `SessionSummary` (derives `Serialize, Deserialize, Type`, :110)
  has `activity` (:149) and `run_started_at_secs` (:136) but no progress. `summary_of` (:744) already
  locks the live run's recorder once for `activity` (:757) and runs several small queries per session
  (`history::latest` at :756 and others).
- Summaries reach the front end through `publish` (:827) via `notify_id` (:822). The Claude hook path
  publishes only when the activity changed (:621–634), so a plan update during Working publishes
  nothing today. The Codex bridge publishes after every batch (`codex_bridge.rs:168`).
- `src-tauri/src/events/recorder.rs:47` keeps the newest plan in `plan`, set in `progress_at` (:87);
  a new run starts empty (:70).
- `src/views/SessionCard.vue` (265 lines): props at :16 (`now` in seconds); body clamped to three lines
  (:88, CSS :234–246, `flex: 1`); waiting line :89–91; footer :92–95 with "ran · updated" and the call
  to action. `OverviewView.vue` passes `now` from `useNow(1000)` (:26) into a CSS grid (:366–367).
- `src/stores/sessions.test.ts:77` builds a full `SessionSummary` literal; `src/shell/status.test.ts:6`
  and `src/views/session/stripSessions.test.ts:6` cast partial ones.
- After 16-1: `step_times::StepStatus`, `store::progress::latest_plan`, and the resume seed in
  `sessions/mod.rs`. Re-check them after 16-1 lands.

## Goal

Each Overview card for a session with a plan shows a thin bar: one segment per step (done, current,
pending) with "Now: <step> · 7 min" and "4 of 9 done" on one line above it. The mockup's run time
beside the bar is left out because the footer already shows it (approved by Matthew, 2026-11-15,
K7). It moves as steps complete, costs no extra query for a live session, and doesn't change the
card's height when it appears.

## Exact Scope

Edit:
- `crates/chorus-core/src/step_times.rs` (`ProgressGlance`, `glance`, `advance_current`, tests)
- `src-tauri/src/events/recorder.rs` (glance state, `plan_version`, `seed_glance`)
- `src-tauri/src/sessions/mod.rs` (`SessionSummary.progress`, `summary_of`, the hook-path republish,
  the run-start glance seed)
- `src/views/SessionCard.vue` (the bar), `src/views/session/progress.ts` and its test
- `src/stores/sessions.test.ts` (fixture field only)
- `src/ipc/bindings.ts` (generated), `src/ipc/index.ts` (re-export `ProgressGlance`)

Nothing else.

## Non-Goals

- Do not revert or commit unrelated changes, including `Python/` and the kickoff doc edits.
- No "no plan" label on cards (D122: hidden). No bar on the fleet strip, Sessions list or Inbox.
- No new command, and no per-card read from the front end.
- No change to the card's other content, its attention colours or the grid.
- No goal-mode behaviour (P17), no Usage screen (P11), no screen-text inference.

## Dependencies

Task 16-1 (`StepStatus`, `latest_plan`, the seed). Independent of 16-2.

## Step-by-step Work

1. Core: `ProgressGlance`, `glance(steps, current_since_ms)` and `advance_current(prev, steps, at_ms)`
   with tests (spec §2). Use 16-1's text matching for "same step".
2. Recorder: keep `shown` (the plan to show) and `current` (step text and since), update both in
   `progress_at`, bump `plan_version`; add `glance()`, `plan_version()` and `seed_glance()`.
3. `sessions/mod.rs`: add `pub progress: Option<ProgressGlance>` to `SessionSummary`. In
   `summary_of`, read the glance with the activity under the same recorder lock; for a session with
   no live run, one bounded `latest_plan` read. At run start, seed the glance from `latest_plan` for
   every run (and the merge plan only when resumed, as 16-1 does). In `receive_hook`, publish when the
   activity or `plan_version` changed.
4. `npm run bindings`; add `progress: null` to the `sessions.test.ts` fixture.
5. `progress.ts`: `cardLine(glance, now)` and `cardSegments(glance)`; `SessionCard.vue` per spec §4.
6. Run the checks, the real-app check, then commit.

## Test Expectations

- `step_times::tests`: `glance_counts_done_and_names_the_current_step`,
  `no_steps_give_no_glance`, `advance_current_keeps_the_start_while_the_same_step_runs`,
  `advance_current_restarts_when_another_step_starts`, `advance_current_clears_when_nothing_runs`.
- `src-tauri`: `recorder::tests::a_task_update_bumps_the_plan_version_without_an_activity_change`
  (TaskCreate then TaskUpdate PostToolUse hooks while Working: activity stays Working, `plan_version`
  rises twice, `glance()` done goes 0 then 1); `recorder::tests::a_seeded_glance_shows_until_a_new_plan`.
  The fixture in `sessions/tests.rs` drives `cmd.exe` shells with a no-op notifier and can't post a
  Claude hook, so the republish condition in `receive_hook` is proved in the real app (the bar moves
  within a second of the panel).
- Vitest: `cardLine` (current with time, no current with next, all done), `cardSegments` (one per step
  up to 24, a continuous bar above 24).

## Verification Commands

```
cargo test -p chorus-core -- step_times::
cargo test -p chorus -- recorder::
npx vitest run src/views/session/progress.test.ts
npm run bindings
npm run check
git status --short
```

## Acceptance Criteria

- [ ] All commands pass; `git status` shows only the scoped files and `?? Python/`.
- [ ] In the isolated instance (Overview §7), Overview with two Claude Code sessions, one working
  through a 4-step task list and one with no plan: the first card's bar moves as each step completes,
  within a second of the panel; "Now" names the current step and its time ticks; the second card
  shows no bar.
- [ ] The card's height doesn't change when its first plan arrives (compare screenshots in
  `_verify/`, or watch the grid: no row moves).
- [ ] A stopped session with a plan keeps its bar, without a ticking time.
- [ ] Seen in Glacier and Dusk, matching the card mockup.

## Review Checklist

- A live session's glance comes from memory under the lock already taken for `activity`; a stopped
  session costs one bounded query; nothing per card from the front end.
- The hook path republishes on a plan change, not on every hook.
- The bar is hidden with no plan; the reserved slot keeps cards from jumping.
- `ProgressGlance` derives `Deserialize` (the summary does); names don't collide in the bindings.
- Colours are tokens; nothing in `tokens.css` changed.
