# P6 narrator review

## 1. Verdict

Partly done: narrator configuration, bounded request/response path, persistent summaries/actions,
no-key fallback and costs are implemented. The integration reviewer must connect plan input and the
queue composer's draft field, then verify the actual app. No production OpenRouter call or full P6
acceptance run was performed.

## 2. Branch

- Branch: `work/p6-narrator`.
- Base: `6fafc074a967727235c52a6d342e44ef2414aad8` (the current P6 checkpoint, not older `main`).
- Final commit: this branch's `P6 narrator: add bounded summaries and persistent next steps` commit;
  obtain its hash with `git rev-parse work/p6-narrator`.
- Changes relative to the base: one implementation/review commit, no push, merge or rebase.

## 3. Files

| Created | Purpose |
|---|---|
| `crates/chorus-core/src/narrator.rs` | Configuration validation, bounded request, strict summary parsing, generic actions, action signatures and cost parsing |
| `src-tauri/migrations/0015_p6_narrator.sql` | Durable narrator jobs, cost receipts and resolved suggestions |
| `src-tauri/src/store/narrator.rs` | Claim-once jobs, startup abandonment, summaries, persistent Done/dismissed dispositions |
| `src-tauri/src/narrator.rs` | Worker, OpenRouter HTTP, vault scrubbing, last-moment gates and typed commands |
| `src/views/session/NarratorPanel.vue` | Completed-turn summary, Next steps, Done/dismissed and known/unknown cost |
| `src/views/settings/NarratorSettings.vue` | Narrator key/model selection and per-project switch |
| `src/views/session/composerDrafts.ts` | Exact copy of queue agent's shared draft helper for isolated compilation; retain the queue implementation at integration |
| `_docs/HANDOFF-P6-NARRATOR-REVIEW.md` | This hand-off |

| Modified | Purpose |
|---|---|
| `crates/chorus-core/src/lib.rs` | Register pure narrator module |
| `src-tauri/src/store/mod.rs` | Register narrator store and migration |
| `src-tauri/src/lib.rs` | Register narrator and start it before hook listener/restore |
| `src-tauri/src/ipc.rs` | Contiguous narrator command registration block |
| `src-tauri/src/events/recorder.rs` **hot** | One enqueue call after a recorded successful turn end |
| `src/ipc/bindings.ts`, `src/ipc/index.ts` | Generated IPC and type exports |
| `src/stores/view.ts`, `src/views/SettingsView.vue` | Add Narrator settings navigation |
| `src/views/session/SessionView.vue` **hot** | Import panel/helper; stage suggestion, switch Friendly, show notice; mount sidebar panel |

No files deleted. No tracked shared roadmap/instructions files changed.

## 4. Surface added

- Migration `0015_p6_narrator.sql`: `narrator_jobs` keyed by turn, with status, summary, reason,
  model, requested flag and nullable USD cost; `narrator_actions` keyed by session and normalised
  send text, with open/done/dismissed disposition. Renumber on integration.
- Commands: `narrator_config`, `narrator_config_save(Config)`,
  `narrator_project_enabled(projectId)`, `narrator_project_set(projectId, enabled)`,
  `narrator_panel(sessionId)`, `narrator_resolve(sessionId, actionId, disposition)`.
- No new events; the panel polls every two seconds while mounted.
- Settings: `narrator` (credential ID/model only), `narrator.project.<id>` (`on`/`off`).
  Projects default on. The default global configuration has no key/model and makes no requests.
- No new dependencies, cargo features, environment variables or files outside the database.
- Runtime OpenRouter endpoint is fixed HTTPS with redirects disabled. The key is used only in the
  Authorization header, never in the request body or a diagnostic. No test endpoint is exposed.
- `_verify/tauri.p6narrator.conf.json` is an ignored isolated configuration for `.dev.p6narrator`,
  port 1443. It was prepared but no UI instance was launched by this branch.

## 5. Plan against result

