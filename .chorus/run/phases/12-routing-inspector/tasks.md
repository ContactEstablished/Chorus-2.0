# P12 · Routing inspector: tasks

Read `spec.md` beside this file and `.chorus/run/decisions.md` before starting a task.

## T1 · Core: routing refuses Anthropic and OpenAI models
- Depends on: none
- Claims: crates/chorus-core/src/routing/launch.rs
- Accept: `REFUSE_FIRST_PARTY` is exactly "Chorus doesn't route Anthropic or OpenAI models through OpenRouter. Run them in Claude Code or Codex on your own sign-in."
- Accept: `routing_refusal` returns `Some(REFUSE_FIRST_PARTY)` for `anthropic/claude-sonnet-5`, `Anthropic/claude-opus-5.5`, `openai/gpt-5.5`, `OPENAI/o9`, `~anthropic/claude-sonnet-latest` and `openai` (no `/`)
- Accept: `routing_refusal` returns `None` for `deepseek/deepseek-v4.1-flash`, `anthropic-labs/x`, `meta/openai-compat` and `~deepseek/x`
- Accept: a stored Balanced selection and a stored Nitro selection for `anthropic/claude-sonnet-5` still read back through `parse_stored_selection` as selections, and `plan_routing_relaunch` returns `Routed` for them
- Status: done

## T2 · Core: upper bounds for the ranking settings
- Depends on: none
- Claims: crates/chorus-core/src/routing/types.rs, crates/chorus-core/src/routing/store.rs
- Accept: `RoutingSettings::validate` accepts `smoothingWindow` 50, `observationMaxAgeDays` 30, `snapshotMaxAgeMinutes` 1440, `fallbackCount` 5, `tieRatio` 2 and `budgetMinTps` 10000
- Accept: `validate` refuses 51, 30.5, 1441, 6, 2.01 and 10000.5 for those fields with exactly "smoothingWindow must be at most 50", "observationMaxAgeDays must be at most 30", "snapshotMaxAgeMinutes must be at most 1440", "fallbackCount must be at most 5", "tieRatio must be at most 2" and "budgetMinTps must be at most 10000"
- Accept: the existing `validate` messages and the assertions in `the_default_settings_are_chorus_1s_and_valid` are unchanged and pass
- Accept: `parse_routing_settings_value(Some(r#"{"fallbackCount":6}"#))` returns the defaults with the warning "stored routing settings did not validate; using defaults"
- Status: done

## T3 · Core: speed history and the Refresh log
- Depends on: none
- Claims: crates/chorus-core/src/routing/history.rs, crates/chorus-core/src/routing/mod.rs
- Accept: `speed_history` gives one row per tag with observations no older than `max_age_days` before `now_ms` (exactly that old is kept), with its observation count, newest time and newest status, the median of its positive `tps_p50` values, the median of its positive `latency_p50_ms` values in seconds, and its lowest `uptime_1d`; each is `None` when the tag has no such value
- Accept: `speed_history` rows follow the given order, then the remaining tags by name; a tag in the order with no observations has no row; an even count's median is the mean of the middle two
- Accept: on `extract_observations(&golden_snapshot())`, `speed_history` gives one row per tag with 1 sample whose `median_tps` and `median_latency_s` equal that tag's observation's `tps_p50` and `latency_p50_ms / 1000`
- Accept: `sample_count` of 3 tags observed at 2 instants is 2
- Accept: `append_refresh_log` puts the new entry first and keeps the newest 50 of 51; `parse_refresh_log(None)` is an empty log; `"nope"`, `"{}"` and an array holding an object with an unknown field are errors; an appended log serialized to JSON reads back equal
- Accept: `SpeedHistoryRow` and `RefreshLogEntry` are among the roots of `front_end_types_export_without_big_integers`, which passes
- Status: done

## T4 · Rust: the refusal in tiers, Refresh and launch
- Depends on: T1
- Claims: src-tauri/src/routing/mod.rs
- Accept: `view` for `anthropic/claude-sonnet-5` and `OpenAI/gpt-5.5` returns `Err(REFUSE_FIRST_PARTY)`, even with the golden snapshot stored under that slug
- Accept: `refresh` for `anthropic/claude-sonnet-5` with a provider id that doesn't exist returns `Err(REFUSE_FIRST_PARTY)` (not "That provider no longer exists."), and `wait_secs` for it stays 0
- Accept: `for_launch` on opencode with an OpenRouter provider and the model `openai/gpt-5.5` refuses Budget, Balanced, Fast and Nitro with `REFUSE_FIRST_PARTY`, and returns `Ok(None)` for OpenRouter default
- Accept: calling `view` for the golden model writes no `routing_launch_choices` row
- Accept: `a_launch_takes_a_tier_ranked_from_what_is_stored` and every other existing test pass unchanged
- Status: done

