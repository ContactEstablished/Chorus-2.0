-- D135: every message Chorus sends to OpenMausBot ("Chorus updates"), scrubbed, written before it
-- is sent and finished with what happened. `text` is the message without the echo instruction.
-- `session_id` and `run_id` say where it came from; a test message has neither.
CREATE TABLE maus_updates (
    id          INTEGER PRIMARY KEY,
    at          INTEGER NOT NULL,
    session_id  INTEGER REFERENCES sessions(id) ON DELETE SET NULL,
    run_id      INTEGER,
    kind        TEXT    NOT NULL,
    text        TEXT    NOT NULL,
    outcome     TEXT    NOT NULL,
    reason      TEXT
) STRICT;
CREATE INDEX maus_updates_at ON maus_updates(at);
