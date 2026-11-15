# Implementation spec 16-2: Where the time went

Read [Task-16-2.md](../Tasks/Task-16-2.md) first. Line numbers were verified on 2026-11-15 at
`c5ee0ff`, before 16-1. 16-1 moves some of them; re-check before editing.

## 1. Shape

The recorder writes one `activity_log` row each time the tracker's activity changes, plus an
`unknown` row when a run starts. `SessionManager::progress` reads the newest run's rows and passes
them to `time_split::split`, a pure function. The result rides on `SessionProgress.split`, so the
panel reads it on the same 1.5 s cycle as the steps.

## 2. `crates/chorus-core/src/time_split.rs` (new, pure)

```rust
pub struct Mark { pub at_ms: i64, pub activity: Activity }       // input only, no Type

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TimeSplit {
    /// Why there is no split ("not split"); `None` when the figures below are real.
    pub not_split: Option<String>,
    pub working_secs: u32, pub you_secs: u32, pub approval_secs: u32, pub idle_secs: u32,
    pub started_at_secs: u32, pub ended_at_secs: Option<u32>, pub turns: u32,
}
pub const MAX_MARKS: u32 = 20_000;
/// A waiting stretch counts as waiting on you for this long, then as Idle (Matthew, 2026-11-15).
pub const IDLE_AFTER_MS: i64 = 15 * 60 * 1000;
pub fn split(run_started_ms: i64, run_ended_ms: Option<i64>, marks: &[Mark], now_ms: i64, turns: u32) -> TimeSplit;
```

A struct, not a tagged enum: struct-variant fields stay snake_case in the bindings today (see
`SessionAction`'s `ask_id`), and a plain struct gives camelCase without relying on specta's handling
of field renames inside variants.

Rules (K6):

1. **No marks:** `not_split: Some("Recorded before Chorus kept state changes.")`, figures zero.
2. **Span:** from `run_started_ms` to `run_ended_ms`, or `now_ms` while the run lives. An end before
   the start gives zero time. Nothing after the run's end counts.
3. **Intervals:** sort marks by `at_ms` (stable). Each mark's state lasts until the next mark, or the
   span's end. Clip every interval to the span. Before the first mark the state is Unknown.
4. **Buckets** (four, Matthew 2026-11-15):

| Activity | Bucket | Why |
|---|---|---|
| Working | working | A turn is in progress. A "no signal" Working session still counts as working: the channel said so last, and silence never changes state (ARCHITECTURE §6) |
| AwaitingApproval | waiting on approval, however long | The agent waits for permission |
| NeedsAnswer | waiting on you, however long | It asked you something; an open question is never Idle |
| Ready | waiting on you for the stretch's first 15 min, then Idle | Its turn ended and it waits for your next prompt |
| Failed | as Ready: 15 min waiting on you, then Idle | The turn ended in an error and the agent waits for you |
| Unknown | as Ready: 15 min waiting on you, then Idle | Inside a run this is only the time before the first event: the agent starting and waiting for its first prompt. After `run_ended` the span has ended |

5. **Waiting stretches.** A stretch is consecutive time in Ready, Failed or Unknown. Its clock starts
   when the stretch starts and does not restart when it moves between those three (for example
   Failed, then Ready after an Idle event). Working, AwaitingApproval or NeedsAnswer end the stretch;
   the next one starts from zero. Within a stretch, the first `IDLE_AFTER_MS` go to waiting on you and
   the rest to Idle: exactly 15 min is all waiting on you; 15 min and 1 s gives 1 s of Idle. A stretch
   cut by the span's end (now, or the run's end) counts only up to there.
6. **Rounding:** sum each bucket in milliseconds, then round each to the nearest second. The four
   totals equal the span within 2 seconds. `turns` is passed through; `started_at_secs` and
   `ended_at_secs` are the span's ends.

The split covers the newest run only. A session restarted or resumed starts a new split, which
matches the header's elapsed time (`runStartedAtSecs`).

`activity.rs`: add

```rust
impl Activity {
    pub fn as_str(self) -> &'static str;          // "unknown", "working", "awaiting-approval",
    pub fn parse(s: &str) -> Option<Self>;         // "needs-answer", "ready", "failed"
}
```

