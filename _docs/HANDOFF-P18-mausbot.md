# Plan: P18 — Working alongside OpenMausBot

Status: **first pass built** on `feature/mausbot-handoff`, 2026-11-18: 18.0–18.3 and 18.5, each
seen in the isolated `p18` instance. Not yet seen: MausBot starting a hand-off by itself, which needs
Chorus paired with a full-access code (D134). 18.4 (MCP) is deferred (D132). The task and spec files
are in [P18/](P18/Phase-18-Overview.md). Decisions D128–D134.

## 1. Goal

Matthew uses OpenMausBot (MausBot) on his phone and laptop over Tailscale, and Chorus at his desk.
P18 lets the two hand work to each other without either one swallowing the other:

- Chorus keeps a **hand-off note** per project: what was done last, where things stand, what's next.
- Work passes with a **baton**. MausBot always works on its own branch, in a worktree Chorus makes,
  so it never edits the checkout Chorus's sessions use.
- When the baton comes back, Chorus shows what MausBot did and lets Matthew carry on from there.
- **Branches don't pile up:** Chorus says which branch MausBot used, and flags any branch unused
  for 30 days. Deleting a branch is always Matthew's click.
- Later, MausBot may **read and prompt** Chorus sessions through a small, local MCP server, so a
  message from the phone can reach a session on the desk PC. That is deferred (D132).

The two apps stay separate programs. Chorus does not ingest MausBot's code (§6).

## 2. Decisions (Matthew, 2026-11-18)

- **D128, the hand-off note lives in the project folder**, at `.chorus/handoff.md`. Git ignores it
  through the repository's local `.git/info/exclude`, so Chorus never edits a tracked `.gitignore`.
- **D129, MausBot is the phone route.** Matthew stops using the Telegram relay. The relay stays built
  but off, and is not removed. This partly supersedes D16 ("Telegram is the only remote access").
  Chorus itself stays loopback-only: MausBot handles Tailscale, and Chorus is never reachable from
  the network.
- **D130, Chorus sets up MausBot's branch** (option A). "Hand to MausBot" makes a worktree on a
  `maus/…` branch and points MausBot at it. MausBot's bots have no branch or worktree support of
  their own, and work directly in whatever folder they are given (verified in 0.1.169, §3).
- **D131, branch hygiene.**
  - Chorus names MausBot's branch when the baton passes, and says when the branch is merged and
    can go.
  - Every branch in a project's repository with no commit for 30 days gets one quiet notice,
    offering Delete or Keep. Keep snoozes the notice for 30 more days.
  - Chorus never deletes a branch on its own. A branch with unmerged commits needs a second,
    explicit confirmation.

## 3. Ground facts (verified 2026-11-18; re-check before relying on them)

### OpenMausBot 0.1.169
Installed at `%LOCALAPPDATA%\Programs\openmausbot`, with its data in `~\.openmausbot`. These facts
come from reading the installed program's source. They are not from a live test.

- **Local API.**
  - The app API is on `127.0.0.1:8799` and the webhook receiver on `:8800`. Both listen on
    loopback only, unless `OMB_LAN=1` is set.
  - Loopback requests with no browser headers can read. Sending a message needs a token:
    `openmausbot token create NAME --out FILE` (scope `client`).
- **CLI.** `openmausbot bots`, `send BOT "…"`, `messages BOT`, `run BOT "…"`, `wait BOT`,
  `events --job THREAD`, `export BOT --out DIR` and `interrupt BOT`. URL and token come from
  `--url`/`--token-file`, or from `OPENMAUSBOT_URL`/`OPENMAUSBOT_TOKEN`.
- **MausBot's own MCP server.**
  - Started with `openmausbot mcp` (stdio).
  - It has 20 tools, among them `list_bots`, `send_bot_message`, `get_bot_messages`,
    `create_task`, `switch_task`, `wait_for_conversation` and `interrupt_conversation`.
  - Its mutating tools need `OPENMAUSBOT_TOKEN`.
