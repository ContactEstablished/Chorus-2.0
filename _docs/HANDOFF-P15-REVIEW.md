# P15 review: the autonomous phase loop (in progress)

A working draft, updated after each slice of [HANDOFF-P15-loop.md](HANDOFF-P15-loop.md). It follows
HANDOFF-COMMON §6's sections. Raw evidence is in `_verify/p15/`, which is never committed.

## 1. Verdict

**Partly done: slices 1 to 3 of 9.** The spikes are done (Telegram's moved to slice 4). One phase at
a time, task after task, runs end to end in the app: scripted and with real Claude Code steps. Steps
call Chorus's own tools to ask questions, log assumptions and report they're done; questions are
answered on the Loop screen, and "make it a rule" stops a later phase asking again. Two real runs on
fixture F1 worked all three phases to Done. Slices 4 to 9 (Telegram, the plan grill, parallel
builders, review, Counsel, limits) are not started.

## 2. Branch

`p15/loop`, from `main` at `d4f4298`.

```
bc51602 P15: a rule reads as the rule, with its question after; one tag per assumption
160ea45 P15 slice 3: Chorus's tools for agents, and questions
7923315 P15: never GPT or Claude through OpenRouter; fixture trust; D55 follow-up (D99)
b01d693 P15: the review draft after slices 1 and 2
77c7ad0 P15: end a STOPPED message's detail with a full stop; show a failing test's name first
aa01550 P15: the Loop screen, fixture F1, and fixes from the first real run
15b2c71 P15: the orchestrator, gatekeeper and run state
613b4c8 P15: the loop's pure core - run files, phase state machine, signatures
9eee629 P15: spike S1 and S2 seen in a terminal
29e0ee6 P15: slice 1 spikes - unattended permissions, allowances, agent tools
6aa0749 P15: record Matthew's answers to the plan's decisions (D98)
8de5d73 P15 plan: the autonomous phase loop (D97)
```

## 3. Files

**Created**

| File | Purpose |
|---|---|
| `crates/chorus-core/src/run_loop/mod.rs` | The phase state machine: plan, build, check, fix, tag; stop conditions; milestones |
| `crates/chorus-core/src/run_loop/files.rs` | `roadmap.md` and `tasks.md` blocks: parse, validate, set a task's status |
| `crates/chorus-core/src/run_loop/signature.rs` | Error signatures from check output (Vitest, Jest, node:test, dotnet, cargo, pytest, Playwright, Go) |
| `crates/chorus-core/src/run_loop/roles.rs` | Role resolution (task, phase, default) and the reviewer vendor rule |
| `crates/chorus-core/src/run_loop/prompts.rs` | The planner's and builder's first prompts |
| `crates/chorus-core/src/run_loop/report.rs` | `gate-report.md`, newest attempt first |
| `crates/chorus-core/src/run_loop/script.rs` | The scripted worker's script format and path checks; scripted questions |
| `crates/chorus-core/src/run_loop/tools.rs` | Chorus's tools for steps: MCP JSON-RPC, the tool list, checking `ask` |
| `crates/chorus-core/src/run_loop/questions.rs` | Answer prompts, the assumptions log and "make it a rule" |
| `src-tauri/src/run_loop/tools.rs` | The tools endpoint: per-step tokens, its MCP config file, what each tool does |
| `src-tauri/src/run_loop/mod.rs` | IPC commands, views, run checks, start and stop |
| `src-tauri/src/run_loop/worker.rs` | The orchestrator: one thread per running run |
| `src-tauri/src/run_loop/gate.rs` | The gatekeeper: check commands in a Job Object, scrubbed |
| `src-tauri/src/run_loop/db.rs` | Runs, phases, steps and checks in SQLite |
| `src-tauri/src/run_loop/tests.rs` | Whole runs against a real git repository with scripted steps |
| `src-tauri/migrations/0023_p15_loop.sql` | The loop's tables |
| `src/views/LoopView.vue` | The Loop screen |
| `scripts/loop-fixtures/make-f1.mjs`, `make-f1-script.mjs` | Fixture F1 and its scripted first phase |
| `_docs/spikes/P15.md` | Slice 1's spikes |

**Modified** (hot files from HANDOFF-COMMON §3 marked ★)

| File | Why |
|---|---|
| ★ `src-tauri/src/ipc.rs` | Nine `loop_*` commands and the `LoopUpdated` event, in one block at the end |
| ★ `src-tauri/src/store/mod.rs` | Migration 0023 listed |
| ★ `src/ipc/bindings.ts` | Regenerated |
| `src-tauri/src/lib.rs` | `run_loop::prepare` before sessions relaunch, `run_loop::start` after |
| `src-tauri/src/events/listener.rs` | `/mcp/{step}`, Chorus's tools, beside `/hook/{session}` |
| `src-tauri/src/sessions/mod.rs` | `CreateSession.loop_step` (never from IPC): a step's flags and token for its first run only; `set_loop_launch` to give them again with an answer; `listener_port`; `end_agent` for an explicit Stop |
| `src-tauri/src/sessions/workspace.rs` | `add_run_worktree`: the run's registered worktree on its run branch |
| `src-tauri/src/git.rs` | `add_worktree_on`, `rev`, `commit_all`, `tag`, `show` |
| `src-tauri/src/automations.rs` | A `loop` prompt source beside `automation` |
| `src-tauri/src/store/delivery.rs`, `crates/chorus-core/src/actions.rs` | Launch prompts' echoes match within 30 min (below) |
| `crates/chorus-core/src/claude.rs`, `src-tauri/src/agents/mod.rs` | `repo_trusted`: Claude Code's trust for a git repository |
| `src-tauri/src/sessions/tests.rs` | `loop_step: None`; a typed-delivery test uses the composer source |
| `src/stores/view.ts`, `src/shell/Rail.vue`, `src/app/App.vue`, `src/components/icons.ts`, `src/ipc/index.ts` | The Loop screen's place, rail entry, icon and types |

## 4. Surface added

- **Migration** `0023_p15_loop.sql`: `loop_runs` (config JSON, state, note, reviewed phase),
  `loop_phases` (progress JSON, stop, tag), `loop_steps` (role, Bot, session and its run, model asked,
  model observed, state, note, summary), `loop_checks` (each check's result, scrubbed output tail,
  signature), `loop_questions` (class, question, options, default, deadline, state, answer, rule,
  delivered).
- **Commands:** `loop_folder_check`, `loop_runs_list`, `loop_run_create`, `loop_run_update`,
  `loop_run_start`, `loop_run_pause`, `loop_run_stop`, `loop_run_approve`, `loop_run_delete`,
  `loop_question_answer`. **Event:** `loop-updated`.
- **Endpoint:** `POST 127.0.0.1:<listener>/mcp/<step>` (MCP, streamable HTTP, JSON answers) with
  `Authorization: Bearer` the step's token. **Environment:** `CHORUS_LOOP_TOKEN` in a step's agent,
  redacted like a key. **Config:** `limits.timedMinutes` (default 15).
- **Files Chorus writes:** in the run's worktree, `.chorus/run/run-state.json` (git-ignored, through
  `.chorus/run/.gitignore`), each phase's `gate-report.md` and task statuses, `assumptions.md` lines
  and `decisions.md` rules; in app data, `loop/mcp-step-<id>.json` (the URL and the token's variable
  name, never the token). Commits and `phase-NN` tags on the run branch, never on the base branch.
