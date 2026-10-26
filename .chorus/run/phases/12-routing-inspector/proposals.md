# P12 · Routing inspector: proposals for `_docs/`

Written by the phase's last task. Nothing under `_docs/` was edited: another branch (`p15/loop`)
already uses the next decision numbers, so these are unnumbered. Matthew numbers them and moves each to
`DECISIONS.md`, `LESSONS.md`, `PLAN.md` or `OPEN-QUESTIONS.md`. The tables and paragraphs follow
those files' style.

## Proposed decisions

All proposed on 2026-10-25, at the P12 kickoff (the run's resumption of P12, D102).

| Date | Decision | Status | Why |
|---|---|---|---|
| 2026-10-25 | **Routing refuses Anthropic and OpenAI models, and says why.** A model whose author part is `anthropic` or `openai` is refused with "Chorus doesn't route Anthropic or OpenAI models through OpenRouter. Run them in Claude Code or Codex on your own sign-in." The author part is the text before the first `/` (the whole slug when there is none), with one leading `~` removed, compared without regard to case: `Anthropic/…`, `~openai/…` and a bare `openai` are refused; `anthropic-labs/x`, `meta/openai-compat` and `~deepseek/x` are not. The check is `chorus_core::routing::routing_refusal`, and it applies to `routing_tiers`, `routing_refresh` (before the provider check, so a refused model starts no cooldown and isn't logged), `for_launch` (Budget, Balanced, Fast and Nitro; OpenRouter default still launches with no routing), `routing_history`, `routing_observe_model` and the observer (it skips them and doesn't list them). A session that already has a stored Balanced or Nitro selection for such a model is left alone: it still reads back and Restart reapplies it. A plain OpenRouter launch with no tier is not refused | Proposed | Matthew's kickoff answer; the refusal's own text gives the reason. Leaving stored selections alone keeps D57's promise that Restart reuses a session's selection |
| 2026-10-25 | **`RoutingSettings::validate` bounds every input.** New upper bounds: `smoothingWindow` at most 50, `observationMaxAgeDays` at most 30, `snapshotMaxAgeMinutes` at most 1440, `fallbackCount` at most 5, `tieRatio` at most 2, `budgetMinTps` at most 10,000. Each refusal reads "<field> must be at most <n>". The new rules come after every existing one, so an input that already failed an old rule still gets the old message. The ranker doesn't call `validate`, and Chorus 1's measured constants and algorithm are unchanged. A stored row over a bound reads as the defaults, with the "did not validate" warning | Proposed | Matthew's kickoff answer: settings are now editable, so no input may be unbounded. Bounds sit at the boundary (Save, and reading the stored row), not in the ranker |
| 2026-10-25 | **Settings → Routing has five parts, top to bottom (key `routing`, icon `shuffle`, after Providers).** (1) **Inspect:** an OpenRouter provider, a model and an effort show what `routing_tiers` returns: Budget, Balanced and Fast cards, a Nitro card (an estimate, with Chorus 1's caveats), an OpenRouter default card, the warnings, the registry line and every candidate with its exclusion reasons, plus the existing Refresh with its cost text, cooldown and notes. The inspector never chooses a tier for a launch. (2) **Data collection:** a switch, off by default, that saves as soon as it changes. (3) **Ranking settings:** folded by default; every `RoutingSettings` field except `dataCollection`, each with its default beside it; edits are a draft with Save, Cancel and Restore defaults. Save sends the whole object, which Rust checks with serde and `validate`; a refusal keeps the draft and names the field by its label. Restore defaults deletes the stored row, so it also turns Data collection off. Saved settings apply to the next ranking (the inspector, New session and Refresh) and never to a running session or a stored selection. (4) **Background observation:** off, or the provider whose key it uses (`routing_watch`, where choosing one is the consent); the last check and its outcome; the next check; and every observed model with Stop observing / Observe again, which keeps the model's data. (5) **History:** the inspected model's speed history, one row per endpoint tag (samples, newest sample's age, median speed and latency, lowest 1-day uptime, a flag when the newest status isn't 0), computed in core as a pure function with no chart, and the last 50 Refreshes. Not built: the Profile selector (Team sessions were discarded; ranking always uses the Interactive profile), charts and Jev for routing (D40) | Proposed | Matthew's kickoff answers. D57 left the inspector for P12. Ranking never changes mid-session, which keeps a launch's selection what the session facts say it was |
| 2026-10-25 | **Routing state added to the `settings` table: `routing_observer_stopped` and `routing_refresh_log`, with no migration.** `routing_observer_stopped` is a JSON array of at most 500 slugs, in the order they were stopped; only Stop observing and Observe again write it. `routing_refresh_log` is a JSON array of entries (when, model, provider name, error or none, endpoints tested, spent and estimate in USD), newest first, at most 50, for every model; every Refresh that got past the checks made before any request (the slug, the refusal, the provider, the cooldown, the slot and the key) is logged, failures included, with zero figures for a failure. A figure that isn't finite is stored as 0. A stored value that can't be read is shown as a problem and never rewritten by reading: an unreadable stopped list makes the observer fetch nothing and say so until Stop or Observe again replaces it, and an unreadable Refresh log is left alone, nothing is logged and the screen reads "The Refresh log couldn't be read." The same rule covers `routing_settings` (an unreadable row reads as the defaults, with a problem, until Save or Restore defaults replaces it; the Data collection switch is disabled meanwhile). Only a user action rewrites a stored value | Proposed | `p15/loop` holds migrations 0023 and 0024, which the installed app already uses, so this run adds none. The rule is `remember()`'s: rewriting an unreadable record would silently lose data |
| 2026-10-25 | **The observer's status lives in memory.** `Routing` keeps when `start_observer` began and the last check (when its tick ended, and its outcome). After a restart the screen reads "No check since Chorus started." The next check is the last tick's end plus 30 minutes, or Chorus's start plus 2 minutes before any tick. Outcomes: models updated (with a count), nothing due, no usable provider (with `routing_provider`'s reason), OpenRouter refused the key, rate-limited, and other failures (with the message). A refused key or a rate limit stops the tick and wins even after some models were updated. A model is due when it has a stored snapshot, routing doesn't refuse it, it isn't stopped and its snapshot is at least 25 minutes old. A tick with no watched provider records no check. Stopping a model removes it from the observer's list only; its snapshot and observations stay. A failure to store a fetched answer now counts as a failed model; before, it was ignored | Proposed | Matthew's kickoff answer. The run adds no migration, and "No check since Chorus started" is true by construction |
| 2026-10-25 | **New session shows a routing refusal, and any other failure to load the tiers, by disabling the tiers.** When `routing_tiers` fails, New session's routing section shows the reason and disables Budget, Balanced, Fast, Nitro and Refresh, each with the reason as its title. OpenRouter default stays available, because it sends no routing. Otherwise New session shows what it showed before. The rule is the pure function `tiersBlocked(view, error)`, which returns the error only when the view failed to load | Proposed | Nobody answered the timed question within 15 minutes, so the default stood. Before, an error only added a red line while the tiers still read "not ranked yet", Refresh stayed enabled, and Nitro was refused only at launch (fail visible, D19) |
| 2026-10-25 | **A model's sample count is its number of updates, and the speed history counts per endpoint within the saved age.** In the observer list, a model's sample count is the number of distinct update times among its stored observations. In the speed history table, a row's samples are that endpoint's observations no older than the saved `observationMaxAgeDays` (one exactly that old is kept). Medians use the ranker's `median` over positive, finite values, so an even count gives the mean of the middle two; a missing value is shown as unknown, never zero; rows follow the stored ranking's candidate order, then other tags by name | Proposed | A model with three endpoints updated twice holds six observations but has been updated twice, and "samples" in the model list should say that. In the history, an observation older than the setting no longer counts |

## Proposed lessons

**A non-finite number turns a stored JSON list unreadable.** `serde_json` writes `NaN` and infinity
as `null`, and an `f64` field refuses `null` when it is read back, so a single bad figure in one
Refresh log entry would have hidden the whole log. Store 0 for a figure that isn't finite, and let
a log that can't be read be reported instead of rewritten.

**The order of the checks decides what a refused action leaves behind.** Refresh tests the model's
refusal before it looks at the provider, the cooldown or the slot, so a refused model starts no
cooldown and writes no log entry; the log begins only after the checks made before any request.
A test for a refused Refresh therefore uses a provider id that doesn't exist: it gets the refusal,
not "That provider no longer exists."

**A bound the ranker never calls guards the boundary, not the ranker.** `RoutingSettings::validate`
runs on Save and when the stored row is read; the ranker doesn't call it, and several of its tests
use values past the new bounds (`budget_min_tps: 100_000.0`). Put new bounds after the existing
rules in `validate`, so older messages still win, and leave the ranker as Chorus 1 measured it.

**Pass the IO in, so the decision can be tested without a network.** The observer's tick takes
its endpoint fetch as a parameter, and `check_outcome`, `observer_due`, `next_check_at` and
`refresh_entry` are pure. Tests then prove "no request was made" and each outcome with a stand-in,
and production sends exactly what it sent before.

**A test that pins another task's wiring ties the next task to it.** Vitest runs in a `node`
environment with no component mounting, so wiring in a `.vue` file can only be pinned by reading
it as text (`?raw`). T10's test fixed the exact `RoutingInspect` tag in `RoutingSettings.vue`, so T11
couldn't pass the observation a Refresh version, and T12 needed Matthew's approval to loosen that
one assertion. Pin behaviour in the plain TypeScript module, and pin text in a `.vue` file only for
what must not change.

## PLAN status

P12's row in the table, in place of the current one:

| P12 | Routing inspector | Built 2026-10-25 and ready for review: Settings → Routing with the inspector, the Data collection switch, editable ranking settings (every field bounded), background observation with its status and per-model Stop observing, the inspected model's speed history and the last 50 Refreshes with their spend; routing refuses Anthropic and OpenAI models and says why, in the inspector and in New session. Every task passed its checks at its gate. **The real-app check is still to come, at the milestone review** (the spec's "Check in the app", 8 steps; step 4 costs at most $0.05 and step 8 a few cents). Open items: New session still says "older than 60 min" whatever the saved age limit is; a failed Refresh's message is cleared in New session as soon as the tiers reload (the Refresh log now records it); a model first refreshed in the inspector joins the observation list only when it reloads |

