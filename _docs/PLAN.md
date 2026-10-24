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

### Current position (2026-10-23)

There are **14 active phases**: P0–P14 with P9 removed. Keep these identifiers: scope and decisions
already refer to them. P0–P5 are closed; P4's full-day acceptance check was explicitly waived on
2026-10-22, not performed (D72). P5's remaining work passed its app checks on 2026-10-22.
M1 is complete with the recorded P1/P4 waivers. P6's final narrator slice is ready for review. Work brought forward from P12 and P14
does not close those phases.

| Phase | Definition | Status and remaining work |
|---|---|---|
| P0 | Foundations and spikes | Closed 2026-10-19; written spike results |
| P1 | Session engine | Closed 2026-10-19; reboot check waived, not performed |
| P2 | Agents and providers | Closed 2026-10-19; Grok checked only as far as launch |
| P3 | Events and durable history | Closed 2026-10-19 for Claude Code, Codex and opencode; Grok has no event channel |
| P4 | App shell and Overview | Closed 2026-10-22; full-day acceptance waived, not performed (D72) |
| P5 | Git and worktrees | Closed 2026-10-22; retained reuse, boot reconcile, worktree panel and diff summaries verified in the app (D73–D74) |
| P6 | Friendly view and Next steps | Ready for review: composer/images/queue, progress and Claude/Codex controls verified; narrated Now / Blocker / Up next, durable suggestions, settings and costs implemented (D82). Raw drafts remain deferred; Codex checklist emission remains conditional. Review fixes 2026-10-24: next steps wait for an ended turn, readiness points to the Friendly composer, CLI facts name the checked version. |
| P7 | Attention and notifications | Built 2026-10-24 and ready for review: inbox order and ages, notification centre, tray badge, fleet strip, Raw search, transcript export and Claude fleet addressing; exit gate and D23 seen in the app ([review](HANDOFF-P7-REVIEW.md)). tray click and menu, strip focus and inbox keys checked with real input. Other DPI scales, a real monitor change and opencode bindings unverified |
| P8 | Bots, Automations, Notes, Documents | Built 2026-10-24 and ready for review: Bots from profiles, Notes and per-run instructions (Claude; Codex through its app-server, awaiting approval), Documents and Automations for Claude Code ([review](HANDOFF-P8-REVIEW.md)) |
| P9 | Voice | Removed; separate dictation app (D22); Chorus retains compatibility (D23) |
| P10 | Council | Planned |
| P11 | Measurement, made visible | Planned; tokens, costs, attention and Usage UI; forecasts remain gated on data completeness |
| P12 | Routing inspector | Launch routing built in P4; Settings → Routing, observer status and history remain |
| P13 | Phone relay | Planned; P13a delivers prompts and status; P13b answers questions and approvals after P6 |
| P14 | More agents and ship | Partly built: Gemini, Aider, Ollama, CI, installer polish and Q28–Q30 fixes; complete real turns, DeepSeek spike/adapter, signing, updates and eventual name migration remain |

**Execution order agreed 2026-10-22:** roadmap consolidation, Q28–Q30 and P5 are complete. Build P6
and P7 for M2, then finish the remaining capability and release phases for M3. P6 began with the
structured action/plan channel and Friendly timeline. Following the 2026-10-23 review (D77),
the Friendly composer and durable queue were prioritised and verified (D78). Progress/control and
multi-phase tasks followed (D81); narrator and Next steps now follow in D82. Review this final P6
slice, then move to P7 attention and notifications.
Record runtime evidence before closing a phase. P4's full-day check was waived by the user, not
replaced by a short smoke test (D72).

Scope changes already agreed and built: worktree creation/Swarm/Close moved from P5 to P4 (D53),
the two-pane New session design and tags are in P4 (D52, D56), and launch routing moved from P12 to
P4 (D57). DeepSeek Harness was added to P14 (D59); it was absent from the original P14 hand-off.

**Parallel work exception:** [HANDOFF-COMMON.md](HANDOFF-COMMON.md) records Matthew's permission to
develop P7, P8, P11, P13 and P14 alongside the main sequence. These are plans, not completion claims.
P14 is partly merged; the other four have no implementation on the current main line. Contracts
from an unfinished phase remain dependencies, especially P8's prompt delivery and P6's ActionRouter.

