# Implementation spec 16-3: Overview card bar

Read [Task-16-3.md](../Tasks/Task-16-3.md) first. Line numbers were verified on 2026-11-15 at
`c5ee0ff`, before 16-1. 16-1 and 16-2 move some of them; re-check before editing.

## 1. Shape (K7)

The card reads a new `SessionSummary.progress: Option<ProgressGlance>`. For a live session the
recorder keeps the glance in memory, updated as each plan snapshot arrives, and `summary_of` reads it
under the recorder lock it already takes for `activity` (`sessions/mod.rs:757`): no query. A session
with no live run costs one bounded query (`store::progress::latest_plan`, newest 20 turns, one row).
The front end makes no per-card call. The hook path republishes the summary when the plan changes,
so the bar moves without waiting for an activity change.

`summary_of` already runs several small queries per session (`history::latest` at :756,
`rows::last_run` at :753 and others), so one bounded read for stopped sessions follows existing
practice; live sessions, the ones republished often, cost nothing extra.

## 2. Core, in `step_times.rs`

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProgressGlance {
    pub done: u32,
    pub total: u32,
    /// Each step's status in plan order (at most 100, as plans are).
    pub statuses: Vec<StepStatus>,
    /// The first in-progress step's text.
    pub current: Option<String>,
    /// When it started, while it runs in the live run; None when stopped or only seeded.
    pub current_since_secs: Option<u32>,
    /// The first pending step after the current one, or the first pending one.
    pub next: Option<String>,
}

/// None for an empty plan (D122: no bar without a plan).
pub fn glance(steps: &[PlanStep], current_since_ms: Option<i64>) -> Option<ProgressGlance>;

/// The current step's text and since when, after a new snapshot at `at_ms`. The same step (same
/// trimmed text, as 16-1's matching) keeps its start; another step starts at `at_ms`; none in
/// progress gives None.
pub fn advance_current(prev: Option<&(String, i64)>, steps: &[PlanStep], at_ms: i64) -> Option<(String, i64)>;
```

`SessionSummary` derives `Deserialize`, so `ProgressGlance` and `StepStatus` must too (16-1's
outputs already do). TypeScript: `progress: ProgressGlance | null`.

The card shows the current interval; the panel shows the step's total. They differ only for a step
that was reopened, which is fine for a glance.

## 3. Recorder (`events/recorder.rs`)

New fields, initialised in `new` (:52–73): `shown: Vec<PlanStep>`, `current: Option<(String, i64)>`,
`plan_version: u64`.

In `progress_at`, where the plan is kept (:87):

```rust
if let chorus_core::progress::Progress::Plan { steps } = &content {
    self.plan = steps.clone();
    self.shown = steps.clone();
    self.current = chorus_core::step_times::advance_current(self.current.as_ref(), steps, at);
    self.plan_version += 1;
}
```

Methods: `glance(&self) -> Option<ProgressGlance>` (from `shown` and `current`'s time),
`plan_version(&self) -> u64`, and `seed_glance(&mut self, steps: Vec<PlanStep>)`, which fills `shown`
only while it is empty and leaves `current` empty.

At the 500-row cap a snapshot isn't stored but still advances the glance, so the card can run ahead
of the panel; the panel's capped note covers it.

## 4. `sessions/mod.rs`

- `SessionSummary` (:112), after `latest_at_secs` (:176):
  `/// The latest plan at a glance (P16); none without a plan (D122).`
  `pub progress: Option<chorus_core::step_times::ProgressGlance>,`
- `summary_of` (:744), replacing the activity read at :757:

```rust
let (activity, glance) = run.as_ref().map_or((Activity::Unknown, None), |r| {
    let r = r.recorder.lock().expect("recorder lock");
    (r.activity(), r.glance())
});
let progress = if run.is_some() { glance } else {
    crate::store::progress::latest_plan(&self.store, row.id).ok().flatten()
        .and_then(|(steps, _)| chorus_core::step_times::glance(&steps, None))
};
```

  and `progress,` in the literal (:760–815).
- Run start: 16-1 seeds the merge plan from `latest_plan` when `resumed`. Change it to one read used
  twice: seed the glance for every run, and the merge plan only when resumed. A fresh conversation's
  card then keeps the session's last plan until the agent writes a new one, as the panel does.
- `receive_hook` (:621–634): note `plan_version()` beside `before`, and publish when the activity
  changed or the version did:

