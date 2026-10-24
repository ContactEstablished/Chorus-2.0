# P6–P8: what is waiting for Matthew

One list of the decisions and checks the `evening/p6-p8` branch needs before it merges. The detail is
in [the P7 review](HANDOFF-P7-REVIEW.md) and [the P8 review](HANDOFF-P8-REVIEW.md); this page only
gathers them, with a recommendation each. Nothing here is decided until Matthew says so.

## Decisions

| # | Question | Recommendation | Where |
|---|---|---|---|
| 1 | Keep Codex instructions over the app-server (`thread/inject_items`)? Seen working; Friendly waits for it, and only text typed in Raw within ~0.6 s of Codex's input box appearing could arrive first (P8 §12). They survived a forced `/compact` (P8 §12); an automatic compaction late in a long session is still unmeasured | Keep it | P8 §9 Q1 |
| 2 | Automation sessions stay open after their turn, one more every run | A per-automation "close when the first turn ends", on by default | P8 §9 Q2 |
| 3 | What counts as "still active" when runs overlap | Busy only: working, waiting for you, or not past its first prompt | P8 §9 Q3 |
| 4 | Native Claude skills through `--plugin-dir` (not built or tried) | Try after this merges, behind the same carrier table | P8 §9 Q4 |
| 5 | Codex automations | Claude Code only for now; Codex later, with the bridge sending the first prompt | P8 §9 Q5 |
| 6 | Each window may call only its own commands (P7-D5); today the strip and splash can call all 100+ | Do it: the strip and splash need four commands between them | P7 §8 |

Smaller questions, each with a recommendation in place: approval notification bodies naming the
ask's subject (P7 §9 Q1, recommended yes) and keeping `fleet_probe` in release builds (P7 §9 Q2,
recommended keep). The proposed decision rows (P7-D1…D5, P8-D1…D6) and lessons (P7-L1…L4, P8-L1…L4)
are written in DECISIONS.md and LESSONS.md style for the reviewer to number.

## Checks that need you

- **An OpenRouter key, entered through the app** (Settings → Providers), to see the narrator make a
  real call; the cap is $3.
- **opencode and Grok**: installing them needs your yes; their shortcut bindings and launches are
  untested here.
- **Another display scale or a second monitor**, and **swapped mouse buttons**: steps in P7 §11.

## Then

Review, and merge `evening/p6-p8` into `main`. Your uncommitted D83 work in the main checkout was
left untouched.
