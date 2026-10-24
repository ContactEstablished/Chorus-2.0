# P8 review — Bots, Automations, Notes, Documents

## 1. Verdict

**Done for Claude Code and Codex 0.163.0, with stated limits.** Notes, Bots, Documents and
Automations are built, and every P8 exit criterion was seen working in the isolated `eve8` instance:
Bot and note instructions applied in Claude Code and in Codex (canaries), a project note reached its
project's session and not another project's, editing a Bot reached a new session but not the running
one, a plan a session wrote appeared in Documents linked to it, and an automation started sessions on
schedule and with Run now, each first prompt recorded once with source `automation`.

Limits: automations start Claude Code Bots only; Documents finds files Claude Code and Codex wrote
with their file tools (Codex's patches since §12), not files written by a shell command; opencode
and Grok aren't installed here, so their instructions are declared unsupported with the reason
shown. Codex instructions use a route found tonight (`thread/inject_items`, not argv). It is built,
but it is a decision for Matthew (§9 Q1). Everything waiting for Matthew across P6–P8 is gathered in
[HANDOFF-P6-P8-DECISIONS](HANDOFF-P6-P8-DECISIONS.md).

## 2. Branch

- Branch `evening/p8`, worktree `Chorus-2.0-eve8`, base `d62e078` (`main`). Not rebased, merged or
  pushed: the evening brief says the reviewer merges.
- Commits before this file (`git log --oneline main..HEAD`):

```
456b388 P8: plain words for opencode's instructions reason
24a6c2e P8: Settings → Agents says how each agent takes instructions
082bfc4 P8: idle relaunched sessions don't block automations; files go at quit
8e878b6 P8: Codex instructions through its private app-server
e4b00bc P8: Automations screen
93f0215 P8: Documents screen
c78f11e P8: Bots screen, Start session and Bot chips in New session
9945597 P8: notes, run instructions for Claude Code, and the P8 data model
```

  Plus this review file as the last commit. Migration `0019_p8_bots_notes.sql`; nothing depends on
  its number.

## 3. Files

**Created**

| File | Purpose |
|---|---|
| `src-tauri/migrations/0019_p8_bots_notes.sql` | Renames `launch_profiles` to `bots` and extends it; `sessions.bot_id`/`bot_snapshot`; `runs.instructions`; `notes`, `automations`, `automation_runs`, `documents` |
| `crates/chorus-core/src/instructions.rs` | Composes a run's text (Bot, notes pinned-then-newest, skill index), 24 KiB cap with "N left out", per-agent `carrier()`, note checks, `SKILL.md` front matter, text hash |
| `crates/chorus-core/src/schedule.rs` | `Schedule` (every N >= 15 min, daily, chosen days), `next_run` with a per-instant offset function (DST), `due()` (fire, wait or missed) |
| `crates/chorus-core/src/documents.rs` | Which written paths are documents and of which kind; title from the first `# heading` |
| `src-tauri/src/notes.rs` | Notes store and commands |
| `src-tauri/src/bots.rs` | Bots store and commands, the copy a session takes at create, the key-refusal check |
| `src-tauri/src/instructions.rs` | Per-run composition, scrubbing, the instruction file, Claude's `--system-prompt-snapshot off` on a changed resume, the Codex hand-off to the bridge, run facts, file clean-up, `instructions_support` |
| `src-tauri/src/documents.rs` | The documents pass over `tool_calls` (its own thread), list and open |
| `src-tauri/src/automations.rs` | Automations store and commands, the scheduler thread, the first prompt's staging and linking |
| `src-tauri/src/p8.rs` | P8's one start-up call |
| `src/views/NotesView.vue`, `BotsView.vue`, `DocumentsView.vue`, `AutomationsView.vue` | The four screens |
| `src/views/session/SessionInstructionsFacts.vue` | Bot and Instructions rows in session facts |
| `src/stores/bots.ts` | Bots list, and the Bot New session applies when Start session opens it |

**Modified** (hot files marked, with the reason)

| File | Change |
|---|---|
| `src-tauri/src/sessions/mod.rs` (hot) | `CreateSession.bot_id` (serde default, so the TS field is optional) and `first_prompt` (serde and specta skip: Rust callers only). `create_in`: `bots::for_launch` before the insert (refuses a missing or archived Bot), `bots::attach` and `automations::stage_prompt` after. `start_run`: one `instructions::prepare` after the redaction list is built (its note joins the session note), its args after the conversation args (non-server path), `notes.extend(instructions::record(…))` after the run row. Exit watcher and `shutdown`: `instructions::run_ended`. The Pair partner literal gets `bot_id: None, first_prompt: None`. `mod codex_bridge` becomes `pub(crate)` |
| `src-tauri/src/sessions/codex_bridge.rs` | An `instructions` field, `set_instructions`, `inject_instructions` once the thread is loaded; `bind` narrowed to `pub(super)` (it exposes the private `Run`) |
| `src-tauri/src/sessions/tests.rs` | The two new fields in six `CreateSession` literals |
| `src-tauri/src/sessions/profiles.rs`, `src-tauri/src/store/profiles.rs` | Table renamed to `bots`; archived Bots hidden from New session; wording |
| `src-tauri/src/store/mod.rs` | 0019 listed; the Aider migration test reads `bots` (0019 renames the table) |
| `src-tauri/src/ipc.rs` | One `// p8:` block at the end of the commands, one `use` line |
| `src-tauri/src/lib.rs` | Six `mod` lines and `p8::start(&manager)?` (before `reopen()`, so stale instruction files go before any relaunch) |
| `crates/chorus-core/src/lib.rs` | Three `pub mod` lines at the end |
| `src/app/App.vue` (hot) | The four `PhasePlaceholder` tags become the screens (async components); the unused import goes |
| `src/views/newSession/NewSessionForm.vue` (hot) | Chip label "Bots", "Save as Bot"/"Update Bot", `botId` set by a chip and sent in `CreateSession`, and the Bot from Start session applied on open with its preset (18 lines) |
| `src/views/session/SessionFacts.vue` (hot) | One import and one `<SessionInstructionsFacts>` tag |
| `src/views/settings/AgentsSettings.vue` | A line per agent: how it takes instructions, or why not |
| `src/ipc/index.ts` | P8 types re-exported |
| `src/ipc/bindings.ts` | Regenerated |

Not touched: `agent.rs` (the carrier is `chorus_core::instructions::carrier()`, a match with a
wildcard arm, so P14's kinds and P7's Claude `-n` merge without a `Spec` field), `events/recorder.rs`,
`SessionSummary`, `Rail.vue`, `OverviewView.vue`, `SessionView.vue`, `stores/sessions.ts`. No files
deleted; `PhasePlaceholder.vue` stays, now unused.

## 4. Surface added

**Migration 0019:** `bots` (was `launch_profiles`) plus `description`, `instructions` (Markdown,
default ''), `tags` (JSON), `skills` (JSON folder paths), `preset` (CHECK solo/pair/workbench/swarm),
`archived_at`; `sessions.bot_id` (FK SET NULL) and `sessions.bot_snapshot` (JSON copy at create);
`runs.instructions` (JSON `RunInstructions`); `notes(id, project_id FK CASCADE, title, body, pinned,
created_at, updated_at)`; `automations(id, name UNIQUE, bot_id FK SET NULL, prompt, schedule JSON,
cwd, project_id FK SET NULL, enabled, next_run_at, created_at, updated_at)`;
`automation_runs(id, automation_id FK CASCADE, session_id FK SET NULL, trigger schedule/manual,
due_at, started_at, outcome started/failed/missed/skipped, note)`; `documents(id, session_id,
project_id, path UNIQUE, kind, title, first_seen_at, last_seen_at, missing_at)`.

**Commands:** `bots_list() -> Bot[]`, `bot_save(BotSave) -> Bot`, `bot_set_archived(id, archived) ->
Bot`, `notes_list() -> Note[]`, `note_save(NoteSave) -> Note`, `note_delete(id) -> bool`,
`session_instructions(sessionId) -> SessionInstructions`, `instructions_support() ->
InstructionSupport[]`, `documents_list() -> Document[]`, `document_open(id)`, `automations_list() ->
Automation[]`, `automation_save(AutomationSave) -> Automation`, `automation_set_enabled(id, enabled)
-> Automation`, `automation_delete(id) -> bool`, `automation_run_now(id) -> AutomationRun` (async),
`automation_runs(id) -> AutomationRun[]`. `CreateSession` gains optional `botId`. The `profiles_*`
commands keep working on `bots`. No events added.

**Settings key:** `documents.scan_cursor` (the last `tool_calls.id` the documents pass read).

**Files Chorus now writes:** `<app data>/state/<session>/instructions.md` per run, scrubbed, deleted
when the run ends, when Chorus quits and at start-up. Nothing in a repository, a worktree or any
agent's own or global configuration.

**Arguments:** Claude Code gets `--append-system-prompt-file <that file>`, and on a resumed
conversation whose recorded instructions differ, `--system-prompt-snapshot off`. An automation's
first prompt (scrubbed) is Claude Code's positional prompt, so it is in argv and `runs.args`, as the
plan allows for prompts. Codex gets nothing new in argv; its text goes over the bridge's
authenticated socket.

**Threads:** `documents` (a pass every 5 s) and `automations` (wakes at the next due time, at most
30 s apart). **Dependencies and features:** none added. The local UTC offset comes from SQLite's
`localtime` modifier, so no `windows` feature was needed.

## 5. Plan against result

The evening brief overrides the plan where they differ (prompt delivery already existed, one
migration numbered 0019, Matthew's answers to §6).

| Task | Result | Notes |
|---|---|---|
| 1. Verify delivery per CLI | Done | Claude Code 2.1.308 and Codex 0.163.0 canaries (§6). opencode and Grok aren't installed, so not verified: declared unsupported with the reason |
| 2. Migration, core, capability | Done | One migration. The capability is `instructions::carrier(AgentKind)`, not a `Spec` field (merges cleanly with P7/P14, same conformance test over `AgentKind::ALL`) |
| 3. Notes | Done | Store, commands, screen with project chips, pinned first, editor, "Untagged notes apply to every project" |
| 4. Injection at run start | Done | Read fresh each run (decision 2); facts show the notes the run got; a write failure launches without and says so (unit-tested, not provoked at runtime) |
| 5. Delivery contract | Reused | P6's `delivery.rs` path; `PromptSource::Automation` already existed. The automation's prompt rides P6's receipts: persisted at create, receipt `written` at launch, the hook's echo matched (`observed`). No `deliver.rs` |
| 6. Bots | Done | Screen, editor for every field except provider and key (kept as saved; set them from New session and Save as Bot), usage line, Archive/Restore, Start session through New session with the preset, Bot chips |
| 7. Documents | Done for Claude Code | A separate pass over `tool_calls` (no recorder edit). Codex's writes come through `apply_patch`, whose recorded target isn't a path |
| 8. Automations | Done for Claude Code | Scheduler, Run now, Pause/Resume, history, missed and skipped rows. The first prompt goes as Claude's positional prompt (no structured ready signal before a first turn; D78 forbids inferring one). Codex and others are refused with a reason |
| 9. Exit checks and review | Done | §6 |

Beyond the plan: Codex instructions over the app-server (decision 5's investigation found a non-argv
route; built and canary-checked), Claude's snapshot handling (found while verifying decision 2), and
refusing key-shaped text in notes, Bot instructions and automation prompts.

## 6. Verification

**`npm run check`** on the final code: vue-tsc clean; vitest 33 passed (9 files); `cargo test
--workspace`: chorus 90 passed, chorus-core 419 passed, purity 3, other suites 3; clippy `-D
warnings` clean; secret scan clean (7 patterns over 357 files). New tests: core 19 (instructions 8,
including the carrier conformance test over `AgentKind::ALL`; schedule 8; documents 3), back end 10
(notes 2, bots 2, documents 1, automations 2, instructions 3).

**Runtime, in `eve8` (all Claude turns on `haiku`, effort Fast; evidence in `_verify/p8/`):**

| Check | What I did | What I saw | Evidence |
|---|---|---|---|
| Note reaches its project only | Projects Alpha and Beta (scratch folders under `%TEMP%`); note "Canary" tagged Alpha, "Everywhere" untagged; a Claude session in each; asked "What is the canary word?" | Alpha: `heliotrope`; Beta: `None.`. Facts: Alpha got 2 notes, Beta 1. The instruction file was 231 and 162 bytes; Claude's command line had the file flag and not the text | `notes-canary-history.json`, `session-facts-instructions.png`, `notes-screen.png` |
| Restart picks up an edited note (decision 2) | Changed the note's word to `marigold`, Restart, asked again | `marigold`; facts say Claude renders its prompt afresh (`--system-prompt-snapshot off`) | `restart-canary-history.json` |
| Bot instructions apply; editing a Bot doesn't reach a running session | Bot "Canary reviewer" (bot word `saffron`, one skill folder); Start session from the Bots screen (dialog filled, "Filled from Canary reviewer."); edited the Bot to `cobalt`; asked the running session; started a second from the Bot and asked | Running session: `saffron`. New session: `cobalt`. Facts: Bot, 1 note, skill `release-notes` | `bot-canary-history.json`, `bot-canary-new-history.json`, `bots-screen.png`, `bot-start-dialog.png` |
| Codex gets Bot and note instructions without argv | Bot "Codex fixer" (`saffron`), session in Alpha; asked for both words | `saffron marigold`. Facts given. Neither word in any `codex.exe` command line or `runs.args`. After a Restart and two Chorus restarts (four runs, all resuming with the same text) the thread's rollout holds one injected developer message, so nothing was injected again | `codex-canary-history.json`, `codex-bot-dialog.png` |
| Automation, Run now | "P8 canary check" (Canary reviewer, every 15 minutes, Alpha folder); Run now from the screen | Session `#1` made; one `prompts` row, source `automation`, receipt `observed`; the turn's answer `cobalt` | `automation-runnow-history.json`, `automations-screen.png` |
| Automation, scheduled firing | Waited for the 23:47:22 due time | Run row `schedule`/`started`, session `#2`, one `automation` prompt row, `observed`, answer `cobalt` | `automation-runs-scheduled.json`, `automation-scheduled-*.json` |
| Missed while closed (decision 3) | Daily automation due 23:50; quit Chorus at 23:48; started it at 23:51 | Row `missed` "Missed: Chorus was closed."; next run moved to the next day; no session made | `automation-missed.json`, `automations-missed.png` |
| Skipped while the previous run is busy | Run now with the previous session relaunched and its activity unknown | Row `skipped` with the reason. That exposed a flaw (an idle relaunched session blocked every run), fixed in `082bfc4`; Run now after the fix started session `#3` normally | `automation-skipped.json`, `automation-runnow-after-relaunch.json` |
| Document appears and opens | Asked a Claude session to write `_docs/PLAN-test.md`; approved the write | Listed within seconds: "Test plan for documents", Plan, linked to its session, project Beta. Open returned success and Windows showed "Pick an app": `.md` has no associated application on this machine (I closed that dialog by its PID) | `documents-screen.png` |
| Planted key | A key-shaped fake key in a note, a Bot's instructions and an automation prompt | All three refused; the key (searched as bytes, never printed, length 57) occurs 0 times in the 18 files of the instance's data folder | `plant-key.json` |
| Instruction files | Counted after quit and after start | 0 left after quit (final build); relaunched runs write their own | console |
| Settings → Agents | Opened it | Each agent says how it takes instructions or why not | `settings-agents.png` |

**Not verified, and why:**
- opencode and Grok instructions: neither CLI is installed here. Declared unsupported in the UI.
- Skills being read by an agent: the index reaches the file (checked), but no turn asked an agent to
  use a skill.
- The 24 KiB cap, "N left out" and a failed file write: unit tests only.
- A daylight-saving change: unit test only (`a_daylight_saving_change_keeps_the_local_time`).
- An archived Bot or a missing folder failing an automation visibly: code path and save-time
  refusals seen; no runtime firing of that case.
- Codex compaction dropping the injected message: not tested (§9 Q1).
- The skipped-run rule after the fix (working, asking or first prompt unseen): only the "not busy"
  side was seen at runtime.
- A document opening in an editor: this machine has no `.md` association, so only the request was
  seen to succeed.

**Turns spent:** Claude Code 3 (CLI probe) + 9 (app), Codex 4 (probe) + 1 (app); all subscription,
all tiny. No API credit.

## 7. Versions verified against

Claude Code 2.1.308 (`--append-system-prompt-file`, `--system-prompt-snapshot`, positional
`[prompt]`, `--plugin-dir`), Codex CLI 0.163.0 (app-server `thread/inject_items`,
`thread/start`/`thread/resume` `developerInstructions`, `model_instructions_file`), Node 24.15.0 (its
`node:sqlite`, used only for local evidence queries). opencode and Grok: not installed.

## 8. Proposed doc changes

**Decisions** (DECISIONS.md row style, status Proposed; D83 is taken, so the reviewer numbers them):

| Id | Date | Decision | Status | Why |
|---|---|---|---|---|
| P8-D1 | 2026-10-23 | **Bots are the launch profiles, renamed and extended.** `launch_profiles` becomes `bots` with description, instructions, tags, skill folders, preset and archive. New session's chips are Bot chips. A session copies the Bot's instructions and skills at create, so editing a Bot never changes it; a Pair's partner gets no Bot | Proposed | Matthew's decision 1 (a). Seen: an edited Bot reached a new session, not the running one |
| P8-D2 | 2026-10-23 | **Notes are read at each run start.** Untagged notes apply everywhere; pinned first, then newest; 24 KiB in all, the rest left out and counted. Session facts list the notes the current run got. A resumed Claude conversation whose recorded instructions differ runs with `--system-prompt-snapshot off` | Proposed | Matthew's decision 2 (a). Claude 2.1.308 otherwise reuses the first request's system prompt on every resume (P8-L1) |
| P8-D3 | 2026-10-23 | **Instructions travel out of argv, per agent.** Claude Code: `--append-system-prompt-file`, a scrubbed file in the session's state folder, deleted when the run ends, at quit and at start. Codex 0.163.0: a developer message the bridge injects (`thread/inject_items`) once its thread is loaded; unchanged text isn't injected again on a resume, changed text is injected saying it replaces earlier Chorus instructions. Codex without the private server, opencode, Grok, Gemini CLI, Aider and Shell get none, and say why. Chorus never writes repository or global agent configuration for this | Proposed | Decision 5: argv rejected. Canary-verified for Claude and Codex |
| P8-D4 | 2026-10-23 | **Skills are a provider-neutral index.** A Bot lists folders holding `SKILL.md`; each run's instructions name them with description and path and say to read `SKILL.md` first. Native loading isn't built | Proposed | Matthew's decision 4 (a); D31 |
| P8-D5 | 2026-10-23 | **Automations run Claude Code Bots while Chorus runs.** Every N >= 15 minutes, daily or chosen days, local time. A time missed while Chorus was closed is recorded as missed, never caught up; a run whose previous session is still working, asking or starting is skipped with the reason; Run now and Pause on the row. The first prompt is persisted at create (source `automation`) and passed as Claude's positional prompt; its hook echo is matched to the row through D75's receipts | Proposed | Matthew's decision 3 (a). No structured ready signal exists before a first turn, and D78 forbids inferring one |
| P8-D6 | 2026-10-23 | **Documents are Markdown plans, specs, reviews, reports and decision records a session wrote** at the top of its folder or under `_docs/`, `docs/` or `doc/`, found by a pass over `tool_calls` and kept with their first session and its project. Opening goes through the opener from Rust after a containment check; a moved or deleted file shows as missing | Proposed | Matthew's decision 6 |
| P8-D7 | 2026-10-23 | **Text that looks like a key is refused** in notes, Bot instructions and descriptions, and automation prompts, as well as being scrubbed wherever it is written | Proposed | Notes and prompts reach files, argv and the database; refusing says so instead of silently changing the text |

**Lessons:**

- **P8-L1.** Claude Code 2.1.308 records the system prompt at a conversation's first request and
  sends that record on every later request and resume, even when a later launch passes a different
  `--append-system-prompt[-file]` (`--system-prompt-snapshot on`, the default). `off` renders it
  afresh. A canary answered with the first file's word after a resume with a second file, and with
  the second's when `off` was passed.
- **P8-L2.** Codex 0.163.0's app-server: `developerInstructions` on `thread/resume` of a loaded thread
  is ignored without a warning; `model_instructions_file` replaces Codex's built-in instructions
  (about 21 KB) rather than adding to them; `thread/inject_items` with a developer message works from
  a second client on a thread with no turns yet, stays in the rollout across a server restart, and
  adds a second copy if injected again.
- **P8-L3.** SQLite's `localtime` modifier gives the local UTC offset at any instant, daylight saving
  included, so a schedule needs no time-zone crate or `windows` feature.
- **P8-L4.** After a relaunch an idle agent's activity is unknown until its next event, so "unknown"
  can't mean "busy" for anything that waits on a session.

**PLAN/SCOPE:** P8's status can move to built-with-limits once merged; the limits are §1's.
**ARCHITECTURE §5:** add the instruction carriers (P8-D3) to the per-agent notes.

## 9. Questions for Matthew

1. **Keep Codex instructions over the app-server?** Built and seen working. Consequences: a first
   prompt typed within about half a second of launch could beat the injection (the bridge polls every
   500 ms); compaction may summarise the injected message away (not tested); a Restart with changed
   notes adds a second developer message that says it replaces the first. Options: (a) keep it;
   (b) turn Codex instructions off until compaction is measured; (c) argv, already rejected.
   *Recommendation: (a), and measure compaction on a long session later.*
2. **Automation sessions stay open after their turn.** Each run leaves an interactive Claude session
   running (Ready). Every 15 minutes that is a new session. Options: (a) leave it to the user;
   (b) close the previous run's session at the next firing when it is Ready and nothing needs you;
   (c) a per-automation "close when the first turn ends". *Recommendation: (c), defaulting on.*
3. **"Still active" for overlapping runs.** I read it as busy: working, waiting for an approval or
   answer, or not yet past its first prompt. An idle previous session doesn't block the next run.
   Options: (a) that; (b) any previous session still open. *Recommendation: (a); with Q2 (c), (b)
   would rarely differ.*
4. **Native Claude skills through `--plugin-dir`.** Claude 2.1.308 loads a plugin folder for that
   session only. Chorus could build a wrapper plugin in its own data folder (`.claude-plugin/plugin.json`
   plus the Bot's skills) per launch; skills would appear as `<plugin>:<skill>`. Not built or tried.
   *Recommendation: try it after this merges, behind the same carrier table.*
5. **Codex automations.** Codex's positional prompt with Chorus's `--remote` TUI isn't verified, and
   it would race the bridge's injection: the TUI would submit the prompt before the bridge (polling
   every 500 ms) injects the instructions, so the first turn would run without them. Options:
   (a) keep automations to Claude Code; (b) for Codex, have the bridge send the first prompt over the
   app-server after injecting (`turn/start`), which needs its own verification. *Recommendation: (a)
   for now, (b) as a follow-up.*

## 10. Risks and known issues

- **Merging with P7: done** on `evening/p6-p8` (§12). P8's migration became `0020`, P7's `-n <name>`
  goes before `p8.args` (so the automation's positional prompt stays last), and `bindings.ts` was
  regenerated.
- **Codex bridge change** (`codex_bridge.rs` is P6's): instructions are injected when Codex's TUI
  already has the thread loaded, before the bridge's own `thread/resume`, and again after that resume
  if still pending (a no-op once given). Injecting materialises a new thread's rollout, so the
  resume may now succeed before the first prompt rather than after it. Seen working for new and
  resumed Codex sessions; not stress-tested.
- An automation's prompt is in argv and `runs.args` (scrubbed), as the plan allows.
- A Bot chip in New session sets the Bot for the session; there is no way to clear it except choosing
  another chip or closing the dialog.
- The Bot editor doesn't edit provider or key (kept as saved).
- New session's chip (×) archives a Bot, as the Bots screen does (restore it there); a failed
  archive is reported. Sessions keep their copy either way.
- `bindings.ts` names `Document` and `Schedule` types; `index.ts` exports `Document` as
  `ChorusDocument` to avoid the DOM global.

## 11. How to review

Read in this order: `0019_p8_bots_notes.sql`; `chorus-core` `instructions.rs`, `schedule.rs`,
`documents.rs`; `src-tauri/src/instructions.rs`, then the `sessions/mod.rs` and `codex_bridge.rs`
diffs; `bots.rs`, `notes.rs`; `automations.rs` (`fire`, `tick`, `stage_prompt`/`staged`/`launched`);
`documents.rs`; then the Vue screens.

Manual test (a dev instance with its own identifier; scratch folders outside any repository; Claude
trusted in them):

1. Settings → Projects: two projects, A and B, with scratch folders.
2. Notes: a note in A, "If asked for the canary word, answer only 'heliotrope'", pinned; a second,
   untagged. Start a Claude session in each folder; ask each for the canary word: A answers
   heliotrope, B doesn't. Session facts list the notes each run got.
3. Edit the note's word, Restart A, ask again: the new word.
4. Bots: New Bot (Claude, instructions with a bot word), Start session, Start: the session's facts
   show the Bot; ask for the bot word. Edit the Bot's word, ask the same session (old word), start
   another from the Bot (new word).
5. A Codex Bot (Codex 0.163.0) with a bot word: start it, ask: the word, and the Codex command lines
   don't contain it.
6. Ask a session to write `_docs/PLAN-test.md` starting with `# Test plan`: Documents lists it, linked
   to the session; delete the file: it shows as missing.
7. Automations: New automation (that Bot, every 15 minutes, folder A, a one-sentence prompt); Run now:
   a session starts; its prompt history shows the prompt once, source automation. Wait for the
   scheduled time: another run. Pause it. Make a daily one due in two minutes, quit Chorus, reopen
   after the time: a "Missed: Chorus was closed." row.
8. Try a note containing a key-shaped string: refused.

## 12. Follow-up on the integration branch (2026-10-24)

Checked again after merging with P7 (`evening/p6-p8`), in an isolated instance:
- An automation's Run now created a Claude Code session whose single turn was recorded with source
  `automation`; it answered with both a global note's and its Bot's canary words, with P7's `-n
  <name>` placed before P8's arguments, and the session's fleet address was its name.
- **Documents now include Codex.** Codex's `fileChange` items list their files under `changes`, so
  the recorded target was empty. The first changed file now names the call, and `fileChange` counts as
  a writing tool. Seen with Codex 0.163.0: a plan written with `apply_patch` appeared in Documents,
  linked to its session. A file written by a shell command (Codex's `pwsh`, Claude's `Bash`) has no
  recorded path and is still not found, for any agent; only the first file of a multi-file patch is.
- **Automation prompts follow `--`.** The positional prompt now comes after the end-of-options
  marker (checked on Claude Code 2.1.308), so the refusal of prompts starting with "-" was removed and
  a Markdown list works; single-word prompts are still refused. Seen: "- Reply with only the canary
  word." ran once with source `automation`.
- **Reviewed:** the migration keeps every profile (rename plus defaulted columns), the instruction
  file is scrubbed with every vault key and the built-in patterns before it is written and is deleted
  at run end and start-up, and a long closure records one "missed" row per automation, not one per
  slot.
- **Skills, seen with an agent:** a Bot listing a skill folder (`SKILL.md` with name and description)
  gave Claude Code the index; asked for what the skill covers, it read that `SKILL.md` and answered
  from it.
- **Size cap, seen in the app:** six notes of about 5 KiB each gave a 20,355-byte instruction file
  with five notes (the pinned one first) and "2 more left out: over the size limit" in session facts.
- **Code review fixes** (commit "P7/P8: fixes from a code review of the merged branch"): Codex
  instructions on a resumed run whose thread wasn't loaded at the first poll; per-run instruction
  files so a slow-exiting run can't delete a Restart's file; a Bot only attaches to its own agent;
  Run now and the scheduler fire one at a time; no instructions warning on plain shells; a drive root
  accepted as a folder. Seen: Codex answered a changed note after Restart; a Claude session restarted
  at once kept its instructions with one file on disk; a Claude Bot didn't attach to Codex.
- **Second code review fixes:** a long automation name keeps room for " #N"; the scheduler re-reads
  an automation before acting and won't overwrite an edit or pause made meanwhile; a prompt that
  can't be tied to its run is unstaged so a restart can't repeat the task; skill descriptions are
  bounded and YAML block descriptions read; one file is one document whatever its path spelling.
  Left for Matthew: an idle automation session isn't closed (decision 2).
- **Through the screens' own forms:** a note saved, refused with a key in it (text kept) and pinned
  first; a Codex Bot made, a bad skill folder refused, Start session opened New session filled from
  it; a weekday automation saved with a "- " prompt, resumed (next Monday 09:30) and paused. Fixed on
  the way: deleting a note or an automation now asks first, and a Bot chip's × archives (restorable)
  instead of deleting.
- **Exit check through the screens on the merged build:** Bots → Start session → New session → Start
  made a session with the Bot's options (Fast, Manual, haiku), its instructions given, and Claude
  answered with the Bot's canary word.
- **Third and front-end reviews:** New session says when an agent change or an archive leaves the
  chosen Bot out; a failed launch's instruction file is swept at the next launch; the Bot editor's
  Tags field no longer drops the first tag when clicked; session facts don't show the previous
  session's Bot; Archive/Restore failures, a failed run-history load and an empty time of day are
  said visibly; repeated polling failures are said once.
