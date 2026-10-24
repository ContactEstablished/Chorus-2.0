# Q19 continuation — question mode versus progress plans

2026-10-22, Codex CLI 0.163.0. This is a bounded protocol test, not P6 acceptance.

Q19 asks whether Chorus can receive a live Codex progress plan and answer structured
questions/approvals while the Raw terminal controls the same conversation. Those are
separate capabilities and must each be observed before enabling the production bridge.

## Plan-mode result

A private stdio app-server, using the existing ChatGPT subscription and the default
`gpt-6-astra` model, ran one tiny turn in an empty scratch folder. The thread was
ephemeral and read-only. No filesystem tool, network tool or delegation was requested.
`turn/start.collaborationMode` selected `plan` with the same model and low effort.

- The model emitted `item/tool/requestUserInput` with a choice between two greetings.
- The test controller returned a question-id-keyed `answers` map.
- The server emitted `serverRequest/resolved` and completed the turn with the chosen greeting.
- No `turn/plan/updated` notification appeared. The final message reported that
  `update_plan` was not callable. That statement describes this tested configuration;
  it does not establish that every model or mode lacks a planning tool.

The installed feature listing reports `default_mode_request_user_input` disabled.
This helps explain why a Default-mode question probe is not interchangeable with
the Plan-mode probe; no global feature or CLI configuration was changed.

The [official app-server reference](https://learn.chatgpt.com/docs/app-server)
distinguishes progress updates (`turn/plan/updated`) from a Plan-mode proposed-plan
item (`plan` / `item/plan/delta`). A prose plan or proposed-plan item alone does not
prove a changing progress checklist.

Private test code and the method-only/controlled-text result are under `_verify/p6/`.
The server was stopped at the end. No production capability was enabled.

## Still open

Observe a real changing progress plan, and verify authenticated Raw TUI/controller
attachment to the same thread, approval replies, competing answers, cancellation,
disconnect recovery, expiry and process shutdown. The question round-trip above used
one controller and no Raw TUI, so it does not satisfy those remaining gates.
