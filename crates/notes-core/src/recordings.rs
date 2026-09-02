//! A note's recording: the application-owned audio file a note was made from.
//!
//! The relation lives in its own table rather than as timestamps inside the
//! note's Markdown, because the note is edited freely -- paragraphs split,
//! merge and disappear -- and a link carried inside the text would break with
//! the first ordinary edit. The transcript segments (`notes.transcript`) carry
//! the timing; this table says which file that timing refers to.
//!
//! `storage_key` is a bare file name inside the application's recordings
//! directory, never a path. This crate stores the value it is given; the
//! desktop application is the only writer and validates it before writing, then
//! resolves it inside that directory -- see
//! `apps/desktop/src-tauri/src/recordings.rs`.

use kw_storage::StorageError;
use serde::Serialize;
use sqlx::{FromRow, SqlitePool};
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

/// Attach one application-owned recording to a note.
///
/// This is deliberately a plain insert: neither an existing attachment nor a
/// file already owned by another note may be replaced silently.
pub async fn attach(
    pool: &SqlitePool,
    note_id: i64,
    storage_key: &str,
    duration_seconds: Option<f64>,
    original_filename: Option<&str>,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO note_recordings
            (note_id, storage_key, duration_seconds, original_filename)
         VALUES (?, ?, ?, ?)",
    )
    .bind(note_id)
    .bind(storage_key)
    .bind(duration_seconds)
    .bind(original_filename)
    .execute(pool)
    .await?;
    Ok(())
}

/// The recording attached to `note_id`, if it has one.
pub async fn get(pool: &SqlitePool, note_id: i64) -> Result<Option<NoteRecording>, StorageError> {
    Ok(sqlx::query_as::<_, NoteRecording>(
        "SELECT note_id, storage_key, duration_seconds, original_filename, created_at
         FROM note_recordings WHERE note_id = ?",
    )
    .bind(note_id)
    .fetch_optional(pool)
    .await?)
}

/// Detach a recording and return the row that named its file.
pub async fn detach(
    pool: &SqlitePool,
    note_id: i64,
) -> Result<Option<NoteRecording>, StorageError> {
    Ok(sqlx::query_as::<_, NoteRecording>(
        "DELETE FROM note_recordings WHERE note_id = ?
         RETURNING note_id, storage_key, duration_seconds, original_filename, created_at",
    )
    .bind(note_id)
    .fetch_optional(pool)
    .await?)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::notes::{self, NewNote};
    use kw_storage::Db;

    async fn db() -> Db {
        Db::open_in_memory().await.unwrap()
    }

    async fn note(pool: &SqlitePool, title: &str) -> i64 {
        notes::create(
            pool,
            &NewNote {
                title: title.to_owned(),
                content: String::new(),
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn attach_and_get_preserve_every_field() {
        let db = db().await;
        let id = note(db.pool(), "Recorded").await;

        attach(
            db.pool(),
            id,
            "20260902-141530.wav",
            Some(42.5),
            Some("meeting.wav"),
        )
        .await
        .unwrap();

        let recording = get(db.pool(), id).await.unwrap().unwrap();
        assert_eq!(recording.note_id, id);
        assert_eq!(recording.storage_key, "20260902-141530.wav");
        assert_eq!(recording.duration_seconds, Some(42.5));
        assert_eq!(recording.original_filename.as_deref(), Some("meeting.wav"));
        assert!(recording.created_at.is_some());
    }

    #[tokio::test]
    async fn attaching_twice_to_one_note_is_an_error() {
        let db = db().await;
        let id = note(db.pool(), "Recorded").await;
        attach(db.pool(), id, "one.wav", None, None).await.unwrap();

        assert!(attach(db.pool(), id, "two.wav", None, None).await.is_err());
    }

    #[tokio::test]
    async fn one_storage_key_cannot_belong_to_two_notes() {
        let db = db().await;
        let first = note(db.pool(), "First").await;
        let second = note(db.pool(), "Second").await;
        attach(db.pool(), first, "shared.wav", None, None)
            .await
            .unwrap();

        assert!(attach(db.pool(), second, "shared.wav", None, None)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn deleting_a_note_cascades_its_recording_row() {
        let db = db().await;
        let id = note(db.pool(), "Recorded").await;
        attach(db.pool(), id, "cascade.wav", None, None)
            .await
            .unwrap();

        assert!(notes::delete(db.pool(), id).await.unwrap());
        assert_eq!(get(db.pool(), id).await.unwrap(), None);
    }

    #[tokio::test]
    async fn detach_returns_the_row_once() {
        let db = db().await;
        let id = note(db.pool(), "Recorded").await;
        attach(db.pool(), id, "detached.wav", Some(3.0), None)
            .await
            .unwrap();

        let recording = detach(db.pool(), id).await.unwrap().unwrap();
        assert_eq!(recording.storage_key, "detached.wav");
        assert_eq!(detach(db.pool(), id).await.unwrap(), None);
    }

    #[tokio::test]
    async fn get_without_a_recording_returns_none() {
        let db = db().await;
        let id = note(db.pool(), "Typed").await;

        assert_eq!(get(db.pool(), id).await.unwrap(), None);
    }
}
