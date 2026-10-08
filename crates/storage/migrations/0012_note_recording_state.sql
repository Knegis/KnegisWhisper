-- Local recording lifecycle. Never inferred from editable note text.
CREATE TABLE note_recording_state (
    note_id INTEGER PRIMARY KEY REFERENCES notes(id) ON DELETE CASCADE,
    storage_key TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('delete-pending','delete-failed','deleted','kept-incomplete','kept-untranscribed')),
    last_error TEXT
);
-- A new attachment must never inherit an older recording's deletion intent.
CREATE TRIGGER clear_previous_recording_state AFTER INSERT ON note_recordings
BEGIN
    DELETE FROM note_recording_state WHERE note_id = NEW.note_id;
END;