| Supporting plan | Role |
|---|---|
| [P7](HANDOFF-P7-attention.md) | Inbox, tray, fleet strip, search and export |
| [P8](HANDOFF-P8-bots-notes.md) | Bots, schedules, notes, documents and shared prompt delivery |
| [P11](HANDOFF-P11-measurement.md) | Measurement UI and explicit completeness |
| [P13](HANDOFF-P13-telegram.md) | Initial relay; interactive answers depend on P6 |
| [P14](HANDOFF-P14-agents-ship.md) · [review](HANDOFF-P14-REVIEW.md) | Original hand-off and actual result; this roadmap includes the later DeepSeek addition |
| [opencode port](HANDOFF-OC2-opencode.md) · [merge notes](HANDOFF-P14-OC2-MERGE.md) | Historical, superseded/completed; retain for context, do not restart |

### Phase definitions

Build in the order above, with the recorded parallel exception. Each phase ends with its exit criteria
**verified at runtime** (a drive of the real app or a real CLI), not only by compiling and passing unit tests.

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
(OPEN-QUESTIONS Q18), so it isn't covered. The Codex hook relay was subsequently bundled in the
installer in P4 (`c064ebd`, D46, L38).*

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

**Acceptance record:** closed 2026-10-22 by explicit user waiver of the full-day check (D72), not by a
full-day result. The user directed work to move to P5. Gemini/Aider launch acceptance alone does not
prove their options apply (P14).

### P5 — Git and worktrees
New session already makes worktrees, Swarm runs, and Close removes a clean worktree and keeps
uncommitted work (D53, built in P4). P5 adds attaching a session to a retained worktree; boot reconcile
with `git worktree list` as the authority; Settings → Projects worktree panel (branch, ahead/behind,
clean); and a diff summary in the session facts ("11 files · +612 −48"). D73–D74 define recovery and
the summary's comparison: net tracked lines against HEAD, with untracked/binary counts shown separately.

**Implemented:**
1. Reconcile recorded worktrees against Git before restoring sessions. Keep history and branches;
   surface missing/unreadable worktrees explicitly. Do not prune other tools' worktrees.
2. Add retained-worktree selection to New session, with backend validation of the selected record,
   repository membership, current existence and folder containment. Pair/Workbench may share the
   selected tree; Swarm continues to make one new tree per agent.
3. Add the worktree panel and session diff summary. Distinguish unknown upstream/status from zero
   changes; cover tracked, staged, unstaged, untracked and binary changes without reading file contents
   into the UI.

**Exit (M1):** the Swarm preset launches N agents in N worktrees, and removing a worktree behind 2.0's
back is healed on the next boot.

Also verify in the real app: a new session can reuse a retained worktree; dirty work survives Close;
the panel shows branch, clean/dirty and ahead/behind (or why unavailable); and session facts report
changed files and line additions/deletions. A missing worktree must not silently relaunch a session
in a different folder or discard its history. A Git failure is unknown, not proof that a worktree is gone.

**Closed 2026-10-22.** Verified in an isolated dev instance with Git 2.50.0.windows.1: three live Codex
sessions launched in three distinct Swarm worktrees; Pair and Workbench shared a selected retained tree;
dirty Close kept its files; a shell launched through the UI's Existing worktree selector reused them.
The panel and session facts showed four changed files, +2/−1 net tracked lines, staged/unstaged,
untracked/binary counts, and 1 ahead / 1 behind. A detached tree was labelled explicitly. With Git
unreadable, records stayed unknown rather than removed, then recovered when readable again.

After terminating the isolated app, removing a worktree through Git and recreating an ordinary folder
at its old path, the next boot kept that session stopped with a missing-worktree explanation and the
original cwd/history. Other sessions restored, dirty work remained, and the external tree and branch
records were untouched. No model prompts were sent. Automated checks: 439 Rust tests, 19 frontend tests,
TypeScript, Clippy and secret scan passed. P4's full-day check remains waived, not performed.

### P6 — Friendly view and Next steps
Timeline from structured events: your messages, agent messages, the plan checklist (from the agent's own
todo or plan tool, or a plan file), collapsed tool-call groups, and question or approval cards with
buttons. Approvals and answers delivered through the agent's structured channel where S5 proved one,
otherwise as keystrokes to the PTY, and the agent's own prompt stays usable whenever Chorus doesn't
answer (D29). Composer that **queues** instructions while the agent is busy and delivers them at the
next pause. Now / Blocker / Up next, and **suggested next steps** from the narrator (D28), with "Done —
won't be suggested again". Without a narrator key, Friendly still shows the timeline, the plan and the
cards, and says what's missing.

