-- KnegisWhisper initial schema.
--
-- This is the *flattened* final shape of the upstream OpenWhispr schema, not a
-- replay of its history. Upstream has no versioned migration runner: it rebuilds
-- the schema idempotently on every boot from `CREATE TABLE IF NOT EXISTS` plus
-- roughly seventy `ALTER TABLE ... ADD COLUMN` blocks wrapped in
-- catch-duplicate-column handlers, which makes the on-disk schema version
-- unknowable from the database itself. There is no reason to reproduce seventy
-- incremental steps for a schema no KnegisWhisper user has ever had.
--
-- Three classes of change from upstream, each deliberate:
--
--   1. Every cloud and sync column is gone -- cloud_id, sync_status,
--      deleted_at, client_*_id, space_id, left_team, is_shared, share_token,
--      owner_user_id, updated_by_user_id, cloud_updated_at -- along with the
--      spaces, pending_vector_purges, and optimistic_folder_delete_rows tables
--      and the UUID-generating triggers that fed them. They exist only to serve
--      a hosted sync service this application does not have.
--
--   2. OAuth tokens are not stored here. Upstream keeps
--      google_calendar_tokens.refresh_token and its Microsoft equivalent as
--      plaintext SQLite columns. This schema stores a token_ref -- the name of a
--      slot in the OS credential store -- and nothing else. See
--      assert_no_credential_columns, which fails if a credential column
--      reappears.
--
--   3. The settings table is new. Upstream keeps settings in electron-store,
--      which has no equivalent here.

-- ---------------------------------------------------------------- dictation

CREATE TABLE transcriptions (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    text              TEXT NOT NULL,
    raw_text          TEXT,
    timestamp         DATETIME DEFAULT CURRENT_TIMESTAMP,
    created_at        DATETIME DEFAULT CURRENT_TIMESTAMP,
    has_audio         INTEGER NOT NULL DEFAULT 0,
    audio_duration_ms INTEGER,
    provider          TEXT,
    model             TEXT,
    status            TEXT NOT NULL DEFAULT 'completed',
    error_message     TEXT,
    error_code        TEXT,
    -- Records the dictation intent (e.g. "translation") so a retry re-runs the
    -- same route rather than whatever the current default happens to be.
    route_kind        TEXT
);

CREATE INDEX idx_transcriptions_created_at ON transcriptions(created_at DESC);

