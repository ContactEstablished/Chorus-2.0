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

**Q19 — Codex plans.** (From spike S5.) In TUI mode Codex 0.162.1 exposes no plan: `update_plan` isn't
written to the rollout and has no hook. Codex's experimental `app-server` does emit `turn/plan/updated`,
and its TUI can attach with `--remote`, the same shape as opencode's serve + attach. *Default: P3 tries
app-server + `--remote` behind a capability flag; until then Friendly says "plan not available for
Codex".* Blocks: P6.

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
