# Hand-off plan: P14 — More agents and ship

**On hold until further notice (2026-10-25, D102).** P14 moves to the end of the roadmap, before P11.
Keep completed work. Matthew intends to have Chorus 2 handle the remaining work later as a
real-world test; do not resume implementation or acceptance checks until he explicitly says so.
This hold overrides the earlier parallel-work permission in HANDOFF-COMMON.md.

**Read [HANDOFF-COMMON.md](HANDOFF-COMMON.md) first.** It holds the isolation, safety, shared-file and
review-file rules this plan relies on. Label `p14`, Vite port 1434, review file
`_docs/HANDOFF-P14-REVIEW.md`.

## 1. Goal

PLAN P14 has six parts:
- **Gemini CLI**, **Aider** and **Ollama (via opencode)** adapters.
- Installer polish.
- A **code-signing decision**.
- An **update channel**.
- **Secret scanning in CI**.
- The step that drops "2" from the installed name once Chorus 1 is retired (D51).

The last two parts change how the app reaches people, and so do signing and updates, so this hand-off
is **adapters and CI built, shipping designed** (§2).

**Exit for this hand-off**, seen working:
1. Each new agent that Matthew lets you install launches from New session with every option it
   declares, and each option is **observed to apply** (PLAN P2's bar), not only passed.
2. An agent that isn't installed shows "not found" with an install hint, never a silent gap.
3. A GitHub Actions workflow, run locally with the same commands, passes on a clean checkout. It runs
   the typecheck, vitest, `cargo test`, clippy and the secret scan.
4. The review file includes the signing, update-channel and rename designs, as proposed decisions
   with options and costs.

## 2. Out of scope, or design only

- **Changing `productName`, the identifier or the install path.** Chorus 1 isn't retired. Design the
  "drop the 2" step (Task 8), and don't ship it.
- **Buying a certificate, or signing anything with a real key.** That is a design and decision
  (Task 9).
- **Turning on the updater.** Design it (Task 10). Wire `tauri-plugin-updater` only after Matthew
  approves the design, and disabled until he gives an endpoint and a key.
- **Pushing, or creating GitHub releases.** You write the workflow file; Matthew pushes it.
- Event channels for the new agents beyond what their CLI offers today. An agent without one is
  "Running" (D47), like Grok.

## 3. Ground facts (verified 2026-10-20; re-check before relying on them)

**None of the three CLIs is installed on this machine** (`where`, `pip show aider-chat`, `uv tool list`
and the Ollama install folder were all checked). Installing them changes Matthew's machine, so ask
first (decision 1). Chorus 1 never had these adapters either. Its notes (`../Chorus/docs/Plan.md`
~134–137) say:
- **Gemini CLI:** `GEMINI_API_KEY` or a Google login, and `-m`.
- **Aider:** "pure API-key workhorse", `--model provider/name`, `--openai-api-base`.
- **Ollama:** "local, zero-key, OpenAI-compatible".

**opencode is now 2.1.1** on this machine (npm `@opencode/cli`; the shim is `npm/opencode.cmd` →
`node_modules\@opencode\cli\bin\opencode.exe`). 2.0's adapter was verified against **1.19.3** from
`opencode-ai`. Ollama rides on opencode, so **re-verify the opencode adapter first** (Task 1).

**How an agent is defined** (about 25 coordinated edits):
- **`crates/chorus-core/src/agent.rs`:**
  - `enum AgentKind`, `ALL` (a fixed-size array, which changes), `as_str`, `display_name` and
    `spec()`.
  - A `Spec` table per agent: `verified` version, `carrier` (`Argv` or `ServerConfig`), `efforts` and
    `permissions` as `(level, args)` with defaults, `model` (`ModelArg::Flag` / `CodexConfig` /
    `ServerConfig`), and `resume` (`None` / `Assigned{create,resume}` / `Discovered{subcommand}` /
    `Server`).
  - The conformance test `declarations_match_the_arguments` loops over `ALL`, and so does
    `names_round_trip`.
- **`cli.rs`:** `parse_version`, and version-fit notes (an unknown version is tried and reported,
  L7). Add each CLI's version line to its test.
- **`shim.rs`:** parses npm `.cmd` shims to `.exe` or `.js` targets (L14). A Python entry point
  (Aider) is a new case; check what `pip` or `uv` installs on Windows (usually `aider.exe`, a launcher).
- **`env.rs`:** a `BASELINE` allow-list (L13). `ChildEnvInput.required` exists for per-agent names,
  but every caller passes `&[]` today. Gemini or Aider config variables and `OLLAMA_HOST` would be its
  first uses.
- **`launch.rs`:** `PARTNER_ORDER` is a fixed `[AgentKind; 4]` (Pair and Workbench partners). Decide
  where the new agents go.
- **`providers.rs`:** `route_support` (an exhaustive match) and `route` (ends in `unreachable!`).
- **`src-tauri/src/agents/mod.rs`:** `resolve` (an exhaustive match) holds the program, leading args
  and env additions, **including each agent's auto-update switch** (L7, L26).
  `conversation_exists` is used for resume.
- **`sessions/mod.rs`:** per-agent branches for hooks (Claude and Codex), the opencode server, the
  Codex originator, resume, and the recorder source.
- **`detect.rs`:** runs `--version` for every kind. Nothing else per kind.
- **Database: `0001_initial.sql:25` has `agent TEXT NOT NULL CHECK (agent IN ('shell', 'claude',
  'codex', 'opencode', 'grok'))`.** A new kind can't be stored until that CHECK changes, and SQLite
  can only change a CHECK by **rebuilding the table**. That is dangerous here:
  - `runs` references `sessions` with `ON DELETE CASCADE`. `session_tags` does too, and the history
    tables (`0005`) reference it without an action.
  - The migration runner (`src-tauri/src/store/mod.rs`) runs each migration **inside a transaction
    with `foreign_keys=ON`**, and `PRAGMA foreign_keys` cannot change inside a transaction.
  - So a plain "create new, copy, drop old, rename" would **cascade-delete every run** (or fail on
    the history FKs) when the old table is dropped.
  - Renaming the old table first is wrong too: SQLite 3.26+ re-points the other tables' FKs to the
    renamed table.
  - Decision 2 has the options. **Test any approach on a copy of a populated database** (yours, made
    by your `p14` instance), and run `PRAGMA foreign_key_check` and row counts before and after.
- **Front end:**
  - `Record<AgentKind, …>` maps break the typecheck until they are filled: `src/raw/labels.ts`
    `AGENT_LABELS`, `src/shell/status.ts` `AGENT_CHIPS`, and `NewSessionForm.vue` `LOGOS`, `PROGRAMS`
    and `SIGN_IN`.
  - Add these by hand: `NewSessionForm.vue` `AGENT_ORDER`, the icons in `src/assets/agents/*.png`
    (drawn as masks), `--agent-*` colours and `.agent-*` classes in `src/styles/tokens.css`.
  - `SessionView.vue` (~56) and `PromptHistory.vue` (~56) hard-code "shell/grok has no event channel".
    Replace that with a flag from `AgentInfo`, so new channel-less agents are covered.
  - `stores/agents.ts` and `AgentsSettings.vue` render from `AgentInfo` and need no change.
- **How opencode gets a model:** `chorus_core::opencode::config_content` builds `provider`, `model`
  and `agent.{build,plan}.{model,variant}` and the permission entry. That goes in
  `OPENCODE_CONFIG_CONTENT`.
  - `providers::route` emits an `openrouter` provider block today.
  - `route_support` refuses opencode on a gateway.
  - `agents/opencode.rs` `check_model` checks the model against the server's `/provider` list. Its
    `NotConnected` result could misreport a keyless local provider.
  - **So Ollama is a provider route for opencode, not a new `AgentKind`** (decision 3).

**Shipping today**
- **`tauri.conf.json`:** productName "Chorus 2", identifier `com.contactestablished.chorus2`, window
  title "Chorus", NSIS only, `installMode: currentUser`, WebView2 `downloadBootstrapper`. It has no
  updater and no signing settings.
- **`tauri.bundle.conf.json`:** builds `chorus-hook` and ships it as `externalBin` (L38).
- **S8** (`_docs/spikes/S8.md`) left three things for P14: signing, the update channel, and the
  wording of the uninstall "delete app data" checkbox. It also found that uninstall leaves
  `HKCU\Software\contactestablished\Chorus` behind.
- **D39** (Proposed): install per user into `%LOCALAPPDATA%\<productName>`. **D51** (Decided):
  "Chorus 2" until Chorus 1 is retired. The window title, `chorus.exe` and the data folders don't
  change. The install folder and the NSIS HKCU keys follow productName.
- **No `.github/` folder exists** in either repo. Chorus 1 is unsigned (`electron-builder.yml`
  ~109–111: SmartScreen warns, and a certificate "costs money").
- **Secret scan:** `scripts/secret-grep.mjs` scans `src`, `src-tauri`, `crates`, `scripts`, `_docs`
  and `_verify`. It never prints a match and exits 1 on any hit. It is in `npm run check`.
  `secret-patterns.json` has six patterns (anthropic, openrouter, openai-project, openai-classic,
  github, aws) and **no Google `AIza…` pattern**, which `GEMINI_API_KEY` needs.

## 4. Tasks, in order (commit after each)

1. **Re-verify opencode 2.1.1** before building on it.
   - In a scratch folder, with isolated XDG folders (L5), check `serve`, `attach`, `OPENCODE_CONFIG_CONTENT`,
     the variant read-back (L35), `prompt_async`, the SSE event names the P3 bridge uses,
     `/provider`, and `OPENCODE_DISABLE_AUTOUPDATE`. Use a `:free` model.
   - Record each difference from 1.19.3. If a fix is small and local to `opencode.rs` or
     `agents/opencode.rs`, make it and add a fixture test. If not, **stop and report**. This affects
     the main line's daily driver, so put it at the top of your review file whatever the outcome.
2. **The agent-kind CHECK** (after decision 2): the migration, the runner change if one is needed,
   and a test that migrates a populated database and keeps every row.
3. **Make adding an agent cheaper,** with no behaviour change:
   - an `AgentInfo` flag for "has an event channel", replacing the hard-coded shell and Grok checks;
   - `PARTNER_ORDER` built from a per-`Spec` partner rank, if that's simple.
   Keep it small. It touches hot files.
4. **Gemini CLI adapter** (after decision 1 permits an install).
   - Verify everything against the installed version's `--help` and docs, and record the version:
     model flag, approval or permission modes (likely `--approval-mode` / `--yolo`; verify), effort
     (if any), resume, the auto-update switch (a setting or environment variable; verify), the sign-in
     path (Google login, or `GEMINI_API_KEY` from the vault), and the trust or first-run prompts.
   - Check whether it has hooks, an event stream or a transcript file you could read later. Record
     what you find; don't build a channel unless it is per-launch and needs no global config.
   - Add the Google key pattern (`AIza[0-9A-Za-z_-]{35}`) to `secret-patterns.json` at the end, with a
     scrubber test.
   - Prove each declared option applies (a tiny turn, on a free tier or Matthew's login; ask before
     spending).
5. **Aider adapter** (after decision 1).
   - Verify against the installed version: `--model`, `--openai-api-base` and keys from the vault,
     `--no-check-update` (or the current switch), `--yes-always` and its relation to permission
     modes, `--restore-chat-history` for resume, `--notifications-command` (a possible turn-end
     signal; it would be a command run under the shell, so check the token rules first: it must never
     need a secret in argv), and the git auto-commit behaviour (`--no-auto-commits` / `--auto-commits`;
     show it as an option, and say what the default is).
   - Aider is API-key only. Run it on OpenRouter `:free` models through the existing route if the
     route fits, and say what had to change.
6. **Ollama through opencode** (after decisions 1 and 3).
   - A provider of kind "Ollama (local)" with a base URL (default `http://127.0.0.1:11434`). Read its
     model list from `GET /api/tags` when the user asks.
   - The opencode config gets a keyless OpenAI-compatible provider block. Verify the block's exact
     shape against opencode 2.1.1's docs.
   - `check_model` must not report "not connected" for a keyless provider. "Ollama isn't running at
     <url>" is a visible error.
   - Chorus never starts or stops Ollama itself.
7. **CI: `.github/workflows/check.yml`.**
   - `windows-latest`, Node 22 and the stable Rust toolchain, with `npm ci` and cargo caches. Steps:
     typecheck, vitest, `cargo test --workspace`, clippy with `-D warnings`, `npm run grep:secrets`.
   - Triggers: push and pull_request. Use least-privilege `permissions: contents: read`. Pin actions
     by version.
   - **Run the same commands locally** on a clean clone of your branch (in a scratch folder), and
     report the timings.
   - Check that `bindings_are_current` and the `externalBin` rule (L38) don't break a CI build. CI
     shouldn't run `npm run build`.
8. **"Drop the 2" design, written in the review file.** Cover what changes when productName becomes
   "Chorus":
   - the install folder, the shortcuts, the Apps entry, and the HKCU keys;
   - the collision with Chorus 1's `Chorus.lnk`, which must already be uninstalled;
   - how an existing "Chorus 2" install moves: Tauri's NSIS hooks (`installerHooks`, verify) that
     detect the old uninstall entry, run its uninstaller silently while **keeping data** (the data
     folders follow the identifier, which doesn't change), and remove the leftover HKCU key;
   - how to test it in a VM or a Windows Sandbox, never on Matthew's machine.
9. **Code-signing decision, written up.**
   - The options, with current costs and requirements **checked on the vendors' sites, dated**:
     (a) unsigned, with SmartScreen warnings until it has a reputation; (b) an OV certificate on a
     hardware token or in cloud HSM signing; (c) Microsoft's managed signing service (Azure Trusted
     Signing or Artifact Signing; check its current name, eligibility and price); (d) EV.
   - For each: what SmartScreen does, the yearly cost, identity checks, and how it plugs into Tauri
     (`bundle.windows.signCommand`) and CI.
   - Recommend one. Note that the updater's signature (Task 10) is separate from code signing.
10. **Update channel design, written up.** The Tauri updater plugin (approved as "later" in ARCHITECTURE §10):
    - its signing keypair and where the private key lives (never in the repo; a CI secret);
    - the endpoint (GitHub Releases `latest.json`, for example);
    - how a per-user NSIS install updates without admin rights;
    - how a dev build is kept off the channel;
    - what the UI shows ("Update available", never automatic without consent);
    - what an update must never do: touch data, or change the identifier.
    Recommend whether to wire it now, disabled.
11. **Installer polish.**
    - The uninstall "delete app data" wording (S8). Check that the installer's icons and names are
      right.
    - Clean up the leftover HKCU key on uninstall, if Tauri's NSIS hooks allow it without touching
      Chorus 1's keys. Verify this on a throwaway install with `/S /NS`, as S8 did. **Never uninstall
      or overwrite Matthew's installed "Chorus 2"**: if one is installed, stop and ask.
12. **Exit checks and review file.**

## 5. Decisions for Matthew (ask before building past them)

1. **May you install Gemini CLI, Aider and Ollama on this machine?** Without them, the adapters can
   only be written from docs and marked unverified. That breaks PLAN P2's bar (each option observed to
   apply).
   - (a) Install all three. Gemini through npm, globally; Aider through `uv tool install aider-chat`
     or pipx (say which, since each leaves a tool folder); Ollama through its Windows installer, plus
     one small model pull, which is several GB.
   - (b) Install Gemini and Aider only, and leave Ollama unverified.
   - (c) None. Write the adapters marked "unverified" and behind a visible notice.
   - *Recommendation: (a), with Ollama's model as small as possible (for example a 1–3B model), and
     each removed afterwards if he prefers.*
