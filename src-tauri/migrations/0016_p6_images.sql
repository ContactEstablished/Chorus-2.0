CREATE TABLE composer_images (
    id INTEGER PRIMARY KEY,
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    queue_id INTEGER REFERENCES prompt_queue(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    png BLOB NOT NULL
);
CREATE INDEX composer_images_owner ON composer_images(session_id, queue_id);
