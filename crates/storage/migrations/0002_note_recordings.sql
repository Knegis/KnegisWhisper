-- A note's recording, kept as an application-owned attachment.
--
-- One row per note, keyed by note_id, so a note has at most one recording and
-- the row disappears with the note. `storage_key` is the bare file name inside
-- the application's recordings directory (`kw_storage::paths::data_dir()` +
-- `meetings`), never a path: the desktop application resolves it inside that
-- directory and refuses anything that is not a plain file name, so nothing
-- stored here can name a file outside it. Timestamps are not embedded in the
-- note's Markdown for the same reason this table exists -- editing the note
-- must not be able to break the link to the audio.
--
-- No AUTOINCREMENT, deliberately: the primary key is the note's own id, and
-- `storage::import::tests::must_be_empty_is_exactly_the_autoincrement_tables`
-- pins the rule that only tables with a surrogate id of their own must be
-- empty before an OpenWhispr import. This table is not imported at all.

CREATE TABLE note_recordings (
    note_id           INTEGER PRIMARY KEY REFERENCES notes(id) ON DELETE CASCADE,
    storage_key       TEXT NOT NULL UNIQUE,
    duration_seconds  REAL,
    original_filename TEXT,
    created_at        DATETIME DEFAULT CURRENT_TIMESTAMP
);