2. **How do we widen the `agent` CHECK?**
   - (a) Teach the migration runner a per-file header such as `-- chorus: foreign_keys off`. It sets
     `PRAGMA foreign_keys=OFF` before the transaction, runs SQLite's documented 12-step rebuild, then
     `PRAGMA foreign_key_check`, then turns the keys back on. It is general and documented, and a
     small change to `store/mod.rs`, with a test.
   - (b) Replace the CHECK with a trigger in the same rebuild, so later agents need no rebuild. This
     still needs (a) once.
   - (c) Keep the CHECK and store new agents under an existing value. Never: it would corrupt
     meaning.
   - *Recommendation: (a) + (b). One careful rebuild now, and adding an agent never needs one again.
     Each new agent is still checked in Rust by `AgentKind::parse`.*
3. **Is Ollama an agent or a provider?**
   - (a) A provider route for opencode (and later for Codex or Aider). It is one adapter less, and it
     reuses opencode's TUI and server events.
   - (b) Its own `AgentKind` running some chat TUI. It is more code, and gets no events.
   - *Recommendation: (a), as PLAN says "Ollama (via opencode)".*
4. **Where do the new agents sit in Pair and Workbench partner order?** *Recommendation: after Codex
   and opencode, before Grok, until they have event channels.*

## 6. Notes on hot files

