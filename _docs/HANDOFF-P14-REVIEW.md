# P14 review — More agents and ship

## 1. Verdict

**Partly done.** Gemini CLI, Aider and Ollama go through Chorus's launch path and are observed
starting with the arguments they declare; a turn ran on Ollama through opencode (in a standalone
probe; in Chorus only after the fix in §12); CI is in; the
installer's uninstall wording and registry leftover are fixed and tested. Task 1 (re-verify opencode
2.1.1) was stopped and reported, as the plan says; **`main` has since done that port itself**, so the
report `_docs/HANDOFF-OC2-opencode.md` is marked superseded. Two limits of this hand-off, both
recorded in §6: **no Gemini or Aider turn was run** (there is no Gemini login, Gemini key or
OpenRouter key on this machine, and a turn needs Matthew's go-ahead), and I cannot click the app (no
GUI control), so runtime checks read the database, the process list and the generated installer
rather than a screen.

## 2. Branch

- Branch `feature/p14`, base `cb415a5` (`git rev-parse main`), worktree
  `Chorus-2.0.worktrees/feature/p14`.
- Base `cb415a5`; **rebased onto `main` at `45eb41f`** once. The only conflicts were in
  `crates/chorus-core/src/opencode.rs` and `src-tauri/src/agents/opencode.rs`, where the Ollama
  commit met `main`'s own opencode 2.x port; both were resolved by keeping main's 2.x code and
  threading the `keyless` flag through it. `bindings.ts` was regenerated.
- Commits (`git log --oneline main..HEAD` after the rebase):

```
9c9ee27 P14: the installer forgets its path, and its data checkbox says what it deletes
ee49f32 P14: a Gemini session with no turn in it can launch again
d19bc43 P14: a CI gate on every push and pull request
acde347 P14: Ollama as a keyless provider for opencode
c8e9354 P14: the Aider adapter
11efd0d P14: the Gemini CLI adapter
d3d6a36 P14: one declaration for "has an event channel" and for partner order
e73bb5b P14: widen the agent CHECK with a rebuild the runner can do
```

Plus this review file as the last commit. The reviewer renumbers `0011`.

## 3. Files

**Created**