## T5 · Rust: commands for the ranking settings
- Depends on: T2, T4
- Claims: src-tauri/src/routing/mod.rs, src-tauri/src/routing/commands.rs, src-tauri/src/ipc.rs, src/ipc/bindings.ts
- Accept: with no stored row, the settings view (`routing_settings`) holds the defaults as both `settings` and `defaults`, `stored: false` and no problem
- Accept: a stored row of `nope`, `{"bogus":1}` or `{"fallbackCount":6}` reads as the defaults with the problem "The saved ranking settings couldn't be read, so the defaults apply. Save or Restore defaults replaces them.", and the row is unchanged after reading
- Accept: saving a valid object stores it whole and the view returns it with `stored: true`; saving `fallbackCount: 6` returns `Err("fallbackCount must be at most 5")` and leaves the stored row unchanged
- Accept: reset deletes the `routing_settings` row
- Accept: after saving `fallbackCount: 1`, `view` and `select_for_launch` for the stored golden model give Balanced 2 endpoints, and 3 again after reset; a selection JSON made by `for_launch` before the save gives the same `describe` text after it
- Accept: `routing_settings`, `routing_settings_save` and `routing_settings_reset` are in `ipc.rs`, and `bindings_are_current` and `command_permissions_match_registration` pass
- Status: done

## T6 · Rust: observer status and Stop observing
- Depends on: T3, T5
- Claims: src-tauri/src/routing/mod.rs, src-tauri/src/routing/commands.rs, src-tauri/src/ipc.rs, src/ipc/bindings.ts
- Accept: `check_outcome` gives `KeyRefused` for a refused key and `RateLimited` for a rate limit even after 2 updates; `Updated { count: 2 }` for 2 updates and a later non-stopping failure; `Failed` with the first failure's message for no updates and two failures; `Updated { count: 0 }` for no updates and no failures
- Accept: `next_check_at(start, None)` is start + 120 000 ms and `next_check_at(start, Some(t))` is t + 1 800 000 ms
- Accept: `observe_tick` records nothing with no watched provider, and `NoProvider` with `routing_provider`'s reason when the watched provider has no key of its own
- Accept: with a usable watched provider (fake key) and stale snapshots only for a stopped model and for `anthropic/claude-sonnet-5`, `observe_tick` records `NothingDue` and makes no request
- Accept: with a usable watched provider (fake key) and `routing_observer_stopped` set to `nope`, `observe_tick` records `Failed` with "The list of stopped models couldn't be read. Stop or observe a model to replace it." and the observer view's `stopped_problem` is that text
- Accept: `routing_observe_model(m, false)` adds `m` to `routing_observer_stopped` and `true` removes it, leaving `m`'s snapshot and observations rows byte for byte unchanged; with an unreadable list it replaces the list (`[m]` or `[]`)
- Accept: `routing_observe_model` refuses `a b`, `deepseek/deepseek-v4.1-flash:nitro`, `openai/gpt-5.5` (with `REFUSE_FIRST_PARTY`) and a 501st stopped slug
- Accept: the observer view lists every stored model except Anthropic and OpenAI ones, in slug order, each with its snapshot's `fetched_at_ms`, `sample_count` and stopped flag, and gives the watched provider's id, name and problem, the last check and `next_at_ms`
- Accept: `routing_observer` and `routing_observe_model` are in `ipc.rs`, and `bindings_are_current` passes
- Status: done

## T7 · Rust: speed history and the Refresh log
- Depends on: T6
- Claims: src-tauri/src/routing/mod.rs, src-tauri/src/routing/commands.rs, src-tauri/src/ipc.rs, src/ipc/bindings.ts
- Accept: on the golden fixture stored by `stored_snapshot`, `routing_history` gives one row per candidate tag of the stored ranking, in its candidate order; for a model with nothing stored it gives an empty list
- Accept: `routing_history` refuses `a b`, a `:nitro` slug and `anthropic/claude-sonnet-5` (with `REFUSE_FIRST_PARTY`)
- Accept: `refresh_entry` maps a success to no error, the probed count, `spentUsd` and `estimateUsd`, and a failure to its message with 0 endpoints tested and $0 spent and estimated, each with the model, the provider's name and the time given
- Accept: `record_refresh` adds entries newest first and keeps 50, and leaves a stored `routing_refresh_log` of `nope` unchanged
- Accept: a Refresh refused for no such provider, no own key, the cooldown or a busy slot leaves `routing_refresh_log` absent
- Accept: `routing_refresh_log` returns the stored entries newest first, and the problem "The Refresh log couldn't be read." with no entries when the stored value is `nope`
- Accept: `routing_history` and `routing_refresh_log` are in `ipc.rs`, and `bindings_are_current` passes
- Status: done

