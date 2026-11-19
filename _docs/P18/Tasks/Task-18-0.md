# Task 18-0: Spikes

## Source Of Truth
- [HANDOFF-P18-mausbot.md](../../HANDOFF-P18-mausbot.md) §5, slice 18.0.
- [Phase-18-Overview.md](../Phase-18-Overview.md) K11.
- The installed OpenMausBot 0.1.169: its server bundle, its MCP server and its CLI help.

## Initial Starting Point (2026-11-18)
- No MausBot code in Chorus.
- The plan's MausBot facts came from an earlier read of the installed program, not from a live run.

## Goal
Answer three questions before 18.2 builds on them:
- **S1:** can Chorus start a MausBot bot working in a given folder?
- **S2:** can MausBot reach an MCP server on loopback with a header token?
- **S3:** does a bot write its own hand-off entry?

Write the results to `_docs/spikes/P18.md`.

## Exact Scope
- **Create:** `_docs/spikes/P18.md`.
- **Edit:** `_docs/spikes/README.md` (one line).

## Non-Goals
- No product code.
- Nothing that changes MausBot without a token Matthew has agreed to: no bots, MCP servers or
  messages.
- Never open MausBot's data files (plan §3).

## Dependencies
None.

## Step-by-step Work
1. **Read the program's source** for `POST /api/bots/<id>/tasks`, `POST /api/bots/<id>/messages`
   and the request-authentication rules.
2. **Confirm the read rules live:** `GET /api/health` and `GET /api/bots` with no token, and an
   unauthenticated `POST` refused.
3. **Read the CLI's help** for `send`, `run`, `new-bot` and `token create`.
4. **Read the MCP registry code** for S2.
5. **Write the results,** marking what is from the source and what was seen live.

## Test Expectations
None. This is a written result.

## Verification Commands
```
git diff --stat -- _docs/spikes
```

## Acceptance Criteria
- `_docs/spikes/P18.md` has a result for S1, S2 and S3, with the evidence for each.
- The doc says plainly which runs wait for a token.
- It has no local paths, usernames, bot names or token values.

## Review Checklist
- Every claim is marked as read from the source or seen live.
- The admin-scope requirement for choosing a folder is quoted.
- The live runs left to do once a token exists are listed (see the spec).
