# Decisions: Chorus 2.0

Written 2026-10-25 in the kickoff session, from Matthew's answers. Phases: P12 · Routing inspector,
then P14 · More agents and ship.

## How to use this file
Read this before asking anything. If it answers your question, follow it.
If it doesn't, apply "Defaults when unsure", log the choice in assumptions.md, and keep going.

## Scope

### This run, and the repository's own rules
- **This run is Matthew's explicit resumption of P12 and of the part of P14 listed below** (the
  resumption D102 waits for). `_docs/PLAN.md`, `_docs/HANDOFF-COMMON.md` and the P14 hand-off still
  say "on hold": for this run, this file wins.
- `CLAUDE.md` applies, with three exceptions for this run:
  1. **A phase passes when every check passes.** Builders can't launch the app, so the real-app
     check is Matthew's, at the milestone review after each phase. The planner ends each `spec.md`
     with a section `## Check in the app`: numbered steps Matthew follows, each with what he should
     see, and any step that costs money marked with its cost.
  2. **Don't edit anything under `_docs/`**, including PLAN, DECISIONS, LESSONS, OPEN-QUESTIONS,
     SCOPE and the hand-offs. Another branch (`p15/loop`) already uses decision numbers D97 to D103,
     so numbers written here would collide. Instead, each phase's last task writes
     `.chorus/run/phases/<phase folder>/proposals.md`. It holds proposed decisions and lessons,
     unnumbered and written in the style of DECISIONS.md and LESSONS.md, plus the PLAN status line
     the phase earns and any OPEN-QUESTIONS it resolves. Matthew numbers and moves them.
  3. **No new SQLite migration.** `p15/loop` holds 0023 and 0024, which Matthew's installed app
     already uses. Keep new state in the existing `settings` table (`crate::store::settings`) or in
     memory.
- Builders work only in the worktree. They can't read Chorus 1 (`../Chorus`), so what they need
  from it is written below. The pure routing cores are already ported to `chorus_core::routing`,
  with Chorus 1's tests and constants.

### P12 · Routing inspector: what exists
Routing tiers for opencode on OpenRouter were brought forward into P4 (D57) and work today.
- New session → opencode → an OpenRouter provider and a model shows Budget, Balanced, Fast, Nitro
  and OpenRouter default (`src/views/newSession/RoutingTiers.vue`).
