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
//! playback, a meeting's deleted recording (KNEG-409) and deleting forever from
//! Trash (KNEG-401) apply the same storage rules. Delete itself removes no
//! file: it moves the note to Trash (`crate::trash`), recording and all.

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

/// Remove the recording `key` names inside `directory`. A file already gone
/// counts as removed, and so does a key that isn't a bare file name: no file
/// of the application's can have one, and nothing outside the directory is
/// ever touched for it. Any other failure is returned, for the caller to undo
/// what it was removing the file for.
pub async fn remove_file(directory: &Path, key: &str) -> std::io::Result<()> {
    let Some(path) = resolve(directory, key) else {
        tracing::warn!(
            storage_key = key,
            "refused to remove an invalid recording key"
        );
        return Ok(());
    };
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Why notes in Trash weren't deleted forever (KNEG-401). The first three are
/// said to the person as they are, in plain words, and none of them deletes
/// a note.
#[derive(Debug, thiserror::Error)]
pub enum PurgeError {
    /// The move to Trash couldn't be shown to be on disk, so nothing was
    /// deleted: a power loss could otherwise bring the notes back out of
    /// Trash with their recordings gone.
    #[error(
        "Trash couldn't yet be confirmed as saved on this computer, so nothing was deleted. Try \
         again in a moment."
    )]
    NotOnDisk,
    /// A recording's file couldn't be removed. The deletion was rolled back,
    /// so every note is still in Trash -- though a recording removed before
    /// the one that wouldn't go is gone -- and trying again finishes.
    #[error(
        "A recording in Trash couldn't be deleted from this computer, so the notes are still in \
         Trash. Close any app that may be using it and try again."
    )]
    RecordingKept,
    /// The recordings were removed, but the notes couldn't then be: they are
    /// still in Trash, without them, and trying again finishes.
    #[error(
        "The recordings were deleted, but the notes couldn't be removed from Trash. Try again to \
         finish."
    )]
    NotesKept,
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// Delete note `id` forever, with its recording, when it is in Trash
/// (KNEG-401). `Ok(false)` for a note that isn't there or isn't in Trash: a
/// live note goes to Trash first, so a screen that is out of date can't
/// delete one for good. See [`empty_trash`] for the order things are deleted
/// in.
pub async fn delete_forever(
    pool: &SqlitePool,
    id: i64,
    directory: &Path,
) -> Result<bool, PurgeError> {
    Ok(empty_trash(pool, &[id], directory).await? > 0)
}

