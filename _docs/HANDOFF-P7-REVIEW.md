# P7 attention and notifications — review

## 1. Verdict

Built: every P7 task. Seen working in isolated dev instances (Claude Code 2.1.308): the 2 s exit
gate, chip-click navigation, the D23 focus check with real OS input, the tray icon's left click with
real input, approval and failure notifications, search, export with a planted fake key, and fleet
addressing. Not verified: the tray menu, other DPI scales and monitors, and opencode's shortcut
bindings (not installed here). P7 is ready for Matthew's review, not closed.

## 2. Branch

`evening/p7` from `main` at `d62e078`, merged with P8 on `evening/p6-p8`.

```
555dbff P7: fleet addressing for Claude Code sessions
646f41f P7: export a session's transcript as Markdown
95e93ed P7: search the Raw terminal's scrollback
6c4dae3 P7: notification centre, tray badge and the real fleet strip
ca83e69 P7: oldest-first Needs you inbox with waiting ages; notification rules and tray badge core
```

The inbox slice (tasks 1–2) was carried over from the earlier `work/p7-attention` worktree's isolated
patch and re-applied onto `main`; that worktree's inherited P6 copy was not used.

## 3. Files

| Created | Purpose |
|---|---|
| `crates/chorus-core/src/inbox.rs` | Waiting start per state; one notification per waiting episode; 30-day window |
| `crates/chorus-core/src/badge.rs` | Tray badge (1–9, 9+) and taskbar overlay drawn into RGBA; tooltip text |
| `crates/chorus-core/src/strip.rs` | Strip placement: remembered position kept on a screen's work area |
| `crates/chorus-core/src/export.rs` | Transcript Markdown, UTC times, safe file names |
| `crates/chorus-core/src/fleet.rs` | Registry parsing, liveness, address states, stickiness, wording (from Chorus 1) |
| `src-tauri/migrations/0019_p7_notifications.sql` | `notifications` table |
| `src-tauri/src/attention.rs` | Observes every summary: tray count, notifications, navigation event, commands |
| `src-tauri/src/tray.rs` | Tray icon, menu, badge, taskbar overlay |
| `src-tauri/src/export.rs` | Export command and save dialog (from Rust) |
| `src-tauri/src/fleet_registry.rs` | 3 s registry poller, roster and acknowledge commands |
| `src-tauri/src/store/attention.rs`, `store/notifications.rs` | Waiting evidence query; notification rows |
| `src/fleet/FleetStrip.vue`, `strip.ts`, `strip.test.ts` | The real strip (replaces `src/spikes/FleetStrip.vue`, deleted) |
| `src/shell/NotificationCentre.vue`, `src/stores/notifications.ts` | Bell and panel |
| `src/shell/inbox.ts`, `inbox.test.ts`, `src/stores/inbox.ts` | Ages, tiers, j/k selection |
| `src/raw/RawSearch.vue` | Raw search bar |
| `src/views/session/ExportTranscript.vue`, `AddressChip.vue`, `FleetRoster.vue`, `src/stores/fleet.ts` | Export, address chip, roster |

Hot-file edits: `sessions/mod.rs` (the `needsYouSince` summary field; one `name_args` line in
`start_run`), `agent.rs` (`Spec::name_args`, a method so no spec literal changed), `lib.rs`
(module lines; the notify closure calls `Attention::observe`; tray, strip and poller starts; main
window close still quits), `ipc.rs` (one P7 block, three events), `App.vue` (j/k back, navigation
listener), `OverviewView.vue` (inbox order and keys), `SessionView.vue` (search, export, chip, roster
tags only). Also `TerminalView.vue`/`SessionTerminal.vue` (search), `TitleBar.vue` (bell),
`AppearanceSettings.vue` (strip switch), `main.ts` (strip route).

## 4. Surface added

- Migration `0019_p7_notifications.sql`: `notifications(id, session_id → sessions ON DELETE CASCADE,
  kind, title, body, created_at, read_at, dedupe_key UNIQUE)`.
- `SessionSummary.needsYouSince` (ms, nullable).
- Commands: `notifications_list`, `notification_read`, `notifications_read_all`, `fleet_shown`,
  `fleet_set_shown`, `fleet_open_session`, `fleet_probe`, `session_export`, `fleet_roster`,
  `fleet_acknowledge`. Removed: `spike_fleet_open`, `spike_fleet_probe`.