- Budget, Balanced and Fast rank the model's endpoints locally and send the top `1 +
  fallbackCount` as `provider.order`, with `allow_fallbacks: false`, `require_parameters: true`,
  `quantizations` and `data_collection: "deny"`. On opencode 2.x this goes in the model's request
  `body`.
- Nitro sends `<model>:nitro`, and OpenRouter picks the fastest endpoint for each request. Its only
  provider preference is `data_collection: "deny"`.
- Refresh (`routing_refresh`) fetches the endpoints, runs two free account checks, ranks, tests
  prompt caching (at most $0.05) and ranks again. Each model gets a one-minute cooldown and one
  Refresh at a time (`Routing` in `src-tauri/src/routing/mod.rs`).
- The observer (`start_observer`) refetches the endpoints of every model with a stored snapshot
  every 30 minutes, the first time 2 minutes after start, with the provider chosen through
  `routing_watch`. It records no status.
- `settings()` reads the stored `routing_settings`, but nothing writes them yet.
- A launch keeps its selection with the session, and Restart reuses it.

### P12 · What to build
A new Settings section, **Routing** (key `routing`, icon `shuffle`, placed after Providers in
`SettingsView.vue`), with these parts, top to bottom:

1. **Inspect.** Inputs:
   - an OpenRouter provider (only `kind = openrouter`);
   - a model (free text, an OpenRouter slug, with the models that have stored routing data offered
     as suggestions);
   - an effort (none, or Chorus's scale, sent as opencode's variant through the existing
     `effort_variant`, because effort decides which endpoints qualify).

   It shows what `routing_tiers` returns for those inputs:
   - a card each for Budget, Balanced and Fast: primary endpoint, provider, speed, price, latency,
     uptime, fallbacks and the rationale;
   - a Nitro card: the likely endpoint as an estimate, with Chorus 1's caveats (actual routing may
     differ; requests may be billed at a provider's priority-tier price; the account's guardrails
     still apply);
   - an OpenRouter default card;
   - the warnings, the registry line, and the table of every candidate with its exclusion reasons.

   It also has the Refresh button, using the existing `routing_refresh`, its cost text, cooldown
   and notes. The inspector never chooses a tier for a launch.
2. **Data collection.** A switch, off by default, that saves at once. Text is under Wording.
3. **Ranking settings.** Folded by default. Every field of `RoutingSettings` except
   `dataCollection`, grouped as:
   - **Every tier:** minimum uptime over a day, uptime to come back after a dip, minimum uptime
     over the last 5 minutes, endpoints that don't state their precision, speed samples averaged,
     samples before speed history counts, keep speed history for, a ranking needs a Refresh after,
     scores this close count as a tie, fallback endpoints after the first.
   - **Budget:** weight on speed, speed floor, share of the median speed.
   - **Balanced:** weight on speed.
   - **Fast:** weight on speed.
   - **Nitro:** no settings; a line saying why (Wording).

   Each field shows its default value beside it. Edits are a draft with **Save** and **Cancel**,
   plus **Restore defaults**, which deletes the stored key. Save sends the whole object, which Rust
   checks with serde and then `RoutingSettings::validate`. A refused Save keeps the draft and shows
   the reason using the field's label. Saved settings apply to the next ranking: the inspector,
   New session and Refresh. They never change a running session or a stored selection.
4. **Background observation.**
   - Off/on, with the provider whose key it uses; choosing one is the consent, as now (keep
     `routing_watch`).
   - Status: when the last check ran and its outcome; when the next one runs.
   - The models it follows, each with its last update, sample count and **Stop observing** /
     **Observe again**. Stopping keeps the model's data and only removes it from the observer's
     list; stopped slugs are kept in a new settings key.
   - The status lives in memory (in `Routing`), so after a restart it reads "No check since Chorus
     started".
   - Outcomes to tell apart: models updated (with a count); nothing due; no usable provider (with
     `routing_provider`'s reason); OpenRouter refused the key; rate-limited; other failures (with
     the message). A refused key or a rate limit stops the tick, as now.
5. **History.**
   - **Speed history** for the inspected model. One row per endpoint tag from the stored
     observations: samples, newest sample's age, median speed (tok/s), median latency (s), lowest
     1-day uptime, and a flag when the newest sample's status isn't 0. A table, no chart. Rows
     follow the ranking's candidate order, then any other tags by name. Compute it in core, as a
     pure function with tests.
   - **Refresh log.** The last 50 Refreshes, newest first, kept in a new settings key, for every
     model. Each entry has: when, model, provider name, result ("Done" or the failure's message),
     endpoints tested, spent and estimate in USD (the `RefreshOutcome` figures). Log every Refresh
     that got past the checks made before any request (provider usable, cooldown, slot, key),
     failures included. If the stored log can't be read, leave it alone, don't log, and show "The
     Refresh log couldn't be read."
6. **Anthropic and OpenAI models are refused by routing.** A model whose author part is
   `anthropic` or `openai` is refused, with the reason under Wording. The author part is the text
   before the first `/`, without a leading `~`, compared case-insensitively. This applies to:
   - `routing_tiers` and `routing_refresh`;
   - `for_launch`, so New session's tiers refuse them. The launch can still run with OpenRouter
     default, which sends no routing;
   - the inspector's new commands;
   - the observer, which skips them.

   Sessions that already have a stored selection are left alone. Put the check in core
   (`chorus_core::routing`) with tests.
7. **Tighter bounds in `RoutingSettings::validate`**, with tests, so every input is bounded:
   `smoothingWindow` at most 50, `observationMaxAgeDays` at most 30, `snapshotMaxAgeMinutes` at
   most 1440, `fallbackCount` at most 5, `tieRatio` at most 2, `budgetMinTps` at most 10,000. The
   existing rules stay.

The Profile selector (Chorus 1's Interactive / Team helper) isn't built: Team sessions were
discarded. Ranking always uses `RoutingProfileId::Interactive`.

### P12 · Check in the app (the planner copies these into spec.md, with any others it needs)
1. Settings → Routing opens, and the inspected model shows the same tiers as New session.
2. A saved ranking change moves New session's tiers for the same model; Restore defaults moves
   them back; an invalid value is refused with its reason.
3. A Refresh on a cheap model (for example `deepseek/deepseek-v4.1-flash`) appears in the log, with
   the spend OpenRouter reported. Costs at most $0.05.
4. With observation on, the status shows a check about 2 minutes after start. A stopped model is
   skipped by the next check.
5. `anthropic/…` and `openai/…` models are refused, with the reason, in the inspector and in New
   session.
6. One opencode turn on Balanced and one on Nitro, on the cheap model. Session facts show the
   routing, and OpenRouter's activity page shows the provider used. A few cents.

### P14 · What to build
1. **An install hint for an agent that isn't installed.**
   - Expose it on `AgentInfo` (for example `installHint`), set only when detection found nothing
     (`found` is false and no version was read). The "opencode is too old" message already says
     how to update.
   - Show it under "Not found" in Settings → Agents, and in the title of New session's disabled
     agent tile.
   - The text lives in core, per agent, with a test:
     - opencode: `npm install -g @opencode/cli`;
     - Gemini CLI: `npm install -g @google/gemini-cli`;
     - Aider: `uv tool install --python 3.12 aider-chat`, plus a note that it doesn't start on
       Python 3.13 or later (L47);
     - Claude Code, Codex and Grok: no command. The repository records none, and builders can't
       check the makers' sites, so say to install it from its maker's instructions;
     - Shell: none.
2. **Gemini CLI's auto-update, stated on screen (Q27, option a).** Chorus can't turn it off for
   one launch (L46). Expose a per-agent note on `AgentInfo` (for example `autoUpdateNote`), from
   core. Only Gemini CLI has one; every other agent's launch already switches updates off. Show it
   in Settings → Agents on Gemini's row, in the warning tone, and in New session's summary when
   Gemini CLI is chosen.
3. **Settings → Agents' sentence (Q24, option b).** Replace "Versions it hasn't checked are tried
   and reported, never refused." with "Versions Chorus hasn't checked are tried and reported; one
   it can't drive says why."
4. Its `proposals.md` records Q24 and Q27 as answered this way, and PLAN's P14 row as "partly
   built", with what remains (the out-of-scope list below).

### P14 · Check in the app
1. Settings → Agents shows the new sentence, and Gemini CLI's update note.
2. New session with Gemini CLI chosen shows the update note in its summary.
3. The install hint, if an agent is missing on the machine. If all are installed, the tests are
   the proof, and the step says so.

## Stack and conventions
- The stack is locked (`CLAUDE.md`). **No new dependency of any kind.** Vue 3 + TypeScript +
  Pinia in `src/`; Rust in `src-tauri/` (IO and wiring) and `crates/chorus-core/` (pure logic, no
  IO, no clock: pass `now_ms` in).
- **IPC.** Add each new command to the list in `src-tauri/src/ipc.rs`; `build.rs` grants it to the
  main window (D92), so don't edit `src-tauri/capabilities/*.json`. After changing any command,
  event or IPC type, run `npm run bindings` and include `src/ipc/bindings.ts` in that task's
  claims. Never edit it by hand: `bindings_are_current` fails while it's stale.
- **Validate at the Rust boundary.** The provider exists and is OpenRouter (and has its own key
  where a request is made); the model passes `is_model_slug`, doesn't end in `:nitro`, and isn't
  Anthropic or OpenAI; settings pass serde (`deny_unknown_fields`) and `validate`; list sizes are
  bounded.
- **Storage.** Use the `settings` table through `crate::store::settings`. Existing routing keys:
  `routing_settings`, `routing_watch_provider` and `routing_launch_choices`. New keys start with
  `routing_`. A stored value that can't be read is shown as a problem and is never overwritten
  except by an explicit user action (Save, Restore defaults, Stop/Observe). That's the pattern of
  `remember()`.
- **Patterns to copy:** `src-tauri/src/routing/mod.rs` and `commands.rs` (service and commands),
  `src/views/newSession/RoutingTiers.vue` (the tier display and formatting), and
  `src/views/settings/NarratorSettings.vue` (a settings section with a provider choice).
- **The UI's logic goes in plain TypeScript modules with Vitest tests** (for example
  `src/views/settings/routingText.ts`): labels, formatting, outcome text, label mapping for
  refusals. `.vue` files only render. You may move `RoutingTiers.vue`'s formatting helpers
  (`tps`, `price`, `pct`, `secs`, `why`, `details`) into such a module and import them from both
  views, provided New session shows exactly what it shows today.
- **Hot files.** Keep edits to `crates/chorus-core/src/agent.rs`, `src-tauri/src/sessions/mod.rs`
  and `src/views/newSession/NewSessionForm.vue` to the lines the task needs.
- Comments and naming match the surrounding code. LF line endings. No local paths, keys or
  machine-specific notes in committed files: the repository is public.
- Don't run git. Chorus commits each task.

## Testing
- Every acceptance criterion is proved by a test the checks run:
  - Rust, through `npm run test:rust` (`cargo test --workspace`);
  - Vitest, through `npm test`: `src/**/*.test.ts` in a `node` environment, with no DOM and no
    component mounting, so UI behaviour is tested through the plain TypeScript modules.
- The checks, in the gate's order: `npm install --prefer-offline --no-audit --no-fund`,
  `npm run bindings`, `npm run typecheck`, `npm test`, `npm run test:rust`, `npm run lint:rust`
  (clippy, `-D warnings`) and `npm run grep:secrets`. These are the only commands a builder can
  run. The first Rust build in a fresh worktree is slow.
- **No test makes a network request**, starts an agent CLI, or reads Matthew's data folders. Use
  `Store::in_memory()` and the fixtures in `crates/chorus-core/src/routing/fixtures/`. Where an IO
  path decides something (the observer's outcome, what a Refresh logs), split the decision into a
  pure function and test that.
- Never delete, skip or weaken an existing test. If an existing test's expectation must change
  because this file decides so (for example a new refusal), change only that assertion, and say
  why in the test.
- Fake keys only, and never print one.

## Wording
- **Voice:** match the app's current text. Plain sentences in sentence case, with British spelling
  in text people read ("colour", "centre"); code identifiers and CSS stay as they are. No
  "please", no exclamation marks. Short.
- **Fail visible:** every disabled control has a title saying why, and every failure says what
  happened and, where there is one, what to do. Unknown is "unknown" or "—", never zero.
- **Numbers:**
  - money under $1 as `$0.0000`, otherwise `$0.00`;
  - speeds as `tok/s`, latency in seconds (`0.42 s`), uptime as `99.62%`, prices as RoutingTiers
    shows them (`$0.123/M`);
  - times relative ("12 min ago", "in 18 min", "just now").
- **Names, as the UI already uses them:** Refresh, Budget, Balanced, Fast, Nitro, OpenRouter
  default, Look again, Settings → Routing, Settings → Agents, New session. The agents are Claude
  Code, Codex, opencode, Grok, Gemini CLI, Aider, Shell.
- **Texts to use:**
  - Routing's intro: "How OpenRouter's endpoints rank for a model, for opencode's Budget, Balanced,
    Fast and Nitro tiers. Launches use a tier only when you choose one in New session."
  - Data collection switch: "Allow providers that may store or train on prompts". Below it: "Off,
    every tier asks OpenRouter only for providers that don't collect prompt data. On, those
    providers can be ranked and chosen too."
  - Nitro under Ranking settings: "Nitro has nothing to set: Chorus adds :nitro to the model and
    OpenRouter sends each request to its fastest endpoint. Data collection still applies."
  - OpenRouter default card: "Chorus sends no routing: OpenRouter picks the provider for each
    request. The data-collection setting isn't sent."
  - Anthropic/OpenAI refusal: "Chorus doesn't route Anthropic or OpenAI models through OpenRouter.
    Run them in Claude Code or Codex on your own sign-in."
  - Observation, off: "Off. Choose a provider to record endpoint numbers every 30 minutes with its
    key. It's free: no prompts or code are sent."
  - Observation, on: "On, with <provider name>." Then "Last check 12 min ago: 3 models updated." (or
    "nothing was due", "OpenRouter refused the key", "OpenRouter rate-limited the check; the next
    check tries again", or "failed: <message>"), and "Next check in 18 min." Before any check: "No
    check since Chorus started."
  - Speed history help: "From the endpoint numbers stored for this model, up to the history
    setting's age."
  - Keep-speed-history help: "Shorter keeps less: older samples are dropped at the next update."
  - Gemini's update note: "Gemini CLI updates itself: it has no switch Chorus can set for one
    launch (checked on 0.64.0), so a newer version can appear at the next launch."
- **Ranking setting labels** (default in brackets):

  | Field | Label |
  |---|---|
  | `minUptimePct` | Minimum uptime over a day (99.5%) |
  | `readmitUptimePct` | Uptime to come back after a dip (99.6%) |
  | `outageGuard5mPct` | Minimum uptime over the last 5 minutes (95%) |
  | `unknownQuantPolicy` | Endpoints that don't state their precision: "Only the model's maker at full precision, or ones verified" (`first-party-and-verified`, default), or "Never" (`strict`) |
  | `smoothingWindow` | Speed samples averaged (6) |
  | `stableMinObservations` | Samples before speed history counts (3) |
  | `observationMaxAgeDays` | Keep speed history for (7 days) |
  | `snapshotMaxAgeMinutes` | A ranking needs a Refresh after (60 min) |
  | `tieRatio` | Scores this close count as a tie (1.01) |
  | `fallbackCount` | Fallback endpoints after the first (2) |
  | `tierWeights.budget` | Budget: weight on speed, the rest is price (0.3) |
  | `budgetMinTps` | Budget: speed floor (30 tok/s) |
  | `budgetMedianFraction` | Budget: or this share of the median speed, whichever is higher (0.5) |
  | `tierWeights.balanced` | Balanced: weight on speed (0.5) |
  | `tierWeights.fast` | Fast: weight on speed (1.0) |

## Defaults when unsure
- Follow the nearest existing pattern in the files named above. Prefer the smaller change.
- Pure logic goes in `chorus-core` with tests; IO in `src-tauri`; UI logic in a plain TypeScript
  module with tests.
- A choice between two reasonable layouts or names: take the one closest to `RoutingTiers.vue` or
  `NarratorSettings.vue`, and log it.
- A value that can't be read, fetched or computed: show it as unknown, with the reason.
- Something that can only be proved in the app or over the network: prove its logic with a unit
  test, and add a step to the spec's `## Check in the app`.
