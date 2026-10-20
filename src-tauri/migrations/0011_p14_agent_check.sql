-- P14: widen `sessions.agent` for the agents this phase adds (Gemini CLI, Aider). A new agent kind
-- can't be stored until the CHECK names it, and SQLite can only change a CHECK by rebuilding the
-- table. The header below turns foreign keys off around this file's transaction (store::migrate,
-- chorus_core::migrate::directives): with them on, `DROP TABLE sessions` would cascade-delete every
-- run and tag, and the history tables' references would refuse the drop. This is SQLite's documented
-- 12-step rebuild: build the new table, copy, drop the old, rename, then check every foreign key
-- before committing.
--
-- chorus: foreign_keys off

CREATE TABLE sessions_new (
  id             INTEGER PRIMARY KEY,
  name           TEXT    NOT NULL,
  agent          TEXT    NOT NULL CHECK (agent IN ('shell', 'claude', 'codex', 'opencode', 'grok', 'gemini', 'aider')),
  cwd            TEXT    NOT NULL,
  state          TEXT    NOT NULL CHECK (state IN ('running', 'exited', 'closed')),
  credential_id  INTEGER REFERENCES credentials (id) ON DELETE SET NULL,
  credential_env TEXT,
  cols           INTEGER NOT NULL,
  rows           INTEGER NOT NULL,
  locked         INTEGER NOT NULL DEFAULT 0 CHECK (locked IN (0, 1)),
  -- Why the session is not running, in words the UI shows as is (fail visible).
  note           TEXT,
  created_at     INTEGER NOT NULL,
  closed_at      INTEGER,
  -- Added by 0002, 0003, 0004, 0007, 0009 and 0010. A rebuild has to carry every column, in the
  -- shape those files left them.
  options          TEXT    NOT NULL DEFAULT '{}',
  agent_session_id TEXT,
  provider_id      INTEGER,
  description      TEXT,
  project_id       INTEGER REFERENCES projects (id) ON DELETE SET NULL,
  worktree_id      INTEGER REFERENCES worktrees (id) ON DELETE SET NULL,
  routing          TEXT
) STRICT;

INSERT INTO sessions_new (
  id, name, agent, cwd, state, credential_id, credential_env, cols, rows, locked, note, created_at,
  closed_at, options, agent_session_id, provider_id, description, project_id, worktree_id, routing
)
SELECT
  id, name, agent, cwd, state, credential_id, credential_env, cols, rows, locked, note, created_at,
  closed_at, options, agent_session_id, provider_id, description, project_id, worktree_id, routing
FROM sessions;

DROP TABLE sessions;
ALTER TABLE sessions_new RENAME TO sessions;