- Events: `notification-added`, `attention-navigate`, `fleet-shown`, `fleet-roster-changed`.
- Settings keys: `fleet.shown`, `fleet.position`.
- Claude Code argv gains `-n <session name>`.
- Dependencies: Tauri's `tray-icon` feature (approved; `image-png` was not needed: the badge is drawn
  from the default window icon's RGBA). `@xterm/addon-search` 0.16.0 (approved in ARCHITECTURE §10).

## 5. Plan against result

1. Needs-you timestamps and order: done.
2. j/k inbox: done (verified natively in the earlier slice; carried over unchanged).
3. Notifications: done. Bell placed in the title bar beside the window buttons, always visible.
4. Tray: done, with taskbar overlay (`set_overlay_icon` exists in Tauri 2.12.4).
5. Real fleet strip: done. Hover not added.
6. Fleet addressing: done, with a measured difference (§8, P7-L1).
7. Scrollback search: done; button plus Ctrl+Shift+F (decision 3).
8. Transcript export: done, with the optional scrubbed agent transcript.
9. Review file: this one.

## 6. Verification

`npm run check` on the merged branch: typecheck clean, 41 vitest, 550 Rust tests, clippy clean,
secret scan clean.

Runtime, isolated `eve7` instance, Claude Code 2.1.308 on haiku (Fast), scratch folder:
- **Exit gate (≤ 2 s):** approval hook recorded at …158576 ms; the tray count changed at …158579 and
  the strip chip appeared at …158579 (3 ms). One notification stored.
- **Chip click:** real OS click on the chip brought Chorus forward on that session.
- **D23 with real input:** an external test window kept the foreground through a click on empty
  strip space and an 80 px drag (and the drag back); typed text landed there ("abcd").
- **Notifications:** one row per episode; unchanged after approving in Raw and after Restart. In the
  merged build, a shell exiting with code 3 notified "failed" once, and a Friendly approval notified
  "approval".
- **Tray (merged build):** the icon sat in Windows 11's hidden-icons flyout with the tooltip "Chorus:
  1 session needs you"; a real click on it brought Chorus forward on the Overview filtered to Needs you.
- **Search:** 3 matches for "hi" with highlights, Enter stepped to the next, a missing word said "No
  matches", Ctrl+Shift+F in the terminal opened the bar.
- **Export:** a fake OpenRouter-shaped key planted in a prompt was in Claude's own transcript and in
  neither exported file; both carry redaction markers. The key was never printed.
- **Fleet:** sessions verified; a rename plus Restart updated the address; two sessions given the same
  name both showed the collision. Another live Claude session (not Chorus's) was listed with no actions.

Not verified: the tray menu items with real input (new tray icons start in the overflow area on
Windows 11); question notifications; other DPI
scales or monitor changes (one monitor at 100 %); opencode's Ctrl+Shift+F (not installed). Codex's
binary showed no Ctrl+Shift+F binding string and Claude Code's default table binds Ctrl+F, not
Ctrl+Shift+F; neither was checked against docs.

## 7. Versions verified against

Claude Code 2.1.308 (`--help`: `-n, --name`; `~/.claude/sessions/<pid>.json` with `peerProtocol` 1);
Codex 0.163.0 (binary strings only, for shortcuts); Tauri 2.12.4; xterm 6.0.0 with addon-search 0.16.0.

## 8. Proposed doc changes

| # | Decision | Status | Why |
|---|---|---|---|
| P7-D1 | **Needs-you order and notifications come from persisted facts.** Oldest waiting first (unknown last); one notification per waiting episode (session, kind, start), kept 30 days; Ready and Completed don't notify; no per-kind toggles yet | Proposed | Matthew's answers 2026-10-23 (Q8); survives restarts without duplicates |
| P7-D2 | **Tray, overlay and strip.** Tauri `tray-icon`; badge drawn in core; the strip is shown by default, never activates, remembers its place; only a chip click brings Chorus forward | Proposed | Matthew's answers (Q4–Q6); D23 checked with real input |
| P7-D3 | **Raw search** opens with a button or Ctrl+Shift+F; `allowProposedApi` only on the searchable terminal (xterm 6 decorations) | Proposed | Q7; Ctrl+F belongs to the terminal |
| P7-D4 | **Claude sessions launch with `-n <name>`**; addresses are read from `~/.claude/sessions` only | Proposed | P7 Task 6 |

| P7-D5 | **Each window may call only its own commands** (proposal, P7 plan §7; not built). Today `build.rs` has no `AppManifest` command list, so every custom command (102 of them, including `session_write` and `vault_*`) can be called from the fleet strip and the splash. Change: pass `tauri_build::AppManifest::new().commands(&[...])` listing every command, which generates an `allow-<command>` permission each; the main window's capability lists them all; `fleet.json` gets only `allow-sessions-list`, `allow-prefs-get` and `allow-fleet-open-session`; `splash.json` only `allow-restore-progress` (and what `splash_replay` needs). A test compares `ipc.rs`'s command list with the manifest list, so a new command can't be forgotten. Every phase's new commands then touch two lists, which is the cost | Proposed | A compromised or buggy small window could otherwise type into a session or read the vault list; the strip and splash need four commands between them |

Lessons:
- **P7-L1:** Claude Code 2.1.308 lets two live sessions hold the same `-n` name (both `nameSource:
  user`), unlike the version Chorus 1 measured, which renamed one. Chorus shows a shared name as a
  collision.
- **P7-L2:** In xterm 6, `registerDecoration` is still proposed API; search highlights throw unless
  `allowProposedApi` is on. The addon also takes `#RRGGBB` colours only.
- **P7-L3:** `procStart` in Claude's registry is the process creation FILETIME in decimal.

PLAN: P7 built and verified as in §6; awaiting Matthew's review.

## 9. Questions for Matthew

1. Notification bodies show the latest prompt or message; for approvals the ask's own subject (for
   example "Write p7.txt") would be more useful. Change it? *Recommendation: yes, a small follow-up.*
