# Chorus 2.0 — Architecture

Status: **agreed** (2026-10-18; D9–D14, D30). Sections marked *spike* are not settled until the matching
P0 spike in [PLAN.md](PLAN.md) reports.

## 1. Shape of the system

```
┌──────────────────────────── Tauri app (one process) ────────────────────────────┐
│                                                                                 │
│  WebView2 windows (Vue 3)              Rust back end                            │
│  ┌──────────────────────┐   commands   ┌──────────────────────────────────────┐ │
│  │ main window          │ ───────────► │ SessionManager ── PTY host (ConPTY)  │ │
│  │  Overview / Session  │ ◄─────────── │      │                │              │ │
│  │  Friendly / Raw      │   events +   │      ▼                ▼              │ │
│  ├──────────────────────┤   channels   │ AgentEvents ◄── hook listener (HTTP) │ │
│  │ splash window        │              │      │      ◄── transcript tailers   │ │
│  │ fleet switcher strip │              │      │      ◄── agent server bridges │ │
│  │  (never activated)   │              │      ▼                               │ │
│  └──────────────────────┘              │ Store (SQLite) · Vault (DPAPI)       │ │
│                                        │ Git · Narrator · Council             │ │
│                                        └──────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────────┘
        │ PTY                                     ▲ hooks POST / files / HTTP+SSE
        ▼                                         │
  claude · codex · opencode · grok · shell   (child processes, in a Job Object)
```

- **One Tauri process** hosts every window and the whole back end. Agents are child processes attached to
  ConPTY, grouped in a Windows Job Object so Close (or a crash of 2.0) never leaves orphans.
