CREATE TABLE turn_progress (
    id INTEGER PRIMARY KEY,
    turn_id INTEGER NOT NULL REFERENCES turns(id) ON DELETE CASCADE,
    source_key TEXT NOT NULL,
    body TEXT NOT NULL,
    at INTEGER NOT NULL,
    UNIQUE(turn_id, source_key)
);
CREATE INDEX turn_progress_order ON turn_progress(turn_id, at, id);