**First bounded slice (2026-10-22):** continued from the clean `c106c1d` P5 checkpoint on
`roadmap-p6`; the three preceding continuation commits and design assets were present. A fresh
lockfile install/build reproduced the baseline: 439 Rust tests, 19 frontend tests, TypeScript,
Clippy and secret scan. No P1/P4 acceptance waiver was reopened.

- ActionRouter has generated contracts for prompts, questions and approvals. Only prompt delivery
  is connected in this slice; reply attempts visibly refuse and preserve the agent's Raw prompt.
- `sessions/delivery.rs` implements HANDOFF-COMMON's shared path, extended with run identity and
  durable request IDs (D75). Prompts are scrubbed and committed before two PTY writes. Same-run
  matching hook echoes link to that prompt once. Busy, unknown, locked and blocked states refuse.
- Session view exposes channel limitations beside the unchanged Raw terminal. Native Codex installs
  now resolve without an npm shim. This is a foundation, **not a working Friendly view**.
- Q19's bounded follow-up verified current app-server initialisation, one completed subscription
  turn, and authenticated TUI attachment through ConPTY. The generated schema contains plan events,
  but the tiny turn emitted none. Plan/control acceptance stays open; see [S5 follow-up](spikes/S5-P6.md).
- In an isolated dev app, Claude Code completed a shared-delivery prompt with one `composer`
  history row and the same Raw transcript; retry returned the observed receipt without a third turn.
  The slice passes 449 Rust tests, 19 frontend tests, TypeScript, Clippy and secret scan.

**Second slice (2026-10-22):** Claude held-hook replies and same-session Friendly/Raw switching now
work in the isolated app. A question was answered and a scratch-file write approved through the
production Friendly cards; Raw showed the same completed turn. A second write was denied with a
reason and left its file absent. Full timeout preserved Raw's prompt; releasing a question to Raw
and answering an approval in Raw while its Friendly hook waited also passed. The timeline displays recorded prompts/final messages, collapsed
tool calls and durable requests/reply attempts. Replies expire, reject stale runs, and never replay
after an uncertain send. See [reply evidence and remaining gates](spikes/S5-P6-REPLIES.md).
At this checkpoint Q19 remained open: the Codex/default-model probe emitted no plan and reported
the plan tool unavailable; schema support alone still does not enable a production bridge.
The subsequent user approval is recorded below (D77). Checks pass: 456 Rust
tests, 19 frontend tests, TypeScript, Clippy and secret scan.

**User review (2026-10-23, D77):** Matthew reviewed the running app and accepted Friendly's
current display, approving Q19 with remaining Friendly features understood. Prompting directly
from Friendly remains required: switching to Raw to send a prompt is a temporary limitation.
This resolves Q19's user approval, not the unperformed Codex plan/control checks, and does not
close P6. Keep unsupported capabilities explicit until verified.

**Composer and queue slice (2026-10-23, D78):** Friendly now saves per-session drafts, sends
instructions to Claude/Codex and queues them durably for a confirmed pause. Cancellation and held
item review share the delivery lock. Drafts survive restarts; unsent instructions are held until
reviewed, while uncertain/written attempts never replay. Raw edits pause automatic dispatch;
manual first sends or Raw-draft recovery use the Send click as the readiness override (D80). Other agents show
the reason their composer delivery is unavailable. No Codex app-server plan/control capability
was enabled by this slice.

Verified in an isolated real app against Claude Code 2.1.308 and Codex 0.163.0: multiline delivery,
agent-confirmed receipts, queued follow-ups and one composer history row per observed prompt.
Claude also covered cancellation, Raw-draft inhibition and forced app termination with an unsent
queue item plus a saved Friendly draft. On restart the draft survived, the queue stayed held,
and an explicit Friendly send completed once without submitting the draft. The live test exposed
and fixed a text/Enter batching issue (L55). Checks: 465 Rust tests, 28 frontend tests, TypeScript,
Clippy and secret scan. See [composer verification](spikes/S5-P6-COMPOSER.md).

