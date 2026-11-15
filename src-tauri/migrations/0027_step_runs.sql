-- P16 (D121): finished, timed runs of named steps, for "usual time" markers. Only skills (by
-- name) and Chorus's own steps are keyed; free-text checklist steps never are, and untimed runs are
-- never written. P16 writes skills only; 'chorus' is kept for Chorus's own steps (P17), so that
-- needs no table rebuild.

CREATE TABLE step_runs (
  id          INTEGER PRIMARY KEY,
  key_kind    TEXT    NOT NULL CHECK (key_kind IN ('skill', 'chorus')),
  key         TEXT    NOT NULL,
  session_id  INTEGER NOT NULL REFERENCES sessions (id),
  turn_id     INTEGER REFERENCES turns (id),
  -- What the run came from, so it is written once: a skill's tool_calls.id.
  source_ref  TEXT    NOT NULL,
  started_at  INTEGER NOT NULL,
  ended_at    INTEGER NOT NULL,
  duration_ms INTEGER NOT NULL CHECK (duration_ms >= 0),
  -- 1 when the time is an estimate (a skill's, from its call to the next step).
  estimate    INTEGER NOT NULL CHECK (estimate IN (0, 1)),
  UNIQUE (key_kind, source_ref)
) STRICT;

CREATE INDEX step_runs_key ON step_runs (key_kind, key, ended_at);
