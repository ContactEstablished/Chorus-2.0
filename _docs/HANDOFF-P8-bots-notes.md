# Hand-off plan: P8 — Bots, Automations, Notes, Documents

**Read [HANDOFF-COMMON.md](HANDOFF-COMMON.md) first.** It holds the isolation, safety, shared-file and
review-file rules this plan relies on. Label `p8`, Vite port 1428, review file
`_docs/HANDOFF-P8-REVIEW.md`. **P8 also owns the prompt-delivery contract** (HANDOFF-COMMON §4), which
P13 depends on, so build it early (Task 5) and keep its signature exactly as written.

## 1. Goal

PLAN P8 and SCOPE:
- **Bots** grow out of launch profiles and presets. A Bot is a reusable agent, provider, model,
  effort, permissions and instructions, with tags and provider-neutral skill folders. That is where
  Chorus 1's skills framework lands (D31). Starting one creates a session. **Editing one never
  changes running sessions.**
- **Automations** run a Bot on a schedule and create ordinary sessions. GitHub triggers come later,
  by polling with `gh` (D34), and are out of scope here.
- **Notes** are project-tagged (or global) context injected into agent instructions (D17). Untagged
  notes apply everywhere. Pinned notes come first.
- **Documents** list the plans, specs and reports that sessions produced, linked back to the session.
  A document inherits its session's project.

**Exit** (PLAN gives none for P8; this is the one to meet): these must be seen working in your `p8`
instance.
- A Bot started from the Bots screen launches with its options, and its instructions are **observed
  to apply** in Claude Code and Codex (a canary, §8).
- A project note reaches a session of that project, and doesn't reach one in another project.
- An automation fires on schedule and creates a session whose first prompt is recorded once, with
  source `automation`.
- A Markdown plan written by a session appears in Documents, linked to that session.

## 2. Out of scope

- GitHub triggers (D34, later).
- The composer, queueing and next steps (P6).
- Writing to repository files to deliver instructions or skills. **Chorus never writes `CLAUDE.md`,
  `AGENTS.md`, `.claude/`, `.codex/`, `opencode.json` or any global agent config** (Chorus 1 rule D49,
  ARCHITECTURE §5, LESSONS L1). Instructions and skills travel through per-launch files in Chorus's
  own data folder, plus flags or environment variables.
- Importing anything from Chorus 1 (D50).

## 3. Ground facts (verified 2026-10-20; re-check before relying on them)

**Chorus 1** (`../Chorus`):
- **The skills framework was planned, never built.** The design is in `docs/Plan.md` §11, lines
  292–330 (`skills/<id>/skill.yaml + instructions.md + scripts/ + prompts/`, a `skills` table and a
  `SkillManager`). The code has only a `skills: boolean` capability flag that nothing reads. Treat the
  plan as an idea, not proven behaviour.
- **Instruction injection was built** (Chorus 1 D148; `src/main/adapters/instructionsCore.ts`,
  `types.ts:257-276`):
  - **Claude:** `--append-system-prompt-file <file>`, not `--append-system-prompt <text>`, because
    argv is readable. The file is per session, under app data. It is wiped at start and deleted on
    every exit path. If writing it fails, the session launches without instructions, and says so.
  - **Codex:** `-c developer_instructions="<one line>"`.
    - The value **must be one line**: a raw newline makes Codex drop the whole override silently.
    - The **last** `-c developer_instructions` wins, so every part must go in one token.
    - It is escaped as a TOML basic string (`mcpConfigCore.ts:40`).
  - **opencode:** deliberately none. The `instructions` config key was never measured, and opencode
    silently ignores keys it rejects (L5).
  - **Grok:** none.
- No scheduler and no documents feature exist in Chorus 1. Every `setInterval` there is a poller.

**Chorus 2.0 today**
- **Profiles:**
  - Migration `0004_launch.sql` has the table `launch_profiles(id, name UNIQUE, agent, options JSON,
    provider_id, credential_id FK → credentials ON DELETE SET NULL, credential_env, created_at,
    updated_at)`. It has no CHECK on `agent`.
  - The store is `store/profiles.rs`. The commands are `sessions/profiles.rs` (`profiles_list`,
    `profile_save`, `profile_delete`), validated through `check_settings` (`sessions/mod.rs` ~787).
  - The front end is `stores/profiles.ts`. `NewSessionForm.vue` shows profile chips (~388), fills
    the form from one (~306) and has save-as-profile (~516).