**Composer review follow-up (2026-10-23):** Friendly opens at the latest end of its chronological
history, with active reply cards and the composer below the messages. It follows growing content
while near the bottom and preserves the reader's position when scrolled up. The button is now
`Queue`. Automatic Raw draft detection/transfer is requested and remains open: the current Raw
edit flag observes input activity, not the CLI's actual editable buffer. Investigate a supported
editor handoff per agent; preserve existing Friendly drafts and verify that transfer cannot leave
text in Raw to be submitted twice. Do not reconstruct the draft from terminal output or assume
keystrokes reflect the final text after history, completion, multiline editing or attachments.

**Image attachments (2026-10-23, D79):** Friendly accepts PNG/JPEG/WebP files and pasted images,
with numbered thumbnails, X removal and a bounded preview modal. Normalized image bytes survive
draft saving, queueing and restart. Explicit image-file references accompany the prompt in Raw;
agents use their own image-reading tools and permissions. Verified with Claude Code 2.1.308 and
Codex 0.163.0, including image-only queue dispatch, restart recovery, clipboard paste, preview and
removal. The checks exposed and fixed longer-paste delivery/echo issues and concurrent Read approval
matching (L57–L58). Checks: 471 Rust tests, 30 frontend tests, TypeScript, Clippy and secret scan.

**Earlier slice plan, superseded by D81–D82:** the remaining progress/control and multi-phase
acceptance work was completed below; Raw draft detection/transfer was explicitly deferred.
The final narrator slice is now ready for user review. P7 and the separate Gemini/Aider/P14 work
remain separate.

**Progress/control slice (2026-10-23, D81):** Intermediate agent text and real task checklists now
persist and display in Friendly. Codex 0.163.0 has a private authenticated server shared with Raw,
with structured questions and once-only approvals. Claude completed a three-step checklist task
through Friendly; Codex completed a phased task plus a queued follow-up through Friendly, with
matching Raw history. Codex's current configuration emitted progress text but no checklist; that
limitation remains explicit. Raw draft detection/transfer is deferred by user request; the bounded
agent-by-agent investigation is [recorded here](spikes/S5-P6-PROGRESS.md).

**Narrator and Next steps (2026-10-23, D82):** Friendly shows turn-labelled AI summaries with
live-state precedence, optional next steps with saved Done/Dismiss/Restore, and Add to prompt
that preserves the existing draft and attachments. Settings selects an OpenRouter model and
provider, with project switches and narrator-only reported/unknown costs. Missing configuration,
provider errors and uncertain attempts remain visible; calls are never automatically retried.
Runtime acceptance and limitations are in [S5 narrator evidence](spikes/S5-P6-NARRATOR.md).
This completes the requested implementation slices; user review is next, followed by P7.

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
Gemini CLI, Aider, and Ollama (via opencode) adapters. **DeepSeek Harness** (`dsh`) on DeepSeek's own
API (D59), after spike S9 settles how Chorus runs it (Q21): on Windows, which of its interfaces gives a
real terminal or a structured event stream, how its key and model are set from 2.0's vault and launch
form, where its sessions live and how they resume, and how it reports cost. Installer polish, code
signing decision, update channel; secret scanning in CI. Once Chorus 1 is retired, the installed name drops its "2" (D51), with a
step that moves an existing "Chorus 2" install.
Done so far (2026-10-21): the Gemini CLI and Aider adapters, Ollama as a provider for opencode (D64),
the CI check with the secret scan, installer polish, and designs for signing (D66, Q25), the update
channel (Q26) and dropping the "2". On 2026-10-22, Q28–Q30's fixes were implemented: conditional
Gemini trust, honest Aider permission labels and per-session Aider history (D69–D71). Still to come:
complete real Gemini/Aider turns and option/resume verification; DeepSeek Harness (spike S9, Q21);
and implementing/validating the release designs when the release decisions are settled.
opencode 2.x came from main (D58), not this phase.

## 5. Ways of working

- One phase at a time, in order. A phase's exit criteria are checked against the real app.
- Decisions go in [DECISIONS.md](DECISIONS.md); learnings that would bite again go in [LESSONS.md](LESSONS.md).
- Small, reviewable commits. Architecture choices are explained briefly before large edits.
- Chorus 1 (`../Chorus`) is the reference implementation for proven logic. Port behaviour and tests, not
  code shape. ARCHITECTURE §11 maps each 2.0 module to its Chorus 1 source.
