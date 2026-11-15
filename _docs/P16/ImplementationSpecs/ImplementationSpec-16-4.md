# Implementation spec 16-4: Usual-time markers

Read [Task-16-4.md](../Tasks/Task-16-4.md) first. Line numbers were verified on 2026-11-15 at
`c5ee0ff`, before 16-1. Earlier tasks move some of them; re-check before editing.

## 1. Shape and the storage choice (K8)

Two ways to know a skill's usual time:

| Option | Cost | Verdict |
|---|---|---|
| Compute from snapshots on every read | Every `Skill` call in every session, plus each session's plan snapshots to find where its estimate ended. `tool_calls` has no index on `tool` (`0005_history.sql:45`), so this scans the table on each 1.5 s read | No |
| Store each finished estimate once, in `step_runs` | One insert per skill call; one indexed read of 20 rows per skill name shown | **Yes** |

Rows are written when a turn closes, because by then every estimate in that turn has ended (a skill's
estimate is bounded by its turn's end, 16-1 rule 8). The write reuses `step_times::skill_estimates`,
so the stored figure is the one the panel shows.

## 2. `crates/chorus-core/src/usual_time.rs` (new, pure)

```rust
pub const MIN_RUNS: usize = 5;          // D121
pub const WINDOW: u32 = 20;             // earlier runs compared
pub const FACTOR: i64 = 3;              // D121: under a third, or over three times
pub const MIN_MEDIAN_MS: i64 = 1_000;   // a median under a second is too small to compare

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum Direction { Shorter, Longer }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UsualMarker { pub direction: Direction, pub usual_secs: u32, pub runs: u32 }

/// The median; for an even count, the mean of the middle two (rounded down). None when empty.
pub fn median_ms(past: &[i64]) -> Option<i64>;

/// `past`: earlier timed runs of one key, in any order. `this_ms`: this run so far.
pub fn marker(past_ms: &[i64], this_ms: i64, finished: bool) -> Option<UsualMarker>;
```

Rules: fewer than `MIN_RUNS` earlier runs → none. Median under `MIN_MEDIAN_MS` → none. Finished and
`this_ms * 3 < median` → shorter. `this_ms > median * 3` → longer, finished or still running (a
running skill can't yet be "shorter"). Exactly a third or three times → none. Use saturating
arithmetic. `runs` is the number of earlier runs; `usual_secs` the median rounded to seconds.

`step_times.rs` changes:

```rust
pub enum EndedBy { StepStart, NextSkill, TurnEnd, RunEnd }    // internal, no Type
pub struct SkillEstimate { /* 16-1 fields */ pub ended_by: Option<EndedBy> }   // None: running
pub struct SkillUse { /* 16-1 fields */ pub usual: Option<UsualMarker> }        // None from compute
```

## 3. Migration and store

`src-tauri/migrations/0027_step_runs.sql` (after 16-2's 0026; check the folder first):

```sql
-- P16 (D121): finished, timed runs of named steps, for "usual time" markers. Only skills (by
-- name) and Chorus's own steps are keyed; free-text checklist steps never are, and untimed runs are
-- never written. P16 writes skills only; 'chorus' is kept for Chorus's own steps (P17), so that
-- needs no table rebuild.

CREATE TABLE step_runs (
  id          INTEGER PRIMARY KEY,
  key_kind    TEXT    NOT NULL CHECK (key_kind IN ('skill', 'chorus')),
  key         TEXT    NOT NULL,
  session_id  INTEGER NOT NULL REFERENCES sessions (id),
  turn_id     INTEGER REFERENCES turns (id),
  -- What the run came from, so it is written once: a skill's tool_calls.id.
  source_ref  TEXT    NOT NULL,
  started_at  INTEGER NOT NULL,
  ended_at    INTEGER NOT NULL,
  duration_ms INTEGER NOT NULL CHECK (duration_ms >= 0),
  -- 1 when the time is an estimate (a skill's, from its call to the next step).
  estimate    INTEGER NOT NULL CHECK (estimate IN (0, 1)),
  UNIQUE (key_kind, source_ref)
) STRICT;

CREATE INDEX step_runs_key ON step_runs (key_kind, key, ended_at);
```

`src-tauri/src/store/step_runs.rs` (new):

```rust
pub struct StepRun<'a> { pub kind: &'a str, pub key: &'a str, pub session_id: u32, pub turn_id: Option<u32>,
    pub source_ref: String, pub started_ms: i64, pub ended_ms: i64, pub estimate: bool }

pub fn insert(store: &Store, run: &StepRun) -> Result<bool, String>;
// INSERT OR IGNORE INTO step_runs (key_kind, key, session_id, turn_id, source_ref, started_at,
//   ended_at, duration_ms, estimate) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)   -- true if written

pub fn past(store: &Store, kind: &str, key: &str, before_ms: i64, limit: u32) -> Result<Vec<i64>, String>;
// SELECT duration_ms FROM step_runs WHERE key_kind = ?1 AND key = ?2 AND ended_at <= ?3
// ORDER BY ended_at DESC, id DESC LIMIT ?4

pub fn persist_turn(store: &Store, session_id: u32, run_id: u32, turn_id: i64, end: TurnEnd) -> Result<u32, String>;
```

`persist_turn` runs inside the recorder's lock, and a Claude hook waits for it, so it returns at
once when the turn had no `Skill` call (`SELECT EXISTS (SELECT 1 FROM tool_calls WHERE turn_id = ?1
AND tool = 'Skill')`, on `tool_calls_turn`). Otherwise: read `store::progress::inputs(store,
session_id)`; call `skill_estimates(&snapshots,
&runs, Some(run_id), &skill_calls)`; keep estimates with `turn_id == turn_id`, a `name`, an
`ended_ms`, and an `ended_by` that is a clean boundary: `StepStart` or `NextSkill` always, `TurnEnd`
only when `end == TurnEnd::Done`. Never `RunEnd`, never interrupted, failed or lost turns: those were
cut off, not finished. Insert each with `kind "skill"`, `key = name`, `source_ref = call_id`,
`estimate: true`. Return how many were written.

`past` takes runs that ended at or before this run started, so an old session's marker says what was
usual then, and a run never compares with itself.

## 4. Writing and reading

Recorder, `apply_change` `Closed` arm (`recorder.rs:235–238`), right after `history::close_turn`:

```rust
if let Err(e) = crate::store::step_runs::persist_turn(store, self.session_id, self.run_id, row, end) {
    eprintln!("chorus: step times for turn {row}: {e}");
}
```

`end` is the arm's `TurnEnd`; it is `Copy` and still used after this call. Both `record()` and
`run_ended()` reach this arm; a lost turn writes nothing by the rule above.

`sessions/progress.rs`, after `compute`: for every `SkillUse` with a name (plan chips and skill rows),
look up `past(store, "skill", name, use.at_secs as ms, WINDOW)`; cache by name within the call (at
most 20 names; beyond that, no marker). `this_ms = secs * 1000`, plus `now - running_since` while
running (`now` from `crate::store::now_ms()`). Set `usual = usual_time::marker(&past, this_ms,
running_since.is_none())`.

## 5. The chip

`progress.ts`:

```ts
export function usualLabel(m: UsualMarker, thisSecs: number): string   // "30 sec · usually ~18 min"
export function usualTitle(m: UsualMarker, skill: string): string
// "Much shorter than the architect skill usually takes (median of 7 runs)" / "Much longer …"
```

In `ProgressPanel.vue`, in the name cell right after the skill chip (or the skill row's name), per
the mockup's note chip:

| Property | Value |
|---|---|
| Layout | `display: inline-flex; align-items: center; gap: 5px; padding: 2px 9px; border-radius: var(--radius-pill); font-size: 12px` |
| Colour | text `var(--status-needs)`; background `color-mix(in srgb, var(--status-needs) 13%, transparent)` |
| Icon | 12px outlined triangle with a mark, `currentColor`, stroke 1.3, `aria-hidden` |
| Text and title | `usualLabel`; `title` and `aria-label` from `usualTitle` |
| Motion | none |

Mockup hex: `#ffc400` / `#e9b85b` → `--status-needs`; its 13–14% soft → the `color-mix` above. This
is the approved mockup's look; it carries no animation, sound, notification or attention change, so
it stays a marker, not an alarm.

## 6. Edge cases

| Case | Behaviour |
|---|---|
| Fewer than 5 earlier runs | No chip |
| Running skill passing 3× the median | "Longer" shows live, from the elapsed time so far |
| Skill cut off by an interrupt, failure or lost run | Not stored; still shown in the panel as an estimate, with a marker only if it is already "longer" |
| Unnamed `Skill` call (before 16-1) | Never stored, never marked |
| Free-text checklist step | Never stored, never marked (D121) |
| Plugin skill names (`anthropic-skills:pdf`) | Kept whole; a different prefix is a different key |
| Same call seen twice (restart, re-close) | `UNIQUE (key_kind, source_ref)` keeps one row |
| Very short skills (median under 1 s) | No chip |
| Sessions from before 16-4 | Not counted; no backfill |
| Clock | Every figure comes from Chorus's stored times, through `skill_estimates` |

## 7. Verification

Build checks: the Task's commands. Then, in the isolated instance (Overview §7), scratch folder,
Claude Code:

1. Prompt: "Create two tasks. Mark task 1 in progress, use the <short installed skill> skill, then
   mark task 1 completed and task 2 in progress, then stop." Repeat in five turns (a new task list
   each time is fine). Each turn's chip shows "~… " and no marker.
2. Sixth turn: the same, but after the skill, run `Start-Sleep 200` before marking task 2 in
   progress. When the estimate passes three times the median, the marker appears while it runs;
   after the turn it stays, with the tooltip "Much longer … (median of 5 runs)".
3. A second skill used once: no marker.
4. Interrupt a turn while its skill runs: no new row for it (check the count the tooltip reports on
   the next run).
5. Glacier and Dusk; no pulse, no notification, no Inbox change.
