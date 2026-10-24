-- P7: the notification centre's history. One row per waiting episode (dedupe_key), kept 30 days.
CREATE TABLE notifications (
    id INTEGER PRIMARY KEY,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    read_at INTEGER,
    dedupe_key TEXT NOT NULL UNIQUE
);
CREATE INDEX notifications_created ON notifications(created_at);
