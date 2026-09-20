CREATE TABLE note_summary_meta (
    note_id INTEGER PRIMARY KEY REFERENCES notes(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL DEFAULT 0,
    edited INTEGER NOT NULL DEFAULT 0,
    source_hash TEXT,
    source_transcript TEXT,
    source_participants TEXT,
    references_json TEXT NOT NULL DEFAULT '[]',
    provider TEXT,
    model TEXT,
    language TEXT,
    prompt_version INTEGER,
    generated_at TEXT
);
CREATE TABLE note_summary_jobs (
    note_id INTEGER PRIMARY KEY REFERENCES notes(id) ON DELETE CASCADE,
    generation INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL CHECK(status IN ('queued','processing','succeeded','failed','interrupted','cancelled')),
    automatic INTEGER NOT NULL DEFAULT 0,
    settings_json TEXT NOT NULL,
    source_transcript TEXT NOT NULL,
    source_participants TEXT,
    expected_revision INTEGER NOT NULL,
    completed INTEGER NOT NULL DEFAULT 0,
    total INTEGER NOT NULL DEFAULT 0,
    chunks_json TEXT NOT NULL DEFAULT '{}',
    error TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX note_summary_pending ON note_summary_jobs(status, created_at);
