# Hand-off plan: opencode 2.1.1 — the v2 API port

> **Superseded, 2026-10-20.** While this was being written, the main line ported opencode 2.x itself
> (commits `340fe5f`, `64836cb`, decisions D58, L41 measured in full). Don't start this plan. What is
> still worth reading here is §3, which records the live measurements (the event vocabulary of one
> turn, the message shape, the configuration echo, the auto-update switch), and §6, which asks
> whether the variant can be verified at all now that the CLI doesn't report it.

**Read [HANDOFF-COMMON.md](HANDOFF-COMMON.md) first.** It holds the isolation, safety, shared-file
and review-file rules this plan relies on. Label `oc2`, Vite port 1435, review file
`_docs/HANDOFF-OC2-REVIEW.md`.

This plan was written on 2026-10-20 by the P14 hand-off, after P14's Task 1 (re-verify opencode
2.1.1). P14's plan says to stop and report when the fix isn't small and local to `opencode.rs` or
`agents/opencode.rs`. It isn't, so P14 left the adapter **unchanged** and wrote this instead. Every
fact below was observed on this machine; the commands are in §3.

**It extends L41** (`_docs/LESSONS.md`), which records the 405 launch failure and the `/api` move and
says the provider and event shapes were unchecked. They are checked here, and the new name says what
else moved. Read L41 first, then §3.

## 1. Goal

Chorus launches, drives and observes opencode 2.x the way it does 1.19.3 today:

