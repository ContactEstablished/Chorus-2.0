CREATE TABLE composer_drafts (
    session_id INTEGER PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    text TEXT NOT NULL DEFAULT '',
    revision INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE prompt_queue (
    id INTEGER PRIMARY KEY,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    run_id INTEGER NOT NULL REFERENCES runs(id),
    request_id TEXT NOT NULL,
    submitted_run_id INTEGER NOT NULL,
    draft_revision INTEGER NOT NULL,
    was_queued INTEGER NOT NULL,
    text TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('queued','held','attempted','cancelled')),
    reason TEXT,
    created_at INTEGER NOT NULL,
    prompt_id INTEGER REFERENCES prompts(id),
    UNIQUE(session_id, request_id)
);
CREATE INDEX prompt_queue_pending ON prompt_queue(session_id,status,id);
