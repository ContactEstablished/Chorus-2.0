# Review: oc2, opencode 2.1.1

## 1. Verdict

Done, with one item measured and not built (reconnect replay, question 1). The plan in
`HANDOFF-OC2-opencode.md` was written on the P14 branch, which forked from `main` at `cb415a5`, before
the main line's own 2.1.1 port (`340fe5f`, `64836cb`, D58). Matthew chose to verify that port end to end
and fix what failed rather than redo it. This branch runs every exit check of the plan in a real `oc2`
app instance and fixes the five gaps that turned up.

## 2. Branch

- Branch `handoff/oc2`, base `45eb41f` (`main`, unchanged since; no rebase needed).
- `git log --oneline main..HEAD`, before this file's commit:

```
97070ab oc2: a prompt sent during an opencode turn no longer marks the first one interrupted
bf3e60e oc2: opencode 2.1.1 gaps found in the real app
```

## 3. Files

Created: this file. Deleted: none.

Modified:

| File | Purpose |
|---|---|
| `crates/chorus-core/src/opencode.rs` | `check_model` takes `/api/integration` and returns `NoKey`; `set_model_body`; `too_old` (the 1.x gate); `summary_entries`; `EventParser` keeps every turn of an execution and ends them together. 5 new tests |
| `crates/chorus-core/src/cli.rs` | The version-line test reads `opencode v2.1.1` |
| `src-tauri/src/agents/opencode.rs` | `Api::set_model`; `check_model` fetches `/api/integration`; `forget_tui_choice` removed; a server that exits before it is ready says so |
| `src-tauri/src/agents/detect.rs` | An opencode older than 2.x is detected as not usable (`found: false`), with the refusal as its `problem` |
| `src-tauri/src/agents/commands.rs` | `launch_args` returns opencode's configuration entries, as its doc comment already promised |
| `src-tauri/src/agents/mod.rs` | One stale comment (`attach`) |
| **`src-tauri/src/sessions/mod.rs`** (hot) | Only `start_opencode` and its one call: the version gate (detecting inline when start-up relaunches come before the first detection), the session's model set again on resume, `forget_tui_choice` dropped. Two stale comments fixed. 33 lines |
| **`src/ipc/bindings.ts`** (generated) | Regenerated: two doc comments changed (`launchArgs`, `Detection.found`). No type changes |

## 4. Surface added

- Migrations, IPC commands and events, settings keys, environment variables, dependencies: none.
- `launch_args` for opencode now returns `key=value` entries (`model=…`, `permissions=[…]`,
  `default_agent=plan`, `variant=high`) instead of an empty list. Claude, Codex and Grok are unchanged.
- `Detection.found` is `false` for opencode below 2.x, with `problem` naming the version Chorus drives.
  The front end already treats `found: false` as "can't be chosen, and says why".
- New opencode API calls: `GET /api/integration` (the model check), `POST /api/session/{id}/model`
  (a resumed run).
- Files: Chorus no longer deletes `xdg-state/opencode/model.json`, which 2.1.1 never writes.

## 5. Plan against result

The plan's tasks, against what `main` already had and what this branch did:

