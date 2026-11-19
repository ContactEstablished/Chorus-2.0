-- P18 (D130): a project handed to OpenMausBot on its own branch, and back. The hand-off note
-- mirrors the active row; this table is the source of truth.
CREATE TABLE handoffs (
  id            INTEGER PRIMARY KEY,
  project_id    INTEGER NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
  state         TEXT    NOT NULL CHECK (state IN ('active', 'returned', 'closed')),
  task          TEXT    NOT NULL DEFAULT '',
  bot_id        TEXT,
  bot_name      TEXT,
  thread_id     TEXT,
  -- Why Chorus didn't start MausBot itself (no pairing, MausBot not running, a refusal).
  start_problem TEXT,
  branch        TEXT    NOT NULL,
  worktree      TEXT    NOT NULL,
  base_branch   TEXT,
  base_sha      TEXT    NOT NULL,
  started_at    INTEGER NOT NULL,
  returned_at   INTEGER,
  returned_by   TEXT CHECK (returned_by IN ('chorus', 'mausbot')),
  -- The entries the return brought back (scrubbed raw Markdown, a JSON array), so the card shows
  -- them even when the project's note couldn't take them.
  returned_entries TEXT,
  closed_at     INTEGER
) STRICT;

-- One hand-off at a time per project.
CREATE UNIQUE INDEX handoffs_one_active ON handoffs (project_id) WHERE state = 'active';
CREATE INDEX handoffs_project ON handoffs (project_id, id);
