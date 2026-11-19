# Implementation spec 18-5: Telegram off

## 1. Migration `0030_p18_telegram_off.sql`

```sql
-- P18 (D129): the phone route is OpenMausBot now, so the Telegram relay is switched off once.
-- It stays built, with its token and pairing; Settings → Phone can switch it back on.
UPDATE relay_config SET enabled = 0;
```

Add it to `MIGRATION_FILES` after 0029.

**The store test:**
1. Open a store at 0029.
2. Set `relay_config.enabled = 1`.
3. Migrate to the latest schema.
4. Expect `relay::db::config(store).enabled == false`, and `credential_id` and `bot_name` unchanged.

## 2. `PhoneSettings.vue`

Copy changes only. Keep every control and command call.

- **The intro (`:2-5` and `:100`):** say the Telegram relay is one way to reach Chorus from a phone,
  that Matthew now uses OpenMausBot for that, and that the relay is off unless switched on here.
- **The off-state line,** shown when `!relay.enabled`: "Off. Your phone route is OpenMausBot now
  (D129), so Chorus doesn't poll Telegram. Switch it on here if you want Telegram back." Use the
  muted text token.

## 3. Docs
- **`DECISIONS.md`:**
  - D16's status becomes `Partly superseded by D129`.
  - Append D133, recording K1–K17 from the phase overview in one row: "Settled by Claude at P18's
    kickoff as implementation choices; told to Matthew".
- **`HANDOFF-P15-loop.md`:** a note at the top. "D132 (2026-11-18): the loop's remote channel
  becomes OpenMausBot. The Telegram relay described below is built but switched off (D129). P15's
  remaining slices design the MausBot bridge."
- **`PLAN.md`:**
  - P13's row: the relay is built and switched off (D129).
  - P15's row: the phone relay "is to go through OpenMausBot (D132); the built Telegram relay is
    off (D129)".
  - P18's row and section: the slices done, with what is still waiting (the pairing run).
- **`HANDOFF-P18-mausbot.md`:** the status line.

## 4. Verification in the app (`p18`)
- Settings → Phone shows the switch off, with the line from §2. Screenshot it, in Glacier.
- Switch it on: it shows Connecting or No token, as before. Switch it off again.
- No Telegram request is made while off. The relay's poll loop idles; a debug log isn't needed.
