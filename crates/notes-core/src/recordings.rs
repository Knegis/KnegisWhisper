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

/// Delete a note's recording and keep the note (KNEG-409): the attachment row
/// and the file go together, and `details` is added to the note's recording
/// details with them, so the note says its recording was deleted exactly when
/// it was.
///
/// One transaction. The row is deleted and the details written first, which
/// takes SQLite's write lock, then the file is removed, and only then does the
/// transaction commit. A file that cannot be removed rolls all of it back: the
/// note keeps its recording and says nothing about deleting it, and the error
/// says why. A file already gone from disk counts as removed. `Ok(false)` when
/// the note has no recording attached, and nothing changes.
pub async fn delete_attached(
    pool: &SqlitePool,
    note_id: i64,
    directory: &Path,
    details: &str,
) -> Result<bool, String> {
    let failed =
        |error: sqlx::Error| format!("could not delete the recording's attachment: {error}");
    let mut tx = pool.begin().await.map_err(failed)?;
    let key: Option<String> =
        sqlx::query_scalar("DELETE FROM note_recordings WHERE note_id = ? RETURNING storage_key")
            .bind(note_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(failed)?;
    let Some(key) = key else {
        tx.rollback().await.map_err(failed)?;
        return Ok(false);
    };
    let Some(path) = resolve(directory, &key) else {
        tx.rollback().await.map_err(failed)?;
        return Err(format!(
            "refused to delete a recording with an invalid name: {key}"
        ));
    };
    sqlx::query(
        "UPDATE notes SET recording_details = CASE
             WHEN recording_details IS NULL OR trim(recording_details) = '' THEN ?1
             ELSE recording_details || char(10) || char(10) || ?1
         END
         WHERE id = ?2",
    )
    .bind(details)
    .bind(note_id)
    .execute(&mut *tx)
    .await
    .map_err(failed)?;
    match tokio::fs::remove_file(&path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            tx.rollback().await.map_err(failed)?;
            return Err(format!("could not delete {}: {error}", path.display()));
        }
    }
    tx.commit().await.map_err(failed)?;
    Ok(true)
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

/// Bare names of all recordings already owned by notes.
pub async fn storage_keys(pool: &SqlitePool) -> Result<Vec<String>, StorageError> {
    Ok(
        sqlx::query_scalar("SELECT storage_key FROM note_recordings")
            .fetch_all(pool)
            .await?,
    )
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

    /* -------------------------------- deleting a note's recording (KNEG-409) */

    const SAYS: &str = "The recording was deleted.";

    async fn recorded_note(pool: &SqlitePool, details: Option<&str>) -> i64 {
        notes::create(
            pool,
            &NewNote {
                title: "Board review".into(),
                content: "typed".into(),
                transcript: Some(r#"[{"text":"hello","timestamp":0.0}]"#.into()),
                recording_details: details.map(str::to_owned),
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn deleting_an_attached_recording_removes_its_row_and_its_file_and_keeps_the_note() {
        let db = db().await;
        let dir = tempfile::tempdir().unwrap();
        let id = recorded_note(db.pool(), Some("Recording sources: microphone.")).await;
        std::fs::write(dir.path().join("owned.wav"), b"audio").unwrap();
        std::fs::write(dir.path().join("unrelated.wav"), b"keep").unwrap();
        attach(db.pool(), id, "owned.wav", Some(12.0), None)
            .await
            .unwrap();

        assert_eq!(
            delete_attached(db.pool(), id, dir.path(), SAYS).await,
            Ok(true)
        );

        assert!(get(db.pool(), id).await.unwrap().is_none());
        assert!(!dir.path().join("owned.wav").exists());
        assert!(dir.path().join("unrelated.wav").exists());
        let note = notes::get(db.pool(), id).await.unwrap().unwrap();
        assert_eq!(note.content, "typed");
        assert!(note.transcript.is_some(), "the transcript stays");
        assert_eq!(
            note.recording_details.as_deref(),
            Some("Recording sources: microphone.\n\nThe recording was deleted.")
        );
        // Nothing left owns a file, and no file is left to own: the
        // recordings directory holds only what was never this note's.
        assert!(storage_keys(db.pool()).await.unwrap().is_empty());
        let left: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, ["unrelated.wav"]);
    }

    #[tokio::test]
    async fn a_note_with_no_details_says_only_that_its_recording_was_deleted() {
        let db = db().await;
        let dir = tempfile::tempdir().unwrap();
        let id = recorded_note(db.pool(), None).await;
        std::fs::write(dir.path().join("owned.wav"), b"audio").unwrap();
        attach(db.pool(), id, "owned.wav", None, None)
            .await
            .unwrap();

        assert_eq!(
            delete_attached(db.pool(), id, dir.path(), SAYS).await,
            Ok(true)
        );
        let note = notes::get(db.pool(), id).await.unwrap().unwrap();
        assert_eq!(note.recording_details.as_deref(), Some(SAYS));
    }

    #[tokio::test]
    async fn a_recording_that_cannot_be_removed_stays_attached_and_says_why() {
        let db = db().await;
        let dir = tempfile::tempdir().unwrap();
        // A directory is a deterministic remove_file failure on Windows and Unix.
        std::fs::create_dir(dir.path().join("locked.wav")).unwrap();
        let id = recorded_note(db.pool(), Some("Recording sources: microphone.")).await;
        attach(db.pool(), id, "locked.wav", Some(12.0), None)
            .await
            .unwrap();

        let error = delete_attached(db.pool(), id, dir.path(), SAYS)
            .await
            .unwrap_err();
        assert!(error.contains("locked.wav"), "{error}");

        let kept = get(db.pool(), id).await.unwrap().unwrap();
        assert_eq!(kept.storage_key, "locked.wav");
        assert!(dir.path().join("locked.wav").exists());
        let note = notes::get(db.pool(), id).await.unwrap().unwrap();
        assert_eq!(
            note.recording_details.as_deref(),
            Some("Recording sources: microphone."),
            "the note does not say a recording it still has was deleted"
        );
    }

    #[tokio::test]
    async fn a_note_with_no_recording_has_nothing_to_delete() {
        let db = db().await;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("unrelated.wav"), b"keep").unwrap();
        let id = recorded_note(db.pool(), None).await;

        assert_eq!(
            delete_attached(db.pool(), id, dir.path(), SAYS).await,
            Ok(false)
        );
        let note = notes::get(db.pool(), id).await.unwrap().unwrap();
        assert_eq!(note.recording_details, None);
        assert!(dir.path().join("unrelated.wav").exists());
    }

    #[tokio::test]
    async fn a_recording_already_gone_from_disk_is_deleted() {
        let db = db().await;
        let dir = tempfile::tempdir().unwrap();
        let id = recorded_note(db.pool(), None).await;
        attach(db.pool(), id, "missing.wav", None, None)
            .await
            .unwrap();

        assert_eq!(
            delete_attached(db.pool(), id, dir.path(), SAYS).await,
            Ok(true)
        );
        assert!(get(db.pool(), id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn an_invalid_recording_name_is_refused_and_left_attached() {
        let db = db().await;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("recordings");
        std::fs::create_dir(&root).unwrap();
        let outside = dir.path().join("outside.wav");
        std::fs::write(&outside, b"keep").unwrap();
        let id = recorded_note(db.pool(), None).await;
        attach(db.pool(), id, "../outside.wav", None, None)
            .await
            .unwrap();

        assert!(delete_attached(db.pool(), id, &root, SAYS).await.is_err());
        assert!(outside.exists());
        assert!(get(db.pool(), id).await.unwrap().is_some());
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
