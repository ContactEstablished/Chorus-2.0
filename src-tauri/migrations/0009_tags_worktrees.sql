-- P4: session tags (D52), and the worktrees New session makes (D53, brought forward from P5).

CREATE TABLE session_tags (
  session_id INTEGER NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
  -- Cleaned by chorus_core::tags: lowercase, at most 24 characters.
  tag        TEXT    NOT NULL,
  position   INTEGER NOT NULL,
  PRIMARY KEY (session_id, tag)
) STRICT;

CREATE INDEX session_tags_tag ON session_tags (tag);

CREATE TABLE worktrees (
  id          INTEGER PRIMARY KEY,
  -- The repository's main working tree, where `git worktree` runs.
  repo_root   TEXT    NOT NULL,
  path        TEXT    NOT NULL,
  branch      TEXT    NOT NULL,
  -- The branch it was made from; null when that folder had a detached HEAD.
  base_branch TEXT,
  created_at  INTEGER NOT NULL,
  -- When Chorus removed it. Its branch is never deleted.
  removed_at  INTEGER
) STRICT;

-- Sessions of one launch (a Pair, a Workbench) can share a worktree.
ALTER TABLE sessions ADD COLUMN worktree_id INTEGER REFERENCES worktrees (id) ON DELETE SET NULL;
