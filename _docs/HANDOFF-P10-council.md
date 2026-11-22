# P10 — Council, standalone, with read-only code access

Decided 2026-11-21 (D136–D138). Chorus 1's Council review, ported to 2.0 as a module of its own:
choose a project, choose a brief, and a panel of models answers its numbered questions, critiques
each other anonymously, and an arbiter rules and writes the findings. New in 2.0: every participant
can **read the project's code** (read-only) to check the brief's claims for itself.

The loop's Counsel (P15 slice 8) will call this module rather than build a second one (D136).

Reference: Chorus 1 at `../Chorus` — `councilCore.ts`, `councilService.ts`, `councilMembers.ts`,
`councilDocketCore.ts`, `stores/council.ts`, `views/CouncilView.vue`.

## 1. What carries over unchanged

- **The protocol.** Round 0 *positions* (blind, members only, in parallel); round 1 *critique* (each
  member sees the others' positions relabelled Position A, B…, in parallel); round 2 *arbitration*
  (the arbiter alone, attributed, with the measured verdict vector); round 3 *synthesis* (the arbiter
  writes the findings body). One pass of each. Floors: fewer than two answered positions aborts; an
  arbiter refusal at arbitration or synthesis aborts (no fallback arbiter).
- **Verdict tokens.** Members: `Qn: AGREE | DISAGREE | QUALIFY`, first token per question wins.
  Arbiter: a `## Verdict` block of `Qn: APPROVED | APPROVED-WITH-REVISIONS | REVISE | REJECTED |
  INSUFFICIENT-INFORMATION`. Prose is never read as a ruling. Member consensus and the arbiter's
  ruling are stored and shown separately.
- **Dissent enforced in code.** Structural dissents from the members' own tokens, critique dissents
  from `DISAGREE:` / `OBJECTION:` / `MISSED:` labels (a closed "nothing" list is dropped), and the
  `## Dissents preserved` section appended to the findings by code whatever the arbiter wrote.
- **Brief parsing.** The first heading whose subject contains "questions" scopes the questions
  (whole document if none; an empty questions section refuses). Lines matching `1.` / `1)` of at
  least 8 characters. 200 KB limit. A brief with a known credential shape is refused before any
  model reads it.
- **Findings document** layout: header (or PARTIAL banner), standing caveat, synthesis, how
  disagreement was detected, elision note, dissents preserved, provenance. Written beside the brief
  as `<stem>-Findings.md`, never overwritten (`-Findings-2.md` … `-99`).
- **Docket**: a per-project history of runs, newest first, with status, duration, turns, tokens,
  cost and a verdict digest; reopen a run's findings, transcript and verdict; remove a run.

## 2. What changes from Chorus 1

| Chorus 1 | 2.0 |
|---|---|
| Members judged from the brief alone | Each participant may call read-only code tools in every round (§3) |
| Critique prompt had no brief or questions | Critique prompt carries the brief and the questions |
| Prompts not stored | Every prompt is stored, scrubbed, before it is sent |
| Cost read before revoking (35–40 % low) | Per-turn `usage.cost` summed, and the run key's cost read from analytics after revoking (L16); the larger is shown, labelled with its source |
| Cancel recorded partial answers as answered | A cancelled turn is recorded as cancelled |
| No single-flight guard; runs could have no project | One run at a time, enforced in Rust; every run belongs to a project |
| Members' free-form parameters stored but never sent | Removed. A member is provider, model, role, max output tokens |
| `canRun` ignored unavailable members | Run is disabled with the reason shown |
| Cases and exhibits designed, never built | Not built: code access replaces them |
| Streaming text deltas | Non-streaming calls in a tool loop; progress events per tool call and per answer |

## 3. Read-only code access (D137)

Participants are OpenRouter chat calls, not agent sessions. Chorus serves the code itself through
three function tools; Rust executes every call.

- `list_files(path?)` — the allowed files under a folder (relative), with sizes; at most 500 rows.
- `read_file(path, start_line?, end_line?)` — numbered lines; at most 64 KB per call.
- `search_code(query, path?, case_sensitive?)` — literal substring search across allowed files; at
  most 200 matching lines, each clipped.

**Which files.** The project's folder as it is now (uncommitted changes included). In a git
repository the allowed set is `git ls-files --cached --others --exclude-standard`, taken once at run
start; outside git, a walk that skips `.git`, `node_modules`, `target`, `dist`, `build`, `bin`, `obj`
and other build output. Then always blocked: `.env*`, key and certificate files (`*.pem`, `*.key`,
`*.pfx`, `*.p12`, `id_rsa*`, `id_ed25519*`…), `.npmrc`, `.netrc`, `.pypirc`, `credentials*`,
`secrets*`, anything under `.git/`, and the run's own findings file. Every `.env…` and `….env` name is
blocked except `.env.example`, `.sample`, `.template` and `.dist`. Symlinks are never readable, and a
read whose real path resolves outside the project is refused. Binary and files over 2 MB are
refused by reason; a line over 4,000 characters is cut with a note. Paths are relative, `..`, absolute
and drive paths refused (pure check in `chorus-core::council::files`).

**Scrubbing.** Every tool result passes the scrubber (vault values and known shapes) before a model
sees it, and is stored scrubbed.

**Budgets per participant turn** (Council settings, defaults): 40 tool calls, 1.5 MB of tool output,
and never more than ~70 % of the model's context length; when any is reached the next request has
`tool_choice: "none"` and a note to answer now. Run key cap: **$10** by default.

**Who reads:** every round, arbiter included.

**Tool support.** A member whose model does not list `tools` in OpenRouter's catalog is refused at
setup and at run start, with the reason. Never run silently without code access.

**Mandate.** Each prompt tells the participant it can read the project and must check the brief's
claims against the code before answering, citing `path:line` for what it verified, and saying when
it could not verify something.

The standing caveat becomes: *"These findings are model deliberation, not verified fact. The
members could read the project's code, but nothing was compiled, run or tested. Verify anything you
are about to rely on."*

## 4. Shape in 2.0

- `crates/chorus-core/src/council/` — pure: `brief` (questions, credential scan), `verdict`
  (member and arbiter tokens, measurement, summary), `dissent`, `prompts`, `findings`, `protocol`
  (the next action from the transcript), `files` (path policy, blocked names), `tools` (tool
  definitions, argument checks, output formatting), `docket` (digest).
- `src-tauri/src/council/` — `mod.rs` (commands), `run.rs` (the worker: key, rounds, tool loop,
  settle, findings write), `code.rs` (the allowed file set and the three tools), `store` queries in
  `src-tauri/src/store/council.rs`. Migration `0032_council.sql`.
- Keys: one capped key per run from the chosen OpenRouter provider's management key, using the
  existing ledger (`capped_keys`, `session_id = 0`, linked from `council_runs.key_ledger_id`),
  revoked at the end and swept at start-up. The minted key goes only in the `Authorization` header
  of Chorus's own HTTPS calls (D138).
- UI: a **Council** rail item (Docket → New council → run surface) and **Settings → Council**
  (members, budgets).
- Events: `CouncilUpdated { runId }` — the screen re-reads the run; no text is pushed.

## 5. Slices (each seen working in the app; commit after each)

1. **Core**: the pure module and its tests, ported from Chorus 1's tests.
2. **Members and settings**: migration, store, member commands, Settings → Council.
3. **The run**: key, rounds, tool loop, code tools, scrubbing, persistence, cancel, findings file,
   settle, start-up heal.
4. **The Council screen**: Docket, new run, live progress, transcript, verdict strip, findings.
5. **Seen working**: a real run on this repository with three members and an arbiter.

Slices 1–4 built 2026-11-21 on `feature/council`. Slice 5: run 1 (dev build, project Chorus 2.0,
DeepSeek v4.1 flash, Qwen 3.7 flash and GLM 5.3 flash, Claude Haiku 5.5 as arbiter, $2 cap) completed
in 4 minutes for $0.049; every member rejected the brief's planted false claim by reading the code,
and the findings file was written beside the brief. Its findings about the code tools were fixed the
same day (LESSONS L86). Not yet seen in the app: choosing a brief through the native file dialog
(the commands behind it were driven directly), and Settings → Council used by hand.

## 6. Out of scope

Cases and exhibits; streaming deltas; per-member parameters other than max tokens; P15's Counsel
triggers (later, calling this module).