- `agent.rs`: new `AgentKind` arms and `Spec` tables. P8 adds an instructions field to `Spec` and P7
  adds Claude's `-n`. Use field defaults, and expect a merge. Set each new agent's instructions
  capability to whatever you verify (default `None`).
- `sessions/mod.rs`: only the per-agent branches the new agents need.
- `NewSessionForm.vue`: the maps and `AGENT_ORDER` only.
- `store/mod.rs`: the runner change (decision 2) is the only logic change there. Test it thoroughly.

## 7. Verification

- **Unit tests:** the conformance test over the new `ALL`, version parsing per CLI, the shim and
  entry-point parsing for Aider, the Google key pattern, the Ollama provider block, and the migration
  rebuild (populated database, every row kept, FK check clean).
- **Runtime, in your `p14` instance**, for each installed agent:
  - It launches.
  - Each declared option is observed to apply: read it back from the agent's own output, config
    echo, or a canary turn.
  - Auto-update is off. Prove it from the CLI's docs, and from no update attempt in its output or
    logs.
  - Resume works, if declared.
  - The status shows "Running", with the "no event channel" note.
  - Not installed: Settings → Agents shows "not found" and the install hint.
- **CI:** a clean clone, the workflow's commands, all green. Report the timings.

## 8. Files you will probably touch

New: `.github/workflows/check.yml`, `src/assets/agents/{gemini,aider}.png` (simple monochrome marks
you draw; never copy a vendor's logo file into a public repo without checking its licence), fixtures,
one migration.
Hot (minimal): `agent.rs`, `cli.rs`, `shim.rs`, `env.rs`, `launch.rs`, `providers.rs`,
`src-tauri/src/agents/{mod,opencode}.rs`, `sessions/mod.rs`, `store/mod.rs` (runner), `labels.ts`,
`status.ts`, `NewSessionForm.vue`, `SessionView.vue`, `PromptHistory.vue`, `tokens.css`,
`secret-patterns.json` (append).
