# Hand-off plan: P11 — Measurement, made visible

**Read [HANDOFF-COMMON.md](HANDOFF-COMMON.md) first.** It holds the isolation, safety, shared-file and
review-file rules this plan relies on. Label `p11`, Vite port 1431, review file
`_docs/HANDOFF-P11-REVIEW.md`.

## 1. Goal

PLAN P11 covers six measures: an attention sampler, per-task cost attribution, subscription metering,
a token ledger and a day summary, **all with UI this time**. The UI is the session facts ("Tokens"),
the Overview headline and a Usage screen. Mission Control's forecasts come only once P3's turn data
meets its completeness bar (D18).

**Exit** (PLAN gives none; this is the one to meet), seen in your `p11` instance:
- A Claude Code turn, a Codex turn and an opencode turn each show **tokens** in session facts. The
  Claude figure includes subagent work (L6).
- An OpenRouter run with a capped key shows its **cost** after the key is revoked (L16).
- The Usage screen shows today's tokens per agent, the cost, attended time, and how complete the turn
  data is.
- Unknowns show as unknown, never as 0.

Principle 9 applies throughout: **measure before forecasting.** Every number says where it came from
and how complete it is.

## 2. Out of scope

- Forecasts. Build the completeness readout that gates them; the forecasts themselves wait for
  Matthew (decision 5).
- An LLM-written day summary. That is the narrator's job (P6, D28), and it can feed this later.
- Vendor quota windows read by scraping the TUI (for example Codex `/status` or Claude `/usage`).
  Chorus only reports what it observed (decision 3).
- Routing tiers and endpoint samples (P12).

## 3. Ground facts (verified 2026-10-20; re-check before relying on them)

**Chorus 1** (`../Chorus/src/main/services/`):
- **Attention sampler** (`attentionCore.ts` is pure; `attention.ts` does the IO):
  - **Inputs:**
    - OS idle seconds (`powerMonitor.getSystemIdleTime()`, which is GetLastInputInfo on Windows).
    - The main window's focus and minimise state.
    - Lock and suspend.
    - A front-end report of the focused session and view, sent only on change.
  - **Timing:** `TICK_SECONDS = 15` and `IDLE_THRESHOLD_SECONDS = 60`, from one interval for the
    whole app.
  - **Classes:** `pane | overhead | blurred | idle | locked`. The first match wins, in this order:
    locked, idle (≥ 60 s), blurred, then the view rules (`attentionCore.ts:138-196`).
  - **Each firing credits exactly one sample.** A firing more than 2 ticks late sets a gap and closes
    the span instead of extending it. Credited time is always samples × tick.
  - **Coverage:** a coverage measure (expected samples against credited ones) exposes holes left by
    a crash.
  - **Privacy:** local only, no log line per tick, never readable by agents. It has a kill switch.
  - **Tests to port:** `attentionCore.test.ts`, especially the focus-state table (60-151),
    precedence (291-326), the exact ≥ 60 s boundary (328), "10 min late credits one sample" (378),
    samples × tick (400), 3 min with no input giving 4 pane then 8 idle samples (447), and coverage
    (541-607).
  - `attentionRollup.ts` is **not** measurement: it is the project light, which 2.0 already has in
    `chorus_core::attention`.
- **Token ledger** (`engineLedgerCore.ts`, `engineLedger.ts`, `src/shared/engineUsageView.ts`):
  - **Per Claude assistant entry, four numbers:**
    - CE = input + 1.25·eph5m + 2.0·eph1h + 0.1·cache_read
    - RLIT = input + cache_creation
    - Naive = input + cache_creation + cache_read
    - Output, kept separately
  - **Sum the parent `message.usage` only:** `usage.iterations[]` repeats it (L6).
  - **Include `<transcript minus .jsonl>/subagents/agent-*.jsonl`.** Don't recurse, and never open
    `.meta.json`. These files are 53–65 % of the cost.
  - **Reading:** a byte cursor per file, scanned on demand. It handles a partial last line,
    truncation and a vanished file.
  - **Display:** a row is `measured | unknown | no-source`. An unknown row has no bar. A coverage
    line reads "N of M carry token data".
  - **Tests:** `engineLedgerCore.test.ts` (the iterations case at 118, sidechains at 289),
    `engineLedger.test.ts` 119-239 (the cursor).
