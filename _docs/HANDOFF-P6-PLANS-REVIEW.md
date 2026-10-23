# P6 plans and intermediate messages review

## 1. Verdict

Partly done: Claude checklist/message capture, persistence and Friendly components are implemented;
full-app acceptance and other adapters' capture remain open. This is not P6 completion.

## 2. Branch

- Branch: `work/p6-plans`; base: `6fafc074a967727235c52a6d342e44ef2414aad8`.
- Implementation: `761590a P6: persist Claude checklists and visible progress messages`.
- Final commit: the review commit at this branch's tip. Inspect `git log --oneline 6fafc07..work/p6-plans`.
- The parent explicitly selected the current P6 HEAD as baseline; no rebase onto older `main`.

## 3. Files

| Created | Purpose |
|---|---|
| `crates/chorus-core/src/progress.rs` | Pure task normalisation, bounded checklist updates, visible assistant text selection and tests |
| `src-tauri/migrations/0015_p6_progress.sql` | Checklist snapshots, durable incompleteness markers and assistant messages |
| `src-tauri/src/store/progress.rs` | Transactional plan updates, replay dedupe, latest plan and bounded message reads |
| `src-tauri/src/events/progress.rs` | Scrubbed hook/transcript capture, shared bounded cursor, integration tests |
| `src-tauri/src/sessions/progress.rs` | Validated read-only typed IPC |
| `src/views/session/FriendlyPlan.vue`, `FriendlyMessages.vue` | Checklist and plain-text progress display |
| This review | Acceptance limits and integration instructions |

| Modified | Purpose |
|---|---|
| `crates/chorus-core/src/actions.rs`, `lib.rs` | Read-only Observed capability and pure module registration |
| `src-tauri/src/agents/mod.rs` | Enable Claude task tools in the child environment only |
| `src-tauri/src/events/mod.rs` | Capture module registration |
| `src-tauri/src/events/recorder.rs` **HOT** | Capture state, sibling-only field access, capture before turn transitions and final drain at run end |
| `src-tauri/src/sessions/mod.rs` **HOT** | Progress module; reuse bounded line reader; poll progress alongside the existing interrupt watcher |
| `src-tauri/src/store/mod.rs`, `ipc.rs` | Migration/module and command registration |
| `src/ipc/bindings.ts`, `index.ts` | Regenerated bindings and type exports |
| `src/views/session/FriendlyTimeline.vue` | Load progress separately from history/replies, component tags and capability notice |
| `src/views/session/ActionChannels.vue` | Explain observed checklist capability |

No deleted files or dependency changes. No shared roadmap/design documentation edited.

## 4. Surface added

- Migration `0015_p6_progress.sql`: `plan_snapshots` (session/run/turn, event identity, steps JSON,
  timestamp); `plan_capture_issues` (run identity); `progress_messages` (session/run/turn, transcript
  UUID, scrubbed text, timestamp). Migration number must be reconciled with parallel branches.
- `session_progress(session_id)` checks session existence and returns `SessionProgress`: latest plan,
  up to 200 recent messages, current run, capability note and live capture error. No new events.
- `store::progress::latest_plan` returns `PlanSnapshot { id, run_id, turn_id, steps, incomplete }`.
  Narrator integration must require matching run and turn and `incomplete == false`.
- Claude launch adds `CLAUDE_CODE_ENABLE_TODO_TOOLS=1`; no global config or new settings keys.
- All new durable content lives in the existing app database. No new export or standalone runtime files.

## 5. Plan against result

| Task | Result |
|---|---|
| Persist plans | Claude successful TaskCreate/TaskUpdate hooks captured, including deletes; each update has a durable replay identity |
| Persist intermediate messages | Claude main transcript assistant text only; reasoning, tool content and sidechains excluded |
| Friendly display | Implemented, including prior-run and incomplete-plan notices; full GUI runtime check still required |
| Preserve history/replies | Existing tests pass; progress IPC failure does not suppress successful history/reply loading |
| Other adapters | Explicitly unavailable; no Codex bridge or unverified rollout parser enabled |
| Full P6 acceptance | Not performed; parent owns integrated composer/narrator and Friendly-only acceptance |

## 6. Verification

`npm run bindings` and final `npm run check` passed: TypeScript, 19 frontend tests, 462 Rust tests
(71 app, 385 core, 3 purity, 3 hook), Clippy with warnings denied, and secret scan (7 patterns,
262 files before this review). Evidence: `_verify/bindings.log`, `_verify/check-final.log`.