and test that each matches `serde_json::to_value` of the same variant.

## 3. Migration and store

`src-tauri/migrations/0026_activity_log.sql`. Check `src-tauri/migrations/` first: if 0026 is
taken, use the next free number and say so in the commit.

```sql
-- P16 (D119): each change of a session's activity, timed by Chorus's clock, so the Progress panel
-- can say where the time went. Written where the recorder applies events, and as 'unknown' when a
-- run starts. Never inferred from screen text. A run with no rows reads "not split".

CREATE TABLE activity_log (
  id         INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES sessions (id),
  run_id     INTEGER NOT NULL REFERENCES runs (id) ON DELETE CASCADE,
  activity   TEXT    NOT NULL CHECK (activity IN
    ('unknown', 'working', 'awaiting-approval', 'needs-answer', 'ready', 'failed')),
  at         INTEGER NOT NULL
) STRICT;

CREATE INDEX activity_log_run ON activity_log (run_id, at, id);
```

Add `("0026_activity_log.sql", include_str!("../../migrations/0026_activity_log.sql")),` after the
0025 entry (`store/mod.rs:57`).

`src-tauri/src/store/activity_log.rs` (new):

```rust
pub fn record(store: &Store, session_id: u32, run_id: u32, activity: Activity, at: i64) -> Result<(), String>;
// INSERT INTO activity_log (session_id, run_id, activity, at) VALUES (?1, ?2, ?3, ?4)

pub fn newest_run(store: &Store, session_id: u32) -> Result<Option<(u32, i64, Option<i64>)>, String>;
// SELECT id, started_at, ended_at FROM runs WHERE session_id = ?1 ORDER BY started_at DESC, id DESC LIMIT 1

pub fn marks_for_run(store: &Store, run_id: u32) -> Result<Vec<Mark>, String>;
// SELECT activity, at FROM activity_log WHERE run_id = ?1 ORDER BY at, id LIMIT 20000
// A row whose activity doesn't parse is skipped.

pub fn turns_in_run(store: &Store, session_id: u32, run_id: u32) -> Result<u32, String>;
// SELECT count(*) FROM turns WHERE session_id = ?1 AND run_id = ?2   (uses turns_session)
```

If `marks_for_run` returns exactly `MAX_MARKS` rows, the split is `not_split: Some("More state
changes than Chorus splits (20,000).")` rather than a wrong total.

## 4. Writing the rows

`events/recorder.rs`, a private helper:

```rust
fn log_activity(&self, store: &Store, before: Activity, at: i64) {
    let now = self.tracker.activity();
    if now != before && let Err(e) = crate::store::activity_log::record(store, self.session_id, self.run_id, now, at) {
        eprintln!("chorus: activity log for session {}: {e}", self.session_id);
    }
}
```

Call sites (verified before 16-1):

| Site | Change |
|---|---|
| `record()` (:189) | `let before = self.tracker.activity();` first; `self.log_activity(store, before, now_ms);` after the events loop (:198–204), before the plan block (:205) |
| `resolved_request()` (:181) | note `before` first; log after the `tracker.apply` at :184 with `crate::store::now_ms()` |
| `run_ended()` (:333) | note `before` first; log after the loop (:335–337) with `crate::store::now_ms()` |

One row per call, not per event: several events in one payload share one `now_ms`, so only the final
state of that instant matters. Errors are printed, as `sessions/mod.rs:1496` does for usage; the
split can't detect a missing row, so the print is the trace.

Run start: in `sessions/mod.rs`, right after `start_run` returns `run_id` (:1249–1259), write
`activity_log::record(&self.store, row.id, run_id, Activity::Unknown, crate::store::now_ms())`. A
run with any row was recorded under this build; a run with none predates it. Ignore a failure the
same way, with a print.

## 5. Assembly and footer

`step_times::SessionProgress` gains `pub split: Option<TimeSplit>` (`None` from `compute`).
`sessions/progress.rs` sets it: `newest_run`; none means `None` (no footer); else `marks_for_run`,
`turns_in_run` and `split(started, ended, &marks, crate::store::now_ms(), turns)`.

`progress.ts`:

