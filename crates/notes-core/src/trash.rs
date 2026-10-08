//! Trash (KNEG-401): a deleted note goes here, and stays until it is emptied.
//!
//! A note in Trash is the same row with `notes.trashed_at` set -- not a copy,
//! and not `deleted_at`, which is OpenWhispr's sync tombstone and refused by
//! the schema checks. It keeps everything it had: its recording and the file
//! behind it, its search index rows, its people, its speaker names. What
//! changes is that nothing finds it any more -- every list, search and count
//! in this crate asks for `trashed_at IS NULL` -- and that the edits the
//! screen makes -- title, notes, folder, people, speaker names, a summary
//! request -- refuse it until it is restored.
//! Restoring clears the column, so the note comes back exactly as it was,
//! search and all, with no re-indexing.
//!
//! Nothing here deletes anything. Deleting forever, one note or the whole
//! Trash, removes rows and files, so it lives with the files in
//! [`crate::recordings`]. Nothing empties Trash on its own.

use std::path::Path;

use kw_storage::StorageError;
use serde::Serialize;
use sqlx::{FromRow, SqlitePool};
use ts_rs::TS;

/// A note in Trash, as the Trash lists it.
#[derive(Debug, Clone, Serialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct TrashedNote {
    pub id: i64,
    pub title: String,
    /// When it was moved to Trash, as SQLite's `CURRENT_TIMESTAMP` writes it.
    pub trashed_at: String,
    /// Whether a recording is attached: it is deleted with the note.
    pub has_recording: bool,
    /// Recording duration, falling back to the note's meeting duration after
    /// intentional recording deletion. `None` when neither was measured.
    pub recording_seconds: Option<f64>,
    /// What the recording's file takes on disk, read from the file itself. 0
    /// with no recording, or one whose file is missing.
    pub recording_bytes: u64,
    /// The name of the file the recording was copied from, for an import. The
    /// original was never the app's and deleting forever doesn't touch it.
    pub original_filename: Option<String>,
}

/// Everything in Trash, most recently trashed first, and what their
/// recordings take on this computer altogether.
#[derive(Debug, Clone, Default, Serialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
pub struct TrashSummary {
    pub notes: Vec<TrashedNote>,
    pub recording_bytes: u64,
}

