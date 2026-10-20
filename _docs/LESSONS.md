# Lessons carried over from Chorus 1

Facts that Chorus 1 paid for, which still apply after the move to Rust. Each was measured, not
remembered. Agent CLIs change fast, so re-verify anything version-specific against the installed
version before relying on it.

## Agent CLIs

**L1 — Inject Claude Code hooks with `--settings <file>`, never the repo's settings.** The flag is
additive. Writing `.claude/settings.json` puts 2.0's config in a tracked file inside a worktree the agent
may commit from. Use the **file** form: the inline JSON form puts the capability token in argv, which any
process can read.

**L2 — Hook stdout is a control channel.** Claude Code parses JSON printed by a hook command as a
*decision*. A fire-and-forget `curl` must use `-o NUL`, or the listener's reply is handed to the agent as
a decision it never made, on every tool call. Use `-m 2` too, because Claude Code **blocks** on hooks: an
unreachable listener must cost two seconds, not hang the session. Call `%SystemRoot%\System32\curl.exe`
by absolute path, because a bare `curl` can resolve to Git Bash's copy. Hook payloads carry Claude's own
`session_id`, not ours, so attribute events by the token in the URL.

**L3 — PTY silence separates idle from working, but only as a backstop.** Measured on Claude Code: idle
at its prompt is completely silent (79–92 s without a byte); a working turn writes every ≤435 ms. That
makes "no bytes for N seconds" a trustworthy signal that the agent **stopped**. It is never a "working"
signal: typing echoes, resizes repaint, and hook-less agents write constantly.

**L4 — Send prompt text and Enter as two separate writes.** A trailing `\r` in the same write is read as
part of a bracketed paste and lands as a newline in the input box; the agent never runs.

**L5 — opencode silently ignores an effort it doesn't recognise.** Wrong variant name, missing model
binding, or a model mismatch: no error, no log, clean exit. The only proof an effort applied is the stored
user message read back through `opencode serve` (`GET /session/{id}/message` → `model.variant`). Its TUI
also remembers the last variant per model in `model.json` under its state home, and that **overrides**
configuration. Isolate `XDG_STATE_HOME` and `XDG_DATA_HOME` in any test that launches it. Its TUI drops
keystrokes typed before the `Ask anything` prompt appears.

**L6 — Claude Code token accounting.** Subagent work lives in `<sessionId>/subagents/*.jsonl` beside the
main transcript, often 50–65% of the cost; reading only `transcript_path` under-counts. `usage.iterations[]`
repeats the parent totals, so sum the parent `usage` only. Cache reads dominate naive totals.

**L7 — CLIs update themselves, and exact version gates fail closed and silently.** In Chorus 1 all three
CLIs self-updated past their pinned versions and features quietly switched off. Launched agents get
auto-update disabled (`DISABLE_AUTOUPDATER=1` for Claude Code, `OPENCODE_DISABLE_AUTOUPDATE=true`, and for
Codex `-c check_for_update_on_startup=false`, since it has no env switch). An unknown version is **tried and
reported**, never disabled without a message. Never run `claude help <x>`: there is no `help` subcommand,
so it is sent as a prompt and spends a turn. Use `--help`.

**L8 — Codex rollouts.** `~/.codex/sessions/**/rollout-*.jsonl`. In code-execution mode every tool call
is an outer `exec` whose input is JavaScript; the real calls are `tools.<name>(…)` inside it, often
concurrent. Older rollouts use `function_call` and `local_shell_call` shapes; parsers must handle both.
Chorus-launched agents run in worktrees, so match the worktree root as well as the repo.

## Windows

**L9 — OS toasts are disabled on this machine** (`ToastEnabled=0`). That is why notifications are
in-app.

**L10 — Synthetic input is invisible to Windows.** CDP keystrokes do not reset the idle timer
(`GetLastInputInfo`) and do not trigger the browser's native paste. Prove idle, presence and clipboard
behaviour with real OS input (`keybd_event`), and expect other apps to steal the foreground mid-test.