- **Worktrees:** `<parent>\.chorus2\<repo>\run-<name>` on `run/<name>`, registered like P5's.
- No dependency added.

## 5. Plan against result

| Slice | Status |
|---|---|
| 1. Spikes | Done: S1 (permissions), S2 (allowances), S3 (agent tools). S4 (Telegram) moved to slice 4 |
| 2. One phase, one task | Done, and beyond: tasks run one after another in dependency order, and passing phases chain to the next until a milestone. Roles are the planner and builder; the reviewer comes in slice 7. The core decides every stop condition the slice has signals for (fix attempts, same signature, wall clock, invalid plan, failing step, stopped task); the others arrive with their slices |
| 3. Agent tools and questions | Done: `ask` (blocking, timed, log only), `log_assumption`, `task_done`; answers and timed defaults go back by resuming the step's conversation; the Loop screen's answer panel and "make it a rule". Claude Code steps only, like slice 2 |
| 4 to 9 | Not started |

Differences from the plan: fixture F1 uses Node's own test runner instead of Vitest, so it needs no
install; each step is a fresh session whose context is the run's files, not a conversation.

## 6. Verification

`npm run check` at `77c7ad0`: typecheck clean; 55 vitest; 118 app and 501 core Rust tests; clippy
clean; secret scan clean.

