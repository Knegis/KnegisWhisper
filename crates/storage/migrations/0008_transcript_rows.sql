-- KNEG-406: a search hit in a meeting transcript opens at the row where the
-- words were said. notes_fts indexes a transcript as one column, so a hit
-- knows its note and not the row. This indexes each transcript segment on
-- its own.
--
-- The rowid packs the row in: note id * 2^20 + the segment's position in the
-- stored array, which is json_each's key and TranscriptRow.index. A note's
-- rows are then one rowid range, which FTS5 reads as a range, so a delete or
-- a rewrite touches only that note. Searched with notes_fts's tokenizer
-- (0007), so a row is found by exactly the words its note is.
--
-- As notes_fts's transcript column has it: only segments that are objects
-- with text, and the nested CASE because json_each raises on malformed JSON.
-- A position past 2^20, or a note id whose rowid would overflow (SQLite makes
-- the product a REAL), is left out rather than failing the note's insert.
-- Only a rewrite of the transcript re-indexes it: autosave and summaries
-- update notes all the time and must not rewrite its rows.
CREATE VIRTUAL TABLE note_segments_fts USING fts5(text, tokenize = 'unicode61 remove_diacritics 0');

INSERT INTO note_segments_fts(rowid, text)
SELECT notes.id * 1048576 + segment.key, json_extract(segment.value, '$.text')
FROM notes, json_each(CASE WHEN json_valid(notes.transcript) THEN CASE WHEN json_type(notes.transcript) = 'array' THEN notes.transcript END END) AS segment
WHERE segment.type = 'object' AND json_type(segment.value, '$.text') = 'text'
  AND segment.key < 1048576 AND notes.id BETWEEN 1 AND 8796093022207;

CREATE TRIGGER note_segments_fts_insert AFTER INSERT ON notes BEGIN
    INSERT INTO note_segments_fts(rowid, text)
    SELECT new.id * 1048576 + segment.key, json_extract(segment.value, '$.text')
    FROM json_each(CASE WHEN json_valid(new.transcript) THEN CASE WHEN json_type(new.transcript) = 'array' THEN new.transcript END END) AS segment
    WHERE segment.type = 'object' AND json_type(segment.value, '$.text') = 'text'
      AND segment.key < 1048576 AND new.id BETWEEN 1 AND 8796093022207;
END;

CREATE TRIGGER note_segments_fts_update AFTER UPDATE OF id, transcript ON notes BEGIN
    DELETE FROM note_segments_fts
    WHERE old.id BETWEEN 1 AND 8796093022207
      AND rowid BETWEEN old.id * 1048576 AND old.id * 1048576 + 1048575;
    INSERT INTO note_segments_fts(rowid, text)
    SELECT new.id * 1048576 + segment.key, json_extract(segment.value, '$.text')
    FROM json_each(CASE WHEN json_valid(new.transcript) THEN CASE WHEN json_type(new.transcript) = 'array' THEN new.transcript END END) AS segment
    WHERE segment.type = 'object' AND json_type(segment.value, '$.text') = 'text'
      AND segment.key < 1048576 AND new.id BETWEEN 1 AND 8796093022207;
END;

CREATE TRIGGER note_segments_fts_delete AFTER DELETE ON notes BEGIN
    DELETE FROM note_segments_fts
    WHERE old.id BETWEEN 1 AND 8796093022207
      AND rowid BETWEEN old.id * 1048576 AND old.id * 1048576 + 1048575;
END;
