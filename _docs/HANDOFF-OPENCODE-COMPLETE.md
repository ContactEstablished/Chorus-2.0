# Review: opencode support completed, opencode 2.1.4

## 1. Verdict

Done. opencode is usable from New session again. It runs on the free models, on OpenRouter through
every routing tier, and on resume. Its tokens and its cost estimate are recorded per turn. The oc2 and
P12 follow-ups are built, except two items that could not be reproduced (§5). Every item in §4 was
seen working in a dev instance, or measured against a scratch `opencode serve`, as each row says.

## 2. Why the tile was disabled

`@opencode/cli` 2.1.4 was installed and complete in `%APPDATA%\npm`, but that folder was on neither
the user PATH nor the machine PATH. Chorus resolved agents only from its inherited PATH, so detection
said "opencode.cmd not found on PATH". The installed CLI was fine and no feature was missing.

## 3. Branch

`feature/opencode-complete`, from `main` at `6733c23`, in eight commits:
- detection (D106);
- the small fixes;
- turns (D108);
- tokens and cost (D107);
- subagents, forms and stalled streams;
- the stream's runtime;
- Nitro cost, with the D60–D64 status;
- this close-out.

Migration **0025** (`turn_usage`) is new; 0023 and 0024 are unchanged.

## 4. Verification

**Checks:** `cargo test --workspace`, `cargo clippy -- -D warnings`, `npm run bindings` (current),
`npm run typecheck`, `npm test` (268) and the secret scan all pass.

**Runtime.** A dev instance with its own identifier and data folder was started from a shell whose
PATH had no `%APPDATA%\npm`, and driven over its WebView's DevTools port. The installed app and its
data were not touched; the OpenRouter keys were added by Matthew in that instance.

| Check | What was seen |
|---|---|
| Detection | opencode found, 2.1.4, with "Found in npm's global folder (…), which isn't on your PATH. Chorus adds it for this agent." on the tile and in Settings → Agents. Gemini's error names both places. |
| Free model, Manual | "Runs on opencode's free models only (no provider)"; a shell call showed Awaiting approval; Allow once in the TUI; turn `done` "DONE", the approval recorded as allowed, the tool `shell: echo two`. |
| Tokens and cost | "Tokens: 15.9k in (15.6k cached) · 41 out · 13 reasoning", "Cost: $0.00 (reported by opencode)". That label was changed later to "opencode's estimate" (L72), which was seen only on the Nitro session. |
| Event stream | The first build with the new read timeout panicked "no reactor running" on the watcher thread, so the session showed "no event". Fixed (L71) and seen working afterwards. |
| Refresh | 16 endpoints, 14 cache-tested, $0.0165 (OpenRouter rate-limited the rest of the test). Tiers: Budget and Balanced CoreWeave fp8, Fast Venice fp8, Nitro likely Fireworks. |
| Each tier | Budget, Balanced, Fast and Nitro each ran one turn, `done` "OK". The facts read "Balanced · coreweave/fp8, then 2 more" and "Nitro: OpenRouter picks the fastest endpoint". The observed model matched the one asked for, `…:nitro` for Nitro. Each run had its own key capped at $3, revoked at Close. |
| Restart | Balanced kept its selection, with no new ranking, and the same `ses_…`. The resumed turn answered about the earlier one. |
| Refusal | `anthropic/claude-haiku-4.5` was refused both in the tiers view and at launch with routing: "Chorus doesn't route Anthropic or OpenAI models through OpenRouter…" |
| New session | The routing section shows the five rows with providers and speeds, Refresh, and "Updated 2 min ago". |
| Nitro cost | opencode reported $0 while the key was billed $0.0024 (L72). After the fix, Cost reads "unknown", with the reason in its tooltip. |
| Job Object | The dev app was killed twice with opencode running; no `opencode.exe` was left. |

**Measured against a scratch `opencode serve` 2.1.4** (isolated `XDG_*` folders, free model, driven
through its HTTP API):
- compaction, synthetic items, steering and cancelling a queued prompt (L69);
- Allow once and Always allow, a three-field form, and a subagent (L70).

The parser tests use trimmed copies of these captures: no prompts or paths.

**Spend:** $0.0165 for Refresh plus $0.0072 billed across six run keys, about **$0.024** in all.

## 5. Not verified

- **`activation: "disabled"`.** 2.1.4 lists a configured provider with no key as `enabled`, and no
  configuration field or API turns one off, so the "switched off" note is still tested with a fixture
  only.
- **A real dropped event stream.** The reconnect is tested against a loopback server that closes or
  stalls; a drop could not be provoked against opencode. Q22 stays (a).
- **Which endpoint each tier's request actually used.** That is on OpenRouter's activity page, which
  needs Matthew's sign-in. Chorus sent each tier's endpoint list, and opencode sends it (L41).

## 6. Decisions and lessons

- **D106:** npm's global folder as a fallback after PATH.
- **D107:** opencode's tokens and cost per turn, lifting D102 for this slice only.
- **D108:** turns versus housekeeping, and a finished steered prompt keeps its reply.
- D60, D61, D62 and D64 are now Decided. Q23 is answered and Q22 is updated.
- **Lessons:** L67 (npm 12 install scripts), L68 (npm's folder off PATH), L69 (2.1.4's usage and
  inbox), L70 (subagents, forms, Always allow, activation), L71 (reqwest read timeout needs a runtime),
  L72 (opencode's cost is an estimate, and 0 for `:nitro`).
- **The verified version is now 2.1.4** (`agent.rs`).

## 7. Left as they were

- **P11's other parts stay held (D102):** the Usage screen, the attention sampler, Claude and Codex
  tokens, and the Overview headline.
- **P14 stays held:** `installHint`, and the Settings → Agents sentence (Q24).
- Routing tiers are for opencode on OpenRouter only.
- The dev instance's data folder `…chorus2.dev.oc` holds Matthew's OpenRouter keys, encrypted with
  DPAPI. Delete it when the review is done.