| What | How | Seen |
|---|---|---|
| Whole runs, deterministic | `run_loop::tests` drive scripted runs against a real git repository and a real `cmd` check | A fix that works passes and tags; the same failure four times stops with the reason; an invalid plan goes back once then stops; checks and roles are required; keys in a check are refused |
| Scripted run in the app | F1 with its scripted first phase, `npm test` as the check | Plan, build (fails: `sum adds two numbers`), fix, pass, tag `phase-01`, waits for Approve (`_verify/p15/loop-scripted.png`) |
| A stuck fix loop in the app | Four builds that fail the same way | STOPPED with the condition, the failing test, the worktree to inspect and the last good state (`loop-stuck.png`) |
| Real run, three phases | F1 with Claude Code (Sonnet) as planner and builder, `npm test` as the check | P01 3 tasks, P02 2, P03 2, each passed first time, tags `phase-01` to `phase-03`, one Approve after P01 (the milestone), then Done in about 8 minutes. Every step records `claude-sonnet-5-5` as the model that answered. The result: 32 tests pass; `sum`, `product`, `help` and the unknown-command pointer behave as the roadmap says; builders logged 7 assumptions; `main` untouched. Allowance used: about 2% of the 5-hour window |
| Unattended flags in a terminal | Spike S1 in the p15 instance's PTY | `dontAsk` refuses without a dialog; `--restricted` keeps files to the worktree |

| Questions, deterministic | `run_loop::tests`: a blocking question waits until answered and becomes a rule that is committed; a timed question takes its default and logs it; the tools endpoint refuses a wrong token, records a question (a key in it scrubbed), an assumption and a summary, and says a stopped step has ended | All pass |
| The answer panel | A scripted question in the app, answered by clicking an option, ticking Make it a rule and Answer (`loop-question.png`) | The rule lands in decisions.md and the step goes on to pass and tag |
| Real questions (scenarios 8 and 10) | F1 whose decisions.md says to ask before wording any message a user sees; Claude Code (Sonnet) steps | P01's planner asked a blocking question with its proposed wording; answered with a rule, its conversation resumed with the answer and finished, logging 4 assumptions and calling `task_done`. P02's `product` used the rule's wording and asked nothing. P03's planner asked about the help text (not covered by the rule); answered, it built exactly that. 37 tests pass, 15 assumptions logged, Done with tags `phase-01` to `phase-03`; allowance moved by under 1% of the 5-hour window |

**Not yet verified:** a Chorus restart in the middle of a real step, or while a step waits on a question (the code
runs the first again and keeps the second waiting; untested in the app); a timed question reaching its
default in the app (unit-tested); the wall-clock stop in the app (unit-tested only); anything but Claude
Code as a step's agent (refused visibly for now).

## 7. Versions verified against

Claude Code 2.1.308 for the spikes, 2.1.310 for the runs (it updated itself during spike S1, see the
spike doc); codex-cli 0.163.0; opencode 2.1.4; Node 24.15; git on PATH.

## 8. Proposed doc changes

**Decisions** (status Proposed):

