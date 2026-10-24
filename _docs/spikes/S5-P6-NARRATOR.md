# P6 narrator and suggested next steps

2026-10-23. D82 implements the requested sequence: Now / Blocker / Up next, saved next-step
actions, then narrator configuration and cost visibility. Q19 remains approved; Raw draft
retrieval stays deferred.

## Implementation

- Migration 0018 records queued/attempted/ready/skipped/failed narration by turn, costs and tokens,
  action dispositions, and project switches. Turn closure and enqueue are atomic. An attempt
  commits before HTTP, and never automatically retries after a failure, timeout or app restart.
- The worker sends bounded scrubbed prompt/result/plan/tool excerpts and current diff counts to
  OpenRouter. Redaction happens before excerpt clipping. There is no raw terminal scraping,
  image upload, arbitrary file read, tool execution, or automatic prompt submission.
- Structured output is validated locally. `has_next_step=false` removes contradictory actions.
  Summaries identify their source turn. Live requests/state take precedence, and earlier-turn
  narration cannot masquerade as the new turn's Blocker/Up next.
- Done and Dismiss persist and are supplied as feedback. Normalized prompt or title matches are
  suppressed locally, including a rewritten prompt with the same dismissed title. Semantic
  paraphrases with different titles and prompts cannot be guaranteed absent; model quality varies.
  Restore explicitly reopens an action. Add to prompt appends to existing text and keeps images.
- Settings → Narrator uses an existing OpenRouter provider/API key and a user-selected model.
  Projects default on once configured. Missing configuration leaves the timeline/replies/composer
  usable with labelled generic suggestions. A management key is not a generation key.
- Reported cost survives malformed-summary validation. Missing usage/timeout/provider failure
  costs remain unknown and are excluded from the reported total. Narrator costs are separate
  from agent costs. Configuration changes do not backfill old turns automatically.

## Real-app checks

Used a separate development identity/database and a scratch Claude Code session; existing user
sessions were not used. Private evidence is ignored under `_verify/narrator`.

- A real Claude Code 2.1.308 turn completed from Friendly. Before configuration, narration was
  skipped visibly, with zero HTTP attempts and generic Continue/Review actions.
- Configured an existing encrypted OpenRouter API credential in the isolated identity. A real
  Gemini 2.5 Flash Lite response produced all summary fields and a review suggestion; OpenRouter
  reported $0.0000794. An automatic later GPT-4.1 nano summary reported $0.0000687.
- The actual Add to prompt button preserved existing draft text and added the suggestion without
  creating another agent turn. The actual Done button stored the disposition. Duplicate narration
  of an attempted turn was refused.
- Project-off prevented the next completed turn from making a call. The Settings checkbox also
  persisted the switch. An invalid model returned HTTP 400; a later provider rate limit returned
  HTTP 429. Both stayed visible, recorded unavailable cost, and did not retry.
- Normal app close/relaunch preserved four narrator attempts, reported costs, Done/Dismiss
  dispositions, the project-off switch and a saved Friendly draft. Agent history was not duplicated.
  Interrupted-attempt recovery/no-replay is additionally covered by a database test.
- Testing exposed a model suggesting another review despite saying no work remained. The final
  contract explicitly classifies whether a next step exists; local normalization clears actions
  when false. Prompt/title matching also prevents a reworded dismissed action from returning.
- The final real Friendly turn, on the updated contract, returned `has_next_step=false`, no
  actions, and "No further action is suggested." The UI showed no open suggestions and retained
  the Done/Dismiss history. Its reported cost was $0.0000815. The final panel was inspected at
  the standard desktop window size.

Validation: 486 Rust tests, 31 frontend tests, TypeScript, Clippy with warnings denied, generated
bindings, production frontend build, secret scan and whitespace checks passed. The production
frontend build retains its existing large-chunk advisory.

## Sources and scope

Verified request/schema and usage shape against current official documentation:
[structured outputs](https://openrouter.ai/docs/guides/features/structured-outputs) and
[usage accounting](https://openrouter.ai/docs/cookbook/administration/usage-accounting).
The public model catalog was checked for the tested models' structured-output parameters/prices.
The selected provider/model must support the requested schema; there is no silent model substitution.

The narrator does not replace agent progress, approve requests, mark agent tasks complete, or
infer unseen Raw drafts. P7 attention/notifications and broader provider/CLI acceptance remain
separate work.
