ALTER TABLE asks ADD COLUMN details TEXT;
CREATE TABLE reply_attempts (
    id INTEGER PRIMARY KEY,
    session_id INTEGER NOT NULL REFERENCES sessions(id),
    run_id INTEGER NOT NULL REFERENCES runs(id),
    ask_id INTEGER NOT NULL UNIQUE REFERENCES asks(id),
    request_id TEXT NOT NULL,
    action TEXT NOT NULL,
    attempted_at INTEGER NOT NULL,
    UNIQUE(session_id, request_id)
);