/// Move note `id` to Trash, and say whether it moved: `false` for a note that
/// doesn't exist or is already there.
///
/// A summary queued or being made for it is cancelled in the same
/// transaction, so nothing goes on working on a note that is gone from view.
/// `updated_at` is not touched: putting a note away is not working on it, and
/// a restored note comes back where it was in the library.
pub async fn trash(pool: &SqlitePool, id: i64) -> Result<bool, StorageError> {
    let mut tx = pool.begin().await?;
    let moved = sqlx::query(
        "UPDATE notes SET trashed_at = CURRENT_TIMESTAMP WHERE id = ? AND trashed_at IS NULL",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?
    .rows_affected()
        > 0;
    if moved {
        crate::summary::cancel_on(&mut tx, id).await?;
    }
    tx.commit().await?;
    Ok(moved)
}

/// Bring note `id` back from Trash, and say whether it came: `false` for a
/// note that doesn't exist or isn't in Trash.
///
/// It comes back as it went, except for a folder deleted while it was in
/// Trash: the folder's foreign key cleared the note's folder then, so it
/// comes back in none.
pub async fn restore(pool: &SqlitePool, id: i64) -> Result<bool, StorageError> {
    let result =
        sqlx::query("UPDATE notes SET trashed_at = NULL WHERE id = ? AND trashed_at IS NOT NULL")
            .bind(id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}

/// Whether note `id` exists and is in Trash, for a caller that was refused
/// and wants to say why.
pub async fn is_trashed(pool: &SqlitePool, id: i64) -> Result<bool, StorageError> {
    let found: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM notes WHERE id = ? AND trashed_at IS NOT NULL")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(found.is_some())
}

/// The row a [`TrashedNote`] is read from, before its file is measured.
#[derive(FromRow)]
struct TrashedRow {
    id: i64,
    title: String,
    trashed_at: String,
    storage_key: Option<String>,
    duration_seconds: Option<f64>,
    original_filename: Option<String>,
}

/// Everything in Trash, most recently trashed first, with what each
/// recording's file takes in `recordings` -- the application's recordings
/// directory.
///
/// Measured as folder sharing measures a recording (`sharing::preview`): the
/// stored name resolved inside the directory and nowhere else, and only a
/// regular file counted, so a missing file, a symlink or a name that would
/// reach outside takes nothing.
pub async fn list(pool: &SqlitePool, recordings: &Path) -> Result<TrashSummary, StorageError> {
    let rows = sqlx::query_as::<_, TrashedRow>(
        "SELECT n.id, n.title, n.trashed_at, r.storage_key, coalesce(r.duration_seconds,n.audio_duration_seconds) AS duration_seconds, r.original_filename
         FROM notes n LEFT JOIN note_recordings r ON r.note_id = n.id
         WHERE n.trashed_at IS NOT NULL
         ORDER BY n.trashed_at DESC, n.id DESC",
    )
    .fetch_all(pool)
    .await?;
    let mut summary = TrashSummary::default();
    for row in rows {
        let recording_bytes = match row.storage_key.as_deref() {
            Some(key) => file_bytes(recordings, key).await,
            None => 0,
        };
        summary.recording_bytes += recording_bytes;
        summary.notes.push(TrashedNote {
            id: row.id,
            title: row.title,
            trashed_at: row.trashed_at,
            has_recording: row.storage_key.is_some(),
            recording_seconds: row.duration_seconds,
            recording_bytes,
            original_filename: row.original_filename,
        });
    }
    Ok(summary)
}

/// The size of the regular file `key` names inside `directory`, or 0.
async fn file_bytes(directory: &Path, key: &str) -> u64 {
    let Some(path) = crate::recordings::resolve(directory, key) else {
        return 0;
    };
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata.len(),
        _ => 0,
    }
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
    use crate::notes::{self, NewNote, NoteUpdate};
    use crate::{people, recordings, sharing, speakers};
    use kw_storage::Db;

    async fn db() -> Db {
        Db::open_in_memory().await.unwrap()
    }

    const ANNA: &str = r#"[{"displayName":"Anna Lindqvist","email":"anna@example.com"}]"#;

    async fn note(pool: &SqlitePool, title: &str, content: &str, folder: Option<i64>) -> i64 {
        notes::create(
            pool,
            &NewNote {
                title: title.to_owned(),
                content: content.to_owned(),
                folder_id: folder,
                participants: Some(ANNA.to_owned()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
    }

    fn ids(listed: &[notes::Note]) -> Vec<i64> {
        listed.iter().map(|note| note.id).collect()
    }

    async fn found(pool: &SqlitePool, query: &str) -> Vec<i64> {
        notes::search(pool, query, 10)
            .await
            .unwrap()
            .into_iter()
            .map(|hit| hit.id)
            .collect()
    }

    async fn trashed_at(pool: &SqlitePool, id: i64, at: &str) {
        sqlx::query("UPDATE notes SET trashed_at = ? WHERE id = ?")
            .bind(at)
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }

    /// The whole round: in Trash a note is found by nothing that lists,
    /// searches or counts notes, `get` still has it and says when it went,
    /// and Restore brings it back to every one of them -- search included,
    /// with no re-indexing.
    #[tokio::test]
    async fn a_trashed_note_is_found_by_nothing_until_it_is_restored() {
        let db = db().await;
        let p = db.pool();
        let folder = notes::create_folder(p, "Customers").await.unwrap();
        let gone = note(p, "Warehouse", "the warehouse plan", Some(folder)).await;
        let kept = note(p, "Office", "the office plan", Some(folder)).await;

        assert!(trash(p, gone).await.unwrap());
        assert!(
            !trash(p, gone).await.unwrap(),
            "a note already in Trash doesn't move again"
        );
        assert!(
            !trash(p, 404).await.unwrap(),
            "nor does one that isn't there"
        );

        assert_eq!(ids(&notes::list(p, None, None, 10).await.unwrap()), [kept]);
        assert_eq!(
            ids(&notes::list(p, None, Some(folder), 10).await.unwrap()),
            [kept]
        );
        assert_eq!(
            ids(&notes::list(p, Some("personal"), None, 10).await.unwrap()),
            [kept]
        );
        assert!(found(p, "warehouse").await.is_empty());
        assert_eq!(found(p, "plan").await, [kept]);
        assert_eq!(people::people(p).await.unwrap()[0].note_count, 1);
        assert_eq!(
            ids(&people::list_by_person(p, "anna@example.com", 10)
                .await
                .unwrap()),
            [kept]
        );
        let in_trash = notes::get(p, gone).await.unwrap().unwrap();
        assert!(
            in_trash.trashed_at.is_some(),
            "get still has it, and says so"
        );
        assert!(is_trashed(p, gone).await.unwrap());
        assert!(!is_trashed(p, kept).await.unwrap());
        assert!(!is_trashed(p, 404).await.unwrap());

        assert!(restore(p, gone).await.unwrap());
        assert!(
            !restore(p, gone).await.unwrap(),
            "a note out of Trash isn't restored twice"
        );
        assert!(!restore(p, kept).await.unwrap(), "nor is a live one");
        let back = notes::get(p, gone).await.unwrap().unwrap();
        assert_eq!(back.trashed_at, None);
        assert_eq!(back.folder_id, Some(folder));
        assert_eq!(found(p, "warehouse").await, [gone]);
        let mut both = ids(&notes::list(p, None, None, 10).await.unwrap());
        both.sort_unstable();
        assert_eq!(both, [gone, kept]);
        assert_eq!(people::people(p).await.unwrap()[0].note_count, 2);
    }

    /// A transcript hit is a search hit like any other: in Trash, the note's
    /// transcript rows are kept but not searched.
    #[tokio::test]
    async fn a_trashed_meeting_is_not_found_by_its_transcript() {
        let db = db().await;
        let p = db.pool();
        let id = notes::create(
            p,
            &NewNote {
                title: "Weekly".into(),
                note_type: Some("meeting".into()),
                transcript: Some(r#"[{"text":"the budget was agreed","timestamp":0}]"#.into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(trash(p, id).await.unwrap());
        assert!(found(p, "budget").await.is_empty());
        assert!(restore(p, id).await.unwrap());
        let hits = notes::search(p, "budget", 10).await.unwrap();
        assert_eq!(hits[0].id, id);
        assert!(
            hits[0].transcript_row.is_some(),
            "its row is found again too"
        );
    }

    /// Nothing edits a note in Trash: each writer answers as for a note that
    /// isn't there, and writes nothing.
    #[tokio::test]
    async fn a_trashed_note_refuses_every_edit() {
        let db = db().await;
        let p = db.pool();
        let folder = notes::create_folder(p, "Work").await.unwrap();
        let id = note(p, "Kickoff", "as it was", None).await;
        assert!(trash(p, id).await.unwrap());
        let before = notes::get(p, id).await.unwrap().unwrap();

        assert!(!notes::update(
            p,
            id,
            &NoteUpdate {
                title: Some("Changed".into()),
                content: Some("changed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap());
        assert!(
            !notes::update(p, id, &NoteUpdate::default()).await.unwrap(),
            "an empty change says it isn't there, as for a missing note"
        );
        assert!(!notes::set_folder(p, id, Some(folder)).await.unwrap());
        let refused = people::set_participants(
            p,
            id,
            &[people::Participant {
                display_name: None,
                email: Some("maja@example.com".into()),
            }],
        )
        .await
        .unwrap();
        assert!(!refused, "a note in Trash keeps its people as they were");
        assert!(!speakers::rename(p, id, "speaker_0", "Anna").await.unwrap());

        assert_eq!(notes::get(p, id).await.unwrap().unwrap(), before);
        assert!(speakers::mappings(p, id).await.unwrap().is_empty());
    }

    /// A folder deleted while its note was in Trash takes its id with it, so
    /// the note is restored into no folder rather than one that is gone.
    #[tokio::test]
    async fn a_note_restored_after_its_folder_was_deleted_is_in_no_folder() {
        let db = db().await;
        let p = db.pool();
        let folder = notes::create_folder(p, "Temporary").await.unwrap();
        let id = note(p, "Filed", "", Some(folder)).await;
        assert!(trash(p, id).await.unwrap());
        assert!(notes::delete_folder(p, folder).await.unwrap());

        assert!(restore(p, id).await.unwrap());
        assert_eq!(notes::get(p, id).await.unwrap().unwrap().folder_id, None);
        assert_eq!(ids(&notes::list(p, None, None, 10).await.unwrap()), [id]);
    }

    /// Sharing a folder packs the notes the folder shows: one in Trash is in
    /// neither the preview nor a package, even when its id is asked for.
    #[tokio::test]
    async fn a_trashed_note_is_left_out_of_a_shared_folder() {
        let db = db().await;
        let p = db.pool();
        let dir = tempfile::tempdir().unwrap();
        let folder = notes::create_folder(p, "Project").await.unwrap();
        let gone = note(p, "Gone", "", Some(folder)).await;
        let kept = note(p, "Kept", "", Some(folder)).await;
        assert!(trash(p, gone).await.unwrap());

        let preview = sharing::preview(p, dir.path(), folder).await.unwrap();
        assert_eq!(
            preview.notes.iter().map(|n| n.id).collect::<Vec<_>>(),
            [kept]
        );
        let options = sharing::ShareOptions::default();
        assert_eq!(
            sharing::capture(p, dir.path(), folder, &[kept], options)
                .await
                .unwrap()
                .notes
                .len(),
            1
        );
        let refused = sharing::capture(p, dir.path(), folder, &[kept, gone], options)
            .await
            .unwrap_err();
        assert!(refused.contains("moved or deleted"), "{refused}");
    }

    /// Other notes' speaker names are offered while naming a speaker; a
    /// note's in Trash are not.
    #[tokio::test]
    async fn a_trashed_note_s_speaker_names_are_not_suggested() {
        let db = db().await;
        let p = db.pool();
        let gone = notes::create(p, &NewNote::default()).await.unwrap();
        let kept = notes::create(p, &NewNote::default()).await.unwrap();
        let target = notes::create(p, &NewNote::default()).await.unwrap();
        speakers::rename(p, gone, "speaker_0", "Erik")
            .await
            .unwrap();
        speakers::rename(p, kept, "speaker_0", "Maja")
            .await
            .unwrap();
        assert!(trash(p, gone).await.unwrap());

        assert_eq!(speakers::suggestions(p, target).await.unwrap(), ["Maja"]);
    }

    /// In Trash a note still owns its recording, so the file is still listed
    /// among the recordings notes own -- which is what keeps crash recovery
    /// from offering it as an interrupted meeting.
    #[tokio::test]
    async fn a_trashed_note_keeps_its_recording() {
        let db = db().await;
        let p = db.pool();
        let id = note(p, "Recorded", "", None).await;
        recordings::attach(p, id, "owned.wav", Some(12.0), None)
            .await
            .unwrap();
        assert!(trash(p, id).await.unwrap());

        assert_eq!(recordings::storage_keys(p).await.unwrap(), ["owned.wav"]);
        assert!(recordings::get(p, id).await.unwrap().is_some());
    }

    /// The Trash lists its notes most recently trashed first, each with its
    /// recording's length and what its file takes, measured on disk, and the
    /// total. A live note's recording is not counted, and a recording whose
    /// file is missing takes nothing.
    #[tokio::test]
    async fn the_trash_lists_its_notes_with_what_their_recordings_take() {
        let db = db().await;
        let p = db.pool();
        let dir = tempfile::tempdir().unwrap();
        let older = note(p, "Older", "", None).await;
        let newer = note(p, "Newer", "", None).await;
        let typed = note(p, "Typed", "", None).await;
        let missing = note(p, "Missing", "", None).await;
        let live = note(p, "Live", "", None).await;
        std::fs::write(dir.path().join("older.wav"), vec![0_u8; 1000]).unwrap();
        std::fs::write(dir.path().join("newer.wav"), vec![0_u8; 250]).unwrap();
        std::fs::write(dir.path().join("live.wav"), vec![0_u8; 5000]).unwrap();
        recordings::attach(p, older, "older.wav", Some(1860.0), Some("standup.m4a"))
            .await
            .unwrap();
        recordings::attach(p, newer, "newer.wav", None, None)
            .await
            .unwrap();
        recordings::attach(p, missing, "missing.wav", Some(30.0), None)
            .await
            .unwrap();
        recordings::attach(p, live, "live.wav", Some(60.0), None)
            .await
            .unwrap();
        for id in [older, newer, typed, missing] {
            assert!(trash(p, id).await.unwrap());
        }
        trashed_at(p, older, "2026-10-01 09:00:00").await;
        trashed_at(p, newer, "2026-10-03 09:00:00").await;
        trashed_at(p, typed, "2026-10-02 09:00:00").await;
        trashed_at(p, missing, "2026-09-30 09:00:00").await;

        let summary = list(p, dir.path()).await.unwrap();
        assert_eq!(
            summary.notes,
            [
                TrashedNote {
                    id: newer,
                    title: "Newer".into(),
                    trashed_at: "2026-10-03 09:00:00".into(),
                    has_recording: true,
                    recording_seconds: None,
                    recording_bytes: 250,
                    original_filename: None,
                },
                TrashedNote {
                    id: typed,
                    title: "Typed".into(),
                    trashed_at: "2026-10-02 09:00:00".into(),
                    has_recording: false,
                    recording_seconds: None,
                    recording_bytes: 0,
                    original_filename: None,
                },
                TrashedNote {
                    id: older,
                    title: "Older".into(),
                    trashed_at: "2026-10-01 09:00:00".into(),
                    has_recording: true,
                    recording_seconds: Some(1860.0),
                    recording_bytes: 1000,
                    original_filename: Some("standup.m4a".into()),
                },
                TrashedNote {
                    id: missing,
                    title: "Missing".into(),
                    trashed_at: "2026-09-30 09:00:00".into(),
                    has_recording: true,
                    recording_seconds: Some(30.0),
                    recording_bytes: 0,
                    original_filename: None,
                },
            ]
        );
        assert_eq!(summary.recording_bytes, 1250);
    }

    #[tokio::test]
    async fn an_empty_trash_lists_nothing() {
        let db = db().await;
        let dir = tempfile::tempdir().unwrap();
        note(db.pool(), "Live", "", None).await;
        assert_eq!(
            list(db.pool(), dir.path()).await.unwrap(),
            TrashSummary::default()
        );
    }

    /// The Trash's sizes come only from inside the recordings directory: a
    /// stored name that would reach outside it is measured as nothing.
    #[tokio::test]
    async fn a_recording_name_outside_the_directory_is_not_measured() {
        let db = db().await;
        let p = db.pool();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("recordings");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(dir.path().join("outside.wav"), vec![0_u8; 400]).unwrap();
        let id = note(p, "Odd", "", None).await;
        recordings::attach(p, id, "../outside.wav", None, None)
            .await
            .unwrap();
        assert!(trash(p, id).await.unwrap());

        let summary = list(p, &root).await.unwrap();
        assert_eq!(summary.notes[0].recording_bytes, 0);
        assert_eq!(summary.recording_bytes, 0);
    }

    #[tokio::test]
    async fn a_deleted_recording_keeps_the_meetings_duration_in_trash() {
        let db = db().await;
        let root = tempfile::tempdir().unwrap();
        let id = notes::create(
            db.pool(),
            &NewNote {
                audio_duration_seconds: Some(1850.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        trash(db.pool(), id).await.unwrap();
        let listed = list(db.pool(), root.path()).await.unwrap();
        assert_eq!(listed.notes[0].recording_seconds, Some(1850.0));
        assert!(!listed.notes[0].has_recording);
        assert_eq!(listed.recording_bytes, 0);
    }
}