- **Codex tokens:** only in Chorus 1's audit script (`teamCodexAudit.test.ts`). The rollout has
  `token_count` events with `info.total_token_usage` (a running total) and per-turn usage
  `{input_tokens, cached_input_tokens, output_tokens, reasoning_output_tokens, total_tokens}`.
  **Verify this against today's Codex rollouts.**
- **opencode tokens:** Chorus 1 read none.
- **Cost attribution** (`attributionCore.ts`, `dispatchAttribution.ts`):
  - A subscription run is counted, never priced. An API-key run is priced from a capped key's
    analytics.
  - **Token columns:** `tokens_in` is the total including cache. `tokens_cached` and
    `tokens_cache_write` are subsets of it, never added to it. `tokens_out` is separate. `cost_usd`
    and `tokens_source` record the price and where it came from.
  - **Unknown is null, never 0.**
  - **Tests:** `attributionCore.test.ts` (billing separation at 62, 0 against unknown at 238, cached
    never folded into input at 288, the reconcile matrix at 428-526).
- **Subscription metering** (`subscriptionMeter.ts`) summed Claude transcript usage inside a time
  window. If more than one Claude session id fell in the window, the answer was unknown. Nothing read
  quota windows.
- **Day summary** (`dayReportCore.ts`) was built from git: the user's own commits that day, and files
  changed that day. An optional LLM wrote 1–3 sentences, and the bullets never depended on it. It was
  stored in `day_reports`.

**Chorus 2.0 today**
- **No token, usage or cost parsing anywhere.**
  - `chorus-core/src/claude.rs` reads the transcript only to spot interrupts. `codex.rs` reads only
    the rollout's head.
  - `opencode.rs` (~228–258) handles `message.updated` for assistant messages, but **ignores its
    `tokens` and `cost` fields**.
- **Transcripts:** `events/recorder.rs` (~91) saves the first `transcript_path` from a hook into
  `runs.transcript_path` (`0006`). `history::session_transcript` looks it up. Codex's rollout is found
  through D43's originator marker. `sessions/mod.rs` (~1175–1215) has a transcript watcher for
  interrupts.
- **`turns`** (`0005`): id, session_id, run_id, agent_turn, prompt_id, started_at, ended_at,
  end_reason, final_message. **There is no tokens column**, although ARCHITECTURE §7 sketches one.
- **Capped keys:**
  - `0003` has the table `capped_keys(…, limit_usd, minted_at, revoked_at, cost_usd, cost_note)`.
  - `providers/keys.rs`: `settle` revokes the key, then polls analytics 6 × 5 s and keeps the highest
    figure. `sweep` handles leftovers at start-up.
  - Session facts already show "Run key" with its cost (`SessionFacts.vue` ~27–32 and ~74–77).
  - Catalog prices per million tokens are in `chorus-core/src/providers.rs` (~207–228).
- **None of ARCHITECTURE §7's measurement tables exist** (`attention_samples`, `costs`, `usage_meter`).
- **Overview headline:** `headline(counts)` in `src/shell/status.ts` (~44–61), used in
  `OverviewView.vue`.
- **There is no Usage screen.** `ViewName` and `Rail.vue`'s list would need it (decision 1).
- **The `windows` crate** already enables `Win32_UI_Input_KeyboardAndMouse` (for GetLastInputInfo),
  `Win32_UI_WindowsAndMessaging` and `Win32_System_Threading`. Lock detection through
  `WTSRegisterSessionNotification` would need `Win32_System_RemoteDesktop`, which is not enabled
  (decision 4).
- **Synthetic input doesn't reset GetLastInputInfo** (L10). Prove idle with real input.
- **Contention:** P5 adds a "Changes" row to the same `SessionFacts.vue` list. P7 adds a field to
  `SessionSummary`, and so does P8.

## 4. Design

