//! Durable, local deletion intent for one specific attached recording.
use std::path::Path;

use serde::Serialize;
use sqlx::{FromRow, SqlitePool};
use ts_rs::TS;

use crate::recordings;

#[derive(Debug, Clone, FromRow)]
struct Stored {
    storage_key: String,
    status: String,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NoteRecordingState {
    /// Unknown for older/imported notes. No text-based backfill.
    pub status: String,
    pub error: Option<String>,
    pub can_retry: bool,
}

async fn stored(pool: &SqlitePool, id: i64) -> Result<Option<Stored>, String> {
    sqlx::query_as(
        "SELECT storage_key, status, last_error FROM note_recording_state WHERE note_id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())
}

/// Only establish intent while the matching attachment is on a live note.
pub async fn establish(pool: &SqlitePool, id: i64, key: &str, status: &str) -> Result<(), String> {
    let changed = sqlx::query("INSERT INTO note_recording_state(note_id,storage_key,status) SELECT ?,?,? WHERE EXISTS(SELECT 1 FROM note_recordings r JOIN notes n ON n.id=r.note_id WHERE r.note_id=? AND r.storage_key=? AND n.trashed_at IS NULL) ON CONFLICT(note_id) DO UPDATE SET storage_key=excluded.storage_key,status=excluded.status,last_error=NULL")
        .bind(id).bind(key).bind(status).bind(id).bind(key).execute(pool).await.map_err(|e| e.to_string())?.rows_affected();
    if changed == 0 {
        return Err("The recording is no longer attached to a live note.".into());
    }
    Ok(())
}

/// Read without mutating: a crashed deletion can leave an attachment to a missing file.
pub async fn get(
    pool: &SqlitePool,
    id: i64,
    directory: &Path,
) -> Result<Option<NoteRecordingState>, String> {
    let Some(saved) = stored(pool, id).await? else {
        return Ok(None);
    };
    let attachment = recordings::get(pool, id).await.map_err(|e| e.to_string())?;
    let live =
        sqlx::query_scalar::<_, i64>("SELECT 1 FROM notes WHERE id=? AND trashed_at IS NULL")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?
            .is_some();
    let retry = matches!(saved.status.as_str(), "delete-pending" | "delete-failed");
    let same = attachment
        .as_ref()
        .is_some_and(|r| r.storage_key == saved.storage_key);
    if retry && !same {
        return Ok(Some(NoteRecordingState {
            status: "unknown".into(),
            error: Some(
                "The attachment changed; the old deletion request cannot be retried.".into(),
            ),
            can_retry: false,
        }));
    }
    let mut status = saved.status;
    if retry {
        let path =
            recordings::resolve(directory, &saved.storage_key).ok_or("Invalid recording name.")?;
        if !tokio::fs::try_exists(path)
            .await
            .map_err(|e| format!("Could not inspect the recording: {e}"))?
        {
            status = "deleted-update-failed".into();
        }
    }
    Ok(Some(NoteRecordingState {
        status,
        error: saved.last_error,
        can_retry: retry && same && live,
    }))
}

/// Explicit retry only. The transaction checks identity and Trash again before removing anything.
pub async fn retry(
    pool: &SqlitePool,
    id: i64,
    directory: &Path,
) -> Result<NoteRecordingState, String> {
    let saved = stored(pool, id)
        .await?
        .ok_or("This note has no recording deletion request.")?;
    if saved.status == "deleted" {
        return get(pool, id, directory)
            .await?
            .ok_or("The note no longer exists.".into());
    }
    if !matches!(saved.status.as_str(), "delete-pending" | "delete-failed") {
        return Err("This recording was kept; there is no deletion to retry.".into());
    }
    let state = get(pool, id, directory)
        .await?
        .ok_or("The note no longer exists.")?;
    if !state.can_retry {
        return Err("Restore the note first, or check whether its recording was replaced.".into());
    }
    match recordings::delete_attached_expected(pool, id, directory, &saved.storage_key).await {
        Ok(true) => {}
        Ok(false) => {
            return Err(
                "The recording changed or the note is in Trash; nothing was deleted.".into(),
            )
        }
        Err(error) => {
            // Keep the durable intent even if this best-effort error update fails.
            let _ = sqlx::query("UPDATE note_recording_state SET status='delete-failed',last_error=? WHERE note_id=? AND storage_key=? AND status IN ('delete-pending','delete-failed')")
                .bind(&error).bind(id).bind(&saved.storage_key).execute(pool).await;
        }
    }
    get(pool, id, directory)
        .await?
        .ok_or("The note no longer exists.".into())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::notes::{self, NewNote};
    use kw_storage::Db;

    #[tokio::test]
    async fn retry_survives_restart_and_never_targets_a_replacement() {
        let root = tempfile::tempdir().unwrap();
        let db = Db::open(&root.path().join("notes.db")).await.unwrap();
        let id = notes::create(db.pool(), &NewNote::default()).await.unwrap();
        recordings::attach(db.pool(), id, "old.wav", None, None)
            .await
            .unwrap();
        tokio::fs::write(root.path().join("old.wav"), b"wav")
            .await
            .unwrap();
        establish(db.pool(), id, "old.wav", "delete-pending")
            .await
            .unwrap();
        db.pool().close().await;
        let db = Db::open(&root.path().join("notes.db")).await.unwrap();
        assert_eq!(
            retry(db.pool(), id, root.path()).await.unwrap().status,
            "deleted"
        );
        assert!(!root.path().join("old.wav").exists());
        assert_eq!(
            retry(db.pool(), id, root.path()).await.unwrap().status,
            "deleted"
        );
        recordings::attach(db.pool(), id, "new.wav", None, None)
            .await
            .unwrap();
        tokio::fs::write(root.path().join("new.wav"), b"wav")
            .await
            .unwrap();
        assert!(get(db.pool(), id, root.path()).await.unwrap().is_none());
        assert!(retry(db.pool(), id, root.path()).await.is_err());
        assert!(root.path().join("new.wav").exists());
    }

    #[tokio::test]
    async fn failed_removal_and_missing_file_are_truthful_and_trash_refuses_retry() {
        let root = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().await.unwrap();
        let id = notes::create(db.pool(), &NewNote::default()).await.unwrap();
        recordings::attach(db.pool(), id, "blocked.wav", None, None)
            .await
            .unwrap();
        tokio::fs::create_dir(root.path().join("blocked.wav"))
            .await
            .unwrap();
        establish(db.pool(), id, "blocked.wav", "delete-pending")
            .await
            .unwrap();
        assert_eq!(
            retry(db.pool(), id, root.path()).await.unwrap().status,
            "delete-failed"
        );
        crate::trash::trash(db.pool(), id).await.unwrap();
        assert!(retry(db.pool(), id, root.path()).await.is_err());
        crate::trash::restore(db.pool(), id).await.unwrap();
        tokio::fs::remove_dir(root.path().join("blocked.wav"))
            .await
            .unwrap();
        assert_eq!(
            get(db.pool(), id, root.path())
                .await
                .unwrap()
                .unwrap()
                .status,
            "deleted-update-failed"
        );
        assert_eq!(
            retry(db.pool(), id, root.path()).await.unwrap().status,
            "deleted"
        );
    }

    #[tokio::test]
    async fn a_failed_commit_after_removal_leaves_retryable_cleanup() {
        let root = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().await.unwrap();
        let p = db.pool();
        let id = notes::create(p, &NewNote::default()).await.unwrap();
        recordings::attach(p, id, "commit.wav", None, None)
            .await
            .unwrap();
        tokio::fs::write(root.path().join("commit.wav"), b"audio")
            .await
            .unwrap();
        establish(p, id, "commit.wav", "delete-pending")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE deferred_failure(value INTEGER REFERENCES notes(id) DEFERRABLE INITIALLY DEFERRED)").execute(p).await.unwrap();
        sqlx::query("CREATE TRIGGER fail_deletion_commit AFTER DELETE ON note_recordings BEGIN INSERT INTO deferred_failure VALUES(-1000); END").execute(p).await.unwrap();
        let state = retry(p, id, root.path()).await.unwrap();
        assert_eq!(state.status, "deleted-update-failed");
        assert!(state.can_retry);
        assert!(state.error.unwrap().contains("file was deleted"));
        assert!(!root.path().join("commit.wav").exists());
        assert!(recordings::get(p, id).await.unwrap().is_some());
        sqlx::query("DROP TRIGGER fail_deletion_commit")
            .execute(p)
            .await
            .unwrap();
        assert_eq!(retry(p, id, root.path()).await.unwrap().status, "deleted");
        assert!(recordings::get(p, id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn no_attached_recording_means_no_kept_claim() {
        let db = Db::open_in_memory().await.unwrap();
        let root = tempfile::tempdir().unwrap();
        let id = notes::create(db.pool(), &NewNote::default()).await.unwrap();
        assert!(establish(db.pool(), id, "missing.wav", "kept-incomplete")
            .await
            .is_err());
        assert!(get(db.pool(), id, root.path()).await.unwrap().is_none());
    }
}