- Command names follow the existing `routing_*` style (for example `routing_settings`,
  `routing_settings_save`, `routing_settings_reset`, `routing_observer`, `routing_observe_model`,
  `routing_history`, `routing_refresh_log`).
- Times are milliseconds since the epoch (`now_ms()`), formatted in the UI.
- If a task is bigger than one session, finish a coherent part with its tests passing, and log what
  is left.

## Always ask
Ask with class blocking, and wait, before:
- adding or upgrading a dependency (npm or cargo), or changing `package.json`, `Cargo.toml` or a
  lockfile;
- adding a migration, or changing the database schema;
- changing what a launch sends to an agent, opencode or OpenRouter: arguments, environment, opencode
  configuration, the request body or `provider` preferences. The one exception is the
  Anthropic/OpenAI refusal decided above;
- adding any code path that sends a network request or can spend money (the inspector reuses the
  existing Refresh, which is allowed);
- touching keys, the vault, the scrubber, `secret-patterns.json`, hook tokens or capability tokens;
- deleting or rewriting stored data: sessions, history, routing data, or a stored setting the user
  didn't just change;
- editing `_docs/`, `CLAUDE.md`, `.github/`, `src-tauri/tauri*.conf.json`,
  `src-tauri/capabilities/`, or anything under `src-tauri/windows/`;