```ts
export function splitLegend(s: TimeSplit): { key: 'working' | 'you' | 'approval' | 'idle'; label: string; secs: number }[]
// in this order: "Agent working", "Waiting on you", "Waiting on approval", "Idle"; zero-second buckets dropped
```

Footer in `ProgressPanel.vue`, inside the open panel after the list, per the mockup:

- Container: padding 14px 20px 18px, 1px `--border` top, column, gap 10px.
- Heading row (wraps, baseline, space-between): "Where the time went" (12px, 600, 0.06em, upper,
  `--text-2`); right, 12px `--text-2`: "Started {timeLabel(startedAtSecs)} · turn {turns}".
- Bar: `role="img"`, `aria-label` "Agent working 47 minutes, waiting on you 13 minutes, waiting on
  approval 4 minutes, idle 20 minutes" (zero buckets left out); flex, 12px high, radius 6px,
  `overflow: hidden`, gap 2px. Segments use `flex: <secs>`, in legend order: working
  `--status-working`; you `--status-needs`; approval `repeating-linear-gradient(135deg,
  var(--status-approval) 0 4px, color-mix(in srgb, var(--status-approval) 40%, transparent) 4px
  8px)`; Idle `--text-muted`, last and quiet.
- Legend (wraps, gap 8px 22px, 13px): 10px swatch (radius 3px; the approval swatch hatched with 3px
  stripes; the Idle swatch `--text-muted`), label, figure in mono 500 with `stepDuration`. The Idle
  label and figure are `--text-muted` too; the others keep the panel's text colours.
- Idle's colour is `--text-muted` (#7d8ba0 Glacier, #6f7789 Dusk), a neutral grey in both themes.
  Not `--status-done`: it is #3a4a60 in Glacier but #a3aabb in Dusk (`tokens.css:31`, `:95`), as
  light as Dusk's `--text-2`, so Idle would be the loudest segment there. Never an alert colour.
- `notSplit` set: one muted line instead of bar and legend, "Where the time went: not split.
  {notSplit}".

The hatch stays in both themes. In Glacier `--status-approval` equals `--status-needs` (#ffc400,
`tokens.css:25–26`), so the hatch is what tells them apart; don't change tokens.

## 6. Edge cases

| Case | Behaviour |
|---|---|
| Session never ran | No run: `split` is `None`, no footer |
| You leave for an hour after a turn ends | 15 min waiting on you, 45 min Idle |
| A question left open for an hour | 60 min waiting on you; NeedsAnswer is never Idle |
| An approval left open for an hour | 60 min waiting on approval; never Idle |
| Turn fails, then an Idle event sets Ready | One stretch: the 15 minutes run across Failed and Ready |
| Agent never gets a first prompt | Unknown: 15 min waiting on you, then Idle |
| Run from before the table | No rows: "not split" |
| Chorus crashed mid-run | The run's `ended_at` is set when Chorus next reconciles it; until then a stopped run counts up to now. Re-check how restore ends crashed runs before relying on this, and note it |
| Approval answered in Raw | The tracker leaves AwaitingApproval on the next tool event; the split follows the tracker |
| Several state changes in one payload | One row, final state (§4) |
| Codex request resolved by the agent | `resolved_request` logs Working if the tracker moved |
| Clock | Chorus's `now_ms` at each site; the agent's timestamps are never used |

## 7. Verification

Build checks: the Task's commands. Then, in the isolated instance (Overview §7), scratch folder:

1. New Claude Code session, Manual permissions. Note the header's elapsed time as it starts.
2. Prompt: "Ask me one question with AskUserQuestion, then write hello.txt." Wait about 60 s
   before answering; wait about 60 s before approving the write.
3. The footer's totals: waiting on you is at least the minute you waited (plus the time before your
   first prompt); waiting on approval is about a minute; no Idle yet. They add up to the elapsed
   time within a minute's rounding.
4. Leave the session Ready for about 17 minutes. Idle appears as a fourth, grey segment of about
   2 minutes, waiting on you grew by 15, and the four totals still add up to the elapsed time.
5. Hover the bar: the accessible label reads the same figures. The approval segment is hatched and
   Idle is grey, not an alert colour; check both themes.
6. Exit the session: totals stop. Restart Chorus: the same totals.
7. Open a session created before this build: "not split".
