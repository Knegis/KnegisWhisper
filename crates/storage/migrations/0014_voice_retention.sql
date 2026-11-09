-- Meeting embeddings were never an enrollment decision. Remove every cache.
DELETE FROM note_speaker_embeddings;
CREATE TRIGGER reject_meeting_embeddings BEFORE INSERT ON note_speaker_embeddings
BEGIN SELECT RAISE(ABORT, 'Meeting voice embeddings must not be persisted'); END;
CREATE TABLE known_voice_state (singleton INTEGER PRIMARY KEY CHECK(singleton=1), generation INTEGER NOT NULL);
INSERT INTO known_voice_state VALUES(1,0);
CREATE TRIGGER invalidate_voice_jobs_on_disable AFTER UPDATE ON settings WHEN NEW.key='meetings.knownVoices'
BEGIN UPDATE known_voice_state SET generation=generation+1 WHERE singleton=1; END;
CREATE TRIGGER invalidate_voice_jobs_on_insert AFTER INSERT ON settings WHEN NEW.key='meetings.knownVoices'
BEGIN UPDATE known_voice_state SET generation=generation+1 WHERE singleton=1; END;
CREATE TRIGGER invalidate_voice_jobs_on_forget AFTER DELETE ON speaker_profiles
BEGIN UPDATE known_voice_state SET generation=generation+1 WHERE singleton=1; END;
CREATE TRIGGER invalidate_voice_jobs_on_correction AFTER INSERT ON speaker_manual_overrides
BEGIN UPDATE known_voice_state SET generation=generation+1 WHERE singleton=1; END;
CREATE TRIGGER invalidate_voice_jobs_on_settings_delete AFTER DELETE ON settings WHEN OLD.key='meetings.knownVoices'
BEGIN UPDATE known_voice_state SET generation=generation+1 WHERE singleton=1; END;
CREATE TRIGGER invalidate_voice_jobs_on_name_update AFTER UPDATE ON speaker_mappings
BEGIN UPDATE known_voice_state SET generation=generation+1 WHERE singleton=1; END;
CREATE TRIGGER invalidate_voice_jobs_on_name_delete AFTER DELETE ON speaker_mappings
BEGIN UPDATE known_voice_state SET generation=generation+1 WHERE singleton=1; END;
CREATE TABLE note_voice_sources (note_id INTEGER PRIMARY KEY REFERENCES notes(id) ON DELETE CASCADE, source_json TEXT NOT NULL);
CREATE TABLE known_voice_enrollments (
 note_id INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
 speaker_id TEXT NOT NULL,
 profile_id INTEGER REFERENCES speaker_profiles(id) ON DELETE SET NULL,
 PRIMARY KEY(note_id,speaker_id)
);
