# Implementation spec 16-1: The pinned panel

Read [Task-16-1.md](../Tasks/Task-16-1.md) first. Line numbers were verified on 2026-11-15 at
`c5ee0ff`; re-check before editing.

## 1. Data flow

Plan snapshots, runs and `Skill` calls → `step_times::compute` (pure) → `SessionManager::progress`
→ `session_progress` → `FriendlyTimeline`'s 1.5 s read → `ProgressPanel`, which only words and ticks.

## 2. `crates/chorus-core/src/step_times.rs` (new, pure)

Inputs (no `Type`; never exported):

```rust
pub struct PlanSnapshot { pub at_ms: i64, pub run_id: Option<u32>, pub steps: Vec<PlanStep> }
pub struct RunSpan { pub id: u32, pub started_ms: i64, pub ended_ms: Option<i64> }
pub struct Call { pub id: u32, pub turn_id: Option<u32>, pub run_id: Option<u32>, pub turn_ended_ms: Option<i64>,
    pub tool: String, pub target: Option<String>, pub started_ms: i64, pub ended_ms: Option<i64>, pub failed: bool }
pub struct Window { pub from_ms: i64, pub to_ms: Option<i64> }   // to_ms None: still running
pub struct SkillEstimate { pub call_id: u32, pub turn_id: Option<u32>, pub name: Option<String>,
    pub started_ms: i64, pub ended_ms: Option<i64> }           // ended_ms None: running
pub const SKILL_TOOL: &str = "Skill";
pub const MAX_SNAPSHOTS: u32 = 2000; pub const MAX_SKILL_CALLS: u32 = 200;
pub const MAX_WINDOW_CALLS: u32 = 500; pub const MAX_SKILL_ROWS: usize = 50;
```

Outputs (`Serialize`, `Deserialize`, `Type`, `rename_all = "camelCase"`; enums `kebab-case`):

```rust
pub enum StepStatus { Pending, InProgress, Completed }            // from_plan(&str) -> Option<Self>
pub enum RowKind { Step, Skill }
pub struct SessionProgress { pub steps: Vec<TimedStep>, pub current: Option<u32>, pub next: Option<u32>,
    pub done: u32, pub total: u32,         // Step rows only
    pub current_tools: Vec<ToolGroup>, pub plan_at_secs: Option<u32>, pub capped: bool }
pub struct TimedStep { pub kind: RowKind, pub text: String, pub status: StepStatus,
    pub started_at_secs: Option<u32>,      // first in progress, this plan
    pub secs: u32, pub running_since_secs: Option<u32>,   // closed time; open interval in the live run
    pub timed: bool,                       // false: completed, never seen in progress
    pub skills: Vec<SkillUse> }
pub struct SkillUse { pub call_id: u32, pub name: Option<String>, pub at_secs: u32,
    pub secs: u32, pub running_since_secs: Option<u32> }     // always an estimate
pub enum ToolKind { Read, Edit, Run, Search, Web, Agent, Other }
pub struct ToolGroup { pub kind: ToolKind, pub tool: String, pub count: u32, pub target: Option<String>,
    pub secs: u32, pub running_since_secs: Option<u32>, pub failed: u32 }
```

TypeScript: `StepStatus = "pending" | "in-progress" | "completed"`, `RowKind = "step" | "skill"`,
camelCase fields (`runningSinceSecs: number | null`). `Snapshot` and `StepState` are taken names.

```rust
pub fn compute(snapshots: &[PlanSnapshot], runs: &[RunSpan], live_run: Option<u32>, skill_calls: &[Call])
    -> (SessionProgress, Option<Window>);   // Window: the current step's last interval
pub fn skill_estimates(snapshots: &[PlanSnapshot], runs: &[RunSpan], live_run: Option<u32>, skill_calls: &[Call])
    -> Vec<SkillEstimate>;                  // compute uses it; 16-4 reuses it, so one rule
pub fn tool_groups(calls: &[Call], window_open: bool) -> Vec<ToolGroup>;
```

Rules, in order (each has a test in Task 16-1):

