# Hand-off rules for parallel phases (P7, P8, P11, P13, P14)

**Priority override, 2026-10-25 (D102): P11 and P14 are on hold until Matthew explicitly resumes
them.** The permission below to start those phases is suspended. P10 and P12 take priority;
P14 and then P11 remain at the end for a future Chorus 2 real-world test. Existing work is retained.

Written 2026-10-20. Matthew is handing five later phases to other models while the main line finishes
P4 and P5. **Read this file first, then your phase's plan:**

| Phase | Plan | Your review file (you write it) | Dev label | Vite port |
|---|---|---|---|---|
| P7 Attention and notifications | [HANDOFF-P7-attention.md](HANDOFF-P7-attention.md) | `_docs/HANDOFF-P7-REVIEW.md` | `p7` | 1427 |
| P8 Bots, Automations, Notes, Documents | [HANDOFF-P8-bots-notes.md](HANDOFF-P8-bots-notes.md) | `_docs/HANDOFF-P8-REVIEW.md` | `p8` | 1428 |
| P11 Measurement, made visible | [HANDOFF-P11-measurement.md](HANDOFF-P11-measurement.md) | `_docs/HANDOFF-P11-REVIEW.md` | `p11` | 1431 |
| P13 Phone relay (Telegram) | [HANDOFF-P13-telegram.md](HANDOFF-P13-telegram.md) | `_docs/HANDOFF-P13-REVIEW.md` | `p13` | 1433 |
| P14 More agents and ship | [HANDOFF-P14-agents-ship.md](HANDOFF-P14-agents-ship.md) | `_docs/HANDOFF-P14-REVIEW.md` | `p14` | 1434 |

`CLAUDE.md` says to build phases in order. **Matthew has made an exception for these five**, so you
may build your phase now. Everything else in `CLAUDE.md` still applies. Read these in full before you
write code: `CLAUDE.md`, `_docs/PLAN.md`, `_docs/ARCHITECTURE.md`, `_docs/SCOPE.md`,
`_docs/DECISIONS.md`, `_docs/LESSONS.md`, `_docs/OPEN-QUESTIONS.md` and `_docs/design/DESIGN.md`.

Your work will be reviewed and merged by the main-line agent (Claude). Treat that reviewer as
skeptical. It will check every claim in your review file against the code and the running app
(LESSONS L20). A claim you didn't verify, marked as such, costs nothing. A claim marked verified that
turns out false costs your whole hand-off its credibility.

---

## 1. Isolation: your own branch, worktree and app instance

Several agents work on this repository at once: the main line on `main`, a `p4` worktree, a locked
agent worktree under `.claude/worktrees/`, and the other hand-offs. Never touch any worktree but your own.

1. **Make your worktree from the current `main`:**
   ```
   git worktree add ../Chorus-2.0-<label> -b handoff/<label> main
   ```
   Example: `../Chorus-2.0-p8` on branch `handoff/p8`. Work only there. Record the base commit
   (`git rev-parse main`) for your review file.
2. **Never commit to `main`, never push, never merge, never rebase another branch.** Matthew pushes.
   The reviewer merges.
3. **Run your own app instance, never Matthew's.** Debug builds accept
   `com.contactestablished.chorus2.dev.<label>` (D44). That gives the instance its own data folder and
   its own single-instance lock. Create `_verify/tauri.<label>.conf.json` in your worktree (`_verify/`
   is gitignored):
   ```json
   {
     "identifier": "com.contactestablished.chorus2.dev.<label>",
     "build": {
       "beforeDevCommand": "cargo build -p chorus-hook && npx vite --port <port> --strictPort",
       "devUrl": "http://localhost:<port>"
     }
   }
   ```
   Run it with `npx tauri dev --config _verify/tauri.<label>.conf.json`. Use the port from the table:
   port 1420 belongs to Matthew's dev instance, and `strictPort` makes a clash fail.
   - **Never** run `npm run dev` from your worktree. It uses the `.dev` identifier, which is Matthew's
     data folder, and your new migrations would be applied to his database. Once a database has
     migration `0010` from your branch, it can never take a different `0010` from `main`.
   - If this config doesn't work as written, say what failed in your review file. Don't edit the
     tracked `tauri*.conf.json` files to work around it.
   - Your instance's database is disposable. The reviewer deletes it after the merge, because your
     migration numbers will probably change.
4. Your worktree has its own `target/`, so the first build is slow. That is expected. Don't point
   `CARGO_TARGET_DIR` at another worktree's.