CREATE TABLE custom_dictionary (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    word       TEXT NOT NULL UNIQUE,
    source     TEXT NOT NULL DEFAULT 'manual',
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE snippets (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    trigger     TEXT NOT NULL,
    replacement TEXT NOT NULL,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Upstream indexes this as unique on lower(trigger) among non-deleted rows.
-- With no soft deletes the predicate is unnecessary.
CREATE UNIQUE INDEX idx_snippets_trigger_lower ON snippets(lower(trigger));

-- ------------------------------------------------------------------- notes

CREATE TABLE folders (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL UNIQUE,
    is_default INTEGER NOT NULL DEFAULT 0,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_folders_sort ON folders(sort_order);

CREATE TABLE notes (
    id                       INTEGER PRIMARY KEY AUTOINCREMENT,
    title                    TEXT NOT NULL DEFAULT 'Untitled Note',
    content                  TEXT NOT NULL DEFAULT '',
    note_type                TEXT NOT NULL DEFAULT 'personal',
    source_file              TEXT,
    audio_duration_seconds   REAL,
    enhanced_content         TEXT,
    enhancement_prompt       TEXT,
    enhanced_at_content_hash TEXT,
    folder_id                INTEGER REFERENCES folders(id) ON DELETE SET NULL,
    transcript               TEXT,
    calendar_event_id        TEXT,
    participants             TEXT,
    diarization_enabled      INTEGER,
    expected_speaker_count   INTEGER,
    created_at               DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at               DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_notes_folder_id ON notes(folder_id);
CREATE INDEX idx_notes_updated_at ON notes(updated_at DESC);

-- FTS5 external-content table mirroring notes. Ported verbatim from upstream,
-- which got this right: the index stores terms, not a second copy of the text.
CREATE VIRTUAL TABLE notes_fts USING fts5(
    title,
    content,
    enhanced_content,
    content='notes',
    content_rowid='id'
);

CREATE TRIGGER notes_fts_insert AFTER INSERT ON notes BEGIN
    INSERT INTO notes_fts(rowid, title, content, enhanced_content)
    VALUES (new.id, new.title, new.content, new.enhanced_content);
END;

CREATE TRIGGER notes_fts_update AFTER UPDATE ON notes BEGIN
    INSERT INTO notes_fts(notes_fts, rowid, title, content, enhanced_content)
    VALUES ('delete', old.id, old.title, old.content, old.enhanced_content);
    INSERT INTO notes_fts(rowid, title, content, enhanced_content)
    VALUES (new.id, new.title, new.content, new.enhanced_content);
END;

CREATE TRIGGER notes_fts_delete AFTER DELETE ON notes BEGIN
    INSERT INTO notes_fts(notes_fts, rowid, title, content, enhanced_content)
    VALUES ('delete', old.id, old.title, old.content, old.enhanced_content);
END;

CREATE TABLE actions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    prompt          TEXT NOT NULL,
    icon            TEXT NOT NULL DEFAULT 'sparkles',
    is_builtin      INTEGER NOT NULL DEFAULT 0,
    sort_order      INTEGER NOT NULL DEFAULT 0,
    translation_key TEXT,
    created_at      DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at      DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- ------------------------------------------------------------------ agent

CREATE TABLE agent_conversations (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    title       TEXT NOT NULL DEFAULT 'Untitled',
    archived_at DATETIME,
    note_id     INTEGER REFERENCES notes(id) ON DELETE SET NULL,
    folder_id   INTEGER REFERENCES folders(id) ON DELETE SET NULL,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_agent_conversations_note ON agent_conversations(note_id);

CREATE TABLE agent_messages (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id INTEGER NOT NULL REFERENCES agent_conversations(id) ON DELETE CASCADE,
    role            TEXT NOT NULL CHECK(role IN ('user', 'assistant', 'system')),
    content         TEXT NOT NULL,
    metadata        TEXT,
    created_at      DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_agent_messages_conversation ON agent_messages(conversation_id);

-- -------------------------------------------------------------- calendars
--
-- token_ref names a slot in the OS credential store; the tokens themselves are
-- never in this file. sync_token is a calendar delta cursor, not a credential.

CREATE TABLE google_calendar_tokens (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    google_email TEXT NOT NULL UNIQUE,
    token_ref    TEXT NOT NULL,
    expires_at   INTEGER NOT NULL,
    scope        TEXT NOT NULL,
    created_at   DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at   DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE google_calendars (
    id               TEXT PRIMARY KEY,
    summary          TEXT NOT NULL,
    description      TEXT,
    background_color TEXT,
    is_selected      INTEGER NOT NULL DEFAULT 1,
    is_primary       INTEGER NOT NULL DEFAULT 0,
    sync_token       TEXT,
    account_email    TEXT,
    created_at       DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE microsoft_calendar_tokens (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    microsoft_email TEXT NOT NULL UNIQUE,
    token_ref       TEXT NOT NULL,
    expires_at      INTEGER NOT NULL,
    scope           TEXT NOT NULL,
    created_at      DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at      DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE microsoft_calendars (
    id                    TEXT PRIMARY KEY,
    summary               TEXT NOT NULL,
    background_color      TEXT,
    is_selected           INTEGER NOT NULL DEFAULT 1,
    is_primary            INTEGER NOT NULL DEFAULT 0,
    sync_token            TEXT,
    sync_token_expires_at INTEGER,
    account_email         TEXT,
    created_at            DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE apple_calendars (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    color       TEXT,
    source_name TEXT,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE calendar_events (
    id              TEXT PRIMARY KEY,
    calendar_id     TEXT NOT NULL,
    provider        TEXT NOT NULL DEFAULT 'google',
    summary         TEXT,
    start_time      TEXT NOT NULL,
    end_time        TEXT NOT NULL,
    is_all_day      INTEGER NOT NULL DEFAULT 0,
    status          TEXT NOT NULL DEFAULT 'confirmed',
    hangout_link    TEXT,
    conference_data TEXT,
    organizer_email TEXT,
    attendees_count INTEGER DEFAULT 0,
    attendees       TEXT,
    synced_at       DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_calendar_events_start ON calendar_events(start_time);

CREATE TABLE contacts (
    email        TEXT PRIMARY KEY,
    display_name TEXT,
    created_at   DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at   DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- ------------------------------------------------------------- diarization

CREATE TABLE speaker_profiles (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    display_name TEXT NOT NULL,
    email        TEXT,
    embedding    BLOB NOT NULL,
    sample_count INTEGER DEFAULT 1,
    created_at   DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at   DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE speaker_mappings (
    note_id      INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    speaker_id   TEXT NOT NULL,
    profile_id   INTEGER REFERENCES speaker_profiles(id) ON DELETE SET NULL,
    display_name TEXT NOT NULL,
    PRIMARY KEY (note_id, speaker_id)
);

CREATE TABLE note_speaker_embeddings (
    note_id    INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    speaker_id TEXT NOT NULL,
    embedding  BLOB NOT NULL,
    PRIMARY KEY (note_id, speaker_id)
);

-- ---------------------------------------------------------------- settings
--
-- New. Upstream keeps these in electron-store. Values are JSON so a setting can
-- grow structure without a migration; keys are namespaced with a dot.
-- Credentials never appear here -- they live in the OS credential store.

CREATE TABLE settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
