# P6 progress, Codex control, and draft feasibility

2026-10-23. Continues the approved Q19/Friendly work. Raw draft detection and transfer are
explicitly deferred at the user's request. No narrator or inferred checklist is added.

## Implemented

- Migration 0017 stores bounded, scrubbed progress entries by turn and stable source identity.
  Repeated message chunks update a row. The timeline shows user-facing intermediate text,
  proposed plans, and the latest real task checklist. It excludes reasoning and subagent text.
- Claude Code 2.1.308: main transcript text is polled; successful TaskCreate/TaskUpdate/TodoWrite
  hooks drive the checklist. `CLAUDE_CODE_ENABLE_TODO_TOOLS=1` enables the task tools measured in S5.
- Codex 0.163.0: each run owns an authenticated loopback app-server and a real remote TUI in the
  same Windows Job Object. The capability token is environment-only; argv contains only its SHA-256
  verifier. Windows WinHTTP supplies WebSocket transport without adding a dependency.
  Permissions/model/effort are set on the server. Raw creates new conversations; the controller
  attaches once the first real prompt materializes the rollout. No synthetic model turn is sent.
- Structured question IDs and command/file approvals use the existing durable reply-attempt path.
  Approval is once only; denial uses the agent's offered decision. Codex does not accept a denial
  message, so that field is absent on its cards. Unknown/secret requests stay in Raw. Expired,
  disconnected or already-attempted replies never replay automatically.
- The bridge is enabled for the measured Codex version. Other versions keep the existing native
  hook path with visible capability limits. Restart is required to change an existing run's adapter.

## Real-app acceptance

Separate development identity and database; existing user sessions were not used for testing.
Private evidence and scratch files are ignored under `_verify`, not committed.

- Claude: Friendly Send -> actual AskUserQuestion -> choose Green -> Friendly Allow once for the
  scratch JSON write -> read-back verification -> all three actual task steps completed. Intermediate
  messages and pending/in-progress/completed checklist states were observed in Friendly. Raw showed
  the same conversation. Final JSON contained `color=Green` and boolean `verified=true`.
- Codex: Friendly Send -> actual request_user_input -> choose Green -> Friendly Allow once -> write
  and read-back verification. A follow-up queued in Friendly changed the color to Blue after another
  approval and finished with the requested marker. Both prompts were recorded once, with confirmed
  delivery, resolved requests, intermediate messages and completed turns in the same Raw thread.
- Codex did not emit `turn/plan/updated` in these tests. The event parser handles that documented
  shape, but current-model checklist emission is **not** claimed as observed. Friendly says plans
  appear when the agent emits them. The task's phases were visible through its progress messages.

## Unsent Raw drafts: bounded investigation only

No supported, general read-only draft getter was found in the interfaces inspected. That is a
finding about this investigation, not proof that every agent implementation lacks one. Transcripts
and turn APIs describe submitted messages; reading their latest user message would mistake history
for an unsent draft. An external editor handoff is a possible future integration, but activates the
editor and must be verified per installed version, including attachments and cancellation. It is not
passive draft detection, and it is not implemented here.

| Agent | Evidence / possible handoff | Retrieval conclusion |
| --- | --- | --- |
| Claude Code | Official interactive-mode reference documents Ctrl+G and Ctrl+X Ctrl+E editing the current prompt. | Candidate editor handoff; no draft getter found in hooks/transcript. Not runtime-certified as a Chorus draft reader. |
| Codex | Official prompt-editor reference documents Ctrl+G with VISUAL/EDITOR and return to composer before sending. Installed 0.163.0 generated ClientRequest schema contains no draft-read method. | Candidate editor handoff; app-server conversation data is not the TUI's unsent buffer. |
| opencode | Official v2 TUI reference documents Ctrl+X then E or /editor for the current prompt, returning to composer before Enter. | Candidate editor handoff; no read-only draft getter established in this investigation. |
| Gemini CLI | Official shortcuts list opening the current prompt or plan in an external editor (Ctrl+G, with older Ctrl+X deprecated). | Documented candidate; installed-version round trip remains untested. |
| Aider | Official /editor configuration describes composing prompts in an external editor. | This alone does not establish retrieval of an existing unsent input buffer. Unverified. |
| Grok | Current official shortcuts document prompt stash/restore. Documentation may differ from Chorus's previously measured CLI version. | Stash is not a read API. No reliable external draft retrieval established. |
| Shell | Input belongs to the selected shell/line editor, not a common agent protocol. | No general retrieval contract; do not guess from terminal rendering. |

Sources checked 2026-10-23:
- [Claude interactive mode](https://code.claude.com/docs/en/interactive-mode)
- [Codex prompt editor](https://learn.chatgpt.com/docs/cli-customization)
- [Codex app-server](https://learn.chatgpt.com/docs/app-server)
- [opencode v2 TUI](https://dev.opencode.ai/v2/docs/cli/tui)
- [Gemini keyboard shortcuts](https://geminicli.com/docs/reference/keyboard-shortcuts/)
- [Aider editor configuration](https://aider.chat/docs/config/editor.html)
- [Grok keyboard shortcuts](https://docs.x.ai/build/keyboard-shortcuts)

## Remaining P6 scope

Narrated Now / Blocker / Up next, suggested Next steps and dismissals remain open. Raw-draft
retrieval stays deferred. Wider CLI-version, provider and non-Claude/Codex acceptance is not implied
by these subscription-backed tests. Q19 remains approved; these limitations do not reopen it.

Additional checks: a Codex approval stayed connected while awaiting a human response; Friendly
Deny resolved it and the proposed file remained absent. Restart resumed the same conversation
without duplicating its existing turn. Full checks pass: 480 Rust tests, 30 frontend tests,
TypeScript, Clippy with warnings denied, generated bindings and the secret scan.