1. **Order.** Sort snapshots by `at_ms`, keeping input order for ties. Unknown statuses read pending.
2. **Matching (K1).** The k-th step with text T in a snapshot is the k-th step with text T in the
   one before (text compared after `trim`). Ids are not used: TodoWrite and Codex have none, so
   `plan()` falls back to the index (`progress.rs:34`). A TaskUpdate rename starts a new step.
3. **Plan epochs (K2).** A snapshot that shares no text with the previous one starts a new plan. Only
   the newest plan's snapshots are used. Rows are the newest snapshot's steps, in its order.
4. **Intervals (K4).** A step opens an interval at the first snapshot that shows it in progress, and
   closes it at the first later snapshot that shows it otherwise or drops it. An interval also closes
   at its run's end: `ended_ms`, else the next run's `started_ms`. Only an interval in `live_run` stays
   open (`running_since_secs`). `secs` sums closed intervals; `started_at_secs` is the first opening.
5. **Not timed (G3).** A completed step never seen in progress has `timed: false` and `secs: 0`. So
   several steps marked done in one update read "not timed".
6. **Starting over.** A step that goes from completed back to pending loses its time. Completed back
   to in progress opens a second interval.
7. **Current and next.** `current` is the first in-progress row; `next` the first pending row after
   it, or the first pending row when none is current. `done`/`total` count `Step` rows.
8. **Skills (D122, K3).** Use `Skill` calls made after the previous plan's last snapshot, so a skill
   called just before the agent writes its list still counts (all calls when this is the first plan;
   the newest `MAX_SKILL_ROWS` when there is no plan). A call's estimate ends at the first of: a step opening an
   interval after it (removed steps count), the next `Skill` call, its turn's end. With none, it runs
   if its run is `live_run`, else it ends at its run's end. Attach it as a chip to the first row whose
   interval contains its time; else to the row that opens the next interval; else make it a
   `RowKind::Skill` row (text = name), placed after the plan's rows in time order, status completed or
   in progress. A call with no target keeps `name: None`.
9. **Tool groups (D120).** For calls in the window: skip `Skill`, `TodoWrite`, `TaskCreate`,
   `TaskUpdate`, `TaskList`, `TaskGet` and `ToolSearch`. Kind: `Read` → read; `Edit`, `MultiEdit`,
   `Write`, `NotebookEdit`, `fileChange` → edit; `Bash`, `PowerShell`, `commandExecution` → run;
   `Grep`, `Glob` → search; `WebFetch`, `WebSearch` → web; `Agent`, `Task` → agent; anything else →
   other, grouped per tool name. Groups keep first-seen order. `target` is set only when every call in
   the group shares it. `secs` sums finished calls; unfinished calls set `running_since_secs` only when
   `window_open`. Names verified against local Claude Code 2.1.316 transcripts and `codex_stream.rs:119`.
10. **Units.** Instants: `(ms.max(0) / 1000)` as `u32`, saturating. Durations: round to the nearest
    second. No clock is read: "now" is the caller's, and the UI adds `now - runningSinceSecs`.

## 3. Small core edits

- `events.rs:58`: insert `"skill"` before `"description"`. A Claude `Skill` call's target becomes
  its name, for example `anthropic-skills:pdf`. Test `a_skill_call_is_named_by_its_skill`.
- `codex_stream.rs`: add `plans: u32` and `last_plan: Option<Vec<PlanStep>>` to `Stream` (:48); the
  reset at :63 clears them per turn. At :84–86, when the steps differ from `last_plan`, bump `plans`
  and push key `format!("codex-plan-{}", self.plans)`; skip an identical update (saves rows under the
  500 cap). Old rows keep `codex-plan`.

## 4. Store and back end

`store/history.rs`: `ProgressEntry` gets `#[serde(rename_all = "camelCase")]` and `pub at_secs: u32`;
the query at :228 becomes `SELECT id,body,at FROM turn_progress WHERE turn_id=?1 ORDER BY at,id`.
`ToolEntry` gets `started_at_secs: u32` and `ended_at_secs: Option<u32>`; the query at :225 also
selects `started_at, ended_at`. Use the existing `secs()` (:196).

`store/progress.rs` (new; `pub mod progress;` in `store/mod.rs`). A plan body starts with
`{"kind":"plan"` (serde writes the tag first; a test pins it). Queries:

```sql
-- inputs(): plan snapshots, newest first, reversed in Rust (limit MAX_SNAPSHOTS)
SELECT tp.at, tp.body, t.run_id, t.id FROM turn_progress tp JOIN turns t ON t.id = tp.turn_id
WHERE t.session_id = ?1 AND tp.body LIKE '{"kind":"plan"%' ORDER BY tp.at DESC, tp.id DESC LIMIT ?2;
-- inputs(): runs
SELECT id, started_at, ended_at FROM runs WHERE session_id = ?1 ORDER BY started_at, id;
-- inputs(): skill calls, newest first, reversed in Rust (limit MAX_SKILL_CALLS)
SELECT c.id, c.turn_id, t.run_id, t.ended_at, c.tool, c.target, c.started_at, c.ended_at, c.failed
FROM tool_calls c JOIN turns t ON t.id = c.turn_id
WHERE t.session_id = ?1 AND c.tool = 'Skill' ORDER BY c.started_at DESC, c.id DESC LIMIT ?2;
-- calls_between(): the current step's window (limit MAX_WINDOW_CALLS)
SELECT c.id, c.turn_id, t.run_id, t.ended_at, c.tool, c.target, c.started_at, c.ended_at, c.failed
FROM tool_calls c JOIN turns t ON t.id = c.turn_id
WHERE t.session_id = ?1 AND (t.ended_at IS NULL OR t.ended_at >= ?2)
  AND c.started_at >= ?2 AND (?3 IS NULL OR c.started_at < ?3) ORDER BY c.started_at, c.id LIMIT ?4;
-- capped(): a turn at the 500-row cap (the newest snapshot's turn, and the newest turn)
SELECT EXISTS (SELECT 1 FROM turn_progress WHERE turn_id = ?1 LIMIT 1 OFFSET 499);
-- latest_plan(): the newest plan in the newest 20 turns, for seeding
SELECT body, at FROM turn_progress WHERE turn_id IN
  (SELECT id FROM turns WHERE session_id = ?1 ORDER BY id DESC LIMIT 20)
  AND body LIKE '{"kind":"plan"%' ORDER BY at DESC, id DESC LIMIT 1;
```

They use `turns_session`, `tool_calls_turn`, `turn_progress_order` and `runs_by_session`. A body that
doesn't parse is skipped, as `recent_turns` does (:230).

`sessions/progress.rs` (new; `mod progress;` beside `mod tests;` at `sessions/mod.rs:2014`): `impl
super::SessionManager { pub fn progress(&self, id: u32) -> Result<SessionProgress, String> }`. It
calls `self.row(id)?` first (an unknown id gives "no session N"), takes `live_run` from
`self.live` → `current_run()` → `id`, then runs `inputs`, `compute`, `calls_between` for the window,
`tool_groups` and `capped`, and nothing else. A child module may use `row`, `live` and `store`.

`sessions/commands.rs`, after `session_history` (:294): `session_progress(state: Manager<'_>,
session_id: u32) -> Result<chorus_core::step_times::SessionProgress, String>`. Register it after
`s::session_usage` (`ipc.rs:67`). Serde checks the shape; `row()` checks the id; there are no other
inputs.

Restart seed: `Recorder::seed_plan(&mut self, steps: Vec<PlanStep>)` sets `self.plan` only if empty.
In `sessions/mod.rs`, after the `Run` literal closes (:1305) and before `*live.run.lock()` (:1306):
when `resumed`, read `store::progress::latest_plan` and seed. A fresh conversation is never seeded:
its TaskCreate ids restart at "1" and would overwrite old steps.

## 5. Front end

`src/views/session/progress.ts` (pure, with `progress.test.ts`):

| Function | Returns |
|---|---|
| `stepDuration(secs)` | under 60 s: "40 sec" ("<1 sec" for 0); under 59.5 min: "5 min"; under 10 h: one decimal, "1.5 h", "1 h"; else "12 h" |
| `elapsedLabel(secs)` | the header's "1 h 04 min", "47 min", "40 sec" (mockup) |
| `liveSecs(t, now)` | `t.secs + max(0, now - t.runningSinceSecs)` when running |
| `toolGroupLabel(g)` | "Read 4 files", "Edited 2 files", "Ran cargo test" (one target) or "Ran 3 commands", "Searched 5 times", "Fetched 2 pages", "Ran 1 subagent", "PowerShell ×3" for other |
| `toolSummary(groups)` | labels joined with " · ", lower-case after the first: "Read 4 files · ran cargo test" |
| `nowNext(p, now)` | `{ now, nowSecs, next }` for the collapsed line |
| `emptyLine(agent)` | "No plan yet. Steps appear when the agent writes a checklist or runs a skill." For opencode: "Chorus doesn't read opencode's checklist yet, so there is no plan here." |

