# P6 integration review

The three authorized P6 workstreams are assembled on `work/p6-integration`: durable Claude progress,
Friendly composer/queue, and narrator/Next steps. P6 remains open. The original working checkout
had concurrent UI/dependency edits, so integration uses a separate worktree and does not include
those uncommitted changes.

## Changes and integration decisions

- Migrations are ordered 0015 progress, 0016 queue, 0017 narrator. Typed IPC bindings were regenerated.
- Actual Claude task hooks produce durable, deduplicated plan snapshots. Visible intermediate text
  comes from a bounded transcript cursor. Missing/corrupt/unsupported evidence is explicit, not
  inferred from terminal text. Other adapters remain capability-gated.
- The composer and queue use D75 receipts. Raw input invalidates the empty-prompt assertion. Queued
  work survives restart but requires explicit rearming for another run; uncertain attempts never replay.
- Narrator plan input accepts only complete same-run/same-turn evidence. Late replies preserve cost
  without exposing stale suggestions. Panels also reject old-turn/old-run model actions. Exact trimmed
  send text preserves distinctions between case-sensitive code instructions.
- Suggestions append to the existing composer draft. Done/dismissed actions are durable. No-key
  fallback explains the missing configuration and shows generic drafts; no inference approves a tool
  or mutates structured attention state.
- Runtime exposed a transient transcript-read warning that remained after recovery. Successful reads
  now clear that warning while retaining warnings about lost/incomplete evidence; two regressions cover it.

## Observed in the isolated app

Claude Code 2.1.308 completed a small greeting task using real task tools and a structured question.
Friendly showed the first step in progress and the second pending, an intermediate assistant message,
the active question, the submitted answer and both completed steps. Raw showed the same answer and
final result. Captured history survived app restart. Restarting during a task and then updating old
tasks correctly left the checklist marked incomplete rather than claiming full evidence.

No-key narrator fallback rendered with an explicit reason. Use draft preserved existing composer text
and appended the suggestion without delivering it. Marking Review Done persisted its disposition.

An initial queue attempt persisted one prompt and receipt but was not observed by Claude: separate
back-to-back text/Enter writes were coalesced into pasted input. It remains uncertain and is not retried.
The delivery path now separates them by a bounded 200 ms gap while holding its serialization locks.
A fresh queue request then reached Claude, produced its prompt hook and completed with the expected
answer in Friendly. The database contains one composer prompt, one observed delivery receipt and one
completed turn for that request. The original uncertain attempt was not replayed.

## Validation and remaining gates

The final integrated check passed 486 Rust tests, 21 frontend tests, TypeScript, Clippy with warnings
denied and the secret scan. The live queue test ran the corrected delivery code.
Individual workstream reviews remain useful implementation detail; this document supersedes their
unresolved migration, registration, plan-input and composer-integration notes.

- Q19 remains open. Codex 0.163.0 Plan mode produced a real structured question and accepted its answer,
  but the default model emitted no progress-plan updates and reported the planning tool unavailable.
  See [bounded probe](spikes/S5-P6-Q19-MODES.md). No production Codex control bridge is enabled.
- A fresh session still needs one Raw bootstrap turn before structured readiness permits Friendly send.
- A configured paid narrator call, project opt-out UI acceptance and the full queue restart/cancel/race
  matrix remain to be observed. Backend tests cover these policies but do not close runtime acceptance.
- Both planned multi-phase Friendly-only exit runs remain open. The greeting probe used a test-only
  bootstrap control and is not presented as that acceptance run. P7 and P14 are unchanged.
- Composer drafts survive view switching in memory; queued instructions are durable across shutdown.

No dependencies were added. Local screenshots, database evidence and test helpers stay ignored.
