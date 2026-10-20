-- P3: where each run's full transcript lives (Claude Code's JSONL, Codex's rollout), as the agent's
-- hooks report it. Chorus keeps the path, not a copy (D7).
ALTER TABLE runs ADD COLUMN transcript_path TEXT;
