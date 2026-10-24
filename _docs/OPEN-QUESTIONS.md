# Chorus 2.0 — Open questions

Questions that still need Matthew. When one is answered, it moves to [DECISIONS.md](DECISIONS.md) and
gets a row in the resolved table below. Each has a default that stands if he doesn't object.

## Still open

**Q18 — How should Chorus read Grok's state?** (From spike S5.) Grok 1.0.62 has Claude-style hooks
but loads them only from global files (`~/.grok/hooks/*.json`, `~/.claude/settings.json`, its
`config.toml`), not per launch; structured approval exists only over ACP (`grok agent stdio`), which
replaces the TUI. Options: (a) PTY only, plus Grok's OSC 7501 working/blocked/done status; (b) a
Chorus-owned `~/.grok/hooks/chorus.json` whose URL is `${CHORUS_HOOK_URL}`, which Grok skips when that
variable is empty, so it only fires for Chorus-launched sessions; (c) drive Grok over ACP and give up its
TUI. None could be tested: Grok isn't signed in on this machine. *Default: (a) for P2; revisit (b) once
Grok is signed in.* Blocks: P3.

**Q21 — How should Chorus run DeepSeek Harness?** (D59.) `dsh` offers a local web UI and the server
behind it (`dsh web`, port 3080), a one-shot headless run (`dsh --profile headless "<task>"`), a Python
SDK, and a terminal UI only through a community plugin profile (`dsh --profile tui`). Options: (a) its
terminal UI in the session's PTY, like every other agent, if the plugin works on Windows; (b) its own
server, driven by Chorus the way opencode is, with Chorus's Friendly view (P6) as the interface; (c)
headless runs only, for Automations and Bots (P8); (d) its web UI in a Chorus window. *Default: decide
after spike S9; likely (b) for supervised sessions plus (c) for Automations, since Chorus reads agent
state only from structured channels.* Blocks: the P14 `dsh` adapter.

**Q22 — Should opencode catch up after a dropped event stream?** (Hand-off oc2, L42.) A dropped stream
reconnects, and what it missed is lost: if a turn ended in the gap, the session shows Working until the
next turn ends. opencode 2.1.1 can't replay. Options: (a) leave it, because the stream is loopback to a
server that lives exactly as long as the run, and an idle one held for 10 minutes; (b) on reconnect, ask
`GET /api/session/active` and the session's outcome, close a turn that ended, and list pending
approvals and forms (about 50 lines in core plus one call), tested by unit tests only, since a drop
can't be provoked. *Default: (a), and (b) if a real drop is ever seen.* Blocks: nothing.

**Q23 — Should a resumed opencode run override an effort picked in its TUI?** (D60.) As built, a model or
effort picked in the TUI lasts until the next Restart or relaunch, which goes back to the session's
options, and the session's facts show the difference while it lasts. The alternative is to keep the
TUI's choice and only show the difference, which makes the session's options a starting point rather
than a setting. *Default: as built, which is what 1.x did.* Blocks: D60 becoming Decided.

**Q24 — Settings → Agents says versions "are tried and reported, never refused"**, and now shows an
opencode 1.x refused (D61). 1.x was checked and is known not to work, so the sentence still holds, but it
reads as a contradiction. Options: (a) leave it; (b) "Versions Chorus hasn't checked are tried and
reported; one it can't drive says why." *Default: (b), a one-line front-end change.* Blocks: nothing.

**Q25 — Sign the installer with Artifact Signing Basic, or stay unsigned for now?** (P14, D66.) (a) Sign:
$9.99 a month, individual validation in the US or Canada, minutes to set up once the Entra account
exists; the first downloads still warn. (b) Stay unsigned until there's something to distribute: $0,
and "Unknown publisher" on every download. *Default: (a) when the first public release is near, not
sooner.* Blocks: D66.

**Q26 — Wire `tauri-plugin-updater` now, switched off?** (P14, Task 10.) (a) Yes: the plugin loads with
an empty endpoint and key, and turning it on is two values later. (b) Later: no dependency until there
is a release to update. *Default: (a).* Blocks: nothing.

**Q27 — Gemini CLI's auto-update: leave it on, or have Chorus turn it off?** (P14, L46.) (a) Leave it:
0.64.0 has no switch per launch, and the only one is the user's settings file, which Chorus mustn't
write. An update shows as a newer version at the next launch. (b) Pass `GEMINI_CLI_SYSTEM_SETTINGS_PATH`
on each launch with auto-update off, which also replaces any real system policy file on the machine.
*Default: (a).* Blocks: nothing.

## Resolved 2026-10-18

Matthew kept the name Chorus, accepted every recommended default, and moved voice to a separate
dictation app (D22, D23).