| Requirement | Result |
|---|---|
| D28 OpenRouter key/model chosen in Settings | Implemented. Plain model-ID entry; no model is silently selected |
| Per-project narrator switch, on by default | Implemented, including an immediate pre-request check after Git IO |
| One call per turn end | Successful recorded turns enqueue once. Claim is durable before HTTP; crashes, errors and restarts never retry; old turns are not backfilled |
| Scrubbed bounded prompt/final/plan/tools/diff/resolved input | Implemented except plan adapter explicitly reports unavailable until integration |
| Now / Blocker / Up next | Implemented, labelled as the completed turn's summary, never live status |
| Question classification | `is_question` is persisted/displayed as a narrator inference. It does not override structured session attention. P7 attention integration remains open |
| Persistent Next steps / Done / dismissed | Implemented; exact send-text equivalents after case/whitespace normalisation never reopen. Last 32 resolved records are fed into the bounded model input |
| Suggestions use shared send path | A suggestion only appends to the queue agent's shared composer draft. It never sends or answers an approval. Composer integration is owned by the queue branch |
| No-key fallback | Implemented with explicit reason and generic Continue/Review drafts. Disabled/missing-key mode hides old model suggestions |
| Costs | Session/turn/model attributed receipts. Missing usage remains unknown, including uncertain calls. The UI shows known sum plus unknown-call count |
| P6 Claude and Codex Friendly-only tasks | Not performed; Q19 remains open |

## 6. Verification

Automated verification is recorded in `_verify/check.log` (ignored). `npm run check` passed:
TypeScript, 19 Vitest tests, 464 workspace Rust tests (74 app, 384 core, 3 purity, 3 hook),
Clippy with warnings denied and secret scan (7 patterns over 258 files). Bindings were regenerated
using `npm run bindings`.

Added meaningful tests cover:

- strict output shape/field sizes/action kinds; no approval action kind;
- UTF-8-safe input bounds and honest missing/negative/zero cost parsing;
- claim once and restart abandonment, unknown cost for an uncertain attempt;
- persistent Done/dismissed suppression, wrong-session disposition refusal;
- hiding prior model suggestions when a key is absent or narrator disabled;
- post-Git configuration, project reassignment, per-project off switch and newer-turn gates;
- the actual summarise pipeline through an injected fake transport: a fake DPAPI credential,
  escaped JSON secret redaction, a valid response and its cost receipt, with no network call;
- absence of a narrator key never reaching the transport.

**Not verified in the app:** summary layout, Settings save/project switch, Done/dismissed across app
restart, suggestion staging with the actual composer, automatic trigger from a real agent turn and
the integrated plan adapter. The integration reviewer owns an isolated `.dev.p6integration` app for
these checks. No paid/live OpenRouter call was made. A fake transport is not evidence that the
chosen live model supports JSON mode or that a particular provider works.

## 7. Versions verified against

