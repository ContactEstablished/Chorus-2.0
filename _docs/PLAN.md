# Chorus 2.0 — Plan

Written 2026-10-18. Chorus 2.0 is a ground-up rebuild of Chorus on **Tauri 2 (Rust) with a Vue 3 web UI**,
keeping 73 of Chorus 1's 92 features, dropping 14, moving the 4 voice features to a separate dictation
app (D22), deferring 1, and adopting the "Harness" mockups in `_assets/` as the new UI direction.

| Doc | What it holds |
|---|---|
| [SCOPE.md](SCOPE.md) | Every Chorus 1 feature: kept (and where it lives in 2.0), changed, new, discarded |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Process model, repo layout, IPC, agent event model, data model, secrets, approved dependencies |
| [design/DESIGN.md](design/DESIGN.md) | Screens, tokens (Glacier and Dusk), status vocabulary, the splash brief |
| [DECISIONS.md](DECISIONS.md) | Decision log for 2.0 (fresh numbering, D1…) |
| [LESSONS.md](LESSONS.md) | What Chorus 1 learned the hard way about agent CLIs, Windows and measurement |
| [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md) | Questions still open (each with a default), and where each resolved question was decided |

---

## 1. What changes, in one paragraph

Chorus 1 is a terminal grid that happens to know a little about its agents. Chorus 2.0 is a **supervisor**:
the first screen answers *"what needs me right now?"* (Overview, grouped Needs you / In flight / Finished),
and each session opens in a **Friendly** view (a readable timeline of what the agent did, its plan, and
any question or approval it is waiting on, with suggested next steps) or a **Raw** view (the real terminal).
Both views are the same running process — switching never restarts the agent. To make that honest, 2.0
reads agent state from **structured channels** (hooks, transcript files, agent servers) rather than
scraping the screen, and records **every prompt durably** in SQLite.

## 2. Principles

1. **Supervise, don't just host.** The default screen ranks sessions by what they need from you.
2. **The terminal is the truth; Friendly is a lens.** The PTY always runs the real CLI. Friendly is built
   from structured events and never blocks access to Raw.
3. **Structured signals over screen-scraping.** Hooks, transcripts and agent servers first. PTY silence is
   only ever a liveness extender (see LESSONS L3), never the source of "working".
