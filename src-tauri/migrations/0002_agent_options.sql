-- P2: what each session asked its agent for, and the agent's own conversation id for resume.

-- The resolved launch options (`chorus_core::agent::LaunchOptions`) as JSON. Model names and enum
-- values only: nothing secret.
ALTER TABLE sessions ADD COLUMN options TEXT NOT NULL DEFAULT '{}';
-- The agent's conversation id (a UUID): chosen by Chorus at create for Claude Code and Grok, found
-- in the agent's files for Codex, or the server session for opencode.
ALTER TABLE sessions ADD COLUMN agent_session_id TEXT;

-- The CLI version detected when the run started, and whether it continued the conversation.
ALTER TABLE runs ADD COLUMN cli_version TEXT;
ALTER TABLE runs ADD COLUMN resumed INTEGER NOT NULL DEFAULT 0 CHECK (resumed IN (0, 1));
