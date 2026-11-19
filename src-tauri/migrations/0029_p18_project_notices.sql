-- P18 (K13): a notice can belong to a project instead of a session (branch hygiene, the baton).
-- `session_id` loses its NOT NULL, and SQLite can only drop a NOT NULL by rebuilding the table. The
-- header below turns foreign keys off around this file's transaction (store::migrate,
-- chorus_core::migrate::directives), so dropping the old table cascades nowhere; the runner checks
-- `PRAGMA foreign_key_check` before it commits.
-- chorus: foreign_keys off

CREATE TABLE notifications_p18 (
    id INTEGER PRIMARY KEY,
    session_id INTEGER REFERENCES sessions(id) ON DELETE CASCADE,
    project_id INTEGER REFERENCES projects(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    read_at INTEGER,
    dedupe_key TEXT NOT NULL UNIQUE,
    payload TEXT,
    CHECK (session_id IS NOT NULL OR project_id IS NOT NULL)
);
INSERT INTO notifications_p18 (id, session_id, kind, title, body, created_at, read_at, dedupe_key)
  SELECT id, session_id, kind, title, body, created_at, read_at, dedupe_key FROM notifications;
DROP TABLE notifications;
ALTER TABLE notifications_p18 RENAME TO notifications;
CREATE INDEX notifications_created ON notifications(created_at);

-- D131: Keep hides a stale branch for 30 days.
CREATE TABLE branch_snoozes (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    branch     TEXT    NOT NULL,
    until_at   INTEGER NOT NULL,
    PRIMARY KEY (project_id, branch)
) STRICT;

-- D131: the stale spell each branch was last flagged for (its notice's dedupe key: its last
-- commit, and the end of any Keep that ran out), so it is flagged once per spell even after the
-- notice itself is pruned (notices are kept 30 days).
CREATE TABLE branch_flags (
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    branch     TEXT    NOT NULL,
    spell      TEXT    NOT NULL,
    PRIMARY KEY (project_id, branch)
) STRICT;
