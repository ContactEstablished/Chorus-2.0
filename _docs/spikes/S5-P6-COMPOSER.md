# P6 — Friendly composer and durable queue

2026-10-23. Implements the next slice agreed in D77/D78; P6 remains open.

## Behaviour

- Friendly saves scrubbed per-session drafts in SQLite. Revision checks prevent stale writes;
  switching views does not abandon an outstanding save. Enter, paste and dictation never submit.
- Send targets the reviewed run. Queue persists up to 32 unsent instructions, processed FIFO by
  the backend without a view attached. Questions, approvals, Raw edits and unobserved delivery
  pause automatic dispatch; it requires structured Ready, not silence.
- Submission atomically clears the saved draft and creates a queue entry. Claiming that entry
  and inserting its prompt/uncertain receipt is another atomic transaction before any PTY write.
  Cancellation shares the action lock and only cancels unsent instructions.
- A run change or app restart holds unsent instructions for explicit review. A held head blocks
  later items. Written or uncertain delivery is never automatically retried. The UI distinguishes
  queued, held, cancelled, written, uncertain and agent-confirmed receipts.
- Initial readiness and possible Raw drafts require an explicit empty-and-ready confirmation
  for manual sending. It is scoped to the run and Raw input revision and cannot override a busy
  agent, pending ask or unobserved delivery. Recognised terminal replies do not count as typing.
- The composer and queue currently support Claude and Codex. Other agents give a visible Raw
  fallback. This does not enable Codex's experimental app-server plan/control bridge.

## Verification

Isolated dev app, separate database and scratch folder, existing subscriptions; no provider-key
turns. Claude Code 2.1.308 and Codex 0.163.0 completed multiline direct prompts and queued follow-ups
through production composer commands. History linked each confirmed prompt once with source
`composer`. Claude's initial and post-restart submissions were also driven through the actual
Friendly buttons. The Raw transcript showed the same conversation and answers.

Claude checks also covered cancelling an unsent instruction (no turn), a prompt queued behind an
unconfirmed/working turn, and Raw text pausing dispatch. The app was forcibly terminated with an
unsent queue item and a separately saved draft. After relaunch the queue item stayed held with no
prompt receipt; the draft text and revision survived. Explicit Send next now completed that item
once in the new run and left the draft untouched. An earlier unconfirmed attempt remained recorded
against its original run and was not replayed.

The first live attempt exposed Enter arriving in the same TUI input batch as the text. It remained
written/unconfirmed and was not retried automatically. L55 records the 150 ms gap that fixed it;
subsequent direct and queued sends completed. L56 records terminal-focus and UI polling findings.

Automated checks cover FIFO ordering, pending asks, unknown readiness, locking, stale runs/Raw
reviews, byte limits/control stripping, draft revision conflicts, secret scrubbing, cancellation,
request-identity reuse, partial writes, transaction failure, held-item recovery, frontend save
ordering and ambiguous IPC responses. Final check: 465 Rust tests and 28 frontend tests pass;
TypeScript, Clippy with warnings denied, generated bindings and secret scan pass.

Private runtime scripts and result files are under `_verify/composer/`; they are not committed.
These bounded checks do not replace P6's planned multi-phase Claude/Codex Friendly-only acceptance.

## Composer layout follow-up (2026-10-23)

The composer and active reply cards now follow the chronological history; opening Friendly scrolls
to the latest end. Content resizing follows the bottom only while the reader is near it, including
late-loading Markdown images. The queue button is labelled `Queue`.
Verified in the running dev app: latest reply above the composer, new button label, saved draft
preserved, and scrolling up stays in place across history refreshes. TypeScript and all 28 frontend
tests pass; secret scan and whitespace checks are clean. No backend changes in this follow-up.

Raw draft import remains an unimplemented requirement. The existing input flag only detects that
Raw may have been edited. The installed Codex app-server request schema has no TUI draft-read or
draft-transfer command, and the connected hooks observe submitted prompts. Claude documents an
[external-editor handoff](https://code.claude.com/docs/en/interactive-mode), which is an avenue to
investigate, not a verified automatic import mechanism. A transfer must handle multiline text,
attachments, conflicting Friendly drafts, unsupported editors and failure without losing text or
leaving it available for a second submission in Raw.

## Image attachments (2026-10-23, D79)

Files and clipboard images are decoded and normalized to PNG, then stored with the draft revision.
Queue submission transfers image ownership atomically. Each image has a thumbnail, zoom control,
bounded modal and removal button; queued and recent items can expand their saved image previews.
Delivery restores managed local image files and adds explicit file references to the persisted
prompt. This uses the agent's image-reading tools, not native TUI attachment chips. Claude manual
mode may ask permission to read the files; Friendly's approval cards remain available.

Isolated app checks against Claude Code 2.1.308 and Codex 0.163.0:

- Both agents opened two reference images and correctly identified a blue circle and orange triangle.
  Claude's two concurrent Read approvals were answered through the production reply commands.
- A forced app restart retained a queued circle and a separate draft square. The queue was held,
  explicit Send next now sent only the circle, and the draft image remained. Queueing that image-only
  draft later dispatched automatically; Codex identified the magenta square. All corrected deliveries
  were observed exactly once with composer history sources.
- Clipboard paste added a third thumbnail; its X removed it before submission. The preview modal
  stayed within the window, Escape closed it and returned focus to the thumbnail.
- L57 records the 750 ms paste gap and exact Claude wrapper matching; L58 records the approval
  bug exposed by simultaneous image reads. Initial probe attempts remain recorded and were not
  automatically retried.

Checks: 471 Rust tests, 30 frontend tests, TypeScript, Clippy with warnings denied and secret scan.
Private scripts/results remain under `_verify/composer/`. No new dependencies or global CLI settings.

Clipboard follow-up: Ctrl+V now works throughout the focused composer, including attachment
controls, and reads the clipboard file list with an items fallback without duplicating entries.
Image files with missing MIME metadata are decoded through the same normalization as file-picker
attachments. Busy/unsupported image paste gives visible feedback; ordinary text paste remains native.
Verified with a real Snipping Tool copy in WebView2: paste into the text box, preview in the bounded
modal, Escape back to the thumbnail, then paste again while that control has focus. Each paste added
exactly one image. Both temporary attachments were removed without submitting a prompt. TypeScript,
all 30 frontend tests, secret scan and whitespace checks passed; no backend changes in this follow-up.

Composer cleanup (D80): removed routine paste/newline guidance and the readiness checkbox. Manual
Send and Send next now supply the existing override; Queue does not. Attachment limits appear in red
only at 8 images or 18 MiB of normalized bytes and disappear after dropping below both thresholds.
An isolated browser fixture using the production component verified threshold boundaries, red styling,
manual Send/Send next now arguments, Queue without an override, and Working/FIFO blockers. The running
WebView2 app received the update through HMR. TypeScript, all 30 frontend tests and secret scan passed.
