# P6–P8: approved decisions and remaining implementation

One list of the decisions and checks for `evening/p6-p8`. The detail is in
[the P7 review](HANDOFF-P7-REVIEW.md) and [the P8 review](HANDOFF-P8-REVIEW.md).
Updated 2026-10-24 with answers from review of those reports and the current implementation.
**Matthew approved the full decision list on 2026-10-24. No questions in this document remain awaiting
his answer.** Approvals and deferrals are recorded as D84–D93. The narrator key status below is
reported by Matthew; this document records no new runtime tests. Approval does not mark the remaining
implementation or verification complete.

## Decisions

| # | Question | Approved answer / remaining work | Where |
|---|---|---|---|
| 1 | Keep Codex instructions over the app-server (`thread/inject_items`)? | **Approved by Matthew, 2026-10-24 (D89): keep it, with a readiness-ordering fix before merge.** The review records successful delivery and survival through forced `/compact`. The claim that Friendly always waits is too strong; see the code finding below. Automatic compaction remains unverified and deferred under D86. | P8 §9 Q1, §12; [D89](DECISIONS.md) |
| 2 | Automation sessions stay open after their turn, one more every run | **Approved by Matthew, 2026-10-24 (D84): close after successful completion by default.** Provide a per-automation "Close after successful completion" setting, on by default; turning it off keeps sessions open. Close the running agent while retaining conversation history, results and documents. Keep sessions open while awaiting an answer or approval, after failure, or when the user has taken over, queued follow-up work or locked the session. A turn ending alone is not proof the task finished. Implementation and runtime verification remain outstanding. | P8 §9 Q2; [D84](DECISIONS.md) |
| 3 | What counts as "still active" when runs overlap? | **Approved by Matthew, 2026-10-24 (D85): busy only.** Working, waiting for approval/answer, or still starting with its first prompt unobserved blocks the next run. Idle sessions do not block. An older session from the same automation that the user resumes manually also blocks while busy. The current implementation checks only the latest automation session; extend it to cover older busy sessions and verify the behavior. | P8 §9 Q3; `src-tauri/src/automations.rs`, `busy_session`; [D85](DECISIONS.md) |
| 4 | Native Claude skills through `--plugin-dir`? | **Approved by Matthew, 2026-10-24 (D90): try in a separate follow-up after merge**, behind the same per-agent carrier table. Keep the existing skill index meanwhile: P8 §12 records an agent reading a listed `SKILL.md`. Native plugin loading is not built or verified, so do not promise it works yet. | P8 §9 Q4, §12; [D90](DECISIONS.md) |
| 5 | Codex automations? | **Approved by Matthew, 2026-10-24 (D91): Claude Code only for now.** Add Codex after verifying that the bridge delivers instructions before sending the first prompt, including failure handling and duplicate prevention. Do not use an unverified positional prompt that can race instruction delivery. | P8 §9 Q5; [D91](DECISIONS.md) |
| 6 | Each window may call only its own commands (P7-D5)? | **Approved by Matthew, 2026-10-24 (D92): yes; implement before merge.** Register the application command manifest and grant commands per window. The strip needs `sessionsList`, `prefsGet` and `fleetOpenSession`; the splash needs `restoreProgress`, plus its required Tauri window API. Test that unauthorized commands are refused and that the manifest stays aligned with IPC registration. | P7 §8; [D92](DECISIONS.md) |

**Keep `fleet_probe` in release builds — approved by Matthew, 2026-10-24 (D93):** yes, restricted to
the main window by the command permissions above. It reads diagnostic window information and is useful
for investigating display/focus issues.
Its shield field is looked up, not assumed. The strip and splash do not need permission to call it.

P7 §9 Q1 (notification bodies naming the ask) is already done. The proposed decision rows
(P7-D1…D5, P8-D1…D6) and lessons (P7-L1…L4, P8-L1…L4) are written in DECISIONS.md and LESSONS.md
style for the reviewer to reconcile with the decision log. All answers and deferrals in this document
are now approved and logged as D84–D93. Preserve the supporting reviews' historical evidence and
verification limits when reconciling their proposal rows; approval is not proof of implementation.

## Code finding and remaining verification

**Codex readiness ordering:** in `src-tauri/src/sessions/codex_bridge.rs`, `bind` sets `ready = true`
before `thread/resume` and its subsequent `inject_instructions` call. When resuming a thread that is
not yet loaded, the earlier injection is skipped, so Friendly can become available while instructions
are still pending. Both `composer.rs` and `delivery.rs` check this ready flag. This ordering was
confirmed in code; the race has not been reproduced in the app.

Fix the ordering and verify the resumed-thread path before merge. Instruction failure must remain
visible, without silently treating instructions as delivered or automatically replaying an uncertain
injection. The approximately 0.6-second Raw startup window in P8 §12 is one measurement, not a bound;
it does not establish that only Raw can beat instruction delivery on every startup path.

**Automatic compaction — approved deferral, 2026-10-24 (D86):** Matthew accepts testing automatic
compaction late in a long session as a tracked follow-up after the readiness fix, rather than a
pre-merge requirement. Forced `/compact` passed according to P8 §12; automatic compaction remains
unverified. Keep that limitation explicit until tested. The readiness-ordering fix and its
verification remain pre-merge work.

## Access and verification status

- **Narrator access — provided:** Matthew reports that a key has been entered in **Chorus 2
  (Dev Build)**. No further key-entry request is pending. A real narrator call still needs verification
  in that build with the stated $3 cap; key entry alone does not prove a successful call. Do not copy
  the credential into documentation or another instance's data directory.
- **CLI installation — approved, 2026-10-24 (D87):** Matthew authorizes installing opencode and Grok
  if absent. Installation, launches and shortcut bindings still need verification; this documentation
  update does not claim either CLI has been installed or tested.
- **Display check — deferred, low priority (D88):** Matthew defers the alternate display-scale and
  second-monitor checks in P7 §11. They are not pre-merge blockers. Later, confirm that empty strip
  space and dragging preserve the other app's focus, and that the strip returns to a visible screen
  after a monitor is removed. Real hardware/DPI behavior remains unverified.
- **Mouse check — deferred, low priority (D88):** Matthew defers the swapped-button check in P7 §11.
  It is not a pre-merge blocker. Later, verify dragging and chip clicks with swapped primary buttons,
  then restore the original setting. The code change exists; real-input behavior remains unverified.

## Then

Implement the pre-merge fixes above and the approved automation behavior, and verify the changes.
Use the provided narrator access and CLI installation authorization for the remaining runtime checks.
Keep automatic compaction, display-scale/monitor and swapped-button checks tracked as deferred work;
do not treat their deferral as successful verification. Reconcile the decision log when merging
`evening/p6-p8` into `main` (D83 is reserved in the main checkout).
The main checkout's uncommitted D83 work remains outside this document edit; no merge was performed.
