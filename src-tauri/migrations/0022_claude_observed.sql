-- What a run's newest turn really ran with (JSON: model, effort), read from Claude Code's transcript
-- and kept after the run ends, so a session Chorus closed still says which model answered.
ALTER TABLE runs ADD COLUMN observed TEXT;