| # | Task | Result |
|---|---|---|
| 1 | Capture the surface | Done. `/openapi.json`; every event name in the 2.1.1 binary; turns with a write, a shell call, approvals allowed and rejected, an interrupt, a failure, a model change and a steered prompt. Evidence in `_verify/oc2-probe/` |
| 2 | Core: model check | `main` had it, with `activation: disabled` as "not connected". **Gap fixed:** a provider with no key was never caught, because 2.1.1 lists keyless OpenRouter as `enabled` and leaves keyless Anthropic out of both lists. Now read from `/api/integration` |
| 3 | Core: event parser | `main` had it. **Gap fixed:** a prompt typed during a turn closed the first turn as interrupted |
| 4 | Core: read-back | Superseded by `main`: the variant is set on the session and read from `session.step.started`, which does carry it (`model.variant`). No message read-back is needed |
| 5 | Tauri: API client | `main` had it. Added `set_model` and the integration fetch |
| 6 | Tauri: event stream | `main` had it. `durable.seq` checked: no replay exists (§6). Not built; question 1 |
| 7 | Launch and isolation | `main` had `--server`. Verified: `--standalone` is not needed, no `opencode service` is started, auto-update is off. **Gap fixed:** `model.json` is gone in 2.x, so a resumed session kept the TUI's last model and effort |
| 8 | Options, observed | Done: model, effort, Plan, Manual and Full access all observed in the app |
| 9 | Resume | Done, with the gap of task 7 fixed |
| 10 | Version gate | **Gap fixed:** a 1.x failed with an HTTP status. It is now refused by name in detection and at launch |
| 11 | Exit checks, review | This file |

