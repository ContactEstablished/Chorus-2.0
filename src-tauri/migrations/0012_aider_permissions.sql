-- Q29: correct Aider's label without changing the arguments it runs with. Other agents and
-- Full access keep their choices. Saved profiles must remain usable after Manual is removed.
UPDATE sessions SET options = json_set(options, '$.permission', 'accept-edits')
WHERE agent = 'aider' AND json_valid(options) AND json_extract(options, '$.permission') = 'manual';

UPDATE launch_profiles SET options = json_set(options, '$.permission', 'accept-edits')
WHERE agent = 'aider' AND json_valid(options) AND json_extract(options, '$.permission') = 'manual';
