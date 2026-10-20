-- P4: projects, which group sessions in the rail and the Overview.

CREATE TABLE projects (
  id            INTEGER PRIMARY KEY,
  name          TEXT    NOT NULL,
  -- The folder its sessions usually work in; a session whose folder is inside it joins it.
  folder        TEXT,
  -- #rrggbb.
  colour        TEXT    NOT NULL,
  position      INTEGER NOT NULL,
  state         TEXT    NOT NULL DEFAULT 'active' CHECK (state IN ('active', 'hidden', 'archived')),
  -- 'chorus1' for projects brought over by the one-time import (D33), with Chorus 1's id.
  imported_from TEXT,
  imported_id   TEXT,
  created_at    INTEGER NOT NULL
) STRICT;

CREATE INDEX projects_position ON projects (position);

-- A deleted project leaves its sessions untagged, still running.
ALTER TABLE sessions ADD COLUMN project_id INTEGER REFERENCES projects (id) ON DELETE SET NULL;