2. Should `fleet_probe` stay in release builds? It is read only. *Recommendation: keep.*

## 10. Risks and known issues

- The strip is a fixed 460 × 56 px; more than three waiting sessions show "+N".
- The tray icon may sit in the hidden-icons area until the user pins it; the taskbar overlay does not.
- Export copies only the newest run's agent transcript.
- `fleet_registry` polls every 3 s for the app's lifetime (cheap: a few small files).

## 11. How to review

Read `inbox.rs`, `badge.rs`, `fleet.rs` and `export.rs` in core, then `attention.rs`, `tray.rs`,
`fleet.rs` and `fleet_registry.rs`. In the app: start a Claude Code session in Manual permission and
ask it to write a file; watch the strip chip, tray badge and bell; click the chip from another app;
search the Raw view; export the transcript; rename the session and Restart to see its address change.

## 12. Follow-up on the integration branch (2026-10-24)

- **Inbox, real keys, merged build:** three failed sessions stayed oldest first across an app restart;
  j, j selected the second; Enter opened it; with the terminal focused Esc stayed with the shell
  (D49); after clicking outside it, Esc returned to the Overview with the next card selected.
- **Code review of `evening/p6-p8`:** ten findings, each checked against the code and fixed (commit
  "P7/P8: fixes from a code review of the merged branch"). For P7: Escape now closes the notification
  panel or Export popover before leaving an inbox-opened session; notifications check their episode
  before querying the request; export reads and scrubs the agent transcript off the async runtime;
  file-path tool targets are kept whole, so Documents finds long paths. The P8 fixes are listed in
  that commit and checked in the app.
- A test-method lesson for later runs: tapping Alt to bring a window forward opens its system menu,
  which stalls the WebView's events until it closes; click the window instead.
- **Strip after a display change:** besides the clamp at open, an open strip is now checked every
  5 s against the screens' work areas and pulled back if it is off them. Seen: moved by a program to
  (6000, 3000), it was back at the nearest corner of the primary work area within 7 s, without
  activating anything. A real monitor unplug or a DPI change still wasn't tried (one monitor).
- **Second code review:** ten more candidates, eight fixed (commit "P7/P8: fixes from a second,
  deeper code review"). For P7: a failed turn followed by the process exiting is one failure, so it
  notifies once; search as you type keeps the current match (seen: "f" to "footb" stayed on "1 of
  N", Enter moved to "2 of 2"); export renders off the async runtime (seen: a real save through the
  dialog). Kept as a trade-off: whole file paths show in tool lists, where they wrap.
- **Front-end tests:** notification store, fleet roster store and the search count wording (41 → 52
  tests with P8's).
- **Through the UI:** Settings → Appearance's strip switch hid and reshowed the strip; Mark all read
  took the unread count from 9 to 0 and cleared the bell's badge; with one failed session the main
  window's taskbar button showed the red "1" overlay (screen capture before and after).
