# Task 16-2: Where the time went

## Source Of Truth

- D116 and D119 in [DECISIONS.md](../../DECISIONS.md); the roadmap's slice 16.2
  ([HANDOFF-P16-P17-progress-goals.md](../../HANDOFF-P16-P17-progress-goals.md) §5).
- [Phase-16-Overview.md](../Phase-16-Overview.md): gates, K6 and K9.
- [ImplementationSpec-16-2.md](../ImplementationSpecs/ImplementationSpec-16-2.md).
- The approved mockup's footer, "Where the time went" (`Main.dc.html`, outside the repo).

## Initial Starting Point (verified 2026-11-15, commit `c5ee0ff`, before 16-1)

- `crates/chorus-core/src/activity.rs:19` `Activity`: Unknown, Working, AwaitingApproval, NeedsAnswer,
  Ready, Failed, serialised kebab-case (:18). It is live state only. `Tracker::apply` (:141) sets it;
  `run_ended` (:195) sets Unknown. A new tracker starts Unknown (:82).
- `src-tauri/src/events/recorder.rs`: the tracker changes in three places only: `record()` (:199),
  `resolved_request()` (:184) and `run_ended()` (:335). Every agent reaches `record()`: Claude hooks
  (`sessions/mod.rs:626`), the Claude transcript interrupt (:1466), opencode (:1489) and the Codex
  bridge (`codex_bridge.rs:149`, :154). `resolved_request` is Codex's (`codex_bridge.rs:152`).
  `run_ended` is called at `sessions/mod.rs:1641`.
- No table records activity over time. The newest migration is `0025_turn_usage.sql`; migrations are
  listed in `MIGRATION_FILES` (`src-tauri/src/store/mod.rs:32`, last entry :57).
- `runs(started_at, ended_at)` with index `runs_by_session` (`0001_initial.sql:54`); `start_run` is
  called at `sessions/mod.rs:1249`.
- After 16-1: `SessionProgress` in `chorus-core/src/step_times.rs`, assembled in
  `src-tauri/src/sessions/progress.rs`, shown by `ProgressPanel.vue`. Re-check these after 16-1 lands.

## Goal

Each change of a session's activity is recorded with Chorus's clock. The panel's footer splits the
newest run's time into four parts: agent working, waiting on you, waiting on approval and Idle, as a
bar with a legend, under "Where the time went · Started 9:02 · turn 7". After a turn ends, the first
15 minutes of waiting count as waiting on you and the rest as Idle; an open question is always
waiting on you (Matthew, 2026-11-15). Approval time is hatched, because in Glacier approval and needs
share a colour; Idle is a quiet grey. A run from before the table reads "not split".

## Exact Scope

Create:
- `src-tauri/migrations/0026_activity_log.sql` (or the next free number; check first)
- `src-tauri/src/store/activity_log.rs`
- `crates/chorus-core/src/time_split.rs`

Edit:
- `src-tauri/src/store/mod.rs` (`MIGRATION_FILES` entry, `pub mod activity_log;`)
- `crates/chorus-core/src/lib.rs` (one line), `activity.rs` (`as_str`, `parse`, a test)
- `crates/chorus-core/src/step_times.rs` (one field, `split`, on `SessionProgress`)
- `src-tauri/src/events/recorder.rs` (a row at each change), `src-tauri/src/sessions/mod.rs` (the
  run-start row), `src-tauri/src/sessions/progress.rs` (fill `split`)
- `src/views/session/ProgressPanel.vue` (the footer), `src/views/session/progress.ts` and its test
- `src/ipc/bindings.ts` (generated), `src/ipc/index.ts` (re-export `TimeSplit`)

Nothing else.

## Non-Goals

- Do not revert or commit unrelated changes, including `Python/` and the kickoff doc edits.
- No split across several runs, no per-turn split, no history chart. Newest run only (K6).
- No backfill for old runs, and no guessed split from turns and asks (D119 chose recording).
- No Overview bar (16-3), no usual times (16-4), no goal-mode behaviour (P17), no Usage screen (P11).
- Never infer state from screen text or PTY silence; "no signal" stays Working, as the tracker says.
- No token changes: keep the hatch, don't recolour approval, and add no token for Idle.
- No setting for the 15-minute Idle threshold; it is a constant (K6).

## Dependencies

