# Kickoff prompt: record decisions, remove voice, run Phase 0

Run in a new session in this repo with: **"Read `_docs/prompts/P0-kickoff.md` and follow it."**

---

You are starting development of **Chorus 2.0**, a ground-up rebuild of Chorus on Tauri 2 (Rust) with a
Vue 3 web UI. The repo has plans but no code yet. Chorus 1 lives at `../Chorus` and is the reference
implementation for proven behaviour.

Before anything else, read in full: `CLAUDE.md`, `_docs/PLAN.md`, `_docs/ARCHITECTURE.md`,
`_docs/SCOPE.md`, `_docs/DECISIONS.md`, `_docs/OPEN-QUESTIONS.md`, `_docs/LESSONS.md` and
`_docs/design/DESIGN.md`. They are the source of truth; this prompt only adds what changed since they
were written.

Three parts, in order. Stop at the end of Part C; Phase 1 gets its own prompt once the spike results are in.

## Safety rules for this whole session

- **Chorus 1 is Matthew's daily driver and may be running agents right now.** Never kill processes by
  name (`electron.exe`, `node.exe`, `claude.exe`, `codex.exe`, `opencode.exe`, `grok.exe`, `cmd.exe`,
  `conhost.exe`). Kill only PIDs you launched, recorded at launch time. Remember that you are yourself a
  `claude.exe` process.
