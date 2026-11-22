-- Speaker labels run after a meeting's note is saved, never before it.
CREATE TABLE note_speaker_jobs (
    note_id INTEGER PRIMARY KEY REFERENCES notes(id) ON DELETE CASCADE,
    generation INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL CHECK(status IN ('queued','processing','succeeded','failed','interrupted','dismissed')),
    -- Whether known voices may be matched against the result.
    recognize INTEGER NOT NULL DEFAULT 0,
    -- The meeting asked for its recording to be deleted once saved. The job
    -- reads the recording, so the deletion waits until it succeeds or is dismissed.
    delete_recording INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX note_speaker_pending ON note_speaker_jobs(status, created_at);