4. **The back end owns sessions.** Windows and views attach to a session id. The UI never spawns processes.
5. **Pure core, thin shell.** Business logic lives in the `chorus-core` crate: no IO, no Tauri, unit-tested.
   (Chorus 1's `*Core.ts` rule, now enforced by a crate boundary instead of a naming convention.)
6. **Nothing is lost.** Every prompt is written to SQLite before or as it is delivered, and survives a crash
   or reboot.
7. **Fail visible.** Every degraded path says why on screen. Chorus 1's worst bugs were silent: version
   gates that disabled options, efforts dropped without error, windows that ignored moves (LESSONS L5, L7, L11).
8. **Secrets leave the vault only as environment variables** for the child process: never argv, never
   plaintext on disk, never logs, history or transcripts.
9. **Measure before forecasting.** No forecast ships until the data it needs is proven complete
   (Chorus 1 recorded 5 of 172 dispatches as finished, so its forecasts had no input).
10. **Fewer controls.** For example, sessions have Restart and Close, nothing else.
11. **Work with dictation like any text field.** Voice lives in a separate, system-wide dictation app
    (D22), so Chorus must not get in its way (D23): Chorus never steals focus (the fleet strip and any
    overlay are non-activating), the composer and the Raw terminal accept inserted or pasted text, and
    nothing Chorus receives is ever auto-submitted.

## 3. Milestones

| Milestone | Phases | You can… |
|---|---|---|
| **M1 Daily driver** | P0–P5 | Replace Chorus 1 for everyday work: launch Claude Code, Codex, opencode, Grok and shells into projects and worktrees; see live state on the Overview; work in the Raw view; restart without losing anything. |
| **M2 Supervisor** | P6–P7 | Run sessions from the Friendly view: answer questions and approvals, queue instructions, follow suggested next steps, and be alerted in-app, in the tray and in the fleet overlay. |
| **M3 Everything kept** | P8–P14 | Bots, Automations, Notes, Documents, council, measurement, routing tiers, the Telegram relay, more agents, and a shippable installer. |

## 4. Phases

Build in this order. Each phase ends with its exit criteria **verified at runtime** (a drive of the real
app or a real CLI), not only by compiling and passing unit tests.

### P0 — Foundations and spikes
Scaffold the Tauri 2 + Vue 3 + TypeScript + Vite + Pinia app with a Cargo workspace (`src-tauri` +
`crates/chorus-core`), generated TypeScript bindings for every command and event, and scripts for `cargo
test`, `cargo clippy`, `vitest`, `vue-tsc` and the secret scan. Separate dev and release app identifiers so
their data never collides (LESSONS L15).

Then run the spikes. Each one either passes or changes the plan before we build on it:

| # | Spike | Pass when |
|---|---|---|
| S1 | **PTY host**: portable-pty (ConPTY) streaming to xterm.js over a Tauri channel | All four agent CLIs launch through their Windows shims; 10 MB of output in a burst renders without freezing the UI; resize and Ctrl+C behave; Close kills the whole process tree (Job Object) |
| S2 | ~~Push-to-talk hotkey~~ | **Removed**: voice moved to a separate app (D22) |
| S3 | **Fleet switcher strip** | An always-on-top strip that is never activated: clicking it leaves `GetForegroundWindow` unchanged, so an external dictation app keeps focus (D23). It can still be dragged through a custom drag (LESSONS L11), measured with `GetWindowRect`, correctly at every DPI scale, and survives display changes |
| S4 | **DPAPI vault** | Round-trip encrypt/decrypt from Rust; a launched child gets the key in its env, and `Win32_Process` shows it nowhere in argv (LESSONS L12) |
| S5 | **Agent channels** | For each agent, we can observe: turn start/end, a pending approval, a question, the final message and the plan; and we can **answer** an approval and **queue** a message. See ARCHITECTURE §5 for the candidate channel per agent. Record what each CLI version really supports |
| S6 | ~~Local transcription~~ | **Removed**: voice moved to a separate app (D22) |
| S7 | **3D splash** | A WebGL scene in WebView2 shows its first frame under ~300 ms and finishes in ≤3 s without delaying session restore |
| S8 | **Installer** | A per-user NSIS build installs without admin rights, sits side by side with Chorus 1, and uninstalls without touching data |

**Exit:** the empty app builds, launches and installs; every remaining spike has a written result in
`_docs/spikes/S<n>.md` (pass, fail, or changed approach).

### P1 — Session engine
SessionManager in Rust: spawn, stream, resize, input and kill PTYs; minimal child environment (allow-list
plus imposed `TERM`/`COLORTERM`, LESSONS L13); secret scrubber on all output; scrollback mirrored to disk
and replayed on attach; restore after restart (relaunch what was running, heal records whose processes
are gone); **Restart** and **Close** as the only session controls (Close kills the session's Job
Object but spares a browser the agent opened, and the child joins its job at spawn, with any process it
started in that moment adopted (D41): S1 findings 1 and 2); agent lock with optional PIN; plain shell sessions (UTF-8 code page). SQLite store with forward-only migrations. DPAPI vault. A bare Raw view to drive it.

**Exit:** a shell and a Claude Code session survive closing their view, an app restart, and a reboot
(they are relaunched); a planted fake key never appears in the pane, the scrollback file or the DB.
*Closed 2026-10-19 on Matthew's call with the reboot not yet run: it takes the same restore path as an
app restart, which was seen working, and only the recorded end reason differs.*

### P2 — Agents and providers
Adapters for **Claude Code, Codex, opencode, Grok and Shell**, each declaring its capabilities (the UI
shows only what is supported, and a test fails if declaration and code disagree). CLI detection with
versions. One effort scale (Fast / Balanced / Deep / Max) mapped per CLI. Permission modes. Conversation
resume where the CLI supports it. Auto-update switched off for launched agents (LESSONS L7). Provider
routes (OpenRouter and OpenAI-compatible gateways), model catalog and shortlist, OpenRouter spend-capped
keys minted per launch and revoked after. Launch dialog (multi-session), session name and note, presets,
saved launch profiles (the seed of Bots in P8).

**Exit:** each agent launches with each declared option, and the option is **observed to apply** (for
example, opencode's effort read back through its server, LESSONS L5), not only passed on the command line.

*Closed 2026-10-19 on Matthew's call with Grok checked only as far as launching: it accepts its options
at launch, but it isn't signed in on this machine, so none could be seen to apply. Every option of Claude
Code, Codex and opencode was seen to apply in the real app, on their own logins, on OpenRouter (with a
capped key per run) and through a gateway.*

### P3 — Agent events and durable history
A localhost listener for agent hooks (per-session capability token, delivered only through the child's
environment: Claude Code HTTP hooks with the token in a header, D36; the `chorus-hook` relay binary for
Codex, D37); transcript and rollout tailers; the opencode server bridge; normalisation into one `AgentEvent` model and
the session state machine (Working, Needs your answer, Awaiting approval, Ready, Completed, Failed —
ARCHITECTURE §6). Turn capture, pairing prompts with turn ends first-in-first-out and closing
interrupted turns without a `Stop` (S5). **Durable history:** every prompt from every source (typed in the
terminal and seen through hooks, the composer, Telegram, automations; dictated text arrives as typed or
pasted text) written to SQLite and scrubbed first; per-turn final messages, tool-call summaries, and
questions and approvals with their answers (D7). Activity lights fed by events, with PTY silence as the
liveness extender only.

**Exit:** across 20 real turns per agent, ≥95% of turns are recorded with start and end, and every
prompt sent survives a forced kill of the app mid-turn.

*Closed 2026-10-19 on Matthew's call: 20 of 20 turns recorded with start and end for Claude Code, Codex
and opencode, and each one's prompt survived a forced kill mid-turn. Grok has no event channel yet
(OPEN-QUESTIONS Q18), so it isn't covered. Bundling the Codex hook relay in the installer is still to do
(D46).*

### P4 — App shell and Overview
The Harness shell, branded Chorus with the seven-bar mark (D24): rail (Overview, Sessions, Bots,
Automations, Documents, Notes, Settings, plus projects), custom title bar, status bar, themes (Glacier
default, and Dusk, D25), text zoom, in-app notices.
**Overview:** status-grouped session cards (or by project, or none), filters, project tags, headline
counts. **Sessions:** searchable list, and the Wall layout of live Raw terminals (D26). **Session view:**
header, status, Raw mode, the session facts panel, and the session strip (filmstrip) for fast switching.
Projects: add, reorder, colours, hide, archive and delete (no import from Chorus 1: D50). Command
palette (Ctrl+K), project switcher (Ctrl+G), prompt history panel with copy. **New session** in Chorus
1's shape (D54) and Matthew's two-pane design (D56), with session tags (D52), worktrees made at launch
(D53, brought forward from P5), and routing tiers for opencode on OpenRouter (D57, from P12). **3D startup splash.** Dictation compatibility (D23): the composer and the Raw terminal accept
text inserted or pasted by another app, and never submit it on their own.

**Exit:** a day of real work in 2.0 without opening Chorus 1.

### P5 — Git and worktrees
New session already makes worktrees, Swarm runs, and Close removes a clean worktree and keeps
uncommitted work (D53, built in P4). Still to come: attaching a session to a worktree an earlier one
left; boot reconcile with `git worktree list` as the authority; worktree panel (branch, ahead/behind,
clean); diff summary in the session facts ("11 files · +612 −48").

**Exit (M1):** the Swarm preset launches N agents in N worktrees, and removing a worktree behind 2.0's
back is healed on the next boot.

### P6 — Friendly view and Next steps
Timeline from structured events: your messages, agent messages, the plan checklist (from the agent's own
todo or plan tool, or a plan file), collapsed tool-call groups, and question or approval cards with
buttons. Approvals and answers delivered through the agent's structured channel where S5 proved one,
otherwise as keystrokes to the PTY, and the agent's own prompt stays usable whenever Chorus doesn't
answer (D29). Composer that **queues** instructions while the agent is busy and delivers them at the
next pause. Now / Blocker / Up next, and **suggested next steps** from the narrator (D28), with "Done —
won't be suggested again". Without a narrator key, Friendly still shows the timeline, the plan and the
cards, and says what's missing.

**Exit:** a Claude Code and a Codex session each run a planned multi-phase task to the end, approved and
steered entirely from Friendly, with the Raw view showing the same history.

### P7 — Attention and notifications
"Needs you" as an inbox worked through with j/k; notification centre (in-app history); tray icon with a
waiting-count badge; the always-on-top fleet switcher strip, which never takes focus (S3, D23); fleet
addressing for Claude sessions; scrollback search; transcript export.

**Exit (M2):** with 2.0 behind other windows, a session needing you is visible within 2 s on the strip
and the tray, and one click lands on its card.

### P8 — Bots, Automations, Notes, Documents
**Bots** grow out of launch profiles and presets: a reusable agent + model + effort + permissions +
instructions (+ provider-neutral skill folders), which is also where Chorus 1's skills framework lands
(D31). **Automations** run a Bot on a schedule (later: on a GitHub trigger, found by polling with `gh`,
D34) and create ordinary sessions. **Notes** are project-tagged (or global) context injected into
agent instructions, the lightweight replacement for the discarded memory graph. **Documents** list
plans, specs and reports that sessions produced, linked back to them.

### P9 — Removed
Voice moved to a separate app, see D22. The number is kept so other docs' phase references stay valid.
What remains for Chorus is the compatibility requirement (D23), built in P4 and P7.

### P10 — Council
Blind answers, anonymous critique, arbiter Findings.md; dissent enforced in code; docket; member setup;
cases and exhibits (attach repo files). Fix the under-reported cost by reconciling from analytics after
revoke (LESSONS L16).

### P11 — Measurement, made visible
Attention sampler, per-task cost attribution, subscription metering, token ledger, day summary, all with
UI this time (session facts, Overview headline, a Usage screen). Mission Control's forecasts only once
P3's turn data meets its completeness bar.

### P12 — Routing tiers
The tiers, Refresh, the account checks, the cache test and endpoint observation came to New session in
P4 (D57). Still to come: the routing inspector (Settings → Routing: its settings, observer status and
history).

### P13 — Phone relay (Telegram)
Prompt sessions, answer questions and approvals, and get digests from your phone, through the same
action pipeline as Friendly. The only remote access in 2.0 (Remote hosts was discarded). Decide here
whether Claude Code's and Codex's built-in remote control modes complement it (D35).

### P14 — More agents and ship
Gemini CLI, Aider, and Ollama (via opencode) adapters; installer polish, code signing decision, update
channel; secret scanning in CI. Once Chorus 1 is retired, the installed name drops its "2" (D51), with a
step that moves an existing "Chorus 2" install.

## 5. Ways of working

- One phase at a time, in order. A phase's exit criteria are checked against the real app.
- Decisions go in [DECISIONS.md](DECISIONS.md); learnings that would bite again go in [LESSONS.md](LESSONS.md).
- Small, reviewable commits. Architecture choices are explained briefly before large edits.
- Chorus 1 (`../Chorus`) is the reference implementation for proven logic. Port behaviour and tests, not
  code shape. ARCHITECTURE §11 maps each 2.0 module to its Chorus 1 source.
