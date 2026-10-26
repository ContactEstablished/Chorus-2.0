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
    -- What the agent said it did (its task_done call).
    summary TEXT,
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

-- Questions a step asked with Chorus's ask tool (slice 3). A blocking question waits for Matthew;
-- a timed one takes its default at deadline_at. The answer goes back to the step's agent as its
-- next prompt (delivered_at). log_only questions are logged in assumptions.md, not kept here.
CREATE TABLE loop_questions (
    id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL REFERENCES loop_runs(id) ON DELETE CASCADE,
    step_id INTEGER NOT NULL REFERENCES loop_steps(id) ON DELETE CASCADE,
    phase INTEGER NOT NULL,
    task TEXT,
    -- blocking | timed
    class TEXT NOT NULL,
    question TEXT NOT NULL,
    options TEXT NOT NULL,
    default_answer TEXT,
    asked_at INTEGER NOT NULL,
    deadline_at INTEGER,
    -- open | answered | defaulted | cancelled
    state TEXT NOT NULL,
    answer TEXT,
    made_rule INTEGER NOT NULL DEFAULT 0,
    answered_at INTEGER,
    delivered_at INTEGER
);
CREATE INDEX loop_questions_run ON loop_questions(run_id, state);

-- The Telegram relay (slice 4, HANDOFF-P13's design). One bot, long polling only, DM only.
-- The token is a credential (DPAPI); nothing here holds it.
CREATE TABLE relay_config (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    credential_id INTEGER REFERENCES credentials(id) ON DELETE SET NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    bot_name TEXT,
    poll_offset INTEGER NOT NULL DEFAULT 0
);
INSERT INTO relay_config (id) VALUES (1);

-- Paired phones: Telegram users whose private chat with the bot is heard.
CREATE TABLE relay_users (
    telegram_id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    paired_at INTEGER NOT NULL
);

-- Messages waiting to go, rendered and scrubbed. kind: question | settled | stopped | milestone |
-- phase-done | done | reply | test. A settled question edits the messages its question was sent as.
CREATE TABLE relay_outbox (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL,
    run_id INTEGER,
    question_id INTEGER,
    chat_id INTEGER,
    reply_to INTEGER,
    html TEXT NOT NULL,
    silent INTEGER NOT NULL DEFAULT 0,
    buttons TEXT NOT NULL DEFAULT '[]',
    created_at INTEGER NOT NULL,
    sent_at INTEGER,
    attempts INTEGER NOT NULL DEFAULT 0,
    error TEXT
);

-- What each message the bot sent was about, so a reply finds its question.
CREATE TABLE relay_messages (
    chat_id INTEGER NOT NULL,
    message_id INTEGER NOT NULL,
    kind TEXT NOT NULL,
    run_id INTEGER,
    question_id INTEGER,
    sent_at INTEGER NOT NULL,
    PRIMARY KEY (chat_id, message_id)
);

-- A button's token and what it does; text never goes in callback_data.
CREATE TABLE relay_buttons (
    token TEXT PRIMARY KEY,
    act TEXT NOT NULL,
    run_id INTEGER,
    question_id INTEGER,
    created_at INTEGER NOT NULL,
    used_at INTEGER
);

-- What arrived and what was done about it. Never the text.
CREATE TABLE relay_audit (
    id INTEGER PRIMARY KEY,
    at INTEGER NOT NULL,
    kind TEXT NOT NULL,
    telegram_id INTEGER,
    note TEXT
);
