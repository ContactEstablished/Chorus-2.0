# Chorus 2.0 — Architecture

Status: **proposed** (2026-10-18). Sections marked *spike* are not settled until the matching P0 spike
in [PLAN.md](PLAN.md) reports.

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
│  │ mic overlay          │              │      │      ◄── agent server bridges │ │
│  │ fleet switcher strip │              │      ▼                               │ │
│  └──────────────────────┘              │ Store (SQLite) · Vault (DPAPI)       │ │
│                                        │ Git · Narrator · Council · Voice     │ │
│                                        └──────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────────┘
        │ PTY                                     ▲ hooks POST / files / HTTP+SSE
        ▼                                         │
  claude · codex · opencode · grok · shell   (child processes, in a Job Object)
```

- **One Tauri process** hosts every window and the whole back end. Agents are child processes attached to
  ConPTY, grouped in a Windows Job Object so Close (or a crash of 2.0) never leaves orphans.
- **Sessions belong to the back end.** A window attaches to a `session_id` and can come and go freely.
- **Windows:** `main` (frameless, custom title bar), `splash` (short-lived), `overlay` (mic, always on top,
  never focused), `fleet` (status strip, always on top, never focused). Each window gets only the Tauri
  capabilities it needs (Tauri 2's per-window permission files).

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
│  │  ├─ sessions/  agents/  events/  store/  vault/  git/  narrator/  voice/  council/ …
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
  TypeScript bindings are **generated** from the Rust types (proposed: `tauri-specta`), so a renamed field
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
- **Restore on boot:** sessions marked running are relaunched (with resume); runs whose process is gone are
  closed out with a reason.
- **Writes that submit:** prompt text and Enter are sent as **two writes**, so the TUI doesn't treat Enter
  as part of a paste (LESSONS L4).

## 5. Agent channels

The Friendly view, the state machine and durable history all depend on knowing what an agent is doing.
Each adapter declares which channels it has; the UI degrades visibly where one is missing.

| | Claude Code 2.1.300 | Codex 0.162.1 | opencode 1.18.34 | Grok 1.0.5 | Shell |
|---|---|---|---|---|---|
| Lifecycle events | **Hooks** via `--settings <file>` (verified in Chorus 1; 2.1.300 adds `PermissionRequest`, `StopFailure`, `PostToolUseFailure`) | **Hooks**: `codex features list` shows `hooks` stable and on (event names: *spike S5*) | **Server events**: run the TUI as `opencode attach <url>` against a `opencode serve` that 2.0 starts (*spike*) | none known (*spike*) | none |
| Timeline content | transcript JSONL (+ `subagents/` files, LESSONS L6) | rollout JSONL (LESSONS L8 for shapes) | server API: sessions, messages, parts | PTY only | PTY only |
| Plan / todo | `TodoWrite` tool calls in the transcript | `update_plan` calls in the rollout (*spike*) | todo parts via API (*spike*) | — | — |
| Questions | `AskUserQuestion` tool (structured options) | final message classification | API (*spike*) | classification | — |
| Answer an approval | candidate: `PermissionRequest` hook held open until you decide in 2.0, returning the decision (*spike*); fallback: keystrokes | hook decision if supported (*spike*); fallback: keystrokes | server permission reply (*spike*) | keystrokes | — |
| Queue a message for the next pause | 2.0 holds it and writes it after `Stop` | `codex queue --thread <id> --message <text>` (verified present) | `prompt_async` (*spike*) | hold until PTY-silent | hold |
| Resume | `--resume <id>` | session id lookup (Chorus 1 adapter) | via server session | resume flag (Chorus 1) | — |

**Fallbacks are explicit.** If a CLI update breaks a parser, the adapter reports "Friendly view
unavailable for codex 0.170: unrecognised rollout format" in the session header, and Raw keeps working.
Parsers are tested against **recorded fixtures per CLI version**, and an unknown version is *tried* and
reported, never silently disabled (LESSONS L7).

**Hook transport** (from Chorus 1, LESSONS L1–L2): a per-session settings file under 2.0's app data holds
hook commands that POST the payload to `127.0.0.1:<port>/hook/<session-token>`. Fire-and-forget hooks
must discard the response (`curl -o NUL`), because hook stdout is a decision channel. Only the approval
hook deliberately returns a body. The listener is bound to loopback, and tokens are random per session and
revoked on Close.

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

At each turn end, a small model reads the last prompt, the final message, the plan, tool-call summaries and
the diff stat (all scrubbed), and returns JSON: `now`, `blocker`, `next`, `is_question`, and up to four
`actions` (`kind`, `title`, `what`, `sends`, `eta`). One call per turn end, cost recorded against the
session, provider and key chosen in Settings (OPEN-QUESTIONS Q7). Done or dismissed actions are fed back
in, so they are not suggested again. With no narrator configured, Friendly shows a generic Continue and
Review step, and a note saying why.

## 10. Proposed dependencies (pending approval — OPEN-QUESTIONS Q9)

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
| Tauri plugins | `global-shortcut`, `single-instance`, `dialog`, `shell`, `opener` (later `updater`) | Official |
| Front end | `vue` 3.5, `pinia`, `vite`, `typescript`, `vue-tsc`, `vitest` | Chosen stack |
| Terminal | `@xterm/xterm` 6 + `addon-fit`, `addon-webgl`, `addon-search`, `addon-unicode11` | Same as Chorus 1; search covers scrollback search |
| Splash | `three` | 3D splash (alternative: hand-written WebGL, if you'd rather not add it) |
| Fonts | Bricolage Grotesque, Instrument Sans, JetBrains Mono, bundled locally (OFL) | The mockup's type; no network at runtime |
| Voice | `whisper-cli.exe` downloaded on first use, not linked | Proven in Chorus 1; avoids building whisper.cpp from source |

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
| `voice` | `voice*.ts`, `whisper*.ts`, `hotkey*.ts`, `voiceOverlay.ts` |
| `measure` | `attention*.ts`, `attributionCore.ts`, `subscriptionMeter.ts`, `engineLedger*.ts`, `dayReport*.ts` |
| `chorus-core::projects` | `projectLifecycleCore.ts`, `src/shared/projectColors.ts` |
| `fleet` | `fleetRegistry*.ts`, `src/shared/fleetRoster.ts` |

Not ported (discarded): `team*`, `memoryService.ts`, `neo4jClient.ts`, `codeIndexCore.ts`,
`symbolExtractorCore.ts`, `graphSchemaCore.ts`, `provenanceCore.ts`, `contextUsage*.ts`, `remote*`,
`kimi.ts`, `apiSession.ts`, `notifications.ts` (OS toasts).