`ProgressPanel.vue`, props `{ session: SessionSummary; progress: SessionProgress | null; error: string }`,
`now = useNow(1000)`, local `open = ref(true)` and `toolsOpen = ref(false)` (folded, D115). Structure,
matching `Main.dc.html`:

- `section.panel[aria-label=Progress]`: `--surface`, 1px `--border`, `--radius-card`, shadow
  `0 8px 28px rgb(0 0 0 / 0.28)` (literal, like `.jump` at `FriendlyTimeline.vue:317`).
- Head (padding 16px 20px 12px, gap 14px, wraps): eyebrow "Progress" (11px, 600, 0.08em, upper,
  `--accent`); `h2` = `session.description || session.name` (heading font, 19px, 600); status pill
  with `st-${session.attention}` (text `var(--st)`, background `var(--st)` at 13%, 8px dot, pulse when
  working); figures: `elapsedLabel(now - runStartedAtSecs)` (mono 17px/500, only when live) over
  "4 of 9 done" (12px `--text-2`, `title` "Plan updated 9:47"); Hide/Show (`aria-expanded`, 1px
  `--border-strong`, transparent, `--text-2`, `--radius-control`, 6px 12px, min-height 34px).
- Segments (`aria-hidden`, padding 0 20px 16px, gap 4px, or 1px over 30 steps): one per step row,
  8px high, radius 4px. Done `--status-ready`, current `--accent` with pulse, pending `--border-strong`.
- Open: `ol.steps.scroll` (padding 6px 12px 10px, border-top, `max-height: min(45vh, 520px)`). Row:
  radius 10px, grid `26px minmax(0,1fr) 64px 92px`, gap 12px, padding 9px 10px; current row background
  `color-mix(in srgb, var(--accent) 7%, transparent)`. Icon (20px SVG, `role=img`, aria-label Done / In
  progress / Up next): done = circle filled `--status-ready` at 13%, stroke and tick `--status-ready`;
  current = ring `--accent-line`, pulsing 4.5px dot `--accent`; pending = dashed ring
  `--border-strong` (3 3). Name 14px, 500 (600 current), `--text-2` when pending. Skill chip: mono
  11.5px, `--tag-lavender` on it at 14%, radius 6px, padding 2px 7px, 11px star, text "name ~12 min",
  `title` "Skill name, about 12 min. Estimate: from the skill call until the next step started."
  Unnamed: "skill", title "Name not recorded". Started: `timeLabel(startedAtSecs)` (mono 12px
  `--text-2`). Duration (mono 13px): `stepDuration(liveSecs)`; current in `--accent`; "not timed"
  italic `--text-muted`; a stopped in-progress step adds " · stopped" in `--text-muted`, no pulse.
- Under the current row, when `currentTools` is not empty: a quiet button with `toolSummary` and a
  chevron (`aria-expanded`); unfolded, `ol` (margin 0 10px 8px 48px, padding 4px 0 4px 14px, 2px left
  border `--accent-line`) of rows, grid `16px 1fr 92px`, 13px: 8px dot (`--accent` running, else
  `--status-ready`; `--status-failed` if any failed), label, time.
- `capped`: a muted note under the list, "Some plan updates weren't stored (500 a turn), so later
  times may be missing." `error`: a `role=alert` line, "Progress could not refresh: …", `--status-failed`.
- Closed: one line (padding 10px 20px 14px, border-top, 13px): pulsing `--accent` dot, "Now:" in
  `--text-2`, the step in 600, its time in mono 12px, "· Next: …". No current step: "All N steps
  done", or the next step alone.
- No rows: the closed line shows `emptyLine(session.agent)` and Hide is not shown.
- The footer, "Where the time went", is 16-2's: four parts (agent working, waiting on you, waiting
  on approval, Idle) after the list in the open panel. 16-1 ends the open panel at the list.

