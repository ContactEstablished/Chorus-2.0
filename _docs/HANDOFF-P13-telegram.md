# Hand-off plan: P13 — Phone relay (Telegram)

**Read [HANDOFF-COMMON.md](HANDOFF-COMMON.md) first.** It holds the isolation, safety, shared-file and
review-file rules this plan relies on. Label `p13`, Vite port 1433, review file
`_docs/HANDOFF-P13-REVIEW.md`.

## 1. Goal, and why this hand-off is split

PLAN P13: prompt sessions, answer questions and approvals, and get digests from your phone, **through
the same action pipeline as Friendly**. Telegram is the only remote access in 2.0 (D16). Also decide
whether Claude Code's and Codex's built-in remote control modes complement it (D35).

That action pipeline (P6's ActionRouter) doesn't exist yet. Hooks only observe today (D46), and
answering an approval from Chorus is P6–P7 work. So this hand-off is in two parts:

- **P13a, build now:** the bot, pairing and security, the long-poll loop, outbound status messages
  and digests, prompting a session from the phone through the shared delivery contract
  (HANDOFF-COMMON §4), and the D35 write-up.
- **P13b, design only:** answering approvals and questions from the phone. Write the interface P13b
  needs from P6's ActionRouter, and show approvals and questions on the phone **read-only** with a
  visible "answer at your desk for now". Don't build a keystroke-based approval path. D29 makes
  Chorus's answers go through structured channels, with the agent's own prompt as the fallback, and
  P6 owns that.

**Exit for this hand-off**, seen working with a real test bot and your `p13` instance:
1. Pairing from the phone works, and a message from any other Telegram account is ignored.
2. An approval, a question and a failure each reach the phone within 5 s, as messages with the
   session's name and project.
3. A message sent from the phone to a Ready Claude Code session runs as its next prompt, and is
   recorded once in prompt history with source `telegram`.
4. `/status` and the daily digest list the sessions that need you.
5. The bot token never appears in a log, an error, the DB (except as a DPAPI blob), argv or the UI.

## 2. Out of scope

- Answering approvals or questions from the phone (P13b, after P6).
- Queueing a phone message while the agent is busy (P6 queues). P13a refuses with the reason, or
  sends anyway where the agent queues natively (decision 4).
- Webhooks. The PC is behind NAT and nothing is exposed (Chorus 1 plan D6), so it uses **long polling
  only**.
- Starting, restarting or closing sessions from the phone.
- Forwarding raw PTY output. Only structured events and Chorus's own text are sent (Chorus 1 plan D10).
- An LLM in the relay. It is deterministic code (Chorus 1 plan D8).

## 3. Ground facts (verified 2026-10-20; re-check before relying on them)

**Chorus 1 never built Telegram.** Its plan, `../Chorus/docs/Telegram/TELEGRAM-PLAN.md` (942 lines,
status Proposed, dated 2026-10-14, for Electron + grammY; `docs/Features/Telegram/` holds an identical
copy), is the reference **design**, not proven behaviour. Read it in full. What carries over:
- **Bot and token:** one bot and one poller per install, because a second `getUpdates` consumer gets
  HTTP 409. Use separate bots for dev and release.
  - Matthew creates the bot with BotFather (`/newbot`). Optionally he sets `/setjoingroups` Disable.
  - The token lives in the vault, is checked with `getMe`, and is **never echoed or logged**. The
    plan scrubs errors for `/bot\d+:[A-Za-z0-9_-]+/`.
- **Pairing:**
  - Chorus shows a 6-digit code (10 min expiry, single use, lockout after 5 wrong tries). The user
    sends `/pair 123456` to the bot.
  - The sender's `from.id` joins the allow-list. Accept an update only from an allowed user in an
    allowed chat; drop everything else and audit it.
  - Updates are limited to `message` and `callback_query` (plus `my_chat_member`). Bot senders and
    edited messages are ignored.
- **Backlog guard:** a message older than 120 s is never sent to an agent automatically. It gets
  [Send] / [Discard] buttons instead.
- **Inbound pipeline:** auth → dedupe → backlog → parse → target → state gate → sanitize → deliver →
  acknowledge → audit.
  - The target, in priority order: reply-to (a Chorus message, looked up in a message map, checking
    the run hasn't changed), then a prefix, then sticky `/use`, then the only live session, then ask
    with buttons.
  - Every acknowledgement names its destination.
- **Sanitizer:** strip C0/C1 controls and ESC/CSI/OSC sequences, and cap at 4,000 characters.
- **Callbacks:** `callback_data` is `v1:<short token>`. The token maps server-side to its target and
  is single-use, with a 30 min expiry. **Text never goes in callback data** (its limit is 64 bytes).
  Always `answerCallbackQuery`.
- **Outbound:**
  - The event-to-message table is in the plan, around lines 553–560.
  - It uses HTML parse mode (the Telegram subset). Truncate the source text *before* rendering.
  - A reply over 3,500 characters is cut, with a "Full reply" document attached.
  - Finishes within 3 s of each other are coalesced.
- **Rate limits:** about 1 message per second per chat, using a token bucket. Honour `retry_after` on
  HTTP 429. Back off 1–60 s on network errors. On a 409 at start, retry for 90 s.
- **Privacy:** bot chats are **not end-to-end encrypted**. The plan's mirror modes are `full`,
  `summary` and `status`, with `status` recommended for client repositories (decision 2).
- **Rejected:** Claude Code's own "Channels" Telegram plugin (Claude-only, one bot per session, a
  research preview).

**Chorus 2.0 today**
- **Writing to a session:** `session_write` (`sessions/commands.rs` ~56) calls `SessionManager::write`
  (`sessions/mod.rs` ~1400). That writes raw bytes to the PTY with a 1 MiB cap. It doesn't sanitize,
  record a source or check state. **Use the delivery contract instead** (HANDOFF-COMMON §4):
  - P8 owns it. Run `git log main -- src-tauri/src/sessions/deliver.rs`.
  - If it is on `main`, rebase onto it and use it.
  - If not, implement the **same contract in the same file**, exactly as specified, and say so in
    your review file. The reviewer keeps one copy.
- **`prompts.source`** is free text. Only `"hook"` and `"server"` are written today, so a phone
  prompt written to the PTY would come back through `UserPromptSubmit` as `"hook"`. The contract's
  "record once" rule fixes that.
- **Asks:**
  - `chorus-core/src/events.rs` defines `ApprovalAsked`, `ApprovalAnswered`, `QuestionAsked` and
    `QuestionAnswered`.
  - `events/recorder.rs` (~181–205) writes them to `asks` through `history::open_ask` and
    `answer_ask`.
  - Answers are only **inferred**: a tool starting means "allowed".
  - Session state reaches the UI as `SessionUpdated` carrying the D47 `attention` word.
- **Hooks only observe:** the listener (`events/listener.rs`) returns an empty 204. S5 proved that a
  held `PermissionRequest` can allow or deny, and that an unanswered one times out (30 s in S5) and
  leaves the agent's own dialog up. Phone latency makes that timeout P13b's central design problem.
  Note it in the P13b interface.
- **Vault:**
  - `vault::add(store, label, secret)` (DPAPI) and `vault::decrypt(store, id)`.
  - `vault::all_for_scrubbing` means a stored token is scrubbed from session output automatically.
  - There is precedent for a config row that points at a credential id
    (`providers/keys.rs`, `management_credential_id`).
- **HTTP:** `reqwest` 0.13 (json, native-tls) is already a dependency. The shared client in
  `providers/mod.rs` (~23–38) has a fixed timeout that is too short for a 30 s long poll, so build the
  relay its own client (timeout ≈ 40 s). **A `reqwest::Error`'s text includes the URL, and Telegram's
  URL contains the token**: call `.without_url()` before formatting any error.
- **Secret patterns:** `crates/chorus-core/secret-patterns.json` has no Telegram token pattern. Add
  one at the end (HANDOFF-COMMON §3), with a test.

## 4. Design

- **Core (`chorus-core::relay`, pure, with tests):**
  - Parsing updates and commands, the auth guard (allow-list plus chat), pairing codes (pass the code
    generator's randomness in; the expiry and lockout state machine is pure), and the backlog guard.
  - Target resolution and the sanitizer.
  - The rate-limit bucket (pass time in).
  - The callback token table, message rendering (HTML escaping, truncation, mirror modes), the digest
    text, and dedupe by `update_id`.
- **Back end (a new `src-tauri/src/relay/` module):**
  - `client.rs`: the Bot API calls (`getMe`, `getUpdates` with `timeout=30` and `allowed_updates`,
    `sendMessage`, `editMessageText`, `answerCallbackQuery`, `setMyCommands`, `sendDocument`). Every
    error goes through `.without_url()` and the scrubber.
  - `poller.rs`: one thread that long-polls and keeps the offset persisted. It stops cleanly on quit,
    and backs off as the plan says.
  - `outbound.rs`: its **own** subscription to session state changes. Don't route through P7's
    notification code (HANDOFF-COMMON §3). It sends approval, question, failure and (decision 3) turn
    finished, each at most once per episode, using the same dedupe idea as P7.
  - `inbound.rs`: the pipeline from §3, delivering through `deliver_prompt(…, PromptSource::Telegram)`.
  - `commands`: `relay_status`, `relay_set_token` (stored in the vault), `relay_pair_start` (returns
    the code shown in the app), `relay_unpair`, `relay_set_mode`, `relay_test` (sends one test
    message).
- **Data, in one migration:**
  - `relay_config`: a single row with credential_id, mode, enabled, digest_time and poll_offset.
  - `relay_users`: telegram_user_id, chat_id, display name, paired_at.
  - `relay_messages`: telegram message_id, chat_id, session_id, run_id, kind and sent_at, for reply-to
    targeting.
  - `relay_audit`: kind, user_id, chat_id, at, and a short scrubbed note. Never the full text.
- **Settings → Phone (Telegram)** card:
  - It shows the connection state, with fail-visible reasons: no token, token rejected, 409 because
    another poller is running, offline.
  - Fields and actions: token entry (never shown back), Pair (shows the code and its expiry), the
    paired users with Unpair, the mirror mode, digest time, Send test, and an on/off switch.
- **Phone commands (P13a):**
  - `/help`, `/status`, `/sessions` (numbered), `/use <n|name>`, `/digest`.
  - A plain message, or a reply to a Chorus message, sends a prompt.
  - Register them with `setMyCommands`.
- **DM only for P13a** (decision 1).

## 5. Tasks, in order (commit after each)

1. **Read the Chorus 1 plan in full.** Check the current Bot API docs for every method and field you
   use (core.telegram.org/bots/api), and record the Bot API version.
2. **Core module** with tests: pairing, auth, sanitizer, targets, rendering, rate limit, callbacks,
   digest, and the Telegram secret pattern.
3. **Migration, client and vault-backed token,** plus Settings → Phone with token entry and `getMe`.
   - Prove the token is never printed. Put a planted fake token in a failing request and check the
     error text, logs and UI.
4. **Pairing and the poller.**
   - Long poll, offset, 409 handling, backoff, and a clean stop on quit. Pairing end to end, and
     unauthorised senders dropped and audited.
5. **Outbound.** Approval, question and failure messages, plus the turn-finished policy, rendered per
   the mirror mode. Coalescing, rate limits and `retry_after`.
6. **Inbound prompts** through the delivery contract: state gate (decision 4), backlog guard,
   acknowledgements naming the destination, and the reply-to message map.
7. **`/status`, `/sessions`, `/use`, `/digest` and the scheduled daily digest.** The digest lists
   sessions needing you (oldest first, with wait time), finished today, and failed.
8. **Read-only approvals and questions:** show them on the phone with the options as text and
   "Answer at your desk for now (phone answers arrive with P6)". No buttons that pretend to work.
9. **D35 write-up, no code.** Read the installed `claude --help` and `codex --help` (and docs) for
   their remote-control modes. Record what they do, where traffic goes, how they authenticate, and
   whether they would bypass Chorus's history, scrubber and attribution. Recommend in the review file
   (a proposed decision).
10. **P13b interface, written in the review file.** Specify the calls P13b needs from P6's
    ActionRouter: answer an approval (allow, deny, deny with a message), answer a question (an option
    or free text), and the timeout and fallback behaviour when the phone answers late (D29: the
    agent's own prompt stays live). Add the callback-token flow that would carry them. Make it
    concrete enough that P6 can implement the other side.
11. **Exit checks and review file** (§8).

## 6. Decisions for Matthew (ask before building past them)

1. **Direct messages or a forum group?** Chorus 1's plan preferred a private forum supergroup with one
   topic per project, because DM topics broke after a Bot API change.
   - (a) DM only for P13a: one chat, with the session named in each message. Topics come later.
   - (b) Forum supergroup with topics now: tidier with many projects, but it needs group setup and
     `can_manage_topics`, and adds a "more than 2 members" risk check.
   - *Recommendation: (a). It is one user, it is simpler and safer, and topics can be added without
     changing the pipeline.*
2. **What text leaves the machine?** Bot chats aren't end-to-end encrypted, and Telegram stores them.
   - (a) Status only by default (session name, project, state, the question's or approval's short
     title), with full final messages opt-in per project.
   - (b) Summaries by default: the first ~500 characters of final messages, scrubbed.
   - (c) Everything, scrubbed.
   - *Recommendation: (a). Some projects are client work.*
3. **Notify when a turn finishes?** Recommendation: only for turns started from the phone, or when the
   desk has been idle ≥ 5 minutes (Chorus 1's rule). Use GetLastInputInfo, as P11 does. Otherwise
   finishes are noise.
4. **A phone prompt while the agent is busy.**
   - (a) Refuse, with "Session is working; send again when it's ready, or wait for queueing (P6)".
   - (b) Send it anyway where the agent queues typed prompts natively (Claude Code does, ARCHITECTURE
     §5), and refuse elsewhere.
   - *Recommendation: (a) for now. One rule for all agents; P6 brings real queueing.*
5. **The test bot.** Runtime checks need Matthew to create a **dev-only** bot with BotFather and enter
   its token in your `p13` instance's Settings (never in chat). Ask him when you reach Task 3. Until
   then, test against unit fixtures.

## 7. Security checklist (each one goes in the review file with how you verified it)

- The token is stored only as a DPAPI blob, used only in request URLs, and never formatted into an
  error (`.without_url()`), a log, an event or the UI. It is in the scrubber's list.
- Every outgoing text passes the scrubber with every vault secret, after rendering.
- Only paired users in their own DM are accepted. A forwarded or edited message, or one from a bot,
  never becomes a prompt.
- Pairing codes are single-use, expire, and lock out after 5 wrong tries.
- A phone prompt is sanitised, capped and recorded before delivery, and it never answers an approval.
- Nothing listens on a port. Long polling only.

## 8. Verification

- **Unit tests:** all of §4's core pieces, including a forged chat id, an unknown user, a stale
  callback, a 4,001-character message, an ESC sequence in the text, and the token pattern.
- **Runtime** (with the test bot, your phone or Matthew's, and your `p13` instance):
  1. Pair. A second Telegram account (or a group the bot is added to) is ignored, and audited.
  2. A Claude Code session in Manual permission is asked to create a file. The approval message
     arrives in ≤ 5 s and is read-only.
  3. Reply "say hello in one word" to a Ready session's message. One prompt row appears, with source
     `telegram`, and the turn runs.
  4. A message sent while the session is working is refused with the reason.
  5. Kill the network (or block `api.telegram.org`) for a minute. The poller backs off, the Settings
     card shows "offline", and it recovers.
  6. Run a second instance against the same bot (or `getUpdates` from a script). The 409 is shown,
     not hidden.
  7. Grep the logs, the DB and `_verify` captures for the token, by hash or length only.

## 9. Files you will probably touch

New: `crates/chorus-core/src/relay/*`, `src-tauri/src/relay/*`, `src/views/settings/PhoneSettings.vue`,
`src/stores/relay.ts`, one migration, and `src-tauri/src/sessions/deliver.rs` (only if P8's isn't on
`main`).
Hot (minimal): `lib.rs` (start the relay; your own state-change listener), `ipc.rs`, `store/mod.rs`,
`SettingsView.vue` (one card), `secret-patterns.json` (append).