/// Delete forever the notes `ids` that are still in Trash, with their
/// recordings, and say how many went (KNEG-401). `ids` are the notes the
/// person was asked about: a note moved to Trash while they were deciding --
/// through the local API, say -- is not among them, and stays. Nothing calls
/// this but a person: Trash is never emptied on its own.
///
/// **The order, and why** -- `delete_attached`'s, so a recording deleted
/// forever can never come back:
///
/// 1. A full WAL checkpoint, so the notes' move to Trash is in the database
///    file and synced. A commit is not synced (WAL, `synchronous = NORMAL`),
///    and without this a power loss after the files went could bring a note
///    back out of Trash, live, with its recording gone. If it can't finish --
///    a reader holding it back past the busy timeout -- nothing is deleted,
///    and the error says so.
/// 2. One transaction, holding SQLite's write lock from its start
///    (`BEGIN IMMEDIATE`), deletes the recordings' rows, taking their names,
///    and the notes -- only those still in Trash.
/// 3. The files are removed, inside that transaction. A file already gone
///    counts as removed. Any other failure rolls the transaction back and is
///    returned: the notes stay in Trash, and the person sees why.
/// 4. The transaction commits.
///
/// Files are never removed after the commit: a crash or a failure between
/// the two would leave a WAV no note owns, which crash recovery offers under
/// Meetings › Interrupted -- a recording deleted forever, offered back, with
/// no error anyone saw. The worst this order leaves is notes still in Trash
/// whose recordings are gone -- after a failed removal of a later file, a
/// failed commit, or a power loss before the commit reached the disk -- and
/// pressing again finishes, the missing files counting as removed.
pub async fn empty_trash(
    pool: &SqlitePool,
    ids: &[i64],
    directory: &Path,
) -> Result<u64, PurgeError> {
    if ids.is_empty() {
        return Ok(0);
    }
    // A JSON array for `json_each`: one bound value, however many notes.
    let ids = format!(
        "[{}]",
        ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
    );
    let (busy, log, checkpointed): (i64, i64, i64) = sqlx::query_as("PRAGMA wal_checkpoint(FULL)")
        .fetch_one(pool)
        .await
        .map_err(StorageError::from)?;
    if !checkpointed_everything(busy, log, checkpointed) {
        return Err(PurgeError::NotOnDisk);
    }
    let mut tx = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(StorageError::from)?;
    let keys: Vec<String> = sqlx::query_scalar(
        "DELETE FROM note_recordings WHERE note_id IN
             (SELECT id FROM notes
              WHERE trashed_at IS NOT NULL AND id IN (SELECT value FROM json_each(?)))
         RETURNING storage_key",
    )
    .bind(&ids)
    .fetch_all(&mut *tx)
    .await
    .map_err(StorageError::from)?;
    let deleted = sqlx::query(
        "DELETE FROM notes
         WHERE trashed_at IS NOT NULL AND id IN (SELECT value FROM json_each(?))",
    )
    .bind(&ids)
    .execute(&mut *tx)
    .await
    .map_err(StorageError::from)?
    .rows_affected();
    for key in &keys {
        if let Err(error) = remove_file(directory, key).await {
            tracing::warn!(storage_key = %key, error = %error, "could not delete a recording in Trash");
            tx.rollback().await.map_err(StorageError::from)?;
            return Err(PurgeError::RecordingKept);
        }
    }
    // The files are gone by now, so a failed commit can't be undone: the notes
    // are still in Trash without them, and saying nothing was deleted would be
    // false.
    tx.commit().await.map_err(|error| {
        tracing::warn!(error = %error, "deleted Trash's recordings but not its notes");
        PurgeError::NotesKept
    })?;
    Ok(deleted)
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
///
/// **Nothing is removed until the note is on disk.** The database is WAL with
/// `synchronous = NORMAL` (`kw_storage::Db::open`), so a commit is not synced:
/// the note and its attachment, committed a moment before, can still be only
/// in the operating system's cache. A power loss then would lose them, and
/// with this function removing the file, the transcript and the recording
/// both. So a `wal_checkpoint(FULL)` runs first: it waits (the busy timeout)
/// for any reader on an older snapshot, copies every committed frame into the
/// database file and syncs it. Only when it says it copied all of them is the
/// note durable and the file removed; otherwise the recording stays attached
/// and the error says why. This deletion's own commit is still unsynced, so a
/// power loss after the file is gone can leave the note -- transcript, typed
/// notes and all -- with an attachment naming a missing file: the recording
/// is gone, as asked, and nothing else is.
///
/// The order is not reversed -- commit, then remove -- because a crash
/// between the two would leave a file no note owns, offered under
/// Interrupted and recoverable into a second note.
pub async fn delete_attached(
    pool: &SqlitePool,
    note_id: i64,
    directory: &Path,
    details: &str,
) -> Result<bool, String> {
    let failed =
        |error: sqlx::Error| format!("could not delete the recording's attachment: {error}");
    let (busy, log, checkpointed): (i64, i64, i64) = sqlx::query_as("PRAGMA wal_checkpoint(FULL)")
        .fetch_one(pool)
        .await
        .map_err(failed)?;
    if !checkpointed_everything(busy, log, checkpointed) {
        return Err(
            "the note couldn't yet be confirmed as written to disk, so its recording was kept \
             with it"
                .to_owned(),
        );
    }
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
    // The file is gone by now, so a failed commit cannot be undone: the note
    // still lists a recording, and saying it is merely kept would be false.
    tx.commit().await.map_err(|error| {
        format!(
            "the recording's file was deleted, but the note couldn't be updated to match, so it \
             may still list a recording that's no longer there: {error}"
        )
    })?;
    Ok(true)
}

/// Whether `PRAGMA wal_checkpoint(FULL)`'s answer -- `busy`, the frames in
/// the WAL (`log`), the frames copied (`checkpointed`) -- means every commit
/// so far is in the database file and synced: nothing held it back, and it
/// copied every frame. `-1` for both is a database with no WAL, in memory.
const fn checkpointed_everything(busy: i64, log: i64, checkpointed: i64) -> bool {
    busy == 0 && log == checkpointed
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

    #[test]
    fn only_a_checkpoint_that_copied_every_frame_makes_the_note_safe_to_rely_on() {
        // (busy, frames in the WAL, frames checkpointed) as
        // `PRAGMA wal_checkpoint(FULL)` answers.
        assert!(checkpointed_everything(0, 12, 12));
        assert!(checkpointed_everything(0, 0, 0), "an empty WAL");
        assert!(
            checkpointed_everything(0, -1, -1),
            "no WAL at all: an in-memory database"
        );
        assert!(!checkpointed_everything(1, 12, 4), "a reader held it back");
        assert!(!checkpointed_everything(1, 12, 12));
        assert!(!checkpointed_everything(0, 12, 4));
    }

    /// A database on disk in WAL mode, as the application opens it, and a
    /// note with its recording attached and the file beside it.
    async fn on_disk(dir: &Path) -> (Db, i64) {
        let db = Db::open(&dir.join("notes.db")).await.unwrap();
        let id = recorded_note(db.pool(), None).await;
        std::fs::write(dir.join("owned.wav"), b"audio").unwrap();
        (db, id)
    }

    /// The same database through a pool that waits only briefly for a
    /// checkpoint, so a held-back one is measured in milliseconds rather than
    /// the application's five seconds.
    async fn impatient(dir: &Path) -> SqlitePool {
        use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(dir.join("notes.db"))
                    .journal_mode(SqliteJournalMode::Wal)
                    .busy_timeout(std::time::Duration::from_millis(100)),
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_recording_on_a_wal_database_is_deleted_once_its_note_is_checkpointed() {
        let dir = tempfile::tempdir().unwrap();
        let (db, id) = on_disk(dir.path()).await;
        attach(db.pool(), id, "owned.wav", Some(12.0), None)
            .await
            .unwrap();

        assert_eq!(
            delete_attached(db.pool(), id, dir.path(), SAYS).await,
            Ok(true)
        );
        assert!(get(db.pool(), id).await.unwrap().is_none());
        assert!(!dir.path().join("owned.wav").exists());

        // What a power loss now would leave: the database file without its
        // WAL. The note and its attachment were written into the file itself
        // before the recording went; only the deletion's own commit is not.
        use sqlx::Connection as _;
        let copy = dir.path().join("as-on-disk.db");
        std::fs::copy(dir.path().join("notes.db"), &copy).unwrap();
        let mut alone = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new().filename(&copy),
        )
        .await
        .unwrap();
        let note: Option<String> = sqlx::query_scalar("SELECT title FROM notes WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut alone)
            .await
            .unwrap();
        assert_eq!(note.as_deref(), Some("Board review"));
        let attached: Option<String> =
            sqlx::query_scalar("SELECT storage_key FROM note_recordings WHERE note_id = ?")
                .bind(id)
                .fetch_optional(&mut alone)
                .await
                .unwrap();
        assert_eq!(attached.as_deref(), Some("owned.wav"));
    }

    #[tokio::test]
    async fn a_recording_is_kept_while_its_note_cannot_be_checkpointed() {
        use sqlx::Connection as _;
        let dir = tempfile::tempdir().unwrap();
        let (db, id) = on_disk(dir.path()).await;
        // A reader on the snapshot from before the attachment: a FULL
        // checkpoint cannot copy past it, so the attachment's commit cannot
        // be shown to be on disk.
        let mut reader = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new().filename(dir.path().join("notes.db")),
        )
        .await
        .unwrap();
        let mut held = reader.begin().await.unwrap();
        let _: i64 = sqlx::query_scalar("SELECT count(*) FROM notes")
            .fetch_one(&mut *held)
            .await
            .unwrap();
        attach(db.pool(), id, "owned.wav", Some(12.0), None)
            .await
            .unwrap();

        let pool = impatient(dir.path()).await;
        let error = delete_attached(&pool, id, dir.path(), SAYS)
            .await
            .unwrap_err();
        assert!(error.contains("written to disk"), "{error}");
        assert!(dir.path().join("owned.wav").exists());
        assert!(get(db.pool(), id).await.unwrap().is_some());
        assert_eq!(
            notes::get(db.pool(), id)
                .await
                .unwrap()
                .unwrap()
                .recording_details,
            None
        );

        // Once the reader lets go, the same call deletes.
        held.rollback().await.unwrap();
        assert_eq!(delete_attached(&pool, id, dir.path(), SAYS).await, Ok(true));
        assert!(!dir.path().join("owned.wav").exists());
    }

    /* ----------------------------------------- deleting forever (KNEG-401) */

    /// A note, moved to Trash, with `key` attached and a file of that name in
    /// `dir`, when there is a `key`.
    async fn trashed(pool: &SqlitePool, dir: &Path, title: &str, key: Option<&str>) -> i64 {
        let id = note(pool, title).await;
        if let Some(key) = key {
            std::fs::write(dir.join(key), b"audio").unwrap();
            attach(pool, id, key, Some(1.0), None).await.unwrap();
        }
        assert!(crate::trash::trash(pool, id).await.unwrap());
        id
    }

    async fn sorted_keys(pool: &SqlitePool) -> Vec<String> {
        let mut keys = storage_keys(pool).await.unwrap();
        keys.sort();
        keys
    }

    /// Empty trash deletes the notes it was asked about that are still in
    /// Trash, and their files: not a live note it is handed, and not one
    /// moved to Trash while the person was deciding.
    #[tokio::test]
    async fn emptying_the_trash_deletes_only_the_notes_asked_about_and_their_files() {
        let db = db().await;
        let p = db.pool();
        let dir = tempfile::tempdir().unwrap();
        let recorded = trashed(p, dir.path(), "Recorded", Some("trashed.wav")).await;
        let typed = trashed(p, dir.path(), "Typed", None).await;
        let live = note(p, "Live").await;
        std::fs::write(dir.path().join("live.wav"), b"audio").unwrap();
        attach(p, live, "live.wav", None, None).await.unwrap();
        std::fs::write(dir.path().join("unrelated.wav"), b"keep").unwrap();
        // Moved to Trash after the question was asked about the others.
        let later = trashed(p, dir.path(), "Later", Some("later.wav")).await;

        assert_eq!(
            empty_trash(p, &[recorded, typed, live], dir.path())
                .await
                .unwrap(),
            2
        );

        assert!(notes::get(p, recorded).await.unwrap().is_none());
        assert!(notes::get(p, typed).await.unwrap().is_none());
        assert!(get(p, recorded).await.unwrap().is_none());
        assert!(!dir.path().join("trashed.wav").exists());
        assert!(notes::get(p, live).await.unwrap().is_some());
        assert!(dir.path().join("live.wav").exists());
        assert!(crate::trash::is_trashed(p, later).await.unwrap());
        assert!(dir.path().join("later.wav").exists());
        assert!(dir.path().join("unrelated.wav").exists());
        assert_eq!(sorted_keys(p).await, ["later.wav", "live.wav"]);
        assert_eq!(
            empty_trash(p, &[], dir.path()).await.unwrap(),
            0,
            "asked about nothing, nothing goes"
        );
    }

    #[tokio::test]
    async fn deleting_forever_takes_only_a_note_in_trash() {
        let db = db().await;
        let p = db.pool();
        let dir = tempfile::tempdir().unwrap();
        let live = note(p, "Live").await;
        std::fs::write(dir.path().join("live.wav"), b"audio").unwrap();
        attach(p, live, "live.wav", None, None).await.unwrap();
        let gone = trashed(p, dir.path(), "Gone", Some("gone.wav")).await;
        let other = trashed(p, dir.path(), "Other", Some("other.wav")).await;

        assert!(
            !delete_forever(p, live, dir.path()).await.unwrap(),
            "a live note is never deleted forever"
        );
        assert!(notes::get(p, live).await.unwrap().is_some());
        assert!(dir.path().join("live.wav").exists());

        assert!(delete_forever(p, gone, dir.path()).await.unwrap());
        assert!(notes::get(p, gone).await.unwrap().is_none());
        assert!(!dir.path().join("gone.wav").exists());
        assert!(
            notes::get(p, other).await.unwrap().is_some(),
            "only the note asked for"
        );
        assert!(dir.path().join("other.wav").exists());
        assert!(!delete_forever(p, gone, dir.path()).await.unwrap());
        assert!(!delete_forever(p, 404, dir.path()).await.unwrap());
    }

    #[tokio::test]
    async fn a_recording_already_gone_from_disk_is_deleted_forever() {
        let db = db().await;
        let p = db.pool();
        let dir = tempfile::tempdir().unwrap();
        let id = note(p, "Missing file").await;
        attach(p, id, "missing.wav", None, None).await.unwrap();
        assert!(crate::trash::trash(p, id).await.unwrap());

        assert!(delete_forever(p, id, dir.path()).await.unwrap());
        assert!(notes::get(p, id).await.unwrap().is_none());
    }

    /// A file that won't go rolls the whole deletion back and says why, in
    /// words the screen shows: every note stays in Trash with its recording
    /// row, so no file is left that no note owns. Once the file can go,
    /// pressing again finishes, a file already removed counting as removed.
    #[tokio::test]
    async fn a_recording_that_cannot_be_removed_keeps_its_notes_in_trash_and_says_why() {
        let db = db().await;
        let p = db.pool();
        let dir = tempfile::tempdir().unwrap();
        let plain = trashed(p, dir.path(), "Plain", Some("plain.wav")).await;
        let locked = note(p, "Locked").await;
        // A directory is a deterministic remove_file failure on Windows and Unix.
        std::fs::create_dir(dir.path().join("locked.wav")).unwrap();
        attach(p, locked, "locked.wav", None, None).await.unwrap();
        assert!(crate::trash::trash(p, locked).await.unwrap());

        let error = empty_trash(p, &[plain, locked], dir.path())
            .await
            .unwrap_err();
        assert!(matches!(error, PurgeError::RecordingKept), "{error:?}");
        assert_eq!(
            error.to_string(),
            "A recording in Trash couldn't be deleted from this computer, so the notes are \
             still in Trash. Close any app that may be using it and try again."
        );
        for id in [plain, locked] {
            assert!(crate::trash::is_trashed(p, id).await.unwrap(), "{id}");
            assert!(get(p, id).await.unwrap().is_some(), "{id}");
        }
        assert!(dir.path().join("locked.wav").exists());
        assert_eq!(sorted_keys(p).await, ["locked.wav", "plain.wav"]);

        std::fs::remove_dir(dir.path().join("locked.wav")).unwrap();
        assert_eq!(
            empty_trash(p, &[plain, locked], dir.path()).await.unwrap(),
            2
        );
        assert!(!dir.path().join("plain.wav").exists());
        assert!(storage_keys(p).await.unwrap().is_empty());
    }

    /// The move to Trash is in the database file itself -- not only in its
    /// WAL, which a power loss can take with it -- before any file is
    /// touched, so a note whose recording goes can't come back out of Trash.
    /// Shown with a removal that fails: the deletion rolls back, and the file
    /// on its own already holds the note, in Trash.
    #[tokio::test]
    async fn the_move_to_trash_is_on_disk_before_any_file_goes() {
        use sqlx::Connection as _;
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("notes.db")).await.unwrap();
        let p = db.pool();
        let id = note(p, "Locked").await;
        std::fs::create_dir(dir.path().join("locked.wav")).unwrap();
        attach(p, id, "locked.wav", None, None).await.unwrap();
        assert!(crate::trash::trash(p, id).await.unwrap());

        // What a power loss would leave: the database file without its WAL.
        let in_file = |name: &'static str| {
            let copy = dir.path().join(name);
            std::fs::copy(dir.path().join("notes.db"), &copy).unwrap();
            async move {
                let mut alone = sqlx::SqliteConnection::connect_with(
                    &sqlx::sqlite::SqliteConnectOptions::new().filename(&copy),
                )
                .await
                .unwrap();
                sqlx::query_scalar::<_, Option<String>>("SELECT trashed_at FROM notes WHERE id = ?")
                    .bind(id)
                    .fetch_optional(&mut alone)
                    .await
                    .ok()
                    .flatten()
                    .flatten()
            }
        };
        assert_eq!(
            in_file("before.db").await,
            None,
            "the move to Trash is only in the WAL yet"
        );

        assert!(matches!(
            empty_trash(p, &[id], dir.path()).await,
            Err(PurgeError::RecordingKept)
        ));

        assert!(
            in_file("after.db").await.is_some(),
            "on disk, in Trash, before the file was tried"
        );
        assert!(crate::trash::is_trashed(p, id).await.unwrap());
    }

    /// While the move to Trash can't be shown to be on disk -- a reader on an
    /// older snapshot holds the checkpoint back -- nothing is deleted, and the
    /// error says so. Once it lets go, the same call deletes.
    #[tokio::test]
    async fn nothing_is_deleted_while_trash_cannot_be_checkpointed() {
        use sqlx::Connection as _;
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("notes.db")).await.unwrap();
        let id = note(db.pool(), "Recorded").await;
        std::fs::write(dir.path().join("owned.wav"), b"audio").unwrap();
        attach(db.pool(), id, "owned.wav", None, None)
            .await
            .unwrap();
        let mut reader = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new().filename(dir.path().join("notes.db")),
        )
        .await
        .unwrap();
        let mut held = reader.begin().await.unwrap();
        let _: i64 = sqlx::query_scalar("SELECT count(*) FROM notes")
            .fetch_one(&mut *held)
            .await
            .unwrap();
        assert!(crate::trash::trash(db.pool(), id).await.unwrap());

        let pool = impatient(dir.path()).await;
        let error = delete_forever(&pool, id, dir.path()).await.unwrap_err();
        assert!(matches!(error, PurgeError::NotOnDisk), "{error:?}");
        assert!(error.to_string().contains("nothing was deleted"), "{error}");
        assert!(dir.path().join("owned.wav").exists());
        assert!(crate::trash::is_trashed(db.pool(), id).await.unwrap());
        assert!(get(db.pool(), id).await.unwrap().is_some());

        held.rollback().await.unwrap();
        assert!(delete_forever(&pool, id, dir.path()).await.unwrap());
        assert!(!dir.path().join("owned.wav").exists());
    }

    #[tokio::test]
    async fn a_database_failure_deletes_nothing() {
        let db = db().await;
        let p = db.pool();
        let dir = tempfile::tempdir().unwrap();
        let id = trashed(p, dir.path(), "Cannot delete", Some("owned.wav")).await;
        sqlx::query("CREATE TRIGGER prevent_delete BEFORE DELETE ON notes BEGIN SELECT RAISE(ABORT, 'test failure'); END")
            .execute(p).await.unwrap();

        assert!(matches!(
            delete_forever(p, id, dir.path()).await,
            Err(PurgeError::Storage(_))
        ));
        assert!(notes::get(p, id).await.unwrap().is_some());
        assert!(get(p, id).await.unwrap().is_some());
        assert!(dir.path().join("owned.wav").exists());
    }

    /// A stored name that would reach outside the recordings directory names
    /// no file of the application's: nothing outside is touched, and the note
    /// still goes.
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
            assert!(crate::trash::trash(db.pool(), id).await.unwrap());
            assert!(
                delete_forever(db.pool(), id, &root).await.unwrap(),
                "{key:?}"
            );
            assert!(outside.exists());
        }
    }

    #[tokio::test]
    async fn concurrent_attach_cannot_survive_deletion_forever() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("notes.db")).await.unwrap();
        let id = note(db.pool(), "Concurrent").await;
        assert!(crate::trash::trash(db.pool(), id).await.unwrap());
        let (deleted, _attached) = tokio::join!(
            delete_forever(db.pool(), id, dir.path()),
            attach(db.pool(), id, "race.wav", None, None),
        );
        assert!(deleted.unwrap());
        assert!(notes::get(db.pool(), id).await.unwrap().is_none());
        assert!(get(db.pool(), id).await.unwrap().is_none());
    }
}
