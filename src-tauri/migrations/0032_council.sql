-- P10 Council (D136–D138): members, runs, turns and the tool calls each turn made.

-- A participant: a model on the Council's OpenRouter provider (Settings → Council), as a member
-- or as the arbiter. Global, not per project.
CREATE TABLE council_members (
  id          INTEGER PRIMARY KEY,
  label       TEXT    NOT NULL UNIQUE COLLATE NOCASE,
  model       TEXT    NOT NULL,
  role        TEXT    NOT NULL CHECK (role IN ('member', 'arbiter')),
  -- NULL: the role's default.
  max_tokens  INTEGER,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
) STRICT;

-- One review of one brief. The brief's text and the roster are copied in at the start, so the
-- run reads the same later even if the brief or a member changes.
CREATE TABLE council_runs (
  id             INTEGER PRIMARY KEY,
  project_id     INTEGER NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
  project_folder TEXT    NOT NULL,
  brief_path     TEXT    NOT NULL,
  brief_text     TEXT    NOT NULL,
  -- [{id, label, model, role}], the arbiter last.
  roster_json    TEXT    NOT NULL,
  limits_json    TEXT    NOT NULL,
  status         TEXT    NOT NULL CHECK (status IN ('running', 'complete', 'failed', 'cancelled', 'abandoned')),
  -- Why it failed, was cancelled or abandoned.
  reason         TEXT,
  -- The per-question glance, once the positions are in.
  summary_json   TEXT,
  findings_path  TEXT,
  findings_error TEXT,
  provider_id    INTEGER,
  -- capped_keys.id of the run's key (its row has session_id 0).
  key_ledger_id  INTEGER,
  started_at     INTEGER NOT NULL,
  ended_at       INTEGER
) STRICT;

CREATE INDEX council_runs_project ON council_runs (project_id, started_at);

-- One participant's turn in one phase. The prompt is stored scrubbed before it is sent; the
-- outcome is NULL while the turn is in flight.
CREATE TABLE council_turns (
  id            INTEGER PRIMARY KEY,
  run_id        INTEGER NOT NULL REFERENCES council_runs (id) ON DELETE CASCADE,
  -- Soft: the roster in the run holds its label and model.
  member_id     INTEGER NOT NULL,
  phase         TEXT    NOT NULL CHECK (phase IN ('positions', 'critique', 'arbitration', 'synthesis')),
  prompt        TEXT    NOT NULL,
  outcome       TEXT    CHECK (outcome IN ('answered', 'refused', 'cancelled')),
  -- The answer, scrubbed, or the reason it has none.
  content       TEXT,
  tokens_in     INTEGER,
  tokens_out    INTEGER,
  tokens_cached INTEGER,
  -- From each response's usage.cost; NULL when no response reported it.
  cost_usd      REAL,
  started_at    INTEGER NOT NULL,
  ended_at      INTEGER
) STRICT;

CREATE INDEX council_turns_run ON council_turns (run_id, id);

-- Every code tool call a turn made: what it asked for and how much came back. The content is
-- not kept; it is the project's code, readable again.
CREATE TABLE council_tool_calls (
  id         INTEGER PRIMARY KEY,
  turn_id    INTEGER NOT NULL REFERENCES council_turns (id) ON DELETE CASCADE,
  call       TEXT    NOT NULL,
  ok         INTEGER NOT NULL CHECK (ok IN (0, 1)),
  -- Bytes returned, or the refusal's reason.
  bytes      INTEGER NOT NULL,
  note       TEXT,
  created_at INTEGER NOT NULL
) STRICT;

CREATE INDEX council_tool_calls_turn ON council_tool_calls (turn_id, id);