Token map (mockup hex → token; no new tokens, no hex in the component):

| Mockup (Glacier / Dusk) | Token |
|---|---|
| `#111925` / `#22262e`, `#182231` / `#2a2f39` | `--surface`, `--surface-chip` |
| `#1d2a3a` / white 7%, `#2a3a50` / white 14% | `--border`, `--border-strong` (Dusk tokens are 6% and 12%) |
| `#edf2f8`, `#b4c0d0`, `#8e9bb0` (and Dusk) | `--text`, `--text-2`, `--text-muted` (token muted is a little darker) |
| `#1ec8f0` / `#4f8cff` and its 12% and 38% | `--accent`, `--accent-soft`, `--accent-line` |
| working `#4d7cff` / `#5ccfdc` | `--status-working` (soft: 14% `color-mix`) |
| ready `#00e676` / `#86d4a4` | `--status-ready` (soft: 13%) |
| lavender `#b9c3fd` / `#b4b8f8`, soft at 13–14% | `--tag-lavender`, background at 14% |
| pending segment `#1f2c3d` / `#3a404c` | `--border-strong` (nearest in both themes) |
| current row tint, 7–8% accent | `color-mix(in srgb, var(--accent) 7%, transparent)` |
| radii 14, 9, 999 | `--radius-card`, `--radius-control`, `--radius-pill` |

Pulse: a scoped `@keyframes` on opacity, 1.6 s (DESIGN.md Motion); `tokens.css:322` stops it under
reduced motion.

`FriendlyTimeline.vue`: refs `progress` and `progressError`; in `load()`, after the narrator block
(:93–97), a separate try/catch reads `commands.sessionProgress(id)` with the same generation guard
and the same "Restart Chorus 2 to load the updated … backend" message; reset both in the session
watch (:116–120). Template: first child of `.thread` (:126), `<div v-if="session.hasEventChannel"
class="pinned"><ProgressPanel … /></div>`. CSS: `.pinned { flex: none; padding: 14px clamp(18px, 3vw,
40px) 0 }`, `.pinned > * { max-width: 920px; margin: 0 auto }`, 14px side padding under 720px.

## 6. Edge cases

| Case | Behaviour |
|---|---|
| Several steps done in one update | Each never seen in progress reads "not timed" (rule 5) |
| Plan replaced wholesale | New epoch; old times dropped (rule 3) |
| Steps removed | Not shown; their interval starts still end skill estimates |
| Over 100 steps | Not stored (`progress.rs:32`, `:61`); the panel shows the last stored plan, and the count's tooltip says when it was updated |
| 500 rows in a turn | Later updates in that turn aren't stored; `capped` shows the note |
| Restart mid-step | Interval cut at the run's end; resumed runs merge TaskUpdate into the seeded plan; a new snapshot opens a new interval |
| Clock | Plan `at` is Chorus's: `record()` passes `now_ms` (`recorder.rs:209`), the Codex bridge `now_ms()` (`codex_bridge.rs:150`) |
| Old Codex rows (`codex-plan`) | One snapshot per turn; steps done inside it read "not timed" |
| Old `Skill` rows (no target) | Chip "skill", tooltip "Name not recorded" |
| Skill estimate label | Always "~" before the time, and the tooltip says it runs from the call to the next step (D115) |

## 7. Verification

Build checks: the Task's commands. Then, in the isolated instance (Overview §7), scratch folder:

1. Claude Code session. Prompt: "Create a task list with four tasks. Work through them one at a
   time, marking each in progress and then completed. In task 2, use the <skill> skill. Mark task 4
   completed directly, without marking it in progress." Pick a short skill from `~/.claude/skills`.
2. The current row ticks; its tool summary unfolds with times; the chip shows "~" and its tooltip;
   task 4 reads "not timed"; Hide and Show work. Starts agree with the timeline's turn times.
3. Quit Chorus mid-step and reopen: the same rows and times; the stopped step didn't grow meanwhile.
4. Glacier, then Dusk (screenshots to `_verify/` only). A shell session shows no panel; opencode
   shows its line. If `codex --version` works, a Codex plan shows with times.