1. a server per run, a TUI attached to it in the PTY, and both die with the run;
2. the session's model, effort and permission reach the agent and are **observed to apply**
   (PLAN P2's bar), not only passed;
3. turn state comes from opencode's own events: a prompt starts a turn, tool calls show, an
   approval is asked and answered, and a turn ends, fails or is interrupted;
4. a session resumes the opencode session it had;
5. an opencode Chorus can't drive says why on screen (D19), never looks alive with no state.

## 2. Out of scope, or design only

- **opencode 1.x.** Unless decision 1 says otherwise, port to 2.x only and make 1.x fail visibly.
  `opencode attach` and the whole v1 HTTP surface are gone in 2.1.1, so supporting both means two
  parsers, two model checks and two stream readers.
- **Other agents, other phases.** This is the opencode adapter only.
- **The global background service.** `opencode service` is a shared daemon; decision 3 recommends
  Chorus never uses it.
- **New UI.** `AgentEvent` and the session views stay as they are; only the parser changes shape.

## 3. What changed, verified 2026-10-20 (opencode v2.1.1, npm `@opencode/cli`)

How the facts were gathered: a scratch folder (outside any repository), isolated `XDG_STATE_HOME`,
`XDG_DATA_HOME` and `XDG_CACHE_HOME`, `OPENCODE_SERVER_PASSWORD` for basic auth,
`OPENCODE_DISABLE_AUTOUPDATE=true`, and the free model `opencode/glm-5.3-flash-free`. One turn ran and
succeeded (8,081 input / 96 output tokens, cost 0). The npm shim still points at
`node_modules\@opencode\cli\bin\opencode.exe`, so `shim.rs` needs nothing.

### 3.1 Commands

| 1.19.3, in the code | 2.1.1 |
|---|---|
| `opencode serve --port 0 --hostname 127.0.0.1` | Unchanged (`serve --help` lists `--hostname`, `--port`, `--cors`, `--service`, `--stdio`) |
| `opencode attach <url> --session <id>` | **Gone.** `opencode attach` prints the top-level help and exits 1. The TUI is now the top-level command: `opencode --server <url> --session <id>` |
| — | `--standalone` runs a private server instead of the background service; `--auto` auto-approves; `--continue`; `--prompt <text>`; `[directory]` is a positional argument |
| — | `service start|stop|status|set`, `session list|delete|export|import`, `api`, `models`, `debug`, `upgrade` |

- The server's line is now `server listening on http://127.0.0.1:<port>` (no `opencode ` prefix).
  `opencode::listening_url` still matches it, because it searches for `listening on http://127.0.0.1:`.
- `opencode --server <url> --session <ses_…>` was run against the probe server: it connected to the
  event stream (`event stream connected` in `--print-logs`), drew the session, and showed the turn
  that the API had run. `opencode service status` read `stopped` before and after, so `--server`
  did not touch the background service.
- `--session` **creates the session if it does not exist**, so Chorus should keep creating it
  through the API and pass the id it owns.
- `--prompt` can deliver a prompt without the PTY. That is interesting for P8's delivery contract,
  but the user's own typing still goes through the PTY; don't change delivery here.

### 3.2 HTTP: everything moved under `/api`, and every answer is wrapped

`GET /openapi.json` is the authority (title `opencode HttpApi`, version `0.0.1`). `GET /` serves a
web app, and the old paths now return that HTML with `200`, which is the dangerous part: a v1 call
doesn't fail, it succeeds with the wrong thing.

- Answers: `{ "location": {...}, "data": ... }`. Errors: `{ "_tag": "InvalidRequestError",
  "message": ..., "kind": ... }` with a 4xx status.
- SSE frames: `data: { "id": "evt_…", "created": <ms>, "type": "…", "data": {...}, "durable": {
  "aggregateID": "ses_…", "seq": n, "version": n } }`, separated by blank lines, with `: heartbeat`
  comments between events. No `properties` key.

| What the adapter needs | 1.19.3 | 2.1.1 |
|---|---|---|
| server's listening line | `opencode server listening on http://127.0.0.1:4096` | `server listening on http://127.0.0.1:51709` (parser still works) |
| create a session | `POST /session` `{title}` → `{id}` | `POST /api/session` `{title?, agent?, model?, location?, permissions?, metadata?}` → `{data:{id, projectID, time, title, location}}`; ids are still `ses_…` |
| does a session exist | `GET /session/{id}` | `GET /api/session/{id}`; a missing one is 404 `SessionNotFoundError` |
| send a prompt | `POST /session/{id}/prompt_async` | **No `prompt_async`.** `POST /api/session/{id}/prompt` `{text, files?, agents?, skills?, metadata?, delivery?, resume?}` → `{data:{id, sessionID, type:"user", payload:{text}, delivery:"steer"}}`. It admits the input and schedules the agent loop unless `resume: false` |
| read a turn back | `GET /session/{id}/message?limit=` | `GET /api/session/{id}/message` → `{data:[…], cursor}` |
| model list / variants | `GET /provider` → `{all:[{id, models:{…, variants}}], connected:[…]}` | `GET /api/provider` → `{location, data:[Provider.Info]}` **with no models and no `connected` list**; models are `GET /api/model` → `{data:[{id, modelID, providerID, name, variants:[{id, settings}], enabled, status, cost, limit, …}]}` (382 entries here) |
| events | `GET /event` | `GET /api/event` |
| answer a permission | `/session/{id}/permissions/…` (v1) | `/api/session/{id}/permission`, `/api/session/{id}/permission/{requestID}`, `/api/session/{id}/permission/{requestID}/reply`; also `/api/permission/request` and `/api/permission/saved` |
| answer a question | `question.asked` → `/session/{id}/…` (v1) | **Forms:** `/api/session/{id}/form`, `/api/session/{id}/form/{formID}`, `/api/session/{id}/form/{formID}/reply` |
| config echo | `GET /config` | `GET /api/config` |
| other useful ones | — | `/api/session/{id}/interrupt`, `/…/compact`, `/…/fork`, `/…/agent`, `/…/model`, `/api/model/default`, `/api/agent` |

### 3.3 Events: new names throughout

Observed top-level `type` values for one successful turn, in order:

```
server.connected
session.inbox.enqueued        data: {inboxID, sessionID, item:{type:"user", payload:{text}, delivery:"steer"}}
session.execution.started     data: {sessionID}
session.instructions.updated  data: {sessionID, delta:{…}}
session.inbox.delivered       data: {sessionID, inboxID}
session.step.started          data: {sessionID, agent:"build", model:{id:"glm-5.3-flash-free", providerID:"opencode"}, assistantMessageID, started}
session.retry.scheduled       data: {sessionID, assistantMessageID, attempt, at, error:{type:"provider.rate-limit", message, status:429, response}}
session.reasoning.started     data: {sessionID, assistantMessageID, ordinal, native:{reasoningField}}
session.reasoning.delta       data: {sessionID, assistantMessageID, ordinal, delta?}
session.reasoning.ended       data: {sessionID, assistantMessageID, ordinal, …}
session.text.started          data: {sessionID, assistantMessageID, ordinal}
session.text.delta            data: {sessionID, assistantMessageID, ordinal, delta?}
session.text.ended            data: {sessionID, assistantMessageID, ordinal, text:"Hello there, friend!"}
session.step.ended            data: {sessionID, assistantMessageID, finish:"stop", rawFinish:"stop", cost, tokens:{input,output,reasoning,cache}}
session.usage.updated         data: {sessionID, cost, tokens:{…}}
session.execution.succeeded   data: {sessionID}
skill.updated
```

The v1 names the parser knows (`message.updated`, `message.part.updated`, `permission.asked`,
`permission.replied`, `question.asked`, `question.replied`, `session.error`) **never appeared**. So
`EventParser::push` returns nothing, and a session shows no turn at all.

Not observed (the probe turn used no tools and asked no permission): the tool-call, permission and
form events. Enumerate them from a turn that runs a tool and one that asks before editing. The event
names to look for are probably `session.tool.*`, `session.permission.*`/`permission.*`, and
`session.form.*`/`form.*`; **verify, don't guess.**

Also unverified: what `durable.seq` is for. It looks like a per-session ordered log, which may allow
replaying what a reconnect missed (`GET /api/event` may take a cursor). Worth ten minutes: today a
dropped stream reconnects and silently misses events.

### 3.4 Stored messages: a different shape, and no variant

`GET /api/session/{id}/message` returned `{data, cursor}` with flat entries:

```json
{"id":"msg_…","time":{"created":1792516301633},"text":"Say hello in three words.","type":"user"}
{"id":"msg_…","time":{"created":1792516325012,"streamed":…,"completed":…},"type":"assistant",
 "agent":"build","model":{"id":"glm-5.3-flash-free","providerID":"opencode"},"content":[
   {"type":"reasoning","text":"…","native":{"reasoningField":"reasoning_content"},"time":{…}},
   {"type":"text","text":"Hello there, friend!"}],
 "finish":"stop","rawFinish":"stop","cost":0,"tokens":{…}}
{"id":"msg_…","time":{"created":…},"type":"idle","outcome":"succeeded"}
```

- The user message carries **only its text** — no `agent`, no `model`, no `variant`.
- The assistant message carries the model as `{id, providerID}` (not `{modelID, providerID,
  variant}`), and no variant.
- `session.step.started` carries `agent` and `model` but **no variant**. The variant is accepted in
  the config (§3.5) but not reported back anywhere Chorus has found.
- So `latest_user_turn`, `Observed.variant` and the variant half of `differences` have no source.
  Decision 2 is what to do about L35's "the variant is read back from the stored message" proof.

### 3.5 Configuration still works, and today's shape is accepted

`OPENCODE_CONFIG_CONTENT` is still read. The exact content Chorus generates today was accepted:

```
{"model":"opencode/glm-5.3-flash-free",
 "agent":{"build":{"model":"opencode/glm-5.3-flash-free","variant":"high"},
          "plan":{"model":"opencode/glm-5.3-flash-free","variant":"high"}},
 "permission":{"edit":"ask","bash":"ask","webfetch":"ask","external_directory":"ask"}}
```

`GET /api/config` echoed it normalised:

```
{"model":{"providerID":"opencode","model":"glm-5.3-flash-free"},
 "permissions":[{"action":"edit","resource":"*","effect":"ask"}, …],
 "agents":{"build":{"model":{"providerID":"opencode","model":"glm-5.3-flash-free","variant":"high"}},
           "plan":{…}}}
```

So `config_content` and the `OPENCODE` spec's table entries can stay as they are. Whether the
variant **takes effect** is the open question of §3.4; whether the config **parses** is settled.
A provider block (`providers::Route::opencode_provider`) was not tested in 2.1.1: check it before
P14's Ollama work is merged on top (P14 adds a keyless provider route).

### 3.6 Auto-update still has its switch

- With `OPENCODE_DISABLE_AUTOUPDATE=true`, the TUI logged `update check skipped reason=disabled
  version=2.1.1 channel=latest`. Keep the variable exactly as it is.
- Without it, the TUI logged `update check current=2.1.1 latest=2.1.1 action=up-to-date` and ran
  `npm list -g --depth=0 @opencode/cli`, `pnpm list -g …`, `bun pm ls -g`, `yarn global list` and
  `vp list -g --json …` as child processes. That is five extra process spawns per launch; the
  variable is worth keeping for that reason alone.
- `opencode upgrade` exists; nothing in the adapter should call it.

### 3.7 What still works (don't rewrite it)

- `shim.rs`: the npm shim is the same shape and points at `opencode.exe`.
- `listening_url`: matches the new line.
- `serve_args()`: `serve --port 0 --hostname 127.0.0.1` is unchanged.
- `is_session_id` / `created_session_id`: ids are still `ses_…`, but the id is now under `data`.
- `config_content` and the spec tables (§3.5).
- `OPENCODE_DISABLE_AUTOUPDATE` (§3.6).
- `XDG_STATE_HOME` / `XDG_DATA_HOME` isolation: **not re-verified.** The TUI's remembered model
  choice (`xdg-state\opencode\model.json`, LESSONS L5) may have moved or gone, since 2.1.1 now
  keeps model/variant in the session. Check before trusting `forget_tui_choice`.

## 4. What this breaks, in code terms

- `crates/chorus-core/src/opencode.rs`
  - `check_model` reads `all`/`connected`/`models`; the new body has `data` and no `connected`.
    It returns `None`, so `model_note` shows nothing — a silent gap where "not connected" used to
    be a visible error.
  - `EventParser` listens for names that no longer exist (§3.3).
  - `latest_user_turn` expects `info.role`/`parts`; the new entries are flat (§3.4).
  - `differences` and `Observed.variant` have no variant source.
  - `config_content` is fine.
  - Its header says "Measured on opencode 1.19.3"; the module should say 2.1.1 when this lands.
- `src-tauri/src/agents/opencode.rs`
  - Every path (`/session`, `/session/{id}`, `/session/{id}/message`, `/provider`, `/event`) and
    every response unwrap (`{data}`) changes. `session_exists` must treat a 404 body/envelope, not
    only a status, as "no".
  - `Api::send` already ignores the body shape; that can stay.
- `src-tauri/src/agents/mod.rs`
  - The TUI args: `sessions/mod.rs` builds `attach <url> --session <id>` through
    `opencode::attach_args`. That function is now `--server <url> --session <id>` on the top-level
    program. Check whether `--standalone` is needed to guarantee the background service is never
    consulted; `--server` alone left `service status` at `stopped` in the probe.
  - `conversation_exists` returns `false` for opencode (server sessions), unchanged.
- `sessions/mod.rs`
  - Prompt delivery, the event reader, permission replies, question replies (now forms), read-back
    and the recorder's source all go through the paths above.
- `agent.rs` / `cli.rs`
  - `OPENCODE.verified` becomes the 2.1.1 version, and `cli.rs`'s version-line test gains
    `opencode v2.1.1` (the CLI prints `opencode v2.1.1` **with a `v`**; `parse_version` already
    strips a leading `v`).
  - If 1.x is refused, that refusal is a visible problem string from `agents::resolve`/`detect`, not
    a silent one (D19).
- Tests to rewrite with real fixtures: `PROVIDERS`/`check_model`, `server_events_become_a_turn`,
  `reads_back_what_the_newest_turn_ran_with`, `the_argument_lists`, and any test that names
  `attach`.

## 5. Tasks, in order (commit after each)

1. **Capture the surface.** With the isolated setup of §3, run `/openapi.json` and one turn that
   (a) writes a file (tool events), (b) asks permission (permission events), (c) is interrupted
   (`/api/session/{id}/interrupt`), (d) fails (a bad model). Save each capture as a fixture. Write
   the event vocabulary and the endpoint list into the review file before changing code.
2. **Core: model check.** Rewrite `check_model` against `/api/model` (`data[]`, `variants[].id`).
   Decide how "provider has no key" is detected now that `connected` is gone — `enabled`? the
   provider's `activation`/`integrationID`? Test with a provider that has no credential, and keep
   the visible "no key or login" note working. Unit tests from the captured body.
3. **Core: event parser.** Rewrite `EventParser` for the events of §3.3 (and the captured tool,
   permission, form and failure events). Keep `AgentEvent` unchanged. One test per event family,
   from the fixtures. A turn starts at `session.inbox.enqueued` (or `session.execution.started`) and
   ends at `session.step.ended` with `finish:"stop"` plus `session.execution.succeeded`; a failure is
   `session.execution.failed`/`session.step.ended` with an error, an interruption is the interrupt
   path. Verify each before relying on it.
4. **Core: read-back.** Rewrite `latest_user_turn` for the flat message shape. If the variant really
   isn't reported, make `Observed.variant` `None` and have `differences` say so once (decision 2).
5. **Tauri: the API client.** New paths and `{data}` unwrapping, `session_exists`, create, prompt,
   permission reply, form reply, message read-back. Add a fixture test for each unwrap.
6. **Tauri: the event stream.** `GET /api/event`, the new frame envelope (`{id, type, data}`, plus
   `: heartbeat` comments, which the current splitter tolerates because they have no `data:` line
   and the blank line ends the block). Consider `durable.seq` for reconnect (§3.3), and say in the
   review whether it was used.
7. **Launch and isolation.** `--server <url> --session <id>` (and `--standalone` if it is needed to
   keep the background service out). Confirm the model/variant memory and `forget_tui_choice`
   against 2.1.1. Confirm no `opencode service` is started, stopped or used by a Chorus run.
8. **Options, observed.** On a free model: the model applies (the turn's message/steps name it),
   the plan agent applies (the step's `agent` is `plan`), a permission mode applies (a tool call
   asks or doesn't), and the effort is either observed or reported as requested-only (decision 2).
9. **Resume.** A session started, a run ended, a new run resuming the same `ses_…`, and the TUI
   showing the earlier turn.
10. **The version gate.** 1.x (or an unknown version) shows a visible problem naming the version
    Chorus supports, instead of a launch with no events.
11. **Exit checks and the review file.**

## 6. Decisions for Matthew (ask before building past them)

1. **2.x only, or both?** (a) 2.x only, and 1.x is refused visibly. (b) Both, with a version switch
   and two parsers. *Recommendation: (a).* This machine has 2.1.1, the v1 surface is gone, and dual
   support doubles everything in §4 for a version nobody here runs.
2. **What proves the effort now?** L35's proof — read the variant back from SQLite — has no source
   in 2.1.1 (no variant in messages, steps or events). (a) Ask opencode upstream whether the
   variant is reported anywhere (`/api/session/{id}/model`? the `agents` config echo? the TUI's
   display?). (b) Treat the effort as "requested, not observable", show that in the session note,
   and keep `differences` to model and agent. (c) Drop effort for opencode until it is observable.
   *Recommendation: (a) first, then (b); never silently pretend it was observed.*
3. **The background service.** The 2.1.1 TUI defaults to a shared background service
   (`opencode service`), which is a global daemon across projects and the user's own terminals.
   (a) Never use it: always `--server` our per-session server (and `--standalone` if that is what it
   takes). (b) Use it, for faster starts. *Recommendation: (a).* It matches today's per-session
   server and the job-object rule.
4. **Permissions and questions, mapped how?** v2 has session permissions and forms. (a) Map them to
   the existing `AgentEvent::ApprovalAsked/Answered` and `QuestionAsked/Answered`, so no view
   changes. (b) Model them as they are, with new event variants. *Recommendation: (a)*, unless the
   form shape can't be represented, in which case say what changed and show both in the review.

## 7. Notes on hot files

Same caution as P14 §6. `chorus_core/src/opencode.rs` and `src-tauri/src/agents/opencode.rs` are the
whole job and are not hot. `sessions/mod.rs` is hot (1,700 lines): keep the opencode branches to the
calls they already make. `agent.rs` changes one `verified` field (and P8/P7 are adding fields there;
use defaults and expect a merge). `cli.rs` adds one version line to a test. `labels.ts`, `status.ts`
and `NewSessionForm.vue` need nothing: the kind already exists.

## 8. Verification

- **Unit tests:** model check from a captured `/api/model` body; one test per event family; message
  read-back; each `{data}` unwrap; the `--server` argument list; the version gate; the config
  content still parsing.
- **Runtime, in your `oc2` instance:** launch; each declared option observed (or reported as
  requested-only); a tool call shows in the session view; a permission is asked and answered in the
  UI; a turn ends with its text; an interrupted turn shows as interrupted; a failed turn shows its
  error; resume shows the earlier turn in the TUI; the status shows the turn lifecycle; no
  `opencode service` process starts; auto-update is off (the TUI's log says
  `update check skipped reason=disabled`).
- **Version gate:** with a 1.x on PATH (or a stub that prints `1.19.3`), the session says so and
  never launches blind.
- **CI:** `npm run check` green before each commit.

## 9. Files you will probably touch

`crates/chorus-core/src/opencode.rs` (the bulk), `crates/chorus-core/src/agent.rs` (one field),
`crates/chorus-core/src/cli.rs` (one test line), `src-tauri/src/agents/opencode.rs` (paths and
unwraps), `src-tauri/src/agents/mod.rs` (the TUI arguments, the launch env), and the opencode
branches only of `src-tauri/src/sessions/mod.rs`. New: fixtures for `/api/model`, one per event
family, and the message bodies. Nothing in the front end should need a change.

## 10. Open questions worth settling before the first commit

- Does `GET /api/event` accept a cursor or sequence, so a reconnect doesn't miss events
  (`durable.seq` suggests it)?
- What are the permission and form event names and reply bodies?
- Is `--standalone` needed alongside `--server` to keep the background service out, or does
  `--server` always mean "don't use the service"?
- Does `--server` return non-zero when the server isn't reachable, so a bad launch is visible?
- Where does 2.1.1 keep the TUI's remembered model and variant (the file `forget_tui_choice`
  removes), if anywhere?
- Does a `providers::Route` block (the keyless one P14 adds for Ollama) parse in 2.1.1?