- **User-added MCP servers.**
  - Stored in `~\.openmausbot\config.json` under `mcpServers`. Adding one through MausBot's UI
    (Apps → MCP servers) or its API takes effect at once. A hand edit needs a MausBot restart.
  - Supported kinds: stdio (`command`, `args`, `env`), Streamable HTTP (`type: "http"`, `url`,
    `headers`) and SSE.
  - An entry can belong to one bot (`bot`), or bots can allow-list servers (`mcpServers` in
    `bots.json`).
  - Secrets can be kept as `vault:<id>` pointers. Credentials go in headers, never in the URL.
- **Where bots work.** In their configured folder (`task.cwd ?? bot.cwd ?? task-workspaces/…`).
  There is no `worktree add` and no `checkout -b` anywhere in the server code.
- **Before every turn,** MausBot snapshots the folder into a shadow git repository of its own
  (`checkpoints\<bot>\<hash>`). It never touches the project's own `.git`.
- **Its records:**
  - transcripts in `messages.db` (SQLite)
  - runtime events in `events\<thread>.ndjson`
  - a monthly approval log in `decisions\YYYY-MM.ndjson`
  - each bot's memory as Markdown, in `workspaces\<bot>\`
- **Rule:** Chorus reads MausBot only through its CLI, API or MCP server, never through these files.
  They are private to a project that releases often (§6).

### Chorus (branch `main` at `3c75fbd` plus later work; re-check at kickoff)
- **Worktrees.**
  - `sessions/workspace.rs`: `make_worktree` (:169) and `add_run_worktree` (:204).
  - `sessions/worktrees.rs`: `worktrees_list` (:117) and `reconcile_worktrees` (:62).
  - `git.rs`: `add_worktree`, `add_worktree_on`, `remove_worktree`, `delete_merged_branch` (:121)
    and `worktrees`.
- **A loopback MCP endpoint already exists.**
  - `run_loop/tools.rs` serves `POST /mcp/<step>` (Streamable HTTP) on the events listener
    (`events/listener.rs:27`).
  - Each step's token reaches its agent only through the environment variable `CHORUS_LOOP_TOKEN`
    (spike S3).
  - P18's MCP server follows the same pattern.
- **Notifications** (`attention.rs`, `store::notifications`) are stored per session
  (`notifications::insert(store, session_id, …)`). Branch and baton notices belong to a project, so
  they need a notice that isn't tied to a session (a migration).
- **Projects** have `folder: Option<String>` and a `state` (`projects.rs:23`).
- **Migrations:** the newest is `0027_step_runs.sql`, so P18 starts at 0028.
- **Telegram:** the relay is built (`relay/`, P15 slice 4), and P15's loop plans to use it (D97).

## 4. Design

### 4.1 The hand-off note (`.chorus/handoff.md`)
Markdown, so Matthew, Chorus and a MausBot bot can all read and write it. A small front matter
block holds the facts Chorus acts on:

```markdown
---
project: Atlas
baton: mausbot            # chorus | mausbot | nobody
branch: maus/2026-11-18-checkout-fix
worktree: D:\work\.chorus2tlas\maus-2026-11-18-checkout-fix
base: main @ 1a2b3c4
updated: 2026-11-18T14:05:00Z
---

## 2026-11-18 14:05 · Chorus → MausBot
Done: … (from the session's plan steps and last reply)
State: branch main @ 1a2b3c4, clean; tests passed at 13:58
Open: …
Next: …
```

Entries are newest first. Each has a heading (time, and who handed to whom) and four labelled
lines: Done, State, Open and Next.

- **Chorus writes an entry** when Matthew hands the baton over, and refreshes a "latest Chorus
  work" entry when a session in the project ends a turn or closes.
- **The entry is built from data Chorus already has:**
  - the plan steps and times (P16)
  - the last reply
  - prompt history
  - `git::snapshot` (branch, uncommitted files)
  - the last check result, where one exists

  An LLM summary is optional, through the narrator. The deterministic version comes first.
