-- Persist the prompt and its delivery attempt together before writing to the agent.
-- An uncertain attempt is never replayed automatically, including after a restart/crash.
CREATE TABLE prompt_deliveries (
  prompt_id INTEGER PRIMARY KEY REFERENCES prompts(id),
  session_id INTEGER NOT NULL REFERENCES sessions(id),
  run_id INTEGER NOT NULL REFERENCES runs(id),
  request_id TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('uncertain', 'written', 'observed')),
  attempted_at INTEGER NOT NULL,
  UNIQUE (session_id, request_id)
) STRICT;
CREATE INDEX prompt_deliveries_run ON prompt_deliveries(run_id, status);
