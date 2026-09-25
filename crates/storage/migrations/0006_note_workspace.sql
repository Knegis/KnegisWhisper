-- The search index owns the derived transcript text, including legacy plain text.
ALTER TABLE notes ADD COLUMN recording_details TEXT;
DROP TRIGGER notes_fts_insert;
DROP TRIGGER notes_fts_update;
DROP TRIGGER notes_fts_delete;
DROP TABLE notes_fts;
CREATE VIRTUAL TABLE notes_fts USING fts5(title, content, enhanced_content, transcript_text);
INSERT INTO notes_fts(rowid, title, content, enhanced_content, transcript_text)
SELECT id, title, content, enhanced_content, CASE WHEN json_valid(transcript) THEN CASE WHEN json_type(transcript) = 'array' THEN COALESCE((SELECT group_concat(json_extract(value, '$.text'), char(10)) FROM json_each(transcript) WHERE type = 'object' AND json_type(value, '$.text') = 'text'), '') ELSE transcript END ELSE COALESCE(transcript, '') END FROM notes;
CREATE TRIGGER notes_fts_insert AFTER INSERT ON notes BEGIN
    INSERT INTO notes_fts(rowid, title, content, enhanced_content, transcript_text)
    VALUES (new.id, new.title, new.content, new.enhanced_content, CASE WHEN json_valid(new.transcript) THEN CASE WHEN json_type(new.transcript) = 'array' THEN COALESCE((SELECT group_concat(json_extract(value, '$.text'), char(10)) FROM json_each(new.transcript) WHERE type = 'object' AND json_type(value, '$.text') = 'text'), '') ELSE new.transcript END ELSE COALESCE(new.transcript, '') END);
END;
CREATE TRIGGER notes_fts_update AFTER UPDATE ON notes BEGIN
    DELETE FROM notes_fts WHERE rowid = old.id;
    INSERT INTO notes_fts(rowid, title, content, enhanced_content, transcript_text)
    VALUES (new.id, new.title, new.content, new.enhanced_content, CASE WHEN json_valid(new.transcript) THEN CASE WHEN json_type(new.transcript) = 'array' THEN COALESCE((SELECT group_concat(json_extract(value, '$.text'), char(10)) FROM json_each(new.transcript) WHERE type = 'object' AND json_type(value, '$.text') = 'text'), '') ELSE new.transcript END ELSE COALESCE(new.transcript, '') END);
END;
CREATE TRIGGER notes_fts_delete AFTER DELETE ON notes BEGIN
    DELETE FROM notes_fts WHERE rowid = old.id;
END;