- changing what New session shows or does, beyond the refusal decided above and P14's two
  additions;
- doing anything this file or `CLAUDE.md` forbids, or that contradicts a recorded decision (D-number).

## Out of scope for this run
- **P14:**
  - complete Gemini CLI and Aider turns, and their option and resume checks (they need keys or
    logins);
  - DeepSeek Harness (spike S9, Q21: an install and a key);
  - code signing (Q25);
  - the updater (Q26: a new dependency);
  - dropping the "2" from the installed name (D51: waits for Chorus 1 to be retired);
  - installer changes, CI workflow changes, installing or updating any CLI.
- **P12:**
  - changing the ranker's algorithm or Chorus 1's measured constants (only the settings are
    exposed, plus the new upper bounds);
  - re-ranking or changing running sessions;
  - routing for agents other than opencode;
  - refusing Anthropic or OpenAI models outside routing (plain OpenRouter launches with no tier);
  - the Team helper profile;
  - charts;
  - Jev for routing (D40).
- **Everything else:** P10, P11, P13 and P15 work; Raw draft transfer; OPEN-QUESTIONS items not named
  here (Q18, Q21 to Q23, Q25, Q26).
- **Builders never:** run the app, an agent CLI, git, or any command other than the checks; make
  network requests; spend money; or edit files outside their task's claims.