- **Sessions belong to the back end.** A window attaches to a `session_id` and can come and go freely.
- **Windows:** `main` (frameless, custom title bar), `splash` (short-lived), `fleet` (status strip, always
  on top, never activated, so an external dictation app keeps focus: D23; it takes input through a native
  input shield, D38, spike S3). Each window gets only
  the Tauri capabilities it needs (Tauri 2's per-window permission files).
- **Dictation compatibility (D23):** voice lives in a separate app (D22). Chorus never steals focus, the
  composer and the Raw terminal accept inserted or pasted text, and nothing Chorus receives is
  auto-submitted.

## 2. Repo layout

```
Chorus-2.0/
├─ src/                    Vue 3 front end (Vite)
│  ├─ app/                 shell: rail, title bar, status bar, routing between screens
│  ├─ screens/             overview, sessions, session (friendly/raw), bots, automations, documents, notes, settings
│  ├─ components/          shared UI (cards, chips, status dot, composer, terminal)
│  ├─ stores/              Pinia stores, one per domain, fed by back-end events
│  ├─ ipc/                 generated bindings (do not edit) + thin typed wrappers
│  └─ styles/tokens.css    design tokens for both themes (see design/DESIGN.md)
├─ src-tauri/              the app crate: Tauri setup, commands, IO (PTY, SQLite, files, HTTP)
│  ├─ src/
│  │  ├─ sessions/  agents/  events/  store/  vault/  git/  narrator/  council/ …
│  │  └─ main.rs / lib.rs
│  ├─ capabilities/        per-window permissions
│  └─ migrations/          numbered, forward-only SQL
├─ crates/chorus-core/     pure logic: no IO, no Tauri, no clock, no randomness passed in implicitly
│                          (state machine, event normalisation, scrubber, effort and permission mapping,
│                           capability tables, transcript parsers, council parsing, ranking …)
├─ _docs/                  plans, decisions, lessons, spike results
└─ _assets/                mockups and reference images
```

**The `chorus-core` rule:** if it can be computed from inputs, it lives in `chorus-core` with unit tests and
fixtures. `src-tauri` does IO and wiring only. The compiler enforces it: `chorus-core` cannot depend on
`tauri`, `tokio`'s IO, `rusqlite` or `std::fs`.

## 3. IPC

- **Commands** (UI → back end) are `#[tauri::command]` functions taking and returning serde types.
  TypeScript bindings are **generated** from the Rust types with `tauri-specta` (D12), so a renamed field
  is a compile error on both sides.
- **Validation lives at the Rust boundary.** Serde rejects malformed shapes; commands then check
  semantics (ids exist, paths are inside the project, lengths are bounded). No validation library in the
  front end is required. Chorus 1's Zod-in-preload CSP failure and its "object could not be cloned" trap
  both disappear: payloads are JSON, and Pinia proxies serialise fine.
- **Events** (back end → UI) carry state changes: session state, agent events, notifications.
- **Channels** carry high-volume ordered streams: PTY output per attached view, using `tauri::ipc::Channel`.
  Output is batched (for example every 8–16 ms) so a 10 MB burst doesn't flood the webview (spike S1).

## 4. Sessions and the PTY pipeline

```
child stdout ─► ConPTY ─► reader thread ─► scrubber ─┬─► scrollback mirror (disk, ring-capped)
                                                     ├─► liveness clock (last byte seen)
                                                     └─► attached views (channel, batched)
UI keystrokes ─► command ─► SessionManager ─► PTY write
composer/actions ─► ActionRouter ─► structured channel if available, else PTY write
```

- **Spawn:** resolve the CLI (npm `.cmd` shims need care: LESSONS L14), build a **minimal env**
  (allow-list copied from the parent, plus imposed values such as `TERM=xterm-256color`,
  `COLORTERM=truecolor`, auto-update off, and decrypted keys), cwd = project or worktree.
- **Run records:** a *session* is long-lived (name, project, bot, history); each launch is a *run* (pid,
  CLI version, args minus secrets, exit code). Restart = end the current run and start a new one in the same
  session, resuming the conversation when the agent supports it.
- **Restore on boot:** sessions marked running are relaunched (with resume from P2); runs whose process is
  gone are closed out with a reason. A clean quit records each live run as `chorus-closed` but leaves the
  session running, so it comes back. A run still open at start-up is recorded as `chorus-crashed`. A child
  that exits while Windows is shutting down doesn't count as exiting by itself (`chorus_core::session`).
- **One emit path:** scrubbed, batched output goes to a per-session hub, which appends to the mirror
  before sending to views. A view that attaches is sent the mirror, then live output, under one lock,
  so nothing is lost or repeated. Views come and go; the session runs headless meanwhile. Chorus answers
  ConPTY's opening cursor request itself (LESSONS L32).
- **History across runs:** when a run starts in a session that has output (Restart, relaunch), the old
  screen and a dividing line are pushed into scrollback and the run gets a blank screen. Anything Chorus
  writes inside the viewport would be erased by ConPTY. Views must set xterm.js's
  `scrollOnEraseInDisplay` (`src/components/terminal.ts`, LESSONS L34).
- **Close spares windows:** Close ends the run's job but leaves processes with visible windows, and what
  runs under them, alive: a browser the agent opened survives (LESSONS L22). A locked session refuses
  Close and Restart.
- **Writes that submit:** prompt text and Enter are sent as **two writes**, so the TUI doesn't treat Enter
  as part of a paste (LESSONS L4).

P6's first slice adds `sessions::actions::ActionRouter` and the shared `sessions::delivery` path
(D75, HANDOFF-COMMON §4). Its typed action request includes the reviewed run and a stable UUID.
The prompt and an uncertain receipt commit atomically before any PTY write; a completed write is
not agent acknowledgement. A matching same-run hook echo within 60 seconds links the existing prompt
and marks the receipt observed. Repeated requests never write again. No automatic retry after an
uncertain write, crash or restart. Busy/unknown state and pending asks refuse; the queue is still
pending. Claude question/approval replies now use held HTTP hooks (D76); other agents refuse with
a Raw fallback. A live hook waits at most 120 seconds, and `Answer in Raw` releases it without a
decision. Scrubbed reply attempts commit before the response; the receipt does not assert which
answer won the race with Raw. Run/ask identity, lock and expiry are checked again at dispatch.
No reply is automatically replayed after a timeout, restart or crash.

## 5. Agent channels

The Friendly view, the state machine and durable history all depend on knowing what an agent is doing.
Each adapter declares which channels it has; the UI degrades visibly where one is missing.

Verified in spike S5 against the installed versions ([spikes/S5.md](spikes/S5.md) has the evidence and
the caveats). *(docs)* marks what could only be read, not run.

| | Claude Code 2.1.300 | Codex 0.162.1 | opencode 1.18.34 | Grok 1.0.62 | Shell |
|---|---|---|---|---|---|
| Lifecycle events | **HTTP hooks** from `--settings <file>`, token in a header from the child's env (D36). Not `SessionStart` (unsupported over HTTP) | **Command hooks** via `-c hooks.<Event>=…` + `--dangerously-bypass-hook-trust`, through the `chorus-hook` relay (D37); needs `PATHEXT` | **Server events** (SSE `/event`) from an `opencode serve` 2.0 starts; TUI as `opencode attach <url> --session <id>` | Hooks only from global files; ACP over stdio *(docs)*; open question Q18 | none |
| Turn start / end | `UserPromptSubmit` … `Stop`, paired first-in-first-out (a queued prompt fires `UserPromptSubmit` early); an interrupted turn sends no `Stop` | `UserPromptSubmit` … `Stop`; rollout `task_started` / `task_complete` | `session.status` busy … `session.idle` | *(docs)* | — |
| Timeline content | transcript JSONL (+ `subagents/` files, LESSONS L6); `Stop.last_assistant_message` | rollout JSONL (LESSONS L8); `Stop.last_assistant_message` | server API: sessions, messages, parts | PTY only for now | PTY only |
| Plan / todo | `TaskCreate`/`TaskUpdate` + `TaskCreated`/`TaskCompleted` hooks, only with `CLAUDE_CODE_ENABLE_TODO_TOOLS=1` (undocumented) | **none in TUI mode** (not in rollout, no hook); app-server `turn/plan/updated` *(docs)* | `todo.updated` events, `GET /session/{id}/todo` | — | — |
| Questions | `AskUserQuestion` at `PreToolUse`, **answered** with `updatedInput.answers` | final message classification | `question.asked`, answered via `POST /question/{id}/reply` | classification | — |
| Answer an approval | **held `PermissionRequest` hook** returns allow / deny + message; Claude's own dialog stays live in parallel and the first answer wins | **`PermissionRequest` hook** prints allow / deny + message | `POST /permission/{id}/reply` once / always / reject + message | keystrokes (ACP only for structured *(docs)*) | — |
| Queue a message for the next pause | the TUI queues typed prompts natively; 2.0 can also hold and write after `Stop` | `codex queue --thread <id> --message <text>` (works on embedded sessions) | `prompt_async` (queued while busy) | hold until PTY-silent | hold |
| Resume | `--resume <id>` | `codex resume <id>` | via server session (`attach --session`) | `--resume <id>` | — |

*opencode is now 2.1.1 (D58, LESSONS L41): its column describes 1.18.34. In 2.x the TUI connects with
`opencode --server <url> --session <id>`, the API is under `/api/`, turns run from `session.inbox.delivered`
to `session.execution.succeeded`, questions are forms (`form.created`), and approvals are
`permission.asked`, answered with `{decision}`. A model or effort picked in the TUI stays on the session,
so a resumed run sets the session's own again (D60). A prompt typed during a turn joins the running
execution, and opencode can't replay events a dropped stream missed (L42).*

**Fallbacks are explicit.** If a CLI update breaks a parser, the adapter reports "Friendly view
unavailable for codex 0.170: unrecognised rollout format" in the session header, and Raw keeps working.
Parsers are tested against **recorded fixtures per CLI version**, and an unknown version is *tried* and
reported, never silently disabled (LESSONS L7).

**Hook transport** (S5, D36, D37; Chorus 1 background in LESSONS L1–L2): the listener is bound to
loopback and hooks POST to `127.0.0.1:<port>/hook/<session-id>`. The per-session token is random,
revoked on Close, and travels **only in the child's environment**: Claude Code's HTTP hooks send it as
`Authorization: Bearer $CHORUS_HOOK_TOKEN` (`allowedEnvVars`), and the `chorus-hook` relay used for
Codex reads it from its own environment. The settings file and argv never hold it. A hook reply is a
decision channel, so fire-and-forget events get an empty 2xx body; only approval and question hooks
return one.

## 6. Session states

| State | Meaning | Typical sources |
|---|---|---|
| **Working** | a turn is in progress | prompt submitted (hook) → until turn end |
| **Awaiting approval** | the agent asked permission for an action and nothing runs until you answer | permission hook or event; the TUI's permission prompt |
| **Needs your answer** | the turn ended with a question for you | `AskUserQuestion`; narrator or heuristic classification of the final message |
| **Ready for next step** | the turn ended and the agent is idle | turn-end event, final message not a question |
| **Completed** | you marked it done, or the agent exited cleanly after its last turn | user action; exit code 0 |
| **Failed** | the process died, or the turn ended in an error | non-zero exit, `StopFailure`, API error events |

Transitions live in `chorus-core` as a pure state machine over `AgentEvent`s plus timer ticks. A stale
Working state (no event and no PTY bytes for N seconds) is surfaced as "no signal", never silently flipped.

## 7. Data model (sketch)

SQLite via `rusqlite`, WAL mode, `synchronous=FULL` (history must survive power loss; write volume is low),
forward-only numbered migrations in `src-tauri/migrations/`.

| Area | Tables |
|---|---|
| Workspace | `projects`, `project_tags`, `settings` |
| Sessions | `sessions` (name, note, project, bot, state, lock), `runs` (pid, agent, version, exit), `worktrees` |
| History | `prompts` (every prompt, source, scrubbed text), `turns` (start, end, tokens), `messages` (final agent message per turn), `tool_calls` (name, target, outcome — not raw output), `asks` (questions and approvals and their answers), `agent_events` (raw, pruned after N days) |
| Suggestions | `actions` (suggested next steps, kind, sends, status: open, sent, done, dismissed) |
| Keys and providers | `credentials` (DPAPI blobs, label, last used), `providers`, `routes`, `model_catalog`, `shortlist`, `capped_keys` |
| Bots and automations | `bots`, `automations`, `automation_runs` |
| Knowledge | `notes`, `documents` |
| Attention | `notifications`, `attention_samples` |
| Cost | `costs` (per run, turn, credential), `usage_meter` |
| Council | `council_cases`, `council_members`, `council_runs`, `council_messages` |
| Routing | `endpoint_samples`, `tier_pins` |

Rule: anything written to `prompts`, `messages`, `tool_calls` or `asks` passes the scrubber first.

## 8. Secrets

- **Vault:** keys are encrypted with Windows DPAPI (`CryptProtectData`, current-user scope) through the
  `windows` crate and stored as blobs in SQLite. They are decrypted only at launch, and never shown again
  after saving. 2.0 starts empty: keys are re-entered (D5).
- **Injection:** decrypted keys go into the child's environment only. Never argv, which other processes
  can read (LESSONS L12). Never a settings file. Never a log line.
- **Scrubbing:** every key in the vault (and known key patterns) is redacted from PTY output, scrollback,
  history rows, narrator input, council transcripts and exports.
- **OpenRouter capped keys:** minted per launch with a spend cap, revoked on run end; spend reconciled from
  analytics afterwards (LESSONS L16).
- **Repo hygiene:** a secret-pattern scan runs before commits and in CI.

## 9. Narrator (Now / Blocker / Up next and suggested steps)

P6 implementation separates pure plan/message parsing, queue gates and narrator validation in
`chorus-core` from capture, SQLite, delivery and HTTP in the shell. Migrations 0015–0017 persist
progress, queue entries and narrator attempts/actions respectively. Claude task hooks populate plan
snapshots; a bounded transcript cursor records visible assistant text without thinking or tool output.
Unknown correlation stays explicit. Other adapters expose missing capture capabilities.

The queue uses D75's delivery receipts and serializes against Raw input, Close, Restart and Lock.
Instructions are scrubbed and committed before delivery; uncertain attempts never replay. Only
queued entries can be cancelled. Restart requires explicit rearming for the reviewed run. The user
checks that Raw's prompt is empty before delivery; any later Raw input invalidates that assertion.

Narrator calls run once per successful completed turn, without startup backfill/retry. Input plans
must be complete and belong to the same run/turn. Responses are bounded, scrubbed and validated;
turn/run/settings are rechecked after the call, preserving cost but suppressing stale suggestions.
Current panels do not expose model suggestions from an earlier run/turn. Exact trimmed send text
identifies resolved actions, preserving case and internal whitespace in code. Draft insertion appends
without sending; `is_question` never overrides structured agent state. A live paid narrator remains
an acceptance gate; fake transport tests cover response/cost/failure behavior.

At each turn end, a small model reads the last prompt, the final message, the plan, tool-call summaries and
the diff stat (all scrubbed), and returns JSON: `now`, `blocker`, `next`, `is_question`, and up to four
`actions` (`kind`, `title`, `what`, `sends`, `eta`). One call per turn end, cost recorded against the
session. The model is a cheap OpenRouter model chosen in Settings; the narrator is on by default and can
be switched off per project (D28). Done or dismissed actions are fed back in, so they are not suggested
again. With no narrator configured, Friendly shows a generic Continue and Review step, and a note saying
why.

## 10. Dependencies (approved 2026-10-18, D30)

Anything not on this list needs Matthew's approval first (CLAUDE.md).

| Purpose | Crate / package | Why this one |
|---|---|---|
| App shell | `tauri` 2.x (CLI 2.12), `@tauri-apps/api` | Chosen stack. Pin 2.x; 3.0 is alpha |
| Typed bindings | `specta` + `tauri-specta` | Generated TS for commands and events |
| PTY | `portable-pty` 0.9 | ConPTY on Windows, used by WezTerm |
| Async runtime | `tokio` | Already Tauri's runtime |
| SQLite | `rusqlite` 0.40 (`bundled`) | Synchronous, simple, bundled SQLite |
| Serialisation | `serde`, `serde_json` | Standard |
| Windows APIs | `windows` | DPAPI, Job Objects, `GetLastInputInfo`, foreground window |
| Hook listener and HTTP client | `axum` (loopback server), `reqwest` (OpenRouter, Telegram) | Standard pair on tokio |
| Errors and logs | `thiserror`, `anyhow`, `tracing` | Standard; logs pass the scrubber |
| Tauri plugins | `single-instance`, `dialog`, `shell`, `opener` (later `updater`) | Official |
| Front end | `vue` 3.5, `pinia`, `vite`, `typescript`, `vue-tsc`, `vitest` | Chosen stack |
| Terminal | `@xterm/xterm` 6 + `addon-fit`, `addon-webgl`, `addon-search`, `addon-unicode11` | Same as Chorus 1; search covers scrollback search |
| Splash | `three` | 3D splash (alternative: hand-written WebGL, if you'd rather not add it) |
| Splash types | `@types/three` (dev only, approved 2026-10-19) | three.js ships no types (S7). Its own dependencies install with it but are never bundled |
| Fonts | Bricolage Grotesque, Instrument Sans, JetBrains Mono, bundled locally (OFL) | The mockup's type; no network at runtime |

## 11. Port map (Chorus 1 → 2.0)

Port the behaviour and the tests; the shape will differ. Paths are relative to the Chorus 1 repo.

| 2.0 module | Chorus 1 source |
|---|---|
| `chorus-core::scrub` | `src/main/services/scrubber.ts`, `secret-patterns.json` |
| `chorus-core::effort`, `capabilities` | `src/main/adapters/argLevels.ts`, `capabilities.ts`, per-agent adapters |
| `agents::env` | `src/main/adapters/env.ts` (allow-list + imposed values) |
| `agents::*` | `src/main/adapters/{claude,codex,opencode,grok,shell}.ts`, `cliShimCore.ts`, `cliDetect.ts` |
| `chorus-core::resume` | `src/main/services/resumeCore.ts` |
| `events::hooks` | `src/main/services/agentEvents.ts`, `agentEventsCore.ts` |
| `chorus-core::turns` | `src/main/services/turnsCore.ts`, `promptCaptureCore.ts` |
| `sessions::scrollback` | `src/main/services/scrollbackCore.ts`, `scrollbackStore.ts`, `sessionOutput.ts` |
| `sessions::restore` | `src/main/services/restore.ts` |
| `chorus-core::lock` | `src/main/services/agentLockCore.ts` |
| `vault` | `src/main/services/vault.ts`, `vaultCore.ts` |
| `git::worktrees` | `src/main/services/worktrees.ts`, `git.ts` |
| `providers` | `modelCatalog*.ts`, `openrouterKeys.ts`, `src/main/routing/*` |
| `council` | `councilCore.ts`, `councilDocketCore.ts`, `councilService.ts`, `councilMembers.ts` |
| `measure` | `attention*.ts`, `attributionCore.ts`, `subscriptionMeter.ts`, `engineLedger*.ts`, `dayReport*.ts` |
| `chorus-core::projects` | `projectLifecycleCore.ts`, `src/shared/projectColors.ts` |
| `fleet` | `fleetRegistry*.ts`, `src/shared/fleetRoster.ts` |

Not ported (discarded): `team*`, `memoryService.ts`, `neo4jClient.ts`, `codeIndexCore.ts`,
`symbolExtractorCore.ts`, `graphSchemaCore.ts`, `provenanceCore.ts`, `contextUsage*.ts`, `remote*`,
`kimi.ts`, `apiSession.ts`, `notifications.ts` (OS toasts).

Not ported (moved to the separate dictation app, D22): `voice*.ts`, `whisper*.ts`, `hotkey*.ts`,
`voiceOverlay.ts`.