**L11 — Three silent reasons a window won't move.** A caller re-placing it on every show; a non-movable
window ignoring `setPosition`; and drag regions doing nothing on a never-focused (`WS_EX_NOACTIVATE`)
window, because Windows won't run the caption move loop for it. Overlays need their own drag
implementation. Measure with `GetWindowRect`, not `window.screenX`.

**L12 — Prove argv contents by process name, not by command-line substring.** A
`CommandLine -like '*claude*'` filter matches the shell running the query and stale processes. Filter
`Win32_Process` by `Name`, sort by `CreationDate`, and compare argv lengths between control cases.

**L13 — Children get a minimal environment.** Copy an allow-list from the parent (`PATH`, `SystemRoot`,
`TEMP`, `TMP`, `HOMEDRIVE`, `HOMEPATH`, `USERPROFILE`), add entries only when something is observed to
break without them (and record what broke), and impose `TERM=xterm-256color` and `COLORTERM=truecolor`.
An over-broad allow-list leaks ambient credentials into agents.

**L14 — npm-installed CLIs are `.cmd` shims on Windows.** Resolve them deliberately (Chorus 1:
`cliShimCore.ts`) rather than hoping the PTY's spawn does what a shell would.

**L15 — Keep dev and installed data apart, and know which database is live.** Chorus 1 accumulated five
databases (including one shadowed by another app's container) and dev worktrees silently sharing one,
where migration numbers collided. Separate identifiers, and show the live DB path in the app.

**L21 — 2.0's binary name matches Chorus 1's for any name-based query.** Windows process names are
case-insensitive, so `Win32_Process` filtered by `Name='chorus.exe'` returns 2.0's dev build *and*
Chorus 1's `Chorus.exe` (five processes on this machine, 2026-10-18). Never kill or inspect "Chorus" by
name: filter by `ExecutablePath` or by a PID recorded at launch, and refuse any tree that contains a
process under Chorus 1's install folder.

**L22 — A Job Object also holds any browser an agent opens.** When an agent opens a URL (a sign-in
flow, "open in browser") and the browser isn't already running, the new browser is the agent's child
and joins the session's job; terminating the job closes the user's browser (seen with Grok's sign-in
and Brave, S1). Close must spare GUI processes with visible windows; only a crash of Chorus may take
everything down.

**L23 — Codex `-c` overrides switch off its shared background server.** codex-cli 0.162.1 warns that
any `-c`, `--enable`, `--disable` or `--search` on the command line forces "embedded mode". The
update-check switch Chorus needs (L7) is a `-c`, so Chorus-launched Codex always runs embedded.

**L24 — Claude Code 2.1.300 hooks, as measured (S5).** HTTP hooks work for every event except
`SessionStart`. A held `PermissionRequest` hook races Claude's own dialog, which appears ~0.3 s later;
the first answer wins, and a hook timeout leaves the dialog up. A hook *deny* with a message lets the
turn continue; the user picking "No" in the dialog interrupts it and sends **no `Stop`**.
`UserPromptSubmit` fires when a prompt is *queued*, before the previous turn's `Stop`. The todo/task
tools are off unless `CLAUDE_CODE_ENABLE_TODO_TOOLS=1` (undocumented). On Windows the shell tool is
`PowerShell`, not `Bash`, so permission rules keyed on `Bash` miss it.

**L25 — Codex 0.162.1 hooks need `PATHEXT`, and its rollouts omit plans and approvals (S5).** Without
`PATHEXT` in the environment every hook fails with "hook exited with code 1". Hooks can be injected
with `-c hooks.<Event>=…` plus `--dangerously-bypass-hook-trust` (both force embedded mode).
`codex queue` reaches an embedded session. `update_plan` and approval requests are never written to the
rollout.

**L26 — An agent CLI launched without its update switch updates itself (S1, S5).** Chorus's first
Grok launch replaced Grok 1.0.5 with 1.0.62 (L7 again) and cleared an expired sign-in. Grok 1.0.62
documents `GROK_DISABLE_AUTOUPDATER=1` and `--no-auto-update`. This machine's Grok config also sets
`permission_mode = "always-approve"`, so Chorus must always pass `--permission-mode` explicitly.

**L27 — opencode serve is a complete control plane, and its free provider can stall (S5).** Events,
questions, permissions, todos and queued prompts all work over the server API with the TUI attached.
Basic auth comes from `OPENCODE_SERVER_PASSWORD` (environment). The keyless free model once took five
minutes to time out a request before opencode retried; a stall is not a failure signal by itself.

**L28 — `WS_EX_NOACTIVATE` does not stop a WebView2 window taking focus (S3).** A click lands on
WebView2's `Chrome_RenderWidgetHostHWND`, which lives in the browser process and activates its
top-level ancestor whatever that window's styles say. A never-activated web window needs a native child
window over the content that answers `MA_NOACTIVATE` and forwards input (D38). Also: build windows in
async commands; building one inside a synchronous command deadlocks on Windows.

**L29 — Tauri's installer names shortcuts after the product name (S8).** With "Chorus" it writes
`Chorus.lnk` to the Start menu and desktop, the same paths as Chorus 1's shortcuts. Its "is the app
running?" check uses the Restart Manager on the full exe path, so it cannot hit Chorus 1. `/NS` installs
with no shortcuts; uninstall keeps app data and leaves `HKCU\Software\<manufacturer>\<product>` behind.

**L30 — Claude Code's PowerShell permission parser fails with the Microsoft Store `pwsh`.** Claude Code
2.1.300 finds `pwsh` on `PATH` and passes it a ~10,300-character `-EncodedCommand` to parse every
PowerShell command. When the first `pwsh.exe` on `PATH` is the Store app alias
(`%LOCALAPPDATA%\Microsoft\WindowsApps\…`), every parse fails with "The command line is too long."
That error is cmd.exe's, and so is its 8,191-character limit; the launch evidently goes through
cmd.exe, inferred, not traced. Every command is then reported as "malformed syntax", so allow rules
never match and every command asks. With a real `pwsh.exe` first on `PATH`, the same allow rule
matched and the command ran (2026-10-19). The Store `pwsh` also puts its own alias folder at the front
of `PATH` for everything launched from it. Chorus should warn when `pwsh` resolves to an app alias.

**L31 — Codex's legacy Windows sandbox fails while a file it must ACL is running.** Codex 0.162.1 with
`[windows] sandbox = "elevated"` runs a setup helper before each sandboxed command. The helper updates
ACLs on the desktop app's runtime, including `cua_node\…\node_repl.exe`, which the shared
`config.toml` also starts as an MCP server. While that file is running the update fails with "os error
32" and every command fails with "helper_unknown_error: setup refresh had errors". The cause is not the
shell. `[features] prefer_mxc = true` (MXC, which the desktop app already uses) fixes it with the
elevated setting kept as fallback. Under MXC, `read-only` blocked writes, and `workspace` allowed
writes inside the workspace and blocked them outside. `codex sandbox -P :read-only -C <dir> -- <cmd>`
tests the sandbox without a model turn.

**L32 — ConPTY waits for the terminal before drawing anything (P1).** portable-pty 0.9 creates the
pseudoconsole with `PSEUDOCONSOLE_INHERIT_CURSOR`. The first output is a cursor-position request
(`ESC[6n`), and ConPTY sends nothing more until it gets an answer. In S1, xterm.js answered it. A
session with no view attached, which includes every session relaunched at boot, would hang with the
request as its only output. Chorus answers the request itself and removes it from the stream
(`chorus_core::conpty`); otherwise xterm.js answers a second time and its reply arrives as typed
input. The answer has to be true, so every run starts at the top-left of a blank screen: a new
terminal, or the one left after a Restart or relaunch (L34).

**L33 — Claude Code 2.1.300's folder-trust prompt defaults to "No, exit" (P1).** In a folder it hasn't
seen, Enter declines, and the session exits with code 1. P2's Claude adapter should recognise that
prompt and say what it is, not just show "exited (1)".

**L34 — ConPTY erases on-screen text it didn't draw (P1).** ConPTY keeps its own copy of the screen
and repaints rows from it. Text that Chorus adds to a session's stream inside the viewport is erased
the next time ConPTY repaints that row. The first version put the dividing line on row 1 after a
relaunch, and Claude Code 2.1.300 caused a repaint from the top-left (`ESC[H ESC[K`) that erased it.
That version also scrolled 60 newlines to move the old screen into scrollback, which left 45 to 60
blank lines after each run's output, so restored history looked lost. Now the restart screen pushes
the old screen, and then the dividing line, into scrollback with Erase in Display, and the run starts
on a blank screen. xterm.js 6 keeps erased rows only with `scrollOnEraseInDisplay`, and then only up to
the last row with text.

**L35 — With `opencode serve` + `attach`, the server's configuration decides a typed prompt's model
and effort (P2, opencode 1.19.3).** Measured with the TUI attached in Chorus's PTY, on free models:
- The attached TUI **ignores the model stored on a session** (`POST /session` with `model`). It uses
  the server's configuration: `model`, and `agent.<name>.variant` beside `agent.<name>.model`. Chorus
  sets both through `OPENCODE_CONFIG_CONTENT` on the server.
- Each model has its own variants (`GET /provider` → `all[].models[id].variants`):
  `glm-5.3-flash-free` has low, high and max; `big-pickle` has none. A variant the model lacks is
  dropped in silence: the stored message has no variant (L5). Only the read-back shows it
  (`GET /session/{id}/message` → the user message's `model.variant`).
- After a prompt, the TUI writes `variant[<provider/model>]` to `opencode/model.json` in its state
  folder. That remembered choice beats the configuration on the next start (L5), so Chorus deletes the
  file from the session's own state folder before each run that sets a model or effort.
- `serve --port 0` tries 4096 first, so the port must be read from its "listening on" line.
  `GET /session/{id}/message?limit=N` returns the newest N, oldest first; a missing session is 404.
- Permissions: `permission` is one action (`"allow"`) or one per tool (`edit`, `bash`, `webfetch`,
  `external_directory`…). With `edit: "ask"` the TUI showed "Permission required" and wrote nothing.
  `default_agent: "plan"` starts the TUI in its plan agent.

**L36 — Agents on OpenRouter, measured in the real app (P2, 2026-10-19).**
- Codex 0.162.1 runs a provider model through `-c model_providers.<id>.*` with the Responses wire API.
  For a model it has no metadata for, it warns after the first turn, "Model metadata for `<model>` not
  found. Defaulting to fallback metadata". Passing `-c model_context_window=<n>` from the catalog
  stops the warning.
- opencode 1.19.3 knows 385 OpenRouter models from its own list, but not the newest
  (`minimax/minimax-m3.1-flash:free` was missing). A `provider.openrouter.models.<id>` block in the
  configuration makes it run one, and variants declared there (`{"<e>":{"reasoning":{"effort":"<e>"}}}`)
  apply: the read-back showed `medium`.
- OpenRouter's `POST /keys` returns the plaintext `key` only in that answer, with `data.hash` naming it
  afterwards. `expires_at` must include seconds. A management key answers `GET /keys`, but can't run
  models. Free models cost $0 in analytics shortly after a run.
- `tauri dev` kills the app on a rebuild, without a clean quit, so a run's key isn't revoked then. The
  start-up sweep revoked it.

**L37 — Agent events, measured in the real app (P3, 2026-10-19).**
- **Codex 0.162.1 runs command hooks under PowerShell here** (the relay saw `PSModulePath`), not
  `cmd` as its source's default suggests. A quoted path followed by an argument is then a parse error,
  so every hook "exited with code 1" and never started the relay. Give the relay unquoted (its 8.3
  short path if it has spaces).
- **Codex unescapes a `-c` value's backslashes twice:** `C:\\Projects\\x.exe` inside the TOML string
  reached the shell as `C:Projectsx.exe`. Use forward slashes in paths passed through `-c`.
- Codex's `/hooks` screen lists hooks from `-c` as "Session flags" needing review even with
  `--dangerously-bypass-hook-trust`; they still run. It shows each hook's command as parsed, which is
  the quickest check. Its `Interrupt` hook fires on Esc and carries the `turn_id`, so an interrupted
  turn closes at once.
- **Claude Code's `prompt_id` is the same on `UserPromptSubmit` and its `Stop`**, and Codex's
  `turn_id` likewise, so turns pair exactly. Claude Code sends nothing when a turn is interrupted. Its
  transcript gets a user message `[Request interrupted by user]` at once (watched every 2 s), and
  `Notification: idle_prompt` comes after about a minute.
- **opencode sends a finished assistant message twice** on its event stream. Ending a turn once per
  assistant message id avoids a second, promptless turn. A tool part's input is empty on its first
  event and filled in later.

**L38 — Tauri's `externalBin` is checked by every cargo build, not only when bundling.** With
`bundle.externalBin` in the configuration, `tauri-build` fails any `cargo build`, `test` or `clippy`
with "resource path `binaries\<name>-<target triple>.exe` doesn't exist" until that file is there
(Tauri 2.12.4). 2.0 therefore keeps it in `src-tauri/tauri.bundle.conf.json`, which only
`npm run build` uses; its `beforeBuildCommand` builds `chorus-hook` and copies it in, and the installer
ships it beside `chorus.exe` as `chorus-hook.exe`. Dev builds keep building it beside the debug exe.

**L39 — git on Windows, for worktrees (git 2.50.0, 2026-10-20).** `git rev-parse --show-toplevel` prints
forward slashes (`C:/work/ledger`); `--git-common-dir` is relative unless `--path-format=absolute`
(git 2.31+) comes first, and with it one call gives the tree, the repository's `.git` and, with
`--abbrev-ref HEAD`, the branch (`HEAD` when detached). `git worktree remove` without `--force` refuses
untracked files as well as changes, and works a moment after the session's process tree ends. A new
worktree is a folder Claude Code hasn't been told to trust, so its trust prompt appears every time.

## Money and models

**L16 — Read-then-revoke under-reports OpenRouter spend.** Reading a capped key's usage right after the
last stream misses the final turn, because OpenRouter hasn't posted it yet. Revoke immediately, then
reconcile from analytics. Chorus 1's council reported about half its true cost for this reason.

**L17 — The OpenRouter catalog is the model authority.** `GET https://openrouter.ai/api/v1/models` is
free and has prices, reasoning efforts, max output tokens and benchmark indices. A `-pro` suffix is the
same model at the same rate with more reasoning, so it costs more per run.

**L18 — Council briefs are read literally.** Every line is treated as addressed to the council: state the
task ("answer these questions; do not review this document"). Numbered lists inside the questions section
multiply the question count. Test the real parser on a brief before a paid run.

## Process

**L19 — Measure adoption before building on it.** The memory graph had zero agent writes; Team sessions
were slower and costlier than one agent; Mission Control's forecasts had 5 of 172 completions as input.
Each was found only by counting. 2.0 counts first (P3's completeness bar) and builds second.

**L20 — Runtime-verify claims, including your own.** Implementer summaries in Chorus 1 sometimes came from
another environment. A phase is done when the real app is seen doing the thing.
