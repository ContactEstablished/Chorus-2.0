# P6 queue and composer review

## 1. Verdict

Implementation complete; real Claude/Codex UI acceptance remains unverified. This branch does not close P6.

## 2. Branch

`work/p6-queue`, based on `6fafc074a967727235c52a6d342e44ef2414aad8` (the current P6 baseline, not the older main line). Implementation commit: `1951c9219910bfe3b775fda993a76f3f2706506c`. The final commit on this branch adds this review. `git rebase main` reported already up to date; no base changes occurred.

```text
1951c92 P6: add durable Friendly instruction queue and Raw draft gate
6fafc07 P6: add Friendly history and expiring Claude reply cards
5e63815 P6: add action contracts and durable prompt delivery
c106c1d Pushing assets
36c8227 Worktrees: finish P5 reuse, recovery and Git summaries
aa69e2d Agents: fix Gemini folder trust and isolate Aider history
15e7b20 Docs: consolidate roadmap status and preserve phase handoffs
50a34a0 Docs: P14's entries numbered (D63-D67, L45-L48, Q25-Q27); review questions Q28-Q30
676d01d P14 main-line review: an Ollama address error, the Pair test, Aider's Manual
17c111e Sessions: Close says when git removed a worktree only in part (L44)
22ab16b Sessions: Close waits for the processes it ended before removing a worktree (L44)
```

## 3. Files

| Created | Purpose |
|---|---|
| `crates/chorus-core/src/queue.rs` | Pure reviewed-run, Ready-state and Raw-draft gate, with tests |
| `src-tauri/migrations/0015_p6_queue.sql` | Durable instruction queue |
| `src-tauri/src/store/queue.rs` | FIFO storage, idempotent enqueue, cancellation and rearm |
| `src-tauri/src/sessions/queue.rs` | Typed boundary, dispatch, snapshot and empty-prompt confirmation |
| `src/views/session/SessionComposer.vue` | Explicit send/queue controls, queue review and outcomes |
| `src/views/session/composerDrafts.ts` | Session-scoped unsent drafts across view switches; suggestions append |
| `src/views/session/composerDrafts.test.ts` | Draft preservation and suggestion staging tests |

| Modified | Purpose |
|---|---|
| `crates/chorus-core/src/actions.rs`, `lib.rs` | Queue capability and core module registration |
| `src-tauri/src/sessions/mod.rs` **hot file** | One worker call; per-run draft/error fields; Raw writes acquire action lock and invalidate empty-prompt assertion |
| `src-tauri/src/sessions/delivery.rs` | Shared locked delivery path, Ready/Raw gate and queue identity validation |
| `src-tauri/src/sessions/tests.rs` | Five queue integration tests and explicit empty-Raw setup for existing delivery fixtures |
| `src-tauri/src/store/delivery.rs` | Queue attempt and prompt receipt commit in the same transaction |
| `src-tauri/src/store/mod.rs` | Module and migration registration |
| `src-tauri/src/ipc.rs`, `src/ipc/bindings.ts`, `src/ipc/index.ts` | Commands, generated bindings and type exports |
| `src/views/session/FriendlyTimeline.vue` | Composer mount; remove obsolete queue limitation |
| `src/views/session/ActionChannels.vue` | Describe implemented composer and queue |

No files deleted. No shared roadmap, decision or lesson documents changed.

## 4. Surface added

Migration `0015_p6_queue.sql`: `instruction_queue` holds session/run/request identity, scrubbed text, queued/cancelled/attempted status, prompt receipt link and creation time. The reviewer may renumber this migration.

Commands:

| Command | Input | Output |
|---|---|---|
| `session_queue` | session ID | reviewed run, Raw assertion, blocker and saved entries |
| `enqueue_instruction` | session ID, run ID, request UUID, text | success or visible refusal |
| `change_queued_instruction` | session ID, instruction ID, optional reviewed run ID | cancel when null; explicitly rearm when a run is supplied |
| `confirm_raw_prompt_empty` | session ID, reviewed run ID | records the user's explicit assertion |

No events, settings, environment variables, dependencies, external files or global configuration added. Prompts and queue entries live in the existing app database. Unsubmitted composer drafts remain in memory.

Integration contract: `stageComposerSuggestion(sessionId, text)` in `composerDrafts.ts` appends to existing drafts with a blank line; it never sends. NarratorPanel can emit `draft(text)` to this helper. `SessionComposer` also exposes `setDraft(text)` with the same append policy.

## 5. Plan against result