Tests cover successful updates, unknown tasks/statuses and step bounds, replay of an old create after
completion, SQLite reopen, run isolation, malformed-event incompleteness, quoted/backslashed/newline
credential redaction, fast transcript text before Stop, transcript replay, thinking/sidechain exclusion,
and byte-safe truncation after scrubbing. Existing hook replies, delivery and history tests pass.

Two bounded Claude subscription probes ran in a scratch folder, with no file/shell/network tools.
The first created a task and changed pending → in_progress → completed. The second captured real
PostToolUse payloads through per-launch command hooks: TaskCreate returned `task: {id, subject}`;
TaskUpdate returned `success: true`, `taskId`, `updatedFields`, and `statusChange`. Both exited
successfully. Evidence: `_verify/task-probe.jsonl`, `_verify/hook-probe.txt`, `_verify/hook-shapes.json`.
These are CLI/schema observations, not GUI acceptance. Existing Claude transcript record keys were
also inspected without copying their contents into the repository.

**Not verified:** the new widgets in a running GUI; a complete planned task controlled entirely
from Friendly; crash/relaunch display in the real app; Codex/opencode/Grok/Gemini/Aider intermediate
capture. No app instance was launched by this branch, so no user app data or process was touched.

## 7. Versions verified against

Claude Code 2.1.308. Existing generated IPC bindings and locked project dependencies were used.
Codex Q19 remains the parent's separate investigation; no new claim about Codex is made here.

## 8. Proposed doc changes

Decision row (Proposed):

| P6-PLANS-D1 | 2026-10-22 | Persist Claude task-tool checklist snapshots and visible assistant transcript text. Enable task tools only in the launched child environment; keep thinking, tool output and sidechains out. Deduplicate by successful tool-call identity or transcript UUID, preserve incomplete-capture markers, and distinguish earlier-run plans. | Proposed | Friendly needs progress between final messages; replay, missing tasks and parser failures must not manufacture a complete plan. |

Lesson (proposed): Claude Code 2.1.308's successful TaskCreate PostToolUse response contains
`task.id`/`task.subject`; TaskUpdate contains `success`, `taskId`, `updatedFields`, and `statusChange`.
Assistant transcript records have a `uuid` per record; retain only `message.content` entries whose
type is `text`. A Stop can occur between polling ticks, so drain the transcript before advancing the
recorder's turn and again at run end. These observations do not establish another adapter's shape.

PLAN/ARCHITECTURE addition (proposed): Claude progress capture is implemented with durable checklist
snapshots and bounded assistant messages; real-app acceptance and other-adapter capture remain open.
Add the three migration tables to the history data-model description. No SCOPE/DESIGN change needed.

## 9. Questions for Matthew

None needed for this bounded implementation. Other-adapter support and Q19 remain explicitly open.

## 10. Risks and known issues

- Capture starts at the transcript's end at the first hook, intentionally leaving earlier resumed
  conversation text in the agent's transcript. It is not a full historical transcript importer.
- Native queued prompts can overlap. Ambiguous messages retain a null turn; Friendly labels them
  “Turn not identified”. This is conservative attribution, not complete merged event ordering.
- Each message is capped at 32 KiB after scrubbing; each checklist at 128 steps with 2 KiB titles.
  Unknown/oversized task updates visibly mark the plan incomplete. A transcript record over the 4 MiB
  reader limit pauses message capture visibly. Backlogs/truncation get a warning and no guessed turn.
- A resumed run that only updates a task created before the run cannot reconstruct the missing title;
  it reports incompleteness rather than importing the previous run's checklist as current.
- The latest checklist is displayed; older snapshots remain durable but have no separate browser yet.
  Intermediate messages show before each turn's collapsed tools, not interleaved with every tool event.
- Merge overlap: migration numbering, module/IPC registrations, generated bindings, actions capability
  enum, ActionChannels, FriendlyTimeline, and recorder/session call sites. Queue/narrator branches own
  replacing their remaining limitation text. Regenerate bindings after integration.

## 11. How to review

Read pure `progress.rs`, store queries/migration, capture module/tests, then the small recorder/session
diff and UI components. Verify narrator's current-run/current-turn/incomplete guards during integration.

For the real-app check, use the isolated `.dev.p6plans` identity and Vite port 1441 with strictPort (or
the parent's separately isolated integration identity). In a scratch Claude session, request two task
items, an intermediate sentence and sequential completion. Observe checklist updates and text in
Friendly, compare Raw, switch views, restart, and confirm the previous plan is labelled historical.
Exercise a question/approval to confirm progress polling does not alter live reply controls. Repeat
or replay a hook in an isolated harness to confirm no duplicate snapshot/message. Inject a malformed
task update and verify its incomplete warning remains after reopening the database. Do not use the
user's normal dev identity or mark P6 done from the CLI probes alone.
