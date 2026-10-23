-- Instructions survive restart, but their reviewed run cannot silently change.
CREATE TABLE instruction_queue (
  id INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES sessions(id),
  run_id INTEGER NOT NULL REFERENCES runs(id),
  request_id TEXT NOT NULL,
  text TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('queued','cancelled','attempted')),
  prompt_id INTEGER REFERENCES prompts(id),
  created_at INTEGER NOT NULL,
  UNIQUE(session_id, request_id)
) STRICT;
CREATE INDEX instruction_queue_pending ON instruction_queue(session_id,status,id);
