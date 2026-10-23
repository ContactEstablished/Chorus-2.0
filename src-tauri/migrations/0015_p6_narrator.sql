-- Narrator jobs are claimed before HTTP. A restart never replays a possibly charged request.
CREATE TABLE narrator_jobs (
    turn_id INTEGER PRIMARY KEY REFERENCES turns(id) ON DELETE CASCADE,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK(status IN ('pending','running','ready','fallback')),
    reason TEXT,
    summary TEXT,
    model TEXT,
    requested INTEGER NOT NULL DEFAULT 0,
    cost_usd REAL,
    created_at INTEGER NOT NULL
);
CREATE INDEX narrator_jobs_pending ON narrator_jobs(status, turn_id);
CREATE TABLE narrator_actions (
    id INTEGER PRIMARY KEY,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    turn_id INTEGER REFERENCES turns(id) ON DELETE SET NULL,
    signature TEXT NOT NULL,
    suggestion TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open' CHECK(status IN ('open','done','dismissed')),
    updated_at INTEGER NOT NULL,
    UNIQUE(session_id, signature)
);
