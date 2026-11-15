-- P16 (D119): each change of a session's activity, timed by Chorus's clock, so the Progress panel
-- can say where the time went. Written where the recorder applies events, and as 'unknown' when a
-- run starts. Never inferred from screen text. A run with no rows reads "not split".

CREATE TABLE activity_log (
  id         INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES sessions (id),
  run_id     INTEGER NOT NULL REFERENCES runs (id) ON DELETE CASCADE,
  activity   TEXT    NOT NULL CHECK (activity IN
    ('unknown', 'working', 'awaiting-approval', 'needs-answer', 'ready', 'failed')),
  -- 1 when 'needs-answer' only came from the final message ending with '?' (D125): no request
  -- is open, so the split lets it go idle like 'ready'.
  guessed    INTEGER NOT NULL DEFAULT 0 CHECK (guessed IN (0, 1) AND (guessed = 0 OR activity = 'needs-answer')),
  at         INTEGER NOT NULL
) STRICT;

-- Covers the panel's read (WHERE run_id ORDER BY at, id), so it never visits the table.
CREATE INDEX activity_log_run ON activity_log (run_id, at, id, activity, guessed);
