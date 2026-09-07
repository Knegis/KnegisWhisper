//! A note's recording: the application-owned audio file a note was made from.
//!
//! The relation lives in its own table rather than as timestamps inside the
//! note's Markdown, because the note is edited freely -- paragraphs split,
//! merge and disappear -- and a link carried inside the text would break with
//! the first ordinary edit. The transcript segments (`notes.transcript`) carry
//! the timing; this table says which file that timing refers to.
//!
//! `storage_key` is a bare file name inside the application's recordings
//! directory, never a path. File resolution and deletion live here so desktop
//! playback and both note-deletion surfaces apply the same storage rules.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use kw_storage::StorageError;
use serde::Serialize;
use sqlx::{FromRow, SqlitePool};
use ts_rs::TS;

/// The production directory; tests supply their own directory to file operations.
pub fn recording_dir() -> PathBuf {
    kw_storage::paths::data_dir().join("meetings")
}

/// One bare filename on both Unix and Windows, including no alternate streams.
pub fn is_valid_storage_key(key: &str) -> bool {
    !key.is_empty()
        && key != "."
        && key != ".."
        && key.trim() == key
        && !key.ends_with('.')
        && !key.contains(['/', '\\', '\0', ':'])
        && Path::new(key).file_name() == Some(OsStr::new(key))
}

/// Resolve only a bare storage key inside a trusted, application-owned directory.
pub fn resolve(directory: &Path, key: &str) -> Option<PathBuf> {
    is_valid_storage_key(key).then(|| directory.join(key))
}

/// Derive the key from a path built by the application's recorder.
pub fn storage_key_for(path: &Path) -> Option<String> {
    let key = path.file_name()?.to_str()?;
    is_valid_storage_key(key).then(|| key.to_owned())
}

/// Best-effort cleanup: deletion of the database row has already committed.
pub async fn remove_file(directory: &Path, key: &str) {
    let Some(path) = resolve(directory, key) else {
        tracing::warn!(
            storage_key = key,
            "refused to remove an invalid recording key"
        );
        return;
    };
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            tracing::warn!(storage_key = key, error = %error, "could not remove a deleted note's recording");
        }
    }
}

/// Delete a note and then its recording. The first DELETE acquires SQLite's
/// write lock, so an attachment cannot slip between its capture and note removal.
/// A failed transaction leaves both rows and the file intact. Filesystem failure
/// after commit is logged and deliberately does not change the deletion result.
pub async fn delete_with_recording(
    pool: &SqlitePool,
    id: i64,
    recording_directory: &Path,
) -> Result<bool, StorageError> {
    let mut tx = pool.begin().await?;
    let key: Option<String> =
        sqlx::query_scalar("DELETE FROM note_recordings WHERE note_id = ? RETURNING storage_key")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
    let removed = sqlx::query("DELETE FROM notes WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?
        .rows_affected()
        > 0;
    tx.commit().await?;
    if removed {
        if let Some(key) = key {
            remove_file(recording_directory, &key).await;
        }
    }
    Ok(removed)
}

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
    attach_and_get(
        pool,
        note_id,
        storage_key,
        duration_seconds,
        original_filename,
    )
    .await
    .map(|_| ())
}

/// Insert and return the attachment in one statement. A later read failure must
/// not make a caller discard a file whose attachment was already committed.
pub async fn attach_and_get(
    pool: &SqlitePool,
    note_id: i64,
    storage_key: &str,
    duration_seconds: Option<f64>,
    original_filename: Option<&str>,
) -> Result<NoteRecording, StorageError> {
    attach_on(
        pool,
        note_id,
        storage_key,
        duration_seconds,
        original_filename,
    )
    .await
}