```rust
let (changed, reply) = { /* … */ (before != recorder.activity() || version != recorder.plan_version(), reply) };
if changed { self.notify_id(session_id); }
```

  The Codex bridge already publishes after every batch (`codex_bridge.rs:168`); opencode has no plan
  channel. No other path changes.

## 5. Card (`SessionCard.vue`)

`progress.ts`:

```ts
export function cardLine(g: ProgressGlance, live: boolean, now: number): { label: string; step: string; secs: number | null }
// live and current: "Now:", the step, now - currentSinceSecs (null without a since)
// not live and current: "Stopped at:", the step, null
// no current, a next: "Next:", the step, null.  Neither: "All steps done", "", null
export function cardSegments(g: ProgressGlance): { kind: 'segments'; statuses: StepStatus[] } | { kind: 'continuous'; donePct: number; currentPct: number }
// segments up to 24 steps, else one continuous bar
```

Insert between the waiting line (:89–91) and `<footer>` (:92):

```vue
<div v-if="s.hasEventChannel" class="progress">
  <template v-if="s.progress">
    <p class="progress-line">
      <span class="now">{{ line.label }} <b>{{ line.step }}</b><span v-if="line.secs !== null" class="mono"> · {{ stepDuration(line.secs) }}</span></span>
      <span class="count">{{ s.progress.done }} of {{ s.progress.total }} done</span>
    </p>
    <div class="bar" role="img" :aria-label="`${s.progress.done} of ${s.progress.total} steps done`"><!-- segments --></div>
  </template>
</div>
```

The slot is reserved (fixed height) on every card with an event channel, drawn only with a plan.
That is what keeps the card from jumping when its first plan arrives (K7). Shells and Grok get no
slot.

The mockup puts "4 of 9 done" and the run's elapsed time under the bar. The card's footer already
shows the elapsed time ("12m · updated …", :43–49), so the count moves to the right of the Now line
and the slot stays one line plus the bar. Matthew approved this change from the mockup on
2026-11-15 (K7). Everything else follows `Card.dc.html`:

| Element | Style |
|---|---|
| `.progress` | `flex: none; height: 30px; display: flex; flex-direction: column; justify-content: flex-end; gap: 6px; min-width: 0` |
| `.progress-line` | `margin: 0; display: flex; justify-content: space-between; gap: 10px; font-size: 12px; color: var(--text-2); white-space: nowrap` |
| `.now` | `overflow: hidden; text-overflow: ellipsis; min-width: 0`; `b`: `var(--text)`, 600; time in `var(--font-mono)` |
| `.bar` | `display: flex; gap: 3px` |
| segment | `flex: 1; height: 5px; border-radius: 3px`; done `--status-ready`, current `--accent` with the 1.6 s opacity pulse, pending `--border-strong` |
| continuous | one 5px track in `--border-strong`, radius 3px, `overflow: hidden`; done width in `--status-ready`, then the current step's width in `--accent` |

Mockup hex: done `#00e676` → `--status-ready`; current `#1ec8f0` → `--accent`; pending `#1f2c3d` →
`--border-strong` (as in 16-1); text `#b4c0d0`/`#edf2f8` → `--text-2`/`--text`. Dusk follows from
the tokens. The card's top line and status colours don't change.

## 6. Edge cases

| Case | Behaviour |
|---|---|
| No plan, or a plan emptied by deletes | `progress: null`; the slot stays empty |
| Over 24 steps | Continuous bar |
| Stopped session with a plan | "Stopped at: …" or "Next: …", no time |
| Chorus restarted, resumed run | Seeded glance, no time, until the agent's next plan update |
| Fresh conversation after Restart | The session's last plan until a new one, as the panel |
| Reopened step | The card times the current interval; the panel the total |
| Several steps in progress | The first in plan order is "Now" |
| Codex | Published per batch already; the glance follows each update |
| Opencode | No plan channel: no bar (the panel explains why) |

## 7. Verification

Build checks: the Task's commands. Then, in the isolated instance (Overview §7):

1. Two Claude Code sessions in scratch folders. In one, prompt a 4-step task list worked one step at
   a time (as 16-1 §7). Leave the other without a plan.
2. On Overview, watch the first card: segments turn green as steps complete, within about a second
   of the panel; "Now:" names the step and its time ticks each second.
3. Watch the card's height and its grid row as the first plan arrives: nothing moves. Save before
   and after screenshots to `_verify/`.
4. The second card shows no bar and the same height as the first.
5. Exit the first session: "Stopped at: …" or "Next: …", no ticking.
6. Glacier, then Dusk.