| Question | Answer | Decision |
|---|---|---|
| Q1 Is the Harness mockup the 2.0 UI? | Yes; nothing else is pending | D9 |
| Q2 Name and logo | Chorus, with the seven-bar mark | D24 |
| Q3 Themes | Glacier (default) and Dusk | D25 |
| Q4 Where do filmstrip and grid go? | Session strip, and a Wall layout under Sessions | D26 |
| Q5 Pop-out windows | Deferred | D27 |
| Q6 What to keep of responses | Final message per turn, tool-call list, asks with their answers; transcripts stay in the agents' files with paths recorded | D7 |
| Q7 The narrator | A cheap OpenRouter model chosen in Settings, on by default, switchable off per project. The JEV half is Q7b | D28 |
| Q8 Approving from 2.0 | Yes, if spike S5 proves it reliable; the agent's own prompt is the fallback | D29 |
| Q9 Dependencies | ARCHITECTURE §10 approved, minus the voice items | D30 (and D4, D12) |
| Q10 Mission Control | Overview board replaces it; forecasts only after turn capture is ≥95% complete | D18 |
| Q11 Skills framework | Folds into Bots and Automations | D31 |
| Q12 Notes as the memory replacement | Yes, plain notes, no graph | D17 |
| Q13 Repo visibility | Public (confirmed on GitHub) | D32 |
| Q14 Windows only? | Yes, for v1 | D21 |
| Q15 Import projects from Chorus 1? | Yes: one-time and read-only, a small P4 task (dropped on 2026-10-20: D50) | D33 |
| Q16 Automation triggers | Schedules first; GitHub triggers later via `gh` polling | D34 |
| Q17 Remote control alternatives | Noted; decided at P13 | D35 |

## Resolved 2026-10-19

| Question | Answer | Decision |
|---|---|---|
| Q7b What is the JEV AI key for? | TypeSafe's Jev API: typed decisions with calibrated probabilities, not text. Stored with a connection test in P2; uses decided per phase | D40 |

## Resolved 2026-10-20

| Question | Answer | Decision |
|---|---|---|
| Q20 What is 2.0 called once installed beside Chorus 1? | "Chorus 2": its own folder, shortcuts and Apps entry; the window still says Chorus. The "2" goes when Chorus 1 is retired | D51 |

## Resolved 2026-10-22

Implemented the recommended defaults while following the accepted roadmap recommendation. Runtime
acceptance of complete Gemini/Aider turns remains P14 work; resolving these questions does not close it.

| Question | Answer | Decision |
|---|---|---|
| Q28 Gemini folder trust | Read existing trust; inherit repository trust only for managed worktrees; explicit refusals win | D69 |
| Q29 Aider permission labels | Accept edits and Full access; migrate old Manual labels without changing arguments | D70 |
| Q30 Aider history isolation | Per-session chat/input files; keep old repository history without automatically importing it | D71 |

## Resolved 2026-10-23

| Question | Answer | Decision |
|---|---|---|
| Q19 Codex plans / Friendly review | Matthew accepts the current Friendly display and approves Q19 with remaining Friendly features understood. Prompting from Friendly is still required; implement the composer and durable queue next. Codex plan/control verification remains implementation work before enabling its bridge, not an outstanding user approval | D77 |

### Q19 evidence and remaining implementation work

Approval records the user's acceptance of the current display, not a successful plan/control test.
P6 and its Friendly-only exit checks remain open. Historical protocol findings follow:

**Q19 — Codex plans.** (From spike S5.) In TUI mode Codex 0.162.1 exposes no plan: `update_plan` isn't
written to the rollout and has no hook. Codex's experimental `app-server` does emit `turn/plan/updated`,
and its TUI can attach with `--remote`, the same shape as opencode's serve + attach. *Default: P3 tries
app-server + `--remote` behind a capability flag; until then Friendly says "plan not available for
Codex".* Previously blocked: P6; approval resolved by D77.

2026-10-22 follow-up on Codex 0.163.0: `app-server --listen`, `--ws-auth capability-token`,
`--ws-token-sha256`, TUI `--remote` and `--remote-auth-token-env` exist. A private server completed
initialisation and a tiny subscription turn; its authenticated TUI reached the scratch-folder trust
prompt through ConPTY. Generated schema includes `turn/plan/updated`, but the tiny turn emitted no
plan notification. **Remaining implementation verification:** observe real plan updates and prove both clients control the
same thread, with approval/question replies, disconnects and Raw fallback. No production bridge
enabled yet. Details and remaining gates: [S5-P6.md](spikes/S5-P6.md).

A further explicit plan-tool probe completed without a plan event. A diagnostic turn using the
actual default `gpt-6-astra` reported `update_plan` unavailable and named `request_user_input`
as a Plan-mode tool. This is bounded evidence for that configuration, not a universal absence
claim. See [the next slice's findings](spikes/S5-P6-REPLIES.md); the Codex exit check stays open; Q19 approval was subsequently recorded in D77.

A Plan-mode follow-up on the same CLI/default model observed a structured question,
accepted its answer and received `serverRequest/resolved`. It still emitted no progress
plan update. Question control and plan capture are distinct capabilities; authenticated
same-thread Raw/controller verification remains open. See [mode evidence](spikes/S5-P6-Q19-MODES.md).
