# Hand-off plan: P7 — Attention and notifications

**Read [HANDOFF-COMMON.md](HANDOFF-COMMON.md) first.** It holds the isolation, safety, shared-file and
review-file rules this plan relies on. Label `p7`, Vite port 1427, review file
`_docs/HANDOFF-P7-REVIEW.md`.

## 1. Goal

PLAN P7: "Needs you" as an inbox you work through with j/k; a notification centre (in-app history); a
tray icon with a waiting-count badge; the always-on-top fleet switcher strip, which never takes focus
(S3, D23, D38); fleet addressing for Claude sessions; scrollback search; transcript export.

**Exit (M2):** with 2.0 behind other windows, a session needing you is visible **within 2 s** on the
strip and the tray, and one click lands on its card. In addition, from S3: clicking and dragging the
strip leaves an external app's text box focused, and typed text still lands there (D23).

Notifications are in-app only (D15). OS toasts are disabled on this machine (L9). Don't use them.

## 2. Out of scope

- Answering approvals or questions from Chorus. That is P6's ActionRouter. The inbox *opens* the
  session; it doesn't answer.
- Jev or a narrator judging turn ends (D40 lists it as a P7 candidate; it stays out of this hand-off).
- Searching the on-disk scrollback mirror. Search covers the terminal's buffer (see Task 7).
- Narrowing which commands each window may call (see §7, a proposal only).
- Close-to-tray or minimise-to-tray. Don't change what closing the main window does.
- Pop-out windows (D27).

## 3. Ground facts (verified 2026-10-20; re-check before relying on them)

**Attention today**
- `crates/chorus-core/src/attention.rs` defines the D47 status word (`Attention`, in urgency order),
  `needs_you()` (approval, input, failed) and `group()`. **It carries no timestamp.** Neither does
  `SessionSummary` (`src-tauri/src/sessions/mod.rs` around 653–716). So there is no "oldest waiting
  first" order and no wait age yet.
- `src/shell/status.ts` has `needsYou`, `counts`, `headline`, `byUrgency` and `groupSessions` (the
  Needs you group is around line 86). The order is urgency, then most recent.
- `src/views/OverviewView.vue` renders the groups and has a "needs" filter pill. It has no keyboard
  navigation. `src/shell/Rail.vue` computes the needs badge in the front end.
- `stores/sessions.ts` mirrors summaries from the `session-updated` event (the only session event).
  `stores/notices.ts` holds short-lived corner messages, not a history.
- Keyboard rules (D49): Chorus's shortcuts apply only while no terminal has focus. `App.vue` has the
  `onKey` handler.

**The fleet strip (spike S3, kept as real code)**
- `src-tauri/src/fleet.rs`: `spike_fleet_open` (async; a window built in a synchronous command
  deadlocks, L28) builds the `fleet` window from `index.html?view=fleet`. It is 420×64, has no
  decorations, is always on top, has no taskbar entry, and is `focused(false)` and `focusable(false)`.
  `install_shield` puts the D38 input shield over it. `track_press` drags in physical pixels, and a
  release within 4 px emits `FleetClick` to the strip page. `spike_fleet_probe` reports geometry, DPI,
  styles and the foreground window.
- `src/spikes/FleetStrip.vue` is a throwaway with hard-coded counts. It turns `fleetClick` into
  `elementFromPoint(x / devicePixelRatio)`. It has no hover, because the shield swallows `WM_MOUSEMOVE`.
- `src/main.ts` loads `FleetStrip.vue` for `?view=fleet`, **without Pinia**.
- `src-tauri/capabilities/fleet.json` grants `core:default` only.
- S3 couldn't test other DPI scales or display changes (one monitor at 100 %). That is still open.

**Tray:** `tauri` is used without the `tray-icon` feature (`cargo tree -i tray-icon -e features` is
empty). The feature needs Matthew's yes (§6, question 1). Loading a PNG into a tray `Image` may also
need Tauri's `image-png` feature; check it.

**Fleet addressing** (Chorus 1: `src/main/services/fleetRegistryCore.ts`, `fleetRegistry.ts`,
`src/shared/fleetRoster.ts`, spec `docs/Features/Fleet Comms/chorus-fleet-comms-spec.md`):
- It is *Claude Code's own* peer fleet. Live Claude sessions write `~/.claude/sessions/<pid>.json`
  (pid, sessionId equal to the transcript file name, cwd, procStart, peerProtocol, name, status,
  nameSource, messagingSocketPath), with a `.key` file beside it. Claude sessions find each other with
  `ListAgents` and message each other with `SendMessage`.
