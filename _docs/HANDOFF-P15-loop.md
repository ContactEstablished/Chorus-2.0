# Hand-off plan: P15 — Autonomous phase loop

**Read [HANDOFF-COMMON.md](HANDOFF-COMMON.md) first.** It holds the isolation, safety, shared-file and
review-file rules this plan relies on. Label `p15`, Vite port 1437, review file
`_docs/HANDOFF-P15-REVIEW.md`. The source is Matthew's spec, "Chorus Autonomous Phase Loop — Spec"
(2026-10-24). It stays local and untracked in the main checkout's `_docs/Automation Loop/`
(decision 10), so this plan never quotes it at length. This plan says how Chorus builds it.

## 1. Goal

The loop runs a multi-phase roadmap phase after phase without Matthew, and stops only at planned
milestones or real blockers. It is a full Chorus capability for any project, not a script for one.
Counsel (P10) and the Telegram relay (P13) are built once, inside this phase, for the loop, and
Telegram is in the first version (D97).

**Exit**, seen working in the real app on the fixture projects in §7:

1. A three-phase fixture roadmap runs from its kickoff files to a milestone packet with no human input
   except the planned reviews, and each passing phase is tagged.
2. Every stop condition in the spec stops the run with a STOPPED message that names the check, the
   last error and how to inspect it, and the last good phase is intact.
3. Blocking, timed and log-only questions behave as the spec says, answered from Telegram and from
   Chorus, and "make it a rule" lands in `decisions.md`, so the question isn't asked again.
4. Counsel settles a split through an anonymized arbitrator, and the packet flags the overrule.
5. Every step records the model that actually ran it (L61), not only the one asked for, and no
   reviewer shares a vendor with the task's builder.
6. An exhausted allowance pauses or fails over as the spec says (simulated, and real if one happens).
7. No key, bot token or capped-key token appears in a run file, a Telegram message, a log or argv.

## 2. Out of scope

