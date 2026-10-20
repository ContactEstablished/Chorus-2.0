-- P4 (brought forward from P12): OpenRouter routing tiers for opencode (D57), kept as Chorus 1 keeps
-- them. Records are JSON in chorus_core::routing's shapes.

-- Per model: the newest endpoint list ('snapshot'), seven days of speed observations per endpoint
-- ('observations') and prompt-cache verdicts per endpoint ('cache').
CREATE TABLE routing_data (
  model      TEXT    NOT NULL,
  kind       TEXT    NOT NULL CHECK (kind IN ('snapshot', 'observations', 'cache')),
  data       TEXT    NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (model, kind)
) STRICT;

-- What the two account checks found for a model with one provider's key.
CREATE TABLE routing_accounts (
  model       TEXT    NOT NULL,
  provider_id INTEGER NOT NULL REFERENCES providers (id) ON DELETE CASCADE,
  data        TEXT    NOT NULL,
  PRIMARY KEY (model, provider_id)
) STRICT;

-- The routing a session launched with, which Restart reuses without ranking again.
ALTER TABLE sessions ADD COLUMN routing TEXT;