Also fixed (not in the plan): New session's summary said "opencode (its own defaults)" whatever was
chosen (since P4's `4b45c7c`); an opencode server that exits at once said "wasn't ready in 30 s".

## 6. Verification

**`npm run check`** (after the last commit): typecheck clean; vitest 6 files, 19 tests passed; cargo
`chorus-core` 357 passed (5 new), `src-tauri` 45 passed, others 3 + 3; clippy `-D warnings` clean;
secret scan clean (6 patterns, 235 files). Log: `_verify/check2.log`.

**Runtime, in the `oc2` instance** (`com.contactestablished.chorus2.dev.oc2`), driven through its WebView's
DevTools port: New session's form for creation, the PTY for prompts and TUI keys, IPC for reading state.
Free models only (`opencode/deepseek-v4.1-flash-free`); nothing was spent. Screenshots are
`_verify/oc2-NN-*.png`.

| Check | What was seen | Evidence |
|---|---|---|
| Launch | Server plus `--server` TUI; TUI footer "DeepSeek V4.1 Flash Free · high"; "2.1.1, the version Chorus checked" | 06 |
| Model and effort observed | "Last turn ran: opencode/deepseek-v4.1-flash-free · high · build agent" | 07 |
| Manual asks | "Awaiting approval" from `permission.asked`; Allow once wrote the file; the shell call asked again | 07, 08 |
| Rejection | Reject in the TUI: turn `interrupted`, `shell` failed, ask answered "rejected" (opencode itself ends the execution, `reason: shutdown`, and its TUI says "interrupted") | 10 |
| Question (form) | "Needs your answer"; answered Blue; turn `done`, final message "Your preferred colour is blue.", ask answer "Blue" | 11, 12 |
| Interrupt | Esc twice: turn `interrupted`, status back to Ready | 14 |
| Plan and Max | `observed.agent = plan`, `variant = max`, no differences | 24 |
| Full access | write and shell both ran, no asks, turn `done` | — (history read over IPC) |
| Failure | Keyless OpenRouter: "Failed", "No cookie auth credentials found" | 21 |
| No key, before the turn | After the fix: "opencode has no key or login for openrouter in this session, so openrouter/deepseek/deepseek-v4.1-flash won't run. Run it on a provider from Settings." | 22 |
| Resume | Same `ses_…`, earlier turns in the TUI | 17 |
| Resume after a TUI effort change | Before the fix: ctrl+t to `max`, a prompt, Restart → still `max` ("ran with effort max, not high"). After: TUI shows "Switched variant to high", next turn observed `high`, no differences | 19, 20, 22 |
| Steered prompt | Before the fix: first turn `interrupted`. After: both `done`, each with its own reply | 32 |
| Summary | "opencode default_agent=plan model=opencode/deepseek-v4.1-flash-free variant=max" | 23 |
| Version gate | Stub shim printing `1.19.3` first on PATH: Settings → Agents shows the refusal in red, New session's tile is disabled with it as tooltip, Restart says "Couldn't start: opencode 1.19.3 is too old: …" | 26, 27 |
| Gate at start-up | Two live sessions, app restarted with the stub: both relaunches refused by name before detection finished | IPC output |
| Server exits at once | Stub printing 2.1.1 that fails `serve`: "opencode serve exited (exit code: 1) before it was ready", in under a second | IPC output |
| `--server` unreachable | `opencode --server http://127.0.0.1:9 --session ses_probe1` in a Chorus shell: "Error: Could not reach server at http://127.0.0.1:9", exit code 1 | 29, 30 |
| No background service | No `serve --service` started by any run (the one present predates this work and isn't Chorus's); the TUI log mentions no service | process list |
| Auto-update off | Every TUI start logged `update check skipped reason=disabled version=2.1.1`; no npm/pnpm/bun list ran | session log |
| Job Object | The app hard-killed four times: every opencode server and TUI it started went with it | process list |

**Could not verify, and why:**
- Reconnect after a dropped event stream: there was no way to drop a loopback stream on demand. An
  idle stream stayed open 10 minutes (40 heartbeats, one per 15 s) until the client's own timeout.
- Forms with several fields, "Always allow", a prompt cancelled from the inbox (`session.inbox.cancelled`),
  compaction, and subagents (child sessions' events carry their own `sessionID`, so the parser ignores
  them): none came up.
- A real opencode 1.x: none is installed. The gate was tested with a stub shim.
- `activation: "disabled"` (`NotConnected`, from `main`'s port): no provider here was switched off. Its
  test uses a made-up fixture.

## 7. Versions verified against

- opencode **2.1.1** (npm `@opencode/cli`, `opencode.exe` behind the npm shim), API `opencode HttpApi`
  0.0.1 from `/openapi.json`.
- Models: `opencode/deepseek-v4.1-flash-free` (variants low, high, max), `opencode/glm-5.3-flash-free`.
- Node 22.14.0 (probe scripts), WebView2 remote debugging for driving the app.

## 8. Proposed doc changes

**Decisions** (DECISIONS.md row style, status Proposed):

| OC2-D1 | 2026-10-20 | **A resumed opencode session is given the session's model and effort again.** opencode 2.x keeps the model and variant on the session, including one picked in the TUI with the last prompt; 1.x's TUI kept it in `model.json`, which Chorus deleted before each run (L35). Each resumed run now sets them with `POST /api/session/{id}/model`; a choice made in the TUI lasts until the next run, and differs visibly ("ran with effort max, not high") while it lasts | Proposed | Measured in the real app on 2.1.1 (OC2-L1). Keeps L35's rule that the session's options decide a run's start |
| OC2-D2 | 2026-10-20 | **opencode below 2.x is refused by name.** Detection marks it unusable with "opencode 1.19.3 is too old: Chorus drives opencode 2.x (checked 2.1.1) …", so Settings and New session say so, and a launch refuses with the same text, reading the version itself when a start-up relaunch comes before detection. 2.x versions other than 2.1.1 are still tried and reported (L7) | Proposed | D58 supports 2.x only; a 1.x has no `/api/` and no `--server`, so it can't be driven at all, and failed with a bare HTTP status |
| OC2-D3 | 2026-10-20 | **opencode's "no key" note comes from `/api/integration`.** A provider whose integration has no connection, and no key of its own in its settings, is named before the first turn fails. A key in the server's environment counts as a connection. A provider with no integration (Ollama) needs none | Proposed | 2.x has no `connected` list; a keyless OpenRouter is listed as enabled and a keyless Anthropic isn't listed at all |

**Lessons:**

**OC2-L1 — opencode 2.1.1, measured in the real app (2026-10-20).**
- The TUI keeps no `model.json` (its state folder has only `latest/tui/tabs.json`, `prompt-history.jsonl`
  and locks). A model or effort picked in the TUI (ctrl+t cycles the variant) is sent with the next
  prompt and stays on the session (a pick with no prompt after it is lost on restart). `POST
  /api/session/{id}/model` `{model: {providerID, id, variant}}` sets it on an existing session, and the
  stream reports `session.model.selected` `{model, previous}`.
- A prompt typed while a turn runs is "steered": `session.inbox.enqueued` at once, delivered after the
  current step ends, and one `session.execution.succeeded` answers both.
- Rejecting an approval fails the tool ("The user declined this tool call"), fails the step and ends the
  execution with `session.execution.interrupted` `{reason: "shutdown"}`; an Esc is `{reason: "user"}`.
- Providers: without keys `/api/provider` lists `openrouter`, `opencode` and `ollama`, all `enabled`,
  and `/api/model` has only their models; Anthropic's 20 appear there with `ANTHROPIC_API_KEY` set. `/api/integration` lists every provider with its
  sign-in methods and `connections`; a key in the environment shows as `{type: "env", name}`. opencode's
  free tier has `settings.apiKey: "public"`; Ollama is built in, at `127.0.0.1:11434/v1`, with no integration.
- `GET /api/event` takes no cursor and opencode can't replay: `GET /api/experimental/session/{id}/log`
  answers only `log.synced` with the newest `seq`, whatever `after` and `follow` say. An idle stream
  sends `: heartbeat` every 15 s and stayed open 10 minutes. Pending asks can be listed with `GET
  /api/session/{id}/permission` and `/form`; whether a session is running, with `GET /api/session/active`.
- `opencode --server <url>` exits 1 at once with "Could not reach server" when nothing answers there.
- Relative `XDG_*` paths break opencode (`EEXIST` on mkdir); give them absolute.
- The TUI still drops what is written to it while it starts (L5 says so for 1.x). Text written about
  the first 6 s after launch lands in the composer, but its Enter is lost (seen by the P14 session
  driving the app over CDP).

**Edits:**
- LESSONS L5, after "that **overrides** configuration": add "(1.x; 2.x keeps the model on the session,
  OC2-L1)".
- ARCHITECTURE, the opencode note under the adapter table: add "A model or effort picked in the TUI
  stays on the session, so a resumed run sets the session's own again (OC2-D1). A prompt typed during a
  turn joins the running execution, and opencode can't replay events a dropped stream missed (OC2-L1)."

## 9. Questions for Matthew

1. **Catch up after a dropped event stream?** Today a dropped stream reconnects and anything it missed
   is lost; if a turn ended in the gap, the session shows Working until the next turn. opencode can't
   replay (OC2-L1). (a) Leave it: the stream is loopback to a server that lives exactly as long as the
   run, and an idle one held for 10 minutes. (b) On reconnect, ask `/api/session/active` and the
   session's `outcome` and close a turn that ended, and list pending approvals and forms (about 50 lines
   in core plus one call in `watch_opencode`), tested by unit tests only, since a drop can't be
   provoked. *Recommendation: (a) now, (b) if a real drop is ever seen.*
