-- P15: the autonomous phase loop's machine state (D97, D98). The documents agents read live in the
-- repository under .chorus/run/ on the run branch; what is here is what the orchestrator decides
-- from, and what the Loop screen shows.

-- One run of a roadmap. `config` (JSON) holds its roles, checks and limits, which only Matthew
-- edits in Chorus: an agent can change files in the repository, so nothing it runs is read from there.
CREATE TABLE loop_runs (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    repo_root TEXT NOT NULL,
    base_branch TEXT NOT NULL,
    run_branch TEXT NOT NULL,
    worktree_id INTEGER REFERENCES worktrees(id) ON DELETE SET NULL,
    worktree_path TEXT,
    config TEXT NOT NULL,
    -- draft | running | paused | waiting | stopped | done
    state TEXT NOT NULL,
    note TEXT,
    -- The last phase Matthew approved at a milestone review.
    reviewed_phase INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

-- A phase of a run. `progress` is chorus_core::run_loop::PhaseRun as JSON.
CREATE TABLE loop_phases (
    id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL REFERENCES loop_runs(id) ON DELETE CASCADE,
    number INTEGER NOT NULL,
    title TEXT NOT NULL,
    folder TEXT NOT NULL,
    progress TEXT NOT NULL,
    -- planning | building | checking | tagging | passed | stopped
    state TEXT NOT NULL,
    stop TEXT,
    tag TEXT,
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    UNIQUE (run_id, number)
);

-- One step: a planner or builder session, a gate run, or the tag. Records the model asked for and
-- the one that actually answered (L61).
CREATE TABLE loop_steps (
    id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL REFERENCES loop_runs(id) ON DELETE CASCADE,
    phase_id INTEGER NOT NULL REFERENCES loop_phases(id) ON DELETE CASCADE,
    -- plan | build | check | tag
    kind TEXT NOT NULL,
    task TEXT,
    attempt INTEGER NOT NULL DEFAULT 0,
    role TEXT,
    bot_id INTEGER REFERENCES bots(id) ON DELETE SET NULL,
    session_id INTEGER REFERENCES sessions(id) ON DELETE SET NULL,
    -- The session's run that took the step's prompt; a relaunched run is a different one.
    session_run_id INTEGER,
    model_asked TEXT,
    model_observed TEXT,
    -- running | done | failed
    state TEXT NOT NULL,
    note TEXT,
    started_at INTEGER NOT NULL,
    ended_at INTEGER
);
CREATE INDEX loop_steps_run ON loop_steps(run_id, id);

-- Each check a gate step ran. `output_tail` is scrubbed.
CREATE TABLE loop_checks (
    id INTEGER PRIMARY KEY,
    step_id INTEGER NOT NULL REFERENCES loop_steps(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    command TEXT NOT NULL,
    passed INTEGER NOT NULL,
    exit_code INTEGER,
    timed_out INTEGER NOT NULL DEFAULT 0,
    duration_ms INTEGER NOT NULL,
    output_tail TEXT NOT NULL,
    signature TEXT
);