| File | Purpose |
|---|---|
| `src-tauri/migrations/0011_p14_agent_check.sql` | Rebuilds `sessions` (agent CHECK: gemini, aider) and `providers` (kind CHECK: ollama) with foreign keys off |
| `crates/chorus-core/src/gemini.rs` | Whether a Gemini CLI chat record has a turn in it (`ChatState`), with fixtures from 0.64.0 |
| `src/assets/agents/gemini.png` | Drawn four-point mark (mask) |
| `src/assets/agents/aider.png` | Drawn chevron-and-bars mark (mask) |
| `src-tauri/windows/hooks.nsh` | `NSIS_HOOK_POSTUNINSTALL`: removes the remembered install path on uninstall (not on update) |
| `src-tauri/windows/English.nsh` | Tauri's English NSIS strings with one changed: the "delete app data" checkbox now says what it deletes |
| `.github/workflows/check.yml` | The gate on push and pull request |
| `_docs/HANDOFF-OC2-opencode.md` (main repo's `_docs`, untracked like the other hand-offs) | The opencode 2.1.1 port, written up for a new session |

**Modified**

| File | Change |
|---|---|
| `crates/chorus-core/src/migrate.rs` | `Directives` and `directives()`: `-- chorus: foreign_keys off`; unknown directives refused |
| `crates/chorus-core/src/agent.rs` (hot) | `Gemini`/`Aider` kinds; `Spec.has_event_channel` and `Spec.partner_rank`; a declared level may have no arguments; the `GEMINI` and `AIDER` tables; tests |
| `crates/chorus-core/src/launch.rs` | `PARTNER_ORDER` const becomes `partner_order()`, built from the ranks |
| `crates/chorus-core/src/providers.rs` (hot) | `ProviderKind::Ollama`, `OLLAMA_URL`, `parse_ollama_tags`, `key_var` → `Option`, `Route.key_var` → `Option`, Ollama/ Aider route arms and refusals, tests |
| `crates/chorus-core/src/opencode.rs` (hot) | `check_model(…, keyless)`; a keyless provider isn't "not connected"; test |
| `crates/chorus-core/src/cli.rs` | Version lines for Gemini and aider |
| `crates/chorus-core/src/shim.rs` | Gemini 0.64.0's shim fixture |
| `crates/chorus-core/src/scrub.rs` + `secret-patterns.json` | The Google `AIza…` key shape, appended, with a scrubber test |
| `src-tauri/src/store/mod.rs` (hot) | Runner: directive handling, `foreign_key_check` before the version bump; tests that migrate a populated database |
| `src-tauri/src/agents/mod.rs` | Gemini and Aider launches; `GeminiChat`/`gemini_chat_state`; the Gemini branch of `conversation_exists` |
| `src-tauri/src/agents/commands.rs` | `AgentInfo.has_event_channel` |
| `src-tauri/src/agents/opencode.rs` | `check_model` takes `keyless` |
| `src-tauri/src/providers/mod.rs` | `test`, `fetch_catalog`, `ollama_base`, `ollama_tags` |
| `src-tauri/src/providers/commands.rs` | Ollama: address rules, no key |
| `src-tauri/src/sessions/mod.rs` (hot) | `SessionSummary.has_event_channel`; a keyless route takes no key and gets an Ollama note; a Gemini id whose chat has no turn is renewed; `check_model(keyless)` |
| `src-tauri/tauri.bundle.conf.json` | Installer icons, English strings, install hooks |
| `src/ipc/bindings.ts` | Regenerated |
| `src/raw/labels.ts`, `src/shell/status.ts` | Gemini and Aider labels and chips; Ollama's provider label |
| `src/views/newSession/NewSessionForm.vue` (hot) | Logos, programs, sign-in lines, agent order |
| `src/views/session/SessionView.vue` (hot), `PromptHistory.vue` | Ask `hasEventChannel` instead of naming shell and Grok |
| `src/raw/ProvidersPanel.vue` | Ollama: no key field, an address, the "never starts it" note |
| `src/styles/tokens.css` | `--agent-gemini`, `--agent-aider`, their classes |
| `src/stores/sessions.test.ts` | The fixture gains `hasEventChannel` |

Every hot-file edit above is one field, one call or one map entry, except `agent.rs`, whose Spec
table is the point of the task.

**Deleted:** none.

## 4. Surface added

- **Migration `0011`** (renumber at merge): rebuilds `sessions` and `providers`; no new tables or
  columns. The runner now reads `-- chorus: <directive>` header lines.
- **IPC:** two new booleans, `SessionSummary.hasEventChannel` and `AgentInfo.hasEventChannel`. No new
  commands or events.
- **Settings keys:** none.
- **Environment variables:** none new. (Gemini's `GEMINI_API_KEY` and aider's
  `OPENROUTER_API_KEY` are the vault's existing per-session key mechanism.)
- **Files Chorus writes:** none new. Gemini and aider keep their own state where they always did
  (`~/.gemini`, the project's `.aider*` files).
- **Dependencies:** none added.

## 5. Plan against result

| Task | Result |
|---|---|
| 1. Re-verify opencode 2.1.1 | **Done as a report; stopped there, as instructed.** The fix is not small: v2 moved the API under `/api`, renamed every event, changed the message shape and dropped `attach`. **`main` has since done the port itself** (`340fe5f`, `64836cb`, D58), so the hand-off `_docs/HANDOFF-OC2-opencode.md` is marked superseded; §3 of it is still the live measurement to compare against. This branch's only opencode work is the `keyless` flag, re-applied onto main's 2.x check in the rebase. |
| 2. The agent CHECK | Done (decision 2a). The runner turns foreign keys off only for a file that asks, checks them before committing, and restores them either way. Tests migrate a populated database at version 10: every row kept, `foreign_key_check` clean, the new kinds accepted, `kimi` refused, `prompts` still references `sessions`. |
| 3. Cheaper agents | Done. `Spec.has_event_channel` and `Spec.partner_rank`; `partner_order()` replaces the fixed array; the two views read the flag. |
| 4. Gemini CLI adapter | Done as far as verification goes (0.64.0). Options, resume, trust, shim, version, Google key pattern. **No turn run** (no login or key). Auto-update has no per-launch switch; see §9. |
| 5. Aider adapter | Done as far as verification goes (0.86.2, under uv's Python 3.12). Options, effort, resume by history, `--no-check-update`, OpenRouter route. **No turn run** (no key). |
| 6. Ollama via opencode | Done: provider, model list from `/api/tags`, the keyless block, the "not running" note, and a **turn that ran on `llama3.2:1b` through opencode with the exact block** (§6). It only became runnable once `main`'s 2.x port arrived in the rebase. **Corrected:** that turn ran in a standalone probe server, not in Chorus. In Chorus the first turn failed (the model was named `ollama/` twice); see §12 for the fix and the in-app turns. |
| 7. CI | Done. Run on a clean clone; timings in §6. |
| 8. "Drop the 2" design | Done, below. |
| 9. Signing decision | Done, below, with dated prices. |
| 10. Update channel design | Done, below. |
| 11. Installer polish | Done: new checkbox wording, installer icons, and the leftover `HKCU\Software\contactestablished\Chorus 2` key removed on uninstall (not on update). Tested with a real `/S /NS` install and uninstall. "Chorus 2" was **not** installed on this machine beforehand (checked), and Chorus 1 was untouched. |
| 12. Exit checks | Partly: see §6 for what is not verified. |

### Task 8 — dropping the "2" (design only)

When Chorus 1 is retired and `productName` becomes `Chorus`:

- **What changes:** the install folder `%LOCALAPPDATA%\Chorus 2` → `%LOCALAPPDATA%\Chorus`; the
  Start-menu file `Chorus 2.lnk` → `Chorus.lnk`; the desktop shortcut likewise; the Apps entry
  (`DisplayName`) `Chorus 2` → `Chorus`; the NSIS uninstall key `Uninstall\Chorus 2` → `Uninstall\Chorus`;
  the remembered install path `HKCU\Software\contactestablished\Chorus 2` → `…\Chorus`. The **window
  title stays "Chorus"**, `chorus.exe` stays, and **the identifier stays**
  `com.contactestablished.chorus2`, so `%APPDATA%` and `%LOCALAPPDATA%` data folders do not move.
- **The shortcut collision:** `Start Menu\Programs\Chorus.lnk` and `Desktop\Chorus.lnk` are Chorus 1's
  exact paths (S8). Chorus 1 must be uninstalled first; the installer's restart-manager check and
  shortcut handling can't tell the two apart by name. Make uninstalling Chorus 1 a written
  precondition of the release, and have the release notes say so.
- **Moving an existing "Chorus 2" install:** Tauri's NSIS template (2.12.3, read for this review)
  reads the remembered install path from `HKCU\Software\<manufacturer>\<productName>` and defaults
  the directory there. A new product name means a different key, so an old install would be left
  beside the new one: two Apps entries, two folders. Fix it with `installerHooks` (verified below to
  work in the bundle config): in `NSIS_HOOK_PREINSTALL`, if the legacy key
  `HKCU\Software\contactestablished\Chorus 2` holds an `$INSTDIR`, run its `uninstall.exe /UPDATE`
  silently (the template's own update path: it keeps data and registry values that the install then
  rewrites), then `DeleteRegKey /ifempty` the legacy key. `/UPDATE` is the switch the template uses
  for in-place upgrades, and the uninstaller's own "delete app data" is skipped in that mode, so
  data under the unchanged identifier survives. Verify the hook name and `$INSTDIR`/`$UpdateMode`
  are in scope in `PREINSTALL` before shipping.
- **Testing it:** in a Windows Sandbox or a VM, never on Matthew's machine: install the current
  "Chorus 2" build `/S /NS`, seed a session (so data exists), install the "Chorus" build `/S /NS`
  over it, check there is one Apps entry, one folder, the old folder gone, and the data intact, then
  uninstall and check the HKCU keys. The sandbox needs the WebView2 bootstrapper (the installer
  downloads it).

### Task 9 — code signing (decision, checked 2026-10-20)

Facts read on the vendors' own sites on 2026-10-20: Microsoft Learn's Artifact Signing quickstart
(updated 2026-10-08) and the Azure Retail Prices API (product still billed as "Trusted Signing");
SSL.com's code-signing pages.

| Option | What SmartScreen does | Yearly cost | Identity checks | How it plugs in |
|---|---|---|---|---|
| **(a) Unsigned** | "Unknown publisher"; every download is checked by reputation, which an unsigned file never builds | $0 | none | nothing |
| **(b) OV certificate** (hardware token or cloud HSM) | Shows the organisation's name; **still warned** until the publisher builds reputation from clean downloads. SSL.com states OV and EV have been treated equally for reputation since March 2024 | SSL.com lists $129/year for 1 year, $116.10/yr for 2, $109.65/yr for 3 (its IV and OV pages showed the same list), plus a FIPS token ($379 one-off) or cloud HSM (attestation fees quoted: Google $75, AWS $1,275 one-off) or its eSigner service | Organisation validation (or individual/sole-proprietor products) | `bundle.windows.signCommand` calling the token/HSM/eSigner tool, or `certificateThumbprint` + `timestampUrl` for a local token |
| **(c) Microsoft Artifact Signing** (formerly Trusted Signing) | Same as an OV certificate: the publisher is named and reputation builds over time. Zero-touch certs in FIPS 140-3 L3 HSMs | Basic **$9.99/month** (5,000 signatures/month, then $0.005 each); Premium $99.99/month (100,000) | Identity validation in the portal. **Individual developers must be in the US or Canada**, with an Azure billing account of type Individual whose legal name and address match a government ID, verified through Microsoft Authenticator (minutes). Organisations: US, Canada, EU, UK, AU, NZ, JP, KR, SG, CH, NO, IL; 1–20 business days | `signCommand` calling `signtool` with the Artifact Signing dlib/plugin and an Entra service principal; the private key never leaves Azure |
| **(d) EV certificate** | Historically instant reputation; Microsoft no longer guarantees that (SSL.com: "moved away from automatic instant reputation"). Only EV is accepted for kernel-mode drivers, which Chorus doesn't ship | SSL.com's EV pages (not read in full here); expect several hundred dollars a year plus a token | Organisation (or sole-proprietor EV for an individual) | same as OV |

**Recommendation: (c), Artifact Signing Basic, when the first public release is near.** It is the
cheapest credible publisher identity ($120/year), the key lives in an HSM Chorus never touches, and
it works from CI with a service principal. It does not remove the first-release SmartScreen warning;
nothing does, except downloads. Sign every release, including updates, with the same identity, so
reputation accumulates, and timestamp everything so old installers keep verifying.

The **updater's own signature** (Task 10) is a separate thing: a minisign key that proves an update
came from us. Code signing answers "who published this"; the updater key answers "is this the update
Chorus expects".

### Task 10 — update channel (design; not wired)

- **Mechanism:** `tauri-plugin-updater` (approved as "later" in ARCHITECTURE §10) with
  `bundle.createUpdaterArtifacts: true` in `tauri.bundle.conf.json`. The bundle then writes the NSIS
  installer and its `.sig`.
- **Keypair:** `npx tauri signer generate`. The **private key is a CI secret**
  (`TAURI_SIGNING_PRIVATE_KEY` + its password) and never enters the repo; the public key goes in
  `tauri.conf.json` under `plugins.updater.pubkey`. Losing the private key means every install must
  be replaced by hand, so back it up in the same place as the DPAPI-key escrow plan.
- **Endpoint:** a static `latest.json` on GitHub Releases (`https://github.com/<org>/<repo>/releases/latest/download/latest.json`),
  published with the installer. The updater compares versions, downloads the NSIS installer, checks
  its signature against the pubkey, and runs it.
- **Per-user NSIS update:** the template's `/UPDATE` mode runs the existing `uninstall.exe` in place
  (skipping its "delete app data") and installs over it; `installMode: currentUser` means no admin.
  Our install hook must not clean the install-path key while `$UpdateMode = 1` (the hook in this
  branch already checks it).
- **Keeping a dev build off the channel:** the updater lives only in `tauri.bundle.conf.json`
  (never in `tauri.dev.conf.json` or the `_verify` config), and `plugins.updater.endpoints` is left
  empty until Matthew supplies an endpoint, so a debug build has no channel at all.
- **What the UI shows:** a quiet "Update available" with the version and **Update** / **Later**;
  never a download without consent, never a silent restart while agents run. After installing, the
  app restarts; sessions in `running` state relaunch, which is already how startup works.
- **What an update must never do:** touch `%APPDATA%`/`%LOCALAPPDATA%` data, change the identifier
  (that would orphan the data), or run an install that needs admin.
- **Recommendation:** wire the plugin **now, disabled** — the code path and the settings exist, the
  endpoint and key are empty, and turning it on is two values once Matthew has picked the repo and
  generated the key.

## 6. Verification

**`npm run check`** (this worktree, final tree): green.

- `vue-tsc --noEmit`: no errors.
- vitest: 6 files, 19 tests passed.
- `cargo test --workspace`: 361 `chorus-core` tests, 47 `chorus_lib` tests, 3 purity, 3
  `chorus-hook`, 0 doc-tests; all passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `secret-grep`: clean, 7 patterns over 220 files.

**CI on a clean clone** (scratch clone of the branch at the final commit, `npm ci` from scratch,
no caches — the workflow's steps, hand-run in this order; GitHub's cache will make later runs
shorter):

Clean checkout of `9c9ee27` (the rebased tree), on this machine:

| Step | Seconds |
|---|---|
| `npm ci` | 4.5 |
| `npm run typecheck` | 3.8 |
| `npm test` | 1.2 |
| `cargo test --workspace` | 70.5 (every crate compiled from scratch) |
| clippy | 39.3 |
| `npm run grep:secrets` | 0.6 |

Under two and a half minutes in total with no caches. GitHub's first run will be slower (a cold
checkout and the toolchain download); later runs are faster with the caches.

`cargo test --workspace` on that clean tree includes `bindings_are_current` (passes), and builds
with `tauri.bundle.conf.json` present and no `binaries/chorus-hook-*.exe` — so the `externalBin`
rule (L38) does not reach a test or clippy build, and the workflow deliberately never runs
`npm run build`.

**Runtime, in the `p14` instance** (`com.contactestablished.chorus2.dev.p14`, port 1434 was taken —
see §10 — so 1435). No GUI can be driven from here, so these are read from the database the app
wrote and from the process table, and the app was stopped by PID afterwards.

| Check | Evidence |
|---|---|
| The migration runs in the app | `user_version` 11 in the dev database; `sessions.agent` CHECK lists shell…aider; `providers.kind` CHECK lists ollama; `PRAGMA foreign_key_check` empty |
| Gemini launches | run 1: program `node.exe` + `…@google\gemini-cli\bundle\gemini.js` (the npm shim resolved), PID in the app's job, CLI version 0.64.0 |
| Gemini's declared options reach it | run 6: `--skip-trust --approval-mode default --session-id <uuid>` |
| Gemini resume | run 3: `--resume <uuid>`, `resumed` 1. It exited 42 — see the bug below |
| Aider launches | runs 2, 4, 5, 7: program `…\.local\bin\aider.exe`, CLI version 0.86.2 |
| Aider's options reach it | run 7: `--no-check-update --restore-chat-history --reasoning-effort high` |
| Aider's default level passes nothing | run 2 (Manual, no effort chosen): `--no-check-update --restore-chat-history` |
| Auto-update off | **aider: yes** — `--no-check-update` is in argv, and 0.86.2's `args.py` gates its version check on it. **Gemini: no** — 0.64.0 has no flag or environment variable for it; its switch is the user's `general.enableAutoUpdate`. §9 Q3. |
| Children die with the run | stopping the app left 0 of its agents and their Python/node children running (Job Object) |
| Not installed | code path only: `resolve` says "… not found on PATH", `detect` records `found: false` with that problem, and `detectionText` shows it. Seen for all three CLIs before they were installed (empty `where`), not in the UI. |
| Ollama on opencode, end to end | run 8 in the dev instance: `opencode.exe --server http://127.0.0.1:57775 --session ses_…`, version 2.1.1, and **no note** on the session, so the keyless model check accepted `ollama/llama3.2:1b` instead of calling the provider "not connected". A probe server with exactly the block `providers::route` builds (`{"ollama":{"npm":"@ai-sdk/openai-compatible","options":{"baseURL":"http://127.0.0.1:11434/v1"},"models":{"llama3.2:1b":{…}}}}`) ran a turn: assistant `model=ollama/llama3.2:1b`, `finish=stop`, 140 output tokens, cost 0. (The 1B model answered with a malformed tool call — the plumbing works, the model is small.) |
| Installer uninstall | `/S /NS` install: folder, Apps entry "Chorus 2", `HKCU\Software\contactestablished\Chorus 2` = install path; `/S` uninstall: folder, Apps entry and key gone, data folders kept, Chorus 1's install/entry/shortcuts untouched |

**A bug this found and fixed.** Run 3 showed a Gemini session that had never had a turn could not be
relaunched: its chat file exists (so `--session-id` is refused) but has no turn (so `--resume` exits
42). `chorus_core::gemini::chat_state` now decides resumability the way Gemini does, and the launch
mints a new id for an empty chat; run 6 is the proof (`--session-id bd84fc66…`, a new id).

**Not verified, and why**

1. **A real turn in Gemini or Aider.** There is no Gemini login or API key on this machine and no
   OpenRouter key available to me; a turn spends Matthew's credit or needs his Google login, and
   the plan says to ask first. So "each declared option is observed to apply" is met only to the
   level of *the CLI accepting the argument at launch*; no canary turn read a model or an effort
   back.
2. **Gemini resume and trust, on a signed-in CLI.** `--resume` was passed and refused (the bug
   above); with a real conversation the CLI should accept it, but that was not seen.
3. **The uninstall checkbox on screen.** Its text is checked in the generated `English.nsh` and in
   the built installer's script (it carries "Delete app data too (sessions, history, keys)"), not
   rendered in a GUI.
4. **Any screen at all.** Session cards, chips, logos and the provider form were checked by the
   typecheck and the generated bindings, not by eye.

## 7. Versions verified against

- **Gemini CLI 0.64.0** (`@google/gemini-cli`, npm global; `gemini --version` prints `0.64.0`).
- **aider 0.86.2** (`aider-chat` through `uv tool install --python 3.12`; uv's CPython 3.12.12;
  `aider --version` prints `aider 0.86.2`). Under the machine's default Python 3.14 it does not
  start at all: `pydub` wants `audioop`, removed in 3.13.
- **Ollama 0.40.6** (winget `Ollama.Ollama`), with `llama3.2:1b`.
- **opencode 2.1.1** (`@opencode/cli`): re-verified here, then used through `main`'s 2.x adapter (D58) for the Ollama turn. The run records `2.1.1`.
- **Tauri 2.12.4 / tauri-cli 2.12.3**, NSIS 3.13 — the bundler's `installer.nsi` and `English.nsh`
  were read at the `tauri-cli-v2.12.3` tag to write the hooks and the strings.
- **Vendors, 2026-10-20:** Microsoft Learn Artifact Signing quickstart (page updated 2026-10-08),
  Azure Retail Prices API (`productName eq 'Trusted Signing'`, prices effective 2024-06-01),
  SSL.com's code-signing pages (prices as shown that day).

## 8. Proposed doc changes

**Decisions** (rows in DECISIONS.md's style; ids for the reviewer to assign):

- **P14-D1 — Migrations may ask for the foreign keys to be off (Decided).** A migration's header can
  carry `-- chorus: foreign_keys off`; the runner turns them off around that file's transaction,
  runs `PRAGMA foreign_key_check` before the version bump, and restores them whatever happens. An
  unknown directive is refused. This is the only safe way to change a CHECK.
- **P14-D2 — Ollama is a provider for opencode, not an agent (Decided).** `ProviderKind::Ollama`,
  default `http://127.0.0.1:11434`, no key, models from `GET /api/tags`, a keyless
  `@ai-sdk/openai-compatible` block in opencode's configuration, model `ollama/<id>`. Chorus never
  starts or stops Ollama.
- **P14-D3 - Gemini CLI's permission levels (Decided).** Manual is `--approval-mode default`,
  Accept edits is `auto_edit`, Plan is `plan`, Full access is `yolo`; no Auto (there is none), and
  a mode is always passed because the user's `general.defaultApprovalMode` setting can be one.
- **P14-D4 — Code signing: Artifact Signing Basic (Proposed).** $9.99/month for 5,000 signatures,
  individual eligibility in the US/Canada with a matching government ID, signed from CI through
  `signCommand` with an Entra service principal. It names the publisher and lets reputation build;
  it does not skip the first release's SmartScreen warning.

**Lessons:**

- **P14-L1 - opencode 2.x facts this phase added.** D58 and L41 (main, 2026-10-20) cover the v2
  API; two things they don't: `GET /api/provider` marks a keyless provider `activation:
  "disabled"`, so a keyless route must not read that as "not connected"; and the Ollama block
  (`npm: "@ai-sdk/openai-compatible"`, `options.baseURL` ending `/v1`) is accepted by 2.1.1 and
  ran a turn. The full measurement is in the (now superseded) OC2 hand-off, section 3.
- **P14-L2 — Gemini CLI 0.64.0.** `--skip-trust` trusts the folder for the session only. Exit 41 is
  "no auth", 42 is "input error": `--session-id` on an existing id and `--resume` on a chat with no
  turn both give it. A chat file is created as soon as a session starts, with a `<session_context>`
  user record that the CLI ignores when deciding resumability. Auto-update has no flag or
  environment variable: it is `general.enableAutoUpdate` in `~/.gemini/settings.json` (System >
  User in the merge), so Chorus can't switch it off without writing the user's settings or
  overriding system policy with `GEMINI_CLI_SYSTEM_SETTINGS_PATH`.
- **P14-L3 - aider on Windows.** Install it with Python 3.12 (`uv tool install --python 3.12`);
  the machine's 3.14 breaks it in `pydub`. `uv`'s launcher is a real `aider.exe`, not a shim. It
  asks to add `.aider*` to `.gitignore` on the first run in a repository. It has no permission
  flags: asking per edit is the default, `--yes-always` is everything. `--restore-chat-history`
  reloads the folder's chat; it has no conversation id. `--reasoning-effort` goes to the model and
  is unset by default.
- **P14-L4 — Tauri's NSIS uninstaller keeps the install path.** `HKCU\Software\<manufacturer>\<product>`
  holds `$INSTDIR` for the next install's default folder and is only removed when "delete app data"
  is ticked. `NSIS_HOOK_POSTUNINSTALL` can remove it when `$UpdateMode <> 1`; `customLanguageFiles`
  replaces a language file wholesale.
- **P14-L5 — Gemini's chat file can exist with no turn, so ids get burned.** See §6; check the
  record's content, not the file's existence.

**PLAN:** under P14, note that P14 ships Gemini, Aider and Ollama (a provider), and that opencode
2.x is main's port (D58), not this phase's.
P14 ships Gemini, Aider and Ollama as a provider.

## 9. Questions for Matthew

1. **Signing: Artifact Signing Basic, or stay unsigned for now?** (a) Sign: $9.99/month, individual
   validation in the US/Canada, minutes to set up once the Entra account exists; first downloads
   still warn. (b) Stay unsigned until there is something to distribute: $0, "Unknown publisher" on
   every download. *Recommendation: (a) when the first public release is near, not sooner.*
2. **Do you want `tauri-plugin-updater` wired now, disabled?** (a) Yes: the plugin loads, the
   endpoint and key are empty, and turning it on is two values later. (b) Later: no dependency
   until there is a release to update. *Recommendation: (a).*
3. **Gemini's auto-update: leave it on, or make Chorus switch it off?** (a) Leave it: 0.64.0 has no
   per-launch switch, and the only one is the user's settings file, which Chorus must not write.
   (b) Pass `GEMINI_CLI_SYSTEM_SETTINGS_PATH` per launch with `general.enableAutoUpdate: false`,
   which would also override any real system policy file on the machine. *Recommendation: (a), and
   say so in the review of the agent; an update shows up as a newer version at the next launch.*
4. **The OC2 hand-off:** main did the port while this was being written, so delete
   `_docs/HANDOFF-OC2-opencode.md` or keep only its section 3 as a measurement. Your call; it is
   untracked, so leaving it is harmless.
5. **May I keep the three CLIs installed?** Gemini 0.64.0 (npm global), aider 0.86.2 + uv's Python
   3.12, Ollama 0.40.6 + `llama3.2:1b` (~1.3 GB). Removing them is one command each, and Chorus
   would then show "not found" for Gemini and aider.

## 10. Risks and known issues

- **opencode 2.x** is `main`'s port (D58), not this branch's; the Ollama route rides on it. Another worktree
  (`Chorus-2.0-oc2`) was running its own dev instance and opencode sessions during this review, so a
  stray `opencode.exe` in the process list may be that phase's, not a leak from here. Mine died with the
  app every time they were checked.
- **Migration 0011 rebuilds two tables.** It is tested on a populated database and ran in the dev
  instance, but it must be renumbered at merge and reviewed by someone else. The runner change is
  the risky part: if a future migration's directive is mistyped it is refused, not ignored.
- **`partner_order()` changes Pair's default reviewer** for a builder on opencode (Grok moves behind
  Gemini/Aider where installed). No test expected the old order, but it is a behaviour change.
- **`has_event_channel` is serialised to the UI**; a merge with P7/P8's `SessionSummary` fields
  should be a clean add, but `sessions/mod.rs` is hot in both.
- **Gemini's `--skip-trust`** was taken from `--help`; with a signed-in CLI it should stop the
  folder-trust prompt, but that was not seen.
- **Aider under Chorus's `TERM=xterm-256color`:** in a redirected console prompt_toolkit complained
  ("expecting a Windows console"); in the app's ConPTY it launched and stayed up, but I could not
  see its screen. If it looks garbled, `--no-pretty`/`--no-fancy-input` are the escape hatch.
- **Port 1434** was held by an unrelated process (PID 16372, started 2026-10-18, unreadable command
  line), so the `p14` instance ran on 1435. I did not touch that process.
- **Seeded rows:** the runtime checks above used sessions inserted into the dev database by hand
  (with `state='running'` so the app relaunched them). The dev database is disposable; delete it.
- **`cli_version` can be empty on a relaunch at startup** (run 3), because detection may not have
  finished; pre-existing, not new here.

## 11. How to review

1. `crates/chorus-core/src/migrate.rs` and `src-tauri/src/store/mod.rs` (the runner) with migration
   `0011`; the two store tests are the evidence.
2. `agent.rs` (the Spec fields and the two new tables), then `launch.rs` (`partner_order`).
3. `providers.rs` (Ollama) with `src-tauri/src/providers/{mod,commands}.rs`, then
   `crates/chorus-core/src/opencode.rs` (`keyless`).
4. `src-tauri/src/agents/mod.rs` (both launches and `gemini_chat_state`) and
   `crates/chorus-core/src/gemini.rs`.
5. `sessions/mod.rs`: the keyless branch, the Ollama note, the Gemini id renewal, the summary field.
6. The installer files and the workflow; then the hand-off `_docs/HANDOFF-OC2-opencode.md`.

Manual test script (in the real app, your instance):

1. Settings → Agents: Gemini CLI and Aider show a version ("0.64.0", "0.86.2"); uninstalling one
   shows "not found" and the install hint.
2. New session → Gemini CLI, no effort (there is none), Manual: it starts (and, without a login,
   exits with the CLI's auth message on the terminal, 41). Approve/deny levels appear as Manual,
   Accept edits, Plan, Full access — no Auto.
3. New session → Aider, Deep, Full access: the summary shows `aider --no-check-update
   --restore-chat-history --reasoning-effort high --yes-always`.
4. Settings → Providers → Add → Ollama (local): test says "Ollama answered at http://127.0.0.1:11434
   and lists 1 local models"; Refresh models lists `llama3.2:1b`. Stopping Ollama and testing again
   says "Ollama isn't running at …".
5. Install the bundle `/S /NS` on a throwaway profile, uninstall `/S`, and check the HKCU key is gone
   and the data folder is kept; the uninstall checkbox reads "Delete app data too (sessions,
   history, keys)".

## 12. After the merge: Ollama's model name (2026-10-20)

**How this reached `main`.** The commits above (`a7612a2`, on `45eb41f`) were pushed to GitHub's
`main` by mistake at 22:05, while the main line had merged `handoff/oc2` locally. The main line merged
the two as `fec5ccd`, resolved as in `HANDOFF-P14-OC2-MERGE.md` §4, and the fix below went on top.

**The bug.** New session offers Ollama's own model names (`llama3.2:1b`), but opencode's rule that a
model is `provider/model` ran even on a provider, so a bare name was refused. Typed as
`ollama/llama3.2:1b`, it passed that rule but was prefixed again by the route, and Ollama answered
"model not found". The turn of §6 that ran had used a standalone probe server, not Chorus.

**The fix.**
- `Spec::resolve_on(asked, on_provider)`: on a provider the model is that provider's own id, and the
  route names it the agent's way. Session creation and `launch_args` (New session's summary) pass
  whether a provider is chosen; `resolve` is `resolve_on(…, false)`.
- A start-up relaunch's model check could run before opencode had loaded Chorus's Ollama block, and
  named a missing key. For a keyless route, `Api::check_model` now waits for that provider to be
  listed (`opencode::lists_provider`), and an unlisted keyless provider is `NoSuchProvider`, never
  `NoKey`.
- Tests: `a_bare_ollama_id_reaches_opencode_as_ollama_slash_it` (resolve, route, configuration), the
  bare id in `agent.rs`, `lists_provider`, and the keyless cases of `check_model`.

**Seen in the `p14` instance** (Ollama with `llama3.2:1b`, opencode 2.1.1, driven over CDP and IPC):
- `launchArgs` for `llama3.2:1b`: without a provider, "opencode takes a model as provider/model…";
  on Ollama, `model=llama3.2:1b`.
- A new session on Ollama with the bare id: "Reply with the single word PONG." ended `done`, "PONG".
- Restart: resumed, no note, observed `ollama/llama3.2:1b` (variant `default`, agent `build`) with no
  differences; a second turn ended `done`, "PING".
- Before the wait, a session relaunched at start-up had the note "opencode has no key or login for
  ollama…". After it, the start-up relaunch left both Ollama sessions without a note (a keyless
  OpenRouter session kept its own), and a typed turn ended `done`, "READY".
- Closing the app ended all six opencode processes it had started.

`npm run check` passed after the last code change (core 369 tests).