## T8 · UI: shared routing formatting, and the refusal in New session
- Depends on: none
- Claims: src/views/newSession/RoutingTiers.vue, src/views/newSession/routingFormat.ts, src/views/newSession/routingFormat.test.ts
- Accept: `RoutingTiers.vue` imports `tps`, `price`, `pct`, `secs`, `why` and `details` from `routingFormat.ts` and defines none of them
- Accept: `tps(41.6)` is "42 tok/s", `price(0.1234)` is "$0.123/M", `price(2)` is "$2.00/M", `pct(99.5)` is "99.50%", `secs(0.4213)` is "0.42 s", and each gives "—" for `null`
- Accept: for a sample `TierResult` with 3 eligible candidates (one below the Budget floor of 30) and 1 excluded, `why` gives "First of 2 endpoints at or above the 30.0 tok/s Budget floor, scored mostly on price.", "First of 3 eligible endpoints, scored equally on price and speed." and "First of 3 eligible endpoints, scored on speed only.", and `details` gives RoutingTiers' four lines exactly
- Accept: `usd(0.0123)` is "$0.0123", `usd(0)` is "$0.0000", `usd(1.234)` is "$1.23", `usd(null)` is "—"
- Accept: `ago` gives "just now" at 59 s and for a future time, "12 min ago", "3 h ago", "1 day ago" and "4 days ago"; `inTime` gives "in 18 min" for 17 min 10 s, "in 1 min" for 5 s or a past time, and "in 2 h" for 2 h 10 min
- Accept: `tiersBlocked(null, 'x')` is 'x'; `tiersBlocked(view, 'x')` and `tiersBlocked(null, null)` are `null`
- Accept: `npm run typecheck` passes
- Status: done

## T9 · UI: Settings → Routing and the inspector
- Depends on: T7, T8
- Claims: src/stores/view.ts, src/views/SettingsView.vue, src/views/settings/sections.ts, src/views/settings/sections.test.ts, src/views/settings/RoutingSettings.vue, src/views/settings/RoutingInspect.vue, src/views/settings/routingInspect.ts, src/views/settings/routingInspect.test.ts
- Accept: `SECTIONS` in `sections.ts` holds `{ key: 'routing', label: 'Routing', icon: 'shuffle' }` directly after the `providers` entry, every other entry is unchanged and in its old order, and `SettingsView.vue` imports `SECTIONS` and renders `RoutingSettings` for `routing`
- Accept: `INTRO` and `DEFAULT_CARD` are exactly decisions.md's Routing intro and OpenRouter default card texts
- Accept: `tierCards` gives Budget, Balanced and Fast cards with the primary endpoint's tag, provider, speed, price, latency and uptime, "Then: a, b" or "No fallbacks", and the rationale; "Refresh to rank this model’s endpoints." with no result; "Fast has no eligible endpoints." for an empty Fast; and "Older than 30 min: Refresh before launching with it." on a stale result with `maxAgeMinutes` 30
- Accept: `nitroCard` gives the likely-endpoint estimate line, the failed rules line when there are rules, and the billing and guardrails line; with no likely endpoint it says "OpenRouter picks the fastest endpoint for each request."
- Accept: `candidateRows` gives every candidate with its reasons joined by "; ", and "below the Budget floor" for one excluded only by the floor
- Accept: `refreshState` gives disabled with the provider's problem as title, disabled with "Refreshed less than a minute ago: again in 42 s" for `retryInSecs` 42, disabled while busy, and enabled with the cost title otherwise; `refreshText` uses `usd` for the spend and estimate
- Accept: `inspectorProviders` keeps only `openrouter` providers; `defaultProviderId` gives the watched id when it is an OpenRouter provider, else the first OpenRouter provider, else `null`; `EFFORT_CHOICES` is `[null, 'fast', 'balanced', 'deep', 'max']`; `modelSuggestions` gives the observer view's model slugs in order
- Accept: `npm run typecheck` passes
- Status: pending

