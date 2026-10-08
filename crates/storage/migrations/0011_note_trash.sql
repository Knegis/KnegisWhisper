-- KNEG-401: a deleted note goes to Trash and stays until emptied.
-- Not `deleted_at`: that is OpenWhispr's sync tombstone, refused by
-- no_sync_or_cloud_columns_survive_anywhere and filtered by the importer.
ALTER TABLE notes ADD COLUMN trashed_at TEXT;
CREATE INDEX idx_notes_trashed ON notes(trashed_at) WHERE trashed_at IS NOT NULL;
