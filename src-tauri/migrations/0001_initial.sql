-- P1: settings, the key vault, sessions and their runs (ARCHITECTURE §7).
-- Times are Unix milliseconds. Nothing in this schema may hold a plaintext key.

CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT;

-- Keys sealed with DPAPI (current user). `fingerprint` is the first 16 hex digits of the key's
-- SHA-256, enough to tell keys apart without revealing them.
CREATE TABLE credentials (
  id           INTEGER PRIMARY KEY,
  label        TEXT    NOT NULL UNIQUE,
  blob         BLOB    NOT NULL,
  fingerprint  TEXT    NOT NULL,
  created_at   INTEGER NOT NULL,
  last_used_at INTEGER
) STRICT;

-- A session is long-lived; each launch of its process is a run. `state` is what the session
-- should be doing: 'running' sessions are relaunched when Chorus starts.
CREATE TABLE sessions (
  id             INTEGER PRIMARY KEY,
  name           TEXT    NOT NULL,
  agent          TEXT    NOT NULL CHECK (agent IN ('shell', 'claude', 'codex', 'opencode', 'grok')),
  cwd            TEXT    NOT NULL,
  state          TEXT    NOT NULL CHECK (state IN ('running', 'exited', 'closed')),
  credential_id  INTEGER REFERENCES credentials (id) ON DELETE SET NULL,
  credential_env TEXT,
  cols           INTEGER NOT NULL,
  rows           INTEGER NOT NULL,
  locked         INTEGER NOT NULL DEFAULT 0 CHECK (locked IN (0, 1)),
  -- Why the session is not running, in words the UI shows as is (fail visible).
  note           TEXT,
  created_at     INTEGER NOT NULL,
  closed_at      INTEGER
) STRICT;

CREATE TABLE runs (
  id         INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
  pid        INTEGER,
  program    TEXT    NOT NULL,
  -- JSON array of arguments. Keys never travel in argv, so nothing here is secret.
  args       TEXT    NOT NULL,
  started_at INTEGER NOT NULL,
  ended_at   INTEGER,
  exit_code  INTEGER,
  end_reason TEXT CHECK (end_reason IN (
    'exited', 'closed', 'restarted', 'chorus-closed', 'chorus-crashed', 'system-shutdown'
  ))
) STRICT;

CREATE INDEX runs_by_session ON runs (session_id, started_at);
