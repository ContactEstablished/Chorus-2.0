CREATE TABLE narrator_jobs (
    turn_id INTEGER PRIMARY KEY REFERENCES turns(id) ON DELETE CASCADE,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'queued',
    note TEXT,
    model TEXT,
    body TEXT,
    cost_usd REAL,
    prompt_tokens INTEGER,
    completion_tokens INTEGER,
    created_at INTEGER NOT NULL,
    finished_at INTEGER
);
CREATE INDEX narrator_pending ON narrator_jobs(status, turn_id);
CREATE TABLE narrator_actions (
    id INTEGER PRIMARY KEY,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    turn_id INTEGER REFERENCES turns(id) ON DELETE CASCADE,
    action_key TEXT NOT NULL,
    body TEXT NOT NULL,
    disposition TEXT NOT NULL DEFAULT 'open' CHECK(disposition IN ('open','done','dismissed')),
    restored INTEGER NOT NULL DEFAULT 0 CHECK(restored IN (0,1)),
    updated_at INTEGER NOT NULL,
    UNIQUE(session_id, action_key)
);
CREATE TABLE narrator_projects (
    project_id INTEGER PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    enabled INTEGER NOT NULL CHECK(enabled IN (0,1))
);