- **Chorus only reads the JSON.** It never opens the socket and never reads a `.key` file.
- The address is the session name, passed as `claude -n <name>` on fresh launches **and** on
  `--resume`. Chorus 1 measured that Claude doesn't keep the name across a resume.
- 2.0 doesn't do this yet. Its Claude spec (`agent.rs` around 185–203) passes `--session-id` and
  `--resume`, but no `-n`. Claude's session id is assigned up front, which makes the join easy.
- Chorus 1's rules to port:
  - `parseRegistryEntry` never throws, drops unknown keys and keeps an unknown status as
    `unrecognised`.
  - Only `peerProtocol` 1 is supported.
  - `isLive` needs the pid alive **and** its start time equal to `procStart`, because files leak on a
    hard kill and pids are recycled.
  - Addresses are normalised (trimmed, lowercased) and duplicates are detected.
  - There are three address states: verified, changed (cause `collision` or unknown) and unknown. A
    `changed` state is sticky until it is acknowledged.
  - Poll every 3 s. An unreadable folder is "unreadable", not an empty fleet.
  - Never serve the last good address once a read fails.
  - The tests worth porting are in `fleetRegistryCore.test.ts`, `fleetRegistry.test.ts` and
    `fleetRoster.test.ts`.
- **Verify all of this against the installed Claude Code** (`claude --version`, `claude --help`, and
  the files a live session writes). Chorus 1 measured an older version.

**Escalation ladder** (Chorus 1 `src/renderer/src/composables/attentionTier.ts`, tested): calm under
30 s, pulse to 5 min, urgent to 20 min, then stale. `ageLabel` formats "4m 12s". One shared clock.
Chorus 1's inbox design (`docs/design/v2/Chorus Attention Inbox.dc.html`) has oldest first,
"j/k navigate · enter focus session · esc back", and answering an item moves to the next.

**Scrollback search:** `@xterm/addon-search` is approved (ARCHITECTURE §10) but not installed. The
terminal keeps `scrollback: 10_000` (`src/components/terminal.ts`). The on-disk mirror is about 4 MB
(`sessions/mirror.rs`).

**Transcript export data:** the history tables `prompts`, `turns` (with `final_message`),
`tool_calls` and `asks` (migration 0005) are already scrubbed. They are read through
`store/history.rs`. `runs.transcript_path` (0006) holds the agent's own transcript file, which is
**not** scrubbed. It is exposed as `SessionHistory.transcriptPath`, newest run only. There is no
session menu yet. `SessionView.vue` has a controls bar and a raw bar.

## 4. Design

