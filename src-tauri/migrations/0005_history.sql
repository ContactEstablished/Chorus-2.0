-- P3: durable history (D7). Every prompt, each turn with its final message, the tool calls (tool
-- and target, never raw output), and questions and approvals with their answers. Text is scrubbed
-- before it is written. Full transcripts stay in each agent's own files.

CREATE TABLE prompts (
  id         INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES sessions (id),
  run_id     INTEGER REFERENCES runs (id),
  -- Where it came from: an agent's hook or server for a prompt typed in its terminal; later the
  -- composer, Telegram and automations.
  source     TEXT    NOT NULL,
  text       TEXT    NOT NULL,
  created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX prompts_session ON prompts (session_id, id);

CREATE TABLE turns (
  id            INTEGER PRIMARY KEY,
  session_id    INTEGER NOT NULL REFERENCES sessions (id),
  run_id        INTEGER REFERENCES runs (id),
  -- The agent's own id for the turn (Claude Code's prompt_id, Codex's turn_id), when it gives one.
  agent_turn    TEXT,
  prompt_id     INTEGER REFERENCES prompts (id),
  started_at    INTEGER NOT NULL,
  ended_at      INTEGER,
  end_reason    TEXT CHECK (end_reason IN ('done', 'interrupted', 'failed', 'lost')),
  final_message TEXT
) STRICT;

CREATE INDEX turns_session ON turns (session_id, id);

CREATE TABLE tool_calls (
  id         INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES sessions (id),
  turn_id    INTEGER REFERENCES turns (id),
  call_ref   TEXT,
  tool       TEXT    NOT NULL,
  target     TEXT,
  started_at INTEGER NOT NULL,
  ended_at   INTEGER,
  failed     INTEGER NOT NULL DEFAULT 0 CHECK (failed IN (0, 1))
) STRICT;

CREATE INDEX tool_calls_turn ON tool_calls (turn_id);

CREATE TABLE asks (
  id          INTEGER PRIMARY KEY,
  session_id  INTEGER NOT NULL REFERENCES sessions (id),
  turn_id     INTEGER REFERENCES turns (id),
  kind        TEXT    NOT NULL CHECK (kind IN ('approval', 'question')),
  ask_ref     TEXT,
  subject     TEXT    NOT NULL,
  asked_at    INTEGER NOT NULL,
  answered_at INTEGER,
  answer      TEXT
) STRICT;

CREATE INDEX asks_turn ON asks (turn_id);

-- What each agent sent, scrubbed and cut short, for diagnosis. Pruned after a while.
CREATE TABLE agent_events (
  id         INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL,
  run_id     INTEGER,
  at         INTEGER NOT NULL,
  source     TEXT    NOT NULL,
  name       TEXT,
  body       TEXT    NOT NULL
) STRICT;

CREATE INDEX agent_events_session ON agent_events (session_id, id);
CREATE INDEX agent_events_at ON agent_events (at);