| ID | Decision |
|---|---|
| P15-D1 | Loop steps run Claude Code with `--restricted --tools <named> --permission-mode dontAsk --allowedTools <edits in the worktree, or the run documents for the planner; each check command> --strict-mcp-config --mcp-config <Chorus's>`, on their first run only; a Restart gives an ordinary interactive session for whoever takes over (confirms D98's decision 1 after S1) |
| P15-D2 | `ask` never holds a tool call open; the answer or timed default arrives as the next prompt (S3: Claude Code gives up at about 60 s) |
| P15-D3 | Check commands live in Chorus's database and are edited only in Chorus, never read from the repository |
| P15-D4 | Each step is a fresh session; its context is the run's files (decisions, spec, tasks, gate report, assumptions), not a conversation |
| P15-D5 | A run works in one registered worktree on `run/<name>`; each passing task is committed there and a phase is tagged when every task passes (per-task branches and squash merges come with slice 6) |
| P15-D6 | The same-signature stop counts fix attempts only, not the first build, so Counsel's trigger (a second fix failing the same way) comes before the stop; three fixes in a row failing the same way stop the task |
| P15-D7 | `run-state.json` is an export of the database and is git-ignored |
| P15-D8 | Prompts passed at launch (`automation`, `loop`) match their echo within 30 minutes; typed prompts keep 60 s (refines D75) |
| P15-D9 | Start refuses a repository Claude Code doesn't trust through its main working tree's own entry, and says how to fix it; Chorus doesn't trust it on Matthew's behalf |
| P15-D10 | Stop ends a step's agent even when someone typed in its session; a normal turn end still leaves a taken-over session open |
| P15-D11 | A step that asks a blocking or timed question ends its turn and Chorus closes its agent; the answer, or the timed default, is the first prompt of its resumed conversation (D96's path), so a waiting step survives a Chorus restart |
| P15-D12 | Time a phase spends on an open blocking question doesn't count against its cap |
| P15-D13 | A timed question's default is taken after `limits.timedMinutes` (15 by default, 1 minute to 24 hours, per run) and logged in assumptions.md |
| P15-D14 | "Make it a rule" appends the answer to the run's decisions.md under "Rules from your answers", committed with the next task |
| P15-D15 | Each step gets its own token for Chorus's tools, in its environment only; the MCP config file names the variable |

**Lessons:** P15-L1 to P15-L6 are in [spikes/P15.md](spikes/P15.md). From slice 2:

- **P15-L7** became **L66** with the fix on `fix/claude-repo-trust` (D100, merged into this branch):
  Claude Code 2.1.310 trusts a git repository, worktrees included, only through its main working
  tree's own entry. P15's run Start uses the fixed check.
- **P15-L8 — A prompt passed at launch can be echoed long after launch:** 103 s behind a trust
  question on 2.1.310, past D75's 60 s window.
- **P15-L9 — `tauri dev` rebuilds and restarts the app on any source change;** an instance that runs
  long jobs needs `--no-watch`.
- **P15-L11 — Told in decisions.md to ask before wording user-facing messages, Claude Code (Sonnet)
  asks at planning time, once, with its proposal,** and a rule from the answer kept the next phase from
  asking (F1, 2.1.310).
- **P15-L10 — specta refuses 64-bit integers and two exported types with one name across crates,**
  and ignores serde's container rename for the exported name.

## 9. Questions for Matthew

**Answered 2026-10-25 (D99):** 1, yes, and the spec's last resort through OpenRouter is withdrawn: never
GPT or Claude through OpenRouter; an OpenRouter last resort is one of `~z-ai/glm-flash-latest`,
`~deepseek/deepseek-v4-flash-latest` or `~moonshotai/kimi-latest`. 2, yes. 3, yes, as its own fix
after slice 3: done as D100 on `fix/claude-repo-trust`, merged into this branch.

1. **Claude and GPT models through OpenRouter in loop roles.** You said they should always use your
   subscriptions. Is that only for testing (my reading), or should Settings → Models refuse an
   OpenRouter route for Claude and GPT models? *Recommendation: refuse it in the loop's roles, with the
   reason shown, since the spec's "Sol through OpenRouter as last resort" contradicts your answer.*
2. **Fixture trust.** Each new fixture repository needs Claude Code's trust once (P15-L7). May I answer
   Yes for repositories I create under `%TEMP%\chorus-p15`? *Recommendation: yes; it is the folder you
   approved, and nothing else is affected.*
3. **D55 on 2.1.310.** Its trust entry for the `.chorus2` folder looks ineffective now (P15-L7).
   *Recommendation: a small follow-up outside P15 to re-measure and correct D55.*

## 10. Risks and known issues

- A builder can edit any file in the worktree, including `package.json` scripts a check runs and the
  phase's `tasks.md`. Slice 6's scope check must read claims from the plan's commit, and checks are
  safer calling tools directly. Test integrity (slice 6) covers weakened tests.
- Only Claude Code runs steps so far; Codex and opencode first prompts and MCP configuration come
  with slice 7 (spike S3 has the shapes).
- Questions are answered only on the Loop screen until Telegram (slice 4).
- A run started before `run-state.json` was ignored has it tracked, so it shows as modified after a tag
  (the F1 run made before that fix).
- Step sessions stay in the session list after they finish (exited), one per step.
- `chorus-p15` fixtures and the p15 instance's data are disposable test material.

## 11. How to review

Read `crates/chorus-core/src/run_loop/mod.rs` (the decisions), then `src-tauri/src/run_loop/worker.rs`
(how they are carried out), `gate.rs`, and `tests.rs`. Then in a dev instance:

1. Make F1: `node scripts/loop-fixtures/make-f1.mjs <folder>`, and start Claude Code there once to
   trust it.
2. Make a Claude Code Bot, then Loop → New run: the folder, the Bot as planner and builder, one check
   `npm test` with 120 s.
3. Start it and watch the phases fill in; open a step's session to see its terminal.
4. When it waits at P01, look at the run branch and tag, then Approve and let it finish.
5. For a free run, set "Scripted steps" to a file from `make-f1-script.mjs`; with `--question` its
   first build asks a question to answer on the Loop screen.
6. For real questions, make F1 with `--always-ask`; its planner asks about message wording.
