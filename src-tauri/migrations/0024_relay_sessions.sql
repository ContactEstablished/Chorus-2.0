-- Ordinary sessions on the phone (P15 slice 4, HANDOFF-P13's P13a, D101).

-- Where plain messages from the phone go (`/use`), and the daily digest's local time ("HH:MM").
ALTER TABLE relay_config ADD COLUMN target_session INTEGER;
ALTER TABLE relay_config ADD COLUMN digest_at TEXT;

-- A session's messages: the session, and for an ask's card, the ask, so a reply or a settled ask
-- finds them.
ALTER TABLE relay_outbox ADD COLUMN session_id INTEGER;
ALTER TABLE relay_outbox ADD COLUMN ask_id INTEGER;
ALTER TABLE relay_messages ADD COLUMN session_id INTEGER;
ALTER TABLE relay_messages ADD COLUMN ask_id INTEGER;

-- Projects that send the phone full text (final messages, an approval's input); the rest send
-- status only.
CREATE TABLE relay_projects (
    project_id INTEGER PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    full_text INTEGER NOT NULL DEFAULT 0
);