## 2. Safety rules (from the P0 kickoff; still in force)

- **Chorus 1 is Matthew's daily driver and may be running agents.** Never kill processes by name
  (`chorus.exe`, `electron.exe`, `node.exe`, `claude.exe`, `codex.exe`, `opencode.exe`, `grok.exe`,
  `cmd.exe`, `conhost.exe`, `pwsh.exe`). On Windows `chorus.exe` also matches Chorus 1 (LESSONS L21),
  and Matthew's own 2.0 instance has the same name. Kill only PIDs you launched and recorded. You may
  yourself be one of those processes.
- **Never touch Chorus 1's installation or data** (`%LOCALAPPDATA%\Programs\Chorus`,
  `%APPDATA%\chorus-app`, `%APPDATA%\chorus`). Don't touch Matthew's 2.0 data
  (`%APPDATA%\com.contactestablished.chorus2` and `…chorus2.dev`) or an installed "Chorus 2" either.
- **Keys:** test with fake keys only. Never print a real or fake key; prove a key is present by its
  length or hash. A real key Matthew gives you for runtime testing goes in **through the app's UI**,
  never through chat, argv, a file or your logs.
- **Money:** subscription turns on Claude Code and Codex are fine if they are few and tiny. Anything
  that spends API credit (OpenRouter, a provider key) needs Matthew's go-ahead first. Prefer `:free`
  OpenRouter models.
