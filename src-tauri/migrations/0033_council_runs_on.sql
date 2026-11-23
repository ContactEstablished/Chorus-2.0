-- D140: where each council member's turns run. 'openrouter' through the run's capped key;
-- 'claude' and 'codex' through the local CLI on its own subscription sign-in.
ALTER TABLE council_members ADD COLUMN runs_on TEXT NOT NULL DEFAULT 'openrouter'
  CHECK (runs_on IN ('openrouter', 'claude', 'codex'));