**Back end**
- **`chorus-core`, extending `attention.rs` or a new `inbox.rs`:**
  - `needs_you_since`: the instant a session entered its current needs-you state, derived from
    persisted facts so that it survives a restart. Approval or question: the open ask's `asked_at`.
    Needs your answer after a turn: the turn's `ended_at`. Failed: the run's `ended_at`.
  - The inbox order (oldest waiting first).
  - The escalation tier for an age.
  - Notification rules: which transitions notify, and de-duplication (one notification per episode,
    keyed by session and the state's own ask, turn or run id).
- **`chorus-core::badge`:** renders the tray badge (a count 1–9 or "9+" over the mark) into an RGBA
  buffer from a tiny built-in bitmap font. It is pure, with tests for 0, 1, 9, 10 and 99. Zero means no
  badge.
- **`chorus-core::fleet`:** the registry parsing, liveness, address state and roster logic, ported
  from Chorus 1 with its tests.
- **New `src-tauri` modules:**
  - `attention/` holds notifications (store, emitter), the tray, and the strip's lifecycle, which
    grows out of `fleet.rs`.
  - `fleet_registry.rs` is the 3 s poller of `~/.claude/sessions`. It gets process start times through
    `GetProcessTimes` (the `windows` crate's `Win32_System_Threading`, already enabled). Check the
    units of `procStart` against a live file.
  - `export.rs` is transcript export.
- **One migration:** `notifications` (id, session_id with FK `ON DELETE CASCADE`, kind, title, body
  scrubbed, created_at, read_at, and a dedupe key that is unique). Add
  fleet-strip prefs (shown or hidden, position) to the existing settings store, not a new table.
- **`SessionSummary`** gets `needsYouSince: Option<i64>` (milliseconds). It is the only field P7 adds.

**Front end**
- **Overview:** "Needs you" ordered oldest first, with a wait age and tier styling. j/k moves a
  visible selection, Enter opens the session, and Esc returns. These keys work only when no terminal
  or text field has focus (D49). Returning from a session you opened from the inbox moves the
  selection to the next item.
- **Notification centre:**
  - A bell with an unread count, placed in the status bar or the title bar. Choose the place that
    matches DESIGN and the mockup screenshots, and say which you chose.
  - A panel lists notifications newest first. A click opens the session and marks the notification
    read; there is also "Mark all read".
  - Notifications are kept for 30 days.
- **The strip (`src/fleet/FleetStrip.vue`, replacing the spike page):**
  - It shows the counts for needs you, working and finished, and a chip per session that needs you,
    in tier colours.
  - It has its own small store fed by `session-updated` (Pinia or a plain reactive store; it must not
    load the whole app).
  - Clicking a chip shows the main window on that session's card. Clicking empty space or dragging
    the strip never activates anything.
  - Hover is optional: forward `WM_MOUSEMOVE` the same way as clicks only if you need it.
- **Raw view search:**
  - A search button in the raw bar, plus a key that doesn't steal an agent's or shell's own keys (§6,
    question 3).
  - Next and previous, and a match count.
  - The bar says "Searches the last 10,000 lines" (fail visible).
- **Export:** a session menu (or a button in the controls bar if there is no menu) with "Export
  transcript…". It opens a Windows save dialog through `tauri-plugin-dialog`, from Rust only, as the
  folder picker does (D54).

## 5. Tasks, in order (commit after each)

1. **Needs-you timestamps and order.**
   - Core: `needs_you_since` and the inbox order, with tests. Then the `SessionSummary` field and the
     Overview order with wait ages.
   - Port `attentionTier` and `ageLabel` to TypeScript beside `status.ts`, with vitest tests. Use one
     shared clock (the existing `now.ts`).
   - *Acceptance:* a fresh approval appears last in the inbox; after an app restart the ages don't
     reset.
2. **j/k inbox.**
   - Keyboard navigation and selection in Overview, and moving to the next item when you come back.
   - Test it with a focused terminal and with a focused text field (no effect), and with focus on the
     board (it works).
3. **Notifications.**
   - The migration, the store, a back-end emitter hooked where session summaries are sent (the notify
     closure in `lib.rs`, kept to one call), a `notification-added` event, and commands for listing,
     marking read and marking all read.
   - Then the centre UI.
   - *Acceptance:* each approval, question and failure notifies exactly once. A Restart doesn't
     duplicate earlier notifications.
4. **Tray (after Matthew's yes on question 1).**
   - The tray icon with the badge from `chorus-core::badge` and a tooltip ("2 sessions need you").
   - Left click shows the main window on Overview → Needs you. The menu has Open Chorus, Show or hide
     the fleet strip, and Quit.
   - Also set the **taskbar overlay icon** of the main window, if Tauri 2.12 exposes it on Windows
     (`set_overlay_icon`); check this. If it doesn't, don't build it; say so.
   - The count comes from `Attention::needs_you` in the back end, the same function the cards use
     (D47).
5. **The real fleet strip.**
   - Replace the spike page, and turn `spike_fleet_open` and `spike_fleet_probe` into real commands
     (keep the probe as a dev-only command if it's useful for tests).
   - Open the strip at start if the pref says so (default: shown; see question 2), remember its
     position, and keep it on screen after a display change (clamp to the nearest monitor's work
     area).
   - Clicking a chip activates the main window and opens that card (question 4).
   - Delete `src/spikes/FleetStrip.vue` when it is replaced.
6. **Fleet addressing (Claude).**
   - Verify the installed Claude Code's `-n`/`--name` flag and its `~/.claude/sessions/*.json`
     format first.
   - Then pass `-n <session name>` on fresh launches and resumes. Add it to the Claude `Spec` as a
     field with a default, so other branches' `Spec` edits still merge.
   - Then port the registry core, add the poller, and show an address chip in the session header
     (verified, changed or unknown, with Chorus 1's wording from `describeAddress`). Add a roster to
     the session facts panel: every session, with the reason a non-Claude one has no address, and
     peers Chorus didn't start listed with no actions.
   - A rename changes the address from the next run. Say so in the chip's tooltip.
7. **Scrollback search.** Install `@xterm/addon-search` and wire it into `TerminalView.vue` /
   `terminal.ts` for the session view's terminal. Wall tiles don't get search.
8. **Transcript export.**
   - Markdown from the database history: for each turn, the prompt with its source and time, the final
     message, the tool calls (tool, target, outcome) and the asks with their answers.
   - Optionally include the agent's own transcript file, **scrubbed with `chorus_core::scrub` and every
     vault secret**, as a second file beside it. Use a checkbox, off by default.
   - Paths must be absolute and the folder must exist. Never overwrite without the dialog's own
     confirmation.
9. **Exit test and review file** (§8).

## 6. Decisions for Matthew (ask before building past them)

1. **Enable Tauri's `tray-icon` feature?** It is Tauri's own tray, an optional part of the approved
   `tauri` crate, but it compiles in code that isn't there now, so CLAUDE.md requires his yes.
   - Without it there is no tray, and the exit criterion can only be met on the strip.
   - The alternative is the taskbar overlay badge alone, which is invisible when the taskbar is
     hidden or the app isn't pinned.
   - *Recommendation: enable it (plus `image-png` if needed).*
2. **Is the strip shown by default?**
   - (a) Shown at start; it can be hidden from the tray menu and Settings.
   - (b) Hidden until turned on.
   - *Recommendation: (a), because the exit criterion assumes it is visible when Chorus is behind
     other windows.*
3. **Which key opens Raw search?** Ctrl+F belongs to the terminal: readline and shells use it for
   "forward one character", and agents may bind it (D49).
   - (a) A button only.
   - (b) The button, plus Ctrl+Shift+F while the terminal has focus.
   - (c) The button, plus Ctrl+F while the board has focus (which never applies to a terminal).
   - *Recommendation: (b). Check that Claude Code, Codex and opencode don't bind Ctrl+Shift+F in
     their `--help` or docs.*
4. **May a strip click bring the main window forward?** D23 says Chorus never steals focus, and S3
   proved clicks and drags on the strip don't. Opening a card is a request to go to Chorus, so it has
   to activate the main window, and a dictation app's target loses focus then.
   - (a) Chip clicks activate the main window. Everything else on the strip never does.
   - (b) Chip clicks only highlight the card. The user then switches to Chorus.
   - *Recommendation: (a). It is what "one click lands on its card" means, and it is an explicit act.*
5. **Which transitions notify?** Recommendation: entering Awaiting approval, Needs your answer and
   Failed. Ready and Completed don't notify, because they would be noisy with five agents; they still
   show on the board. Offer a per-kind toggle in Settings only if he wants one.

## 7. Proposal only (write it up; don't build it)

`src-tauri/build.rs` has no `AppManifest` command allow-list, so **every custom command (including
`session_write` and `vault_*`) can be called from the strip and splash windows**. Narrowing it means
listing every command for the main window's capability, which touches every phase's commands. Write it
up in the review file as a proposed decision for the main line, with the change you'd make.

## 8. Verification

- **Unit tests:** inbox order and `needs_you_since`, notification dedupe, badge rendering, tier
  ladder, and registry parsing, liveness and address state (port Chorus 1's cases, including a
  recycled pid and an unreadable folder).
- **Runtime, in your `p7` instance**, with real OS input (L10), never DevTools events. Use
  `SetCursorPos` plus `mouse_event`, and check `WindowFromPoint` before each click.
  1. Put Chorus behind another window. Make a Claude Code session ask for approval: start it in
     Manual permission in a scratch folder, and ask it to create a file. Measure the time from the
     hook's arrival (in your log) to the strip chip and the tray badge changing. **Both must be ≤ 2 s.**
     Record the measurement.
  2. Click the chip. The main window shows that card.
  3. The D23 check: focus a text box in another app, click the strip (not a chip), drag it, then type
     with real keystrokes. The text lands in the other app, and `GetForegroundWindow` never changes.
  4. j/k through three waiting sessions. Restart the app; the order and the ages are kept.
  5. Fleet: two Claude sessions show "verified". Rename one and restart it: its address updates. Then
     give two sessions the same name: the address shows "changed (collision)".
  6. Search finds a word in the terminal history. Export produces Markdown with a planted fake key
     shown as redacted, in both files.
- Not testable here: other DPI scales and display changes (one monitor). Say so; don't change
  Matthew's display settings.

## 9. Files you will probably touch

New: `crates/chorus-core/src/{inbox,badge,fleet}.rs`, `src-tauri/src/attention/*`,
`src-tauri/src/fleet_registry.rs`, `src-tauri/src/export.rs`, `src/fleet/*`,
`src/views/NotificationCentre.vue` (or under `src/shell/`), `src/stores/notifications.ts`, one
migration.
Hot (minimal edits): `sessions/mod.rs` (one summary field), `lib.rs`, `ipc.rs`, `App.vue` (j/k
dispatch, centre toggle), `OverviewView.vue`, `SessionView.vue` (search, export, address chip),
`agent.rs` (Claude `-n`), `main.ts` (the strip route), `src-tauri/Cargo.toml` (the tray feature),
`package.json` (addon-search).
Replace: `src-tauri/src/fleet.rs` (grows into the real module), `src/spikes/FleetStrip.vue` (deleted).