- **Never touch Chorus 1's installation or data:** `%LOCALAPPDATA%\Programs\Chorus`,
  `%APPDATA%\chorus-app` (the installed app's data) or `%APPDATA%\chorus` (its dev data). Chorus 1's
  identifier is `com.contactestablished.chorus`, so 2.0 must use a different one (see Part B).
- **Keys:** test with fake keys only. Never print a real or fake key; prove presence by length or hash.
- **Money:** subscription turns on Claude Code and Codex are fine if kept few and tiny. Anything that
  spends API credit (OpenRouter, provider keys) needs Matthew's go-ahead first; prefer `:free` OpenRouter
  models where a test needs one.
- **Agent state on this machine:** run probe agents in a scratch directory outside any real repo. Give
  every opencode process its own temporary `XDG_STATE_HOME` and `XDG_DATA_HOME` (LESSONS L5). Claude Code
  asks "Do you trust this folder?" in a new directory; expect it, don't mistake it for a hang.
- **Evidence** (logs, screenshots, captured output) goes in `_verify/`, which is gitignored. Spike write-ups
  in `_docs/spikes/` summarise it without local paths, usernames or keys.

## Part A — Record Matthew's decisions and remove voice

Matthew answered on 2026-10-18: **keep the name Chorus, accept every default in OPEN-QUESTIONS.md**, and
**move push-to-talk out of Chorus** into a separate, system-wide dictation app he can use with every
application. Apply that to the docs:

1. **Voice is out of Chorus.** All four Voice features leave scope: push-to-talk and click-to-talk, local
   transcription, mic overlay and target ring, and transcript cleanup. They go to a separate app.
   - `SCOPE.md`: move the Voice table into a new **"Moved to a separate app"** section with that reason;
     update the counts at the top (73 kept, 14 discarded, 4 moved out, 1 deferred); remove voice from any
     other row (for example the "mic button on composer").
   - `PLAN.md`: mark **P9 as removed** ("Voice moved to a separate app, see D<n>"). **Do not renumber
     phases or spikes**; other docs refer to them. Mark spikes **S2** (push-to-talk hotkey) and **S6**
     (local transcription) as removed. Reword **S3** so it tests the fleet switcher strip: always on top,
     never takes focus, draggable (LESSONS L11). Remove voice from the M3 milestone text.
   - `ARCHITECTURE.md`: remove the mic overlay window and "Voice" from §1; in §10 drop the
     `global-shortcut` plugin and the `whisper-cli.exe` row; in §11 move the voice row to "Not ported".
   - `DECISIONS.md`: mark D20 **Superseded** by the new voice decision.
   - Grep all docs for `voice`, `push-to-talk`, `whisper`, `mic`, `dictat`, `transcript cleanup`,
     `global-shortcut`, `S2`, `S6` and `P9`, and fix anything left.
   - Add a **compatibility requirement** (a decision, plus a line in PLAN principles or P4): an external
     dictation app must work with Chorus like any text field. Chorus never steals focus (the fleet strip
     and any overlay are non-activating), the composer and the Raw terminal accept inserted or pasted
     text, and nothing Chorus receives is ever auto-submitted.

2. **Name:** Chorus, with the seven-bar mark. The mockup's "Harness" wordmark and rotated-square glyph are
   placeholders. Update `DESIGN.md`'s Logo section and the rail description. The mark's geometry is in
   DESIGN.md; Chorus 1's drawing is `../Chorus/src/renderer/src/components/ChorusMark.vue`.

3. **Every other open question takes its default.** Record each as a Decided entry in `DECISIONS.md`
   (next free D-numbers, dated 2026-10-18), and change the Proposed entries they settle to Decided: D4,
   D9–D14, D17–D19 and D21. Fold Q6 into D7. The defaults, briefly:
   Q1 Harness mockup is the UI, nothing else pending · Q3 Glacier (default) and Dusk · Q4 session strip and
   Wall layout · Q5 pop-out windows deferred · Q6 responses kept as final message per turn, tool-call list
   and asks with their answers; transcripts stay in the agents' files with paths recorded · Q7 narrator on
   a cheap OpenRouter model chosen in Settings, on by default, switchable off per project · Q8 Chorus
   answers approvals itself if S5 proves it reliable, falling back to the agent's own prompt · Q9 the
   dependency list in ARCHITECTURE §10 is approved (minus the voice items removed above) · Q10 Overview
   board replaces Mission Control; forecasts only after turn capture is ≥95% complete · Q11 skills fold
   into Bots and Automations · Q12 Notes replace project memory · Q13 the repo is public (confirmed on GitHub) · Q14
   Windows only for v1 · Q15 one-time, read-only project import from Chorus 1 (a small P4 task; add it to
   SCOPE) · Q16 schedules first, GitHub triggers later via `gh` polling · Q17 Claude Code and Codex remote
   control are noted and decided at P13.
   - **Still open:** what the JEV AI key (TypeSafe) is for. It stays a stored key with a connection test
     until Matthew says. Keep only this in `OPEN-QUESTIONS.md`, and turn the rest of the file into a short
     "Resolved 2026-10-18" table mapping each Q to its D-number.

4. Check every relative link and `#anchor` across the docs still resolves, and that no doc contains CRLF.

**Commit Part A** (see "Commits" below).

## Part B — Scaffold

Verify the toolchain first and record the versions in `_docs/spikes/README.md`: `rustc`, `cargo`,
`node`, `npm`, and the installed WebView2 runtime. Then check the current Tauri 2 versions with
`npm view` and `cargo search` and **pin 2.x**, since 3.0 is in alpha.

Build the layout in ARCHITECTURE §2:

- Tauri 2 + Vue 3 + TypeScript (strict) + Vite + Pinia. Starting from `create-tauri-app`'s vue-ts
  template is fine, as long as the result is Tauri 2 and matches the layout.
- A Cargo workspace with `src-tauri` and `crates/chorus-core`. **Enforce the pure-core rule
  mechanically:** `chorus-core` depends on nothing beyond `serde` and similar pure crates, and a
  `clippy.toml` (or a test) forbids `std::fs`, `std::process`, `std::net` and `std::env` inside it. Add
  one trivial function with a unit test to prove the wiring.
- Generated TypeScript bindings via `specta` and `tauri-specta`, with one example command and one event
  wired end to end into a Pinia store. If `tauri-specta` doesn't work with the pinned Tauri, **stop and
  ask**; don't swap in another library.
- **Identifiers:** release `com.contactestablished.chorus2`, dev `com.contactestablished.chorus2.dev`,
  each with its own data directory. Show the live data directory in the window. The product name is
  "Chorus". S8 decides whether the install folder collides with Chorus 1's.
- npm scripts: `dev`, `build`, `typecheck` (`vue-tsc`), `test` (`vitest`), `test:rust`, `lint:rust`
  (`cargo clippy --workspace -- -D warnings`), `grep:secrets` (port `../Chorus/scripts/secret-grep.mjs`
  and `../Chorus/src/main/services/secret-patterns.json`), and `check`, which runs all of them.
- A minimal window showing "Chorus 2.0", the app version and the data directory. No real UI yet.

Exit: `npm run check` passes, `npm run dev` opens the window, and a release build runs. **Commit.**

## Part C — Spikes

Run them in this order: **S1, S4, S5, S3, S7, S8**. S2 and S6 were removed in Part A. Write each result
to `_docs/spikes/S<n>.md` with:
- **Verdict:** pass, fail, or changed approach.
- The versions tested.
- What was measured and how.
- What it means for the plan.

If a result changes the plan, update PLAN, ARCHITECTURE or DECISIONS in the same commit. Keep spike code
that will become real code in its real place; put throwaway harnesses in `_verify/`.

- **S1 — PTY host.** `portable-pty` (ConPTY) streaming to xterm.js over a `tauri::ipc::Channel`, batched.
  - Launch each agent through its real install: `claude` is a native exe in `~/.local/bin`; `codex` and
    `opencode` are npm `.cmd` shims (LESSONS L14; see `../Chorus/src/main/services/cliShimCore.ts`);
    `grok` is a native exe.
  - Use a minimal child environment (LESSONS L13) and auto-update off (LESSONS L7).
  - Measure UI frame time during a 10 MB output burst, resize, and Ctrl+C.
  - Prove that Close kills the whole process tree through a **Job Object**: list processes by `Name`
    and `CreationDate` before and after (LESSONS L12), and only ever kill your own PIDs.
- **S4 — DPAPI vault.** Use `CryptProtectData`/`CryptUnprotectData` (current-user) via the `windows`
  crate and round-trip a fake key. Launch a child with the key in its environment. Prove it arrived (the
  child reports its length or hash) and that it is absent from argv (`Win32_Process` by name, LESSONS
  L12). Wire the scrubber so the fake key printed by the child is redacted on screen.
- **S5 — Agent channels.** The most important spike. For each agent, fill in the capability matrix in
  ARCHITECTURE §5 with what the **installed version actually does**. Verify against each CLI's own docs,
  `--help` and files, never memory. Never run `claude help`.
  - **Claude Code:** hooks via a `--settings <file>` that POSTs to a loopback listener with a per-session
    token, using the `curl -o NUL -m 2` pattern (LESSONS L1–L2). Check whether the installed version has a
    native HTTP hook type; prefer it if so. Test whether a held-open **`PermissionRequest`** hook can
    approve or deny from Chorus: read the decision schema from the official hooks docs. Also test the
    `AskUserQuestion` events and the transcript tail.
  - **Codex:** hook event names and config, the rollout file tail (LESSONS L8), and `codex queue
    --thread <id> --message <text>`.
  - **opencode:** `opencode serve`, with the TUI in the PTY via `opencode attach <url>`. Test the event
    stream, replying to a permission, and `prompt_async`.
  - **Grok:** find out what it offers.
  - Send prompt text and Enter as two separate writes (LESSONS L4).
- **S3 — Fleet strip window.** Always on top and never activated: `GetForegroundWindow` must not change
  when it is clicked, so an external dictation app keeps focus. Draggable through a custom drag (LESSONS
  L11); measure with `GetWindowRect`. Correct across DPI scales.
- **S7 — 3D splash.** Prototype the concept in DESIGN.md's "Startup splash" with `three`: particles, one
  stream per restored session, converge into the seven-bar mark, which turns in 3D. First frame under
  ~300 ms, done in ≤3 s, skippable, with a reduced-motion version. It must not delay startup work.
  **Show Matthew** (screenshots, or ask him to run it) before calling it done; he wanted it exciting and
  eye-catching.
- **S8 — Installer.** Build a per-user NSIS installer with no admin rights. Confirm:
  - it installs side by side with Chorus 1 and shares no folders, data or shortcuts;
  - uninstalling leaves 2.0's data and never touches Chorus 1;
  - the WebView2 runtime is handled.
  Record the install path decision.

**Stop and ask Matthew** when:
- a spike fails in a way that changes the plan;
- you need a dependency not on the approved list;
- a test would spend API credit;
- anything would touch Chorus 1 or its data.

## Commits

- Commit after Part A, after Part B, and after each spike, with small focused commits on `main`.
  **Don't push**; Matthew pushes.
- Commit only the paths you changed: `git commit -- <paths>` or `git add <paths>`, never `git add -A`
  or `git add .`.
- **Don't modify, re-add or extract `_assets/Harness UI mockups.zip` into the repo.** It contains a
  screenshot of Chorus 1 showing client project names. The repo is public, and Matthew decides what
  happens to it.
- Before each commit, run `npm run grep:secrets` (once it exists) and check for local paths
  (`C:\Users\…`) in anything staged.
- End each commit message with the co-author line your harness gives you.

## Final report

Lead with the verdict in one line: is Phase 0 done, and is the plan intact? Then:
- a table with one row per spike (verdict, one-line finding);
- what changed in the docs;
- anything Matthew needs to decide or look at, including the splash.

Keep it short. Matthew wants the state of the world first and procedure only if it changes his next step.