Task 16-1 (the panel, `SessionProgress`, `sessions/progress.rs`).

## Step-by-step Work

1. Core: `Activity::as_str()` and `Activity::parse()` matching the serde names, with a round-trip
   test. Write `time_split.rs` (spec §2) with its tests.
2. Migration `0026_activity_log.sql` (spec §3) and its `MIGRATION_FILES` entry.
3. `store/activity_log.rs`: `record`, `newest_run`, `marks_for_run`, `turns_in_run` (spec §3).
4. Recorder: note the activity before, and write one row after, in `record()`, `resolved_request()`
   and `run_ended()` when it changed (spec §4). In `sessions/mod.rs`, write an `unknown` row right
   after `start_run` returns.
5. `sessions/progress.rs`: compute the split for the newest run and set `split`. `npm run bindings`.
6. Panel footer per spec §5; `progress.ts` gets `splitLegend`.
7. Run the checks, the real-app check, then commit.

## Test Expectations

- `time_split::tests`: `no_marks_is_not_split`, `each_state_counts_in_its_bucket`,
  `a_live_run_counts_up_to_now`, `nothing_after_the_run_end_counts`,
  `marks_outside_the_run_are_clipped`, `totals_add_up_to_the_span_within_rounding` (all four
  buckets), `out_of_order_marks_are_sorted`, and for Idle (`IDLE_AFTER_MS`):
  `ready_for_exactly_fifteen_minutes_is_all_waiting_on_you`,
  `ready_for_fifteen_minutes_and_a_second_has_one_second_idle`,
  `two_short_ready_gaps_never_add_up_to_idle` (two 10-minute gaps split by Working: 20 min waiting
  on you, no Idle), `an_open_question_is_never_idle`, `an_open_approval_is_never_idle`,
  `failed_follows_the_fifteen_minute_rule`, `unknown_before_the_first_event_becomes_idle_after_fifteen_minutes`,
  `failed_then_ready_is_one_stretch`.
- `activity::tests::activity_names_round_trip`.
- `src-tauri`: `store::activity_log::tests::marks_come_back_in_order_for_one_run`;
  `recorder::tests::each_activity_change_is_logged_once` (a hook sequence with an approval and a
  question: one row per change, none for events that don't change it);
  `store::tests::migrations_are_all_listed` still passes.
- Vitest: `splitLegend` (four labels in order, zero buckets dropped, rounded figures) and the "not
  split" line.

## Verification Commands

```
cargo test -p chorus-core -- time_split:: activity::
cargo test -p chorus -- activity_log:: recorder:: store::tests::
npx vitest run src/views/session/progress.test.ts
npm run bindings
npm run check
git status --short
```

## Acceptance Criteria

- [ ] All commands pass; `git status` shows only the scoped files and `?? Python/`.
- [ ] In the isolated instance (Overview §7), a Claude Code session with Manual permissions (so a
  write asks for approval), in a scratch folder: ask a question through AskUserQuestion and wait
  about a minute before answering; ask for a file write and wait about a minute before approving.
  The footer's totals add up to the header's elapsed time, give or take rounding, and the two waits
  are close to what you waited.
- [ ] Left Ready for about 17 minutes, the session shows about 2 minutes of Idle as a fourth, grey
  segment and legend entry, and the four totals still add up to the elapsed time.
- [ ] The approval segment and its legend swatch are hatched in Glacier and in Dusk; Idle is grey
  (`--text-muted`) in both, never an alert colour.
- [ ] A session whose newest run started before this build reads "not split" with its reason.
- [ ] After the session exits, the totals stop growing; after Chorus restarts they read the same.

## Review Checklist

- Rows are written only where the tracker changes (`recorder.rs` three sites) and at run start; no
  other inference.
- One row per change, with the same `now_ms` the events were recorded with.
- The split is pure, takes `now` as a parameter, and every in-run millisecond lands in one bucket.
- Unknown, Failed, Ready, NeedsAnswer and after-exit time are counted exactly as K6 says, and the
  spec's table matches. `IDLE_AFTER_MS` is one named constant in `time_split.rs`, used nowhere else.
- Migration is numbered, forward-only, `STRICT`, listed, and indexed for the read it serves.
- Colours are tokens; the hatch uses `--status-approval`; nothing in `tokens.css` changed.
