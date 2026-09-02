//! A note's recording: the application-owned audio file a note was made from.
//!
//! The relation lives in its own table rather than as timestamps inside the
//! note's Markdown, because the note is edited freely -- paragraphs split,
//! merge and disappear -- and a link carried inside the text would break with
//! the first ordinary edit. The transcript segments (`notes.transcript`) carry
//! the timing; this table says which file that timing refers to.
//!
//! `storage_key` is a bare file name inside the application's recordings
//! directory, never a path. The desktop application resolves it inside that
//! directory and refuses anything else, so a row here cannot name a file
//! outside it -- see `apps/desktop/src-tauri/src/recordings.rs`.

use serde::Serialize;
use sqlx::FromRow;
use ts_rs::TS;

/// A recording attached to a note, as stored.
#[derive(Debug, Clone, FromRow, Serialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct NoteRecording {
    pub note_id: i64,
    /// The file name inside the application's recordings directory. Never a
    /// path, and never resolved anywhere but inside that directory.
    pub storage_key: String,
    pub duration_seconds: Option<f64>,
    /// What the file was called before it became application-owned, where
    /// there was such a name. A meeting recording has none.
    pub original_filename: Option<String>,
    pub created_at: Option<String>,
}
