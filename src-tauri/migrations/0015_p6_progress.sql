-- Structured checklist snapshots and visible assistant text; never thinking or raw tool output.
CREATE TABLE plan_snapshots (
    id INTEGER PRIMARY KEY,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    run_id INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    turn_id INTEGER REFERENCES turns(id) ON DELETE SET NULL,
    event_ref TEXT NOT NULL,
    steps TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(run_id, event_ref)
);
CREATE INDEX plan_snapshots_session ON plan_snapshots(session_id, id DESC);
CREATE TABLE plan_capture_issues (
    run_id INTEGER PRIMARY KEY REFERENCES runs(id) ON DELETE CASCADE
);
CREATE TABLE progress_messages (
    id INTEGER PRIMARY KEY,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    run_id INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    turn_id INTEGER REFERENCES turns(id) ON DELETE SET NULL,
    message_ref TEXT NOT NULL,
    text TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    UNIQUE(session_id, message_ref)
);
CREATE INDEX progress_messages_session ON progress_messages(session_id, id DESC);
