# Chorus 2.0 — Design

Source of truth: the **Harness** mockups in `_assets/Harness UI mockups.zip` (`Harness.dc.html` in the
Glacier theme, `Harness (Dusk).dc.html`, and `uploads/glacier-theme.css`). Screenshots of every screen are
in `_assets/mockup-screens/`. Where this file and the mockup disagree, the mockup wins, except for the
product name and logo: the app is **Chorus**, with the seven-bar mark ([Logo](#logo), D24).

## Screens

| Screen | Screenshot | What it does |
|---|---|---|
| Overview | `glacier-overview.png`, `dusk-overview.png` | Headline ("3 sessions need you · 1 working · 6 total"); status filter pills; project tag chips (+ New tag); Group by Status / Project / None; cards in **Needs you**, **In flight** and **Finished** groups |
| Sessions | `glacier-sessions.png` | Searchable list of every session |
| Session — Friendly | `glacier-session-review.png`, `-answer`, `-choose`, `-watch`, `-recover`; `dusk-session-friendly.png` | Header (agent chip, name, project, dir, branch, agent / model, status pill, Friendly/Raw toggle); Now / Blocker / Up next; timeline; composer; right panel with readiness, **Next steps**, **Done — won't be suggested again**, and session facts |
| Session — Raw | `glacier-session-raw.png` | Terminal with "attached · pty 0 · 118×38 · scrollback 10k", buttons ^C / clear / wrap; same right panel |
| Bots | `glacier-bots.png` | Cards: agent, model, description, tags, usage, Edit, Start session |
| Automations | `glacier-automations.png` | Rows: name, bot · schedule or trigger · outcome, last run |
| Documents | `glacier-documents.png` | Project-tagged rows linked to the session that produced each one |
| Notes | `glacier-notes.png` | Project-tagged (or "All projects") notes; pinned notes first |
| Settings | `glacier-settings.png` | Providers (connection state), notifications, appearance |

**Rail:** the seven-bar mark and the wordmark **Chorus** at the top (where the mockup draws a rotated
square and "Harness"), then Overview (badge = sessions needing you), Sessions, Bots, Automations,
Documents, Notes, Settings; then **Projects** (colour dot, session count, attention dot, +); **New
session** pinned at the bottom. The toggle beside the wordmark collapses it to an icon rail, with
labelled buttons and an expand control. On first use, windows under 720 px start collapsed. An explicit
choice is saved locally and takes precedence over window width.

**Session details:** the panel toggle beside Friendly/Raw shows or hides the right pane in either
view. Hidden details give their space back to the timeline or terminal. This choice is saved locally
across sessions and app launches; windows under 980 px start with details hidden on first use.

**Session header:** Codex uses its bundled logo in the agent square. A folder icon opens the
session's recorded working directory in File Explorer. Display paths are capped at 44 characters
with middle elision, keeping the final folder where it fits; hover shows the full path. Omit the
repeated agent name after the path. The selected Friendly/Raw option uses the same theme accent
as the Agent message label, with a subtle accent background and border.

**Friendly conversation (D105):** the session reads as a message thread, not a list of equal cards.
The thread scrolls on its own above a **dock** that holds, in order, any open approval or question
card, the glance bar and the composer, so the field is always in reach however far back you read.
Each exchange is one turn: a centred divider (time, duration, Done / Failed / Interrupted, a pulsing
dot while open); **what you sent** on the right, in the accent, folded past ten lines with "Show the
full prompt" and badged when it came from the phone, an automation or the loop; **what the agent did**
on a thin rail beneath it (its notes in order, the latest plan, question and approval records, the
tool-call list), open while the turn runs and folded to "What it did · 4 notes · 7 tool calls" once
it has replied; and **its reply** on the left beside the agent's logo, the widest and strongest card.
Every second reply sits on `--surface-raised-alt`, a shade lighter than `--surface-raised`, so the
eye catches where one exchange ends and the next begins.
A prompt a harness injected (a `<task-notification>`, a `<system-reminder>`) shows as one quiet
centred line with its raw text behind a click, never as your words. Copy and Source appear when a card
is pointed at or focused. Scrolling up stops the view following new content and shows a **Latest**
button that counts what arrived meanwhile ("3 new"); "Show earlier turns" at the top reads another
hundred, keeping your place, up to the back end's five hundred. The details panel's prompt list has a
**Show** link that scrolls the thread to that turn and flashes it.

**Glance bar:** one line above the composer, "Now · <where the agent is>", with suggestion chips
("+ Review", "+ Continue") that put the step in the draft, or the blocker when the agent waits on you.
It opens into the full At a glance panel (Now / Blocker / Up next, the last summary, suggestions with
Done and Dismiss, narrator cost and the project switch).

**Friendly composer:** one field that grows with its text (two lines to about ten), its controls on
a single row beneath: Attach images, the draft status, Queue and Send. The placeholder says what a
message does in the session's current state (below). Enter or Send submits it; Ctrl+Enter (or
Shift+Enter) adds a line; Queue is the button only, never anything else. Send targets the reviewed live run; Queue waits for the next
confirmed pause. Disabled actions explain themselves in their tooltip and a line beneath. The queue
and recent receipts fold beneath the field: each instruction with cancellation and review/resume
controls for held items, and receipts that distinguish written-to-terminal, agent-confirmed and
uncertain delivery. After restart, drafts remain drafts and unsent instructions are held for review.
Manual Send remains the readiness override (D80); never use it to answer an approval or bypass a busy
agent.

## Status vocabulary

| State | Label | Card CTA | Readiness panel |
|---|---|---|---|
| `approval` | Awaiting approval | Review request → | **Waiting on you.** The agent asked permission for a consequential action. Approve or deny to continue — nothing runs until then. |
| `input` | Needs your answer | Answer → | **Waiting on you.** The agent asked a question and is paused. Answer here or type your own reply. |
| `failed` | Failed | Recover → | **Agent stopped.** The process exited. History is intact — choose how to recover. |
| `working` | Working (pulsing dot) | Watch → | **Agent is busy.** Instructions you send now are queued and delivered at the next pause. |
| `ready` | Ready for next step | Choose next step → | **Ready for the next step.** Work from the previous phase is complete. Pick a step or write your own instruction. |
| `done` | Completed | Open → | **Session finished.** You can still ask follow-ups; the agent will resume in the same context. |

Urgency order: approval, input, failed, working, ready, done. **Needs you** = approval + input + failed.

**Composer placeholder by state:** approval "Or reply to the request in your own words…", input "Type your
answer, or ask a follow-up…", ready "Write your own instruction…", working "Queue an instruction for the
next pause…", done "Ask a follow-up in the same context…", failed "Tell the agent how to recover…".

**Action kinds:** CONTINUE (accent) "Send to session", ANSWER (needs) "Send answer", APPROVE (approval)
"Approve & run", DENY (muted) "Send denial", RETRY (failed) "Retry". Each action shows *what it does*,
*what it sends* (verbatim) and an ETA. A blocked action says why ("Blocked — resolve the approval first").

## Tokens

### Glacier (from `glacier-theme.css`)

| Token | Value | | Token | Value |
|---|---|---|---|---|
| `--bg` | `#0a0f16` | | `--text` | `#edf2f8` |
| `--bg-sidebar` | `#0d131c` | | `--text-2` | `#b4c0d0` |
| `--surface` | `#111925` | | `--text-muted` | `#7d8ba0` |
| `--surface-raised` | `#151e2c` | | `--accent` | `#1ec8f0` |
| `--surface-raised-alt` | `#1a2434` | | `--accent-hover` | `#4dd6f5` |
| `--surface-chip` | `#182231` | | | |
| `--border` | `#1d2a3a` | | `--on-accent` | `#03202a` |
| `--border-strong` | `#2a3a50` | | `--accent-soft` / `--accent-line` | accent at 12% / 35% |
| `--status-needs` | `#ffc400` | | `--status-working` | `#4d7cff` |
| `--status-ready` | `#00e676` | | `--status-failed` | `#ff3d5a` |
| `--status-done` | `#3a4a60` | | status `-soft` tints | each at 12% |
| `--tag-pink` | `#f472b6` | | `--tag-tan` | `#e0a96d` |
| `--tag-lavender` | `#a5b4fc` | | `--tag-lime` | `#c4e05a` |
| `--radius-card` | 14px | | `--radius-control` / `--radius-pill` | 9px / 999px |

### Dusk (derived from `Harness (Dusk).dc.html`; there is no Dusk CSS file yet)

| Token | Value | | Token | Value |
|---|---|---|---|---|
| `--bg` | `#1b1e25` | | `--text` | `#e9ebf1` |
| `--bg-sidebar` | `#171a20` | | `--text-2` | `#a3aabb` (body text `#c3c8d4`) |
| `--surface` | `#22262e` | | `--text-muted` | `#6f7789` (dim `#5a6273`) |
| `--surface-raised-alt` | `#2b313b` | | `--accent` | `#4f8cff` |
| `--surface-chip` | `#2a2f39` | | | |
| `--border` | `rgba(255,255,255,.06)` | | `--accent-hover` | `#8db5ff` |
| `--status-working` | `#5ccfdc` | | `--status-needs` (question) | `#e9b85b` |
| `--status-approval` | `#f09a86` | | `--status-ready` | `#86d4a4` |
| `--status-done` | `#a3aabb` | | `--status-failed` | `#ef7a7a` |
| `--tag-pink` | `#e39ac0` | | `--tag-tan` | `#c9a77a` |
| `--tag-lavender` | `#9ea4f5` | | `--tag-lime` | `#b9c46e` |

**Note:** Dusk gives *approval* its own colour; Glacier uses `--status-needs` for both. 2.0 defines
`--status-approval` and `--status-input` in both themes (in Glacier both are `#ffc400`).

### Agent chips (both themes)
Claude Code `CC` `#e7c3a4` · Codex `CX` `#a9c4ee` · opencode `OR` `#c9b6f2`, each on its colour at 14%. Grok,
Shell and later agents need chips in the same family.

### Type
Headings **Bricolage Grotesque** 500–700 · UI **Instrument Sans** 400–600 · code and terminal
**JetBrains Mono** 400–600. Bundled with the app, never fetched at runtime.

### Motion
Working dot pulse (1.6 s), "SUMMARY UPDATING" shimmer (1.8 s), cards and toasts rise in 250 ms. All motion
respects `prefers-reduced-motion`.

## Startup splash

Matthew's brief: *"more exciting, 3D, and more engaging. Still only a few seconds but make it eye
catching."*

Working concept, to prototype in spike S7 and show him before building:

- **Voices converging.** Particles (one stream per session being restored, coloured by project) sweep in
  from depth and condense into the seven-bar mark, which then turns in 3D under soft bloom.
- **Real information.** "Restoring 6 sessions" counts up as each session actually comes back, so the
  animation is honest about what the app is doing.
- **Budget.** First frame under ~300 ms, done in ≤3 s, skippable with any key or click; reduced-motion
  users get a short static version. The splash never delays restore; it runs while restore happens.

## Logo

The app is called **Chorus** and its logo is Chorus's official **seven-bar symmetric mark** (D24). The
mockup's "Harness" wordmark and rotated-square glyph are placeholders: replace both wherever they appear
(rail, title bar, splash, window title, installer).

**Geometry** (from Chorus 1's `src/renderer/src/components/ChorusMark.vue`, the only place Chorus 1 draws
it): seven bars, each 6 px wide on a 13 px pitch, fully rounded (`rx` = 3), vertically centred in an
84 × 76 box. Heights from left to right are **28 / 44 / 62 / 76 / 62 / 44 / 28**, so bar *i* sits at
x = 13·*i* and y = (76 − h) / 2. Scale the whole box; never stretch it to another aspect.

**Colour:** the centre bar, the tallest, is the only coloured one and takes the theme's `--accent`. The
three bars either side are neutral and mirror each other, darkest at the outside: `--logo-bar-low`
(outer pair), `--logo-bar-mid`, `--logo-bar-high` (the pair beside the centre). Chorus 1 used
`#3E4650`, `#4A535E` and `#5A646F`; each 2.0 theme sets its own three steps.

**The symmetry is the mark.** A chorus is a lead voice with the rest ranged around it. Chorus 1's mocks
once drew a six-bar version with the coloured bar off-centre; that is wrong, and so is any variant that
breaks the mirror.