- **Launch shape:**
  - `chorus-core/src/agent.rs` has `LaunchOptions { effort, permission, model }`, `Effort` and
    `Permission`. `Spec::resolve` refuses options an agent doesn't declare, and `option_args` builds
    argv. A conformance test (`declarations_match_the_arguments`) loops over every agent.
  - `launch.rs` holds the presets (Solo, Pair, Workbench, Swarm) and `plan()`, kept separate from
    profiles.
  - `CreateSession` and `LaunchRequest` are in `sessions/mod.rs` (~183–219). The struct literal in
    `launch()` (~1508) has no `..Default`, so every new field must be added there too.
- **Sessions copy their options** onto their own row (`0002`), and `start_run` (`sessions/mod.rs`
  ~892–1012) reads that row. So Restart and boot restore never look at the profile, which already
  satisfies "editing a Bot never changes running sessions". There is **no `bot_id`** on sessions.
- **No instructions are passed to any agent today.**
  - `start_run` builds argv in this order: route args, hook args (Codex's are themselves `-c hooks.*`),
    `option_args`, conversation args.
  - Natural homes for a per-run instruction file are the session's `state_dir` (`data/state/<id>`) or
    beside `hooks/claude-session-<id>.json`.
  - opencode already gets JSON config through `chorus_core::opencode::config_content` in
    `OPENCODE_CONFIG_CONTENT`.
  - **`runs.args` stores argv as JSON** (`0001`), so anything put in argv is persisted and visible in
    the process list.
- **No scheduler.** `app::start_pulse` (`app.rs` ~61) emits a 1 s heartbeat. The background threads
  at setup (`lib.rs`) are sweep-keys, relaunch and detect-agents.
- **Front end:**
  - `Rail.vue` already lists Bots, Automations, Documents and Notes, and `ViewName` includes them.
  - `App.vue` (~83–110) renders `PhasePlaceholder.vue` for each. Replace those four tags with your
    screens.
- **History:** `prompts.source` is free text. Only `"hook"` and `"server"` are written today. The
  migration comment reserves values for "the composer, Telegram and automations". `tool_calls` holds
  each tool and its target (P3), so files a session wrote are already recorded.
- **Design:** DESIGN.md "Screens", plus `_assets/mockup-screens/glacier-{bots,automations,documents,notes}.png`:
  - **Bots:** cards with agent and model, description, behaviour tags ("Read-only", "Ask before DB
    writes"), a usage line ("Used by 3 sessions", "Triggered by automation"), Edit and Start session.
  - **Automations:** rows of name, bot · schedule · outcome, and last run ("last run ✓",
    "12 runs this week", "paused").
  - **Documents:** title, project chip, kind ("Plan", "Review report", "Spec", "Decision record") and
    edited time; "Documents inherit the tag of the session that created them".
  - **Notes:** project filter chips, "Untagged notes apply to every project", and a pinned badge.
- **Contention:** `NewSessionForm.vue` changed in five of the last eight P4 commits. P5 will probably
  touch `CreateSession.workspace` (attaching to a kept worktree) and add a migration.

## 4. Design

**Data, in one migration:**
- **Bots *are* the launch profiles, grown.** Rename `launch_profiles` to `bots` and add `description`,
  `instructions` (Markdown), `tags` (JSON array, cleaned by `chorus_core::tags`), `skills` (a JSON
  array of folder paths), `preset` (nullable: Solo, Pair, Workbench or Swarm) and `archived_at`. Keep
  `name UNIQUE`. Keep or rename the store and commands as `bots_*`, and make sure New session's chips
  keep working. *(Decision 1, if Matthew prefers a separate table.)*
- **`sessions.bot_id`:** nullable, with an FK `ON DELETE SET NULL`.
- **`sessions.instructions`:** the Bot's instructions **copied at create**, so later edits don't
  reach the session.
- `notes(id, project_id NULL with FK ON DELETE CASCADE, title, body, pinned, created_at, updated_at)`.
- `automations(id, name, bot_id FK, prompt, schedule JSON, cwd, project_id, enabled, next_run_at,
  created_at, updated_at)` and `automation_runs(id, automation_id, session_id NULL, due_at,
  started_at, outcome, note)`.
- `documents(id, session_id, project_id, path, kind, title, first_seen_at, last_seen_at, missing_at)`.
  The path is unique per project.

**Core (`chorus-core`, pure, with tests):**
- `instructions.rs`: composes a run's instruction text from the session's copied Bot instructions, the
  notes that apply (global, plus the session's project; pinned first, then the most recently updated)
  and the skill index (below). It is capped: say 24 KiB in total, oldest unpinned notes dropped first,
  with a visible "N notes left out" line in the text and in session facts. It renders **per agent**:
  Markdown for a file, and a single escaped line for Codex.
- An instructions capability on each agent `Spec`: `None`, `AppendSystemPromptFile`, `CodexConfig`,
  `OpencodeConfig` and so on, as verified. **Add it as a field with a default**, so P14's new agents
  and P7's Claude `-n` merge cleanly. Extend the conformance test.
- `schedule.rs`:
  - The kinds are every N minutes (N ≥ 15), daily at HH:MM, and on chosen weekdays at HH:MM, all in
    local time with the offset passed in.
  - `next_run(schedule, after, utc_offset)` handles DST by taking an offset per call.
  - Missed-run policy (decision 3).
- `documents.rs`: classifies a written path as a document or not, and as which kind. Markdown under
  `_docs/`, `docs/`, or at the top level, named like plan, spec, report, review, decision, findings,
  `ADR-*` and so on. It also extracts a title from the first `# heading`.

**Skills, provider-neutral (D31).** A skill is a folder holding `SKILL.md` (front matter `name` and
`description`). The Bot lists folders. At launch, the instruction text gets a short index: the name,
the description and the folder's absolute path, with "read SKILL.md before using it". Every agent can
follow that, and nothing is written into the repository or a global config. Native per-agent skill
loading is a follow-up only if a per-launch, non-repo, non-global mechanism is verified (decision 4).

**Back end (new `src-tauri` modules):** `bots/`, `notes/`, `automations/` (the scheduler thread), and
`documents/`. Plus `sessions/deliver.rs` (the shared contract) and the instruction-file writer.

## 5. Tasks, in order (commit after each)

1. **Verify instruction delivery per installed CLI, before writing code.**
   - For each agent, read `--help` and the docs, and record the version.
     - **Claude Code:** `--append-system-prompt-file`.
     - **Codex:** `developer_instructions`, and whether a file-based key exists (for example
       `model_instructions_file` or `experimental_instructions_file`) that would keep the text out of
       argv and `runs.args`.
     - **opencode 2.1.1:** the `instructions` config key (an array of file paths) through
       `OPENCODE_CONFIG_CONTENT`.
     - **Grok:** whatever it has.
   - Then prove each one with a **canary** in a scratch folder. Use the instruction "If asked for the
     canary word, answer only 'heliotrope'", then ask. Keep it to one tiny turn per agent: Claude Code
     and Codex on Matthew's subscription, and opencode on a `:free` model, or skip it if none works.
   - Write the results in `_verify/`, and summarise them in the review file. Anything unverified is
     declared `None`, with the reason shown in the UI.
2. **Migration, core types and the instructions capability.** The migration from §4,
   `instructions.rs`, the `Spec` field, and the conformance test.
3. **Notes.**
   - Store and commands (list with a project filter, save, delete, pin).
   - The Notes screen: project chips, pinned first, an editor, and "Untagged notes apply to every
     project".
4. **Instruction injection at run start.**
   - In `start_run`, compose the text with `instructions.rs` (one call), and deliver it per the
     verified mechanism.
   - Write the file to the session's state folder, scrubbed. Delete it when the run ends, and on boot
     for runs that have ended.
   - Notes are read fresh at each run start (decision 2).
   - Session facts show "Instructions: Bot + 3 notes" or "Not supported by Grok".
   - A failure to write the file launches without instructions and says so on the session (fail
     visible).
5. **The prompt-delivery contract** (HANDOFF-COMMON §4), exactly as specified: `sessions/deliver.rs`,
   the matching function in core, and one call in `events/recorder.rs`. Give it unit tests and a
   runtime check of one row per delivered prompt.
6. **Bots.**
   - The table rename and store, and the Bots screen (cards as in the mockup, with an editor for
     every field). Start session goes through the existing launch path, with the Bot's preset. A
     session's `bot_id` is set, and its instructions are copied.
   - The usage line on each card ("Used by N sessions", "Triggered by automation").
   - "Bot" in session facts.
   - New session's profile chips become Bot chips, with **the smallest edit to `NewSessionForm.vue`
     that does it** (rename the labels and the store; keep the layout).
7. **Documents.**
   - After each turn ends, scan that turn's `tool_calls` for file writes whose path is inside the
     session's working folder and that `documents.rs` accepts. Upsert a row, reading only the file's
     first heading. Hook this from the recorder's turn-end path with one call, or run it as a
     separate pass over new `tool_calls` rows (preferred: no recorder edit).
   - The Documents screen: project chips, kind, title, edited time, and a link to the session. Open
     the file with `tauri-plugin-opener` (approved), from Rust only, after checking the path is still
     inside the project. A missing file shows "missing", not an error.
8. **Automations.**
   - A scheduler thread wakes on the next due time (and on the minute, as a backstop). It creates the
     session through the normal launch path, with the automation's Bot, folder and project.
   - It delivers the first prompt with `deliver_prompt(…, PromptSource::Automation)` once the agent
     is ready. Find a structured "ready" signal per agent, or pass the prompt at launch if the CLI
     takes one (for example a positional prompt argument). If you pass it at launch, it goes into
     argv and `runs.args`; that's acceptable for prompts (they are persisted anyway), but scrub it.
     The run's first reported prompt must still be recorded **once**, with source `automation`.
   - Record an `automation_runs` row for each firing, with the outcome taken from how the session
     ended.
   - An automation whose Bot is archived, or whose folder is missing, fails visibly, and the run row
     says why.
   - The Automations screen: rows as in the mockup, "Run now", "Pause", and the history.
9. **Exit checks and review file** (§8).

## 6. Decisions for Matthew (ask before building past them)

1. **Are Bots the same table as launch profiles?**
   - (a) Rename and extend `launch_profiles`. Existing profiles become Bots, and New session's chips
     keep working with a one-word change.
   - (b) A separate `bots` table, with profiles kept as they are.
   - *Recommendation: (a). SCOPE says "saved launch profiles become Bots", and two near-identical
     concepts would confuse the launch form.*
2. **When do Notes apply to a running session?** The Bot's instructions are copied at create, so
   editing a Bot doesn't reach running sessions.
   - (a) Notes are read at each run start: a Restart picks up edited notes, but a running agent
     doesn't change.
   - (b) Notes are copied at create, like the Bot.
   - *Recommendation: (a). Notes are living context; facts show which notes the current run got.*
3. **Missed automation runs while Chorus was closed.**
   - (a) Skip them, and record them as "missed (Chorus was closed)".
   - (b) Run once at start-up to catch up.
   - *Recommendation: (a). A burst of sessions at start-up is surprising and could spend money; offer
     "Run now" on the row.*
4. **Skills.**
   - (a) The provider-neutral index in the instructions (§4) for every agent, now.
   - (b) Also native loading where a per-launch mechanism is verified.
   - *Recommendation: (a) now. (b) only after you report what each CLI offers, because native skill
     folders are usually repo- or home-relative, which this design forbids.*
5. **Codex instructions in argv**, if Task 1 finds no file-based key. The text would be in the process
   list and in `runs.args`.
   - (a) Accept it, scrubbed in `runs.args`.
   - (b) Turn instructions off for Codex.
   - *Recommendation: (a), and tell him, since notes aren't secrets but may be private. Say this in
     Settings → Agents.*

## 7. Notes on hot files

- `sessions/mod.rs`: new `CreateSession` fields (`bot_id`, `instructions`) with their literal in
  `launch()`, and one instructions call in `start_run`. Nothing else.
- `events/recorder.rs`: one call for "record once" (Task 5). Documents should not need it.
- `NewSessionForm.vue`: the chips only (Task 6).
- `App.vue`: replace the four `PhasePlaceholder` tags.
- `agent.rs`: the instructions field on `Spec`, with a default.

## 8. Verification

- **Unit tests:** instruction composition (order, cap, Codex single line, scrubbing), schedule
  `next_run` (DST change, weekdays, minimum interval), document classification, delivery matching and
  validation, and the extended conformance test.
- **Runtime, in your `p8` instance:**
  1. Canary instructions from a Bot apply in Claude Code and Codex. For opencode, record what you
     verified. For Grok and Shell, the UI says "not supported".
  2. A note tagged project A reaches a session in A (canary), and doesn't reach one in B.
  3. Edit a Bot while its session runs: the session is unchanged. A new session from it gets the edit.
  4. An automation on a 15-minute schedule (or "Run now", plus one real scheduled firing) creates a
     session. Its first prompt shows once in prompt history, with source `automation`.
  5. A session asked to write `_docs/PLAN-test.md` in a scratch repository shows it in Documents, and
     the link opens it.
  6. A planted fake key in a note never appears in the instruction file, `runs.args` or the DB
     (search them by hash or length, never by printing the key).

## 9. Files you will probably touch

New: `crates/chorus-core/src/{instructions,schedule,documents,deliver}.rs`,
`src-tauri/src/{bots,notes,automations,documents}/*`, `src-tauri/src/sessions/deliver.rs`,
`src/views/{BotsView,AutomationsView,DocumentsView,NotesView}.vue` (plus subcomponents),
`src/stores/{bots,notes,automations,documents}.ts`, one migration.
Hot (minimal): `sessions/mod.rs`, `events/recorder.rs`, `agent.rs`, `App.vue`, `NewSessionForm.vue`,
`SessionFacts.vue`, `lib.rs`, `ipc.rs`, `store/mod.rs`, `src-tauri/Cargo.toml` (opener plugin).
