# Chorus 2.0 — Open questions

Each question has a recommended default. If Matthew doesn't object, the default stands and the answer
moves to [DECISIONS.md](DECISIONS.md). **Blocks** says the earliest phase that needs the answer.

## Needed before P0 starts

**Q2 — Name and logo.** The mockup says **Harness** with a rotated-square glyph; the repo is Chorus 2.0
and Chorus has an official seven-bar mark. The name sets the app identifier, install folder and window
title. *Default: Chorus, with the seven-bar mark.* Blocks: P0.

**Q9 — Dependencies.** CLAUDE.md requires approval before adding dependencies. The proposed list is in
[ARCHITECTURE.md §10](ARCHITECTURE.md#10-proposed-dependencies-pending-approval--open-questions-q9).
Two are judgement calls: `tauri-specta` (generated TypeScript bindings) and `three` (3D splash).
*Default: approve the list as written.* Blocks: P0.

**Q13 — Repo visibility.** Chorus 1 is public. If 2.0 is too, test evidence, screenshots with local paths,
and machine-specific notes stay out of commits. *Default: assume public.* Blocks: first commit.

## Needed before the UI phases

**Q1 — Is the Harness mockup the 2.0 UI?** It changes the shape of the app: an Overview board first, and
sessions opened in a Friendly or Raw view, rather than a terminal grid first. It also adds Bots,
Automations, Documents and Notes. Is this the set of "other requirements" you mentioned, or are there
more? *Default: yes, the mockup is the direction; nothing else is pending.* Blocks: P4.

**Q3 — Themes.** *Default: ship Glacier (default) and Dusk.* Blocks: P4.

**Q4 — Where do filmstrip and grid go?** The mockup has no terminal grid. *Default: the session view gets
a collapsible session strip for fast switching (the filmstrip's job), and Sessions gets a "Wall" layout
of live Raw terminals (the grid's job).* Blocks: P4.

**Q5 — Pop-out windows.** The triage has no verdict for this one. *Default: out of scope for now. The
architecture allows it later (any window can attach to a session).* Blocks: nothing.

## Needed before P3 and P6

**Q6 — What to keep of responses.** *Default: keep every prompt in full; for each turn, keep the agent's
final message, the tool-call list (tool, target, outcome — not raw output), and any question or approval
with its answer. Full transcripts stay in each agent's own files, and 2.0 records their paths.* That gives
a complete, searchable history without duplicating megabytes of tool output. Blocks: P3.

**Q7 — The narrator, and what the JEV AI key is for.** Now / Blocker / Up next and suggested next steps
need a small model call at the end of each turn, which sends scrubbed excerpts of the session to a
provider. Which key should pay for it, and is that acceptable for all projects? Separately, what is
JEV / TypeSafe? If it's an LLM gateway, it could be the narrator's provider (and serve transcript cleanup
and day summaries). *Default: a cheap OpenRouter model chosen in Settings, on by default, switchable off
per project.* Blocks: P6.

**Q8 — Approving from 2.0.** For Claude Code, the candidate is that 2.0 answers the agent's permission
request itself (holding its approval hook open until you click), falling back to the agent's own prompt
in the terminal whenever 2.0 doesn't answer. *Default: yes, if spike S5 shows it's reliable.* Blocks: P6.

## Needed later

**Q10 — Mission Control.** *Default: the Overview board replaces it; cost and attention appear in session
facts and a Usage screen; forecasts are built only once turn capture is proven ≥95% complete.* Blocks: P11.

**Q11 — Skills framework.** *Default: folded into Bots (instructions plus provider-neutral skill folders)
and Automations (app-run work), not a separate feature.* Blocks: P8.

**Q12 — Notes as the memory replacement.** *Default: plain notes, project-tagged or global, injected into
each agent's instructions at launch; no graph.* Blocks: P8.

**Q14 — Windows only?** *Default: yes for v1.* Blocks: nothing now.

**Q15 — Import projects from Chorus 1?** Keys are re-entered (D5), but a one-time read-only import of
projects (path, name, colour, order) from Chorus 1's database would save setup. *Default: yes, as a
small P4 task.* Blocks: P4.

**Q16 — Automation triggers.** The mockup shows schedules and a "pull request opened" trigger.
*Default: schedules first; GitHub triggers later by polling with `gh`, since there's no server to receive
webhooks.* Blocks: P8.

**Q17 — Remote control alternatives.** Claude Code 2.1.300 has a built-in `--remote-control` mode, and
Codex has an experimental one. They could complement the Telegram relay for single sessions.
*Default: note it, decide at P13.* Blocks: P13.
