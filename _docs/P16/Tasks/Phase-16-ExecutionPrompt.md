Execution prompt for Phase 16, Task 16-1 (the pinned progress panel). Paste the fenced block into a fresh Claude Code session opened at `C:\Projects\ContactEstablished\Chorus-2.0`.
Tasks 16-2, 16-3 and 16-4 each get this same prompt with their own task number swapped in, and each starts only after the previous task is committed.

````markdown
## 1. Role

You are the Coordinator for Chorus 2.0 Phase 16 (Progress panel), Task 16-1: the pinned panel.

Repo root: `C:\Projects\ContactEstablished\Chorus-2.0`. Expected branch: `feature/progress-and-goals`. Confirm it with `git branch --show-current`. Do not switch branches without instruction.

## 2. Goal

Friendly gets a panel pinned above the conversation showing the session's latest plan across all turns: each step's status, start time and rounded duration, "not timed" where it can't be timed, skill chips with estimated times, and a folded summary of the current step's tool calls with times. Hide collapses it to one line: "Now … · Next …".

Prime constraint: P16 adds no new agent behaviour. It records what Chorus already receives and shows it.

Done means seen working in the real app, not just compiled and tested.

## 3. Ground yourself first

Read these before editing, in this order:

1. `CLAUDE.md`
2. `_docs/P16/Phase-16-Overview.md` (gates G1–G7, rules K1–K10, the isolated instance in §7, exit criteria in §8)
3. `_docs/P16/Tasks/Task-16-1.md`
4. `_docs/P16/ImplementationSpecs/ImplementationSpec-16-1.md`
5. `_docs/HANDOFF-P16-P17-progress-goals.md`: §3 (facts) and §5 (P16)
6. Decisions D115, D120, D122 and D123 at the bottom of `_docs/DECISIONS.md`
7. `_docs/design/DESIGN.md` and `src/styles/tokens.css`
8. `_docs/LESSONS.md`

Code areas (line numbers verified 2026-11-15 at commit c5ee0ff; re-check each before editing):
- `crates/chorus-core/src/progress.rs:8,30,34,42`
- `crates/chorus-core/src/events.rs:40,58`
- `crates/chorus-core/src/codex_stream.rs:48,63,85`
- `src-tauri/src/store/history.rs:160,162,165,177,225,228`
- `src-tauri/src/events/recorder.rs:84,87,189,205-210`
- `src-tauri/src/sessions/mod.rs:1249,1295-1306,2014`
- `src-tauri/src/sessions/commands.rs:294`
- `src-tauri/src/ipc.rs:66-67`
- `src/views/session/FriendlyTimeline.vue:83,103,126,173`
- `src/views/session/SessionView.vue:280`

Git checks: `git branch --show-current`, `git status --short`, `git log --oneline -3`.

## 4. Pre-existing changes

At kickoff the tree had an untracked `Python/` folder of unknown origin. Do not read into it, and do not change, delete, stage or commit it. Stage files by explicit path only, never `git add -A` or `git add .`. Do not revert or commit anything outside this task's files. (Earlier kickoff doc edits are committed before this prompt is used.)

## 5. Implementation scope

Exact Scope list from Task-16-1:

- Create `crates/chorus-core/src/step_times.rs`, `src-tauri/src/store/progress.rs`, `src-tauri/src/sessions/progress.rs`, `src/views/session/ProgressPanel.vue`, `src/views/session/progress.ts`, `src/views/session/progress.test.ts`.
- Edit `crates/chorus-core/src/lib.rs` (one pub mod line), `events.rs` (add "skill" to tool_target's keys before "description", plus test), `codex_stream.rs` (key per plan update `codex-plan-<n>`, identical updates skipped, plus test), `src-tauri/src/store/mod.rs` (one pub mod line), `store/history.rs` (at_secs on ProgressEntry with camelCase; started/ended secs on ToolEntry), `events/recorder.rs` (seed_plan), `sessions/mod.rs` (mod progress; resume seed), `sessions/commands.rs` (session_progress command), `ipc.rs` (register after s::session_usage), `src/views/session/FriendlyTimeline.vue` (mount as first child of .thread, read sessionProgress in load() in its own try/catch), `src/ipc/index.ts` (re-export types), `src/views/session/conversation.test.ts` (fixture fields only); `src/ipc/bindings.ts` only via `npm run bindings`.

Resolved decisions, quoted with dates:

- D115 (2026-11-14): pinned panel; rounded durations from Chorus's clock; "not timed"; skill time as an estimate; per-turn plan boxes kept.
- D120 (2026-11-15): under the current step, fold a summary of its tool calls with times; mockup approved.
- D122 (2026-11-15): one Codex progress row per plan update; tool_target reads skill; a skill is a chip on the step in progress, or its own row with no plan.
- D123 (2026-11-15): K1–K10 accepted, including K1 steps matched by text; K2 a snapshot sharing no text starts a new plan; K3 skill attribution and estimate end; K4 step time is the sum of in-progress intervals cut at run end; K5 duration wording in TypeScript with every number from chorus-core; K9 isolated instance label p16, port 1436; K10 one-line empty state, and opencode says Chorus doesn't read its checklist yet.

The spec's §5 token map is the look: no hex in the component, no tokens.css changes.

## 6. Strict non-goals

Do none of the following:

- No time split, activity table or migration (16-2).
- No Overview bar (16-3).
- No usual times (16-4).
- No goal mode (P17).
- No Usage screen (P11).
- Never infer state from screen text.
- No new dependency.
- No tokens.css changes.
- No edits to the per-turn plan boxes.
- No reading opencode's checklist.
- Do not hand-edit `src/ipc/bindings.ts`.
- Do not edit DECISIONS.md for K1–K10.
- Do not use `npm run dev` (it would migrate the shared dev database).

## 7. Required workflow

Work as coordinator:

1. Track steps with TaskCreate/TaskUpdate so Chorus's own panel shows them.
2. Implement in small steps: core first with tests, then store/back end, then bindings, then front end. Use subagents for independent work if useful.
3. Run a spec-compliance review against ImplementationSpec-16-1, then a code-quality review. Resolve every finding.
4. Run the verification in section 8.
5. Make ONE intentional commit for Task 16-1: a plain-English-first message, ending with the line `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
6. Push to the existing upstream of `feature/progress-and-goals` after committing (the user approved pushing this branch). Do not open a PR or merge to main unless asked.

## 8. Verification

From the repo root, as written:

```
cargo test -p chorus-core -- step_times:: events:: codex_stream::
cargo test -p chorus -- store::progress:: recorder:: store::history::
npx vitest run src/views/session/progress.test.ts
npm run bindings
npm run check
git status --short
```

`npm run check` runs typecheck, vitest, cargo test --workspace (which also fails on stale bindings via ipc::tests::bindings_are_current), clippy -D warnings and the secret scan.

Runtime gate (run it, don't just compile):

1. Create `_verify/tauri.p16.conf.json` (gitignored) with:

```json
{
  "identifier": "com.contactestablished.chorus2.dev.p16",
  "build": {
    "beforeDevCommand": "cargo build -p chorus-hook && npx vite --port 1436 --strictPort",
    "devUrl": "http://localhost:1436"
  }
}
```

2. Run `npx tauri dev --config _verify/tauri.p16.conf.json`.
3. In a scratch folder outside any repo, start a Claude Code session (Claude Code 2.1.316 here). Pick a short skill from `~/.claude/skills` and put its name where the prompt says `<skill>`. Prompt:

> Create a task list with four tasks. Work through them one at a time, marking each in progress and then completed. In task 2, use the `<skill>` skill. Mark task 4 completed directly, without marking it in progress.

Observe in the real app, in Glacier first, then in Dusk (Settings → Appearance, or palette "Switch to Dusk"):

- done/current/next rows with rounded durations and start times; the current row ticks; its tool summary unfolds with times.
- the skill chip shows "~" and its estimate tooltip.
- task 4 reads "not timed".
- Hide shows "Now … · Next …"; Show restores the full panel.
- quit Chorus mid-step and reopen: same rows and times, and the stopped step didn't grow.
- a shell session shows no panel.
- a session with no plan shows the one-line empty state, while per-turn plan boxes still show.
- opencode shows its line.
- Codex only if `codex --version` works; otherwise report "not checked".

Screenshots go to `_verify/` only, never committed (the repo is public).

## 9. Failure honesty

- If a verification command fails for an unrelated environment reason, capture the exact output, explain it, and do not claim success.
- If a spec fact is wrong (a line moved, a payload differs, e.g. the Skill hook payload's `tool_input`, which nobody has captured live yet), stop, record what you found, and adjust minimally or ask.

## 10. Final report

Report one status: DONE, DONE_WITH_CONCERNS, NEEDS_CONTEXT or BLOCKED. Then give:

- files changed;
- build and test results;
- runtime results, with exactly what was observed in each theme (Glacier, Dusk);
- review outcomes;
- confirmation of each non-goal;
- residual risks and anything not checked (Codex, colours by eye);
- final `git status --short`;
- commit SHA and push result.
````