- **Core (`chorus-core::measure`, pure, with tests):**
  - **`ledger`:** a `TokenUsage { input, cached, cache_write, output, reasoning }` where every field is
    `Option<u64>`, plus a source (`claude-transcript`, `codex-rollout`, `opencode-server`,
    `openrouter-analytics`).
    - Parsers per agent, taking lines or JSON values (no IO). The Claude rules come from Chorus 1
      (parent `usage` only, subagent files included). Codex parses `token_count`. opencode parses
      `message.updated` tokens and cost.
    - Attributing usage to a turn uses the turn's time window, or the agent's own prompt or turn id
      where the transcript carries one. Check whether Claude's entries carry `promptId`, and prefer it.
  - **`attention`:** a port of the sampler core, with the tick, idle threshold, classes, precedence,
    gaps and coverage. Rename its classes to 2.0's views: Session or Wall terminal focus is "session";
    other screens are "overhead".
  - **`cost`:** turns tokens into an estimated cost from catalog prices (marked `estimate`), and
    reconciles that with analytics (marked `measured`). A subscription run is counted, never priced.
  - **`completeness`:** the share of turns with both start and end, per agent, over a window. It is
    D18's gate.
  - **`day`:** a day summary from Chorus's own data: sessions run, turns, prompts, tokens and cost per
    agent, attended time, and documents written if P8's table exists (or leave that out). Optionally,
    commit counts per project through the existing `git` module.
- **Data, in one migration:**
  - `turn_usage(turn_id PK FK → turns ON DELETE CASCADE, input, cached, cache_write, output,
    reasoning, cost_usd, cost_kind, source, updated_at)`, with every number nullable.
  - `attention_samples` as spans: `(id, started_at, ended_at, seconds, class, session_id NULL,
    project_id NULL, tick_seconds)`.
  - `run_costs` per run and credential, unless `capped_keys.cost_usd` already covers it. Don't
    duplicate it; say what you chose.
  - The settings keys `attention_capture` (on or off) and `usage_since`.
- **Back end (a new `src-tauri/src/measure/` module):**
  - `ledger` reads transcripts and rollouts with a byte cursor per file, when a turn ends and lazily
    when the Usage screen opens. It is never driven by every hook.
  - `sampler` runs a 15 s tick thread: GetLastInputInfo, plus the main window's focus and visibility,
    plus the focused session the front end reports.
  - opencode tokens come from the server events it already receives. Add one call where
    `message.updated` is handled.
  - Run costs are added after `providers::keys::settle`.
- **UI:**
  - **Session facts:** "Tokens: 41.2k in (38.0k cached) · 2.1k out", and "Cost: $0.04 (measured)" or
    "estimate", or "subscription". Unknown says "unknown" and gives the reason in a tooltip.
  - **Overview headline:** add "· 1.2M tokens today · 3h 10m attended". Keep it to one line, and
    **don't change** `headline()`'s existing counts.
  - **Usage screen:**
    - Today, the last 7 days and the last 30, per agent and per project: tokens, cost (split into
      measured, estimated and subscription), and attended time.
    - Completeness per agent ("turns recorded with start and end: 98 % of 212").
    - A coverage line for the attention sampler.
    - The day summary.
  - **Palette:** a "Today" entry that shows the day summary.

## 5. Tasks, in order (commit after each)

1. **Verify the token sources against the installed CLIs.** Record the versions.
   - **Claude Code:** a real transcript and its `subagents/` folder. Check the `usage` fields and
     whether entries carry a prompt id.
   - **Codex:** a real rollout's `token_count` and per-turn usage. Use files on disk from your own
     tiny turns, not Matthew's real project transcripts. Copy fixtures **only after removing all
     content except the usage fields.**
   - **opencode 2.1.1:** `message.updated` tokens and cost.
   - Save trimmed **fixtures** (usage fields only; no prompts, paths or code) under
     `crates/chorus-core/tests/fixtures/` or beside the module, as the repo already does for parsers.
2. **The core ledger and parsers,** with tests ported from Chorus 1 and the fixtures.
3. **Migration and back-end ledger.**
   - Fill `turn_usage` when a turn ends. Back-fill lazily for earlier turns of runs whose transcript
     still exists.
   - Commands: `session_usage(session_id)` and `usage_summary(range, group_by)`.
   - Add `tokens` to `SessionSummary` or to session facts. Choose the one that costs less on the hot
     path, and say why.
4. **Cost.** Estimates from the catalog, run costs measured after revoke, and the subscription label.
   Port the reconcile tests.
5. **Attention sampler** (after decision 2).
   - The core port with its tests, the tick thread, the front end's focus report (a tiny `view`
     store watcher that sends a command only on change), the kill switch in Settings, and the coverage
     readout.
6. **Completeness and the day summary.** The core, a command, the Usage section, and the palette
   entry.
7. **UI.** The session facts rows, the Overview headline, and the Usage screen (placed per
   decision 1).
8. **Exit checks and review file** (§8).

## 6. Decisions for Matthew (ask before building past them)

