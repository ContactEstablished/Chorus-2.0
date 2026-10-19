-- P2: providers (OpenRouter, OpenAI-compatible gateways, Jev), their model catalogs and shortlists,
-- and the ledger of capped keys minted for runs. Keys stay in `credentials`; rows here only point
-- at them.

CREATE TABLE providers (
  id                       INTEGER PRIMARY KEY,
  kind                     TEXT    NOT NULL CHECK (kind IN ('openrouter', 'gateway', 'jev')),
  name                     TEXT    NOT NULL UNIQUE,
  -- A gateway's address; OpenRouter and Jev use their fixed ones.
  base_url                 TEXT,
  -- The key agents (or the connection test) use.
  credential_id            INTEGER REFERENCES credentials (id) ON DELETE SET NULL,
  -- OpenRouter only: the management key that mints a capped key per run.
  management_credential_id INTEGER REFERENCES credentials (id) ON DELETE SET NULL,
  -- The spend cap of each minted key, in US dollars.
  cap_usd                  REAL,
  tested_at                INTEGER,
  test_ok                  INTEGER CHECK (test_ok IN (0, 1)),
  test_note                TEXT,
  catalog_refreshed_at     INTEGER,
  created_at               INTEGER NOT NULL
) STRICT;

-- What the provider's model list said, one row per model, kept when a model disappears.
CREATE TABLE model_catalog (
  provider_id   INTEGER NOT NULL REFERENCES providers (id) ON DELETE CASCADE,
  model_id      TEXT    NOT NULL,
  -- `chorus_core::providers::CatalogModel` as JSON.
  data          TEXT    NOT NULL,
  first_seen_at INTEGER NOT NULL,
  missing_since INTEGER,
  PRIMARY KEY (provider_id, model_id)
) STRICT;

-- The models the user picks from at launch. Not tied to the catalog: an id can be shortlisted
-- before a refresh lists it.
CREATE TABLE shortlist (
  provider_id INTEGER NOT NULL REFERENCES providers (id) ON DELETE CASCADE,
  model_id    TEXT    NOT NULL,
  added_at    INTEGER NOT NULL,
  PRIMARY KEY (provider_id, model_id)
) STRICT;

-- Every key minted for a run. The key itself is never stored, only OpenRouter's hash of it. A row
-- with `revoked_at` NULL is a key that may still be live; start-up sweeps those.
CREATE TABLE capped_keys (
  id           INTEGER PRIMARY KEY,
  provider_id  INTEGER,
  session_id   INTEGER NOT NULL,
  run_id       INTEGER,
  hash         TEXT,
  limit_usd    REAL    NOT NULL,
  minted_at    INTEGER NOT NULL,
  revoked_at   INTEGER,
  -- What OpenRouter's analytics settled on after revoking; NULL until known.
  cost_usd     REAL,
  cost_note    TEXT
) STRICT;

CREATE INDEX capped_keys_open ON capped_keys (revoked_at) WHERE revoked_at IS NULL;

-- The provider a session's agent runs on, if not its own login or key. No foreign key on purpose:
-- if the provider is deleted, the next launch says so instead of quietly running on the agent's
-- own login.
ALTER TABLE sessions ADD COLUMN provider_id INTEGER;