2. **Resume overrides a TUI choice (OC2-D1).** If you pick an effort in the TUI, it lasts until the next
   Restart or relaunch, which goes back to the session's options. The alternative is to keep the TUI's
   choice and only show the difference, which makes the session's options a starting point rather than
   a setting. *Recommendation: as built, which is what 1.x did.*
3. **Settings → Agents says versions "are tried and reported, never refused"**, and now shows a 1.x
   refused. 1.x was checked and is known not to work, so the sentence still holds, but it reads as a
   contradiction. (a) Leave it. (b) "Versions it hasn't checked are tried and reported; one it can't
   drive says why." *Recommendation: (b), a one-line front-end change for the main line.*
4. **The stale plan file.** `_docs/HANDOFF-OC2-opencode.md` (untracked in the main checkout) still
   describes the pre-port code. *Recommendation: delete it, or add a line at its top pointing to D58
   and this review.*

## 10. Risks and known issues

- **P14 conflicts with this branch** (not with `main`: P14 has since rebased onto `45eb41f`). A merge
  of the two conflicts in three files, all at `check_model`: P14 adds a `keyless` flag, this branch an
  integrations argument. The resolution is `check_model(providers, models, integrations, model,
  keyless)`, with `keyless` skipping both "switched off" and "no key". It was built and passed `npm run
  check` twice, once here and once by the P14 session, which also resumed an Ollama session through this
  branch's `set_model` with no note. The steps are in `_docs/HANDOFF-P14-OC2-MERGE.md` (main checkout,
  untracked). The intended order is this branch first, then P14 rebases onto `main`.
