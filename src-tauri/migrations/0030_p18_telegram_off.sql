-- P18 (D129): the phone route is OpenMausBot now, so the Telegram relay is switched off once.
-- It stays built, with its token and pairing; Settings → Phone can switch it back on.
UPDATE relay_config SET enabled = 0;
