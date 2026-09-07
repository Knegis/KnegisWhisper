CREATE VIRTUAL TABLE transcriptions_fts USING fts5(text, content='transcriptions', content_rowid='id');
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

CREATE TABLE import_jobs (
  id TEXT PRIMARY KEY,
  stage TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  provider TEXT,
  model TEXT,
  note_id INTEGER REFERENCES notes(id) ON DELETE SET NULL,
  error TEXT
);
