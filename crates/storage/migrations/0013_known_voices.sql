-- Legacy/imported embeddings intentionally have no model identity.
ALTER TABLE speaker_profiles ADD COLUMN model_id TEXT;
ALTER TABLE speaker_profiles ADD COLUMN embedding_format TEXT;
ALTER TABLE speaker_profiles ADD COLUMN speech_seconds REAL;
ALTER TABLE note_speaker_embeddings ADD COLUMN model_id TEXT;
ALTER TABLE note_speaker_embeddings ADD COLUMN embedding_format TEXT;
ALTER TABLE note_speaker_embeddings ADD COLUMN speech_seconds REAL;
ALTER TABLE note_speaker_embeddings ADD COLUMN enrolled_profile_id INTEGER REFERENCES speaker_profiles(id) ON DELETE SET NULL;
ALTER TABLE speaker_mappings ADD COLUMN origin TEXT NOT NULL DEFAULT 'manual';
CREATE TABLE speaker_manual_overrides (
    note_id INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    speaker_id TEXT NOT NULL,
    PRIMARY KEY (note_id, speaker_id)
);
