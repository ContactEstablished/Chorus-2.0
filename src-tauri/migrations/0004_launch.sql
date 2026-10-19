-- P2: the user's own note on a session (what it is for), and saved launch profiles (the seed of
-- Bots in P8).

-- Shown on the session's card and header. `note` stays Chorus's own (why it isn't running, what a
-- launch found).
ALTER TABLE sessions ADD COLUMN description TEXT;

-- A named set of launch settings: agent, options, provider, and a stored key as a variable. Keys
-- stay in `credentials`. `provider_id` has no foreign key, so a deleted provider is reported when
-- the profile is used rather than quietly dropped.
CREATE TABLE launch_profiles (
  id             INTEGER PRIMARY KEY,
  name           TEXT    NOT NULL UNIQUE,
  agent          TEXT    NOT NULL,
  options        TEXT    NOT NULL DEFAULT '{}',
  provider_id    INTEGER,
  credential_id  INTEGER REFERENCES credentials (id) ON DELETE SET NULL,
  credential_env TEXT,
  created_at     INTEGER NOT NULL,
  updated_at     INTEGER NOT NULL
) STRICT;
