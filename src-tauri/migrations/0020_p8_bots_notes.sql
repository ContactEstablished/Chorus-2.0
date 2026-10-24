-- P8: Bots (the launch profiles, grown), notes, automations and documents.
-- Nothing here holds a plaintext key: instruction text and prompts are scrubbed before they are
-- written, and a Bot's key stays in `credentials`.

-- A Bot is a saved launch profile with a description, instructions, tags, skill folders and a
-- preset (decision 1). Existing profiles keep their settings and become Bots.
ALTER TABLE launch_profiles RENAME TO bots;
ALTER TABLE bots ADD COLUMN description TEXT;
-- Markdown, added to the agent's instructions of every session started from the Bot.
ALTER TABLE bots ADD COLUMN instructions TEXT NOT NULL DEFAULT '';
-- JSON array of labels, cleaned like session tags.
ALTER TABLE bots ADD COLUMN tags TEXT NOT NULL DEFAULT '[]';
-- JSON array of absolute folder paths, each holding a SKILL.md (provider-neutral skills, D31).
ALTER TABLE bots ADD COLUMN skills TEXT NOT NULL DEFAULT '[]';
-- The preset Start session uses: solo, pair, workbench or swarm. NULL is Solo.
ALTER TABLE bots ADD COLUMN preset TEXT CHECK (preset IN ('solo', 'pair', 'workbench', 'swarm'));
-- An archived Bot is hidden from New session and can't be started; its sessions keep running.
ALTER TABLE bots ADD COLUMN archived_at INTEGER;

-- The Bot a session was started from, and what it carried, copied at create so editing the Bot
-- never changes the session (JSON: name, instructions and skill folders).
ALTER TABLE sessions ADD COLUMN bot_id INTEGER REFERENCES bots (id) ON DELETE SET NULL;
ALTER TABLE sessions ADD COLUMN bot_snapshot TEXT;

-- What a run's instructions held (JSON: the Bot, the notes by id and title, how many were left
-- out, the skills, or why none were given). Shown in session facts.
ALTER TABLE runs ADD COLUMN instructions TEXT;

-- Context added to agents' instructions at each run start (D17). A note with no project applies
-- to every project. Pinned notes come first.
CREATE TABLE notes (
  id         INTEGER PRIMARY KEY,
  project_id INTEGER REFERENCES projects (id) ON DELETE CASCADE,
  title      TEXT    NOT NULL,
  body       TEXT    NOT NULL,
  pinned     INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
) STRICT;

CREATE INDEX notes_by_project ON notes (project_id, pinned, updated_at);

-- A Bot run on a schedule, only while Chorus runs. `schedule` is JSON
-- (`chorus_core::schedule::Schedule`); `next_run_at` is the next due time in Unix milliseconds.
CREATE TABLE automations (
  id          INTEGER PRIMARY KEY,
  name        TEXT    NOT NULL UNIQUE,
  bot_id      INTEGER REFERENCES bots (id) ON DELETE SET NULL,
  prompt      TEXT    NOT NULL,
  schedule    TEXT    NOT NULL,
  cwd         TEXT    NOT NULL,
  project_id  INTEGER REFERENCES projects (id) ON DELETE SET NULL,
  enabled     INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
  next_run_at INTEGER,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
) STRICT;

-- Each firing, including the ones that didn't start a session and why (decision 3).
CREATE TABLE automation_runs (
  id            INTEGER PRIMARY KEY,
  automation_id INTEGER NOT NULL REFERENCES automations (id) ON DELETE CASCADE,
  session_id    INTEGER REFERENCES sessions (id) ON DELETE SET NULL,
  trigger       TEXT    NOT NULL CHECK (trigger IN ('schedule', 'manual')),
  due_at        INTEGER NOT NULL,
  started_at    INTEGER,
  outcome       TEXT    NOT NULL CHECK (outcome IN ('started', 'failed', 'missed', 'skipped')),
  note          TEXT
) STRICT;

CREATE INDEX automation_runs_by_automation ON automation_runs (automation_id, id);

-- Plans, specs and reports sessions wrote (`chorus_core::documents`). A document inherits its
-- session's project. The path is absolute and unique.
CREATE TABLE documents (
  id            INTEGER PRIMARY KEY,
  session_id    INTEGER REFERENCES sessions (id) ON DELETE SET NULL,
  project_id    INTEGER REFERENCES projects (id) ON DELETE SET NULL,
  path          TEXT    NOT NULL UNIQUE,
  kind          TEXT    NOT NULL,
  title         TEXT,
  first_seen_at INTEGER NOT NULL,
  last_seen_at  INTEGER NOT NULL,
  missing_at    INTEGER
) STRICT;