- **Machine changes:** installing a CLI, a runtime or a global npm or pip package, or changing a global
  agent config (`~/.claude`, `~/.codex`, `~/.grok`, opencode's config), needs Matthew's yes first.
  Reading those files is fine.
- **Probe agents** run in a scratch folder outside any real repository. Every opencode process gets its
  own temporary `XDG_STATE_HOME` and `XDG_DATA_HOME` (LESSONS L5). Claude Code asks "Do you trust this
  folder?" in a new folder (L33); that prompt is not a hang.
- **Never run `claude help <x>`** (it spends a turn). Use `--help`. Send prompt text and Enter as two
  separate writes (L4).
- **Evidence** (logs, screenshots, captured output) goes in `_verify/`. It is never committed. The repo
  is public (D32): no local paths (`C:\Users\…`), usernames, keys or client project names in anything
  you commit, your review file included.

## 3. Shared files: how to keep the merge cheap

Five branches plus the main line will touch some of the same files. Follow these rules exactly.

| File or area | Rule |
|---|---|
| `src-tauri/migrations/` and `MIGRATION_FILES` in `src-tauri/src/store/mod.rs` | Migrations must be numbered without gaps (`chorus_core::migrate`), so two branches can't both keep `0010`. Use the next free number on your branch and name it `00NN_<label>_<what>.sql` (for example `0010_p8_bots_notes.sql`). Prefer **one migration file for your phase**; a second only if the first has already shipped to your own test database and you need more. Expect the reviewer to renumber it. Nothing in your code may depend on the number. Migrations are forward-only: never edit a migration that is on `main`. |
| `src/ipc/bindings.ts` | Generated. Never hand-edit it. Regenerate with `npm run bindings`. At merge the reviewer regenerates it, so a conflict there doesn't matter. |
| `src-tauri/src/ipc.rs` | Add your commands as **one contiguous block at the end** of `collect_commands![…]`, headed by a `// <label>: …` comment. Add events at the end of `collect_events![…]` in the same way. |
| `src-tauri/src/lib.rs` | Add your `mod` lines and, ideally, **one** `<module>::start(…)` call in `setup`. Keep the logic in your module. |
| Hot files: `src-tauri/src/sessions/mod.rs` (1,700 lines, edited in most P4 commits), `src/app/App.vue`, `src/views/newSession/NewSessionForm.vue`, `src/views/session/SessionView.vue`, `src/views/session/SessionFacts.vue`, `src/views/OverviewView.vue`, `src/shell/Rail.vue`, `src/stores/sessions.ts`, `crates/chorus-core/src/agent.rs`, `src-tauri/src/events/recorder.rs` | Edit them as little as possible: a field, a call into your module, a component tag. Put the logic in new files. List **every** edit to these files in your review file, with the reason. |
| `crates/chorus-core/secret-patterns.json` | Append your patterns at the end. Never reorder or remove one. |
| `Cargo.toml`, `package.json` and lock files | Only for dependencies the plan allows (§5). Keep the edit to one line where you can. |
| `_docs/DECISIONS.md`, `LESSONS.md`, `PLAN.md`, `SCOPE.md`, `ARCHITECTURE.md`, `OPEN-QUESTIONS.md`, `design/DESIGN.md`, `README.md` | **Don't edit them.** Their numbering (D56 onward, L40 onward) would collide across branches. Write proposed entries in your review file (§6) with provisional ids such as `P8-D1` and `P8-L1`, in the house style. The reviewer assigns the real numbers. |
| `CLAUDE.md`, `AGENTS.md`, `.claude/`, `.gitattributes`, `.editorconfig`, `scripts/` | Don't edit them unless your plan says so. |

**Where the hand-offs overlap each other.** Read this table, and stay inside your column.

| Shared piece | Owner | Others |
|---|---|---|
| Delivering a prompt to a session from Chorus, recorded once with its source (§4) | **P8** (automations need it) | P13 uses it. If it's not on `main` when P13 starts, P13 builds the same contract in the same file (§4), and the reviewer keeps one copy |
| `SessionSummary` fields | each adds its own field | P7 adds when-it-started-needing-you; P11 adds tokens; P8 adds the bot. Add your field; don't reshape the others |
| State-transition reactions (the notify closure in `lib.rs`) | P7 (notifications, tray, strip) | P13 (outbound messages) subscribes **its own** listener; don't route through P7's code |
| New screens in the rail | P8 (fills the four existing placeholders) | P11 may add Usage (its plan has the decision). P7 adds no rail entry |
| `agent.rs` `Spec` | P8 (an instructions capability) | P14 (new agent kinds), P7 (Claude's `-n <name>`). Add fields with defaults so the others still compile after the merge |
| Secret patterns | P13 (Telegram bot token), P14 (Google `AIza…` key) | append only |

## 4. The prompt-delivery contract (owned by P8, used by P13)

PLAN P6 will build the full ActionRouter (structured channels, queueing, answering approvals). The two
hand-offs need a smaller piece now: send a prompt from Chorus to a running session and record it
**once**, with its real source. Today only `"hook"` and `"server"` are written to `prompts.source`
(`events/recorder.rs`). A prompt Chorus writes to the PTY would come back through `UserPromptSubmit`
and be recorded as `"hook"`.

The contract lives in a **new file, `src-tauri/src/sessions/deliver.rs`**:

```rust
/// Where a prompt came from, as stored in `prompts.source`.
pub enum PromptSource { Automation, Telegram }   // P6 adds Composer
// stored as "automation" / "telegram"

pub struct Delivered { pub prompt_id: i64 }

/// Sends `text` to the session's agent and records it once.
pub fn deliver_prompt(manager: &SessionManager, session_id: i64, text: &str, source: PromptSource)
    -> Result<Delivered, String>;
```

The rules, all of them required:
1. Validate at the boundary. The session exists and has a live run. The text is 1 to 16 KiB after
   trimming. Control characters are removed, except `\n` and `\t`, and so are ESC sequences.
2. **Persist before delivering.** Scrub the text, then insert the `prompts` row with this source
   (`store::history::insert_prompt`). Only then write to the agent.
3. Deliver as the user would type it. Write the text, then Enter, as **two separate writes** (L4).
   Multi-line text goes through as a bracketed paste, followed by a separate Enter. A prompt sent from
   the phone or by an automation is an explicit send, so submitting it doesn't break D23, which is
   about text from dictation.
4. **Record once.** Register the delivered prompt as expected for that run. When the agent's own
   channel reports a prompt (a Claude or Codex hook, or opencode's server) that is the same scrubbed
   text on the same run within 60 s, the recorder links it to the existing row. It doesn't insert a
   second one. Keep this a small, pure matching function in `chorus-core` with unit tests, and keep
   the recorder change to one call.
5. Refuse visibly. A locked session, a session with no live run, or an `approval` state each return an
   `Err` whose text the caller can show, for example "Session is waiting for an approval. Answer it
   first". `deliver_prompt` never answers an approval.
6. Never put the text in argv, a log line or an event payload that isn't scrubbed.

If your phase is the one building it, give it unit tests (matching and validation) and a runtime
check: one prompt delivered to a Claude Code session shows **one** history row with your source.

## 5. Code rules (a reminder of `CLAUDE.md`, plus what has bitten before)

- **Pure logic in `crates/chorus-core`**, with unit tests. No IO, no clock, no randomness, no Tauri.
  `tests/purity.rs` enforces it. `src-tauri` does the IO and wiring only.
- **Typed IPC.** Commands take and return serde + specta types. Validate semantics at the Rust
  boundary: ids exist, paths are inside the project, sizes are bounded.
- **Fail visible (D19).** A degraded feature says why on screen. Never disable an option silently.
- **Secrets:** environment variables only, never argv, files, logs, events or exports. The scrubber
  runs on everything you persist or show. With `reqwest`, **an error's text includes the request URL**;
  if a secret could be in the URL (Telegram's API puts the bot token in the path), call
  `.without_url()` on the error before formatting it.
- **Dependencies:** only those in ARCHITECTURE §10. These are approved but not installed yet, so you
  may add them when your plan needs them: `@xterm/addon-search`, `@xterm/addon-unicode11`,
  `tauri-plugin-opener`, `tauri-plugin-shell`, and later `tauri-plugin-updater` (P14, after Matthew
  says yes). **Anything else needs Matthew's yes first.** That includes a new cargo feature of an approved
  crate that changes what is compiled in, such as Tauri's `tray-icon`, and a new `windows` crate
  feature. Ask, give your recommendation, and wait.
- **Agent CLIs:** verify every flag, file format and event against the installed CLI's `--help`, docs
  or files before you hardcode it. Record the version you verified against. Note that **opencode on
  this machine is now 2.1.1** (npm `@opencode/cli`); 2.0's adapter was verified against 1.19.3.
- Line endings are LF. Code reads like the code around it: same comment density, naming and the
  British spelling the docs use.
- Small commits on your branch, each prefixed `<label>:` (for example `P8: notes table and screen`).
  Stage only the paths you changed (`git add <paths>`, never `git add -A` or `.`), and end each message
  with the co-author line your harness gives you.
- **Before each commit:** `npm run check` (typecheck, vitest, `cargo test --workspace`, clippy with
  `-D warnings`, and the secret scan) passes. Check staged files for local paths.

## 6. Finishing: rebase, then the review file

1. **Try a rebase onto the latest `main`** (`git rebase main`). Resolve conflicts that are only in
   `bindings.ts` (regenerate it), migration numbering (renumber yours), or the registration lists in
   `ipc.rs`, `lib.rs` and `store/mod.rs`. If anything else conflicts, `git rebase --abort` and list the
   conflicting files in your review file; the reviewer will merge them. Run `npm run check` again
   after a rebase.
2. **Write `_docs/HANDOFF-<label>-REVIEW.md`** as your last commit, with these sections in this order:

   1. **Verdict.** One line: done, partly done (what's missing), or blocked (on what).
   2. **Branch.** Branch name, base commit, final commit, and `git log --oneline main..HEAD`.
   3. **Files.** Tables of files created, modified and deleted, each with a one-line purpose.
      Mark edits to the hot files (§3) and give the reason for each.
   4. **Surface added.** Migrations (file, tables and columns), IPC commands and events (name,
      input, output), settings keys, environment variables, files Chorus now writes and where, and
      dependencies or features added.
   5. **Plan against result.** Each task in your plan: done, partly done or not done, with one line
      on what differs.
   6. **Verification.** What `npm run check` printed (counts, pass or fail). For each exit
      criterion and runtime check: what you did, how, what you saw, and the evidence file in
      `_verify/` (not committed). **List separately what you could not verify, and why.**
   7. **Versions verified against.** Each CLI and API, with its version.
   8. **Proposed doc changes.** Full text, ready to paste, for: decisions (`<label>-D1…`, in
      DECISIONS.md's row style, status Proposed), lessons (`<label>-L1…`), and any edits to PLAN,
      SCOPE, ARCHITECTURE (including §10 if you got a dependency approved) or DESIGN.
   9. **Questions for Matthew.** For each: the consequences, the options, and your recommendation.
   10. **Risks and known issues.** Bugs you know of, shortcuts, deviations from the plan, merge risks,
       and anything that conflicts with another hand-off (§3).
   11. **How to review.** The order to read the files in, and a short manual test script that a
       reviewer can follow in the real app.

   Keep it factual. Write numbers, not adjectives, and don't use local paths.

## 7. When to stop and ask Matthew

Ask, with your recommendation, and wait, when:
- you need a dependency, feature or machine change that §2 or §5 doesn't already allow;
- a test would spend API credit;
- anything would touch Chorus 1's or Matthew's data;
- a design decision your plan lists as Matthew's comes up (each plan has a "Decisions for Matthew"
  section with options and a recommendation; ask before you build past it);
- something you find changes the plan. Then say what you found, what it changes, and what you propose.

Matthew prefers decisions spelled out: what is being decided, what each option leads to, and what you
recommend. Don't send him a bare "A or B?".