- **MausBot writes its own entry before handing back,** following an instruction snippet Chorus
  gives Matthew to paste into the bot's instructions (or that Chorus sends with the hand-off
  message). If MausBot writes nothing, Chorus writes its entry on hand-back from git: the commits on
  the `maus/…` branch and the files they changed.
- **Writes are atomic:** to a temporary file, then a rename. A note that won't parse is kept as it
  is, and Chorus shows why, not a reset (fail visible).
- **The note counts as untrusted text.** It is scrubbed before Chorus shows it or sends it, and
  "Continue from here" puts it in the composer for Matthew to read. It never auto-sends.
- **The note's size is bounded.** Older entries move to `.chorus/handoff-archive.md` beyond 20
  entries or 64 KB.

### 4.2 The baton and MausBot's worktree
Chorus's database holds the baton (`chorus`, `mausbot` or `nobody`) and the active `maus/…` branch,
and mirrors them into the note.

**Hand to MausBot** (a button on the project, and in the Overview's project menu):
1. Pick the base, which defaults to the project's checked-out branch. Name the branch
   `maus/<yyyy-mm-dd>-<slug>`, where the slug comes from an optional one-line task.
2. Make the worktree with the existing worktree code, next to the repository, as Chorus does
   for sessions.
3. Write the hand-off entry, then point MausBot at the worktree. Spike S1 decides how: the API's
   `create_task` with a folder if it takes one, otherwise `send` to a chosen bot with the folder in
   the message.
4. Show a notice: "Atlas is with MausBot on `maus/2026-11-18-checkout-fix`."

While MausBot has the baton:
- Chorus's own sessions keep working in the main checkout. The worktree keeps the two apart, so
  nothing is locked.
- The project shows a quiet banner naming the branch and when MausBot last changed it.

**The baton comes back** in one of three ways:
- Matthew clicks **Take back**.
- Chorus sees the note's `baton:` line change (a file watch).
- Level 3: the bot calls `handoff_return`.

Chorus then shows a **return card** on the project:
- MausBot's entry, or the one Chorus wrote from git
- the commits ahead of the base, and the files changed
- **Continue from here**, which fills a session's composer
- **Open in Raw** for a diff
- the branch's state: "merged", or "not merged yet: 4 commits ahead"

Chorus does not merge on its own. The merge is Matthew's, or one of his sessions'.

### 4.3 Branch hygiene
- **When the baton passes,** a notice names the branch.
- **On return,** the card says whether the branch is merged. Once it's merged, it offers
  **Delete branch** (which removes the worktree first, then `git::delete_merged_branch`).
- **A daily sweep** (plus one at startup) covers each project repository's local branches. Skipped:
  - the default branch
  - the checked-out branch
  - any branch in use by a live session's worktree
  - any branch Matthew snoozed in the last 30 days

  A branch whose newest commit is over 30 days old gets **one notice** saying "unused for 30 days",
  whether it is merged, and how many commits it is ahead. It offers Delete or Keep 30 days.
- **Deleting an unmerged branch** needs a second confirmation that names the number of commits that
  will be lost. Deleting a branch with a worktree removes the worktree first, and only if it is
  clean.
- **Storage:** a project-level notice (migration 0028 lets a notification have no session), and a
  `branch_snoozes` table.

### 4.4 Level 3, deferred (D132): Chorus's MCP server for MausBot
Kept here as the design to start from. Not built in P18's first pass.
- **The endpoint** is `POST /mcp/mausbot`, Streamable HTTP, on the existing loopback listener.
  It is never on a network address.
- **The token.** Chorus generates a random token and stores it with DPAPI. Matthew sees it once,
  to paste into MausBot's MCP server entry as an `Authorization` header (kept in MausBot's vault).
  Rotate and revoke are in Settings.
- **The tools:**
  - `list_projects`
  - `project_status`: sessions, their attention state, progress, and the last reply (scrubbed,
    capped at 4 KB)
  - `read_handoff`
  - `send_prompt(session, text)`
  - `handoff_return(project, entry)`
