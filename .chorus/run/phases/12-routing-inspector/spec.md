# P12 · Routing inspector

Read `.chorus/run/decisions.md` first: its "P12 · What to build", Wording and Testing sections are
part of this spec. Where this file names a text, function or file, use that one.

## What this phase delivers

A new Settings section, **Settings → Routing** (key `routing`, icon `shuffle`, placed after
Providers). It shows how OpenRouter's endpoints rank a model for opencode's Budget, Balanced, Fast
and Nitro tiers, and lets Matthew tune the ranking. From top to bottom:

1. **Inspect.** Inputs: an OpenRouter provider, a model (free text, with the models that have stored
   routing data offered as suggestions) and an effort. Below them: cards for Budget, Balanced and
   Fast; a Nitro card (an estimate, with Chorus 1's caveats); an OpenRouter default card; the
   warnings; the registry line; and every candidate with its exclusion reasons. Refresh (the
   existing `routing_refresh`) shows its cost text, cooldown and notes. The inspector never chooses
   a tier for a launch.
2. **Data collection.** A switch, off by default, that saves as soon as it changes.
3. **Ranking settings.** Folded by default. Every `RoutingSettings` field except `dataCollection`,
   grouped as Every tier, Budget, Balanced, Fast and Nitro, each with its default beside it. Edits
   are a draft with Save, Cancel and Restore defaults.
4. **Background observation.** Off, or the provider whose key it uses (`routing_watch`). It shows
   the last check and its outcome, the next check, and every observed model with **Stop
   observing** / **Observe again**.
5. **History.** The inspected model's speed history for each endpoint, and the last 50 Refreshes
   with their spend.

Two changes sit underneath:
- **Routing refuses Anthropic and OpenAI models**, with the reason. This covers `routing_tiers`,
  `routing_refresh`, `for_launch` (so New session's tiers), the new commands and the observer.
  Sessions with a stored selection are left alone.
- **`RoutingSettings::validate` bounds every input.**

What this phase doesn't do: change the ranker or Chorus 1's constants, re-rank or change running
sessions or stored selections, add a dependency, a migration or a network path, or build a Profile
selector. Ranking always uses `RoutingProfileId::Interactive`. Launches send what they send today;
the refusal is the only exception.

## Design

### Core (`crates/chorus-core/src/routing`)

**Refusal (`launch.rs`).**
- `pub const REFUSE_FIRST_PARTY: &str = "Chorus doesn't route Anthropic or OpenAI models through
  OpenRouter. Run them in Claude Code or Codex on your own sign-in."` This is the Wording text
  exactly, on one line, with straight apostrophes.
- `pub fn routing_refusal(model: &str) -> Option<&'static str>` finds the model's author part and
  returns the refusal for Anthropic and OpenAI. The author part is the text before the first `/`, or
  the whole slug when there is no `/`. One leading `~` is removed. The result is compared with
  `anthropic` and `openai`, ignoring ASCII case.

**Bounds (`types.rs`).** `validate` keeps every existing rule and message, and adds these:

| Field | Bound | Message |
|---|---|---|
| `smoothingWindow` | at most 50 | `smoothingWindow must be at most 50` |
| `observationMaxAgeDays` | at most 30 | `observationMaxAgeDays must be at most 30` |
| `snapshotMaxAgeMinutes` | at most 1440 | `snapshotMaxAgeMinutes must be at most 1440` |
| `fallbackCount` | at most 5 | `fallbackCount must be at most 5` |
| `tieRatio` | at most 2 | `tieRatio must be at most 2` |
| `budgetMinTps` | at most 10000 | `budgetMinTps must be at most 10000` |

The ranker doesn't call `validate`, so existing ranker tests that use values outside these bounds
(for example `budget_min_tps: 100_000.0` in `launch.rs`) stay as they are.

**History (`history.rs`, new, registered in `routing/mod.rs`).**
- `SpeedHistoryRow { tag, samples: u32, newest_at_ms, newest_status, median_tps: Option<f64>,
  median_latency_s: Option<f64>, lowest_uptime_1d: Option<f64> }`. It is serialized in camelCase,
  and `newest_at_ms` and `newest_status` are `i64` exported as `f64`, as `types.rs` does.
- `speed_history(observations, order: &[String], max_age_days: f64, now_ms: i64) ->
  Vec<SpeedHistoryRow>`:
  - It uses only the observations no older than `max_age_days` before `now_ms`. As in
    `merge_observations`, an observation exactly that old is kept.
  - It gives one row per tag that has any observation. `samples` is the tag's observation count.
    The newest observation gives `newest_at_ms` and `newest_status`.
  - `median_tps` is the median of the tag's positive, finite `tps_p50` values. `median_latency_s`
    is the same for `latency_p50_ms`, divided by 1000. `lowest_uptime_1d` is the lowest
    `uptime_1d`. With no values, each is `None`.
  - Medians use the ranker's `eligibility::median`, so an even count gives the mean of the middle
    two.
  - Rows follow `order` first, then the tags not in it, by name (`js::by_code_unit`). A tag in
    `order` with no observations gets no row.
- `sample_count(observations) -> u32`: the number of distinct `observed_at_ms` values, which is the
  number of updates kept.
- `REFRESH_LOG_MAX = 50`.
- `RefreshLogEntry { at_ms, model, provider_name, error: Option<String>, endpoints_tested: u32,
  spent_usd: f64, estimate_usd: f64 }` is serialized in camelCase with `deny_unknown_fields`, and
  `at_ms` is exported as `f64`. When `error` is `None`, the UI shows "Done".
- `parse_refresh_log(raw: Option<&str>) -> Result<Vec<RefreshLogEntry>, …>`. `None` is an empty
  log. Anything that isn't a JSON array of entries is an error.
- `append_refresh_log(log, entry) -> Vec<RefreshLogEntry>` puts the entry first and keeps the
  newest `REFRESH_LOG_MAX`.

### Settings keys (the `settings` table)

| Key | Holds | Written by |
|---|---|---|
| `routing_settings` (exists) | the whole `RoutingSettings` object | Save and the Data collection switch. Restore defaults deletes it. |
| `routing_watch_provider` (exists) | the observer's provider id | `routing_watch` (unchanged) |
| `routing_observer_stopped` (new) | a JSON array of slugs, at most 500 | Stop observing and Observe again |
| `routing_refresh_log` (new) | a JSON array of `RefreshLogEntry`, newest first, at most 50 | each Refresh that passed the checks made before any request |

A stored value that can't be read is reported and never rewritten by reading. Only the user
actions above rewrite one.

### Commands (`src-tauri/src/routing`)

`routing_tiers`, `routing_refresh` and `routing_watch` keep their signatures. New commands:

| Command | Arguments | Returns |
|---|---|---|
| `routing_settings` | — | `RoutingSettingsView { settings, defaults, stored: bool, problem: Option<String> }` |
| `routing_settings_save` | `settings: RoutingSettings` | `RoutingSettingsView` |
| `routing_settings_reset` | — | `RoutingSettingsView` |
| `routing_observer` | — | `ObserverView` |
| `routing_observe_model` | `model: String, observe: bool` | `ObserverView` |
| `routing_history` | `model: String` | `Vec<SpeedHistoryRow>` |
| `routing_refresh_log` | — | `RefreshLogView { entries, problem: Option<String> }` |

- **Settings.**
  - `routing_settings` returns the stored settings over the defaults, and the defaults.
  - When the stored row can't be read (`parse_routing_settings_value` gives a warning), `problem`
    is "The saved ranking settings couldn't be read, so the defaults apply. Save or Restore
    defaults replaces them."
  - Save takes the whole object. serde checks it through the command argument, then `validate`.
    It stores the whole object, or returns `validate`'s message and leaves the row as it was.
  - Reset deletes `routing_settings`.
- **Model checks on the new commands.** `routing_history` and `routing_observe_model` refuse a
  model that fails `is_model_slug`, one that ends in `:nitro` (`REFUSE_NITRO_MODEL`) and one that
  `routing_refusal` refuses.
- **The refusal on existing paths.**
  - `view` refuses after its slug check.
  - `refresh` refuses before it checks the provider, so a refused Refresh starts no cooldown and
    isn't logged.
  - `for_launch` refuses Budget, Balanced, Fast and Nitro after the `:nitro` check. OpenRouter
    default still returns early with no routing.

### Observer

- `Routing` keeps the observer's status in memory: when `start_observer` began, and the last check
  (when it ended and its outcome).
- `next_check_at(started_ms, last_check_ms)` is pure. It gives the last check + 30 min, or the
  start + 2 min before any check. A check's time is when its tick ended, which is when the sleep
  begins.
- `lib.rs` is unchanged: `Routing::default()` and `start_observer` are called as now.
- `CheckOutcome` is one of:
  - `Updated { count }`
  - `NothingDue`
  - `NoProvider { reason }`
  - `KeyRefused`
  - `RateLimited`
  - `Failed { message }`
- Each tick does the following, in order:
  1. If no provider is watched, it records nothing.
  2. If `routing_provider` refuses the watched provider, it records `NoProvider` with that reason.
  3. If `routing_observer_stopped` can't be read, it records `Failed` with "The list of stopped
     models couldn't be read. Stop or observe a model to replace it." and fetches nothing.
  4. A model is due when all of these hold: it has a stored snapshot, `routing_refusal` doesn't
     refuse it, it isn't stopped, and its snapshot is at least 25 minutes old. With nothing due,
     the tick records `NothingDue`.
  5. If the key can't be read, it records `Failed` with that message.
  6. It fetches each due model, skipping any whose slot is busy, as now. Then the pure
     `check_outcome` decides the outcome:
     - A refused key gives `KeyRefused`, and a rate limit gives `RateLimited`. Either stops the
       tick, as now, and wins even if models were updated first.
     - Otherwise, if any model was updated or none failed, the outcome is `Updated` with the count.
     - Otherwise it is `Failed` with the first failure's message.
- `ObserverView` holds:
  - the watched provider's id and name;
  - `problem`: what `routing_provider` says about the watched provider now, if anything;
  - the last check, if any;
  - `next_at_ms`;
  - `stopped_problem`, the text in step 3 when the list can't be read;
  - `models`: every model with a stored snapshot except those `routing_refusal` refuses, in slug
    order. Each has its snapshot's `fetched_at_ms`, its `sample_count` and whether it is stopped.
- `routing_observe_model(model, false)` adds the slug to `routing_observer_stopped`, and `true`
  removes it. Neither touches the model's routing data. Adding a 501st slug is refused. When the
  stored list can't be read, the action replaces it: Stop gives `[model]`, and Observe again gives
  `[]`.

### Refresh log

- A Refresh is logged once it has passed every check before a request: the slug, the refusal, the
  provider (`routing_provider`), the cooldown, the slot and the key. A Refresh refused by any of
  them isn't logged.
- `refresh_entry` (pure) builds the entry, timed when the Refresh ended, with the model and the
  provider's name.
  - A success gives no error, `probed.len()` endpoints tested, and the spend and estimate.
  - A failure gives its message and zero figures. Every failure inside `run_refresh` comes before
    the cache test spends anything, except storing its results, and that is a write to the same
    database the log uses.
- `record_refresh(store, entry)` appends the entry through `append_refresh_log`. When the stored log
  can't be read, it leaves the log unchanged.
- `routing_refresh_log` returns the entries. When the log can't be read, it returns the problem
  "The Refresh log couldn't be read." instead.
- `routing_history(model)` returns `speed_history` of the stored observations with the saved
  `observationMaxAgeDays`. The order is the stored ranking's candidate order: `rank_stored` with no
  provider and no effort, or no order when it can't rank.

### UI files

| File | What it holds |
|---|---|
| `src/views/newSession/routingFormat.ts` (+ `.test.ts`) | `tps`, `price`, `pct`, `secs`, `why(tier, result)` and `details(c)`, moved from `RoutingTiers.vue`, which imports them; new `usd`, `ago` and `inTime`; `tiersBlocked` (below) |
| `src/views/settings/sections.ts` (+ `.test.ts`) | `SECTIONS`, moved from `SettingsView.vue` |
| `src/views/settings/RoutingSettings.vue` | the section: its heading, the intro, the parts in order, and how they connect |
| `src/views/settings/RoutingInspect.vue`, `routingInspect.ts` (+ `.test.ts`) | part 1 |
| `src/views/settings/RoutingRanking.vue`, `routingRanking.ts` (+ `.test.ts`) | parts 2 and 3 |
| `src/views/settings/RoutingObservation.vue`, `routingObservation.ts` (+ `.test.ts`) | part 4 |
| `src/views/settings/RoutingHistory.vue`, `routingHistory.ts` (+ `.test.ts`) | part 5 |

How the parts connect. Each part is built with these from the start, so later tasks change only
`RoutingSettings.vue`.
- `RoutingInspect` emits `inspect` (`{ providerId, model, effort }`) and `refreshed`. It reloads
  when its `version` prop changes.
- `RoutingRanking` emits `saved` after Save, Restore defaults or the switch. The parent then bumps
  the inspector's `version`.
- `RoutingObservation` and `RoutingHistory` take a `version` prop, which the parent bumps after a
  Refresh. `RoutingHistory` also takes the inspected `model`.

**The inspector** (`routingInspect.ts`):
- `INTRO` and `DEFAULT_CARD` are the Wording texts.
- `tierCards(view, maxAgeMinutes)` builds the Budget, Balanced and Fast cards. Each has the primary
  endpoint's tag, provider, speed, price (blended), latency and 1-day uptime, then "Then: a, b" or
  "No fallbacks", then the rationale.
  - With no result, a card says "Refresh to rank this model’s endpoints."
  - An empty tier says "<Tier> has no eligible endpoints."
  - A stale result adds "Older than <maxAgeMinutes> min: Refresh before launching with it." The
    minutes come from the saved `snapshotMaxAgeMinutes`.
- `nitroCard(view)` uses `RoutingTiers.vue`'s texts, with its curly apostrophes.
  - With a likely endpoint: "Likely <provider> (<tag>) at <speed>: an estimate; actual routing may
    differ.", and "That endpoint fails: <rules>." when it fails any rules.
  - Without one: "OpenRouter picks the fastest endpoint for each request."
  - Always: "Requests may be billed at a provider’s priority-tier price. Your account’s guardrails
    still apply."
- `candidateRows(result)` lists every candidate: tag, provider, speed, price, latency, uptime and
  its reasons. The reasons are `excludedBy` joined with "; ", or "below the Budget floor" when only
  the floor excludes it.
- `registryLine(view)` is RoutingTiers' registry sentence.
- `refreshState(view, busy)` says whether Refresh is disabled, and gives its title:
  - the provider's problem;
  - or "Refreshed less than a minute ago: again in N s";
  - or "Refreshing…";
  - or RoutingTiers' cost title.
- `refreshText(outcome)` gives RoutingTiers' "Refreshed. …" sentences, with amounts through `usd`.
- `inspectorProviders(providers)` keeps only `kind = openrouter`.
- `defaultProviderId(providers, watchedId)` gives the watched provider, else the first OpenRouter
  provider, else `null`.
- `EFFORT_CHOICES` is `[null, 'fast', 'balanced', 'deep', 'max']`, labelled "No effort", then
  `EFFORT_LABELS`.
- `modelSuggestions(observer)` gives the observer's model slugs.
- A failed Refresh's message stays shown until the next Refresh or a change of inputs.

**Ranking settings** (`routingRanking.ts`):
- `RANKING_FIELDS`, in decisions.md's groups and order, each field with:
  - its key path (for example `tierWeights.budget`);
  - its group;
  - its label: the decisions.md table's label without the bracketed default;
  - a short label, shown under the group's heading, without the "Budget: ", "Balanced: " or "Fast: "
    prefix;
  - whether it takes a whole number (`smoothingWindow`, `stableMinObservations`,
    `fallbackCount`);
  - its unit.
- `defaultText(field)` gives exactly the table's bracketed defaults: "99.5%", "99.6%", "95%", "Only
  the model's maker at full precision, or ones verified", "6", "3", "7 days", "60 min", "1.01",
  "2", "0.3", "30 tok/s", "0.5", "0.5", "1.0".
- `toDraft(settings)` and `fromDraft(draft, saved)`. `fromDraft` keeps the saved `dataCollection`.
  A field that isn't a number, or isn't a whole number where the field needs one, is refused before
  sending with "“<label>” needs a number." or "“<label>” needs a whole number."
- `refusalText(message)` replaces each camelCase field name in a Rust message with its label in
  curly quotes.
  - Example: "fallbackCount must be at most 5" becomes "“Fallback endpoints after the first” must
    be at most 5".
  - A message naming no field is shown as it is.
- `withDataCollection(saved, allow)` changes only `dataCollection`.
- `switchBlocked(view)` gives the view's `problem` when the stored settings can't be read. The
  switch is then disabled, with that text as its title. Save and Restore defaults stay available.
- Restore defaults' title is "Deletes the saved ranking settings, Data collection included: every
  setting goes back to its default."
- A refused Save keeps the draft and shows the reason.
- The texts for the Data collection switch, Nitro and Keep-speed-history help are the Wording texts
  exactly.

**Observation** (`routingObservation.ts`):
- `observerLines(view, now)` gives these lines:
  - When off, the Wording text "Off. Choose a provider…".
  - When on, "On, with <name>.", then the provider's `problem` (warning tone) if it has one.
  - The last check: "Last check <ago>: " followed by one of:
    - "3 models updated." or "1 model updated."
    - "nothing was due."
    - "no usable provider. <reason>"
    - "OpenRouter refused the key."
    - "OpenRouter rate-limited the check; the next check tries again."
    - "failed: <message>", with a full stop added only when the message lacks one.
  - Before any check, "No check since Chorus started." instead.
  - Then "Next check <inTime>."
- `modelRows(view, now)` gives each model "Updated <ago> · <n> samples" (or "1 sample", or "Not
  updated yet"). A stopped model is marked "Stopped" and offers "Observe again". Others offer "Stop
  observing".
- `observerProviders(providers)` lists the OpenRouter providers. One without its own key is
  disabled, with the title "Routing needs <name>'s own API key (a management key can't fetch
  endpoints)."

**History** (`routingHistory.ts`):
- `speedRows(rows, now)` gives each row's tag, samples, newest sample's age (`ago`), speed (`tps`),
  latency (`secs`) and lowest uptime (`pct`), plus "status <n>" when the newest status isn't 0.
  Unknown values are "—".
- `logRows(entries, now)` gives when (`ago`), model, provider, "Done" or the message, endpoints
  tested, spent and estimate (`usd`).
- Empty states:
  - "Inspect a model to see its speed history."
  - "No speed history for this model yet."
  - "No Refreshes yet."
- The log's `problem` is shown as it is. The speed history help is the Wording text.

**Formatting** (`routingFormat.ts`):
- `usd`: under $1 as `$0.0000`, otherwise `$0.00`; `null` is "—".
- `ago(atMs, nowMs)`:
  - "just now" under a minute, and for future times;
  - "N min ago" under an hour;
  - "N h ago" under a day;
  - "1 day ago", "N days ago" after that.
- `inTime(atMs, nowMs)` gives "in N min", rounded up, at least 1. From 60 min up it gives "in N h".

**The refusal in New session** (`tiersBlocked`):
- When `routing_tiers` fails (the view couldn't load), `tiersBlocked(view, error)` returns the
  error.
- RoutingTiers then disables Budget, Balanced, Fast, Nitro and Refresh, with the error as their
  title. OpenRouter default stays available.
- Otherwise New session shows exactly what it shows today.

## Acceptance criteria

Each criterion is proved by a test that the checks run.

**Refusal**
1. `routing_refusal` refuses `anthropic/…`, `Anthropic/…`, `openai/…`, `OPENAI/…`, `~anthropic/…`
   and a bare `openai`, with exactly `REFUSE_FIRST_PARTY`. It allows `deepseek/deepseek-v4.1-flash`,
   `anthropic-labs/x`, `meta/openai-compat` and `~deepseek/x`.
2. For a refused model, `routing_tiers` returns `REFUSE_FIRST_PARTY` even with a snapshot stored.
   `routing_refresh` returns it before checking the provider, starts no cooldown and logs nothing.
3. For a refused model on opencode with an OpenRouter provider, `for_launch` refuses Budget,
   Balanced, Fast and Nitro with `REFUSE_FIRST_PARTY`, and returns `Ok(None)` for OpenRouter
   default.
4. For a refused model, `routing_history` and `routing_observe_model` return `REFUSE_FIRST_PARTY`,
   `routing_observer` doesn't list it, and the observer doesn't treat it as due.
5. A stored Balanced or Nitro selection for `anthropic/…` still reads back through
   `parse_stored_selection`, and `plan_routing_relaunch` reapplies it.
6. `tiersBlocked` returns the error only when the tiers view failed to load. With a loaded view, or
   with no error, it returns `null`.

**Bounds**
7. `validate` accepts each new bound exactly and refuses just above it with the message in the
   table. Every existing rule and message is unchanged, and the defaults validate.
8. `parse_routing_settings_value` reads a stored `{"fallbackCount":6}` as the defaults, with the
   "did not validate" warning.

**Ranking settings**
9. `routing_settings` returns the stored settings over the defaults, the defaults, and whether a
   row is stored. With no row, it returns the defaults with `stored: false`.
10. A stored row that isn't JSON, has an unknown field or fails `validate` reads as the defaults,
    with the problem text. Reading leaves the row byte for byte as it was.
11. `routing_settings_save` stores a valid object. An invalid one is refused with `validate`'s
    message, and the stored row is unchanged.
12. `routing_settings_reset` deletes `routing_settings`.
13. After saving `fallbackCount: 1`, the stored golden model's Balanced tier has 2 endpoints in
    `view` and in `select_for_launch`. After a reset, it has 3 again. A selection made before the
    save still describes and reads back unchanged. `view` writes no `routing_launch_choices` row.
14. `RANKING_FIELDS` covers every `RoutingSettings` field but `dataCollection`, once each, in the
    decisions' groups and order, with their labels. `defaultText` gives the bracketed defaults.
15. `fromDraft(toDraft(defaults), defaults)` returns the defaults. `fromDraft` refuses a non-number
    and a fractional whole-number field, naming the label. `refusalText` names labels, not camelCase
    names. `withDataCollection` changes only `dataCollection`. `switchBlocked` returns the problem
    when there is one.

**Observer**
16. `check_outcome`:
    - A refused key gives `KeyRefused` and a rate limit gives `RateLimited`, even after updates.
    - Otherwise, updates give `Updated { count }`.
    - Otherwise, failures give `Failed` with the first message.
    - With no updates and no failures, it gives `Updated { count: 0 }`.
17. `next_check_at` gives the start + 2 min before any check, then the last check + 30 min.
18. A tick records:
    - nothing with no watched provider;
    - `NoProvider` with `routing_provider`'s reason for an unusable one;
    - `NothingDue` when the only stale models are stopped or refused.
    None of these sends a request.
19. When the stopped list can't be read, the tick records `Failed` with the stopped-list text, and
    `routing_observer` reports it as `stopped_problem`.
20. `routing_observe_model`:
    - It adds or removes the slug.
    - It leaves the model's snapshot and observations unchanged.
    - It refuses a bad slug, a `:nitro` slug, a refused model and a 501st slug.
    - It replaces an unreadable list.
21. `routing_observer` lists the stored models in slug order, each with its snapshot time,
    `sample_count` and stopped flag, plus the watched provider's id, name and problem.
22. `observerLines` gives the Wording texts for off, on, each outcome, before any check, and the
    next check. `modelRows` and `observerProviders` give the texts above.

**History and Refresh log**
23. `speed_history`:
    - It follows the order rules, uses medians of positive values and the lowest 1-day uptime.
    - It leaves out observations older than the age, and gives `None` for missing values.
    - On the golden fixture's observations it gives one row per tag, with 1 sample whose speed
      and latency equal the snapshot's p50 values.
24. `sample_count` counts distinct observation times.
25. `append_refresh_log` puts new entries first and keeps 50. `parse_refresh_log(None)` is empty.
    Text that isn't an array of entries is an error. An appended log round-trips.
26. `refresh_entry`:
    - A success maps to no error, the probed count, spent and estimate.
    - A failure maps to its message and zero figures.
    - `record_refresh` leaves an unreadable stored log unchanged.
27. A Refresh refused before any request (no such provider, no own key, cooldown, another running)
    adds nothing to the log. `routing_refresh_log` returns the entries, or "The Refresh log
    couldn't be read." when it can't read them.
28. On the stored golden fixture, `routing_history` gives one row per candidate tag, in candidate
    order.
29. `speedRows` and `logRows` format as described, with "—" for unknown values.

**Section and inspector**
30. `SECTIONS` has `{ key: 'routing', label: 'Routing', icon: 'shuffle' }` directly after
    Providers.
31. `tierCards`, `nitroCard`, `candidateRows`, `registryLine`, `refreshState`, `refreshText`,
    `inspectorProviders`, `defaultProviderId`, `EFFORT_CHOICES` and `modelSuggestions` give the
    texts and values above. `INTRO` and `DEFAULT_CARD` are the Wording texts.
32. Tests pin today's outputs of `tps`, `price`, `pct`, `secs`, `why` and `details`. `usd`, `ago` and
    `inTime` give the formats above.

**Checks**
33. Every check passes: `npm run bindings` leaves `bindings.ts` current, plus `typecheck`, `npm
    test`, `test:rust`, `lint:rust` and `grep:secrets`. Every new command is in `ipc.rs`.

## Known limits, left alone (recorded in proposals.md)
- New session's routing section still says "older than 60 min" (`RoutingTiers.vue`), even when the
  saved `snapshotMaxAgeMinutes` differs.
- In New session, a failed Refresh's message is cleared as soon as the tiers reload. The Refresh
  log now records the failure.

## Check in the app

Matthew follows these at the milestone review. Free unless marked.

1. Open Settings → Routing. It sits after Providers, with the shuffle icon and the intro. Choose
   your OpenRouter provider, `deepseek/deepseek-v4.1-flash` and no effort. You see the Budget,
   Balanced, Fast, Nitro and OpenRouter default cards, the warnings, the registry line and the
   candidate table. Now open New session → opencode with the same provider and model. Each tier's
   provider matches its card.
2. Ranking settings is folded. Open it: each field shows its default beside it.
   - Set "Fallback endpoints after the first" to 1 and Save. The Balanced card lists one fallback,
     and New session's Balanced tooltip lists one endpoint after "Then:".
   - Restore defaults. There are two fallbacks again, and Data collection is off.
   - Type 6 and Save. It is refused with "“Fallback endpoints after the first” must be at most 5",
     and the 6 stays in the field. Cancel brings back the saved value.
3. Turn Data collection on, leave Settings and come back: it is still on. Turn it off.
4. Refresh the cheap model in the inspector. The cost text shows the spend. The Refresh log gets a
   new first row: "just now", the model, your provider, Done, the endpoints tested, and the same
   spend. **Costs at most $0.05.**
5. Observation:
   - Choose your provider under Background observation, then quit and restart Chorus. The status
     reads "No check since Chorus started." and "Next check in 2 min".
   - About 2 minutes after start, it reads "Last check … : N models updated." (or "nothing was
     due.").
   - Stop observing one model. It shows Stopped. After the next check, 30 minutes later, its
     "Updated" time hasn't moved while the others' have. Observe it again.
6. Speed history for the inspected model shows one row per endpoint, in the candidate table's
   order, with samples, the newest sample's age, speed, latency and lowest uptime.
7. Inspect any `anthropic/…` model, and any `openai/…` model. Each shows "Chorus doesn't route
   Anthropic or OpenAI models through OpenRouter. Run them in Claude Code or Codex on your own
   sign-in." In New session → opencode → your OpenRouter provider with the same model, the routing
   section shows the same reason. Budget, Balanced, Fast, Nitro and Refresh are disabled, with it as
   their title, and only OpenRouter default can be chosen. Don't launch.
8. Run one opencode turn on Balanced and one on Nitro, on the cheap model. The session facts show
   "Balanced · <endpoint>, then 2 more" and "Nitro: OpenRouter picks the fastest endpoint".
   OpenRouter's activity page shows the provider each used. **A few cents.**
