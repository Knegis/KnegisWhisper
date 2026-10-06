-- KNEG-405: å, ä and ö are letters of their own in Swedish. FTS5's default
-- tokenizer, unicode61, removes diacritics, so "for" found "för", "har" found
-- "här" and "sa" found "så". Both search indexes are rebuilt and refilled with
-- remove_diacritics 0. Letter case still folds ("här" finds "HÄR"); the cost is
-- that every accent now counts, so "cafe" no longer finds "café".

-- Notes, as 0006 left them, with the tokenizer added.
DROP TRIGGER notes_fts_insert;
DROP TRIGGER notes_fts_update;
DROP TRIGGER notes_fts_delete;
DROP TABLE notes_fts;
CREATE VIRTUAL TABLE notes_fts USING fts5(title, content, enhanced_content, transcript_text, tokenize = 'unicode61 remove_diacritics 0');
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

-- Dictation history, as 0003 left it, with the tokenizer added.
DROP TRIGGER transcriptions_fts_insert;
DROP TRIGGER transcriptions_fts_update;
DROP TRIGGER transcriptions_fts_delete;
DROP TABLE transcriptions_fts;
CREATE VIRTUAL TABLE transcriptions_fts USING fts5(text, content='transcriptions', content_rowid='id', tokenize = 'unicode61 remove_diacritics 0');
CREATE TRIGGER transcriptions_fts_insert AFTER INSERT ON transcriptions BEGIN
  INSERT INTO transcriptions_fts(rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER transcriptions_fts_update AFTER UPDATE OF text ON transcriptions BEGIN
  INSERT INTO transcriptions_fts(transcriptions_fts, rowid, text) VALUES ('delete', old.id, old.text);
  INSERT INTO transcriptions_fts(rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER transcriptions_fts_delete AFTER DELETE ON transcriptions BEGIN
  INSERT INTO transcriptions_fts(transcriptions_fts, rowid, text) VALUES ('delete', old.id, old.text);
END;
INSERT INTO transcriptions_fts(transcriptions_fts) VALUES ('rebuild');
