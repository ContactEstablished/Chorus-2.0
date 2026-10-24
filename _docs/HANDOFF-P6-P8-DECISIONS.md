# P6–P8: approved decisions and remaining implementation

One list of the decisions and checks for `evening/p6-p8`. The detail is in
[the P7 review](HANDOFF-P7-REVIEW.md) and [the P8 review](HANDOFF-P8-REVIEW.md).
Updated 2026-10-24 with answers from review of those reports and the current implementation.
**Matthew approved the full decision list on 2026-10-24. No questions in this document remain awaiting
his answer.** Approvals and deferrals are recorded as D84–D93. The narrator key status below is
reported by Matthew. Approval does not mark the remaining implementation or verification complete.
The pre-merge work and the two automation changes are now built and were seen working; see
[Implemented after approval](#implemented-after-approval).

## Decisions

| # | Question | Approved answer / remaining work | Where |
|---|---|---|---|
| 1 | Keep Codex instructions over the app-server (`thread/inject_items`)? | **Approved by Matthew, 2026-10-24 (D89): keep it, with a readiness-ordering fix before merge.** The review records successful delivery and survival through forced `/compact`. The claim that Friendly always waits is too strong; see the code finding below. Automatic compaction remains unverified and deferred under D86. **Done:** `e4a83b5`, below. | P8 §9 Q1, §12; [D89](DECISIONS.md) |
| 2 | Automation sessions stay open after their turn, one more every run | **Approved by Matthew, 2026-10-24 (D84): close after successful completion by default.** Provide a per-automation "Close after successful completion" setting, on by default; turning it off keeps sessions open. Close the running agent while retaining conversation history, results and documents. Keep sessions open while awaiting an answer or approval, after failure, or when the user has taken over, queued follow-up work or locked the session. A turn ending alone is not proof the task finished. **Done:** `cf12c05`, below. | P8 §9 Q2; [D84](DECISIONS.md) |
| 3 | What counts as "still active" when runs overlap? | **Approved by Matthew, 2026-10-24 (D85): busy only.** Working, waiting for approval/answer, or still starting with its first prompt unobserved blocks the next run. Idle sessions do not block. An older session from the same automation that the user resumes manually also blocks while busy. The current implementation checks only the latest automation session; extend it to cover older busy sessions and verify the behavior. **Done:** `cf12c05`, below. | P8 §9 Q3; `src-tauri/src/automations.rs`, `busy_session`; [D85](DECISIONS.md) |
| 4 | Native Claude skills through `--plugin-dir`? | **Approved by Matthew, 2026-10-24 (D90): try in a separate follow-up after merge**, behind the same per-agent carrier table. Keep the existing skill index meanwhile: P8 §12 records an agent reading a listed `SKILL.md`. Native plugin loading is not built or verified, so do not promise it works yet. | P8 §9 Q4, §12; [D90](DECISIONS.md) |
| 5 | Codex automations? | **Approved by Matthew, 2026-10-24 (D91): Claude Code only for now.** Add Codex after verifying that the bridge delivers instructions before sending the first prompt, including failure handling and duplicate prevention. Do not use an unverified positional prompt that can race instruction delivery. | P8 §9 Q5; [D91](DECISIONS.md) |
| 6 | Each window may call only its own commands (P7-D5)? | **Approved by Matthew, 2026-10-24 (D92): yes; implement before merge.** Register the application command manifest and grant commands per window. The strip needs `sessionsList`, `prefsGet` and `fleetOpenSession`; the splash needs `restoreProgress`, plus its required Tauri window API. Test that unauthorized commands are refused and that the manifest stays aligned with IPC registration. **Done:** `31bc962`, below. | P7 §8; [D92](DECISIONS.md) |

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
confirmed in code; the race has not been reproduced in the app. **Fixed in `e4a83b5`**; it was also
wider than described: when `thread/resume` failed, the retry loop kept Friendly open with the
instructions still pending.

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
  (Dev Build)**. A real narrator call still needs verification with the stated $3 cap; key entry alone
  does not prove a successful call. Do not copy the credential into documentation or another
  instance's data directory. Running this branch on the Dev Build's data would apply migrations
  0019–0021, which `main` can't take back, so Matthew chose to enter the key again in the review
  instance, through its Settings → Providers. **Verified 2026-10-24**, below.
- **CLI installation — approved, 2026-10-24 (D87):** Matthew authorizes installing opencode and Grok
  if absent. Installation, launches and shortcut bindings still need verification; this documentation
  update does not claim either CLI has been installed or tested. The packages are `@opencode/cli`
  (the 2.x line D58 targets; `opencode-ai` is still the 1.x line) and xAI's installer at
  `https://x.ai/cli/install.ps1` (stable Grok 1.0.70; Chorus was last verified on 1.0.62). The agent
  session's safety check blocked running either installer, so Matthew runs them. **opencode 2.1.4
  installed and verified**, below. Grok 1.0.70 is installed and detected; Matthew deferred its
  sign-in and launch checks (D94).
- **Display check — deferred, low priority (D88):** Matthew defers the alternate display-scale and
  second-monitor checks in P7 §11. They are not pre-merge blockers. Later, confirm that empty strip
  space and dragging preserve the other app's focus, and that the strip returns to a visible screen
  after a monitor is removed. Real hardware/DPI behavior remains unverified.
- **Mouse check — deferred, low priority (D88):** Matthew defers the swapped-button check in P7 §11.
  It is not a pre-merge blocker. Later, verify dragging and chip clicks with swapped primary buttons,
  then restore the original setting. The code change exists; real-input behavior remains unverified.

## Implemented after approval

Each was seen in the review instance (`com.contactestablished.chorus2.dev.eve`), driven through its
own IPC and UI. `npm run check` passed before each commit.

| Decision | Commit | Seen working | Not covered |
|---|---|---|---|
| D89 Codex readiness | `e4a83b5` | On Restart with changed notes, twice, the bridge logged "Friendly waits for its instructions", gave them about 430 ms later, then opened Friendly. Polling from the page never saw Friendly ready before the instructions were marked given, and Codex answered with the new note's canary word. A new thread opens Friendly 1–2 ms after its instructions. | A failed injection was not forced in the app. In code it is recorded visibly (session note, not given), is not retried, and then Friendly opens. |
| D92, D93 window commands | `31bc962` | From the strip, `sessions_list` and `prefs_get` answer and `fleet_open_session` switches the main window; `app_info`, `fleet_probe`, `vault_list`, `notifications_list`, `automations_list` and `restore_progress` are refused as "not allowed on window fleet". The splash may call only `restore_progress` and still closes itself. The main window can call every command. | A release build was not run. Its access list is the same; only the refusal message is shorter. |
| D84 close after success | `cf12c05` | With the setting on and nobody touching the run, Chorus closed the agent 7–20 s after the turn ended. The session stays exited and reads Done, with no notification, and the run reads "finished, and Chorus closed its agent". Restart then repeated the earlier reply. With Raw typing during the run, or with the setting off, the session was still open 40 s after its turn. The editor shows the checkbox with each automation's value. | A Friendly follow-up, a queued prompt and a lock are covered by the rule's unit tests, not by app runs. Background work an agent leaves running after its turn is stopped with it. |
| D85 overlap | `cf12c05` | With an older session from the same automation working and the latest idle, Run now was skipped: "Skipped: Dash prompt run #2, from an earlier run, is still working." | — |

**What "close" does (D84):** it stops the agent and leaves the session *exited*, not *closed*. A
closed session leaves the UI and can't be restarted, which would lose the follow-up D84 keeps.
Exited sessions are not relaunched when Chorus starts.

**Found and fixed while verifying D84:** the first run read as failed and raised a failure
notification, because a stopped agent exits non-zero. A run Chorus closed after it finished now
counts as done.

**Proposed lesson (to number):** in Tauri 2.12, once `AppManifest::commands` is set, every app
command is checked against the calling window's capabilities, the main window's included. Each
command's permission is `allow-<name with hyphens>`. Debug builds say where a refused command is
allowed; release builds say only "Command X not allowed by ACL".

## Checked with the provided access

In the review instance, on Matthew's OpenRouter provider (its own key, plus a management key that
mints a key per opencode run, capped at the provider's $3). Everything below spent about $0.0017 in
total, as OpenRouter reported it.

| Check | Seen working | Not covered |
|---|---|---|
| Narrator call (D82) | One automation turn each with `openai/gpt-4.1-nano` and `google/gemini-2.5-flash-lite`: a summary was stored about 8 s after the run started, and the session's panel shows it with the model, time and reported cost ($0.00008 and $0.00005). Two opencode turns were then summarized too. | Provider failures and restarts were checked on `main` (D82); not again here. |
| Narrator output | Gemini 2.5 Flash Lite followed the prompt's rules: a finished task got no suggestions and "No further action is suggested". GPT-4.1 nano suggested sending the agent's own answer back, which the prompt forbids. | Not a Chorus defect, but the model choice matters; Gemini 2.5 Flash Lite is the better cheap default, and Matthew chose it. |
| opencode 2.1.4 launch | Detected as 2.1.4 ("newer" than the verified 2.1.1). A session on OpenRouter with `google/gemini-2.5-flash-lite` minted its own capped key, ran a turn (Working → Ready in about a second, reply "juniper", no tools) and reported the model it really used, matching the one asked for. Restart kept the opencode session id and the resumed conversation answered "juniper" about the earlier turn. Restart and Close each revoked the run's key, and each run's cost was read back from OpenRouter: $0.00073 and $0.00074. | Permissions, approvals and questions were not exercised on 2.1.4. A new session reads Running, not Ready, until its first turn, as before: opencode reports nothing until a turn. |
| opencode keys (P7-D3) | In Raw, Ctrl+Shift+F sent as browser key events opened Chorus's search bar and wrote no key to opencode (only the focus-out report opencode asks for); Escape closed it and focus returned. Ctrl+F still reached opencode (`\x06`; in its prompt, "move cursor right"). The 2.1.4 binary has no `ctrl+shift+f` binding string. | Sent through the WebView's input pipeline (DevTools protocol), not a physical keyboard. |

**Install notes:** npm 12.2.0 blocks package install scripts unless allowed, and `@opencode/cli`
places its real binary in a postinstall step, so a plain `npm i -g @opencode/cli` leaves a 229-byte
placeholder that only prints "postinstall script was not run". `npm install -g
--allow-scripts=@opencode/cli @opencode/cli@2.1.4` completes it; the script only copies the platform
package npm already downloaded. When npm's global folder (`%APPDATA%\npm` by default) isn't on
PATH, Chorus reports "opencode.cmd not found on PATH"; it was missing here, so the review instance
was started with that folder added to its own PATH. The user's PATH was not changed.

## Then

The pre-merge fixes and the approved automation behavior are implemented and verified (above).
The narrator and opencode checks are done (above), and Matthew is testing the narrator and automations
in the review instance. Keep automatic compaction (D86), display-scale/monitor and swapped-button
checks (D88) and Grok's sign-in and launch (D94) tracked as deferred work;
do not treat their deferral as successful verification.

**D83 is now on this branch too.** Matthew's uncommitted D83 change in the main checkout (project
folders required and unique, with a Browse button on Add and Edit) was copied here unchanged, at his
request, so he can test it with P6–P8. Two P8 tests that made projects without folders now give them
one. In the review instance: a missing or relative folder is refused; a folder is stored resolved; the
same folder in another spelling is refused, also when its project is archived; a project keeps its
own folder on Edit; a folderless project reads "Folder required" and can't be saved without one; Add
stays disabled until both fields are filled. Not covered: the Windows folder dialog itself. Before
merging into `main`, discard or stash the main checkout's uncommitted D83 copy, which this branch
now carries. No merge was performed.
