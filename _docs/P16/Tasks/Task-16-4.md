# Task 16-4: Usual-time markers

## Source Of Truth

- D116 and D121 in [DECISIONS.md](../../DECISIONS.md); the roadmap's slice 16.4 ("a quiet marker …
  no notification and no alarm styling").
- [Phase-16-Overview.md](../Phase-16-Overview.md): gates, K3, K8 and K9.
- [ImplementationSpec-16-4.md](../ImplementationSpecs/ImplementationSpec-16-4.md).
- The approved mockup's marker chip ("Last time 40 sec · usually ~6 min", `Main.dc.html`, outside
  the repo); spec §5 records its style.

## Initial Starting Point (verified 2026-11-15, commit `c5ee0ff`, before 16-1)

- Nothing stores a step's or skill's duration across sessions. `tool_calls` (`0005_history.sql`) has
  `tool`, `target`, `started_at`, `ended_at`, with an index on `turn_id` only (:45), so "every run of
  skill X" can't be read cheaply from it.
- A `Skill` call's target is empty today (`events.rs:58`); 16-1 makes it the skill's name.
- `src-tauri/src/events/recorder.rs:235` handles `TurnChange::Closed`: it closes the turn row
  (:238) with its `TurnEnd` (done, interrupted, failed or lost), and is reached from `record()` and
  `run_ended()` (:335).
- After 16-1: `step_times::skill_estimates` (the one rule for a skill's estimate), `SkillUse` with
  `call_id`, `store::progress::inputs`, `sessions/progress.rs`. After 16-2: migration 0026 is taken.
  Re-check both after they land.

## Goal

Each finished skill estimate is stored under the skill's name. Once a skill has 5 earlier timed runs,
a run under a third or over three times their median gets a quiet chip in the panel: "30 sec ·
usually ~18 min", with a tooltip. Free-text checklist steps never get one, and untimed runs never
count (D121).

## Exact Scope

Create:
- `src-tauri/migrations/0027_step_runs.sql` (or the next free number; check first)
- `src-tauri/src/store/step_runs.rs`
- `crates/chorus-core/src/usual_time.rs`

Edit:
- `src-tauri/src/store/mod.rs` (`MIGRATION_FILES` entry, `pub mod step_runs;`)
- `crates/chorus-core/src/lib.rs` (one line); `step_times.rs` (`ended_by` on `SkillEstimate`,
  `usual` on `SkillUse`)
- `src-tauri/src/events/recorder.rs` (persist at turn close), `src-tauri/src/sessions/progress.rs`
  (attach markers)
- `src/views/session/ProgressPanel.vue` (the chip), `src/views/session/progress.ts` and its test
- `src/ipc/bindings.ts` (generated), `src/ipc/index.ts` (re-export `UsualMarker`)

Nothing else.

## Non-Goals

- Do not revert or commit unrelated changes, including `Python/` and the kickoff doc edits.
- No marker on free-text checklist steps, ever (D121). No writer for Chorus's own steps yet: the
  table allows `'chorus'` for P17, nothing in P16 writes it.
- No notification, Inbox entry, attention change, sound, pulse or red (roadmap 16.4).
- No backfill from older sessions; counting starts with this build.
- No Usage screen or statistics page (P11). No goal-mode behaviour (P17). No screen-text inference.

## Dependencies

Task 16-1 (skill names, `skill_estimates`, the panel). Task 16-2 only for the migration number.

## Step-by-step Work

1. Core: `usual_time.rs` (spec §2) with tests. In `step_times.rs`, add `ended_by` to
   `SkillEstimate` and `usual: Option<UsualMarker>` (None from `compute`) to `SkillUse`.
2. Migration `0027_step_runs.sql` (spec §3) and its `MIGRATION_FILES` entry.
3. `store/step_runs.rs`: `insert`, `past`, `persist_turn` (spec §3).
4. Recorder: after `close_turn` in the `Closed` arm (:235–238), call `persist_turn` for that turn;
   print an error, don't fail the turn.
5. `sessions/progress.rs`: for each named `SkillUse`, read up to 20 earlier runs once per name and
   set `usual`. `npm run bindings`.
6. `progress.ts`: `usualLabel`, `usualTitle`; the chip in `ProgressPanel.vue` (spec §5).
7. Run the checks, the real-app check, then commit.

## Test Expectations

- `usual_time::tests`: `no_marker_before_five_runs`, `a_run_under_a_third_of_the_median_is_shorter`,
  `a_run_over_three_times_the_median_is_longer`, `exactly_a_third_or_three_times_is_not_marked`,
  `a_running_skill_can_only_be_longer`, `median_of_an_even_count_is_the_mean_of_the_middle_two`,
  `tiny_medians_give_no_marker`.
- `step_times::tests::estimates_say_what_ended_them`.
- `src-tauri`: `store::step_runs::tests::a_run_is_written_once_per_call`,
  `store::step_runs::tests::past_runs_are_those_that_ended_before`,
  `recorder::tests::a_done_turn_stores_its_named_skill_estimates`,
  `recorder::tests::cut_off_or_unnamed_skills_are_not_stored`.
- Vitest: `usualLabel` ("30 sec · usually ~18 min"), `usualTitle` (shorter, longer, run count).

## Verification Commands

```
cargo test -p chorus-core -- usual_time:: step_times::
cargo test -p chorus -- step_runs:: recorder:: store::tests::
npx vitest run src/views/session/progress.test.ts
npm run bindings
npm run check
git status --short
```

## Acceptance Criteria

- [ ] All commands pass; `git status` shows only the scoped files and `?? Python/`.
- [ ] In the isolated instance (Overview §7), in a scratch Claude Code session: five turns, each using
  the same short installed skill and then starting a next task right away. A sixth turn uses it and
  then waits (for example, runs `Start-Sleep 200` in PowerShell) before starting the next task. The
  sixth chip shows "~… · usually ~…" with "Much longer than the … skill usually takes (median of 5
  runs)". Equally valid: five slow runs, then a quick one, for "shorter".
- [ ] A second skill used one to four times shows no marker.
- [ ] Interrupting a turn mid-skill stores nothing for that call.
- [ ] The chip matches the mockup in Glacier and Dusk; nothing pulses, no notification appears.

## Review Checklist

- Only `Skill` calls with a name are written; free-text steps can't reach the table.
- Estimates cut by an interrupted, failed or lost turn are not written; untimed runs never are.
- One rule for the estimate: `persist_turn` uses `step_times::skill_estimates`, not a copy.
- `past` is bounded (20), indexed (`step_runs_key`), and counts only runs that ended earlier.
- The comparison is pure, with strict "under a third" and "over three times", and needs 5 runs.
- Migration numbered, forward-only, `STRICT`, listed; `'chorus'` allowed but unused.
- Colours are tokens; no alarm styling beyond the approved chip.