pub(crate) async fn attach_on<'e, E: sqlx::Executor<'e, Database = sqlx::Sqlite>>(
    pool: E,
    note_id: i64,
    storage_key: &str,
    duration_seconds: Option<f64>,
    original_filename: Option<&str>,
) -> Result<NoteRecording, StorageError> {
    Ok(sqlx::query_as::<_, NoteRecording>(
        "INSERT INTO note_recordings
            (note_id, storage_key, duration_seconds, original_filename)
         VALUES (?, ?, ?, ?)
         RETURNING note_id, storage_key, duration_seconds, original_filename, created_at",
    )
    .bind(note_id)
    .bind(storage_key)
    .bind(duration_seconds)
    .bind(original_filename)
    .fetch_one(pool)
    .await?)
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

    #[tokio::test]
    async fn deletion_removes_only_the_owned_file_and_is_idempotent() {
        let db = db().await;
        let dir = tempfile::tempdir().unwrap();
        let id = note(db.pool(), "Delete me").await;
        std::fs::write(dir.path().join("owned.wav"), b"audio").unwrap();
        std::fs::write(dir.path().join("unrelated.wav"), b"keep").unwrap();
        attach(db.pool(), id, "owned.wav", None, None)
            .await
            .unwrap();
        assert!(delete_with_recording(db.pool(), id, dir.path())
            .await
            .unwrap());
        assert!(notes::get(db.pool(), id).await.unwrap().is_none());
        assert!(get(db.pool(), id).await.unwrap().is_none());
        assert!(!dir.path().join("owned.wav").exists());
        assert!(dir.path().join("unrelated.wav").exists());
        assert!(!delete_with_recording(db.pool(), id, dir.path())
            .await
            .unwrap());
        let plain = note(db.pool(), "No recording").await;
        assert!(delete_with_recording(db.pool(), plain, dir.path())
            .await
            .unwrap());
        let missing = note(db.pool(), "Missing file").await;
        attach(db.pool(), missing, "missing.wav", None, None)
            .await
            .unwrap();
        assert!(delete_with_recording(db.pool(), missing, dir.path())
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn database_failure_rolls_back_attachment_and_preserves_file() {
        let db = db().await;
        let dir = tempfile::tempdir().unwrap();
        let id = note(db.pool(), "Cannot delete").await;
        std::fs::write(dir.path().join("owned.wav"), b"audio").unwrap();
        attach(db.pool(), id, "owned.wav", None, None)
            .await
            .unwrap();
        sqlx::query("CREATE TRIGGER prevent_delete BEFORE DELETE ON notes BEGIN SELECT RAISE(ABORT, 'test failure'); END")
            .execute(db.pool()).await.unwrap();
        assert!(delete_with_recording(db.pool(), id, dir.path())
            .await
            .is_err());
        assert!(notes::get(db.pool(), id).await.unwrap().is_some());
        assert!(get(db.pool(), id).await.unwrap().is_some());
        assert!(dir.path().join("owned.wav").exists());
    }

    #[tokio::test]
    async fn invalid_keys_never_remove_files_outside_recordings() {
        let db = db().await;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("recordings");
        std::fs::create_dir(&root).unwrap();
        let outside = dir.path().join("outside.wav");
        std::fs::write(&outside, b"keep").unwrap();
        for key in [
            "../outside.wav",
            "..\\outside.wav",
            "C:outside.wav",
            "a.wav:stream",
            "/outside.wav",
            "a.",
            "",
            ".",
            "..",
        ] {
            assert!(resolve(&root, key).is_none());
            let id = note(db.pool(), "Bad key").await;
            attach(db.pool(), id, key, None, None).await.unwrap();
            assert!(delete_with_recording(db.pool(), id, &root).await.unwrap());
            assert!(outside.exists());
        }
    }

    #[tokio::test]
    async fn cleanup_failure_warns_but_does_not_undo_deletion() {
        use tracing::instrument::WithSubscriber as _;
        #[derive(Clone)]
        struct Capture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl std::io::Write for Capture {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let bytes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let capture = Capture(bytes.clone());
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_ansi(false)
            .with_writer(move || capture.clone())
            .finish();
        let db = db().await;
        let dir = tempfile::tempdir().unwrap();
        // A directory is a deterministic remove_file failure on Windows and Unix.
        std::fs::create_dir(dir.path().join("locked.wav")).unwrap();
        let id = note(db.pool(), "Cleanup fails").await;
        attach(db.pool(), id, "locked.wav", None, None)
            .await
            .unwrap();
        assert!(delete_with_recording(db.pool(), id, dir.path())
            .with_subscriber(subscriber)
            .await
            .unwrap());
        assert!(get(db.pool(), id).await.unwrap().is_none());
        assert!(String::from_utf8(bytes.lock().unwrap().clone())
            .unwrap()
            .contains("could not remove a deleted note's recording"));
    }

    #[tokio::test]
    async fn concurrent_attach_cannot_survive_note_deletion() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("notes.db")).await.unwrap();
        let id = note(db.pool(), "Concurrent").await;
        let (deleted, _attached) = tokio::join!(
            delete_with_recording(db.pool(), id, dir.path()),
            attach(db.pool(), id, "race.wav", None, None),
        );
        assert!(deleted.unwrap());
        assert!(notes::get(db.pool(), id).await.unwrap().is_none());
        assert!(get(db.pool(), id).await.unwrap().is_none());
    }
}
