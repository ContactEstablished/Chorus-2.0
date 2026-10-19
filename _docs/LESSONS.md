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