- Merging to main and deploying (the spec's own limits).
- The kickoff conversation's content. Chorus starts and keeps the session; Matthew has the talk.
- Installing third-party skills into Claude Code's global config. Chorus never writes `~/.claude`
  (decision 7).
- Running while Chorus is closed. Automations only run while Chorus runs (P8), and so does the loop.
- Webhooks: Telegram uses long polling only (P13 §2).
- A model inside the orchestrator, gatekeeper or integrator, except the integrator's conflict
  resolution.

## 3. What already exists

| Spec piece | Built in Chorus | Still to build |
|---|---|---|
| Roles and models (Settings → Models) | Bots (P8): agent, model, effort, provider. OpenRouter catalog Refresh and launch tiers (P4, D57) | Roles mapped to Bots, fallback chains, the vendor rule, the Test button |
| "Record the model that ran it" | The actual-model readout for Claude Code and opencode (L61) | Codex's, if its app-server reports it |
| Builders in worktrees | P5 worktrees, with Claude Code's trust entry written for them (D55) | File claims, task branches, the integrator |
| Starting a step unattended | Automations (D84, D85); a positional first prompt, also on resume (D96, L64); Codex turns through its app-server bridge; opencode through its server | Chaining steps into phases |
| Questions and approvals | Friendly's reply cards for Claude Code and Codex (P6, P7) | Escalation classes, timed defaults, Telegram answers |
| Counsel | Nothing yet (P10 planned the same shape) | All of it |
| Telegram | A full design (HANDOFF-P13 §4), no code | All of it, plus the loop's message types |
| Spend caps | Per-run capped OpenRouter keys, revoked with the cost read back | A per-run cap across many sessions |

## 4. Ground facts (checked 2026-10-24; re-check before relying on them)

- **Claude Code 2.1.308:** `--permission-mode` takes `acceptEdits`, `auto`, `bypassPermissions`,
  `manual`, `dontAsk` or `plan`. It also has `--allowedTools`/`--disallowedTools`,
  `--mcp-config <files>` with `--strict-mcp-config`, `--plugin-dir` and `--settings`. A positional
  prompt after `--` starts the first turn, also with `--resume` (L64).
- **Codex 0.163.0 app-server:** `account/rateLimits/read` and the `account/rateLimits/updated`
  notification. Each window has `usedPercent`, `resetsAt` and `windowDurationMins`, and errors carry
  `rateLimitExceeded` with a `rateLimitReachedType`. `-c key=value` overrides any config value.
- **Claude Code 2.1.308's binary contains** `rate_limits` with `five_hour`, `seven_day`,
  `used_percentage` and `resets_at`, plus a `StopFailure` hook and `rate_limit_error`. This is string
  evidence only. Spike S2 finds where they are delivered (likely the status line's input) before
  anything relies on them.
- **Measured in slice 1 ([spikes/P15.md](spikes/P15.md), 2026-10-25):** `dontAsk` with an allowlist refuses
  without prompting in print mode; `--restricted --tools …` (new in 2.1.308) leaves only the named
  tools, ignores user and project settings and confines file tools to the working folders; the
  account's claude.ai connectors load unless `--strict-mcp-config` is passed. Claude Code reports
  both windows in a `rate_limit_event`. All three CLIs call tools on a loopback MCP endpoint with the
  token only in the environment, but Claude Code gives up on a call after about 60 s. Codex's
  `workspace-write` blocks network and writes outside the workspace (and `%TEMP%`), not reads.
- **No new dependency is needed for the main pieces.** `reqwest` (Telegram's Bot API) and `axum` (a
  loopback server) are on the approved list (ARCHITECTURE §10, D30).

## 5. Design

**Core (`chorus-core::run_loop`, pure, with tests):**

- The run, phase, task and step state machine, and the shape it persists.
- Stop conditions as one function of counters and signals, and error signatures (normalized failing
  test names and first error line).
- Model resolution (task override, then phase override, then role default), fallbacks (low-risk tasks
  only), the vendor rule and the screen's warnings.
- The scope check (changed files against claims, with lockfile and generated-file allow-lists), and
  test-integrity heuristics (test count, skip markers, removed assertions) per language.
- Escalation classes, timed defaults with time passed in, and "make it a rule".
- Counsel briefs, anonymization, agreement, the per-phase cap and spike orders.
- Reading and writing the run files: `roadmap.md` front matter and `tasks.md` task blocks.
- The relay core from HANDOFF-P13 §4: update parsing, the auth guard, pairing, callback tokens within
  64 bytes, rendering, the rate bucket and dedupe.

**Back end (`src-tauri/src/run_loop/`):**

- **Orchestrator:** one worker per run, advancing steps. It starts sessions through the normal launch
  path from Bots, gives each its first prompt through a structured channel (D96), and learns that a
  turn ended from hooks, the Codex app-server or opencode's server, never from screen text.
- **Gatekeeper:** runs the run's check commands in a Job Object with timeouts, scrubs their output,
  and writes `gate-report.md`. Commands come only from the run's config, which Matthew writes.
- **Agent tools:** a loopback MCP endpoint on the existing listener, with a per-run token. It offers
  `ask(class, question, options, default)`, `log_assumption`, `task_done` and `request_counsel`.
  `ask` never holds the call while a person decides (Claude Code gives up after about 60 s): it
  records the question and returns at once, the agent ends its turn, and the answer or the timed
  default arrives as the session's next prompt, so a question also survives a Chorus restart.
  Sessions get it through a config file in app data (`--mcp-config` for Claude Code, `-c mcp_servers…`
  for Codex, opencode's config), never through the repo. Spike S3 confirms each CLI (decision 2).
- **Git:** the run branch, task branches in P5 worktrees, squash merges, `phase-NN` tags and rollback.
- **Relay:** HANDOFF-P13's design, plus the loop's message types, inline buttons and reply routing.
- **Counsel:** independent panel sessions, then an arbitrator in a fresh session.
- **Allowances:** Codex from its app-server, Claude Code from whatever S2 finds, and OpenRouter
  through capped keys. Keep Windows awake while a run is active.

**Data:** a migration for runs, phases, tasks and steps (model asked, model observed, session,
attempts, error signature), questions, counsel cases, and the relay tables from HANDOFF-P13 §4.

**Run files (decision 3):** the documents agents read go in the repo, on the run branch, under
`.chorus/run/`: roadmap, decisions, assumptions, phase specs, tasks, gate reports, reviews, counsel
and milestone packets. A rollback to a tag then resets them with the code. Machine state (attempt
counts, the model per step, Telegram message ids) lives in Chorus's database, with a read-only
`run-state.json` exported beside it for people and agents.

**UI:**

- **A Loop screen:** runs, a phase timeline, each step's session and model, open questions, stop
  reasons, and the milestone packet with Approve, Approve with notes and Roll back.
- **Settings → Models:** each role is a Bot plus a fallback Bot, with a Test button and the warnings.
- **Settings → Phone:** from HANDOFF-P13.

## 6. Slices, in order (each ends seen working in the app; commit after each)

1. **Spikes, no product code.** Done 2026-10-25 except where noted: results in
   [spikes/P15.md](spikes/P15.md).
   - S1: does `dontAsk` with an allowlist refuse unlisted tools without prompting? What do Codex's
     approval and sandbox settings do unattended?
   - S2: Claude Code's allowance: where `rate_limits` arrives, and what `StopFailure` carries.
   - S3: the loopback MCP endpoint seen as tools by Claude Code, Codex and opencode, with the token
     kept out of argv.
   - S4: a Telegram dev bot (decision 11): long polling and buttons from Windows. *Moved to the
     start of slice 4, where the token has a Settings field to go in.*
   - Seen in a terminal in the `p15` instance: `dontAsk` draws no dialog, and the status line
     receives `rate_limits`. Fixtures live under `%TEMP%\chorus-p15`, which Claude Code trusts.
2. **One phase, one task.** The orchestrator, run state, the gatekeeper's checks, roles from Bots, the
   model logged per step, and every stop condition. Prove it with the scripted agent (§7), then with
   one real run on fixture F1. *Done 2026-10-25; see [the review draft](HANDOFF-P15-REVIEW.md).*
3. **Agent tools and questions in Chorus.** `ask`, `log_assumption` and `task_done`; the three
   classes, timed defaults, the assumptions log, and "make it a rule". *Done 2026-10-25.*
4. **Telegram.** Spike S4 first, then HANDOFF-P13's P13a, with phone answers now possible (P6 built the reply channels),
   plus the loop's message types, buttons, reply routing and commands. *Loop half and S4 done
   2026-10-25; the session features follow D101.*
5. **Chaining and the plan grill.** Kickoff files, the planner reading the code the last phase left,
   the plan griller on a second vendor, and GRILL ROUND batches.
6. **Parallel fan-out.** File claims, per-task worktrees, the integrator, and the scope check.
7. **Cross-vendor review and milestone packets,** with rollback by tag and `/stop`.
8. **Counsel.** The panel, the anonymized arbitrator, spikes, the triggers and the cap.
9. **Limits.** Both allowances, failover, the run's spend cap, wall-clock caps and keeping awake.

The spec's tier picker already exists for launches (P4). Slice 2 exposes it for OpenRouter roles.

## 7. Test scenarios

Two throwaway fixture projects, created by a script so every run starts clean:

- **F1:** a tiny TypeScript CLI with Vitest. Its checks are fast and cheap.
- **F2:** a minimal ASP.NET Core API plus a Vue page, the stack in the spec's template, with
  `dotnet build/test`, ESLint and a Playwright smoke test.

There is also a **scripted agent**: a test double that makes a planned sequence of edits and tool
calls. It makes most scenarios deterministic and free of model cost. Real models run only where the
scenario is about a model.

| # | Scenario | Proves | Runs on |
|---|---|---|---|
| 1 | Three phases to a milestone packet | The happy path, tags, packet, model log | Real, F1 |
| 2 | A check fails, and the second attempt fixes it | Fix loop | Scripted |
| 3 | The same check fails three times | Counsel diagnoses before attempt 3, then STOPPED | Scripted + real Counsel |
| 4 | The same error signature three times in a row | Task stop | Scripted |
| 5 | Scope broken, corrected once, broken again | Scope stop | Scripted |
| 6 | A test deleted or skipped to get green | Integrity check | Scripted |
| 7 | A planted fake key in a commit | Secrets check; the key never reaches Telegram | Scripted |
| 8 | A blocking question | Its task pauses, others run, a Telegram button answers it | Real Telegram |
| 9 | A timed question nobody answers | The default after a shortened timeout, logged | Scripted |
| 10 | "Make it a rule" | `decisions.md` grows; not asked again next phase | Real |
| 11 | The reviewer flags an unplanned architecture change | Immediate stop | Scripted |
| 12 | Counsel splits | Anonymized arbitrator decides; packet flags the overrule | Real |
| 13 | Counsel splits on a fact | A spike task, then a decision | Real, small |
| 14 | An allowance runs out (simulated) | Low-risk to fallback, the rest paused until reset | Scripted |
| 15 | The OpenRouter cap is reached | Pause and ask | Real, tiny cap |
| 16 | Chorus is killed mid-build | Resume from state, nothing lost or run twice | Real |
| 17 | `/stop P02` | Rollback to `phase-01`, earlier phases untouched | Real |
| 18 | Parallel tasks, then a merge conflict | Claims, integrator, conflict resolution | Scripted + real |
| 19 | The builder fell back to Sol | The reviewer isn't OpenAI | Scripted |
| 20 | The wall-clock cap | Phase stop | Scripted |
| 21 | F2 end to end, one phase | The .NET + Vue checks and Playwright smoke run | Real, F2 |

## 8. Risks

- **Allowance windows** (Claude Max's 5-hour and weekly limits, Codex's windows) will throttle runs
  that use many sessions. Slice 9 makes this visible rather than surprising.
- **Unattended permissions** are the security edge (decision 1).
- **Cheap models are unreliable judges.** The narrator missed a fix suggestion in 1 of 4 runs (D95).
  A fallback reviewer may send work back, but not pass a phase on its own.
- **CLI drift:** flags, models and limits change every few weeks. Each slice re-checks the facts it
  uses (§4).
- **Size:** this is the largest phase yet. The slices keep each review small.

## 9. Decisions for Matthew (ask before building past them)

**Answered 2026-10-25 (D98, D99):** D99 adds that GPT and Claude models never run through OpenRouter;
an OpenRouter last resort is `~z-ai/glm-flash-latest`, `~deepseek/deepseek-v4-flash-latest` or
`~moonshotai/kimi-latest`.

**First answers (D98):** 1 to 8 as recommended (1 still depends on S1). 9: OpenRouter spend is
capped at $2 per run and $6 for all of P15's testing, and Claude and ChatGPT models run only on
Matthew's own subscriptions, through the Claude Code and Codex sign-ins, never through an OpenRouter
or API key. 10: the spec stays local. 11 happens at S4.

The $6 covers every OpenRouter call made while testing P15, the narrator's and Counsel's included,
and is tallied from each capped key's cost read back. A real-model test doesn't start while a
Claude or ChatGPT window (5-hour or weekly) is over 70% used, and stops for Matthew at 90%, so
testing leaves him headroom. Reaching any cap means stop until Matthew reviews.

1. **Unattended tool permissions.**
   - (a) `dontAsk` with an allowlist: edits in the task's worktree and the run's check commands.
     Anything else is refused, and the agent can raise it with `ask` as a Blocking question.
   - (b) `auto` mode: Claude Code's classifier decides.
   - (c) `bypassPermissions` inside task worktrees.
   - *Recommendation: (a), if S1 confirms it.*
2. **How agents reach the orchestrator.** (a) Chorus tools over a loopback MCP endpoint; (b) files the
   agent writes, read after each turn. *Recommendation: (a). It is a structured channel and carries
   a question's class, options and default.*
3. **Where run files live.** (a) Split, as §5 says; (b) everything in the repo; (c) everything outside
   it. *Recommendation: (a).*
4. **What the reviewer sees.** *Recommendation: the whole repo at the phase branch, pointed at the diff
   since the last phase tag.*
5. **How tasks merge.** *Recommendation: squash each task into the phase branch, then merge and tag
   the phase.*
6. **What text Telegram carries.** Questions need their text and options. *Recommendation: question
   text, options and short titles, and never code, diffs, file contents or paths. A project can be
   set to status only (HANDOFF-P13 decision 2).*
7. **Grill Me.** (a) Matthew installs it himself for kickoff; (b) Chorus loads a copy from its own
   folder with `--plugin-dir` (D90). Either way, read the skill's prompt first. *Recommendation: (a)
   for the first runs.*
8. **DeepSeek.** *Recommendation: through opencode on OpenRouter now, with capped keys; the DeepSeek
   Harness (D59) stays in P14.*
9. **A budget for real-model scenarios:** an OpenRouter cap per run and overall, and how much of the
   subscriptions' allowance testing may use.
10. **The spec's home.** Commit it to the repo (which is public) as `_docs/loop/SPEC.md`, or keep it
    local?
11. **The test bot.** At slice 1's S4, Matthew creates a dev-only bot with BotFather and enters its
    token in the `p15` instance's Settings, never in chat.
