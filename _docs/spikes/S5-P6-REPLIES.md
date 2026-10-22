# P6 continuation — held replies and Friendly history

2026-10-22, from `5e63815` on `roadmap-p6`. The P5 baseline and assets remain intact.
This slice does not close P6 or reopen the P1/P4 waivers.

## Implemented

- Claude Code HTTP `PermissionRequest` and `PreToolUse` for `AskUserQuestion` hold a response
  for up to 120 seconds. Other hooks remain observation-only. Unsupported/oversized requests
  fall back to Raw. Claude's hook timeout is 125 seconds, leaving room for the server to return.
- The ActionRouter validates session, reviewed run, ask identity, lock, lifetime and reply shape.
  Migration 0014 records scrubbed full requests and reply attempts before sending. A request UUID
  is reusable only for the same reply; a second decision for the same ask cannot be sent.
- A reply receipt means attempted delivery, not agent acceptance. Raw can win the race. Subsequent
  structured events settle the ask; history separately displays the attempted answer or decision.
  Crash/restart never replays a reply. Expiry/disconnection and run/turn end close the channel.
- `Answer in Raw` releases the hook with no decision. This is especially important for questions:
  Claude has not displayed its native question yet while `PreToolUse` is pending. Its permission
  dialog, in contrast, remains available while `PermissionRequest` is held.
- Friendly/Raw switches views of the same backend process. Friendly displays recorded prompts,
  final agent messages, collapsed tool groups, requests and replies. Plan, queue and narrator
  limitations remain explicit. Selecting/pasting text does not submit a reply.

The original Chorus event service was consulted as an observation reference. No changes were made
there. No dependencies, global CLI settings or installed Chorus data were changed.

## Current CLI and protocol evidence

Claude Code remains 2.1.308; Codex remains 0.163.0. The current
[Claude hook reference](https://code.claude.com/docs/en/hooks) documents the response shapes used:
permission decisions are nested under `hookSpecificOutput.decision`; question replies preserve
the original input and add an answers map. Runtime, rather than documentation alone, gates enablement.

Q19: another bounded, subscription-only app-server turn asked for explicit plan-tool updates;
it completed with no `turn/plan/updated`. A diagnostic turn using the actual default model,
`gpt-6-astra`, reported `update_plan` unavailable and named `request_user_input` as a Plan-mode
tool. That is the model's reported tool availability, not proof that every model/configuration
lacks plans. No plan event was observed. The generated schema and
[official app-server reference](https://learn.chatgpt.com/docs/app-server) still describe plan
notifications. The production Codex bridge remains disabled pending actual plan events and
same-thread Raw/controller verification, including authenticated transport and replies.

## Real-app verification

The isolated dev identity/data from the previous slice was reused; all model work occurred in its
scratch folder. A temporary harness only initiated bounded test prompts and was removed afterwards.
The question and approval actions themselves used the production Friendly UI and generated IPC.

- Claude asked which greeting to use. Choosing **Hi** filled the field without sending anything.
  **Send answer** returned the answer; the structured post-tool event recorded it.
- The next card showed the complete Write input. **Allow once** created the scratch file containing
  `Hi`; the agent completed. Raw showed the same question, answer, write and completion, including
  “Allowed by PermissionRequest hook”. Switching views did not create a run or restart the process.
- A separate tiny write request was denied with an explicit reason. The file was absent; Claude's
  final message repeated the reason and confirmed it stopped without retrying. Raw showed the denial.
  There was no dedicated denial acknowledgement hook, so the receipt/history distinguishes the
  attempted denial from the separately recorded final message.
- A third approval was left unanswered for the full 120 seconds. Friendly disabled its reply
  controls with an expired-channel explanation, no receipt was inserted and the file stayed absent.
  Raw still displayed Claude's native approval. Explicitly answering **Yes** there created the file.
- A fourth turn released a question through **Answer in Raw**. Claude's native question appeared
  and accepted its answer there. Its next Write approval was shown simultaneously in Friendly and
  Raw; answering **Yes** in Raw while the hook still had over 90 seconds left withdrew the Friendly
  card and completed the write. No Friendly approval receipt was inserted for that Raw decision.
  This exposed a redundant `PermissionRequest` for the native question itself; the normaliser now
  ignores it, retaining the original question state, with a regression assertion.

Automated coverage includes duplicate and changed request IDs, stale runs, locking, same-tool
parallel-call correlation, competing Raw events, expired/disconnected hooks, database failure
before send, Close dropping a held reply, complete multi-question answers and decoded-JSON secret
scrubbing. Final checks: 456 Rust tests, 19 frontend
tests, TypeScript, Clippy and secret scan. Bindings regenerated; temporary harness removed.

## Remaining work

Codex plans/control (Q19), other agents' reply channels, persisted plan capture, intermediate agent
messages, Raw draft coordination, composer/queue, narrator configuration/costs and persistent Next
steps remain. Both planned multi-phase tasks completed entirely from Friendly remain open: the
bounded checks above are not those exit criteria. Gemini/Aider/P14 limits are unchanged.