And the P12 paragraph further down ("### P12 — Routing tiers"), whose last sentence is now done:
"The tiers, Refresh, the account checks, the cache test and endpoint observation came to New
session in P4 (D57). The routing inspector (Settings → Routing: its settings, observer status and
history, and the refusal of Anthropic and OpenAI models) is built; the real-app check at the
milestone review is still to come."

## Open questions

This phase resolves no entry in `OPEN-QUESTIONS.md` (Q18 and Q21 to Q28 are untouched).

Two known limits were left alone on purpose, because both sit in New session, where the run may
change only the refusal. They are the open items in the PLAN row above, each with a default that
stands if Matthew doesn't object.

**New session's "older than 60 min" ignores the saved age limit.** (P12.) `RoutingTiers.vue` writes
the number 60, but Settings → Routing now lets `snapshotMaxAgeMinutes` change, and the ranking
marks a result stale by the saved value. With a saved 120 min, New session says "older than 60 min"
on a result that isn't stale. Options: (a) leave it; (b) read the saved setting in New session, so
its text matches the inspector's. *Default: (b), in the next change that touches New session.*

**A failed Refresh's message disappears in New session.** (P12.) The message is cleared as soon as
the tiers reload. The inspector keeps a failed Refresh's message until the next Refresh or a change
of inputs, and the Refresh log records the failure with its reason. Options: (a) leave it, as the
log has the reason; (b) keep the message in New session until the next Refresh. *Default: (a).*

Found while building, and not a question: Background observation reloads when it opens, when the
ranking settings change, and once its next check is due (it looks every 15 seconds), but not after a
Refresh. A model
refreshed for the first time in the inspector therefore shows in the observation list when the
section is next opened. Passing the observation `inspectVersion + refreshVersion`, as the history
gets, fixes it, and changes the one line `routingObservation.test.ts` pins.
