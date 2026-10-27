-- Tokens and cost per turn, as the agent reported them (P11's shape, D107; opencode only so far).
-- Every figure is nullable: unknown is not 0.

CREATE TABLE turn_usage (
  turn_id     INTEGER PRIMARY KEY REFERENCES turns (id) ON DELETE CASCADE,
  input       INTEGER,
  cached      INTEGER,
  cache_write INTEGER,
  output      INTEGER,
  reasoning   INTEGER,
  cost_usd    REAL,
  -- 'reported': the agent's own figure. P11 adds 'estimate' and 'measured'.
  cost_kind   TEXT CHECK (cost_kind IN ('reported', 'estimate', 'measured')),
  -- Where the figures came from, as `measure::ledger::UsageSource` names it.
  source      TEXT NOT NULL,
  updated_at  INTEGER NOT NULL
) STRICT;