1. **Where does Usage live?** DESIGN's rail doesn't have it, and PLAN calls it "a Usage screen".
   - (a) A rail entry "Usage" below Notes. It's one more line in the rail and needs a DESIGN edit.
   - (b) A Usage tab in Settings. That leaves the rail as in the mockup, but hides something you look
     at often.
   - (c) Reachable only from the Overview headline and the palette.
   - *Recommendation: (a). It is a screen you return to daily, and the headline links to it.* Build
     the screen as its own component either way, so moving it costs one line.
2. **Is the attention sampler on by default?** It records only local spans of which view had focus
   and for how long, never content, and nothing leaves the machine. The data feeds attended time and
   per-task attribution.
   - (a) On, with a Settings switch and a note on what it records.
   - (b) Off until turned on.
   - *Recommendation: (a), with the note visible on the Usage screen.*
3. **What does "subscription metering" mean in 2.0?** Chorus can't see vendor quotas without scraping
   the TUI (principle 3).
   - (a) Observed usage only: tokens on subscription runs per day, and per rolling 5 h for Claude
     Code. Label it "as seen by Chorus, not your plan's limit".
   - (b) Also read the vendor's own limit reports, if a CLI writes them to a file or hook (verify
     first).
   - *Recommendation: (a) now. Report anything you find for (b).*
4. **Detect a locked screen?** That needs the `windows` crate's `Win32_System_RemoteDesktop` feature
   (WTS session notifications).
   - (a) Yes, add the feature, so locked time is its own class.
   - (b) No. Locked time shows as idle after 60 s, which is what the totals need anyway.
   - *Recommendation: (b). One less feature, and the same totals.*
5. **Forecasts.** D18 allows them once turn capture is proven ≥ 95 % complete; P3 measured 20 of 20
   per agent.
   - (a) Out of this hand-off. Show completeness, and let Matthew decide later with the data in hand.
   - (b) Build a simple per-session ETA from turn durations.
   - *Recommendation: (a). Principle 9, and a separate decision from building the meter.*

## 7. Notes on hot files

- `events/recorder.rs`: at most one call at turn end, to schedule a ledger read. Do the work in
  `measure`.
- `chorus-core/src/opencode.rs`: return the tokens from the existing `message.updated` parse. Don't
  restructure it.
- `sessions/mod.rs`: one summary field, if you choose that.
- `SessionFacts.vue`: add rows. Don't reorder the existing ones; P5 is adding "Changes".
- `OverviewView.vue` and `status.ts`: an extra headline segment only.

## 8. Verification

- **Unit tests:** the Claude ledger (iterations not doubled, subagents counted, a partial last line,
  truncation), Codex `token_count`, opencode tokens, the cost estimate against measured, unknown
  against 0, the sampler table and its boundaries, coverage, completeness, and the day summary.
- **Runtime, in your `p11` instance:**
  1. One tiny Claude Code turn that uses a subagent (ask it to use the Task or Agent tool for a
     trivial search). Its Tokens row includes the subagent's usage. Cross-check by hand against the
     files, and record both totals.
  2. One tiny Codex turn and one opencode turn on a `:free` model show tokens.
  3. Cost: only with Matthew's go-ahead, one OpenRouter run on a `:free` model through a capped key.
     After the revoke, Cost shows "measured" ($0.00 is a real value for free models; unknown is not 0).
     Without his go-ahead, test it with unit fixtures and say so.
  4. Idle: with **real** input, then 2 minutes without touching anything, the spans show session time
     and then idle. Prove it with `keybd_event` or real use, never synthetic events (L10).
  5. The Usage screen and the headline agree with the per-session figures for today.
  6. Kill the app mid-turn: at most one tick (15 s) of attention is lost, and coverage shows the hole.

## 9. Files you will probably touch

New: `crates/chorus-core/src/measure/{mod,ledger,attention,cost,completeness,day}.rs`, fixtures,
`src-tauri/src/measure/*`, `src/views/UsageView.vue`, `src/stores/usage.ts`, one migration.
Hot (minimal): `events/recorder.rs`, `chorus-core/src/opencode.rs`, `sessions/mod.rs` (maybe),
`SessionFacts.vue`, `OverviewView.vue`, `status.ts`, `App.vue`, `Rail.vue` and `stores/view.ts` (only
for decision 1 (a)), `src/shell/palette.ts`, `lib.rs`, `ipc.rs`, `store/mod.rs`.