- Durable queue and composer: implemented for Claude/Codex's existing verified prompt path; other adapters report an explicit limitation.
- FIFO and one instruction per confirmed pause: implemented. A prior unobserved delivery blocks the next one.
- Raw coordination: all user Raw IPC writes, including terminal paste and wall views, acquire the action lock and invalidate the empty-prompt assertion. No terminal editor inference is used.
- Cancellation: queued entries only; attempted entries cannot be represented as cancelled or unsent.
- Restart: original run identity persists. The stale head blocks later entries until explicitly reviewed/rearmed or cancelled. Every new run starts with Raw draft state unknown.
- Duplicate policy: stable request UUID; changed text is refused; attempted requests never write again. This is deduplicated attempt delivery, not proof of exactly-once agent execution.
- Uncertainty: prompt, receipt and attempted queue status commit atomically before PTY writes. Partial writes remain uncertain. Neither crash nor restart replays them.
- Scrubbing: queue text is scrubbed before persistence and that scrubbed text is what the queue sends. Immediate sends retain the existing scrubbed-history contract.
- Q19/control bridge: unchanged and disabled.

## 6. Verification

`npm run bindings` regenerated IPC successfully. `npm run check` passed: TypeScript, 21 frontend tests in seven files, 462 Rust tests (73 app + 383 core + 3 purity + 3 hook), Clippy with warnings denied, and the secret scan. The staged final check is recorded in `_verify/p6-queue-check.log` (not committed). A final TypeScript check also passed after wording changes. `git diff --check` passed.

The five new integration tests use real isolated shell processes with intercepted writers and synthetic structured events. They verify persist-before-write order, FIFO, acknowledgement gating, Raw invalidation, cancellation, uncertain writes/reopen, stale-run review, database refusal before write, secret scrubbing and state/lock/approval guards. These are not real-agent acceptance tests.

Not verified: production Friendly UI against live Claude or Codex; app restart/forced kill with queued instructions in the real UI; complete Friendly-only multi-phase tasks. No API-credit calls were made. No installed app data or global agent configuration was changed.

## 7. Versions verified against

No new CLI flags, commands or protocols added. Existing hook/PTY delivery contracts remain unchanged. Generated bindings use the repository's locked Tauri/Specta toolchain.

## 8. Proposed doc changes

Proposed decision row:

| P6-QUEUE-D1 | Proposed | Durable instructions retain their reviewed run and require review after restart. Raw input invalidates an explicit empty-prompt assertion. Dispatch requires structured Ready, no pending question/approval, an unlocked live run and no unobserved delivery. FIFO attempts persist atomically before writing and never replay after uncertainty. |

Proposed Architecture addition: the queue worker evaluates structured state on the existing signal-watch cycle (up to ten seconds). Time and PTY silence never establish readiness. Queue rows are scrubbed before persistence; attempted rows link the existing prompt receipt. A persisted stale run pauses the queue until explicit rearm/cancellation.

Proposed Plan update: queue/composer implementation and automated safety checks are available; real-app acceptance remains open. First instructions still require an observed structured Ready state, so a new session with Unknown state cannot yet be driven entirely from Friendly.

## 9. Questions for Matthew

None required to merge this bounded implementation. The existing Q19/runtime acceptance work remains open.

## 10. Risks and known issues

- A fresh session with no structured Ready event cannot send a first prompt from Friendly. This conservative inherited limitation is explicit; no screen-derived readiness was added.
- Empty Raw state is a user assertion, not a claim that Chorus can inspect the TUI editor. A new Raw write always invalidates it, even a navigation key.
- Dispatch can wait up to the existing ten-second signal-watch interval after Ready. No busy-loop or additional per-session thread was added.
- Exact agent execution cannot be proven from PTY writes. Written/uncertain/observed receipts remain distinct.
- Unsubmitted drafts survive view switches in memory but not application shutdown. Queued instructions are durable.
- Parallel plan/narrator branches overlap registration lists, action capability wording, migration numbers and FriendlyTimeline. Keep each branch's distinct additions and regenerate bindings.

## 11. How to review

Read the core gate, queue store, shared delivery transaction, queue session boundary, manager Raw locking, then composer and tests.

In an isolated dev identity and scratch folder:

1. Start a Claude session, complete a tiny Raw turn to establish Ready, and leave a Raw draft unsent.
2. In Friendly, stage text and try sending; it must remain blocked until Raw is checked and its draft cleared/submitted. Pasting or Enter in the composer must not submit.
3. Confirm the empty Raw prompt. Queue two instructions while a turn is active. Observe FIFO delivery at successive structured Ready pauses, one history prompt per delivery.
4. Make the agent ask a question or approval. Queued instructions must wait without answering it.
5. Add Raw input after confirmation and verify dispatch pauses until another explicit check.
6. Cancel a pending instruction; it must never reach Raw. Attempted outcomes must not offer cancellation or retry.
7. Restart with pending instructions. Inspect and rearm the old head explicitly; later entries must not skip it.
8. Force-close the isolated app after a delivery attempt and reopen. Written/uncertain attempts must never replay.