- **A different resolution exists as `oc2-rebased-onto-p14` (`e082dfe`). Don't merge it.** Another
  session rebased this branch onto `feature/p14` and dropped `keyless`. `handoff/oc2` was put back to
  `6db0057` (its backup, `oc2-before-rebase`, is the same commit). That ref only keeps the work; delete
  it after the merge.
- **P14's Ollama route fails its first turn** (found by that session, cause confirmed in P14's code, not
  this branch's). opencode's model field must be `provider/model`, so the user types
  `ollama/llama3.2:1b`. P14's Ollama check compares that with Ollama's bare names (a spurious "Ollama
  doesn't list…" note), and its route prefixes `ollama/` again, so Ollama is asked for
  `ollama/llama3.2:1b`: "model not found". Reported to P14 to fix on its branch.
- This instance's data folder (`…chorus2.dev.oc2`) now holds P14's migration `0011` and an Ollama
  provider row from that session. Delete the folder before running this branch's instance again.
- **Port clash.** P14's worktree runs Vite on 1435, the port the plan gave `oc2` (P14's own is 1434). This
  instance used 1445. Nothing in the tracked `tauri*.conf.json` files was changed.
- A launch that comes before the first detection (start-up relaunches) now runs `opencode --version`
  itself: about a second, with a 10 s cap.
- A rejected approval shows its turn as `interrupted`, because opencode ends the execution that way. It
  matches opencode's own TUI.
- Session panel wording, not changed: an opencode session with no provider says "Runs on: its own
  sign-in", but a session's opencode has its own data folder, so the user's own login isn't there.
  Only opencode's free tier (or a Chorus provider) works. The new no-key note says so when it matters.
- The no-key check makes no claim when `/api/integration` doesn't answer in the expected shape.

## 11. How to review

Read in this order: `crates/chorus-core/src/opencode.rs` (the tests at the bottom show each rule),
`src-tauri/src/agents/opencode.rs`, the `start_opencode` hunk of `src-tauri/src/sessions/mod.rs`,
`src-tauri/src/agents/detect.rs`, `src-tauri/src/agents/commands.rs`.

Manual test (opencode 2.1.1, a scratch folder, free models only):
1. New session → opencode, model `opencode/deepseek-v4.1-flash-free`, Deep, Manual. The summary shows
   `model=… permissions=[…] variant=high`. Start.
2. Type "Use your write tool to create notes.txt containing alpha, then run: cat notes.txt". The status
   becomes Awaiting approval; Allow once, then Reject the shell call. The prompt shows `interrupted`, 2
   tool calls.
3. Type a long request, and while it runs type "Also end with FIN." Both prompts end `done`.
4. In the TUI press ctrl+t until the footer says `max`, send "Reply OK". The facts say "ran with effort
   max, not high". Restart: the TUI says "Switched variant to high"; the next turn shows no difference.
5. New session with model `openrouter/deepseek/deepseek-v4.1-flash` and no provider: the session's
   note names the missing key before any prompt.
6. Optional: put a shim printing `1.19.3` first on PATH (an npm-style `opencode.cmd` pointing at a
   Node script) and start the app. Settings → Agents shows the refusal, and the opencode tile is disabled.
