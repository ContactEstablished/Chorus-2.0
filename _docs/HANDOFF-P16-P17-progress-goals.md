# Roadmap: P16 Progress panel and P17 Goal mode

Written 2026-11-14 from D115–D118. P16 is done (2026-11-15); P17 is next and provisional until its
own kickoff. P16's task docs are in [P16/](P16/Phase-16-Overview.md), from its kickoff on 2026-11-15.

## 1. Purpose

**P16 — Progress panel.** Friendly gets a panel pinned at its top that shows the whole session's
plan: which steps and skills are done, which is current and which come next, and roughly how long
each took. A glance should be enough to see that something looks wrong ("Architect ran for 30
seconds"). Exact times don't matter; rounded ones are enough.

**P17 — Goal mode, the keep-going loop.** A session can carry a standing goal. After each turn
Chorus decides whether to send a continue message, so the agent keeps working instead of stopping to
ask when it could work out how to continue. P16 comes first so goal mode is watched as it is built.

**Contract:**

- P16 adds no new agent behaviour. It only records what Chorus already receives, and displays it.
- P17 never changes a session's permissions. A goal session keeps the permissions it launched with.

## 2. Source of truth

- **Decisions:** D115–D118 in [DECISIONS.md](DECISIONS.md). If this roadmap and a decision
  disagree, the decision wins until it is changed.
- **Goal mode's design source:** OpenMausBot's `docs/plans/2026-09-02-goal-mode-v2.md`
  ([repo](https://github.com/milind-soni/OpenMausBot), Apache-2.0). Ideas are adapted, not code
  copied.
- **Code:** the files in §3.
- **Styling:** [design/DESIGN.md](design/DESIGN.md) and the theme tokens it names.

## 3. Verified ground facts

Re-check a fact before relying on it; the code moves.

| # | Fact | Checked |
|---|---|---|
| F1 | `crates/chorus-core/src/progress.rs`: `PlanStep { id, text, status }` has no timestamps. `claude_plan` reads PostToolUse for `TodoWrite`, `TaskCreate` and `TaskUpdate`; `plan()` also parses Codex-style `step` items. Statuses are pending, in-progress and completed; at most 100 steps | verified 2026-11-14 |
| F2 | `src-tauri/migrations/0017_p6_progress.sql`: table `turn_progress(id, turn_id, source_key, body, at)`. Each plan snapshot is already stored per turn with an `at` time. But `ProgressEntry { id, content }` in `store/history.rs` does not pass `at` to the front end | verified 2026-11-14 |
| F3 | `src/views/session/FriendlyTimeline.vue:173` renders `latestPlan(turn)` (from `src/views/session/conversation.ts:93`) inside each turn. The plan is per turn, not per session | verified 2026-11-14 |
| F4 | `src-tauri/migrations/0005_history.sql`: `tool_calls(tool, target, started_at, ended_at, failed, turn_id)` and `asks(kind, asked_at, answered_at)`, with kind `approval` or `question`. Tools and asks already have timings | verified 2026-11-14 |
| F5 | `crates/chorus-core/src/events.rs:40`: `tool_target` picks `file_path`, `path`, `command`, `cmd`, `pattern`, `url`, `query` or `description`, not `skill`. A Claude Code `Skill` call has no target, so the skill's name is lost in display. Nothing in the code names the `Skill` tool yet | verified 2026-11-14 |
| F6 | `crates/chorus-core/src/activity.rs`: `Activity` is Unknown, Working, AwaitingApproval, NeedsAnswer, Ready or Failed. It is live state only; no table records activity changes over time (no migration has one). The latest migration is `0025_turn_usage.sql` | verified 2026-11-14 |
| F7 | `src-tauri/src/run_loop/tools.rs`: P15's loopback tool server already serves `ask`, `log_assumption` and `task_done`, authorised per step with a token (`issue`, `authorised`, `config_file`). It is scoped to loop steps, not ordinary sessions | verified 2026-11-14 |
| F8 | `crates/chorus-core/src/hooks.rs` already writes a Claude Code `Stop` hook (checked in tests around line 81). Chorus's hook is a notifier today, never a blocker | verified 2026-11-14 |
| F9 | `src/views/SessionCard.vue` (265 lines) is the Overview card | verified 2026-11-14 |
| F10 | Installed Claude Code is 2.1.316. Codex is not on PATH in this shell; re-check before P17's spike | verified 2026-11-14 |
| F11 | `src-tauri/src/events/recorder.rs:205`: each Claude Code plan change is its own row, keyed by `tool_use_id`, with `at` from Chorus's clock. Codex's `turn/plan/updated` uses one key per turn, `codex-plan` (`crates/chorus-core/src/codex_stream.rs:85`), and `history::progress` upserts with `ON CONFLICT … DO UPDATE SET body`, keeping the first `at`. So Codex's step changes within a turn have no stored times today. A turn keeps at most 500 progress rows | verified 2026-11-14 |
| F12 | `turns` (`0005_history.sql`) has `started_at` and `ended_at`, so turn spans are already stored | verified 2026-11-14 |

## 4. Global decisions and gates

| Decision | Status | Summary |
|---|---|---|
| D115 | RESOLVED 2026-11-14 | A pinned Progress panel comes first, as P16: the whole session, each step with a status, skill chips, rounded durations from Chorus's clock, sub-steps folded, per-turn plan boxes kept as history |
| D116 | RESOLVED 2026-11-14 | P16 also gets a time split, an Overview card bar and "usual time" markers, in that order, after the panel |
| D117 | RESOLVED 2026-11-14 | Goal mode is P17, after P16: the agent's verdict comes through a Chorus tool, guards pause and never stop, permissions never change |
| D118 | RESOLVED 2026-11-14 | Spike Claude Code's Stop hook first. Other CLIs, and Claude if the spike fails, get a continue message |

| Gate | Rule |
|---|---|
| G1 | Seen working in the real app (CLAUDE.md). Compiled and unit-tested is not done |
| G2 | Rows come only from structured channels: the agent's checklist, skill calls and Chorus's own steps. Never screen text |
| G3 | Durations come from Chorus's clock when each change arrives. A step that can't be timed says "not timed" (fail visible) |
| G4 | Styled with theme tokens, in Glacier and Dusk. Matthew approves a mockup before slice 16.1's code |
| G5 | Pure logic lives in `chorus-core` with unit tests: durations, the time split, the usual-time comparison, the goal state machine and its guards |
| G6 | The Stop-hook spike's result is recorded with the CLI version before P17 relies on it |
| G7 | Checks before each commit, per CLAUDE.md: `cargo test`, `cargo clippy -- -D warnings`, `npm run typecheck`, `npm test` and the secret scan |

## 5. Phases

### P16 — Progress panel

**Status: done 2026-11-15** on `feature/progress-and-goals`, not yet merged to `main`. Task docs in [P16/](P16/Phase-16-Overview.md). Q1–Q6 were settled at kickoff (D119–D122). Each slice's exit criterion was seen in the real app (isolated `p16` instance, Claude Code 2.1.316, Glacier and Dusk): 16.1 the pinned panel (`26c72f0`), 16.2 where the time went (`d3d2832`), 16.3 the Overview card bar (`31ddd3b`) and 16.4 usual-time markers (`d019c78`). Implementation choices made on the way are D124–D127. Not checked: Codex, which isn't installed here.

Display rules for every slice:

- Durations are rounded: "5 min", "1.5 h". Seconds only under one minute ("40 sec").
- A skill's time runs from its call until the next step starts, and is labelled as an estimate.
- A step the agent marked done without being seen in progress reads "not timed".

Slices, in order. Each ends seen working in the app; commit after each.

**16.1 The pinned panel.**

- `ProgressEntry` gains `at` (F2), through the generated bindings.
- A session-level read: the session's latest plan across its turns, with the snapshots behind it.
  Inputs are validated at the Rust boundary.
- A pure function turns successive snapshots into step times: a step starts at the first snapshot
  that shows it in progress and ends at the first that shows it completed.
- Codex's plan updates are kept so they can be timed (F11). How is decided at kickoff (Q2).
- Record the `Skill` tool's skill name (F5), so a step a skill drove gets a skill chip.
- The panel: a styled progress bar, each step with its status, skill chip and duration. The agent's
  sub-steps fold under the current step. The per-turn plan boxes (F3) stay as history.

*Done when:* in a real Claude Code session with a multi-step plan and a skill call, the panel shows
done, current and next steps with rounded durations, a skill chip with its time marked as an
estimate, and "not timed" for a step that went straight to completed. It reads the same after
Chorus restarts, and is seen in Glacier and Dusk. A Codex session shows its plan too, if Codex is
installed (F10).

**16.2 Time split.** The panel splits the session's time into agent working, waiting on you and
waiting on approval. Activity is live only (F6), so the history has to come from somewhere. Two
options, decided at kickoff (Q1):

- **(a) Record activity changes.** A new migration (the next free number) with one row per change,
  timed by Chorus's clock. Exact, and it sees states that turns and asks don't, such as Failed.
  Costs a write per change, and sessions from before it can't be split.
- **(b) Derive from turns and asks** (F4, F12). Approval waits come from `asks` of kind `approval`,
  question waits and the gaps between turns count as waiting on you, and the rest of each turn as
  working. No migration, and it works for past sessions. But it only sees what those tables hold,
  so failures and waits outside a turn are guesses.
- *Tentative recommendation: (a), because the split should show what happened, not a guess. Past
  sessions then say "not split" rather than show a made-up split.*

*Done when:* a real session that had a question and an approval shows three totals that add up to
its elapsed time, give or take rounding, and each wait matches what happened.

**16.3 Overview card bar.** A thin progress bar on the Overview card (`SessionCard.vue`, F9):
completed steps over all steps of the session's latest plan, in theme tokens.

*Done when:* Overview shows bars that move as steps complete, in both themes, and a card's layout
doesn't jump when its bar appears.

**16.4 "Usual time" markers.** Once a skill or step name has N past timed runs (N decided at
kickoff), a step far shorter or longer than its usual time gets a quiet marker: "30 sec, usually
~18 min". It is a marker, not an alarm: no notification and no alarm styling. The comparison is pure
logic in `chorus-core`. Untimed steps never count towards a usual time.

*Done when:* after N timed runs of one skill, a much shorter run shows the marker, a name with fewer
runs shows none, and both are seen in the app.

### P17 — Goal mode (PROVISIONAL)

**Not authoritative until its own kickoff, after P16 closes.** Re-check every fact in §3 first.
Exit criteria are written at kickoff. The outline, in order:

- **17.0 Spike (D118), no product code.** Can the installed Claude Code's `Stop` hook refuse the end
  of a turn and pass a reason back? Today Chorus's hook only notifies (F8). Record the result and
  the CLI version in `spikes/P17.md`. Check whether Codex is installed now (F10).
- **17.1 Goal record and state machine.** A durable goal per session (a new migration), and its
  state machine in `chorus-core`, with tests. Every paused state carries its reason.
- **17.2 `goal_status` tool.** P15's loopback tool server (F7) is extended from loop steps to
  ordinary goal sessions, with a per-session token kept out of argv. The agent reports `continue`,
  `completed`, `needs-input` or `blocked` (D117).
- **17.3 Continue.** After each turn Chorus decides whether to continue. Claude Code continues
  inside the turn if 17.0 passed. Every other CLI, and Claude if it failed, gets Chorus's continue
  message through the normal delivery path, persisted and scrubbed like every prompt.
- **17.4 Guards (D117).** Guards pause, never stop, and every pause names its reason in the UI.
  - A 20-turn budget, then pause.
  - 3 turns with no tool calls, then pause.
  - A nudge on repeated replies or tool calls.
  - A nudge when a turn ends by handing back while the verdict is still `continue`.
  - A first `blocked` is answered with "name the blocker and try one alternative".
  - Your own message pre-empts the goal.
  - Goals are restored as paused after a restart.
- **17.5 Acceptance criteria.** A checklist written at goal start says what "done" means. It feeds
  the P16 panel as top-level rows, with the agent's own steps folded under them.
- **17.6 Checks before "completed".** Where the project has checks, P15's gatekeeper runs them
  before a goal is marked completed.
- **Later slices:** transient-error retry, and repeated-tool-call detection beyond 17.4's nudge.

## 6. Out of scope

- OpenMausBot's chat-app layer, Composio, voice, cloud desktops, auto routing and memory system.
- A judge model. The agent reports its own verdict through the tool (D117).
- Changing a session's permissions, in either phase.
- P11's Usage screen (on hold, D102). P16 records step times; it doesn't build Usage.

## 7. Open questions for kickoff

Q1–Q6 were settled at P16's kickoff on 2026-11-15: Q1 by D119, Q2, Q3 and Q6 by D122, Q4 by D120, Q5 by D121. Q7 and Q8 stay open for P17.

1. **Time split source** (16.2): record activity changes, or derive from turns and asks?
2. **Codex step times** (F11): one row per plan update, or step times kept as updates arrive?
3. **Skill name** (F5): add `skill` to `tool_target`, or keep it in its own field? Is a skill a chip
   on the step in progress when it was called, and its own row when there is no plan?
4. **Sub-steps:** what folds under the current step, the agent's checklist items or its tool calls?
5. **Usual time** (16.4): N, how far off counts as "far", and how step names match across runs
   (skill names are stable, free-text steps are not).
6. **Overview bar** (16.3): hidden, or labelled "no plan", for a session with no plan?
7. **P17:** how a goal and its acceptance criteria are set, and whether a goal outlives a resumed
   session.
8. **P17:** what a failed check does before "completed": pause, or send the failure back as the next
   continue message?
