# Starting a loop run by hand

Until slice 5 builds kickoff into Chorus, a run's two documents are written in an ordinary Claude
Code session that reads the repository and grills Matthew, then committed on a branch the run
starts from. This page is that procedure, with the settings for running the loop on Chorus 2
itself (P12 and P14, the "real-world test" D102 holds them for).

## 1. Before the kickoff

- Commit what the repository's working copy holds that the run should see (for Chorus 2: the
  D102 changes to `PLAN.md`, `DECISIONS.md` and the hand-offs). The run starts from a commit, never
  from uncommitted files.
- Claude Code must trust the repository through its main working tree (D100): start Claude Code
  there once if it never has been.
- Settings → Phone: the bot's token, a paired phone, and the relay on. One bot is read by one
  Chorus at a time (409), so turn the relay off in any other Chorus first.

## 2. The kickoff session

In Chorus, start a Claude Code session in the repository (Manual permission, a strong model), and
send it this prompt, edited for the run:

> We're preparing an unattended Chorus Loop run on this repository. The loop builds the phases
> below one at a time: a planner writes each phase's spec and tasks, builders implement the tasks
> in a worktree on a branch of the run's own, and a phase passes when every check passes. Builders
> may only edit files in the worktree and run the check commands; they can't install anything, use
> keys or spend money. When stuck they ask me a question on my phone, so the fewer questions they
> need, the better.
>
> Phases: **P12 · Routing inspector** and **P14 · More agents and ship** (only what is left that
> needs no installs, keys, spending or release decisions; see D102).
>
> 1. Read `CLAUDE.md`, `_docs/PLAN.md`, `_docs/SCOPE.md`, `_docs/ARCHITECTURE.md`,
>    `_docs/DECISIONS.md`, `_docs/LESSONS.md`, and the hand-offs and reviews for these phases
>    (`_docs/HANDOFF-P14-*.md`, and whatever P12 has), then the code they touch.
> 2. Grill me: one question at a time, each with your recommended answer and why, until a builder
>    could do the work without asking. Cover what is in and out of each phase, acceptance
>    criteria a check can verify, wording rules for anything a user sees, anything that needs my
>    machine, accounts or money (it goes out of the run, or becomes a decision now), and how risky
>    each phase is.
> 3. Write `.chorus/run/roadmap.md` and `.chorus/run/decisions.md` in the formats below, show them
>    to me, and change them until I say they're right.
> 4. Then create the branch `loop/p12-p14` from `main`, commit only those two files there, switch
>    back to `main`, and don't push. Change nothing else.

### `roadmap.md`

```markdown
# Roadmap: <project>

## P12 · Routing inspector
- Goal: <one or two sentences: what is true when the phase is done>
- Risk: normal

## P14 · More agents and ship
- Goal: <...>
- Risk: high
```

Phases are `## Pnn · Title` (P01 to P99, any numbers, run in the order listed); `Risk` is
`normal` or `high` (a high-risk phase always ends at a milestone). An optional
`- Models: builder=<model>` line overrides a role's model for that phase.

### `decisions.md`

Builders read it before asking anything; a question it answers is never asked. Sections:

```markdown
# Decisions: <project>

## How to use this file
Read this before asking anything. If it answers your question, follow it.
If it doesn't, apply "Defaults when unsure", log the choice in assumptions.md, and keep going.

## Scope
## Stack and conventions
## Testing
## Wording
## Defaults when unsure
## Always ask
## Out of scope for this run
```

Chorus adds "Rules from your answers" below when a phone answer is made a rule.

## 3. The run

Loop → New run:

- **Folder:** the repository. **Base branch:** `loop/p12-p14`.
- **Roles:** a Claude Code Bot as planner and one as builder (loop steps run Claude Code only, for
  now).
- **Checks**, in this order; each is also a command builders may run while they work:

  | Name | Command | Timeout |
  |---|---|---|
  | deps | `npm install --prefer-offline --no-audit --no-fund` | 900 s |
  | bindings | `npm run bindings` | 2400 s |
  | typecheck | `npm run typecheck` | 600 s |
  | web tests | `npm test` | 600 s |
  | rust tests | `npm run test:rust` | 2400 s |
  | clippy | `npm run lint:rust` | 2400 s |
  | secrets | `npm run grep:secrets` | 300 s |

  The worktree starts without `node_modules` or a Rust build, so the first round is slow.
- **Milestones:** every phase (`1`), so each one waits for your review on the run branch.

## 4. What this run doesn't have yet

Slices 5 to 9 aren't built: no plan grill by a second model, no parallel builders, no reviewer
step (the milestone review is yours), no Counsel, and no allowance watch. A step that hits a
subscription limit fails like any other, and the run stops and tells the phone. Keep the computer
awake: nothing stops it sleeping mid-run.