## T10 · UI: Data collection and Ranking settings
- Depends on: T9
- Claims: src/views/settings/RoutingSettings.vue, src/views/settings/RoutingRanking.vue, src/views/settings/routingRanking.ts, src/views/settings/routingRanking.test.ts
- Accept: `RANKING_FIELDS` lists the 15 fields of `RoutingSettings` other than `dataCollection` (with `tierWeights.budget`, `tierWeights.balanced` and `tierWeights.fast` as separate fields), once each, in decisions.md's groups and order, with decisions.md's labels
- Accept: `defaultText` gives, in field order, "99.5%", "99.6%", "95%", "Only the model's maker at full precision, or ones verified", "6", "3", "7 days", "60 min", "1.01", "2", "0.3", "30 tok/s", "0.5", "0.5", "1.0"
- Accept: `fromDraft(toDraft(defaults), saved)` equals the defaults with `saved.dataCollection`; `fromDraft` refuses `abc` with "“Speed samples averaged” needs a number." and `2.5` for `fallbackCount` with "“Fallback endpoints after the first” needs a whole number."
- Accept: `refusalText("fallbackCount must be at most 5")` is "“Fallback endpoints after the first” must be at most 5"; `refusalText("readmitUptimePct below minUptimePct")` names both labels and neither camelCase name; a message naming no field is returned unchanged
- Accept: `withDataCollection(saved, true)` differs from `saved` only in `dataCollection: 'allow'`; `switchBlocked` gives the view's problem when it has one and `null` otherwise
- Accept: the switch label, its help, the Nitro line and the Keep-speed-history help are exactly decisions.md's texts, and Restore defaults' title is "Deletes the saved ranking settings, Data collection included: every setting goes back to its default."
- Accept: `npm run typecheck` passes
- Status: pending

## T11 · UI: Background observation
- Depends on: T10
- Claims: src/views/settings/RoutingSettings.vue, src/views/settings/RoutingObservation.vue, src/views/settings/routingObservation.ts, src/views/settings/routingObservation.test.ts
- Accept: `observerLines` gives decisions.md's Off text with no watched provider, and "On, with OpenRouter." for a watched provider named OpenRouter
- Accept: `observerLines` gives "Last check 12 min ago: 3 models updated.", "… 1 model updated.", "… nothing was due.", "… no usable provider. <reason>", "… OpenRouter refused the key.", "… OpenRouter rate-limited the check; the next check tries again." and "… failed: Could not reach OpenRouter." (one full stop), or "No check since Chorus started." before any check, then "Next check in 18 min."
- Accept: `modelRows` gives "Updated 12 min ago · 40 samples", "· 1 sample", "Not updated yet" with no snapshot time, and "Stopped" with "Observe again" for a stopped model, else "Stop observing"
- Accept: `observerProviders` lists only OpenRouter providers, and one without its own key is disabled with "Routing needs <name>'s own API key (a management key can't fetch endpoints)."
- Accept: `npm run typecheck` passes
- Status: pending

## T12 · UI: Speed history and the Refresh log
- Depends on: T11
- Claims: src/views/settings/RoutingSettings.vue, src/views/settings/RoutingHistory.vue, src/views/settings/routingHistory.ts, src/views/settings/routingHistory.test.ts
- Accept: `speedRows` gives a row's tag, samples, "12 min ago", "42 tok/s", "0.42 s", "99.62%", and "status 2" only when the newest status is 2; missing speed, latency and uptime are "—"
- Accept: `logRows` gives "just now", the model, the provider, "Done" for no error or the error's message, the endpoints tested, and "$0.0123" and "$0.0500" for spent and estimate
- Accept: the speed history help is exactly decisions.md's text, and the empty states are "Inspect a model to see its speed history.", "No speed history for this model yet." and "No Refreshes yet."
- Accept: `npm run typecheck` passes
- Status: pending

## T13 · Proposals for _docs
- Depends on: T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12
- Claims: .chorus/run/phases/12-routing-inspector/proposals.md
- Accept: `proposals.md` has the sections "Proposed decisions", "Proposed lessons", "PLAN status" and "Open questions", and assigns no new D-number or L-number (existing ones such as D57 may be cited)
- Accept: its proposed decisions cover the Anthropic/OpenAI refusal, the new bounds, Settings → Routing's five parts, the keys `routing_observer_stopped` and `routing_refresh_log`, the observer status kept in memory, and the refusal shown in New session's tiers
- Accept: its PLAN status gives P12's row as built, with the real-app check at the milestone review still to come, and it lists spec.md's two known limits as open items
- Status: pending
