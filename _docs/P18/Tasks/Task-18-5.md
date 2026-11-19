# Task 18-5: Telegram off

## Source Of Truth
- D129 and D132.
- [Phase-18-Overview.md](../Phase-18-Overview.md) K16.
- [ImplementationSpec-18-5.md](../ImplementationSpecs/ImplementationSpec-18-5.md).

## Initial Starting Point (verified 2026-11-18)
- **The relay is on:**
  - `relay_config.enabled INTEGER NOT NULL DEFAULT 1` (`0023_p15_loop.sql:113`), with the row
    seeded at `:117`.
  - `relay::start` runs unconditionally (`lib.rs:137-138`). With `enabled` false it idles
    (`relay/inbound.rs:40-53`), and `active_token()` returns None (`relay/mod.rs:61-67`).
- **The switch** is in Settings → Phone (`src/views/settings/PhoneSettings.vue:137`), and calls
  `relay_set_enabled` (`relay/mod.rs:245-249`). The copy still describes Telegram as the phone
  route (`PhoneSettings.vue:2-5`, `:100`).
- **The docs:**
  - D16 says "Telegram is the only remote access" (`DECISIONS.md:27`).
  - PLAN's P15 row says the relay goes through OpenMausBot (`PLAN.md:98`), which overstates what is
    built.
  - `HANDOFF-P15-loop.md:13-14` still names Telegram as the loop's channel.

## Goal
The Telegram relay is off, on every database, and Settings → Phone says why. It can still be
switched back on. The docs say the phone route is MausBot.

## Exact Scope
- **Create:** `src-tauri/migrations/0030_p18_telegram_off.sql`.
- **Edit:**
  - `src-tauri/src/store/mod.rs` (the list)
  - `src/views/settings/PhoneSettings.vue` (the copy only)
  - `_docs/DECISIONS.md` (D16's status cell, and D133 for K1–K17)
  - `_docs/HANDOFF-P15-loop.md` (a note at the top)
  - `_docs/PLAN.md` (the P13, P15 and P18 rows)
  - `_docs/HANDOFF-P18-mausbot.md` (status)

## Non-Goals
- Removing the relay's code, its token or its pairing (D129: off, not removed).
- Building P15's MausBot bridge (P15's own slices).
- Do not revert or commit unrelated changes.

## Dependencies
18-3, for the migration number.

## Step-by-step Work
1. **Migration:** `UPDATE relay_config SET enabled = 0;`, with a comment naming D129. Add it to the
   list.
2. **Store test:** a database at 0029 with `enabled = 1` reads `enabled = false` after migrating.
3. **`PhoneSettings.vue`:** when off, say why (spec §2). Keep the switch.
4. **Docs** (spec §3).
5. **Real app:** Settings → Phone shows off, with the reason. Switching it on and off still works.
   Screenshot it.

## Test Expectations
The migration test above. No UI test beyond typecheck.

## Verification Commands
```
cargo test -p chorus --lib store::tests
npm run check            # PowerShell
git status --short
```

## Acceptance Criteria
- A fresh database, and an upgraded one, both have `enabled = 0`.
- Settings → Phone shows the relay off, with the D129 sentence, and the switch still works.
- D16 is marked "Partly superseded by D129". The PLAN rows match what is built.

## Review Checklist
- The token and pairing rows are untouched.
- The relay's threads still start, and idle.
- The copy doesn't claim MausBot does anything Chorus doesn't build yet.