- Existing locked dependencies: reqwest 0.13.5, Tauri 2.12.4, Vue 3.5.44.
- No CLI flags or channels changed.
- OpenRouter official documentation read on 2026-10-22:
  [Create a chat completion](https://openrouter.ai/docs/api/api-reference/chat/create-a-chat-completion)
  and [Usage Accounting](https://openrouter.ai/docs/cookbook/administration/usage-accounting).
  Request uses nonstreaming chat completions, JSON-object output, required provider parameter
  support and `max_tokens: 1200`. Nonstreaming responses report `usage.cost`; missing cost is
  explicitly unknown. No additional usage API call is made.

## 8. Proposed doc changes

Proposed decision (assign the next D-number on integration):

| ID | Date | Decision | Status | Why |
|---|---|---|---|---|
| P6-N-D1 | 2026-10-22 | Narrator suggestions are drafts that require explicit Send. Each successful recorded turn gets at most one durable narrator attempt. Startup never backfills old turns or replays pending/uncertain calls. Missing keys, disabled projects, invalid model responses and HTTP failures show generic Continue/Review with the reason. Known API usage and unknown charges are distinct. Done/dismissed steps stay resolved by normalised send text and are fed back into future requests. | Proposed | A summary must not silently send instructions, approve tools, repeat a charge after a crash, or imply zero cost when usage was not observed |

Suggested PLAN wording after integrated runtime verification: narrator configuration, persistent
Next steps and generic no-key fallback are implemented; live model verification and both complete
Friendly-only acceptance tasks remain unverified until performed. Preserve the Q19 gate.

Suggested ARCHITECTURE §9 addition: turn-attempt and action tables above, strict bounded JSON,
summary-of-completed-turn labelling, unknown costs and no automatic replay.

No new hard-won CLI lesson is claimed. JSON-value scrubbing follows the existing escaped-secret lesson.

## 9. Questions for Matthew

None blocks this implementation. A live OpenRouter smoke test would spend API credit and requires
separate explicit approval under HANDOFF-COMMON. Keep using the no-key path until a model/key is
deliberately configured. Q19 and the full P6 acceptance task remain separate.

## 10. Risks and known issues

- `plan_input` intentionally returns unavailable on this isolated branch. Integrator should call
  `store::progress::latest_plan`, accept only matching run and turn with `incomplete == false`, and serialise its steps. No
  earlier turn's plan should be silently presented as the current turn's plan.
- Queue's `composerDrafts.ts` is duplicated identically so this branch compiles. Until queue's
  composer is merged, the staged draft has no visible editable composer. Do not claim that branch
  alone is a complete send flow.
- A single worker processes summaries serially; a queued turn superseded by a newer turn/run is
  skipped visibly before spending. In-flight calls may complete after a project is switched off;
  UI fallback immediately hides their model suggestions.
- The narrator is a completed-turn interpretation. It cannot prove completion, approvals, blockers
  or ETA. Model-inferred questions do not change the agent's structured state.
- Deduplication guarantees equivalent send text, not semantic similarity under arbitrary rewording.
  Prompt feedback asks the model to avoid rephrasing resolved work.
- A timeout/crash can charge a request without a cost receipt. It stays unknown and is never retried.
- The persisted settings support a manually entered model ID, not a narrator-specific catalog picker.
- No settings-change lock spans the remote request. Settings/project/turn are rechecked immediately
  before HTTP; changes after dispatch cannot unsend an in-flight request.

## 11. How to review

Read pure core, migration/store, worker, then the two Vue components. Resolve the registration lists,
renumber the migration, connect `plan_input`, retain queue's draft helper and regenerate bindings.

No-key runtime script in the isolated integrated app:

1. Start with no narrator configuration. Open a Claude or shell session and its Friendly view.
   Verify the no-key explanation, generic Continue/Review, and unknown blocker wording. Raw stays
   accessible; no OpenRouter request should occur.
2. Put text in the composer. Choose Use draft on Review. Verify the existing text remains, the
   suggestion appends, a notice appears and no prompt/history delivery happens until explicit Send.
3. Mark Continue Done, Review Dismiss. Reopen the session, restart the isolated app and verify both
   remain resolved, including after a subsequent recorded turn.
4. Open Settings → Narrator. Save no-key configuration, create/select an isolated project and turn
   its narrator off. Verify that sessions in that project show the off explanation immediately.
5. Using fake transport/backend tests, verify a model summary and known cost. Remove the configured
   key or disable its project; verify old model suggestions disappear while generic fallback is shown.

Full P6 acceptance script (perform only after Q19/control bridge works):

1. In separate scratch repositories launch one Claude Code and one Codex session. Ask each for a
   three-stage plan: inspect a tiny fixture, propose a change, implement and verify it. Require one
   clarification and one harmless file-write approval before implementation.
2. Stay in Friendly to answer the question, inspect the plan, approve/deny the write, and queue a
   second instruction while the agent is busy. Verify the queue's receipt and exactly one prompt row.
3. Stage a Next steps draft, edit it, explicitly Send, and finish the task. Mark a suggestion Done.
4. Open Raw after completion and verify it is the same thread/process history, with matching prompts,
   replies and plan progress. Reopen Friendly and verify history/actions survive app restart.
5. Record CLI/model versions and exact observations. Do not close P6 if either agent's plan/control
   gate is missing, if a hidden Raw action was needed, or if an acceptance step was merely inferred.