- **How `send_prompt` behaves:**
  - It goes through the same delivery path as Friendly.
  - It is stored in prompt history with source `mausbot`.
  - It is refused with a reason when the session is busy (or queued, where the agent queues),
    locked, or doesn't exist.
- **Left out on purpose:** answering approvals or questions, starting, restarting or closing
  sessions, changing permissions or options, and anything to do with keys. MausBot's own control
  plane excludes the same kinds of action.
- **Limits:** about 30 calls a minute per token, prompts of at most 16 KB, and every call logged
  (scrubbed).

## 5. Slices (each ends seen working in the real app, then committed)

| Slice | What | Exit, seen in the app |
|---|---|---|
| **18.0 Spikes** | S1: can MausBot's API, CLI or `create_task` start a bot working in a given folder? S2: does MausBot connect to a Streamable HTTP MCP server on loopback with a header token, and list its tools? S3: will a bot reliably write its own hand-off entry from an instruction snippet? | A written result for each, in `_docs/spikes/` |
| **18.1 Hand-off note** | Writer, format, `.git/info/exclude`, refresh on turn end and close, and a viewer on the project | The note appears and refreshes in a real project, git ignores it, and a broken note shows why |
| **18.2 Baton and worktree** | Hand to MausBot, the banner, Take back, the file watch, and the return card with Continue from here | A real MausBot bot works on `maus/…`, hands back, and the card shows its commits; Continue fills the composer |
| **18.3 Branch hygiene** | Project-level notices (migration 0028), the baton and return notices, the 30-day sweep, Delete and Keep, and the unmerged double confirmation | A branch backdated past 30 days in a scratch repository is flagged once; Keep snoozes it; Delete removes a merged branch and its worktree |
| **18.4 MCP for MausBot** (deferred, D132) | `/mcp/mausbot`, the token and Settings, the five tools, limits and refusals | From the phone, a MausBot bot reads a project's status and sends a prompt that runs in a Ready Chorus session, recorded once with source `mausbot` |
| **18.5 Telegram off** | Telegram defaults to off, D16 is marked partly superseded, and P15's plan says its remote channel is MausBot (D132) | Settings shows the relay off, with the reason |

## 6. Out of scope

- **Ingesting MausBot's code.** It is TypeScript, React and Electron, which the locked stack rules
  out, and both apps would claim the same agent processes.
- **Reading MausBot's private files** (`messages.db`, `events/`, `bots.json`). Chorus uses only its
  CLI, API or MCP server.
- **Merging automatically,** or deleting any branch without a click.
- **Answering approvals or questions through MausBot.**
- **Exposing Chorus beyond loopback.**

## 7. Security checklist
- Chorus's MCP token and its MausBot client token are stored only as DPAPI blobs. They are passed
  in a header or an environment variable, never in argv, a log, the note or an export.
- `/mcp/mausbot` listens on loopback, rejects any browser `Origin`, and compares tokens in constant
  time.
- The note and every MausBot reply are scrubbed before Chorus shows them or sends them, and are
  treated as data, not instructions.
- Branch deletion checks that the branch is merged, and that a worktree is clean before removing it.

## 8. Verification notes
- Use an isolated instance (`p18`, with its own data and port), a scratch git repository, and a
  dedicated test bot in Matthew's MausBot. MausBot has a single install with Matthew's real data, so
  every check uses only the test bot, and nothing else in MausBot is changed.
- Never touch Matthew's installed Chorus data. Stop only the processes the check launched.

## 9. Settled at planning (D132)
- **P18 is next,** before P17 (goal mode) and the rest of P15.
- **The first pass is 18.0–18.3 and 18.5.** 18.4, the MCP level, waits until Matthew has used the
  note, the baton and the branch notices.
- **P15's loop uses MausBot, not Telegram,** as its remote channel. The loop needs both directions:
  sending to the phone and hearing the answer. P15's remaining slices design that bridge. They can
  use MausBot's local API (send with a token, read a bot's thread) without waiting for 18.4.
