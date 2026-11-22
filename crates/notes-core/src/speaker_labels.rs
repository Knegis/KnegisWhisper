//! Speaker labels as a durable job on a note that is already saved.
//!
//! A meeting's note used to be written only after its speakers were analysed,
//! so anything that ended the application during that step -- minutes long on
//! an hour's recording -- left no note at all. The note is now saved first,
//! with its transcript unlabelled, and this job adds the labels afterwards.
//!
//! **A job that did not finish is never restarted on its own.** If analysing
//! the recording is what ended the application, starting it again at launch
//! would end it again. [`recover`] marks it interrupted and the note offers
//! Retry and Continue without.
//!
//! **The automatic summary waits for the job.** It reads the transcript and
//! names its speakers, so it is queued when the job settles -- succeeded,
//! failed, interrupted or dismissed -- and not with the note.
//!
//! **So does deleting the recording**, when the meeting asked for that: the
//! job reads it. `delete_recording` holds the request until the job succeeds
//! or is dismissed ([`removals_due`]). While the job is failed or interrupted
//! the recording stays, because it is the only way to try again.

use serde::Serialize;
use sqlx::{FromRow, SqliteConnection, SqlitePool};
use ts_rs::TS;

const INTERRUPTED: &str =
    "Speaker labels were interrupted before they finished. The note and its transcript are saved.";
const UNATTACHED: &str =
    "Speaker labels could not run: the recording is not attached to this note.";

/// What a meeting asks of the job when its note is saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Request {
    /// Whether known voices may be matched against the speakers found.
    pub recognize: bool,
    /// Delete the recording once the job succeeds or is dismissed.
    pub delete_recording: bool,
}

/// One claimed job.
#[derive(Debug, Clone, FromRow, PartialEq, Eq)]
pub struct Job {
    pub note_id: i64,
    pub generation: i64,
    pub recognize: bool,
}

/// What a note shows of its speaker labels. `None` for a note that never
/// asked for them.
#[derive(Debug, Clone, Serialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerLabelState {
    /// `queued`, `processing`, `succeeded`, `failed`, `interrupted` or
    /// `dismissed`.
    pub status: String,
    pub error: Option<String>,
    /// Whether the job can be started again: it is not running, the note is
    /// not in Trash, and its recording is still attached.
    pub can_retry: bool,
}

/// Queue the job for a note being created, in that note's transaction.
pub async fn enqueue_on(
    connection: &mut SqliteConnection,
    id: i64,
    request: Request,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO note_speaker_jobs(note_id,status,recognize,delete_recording) VALUES (?,'queued',?,?)",
    )
    .bind(id)
    .bind(request.recognize)
    .bind(request.delete_recording)
    .execute(connection)
    .await?;
    Ok(())
}

/// At launch: a job that was running is interrupted, and a queued one whose
/// recording was never attached cannot run. Both notes get their summary.
pub async fn recover(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let settled: Vec<i64> = sqlx::query_scalar(
        "UPDATE note_speaker_jobs SET
            error=CASE status WHEN 'processing' THEN ? ELSE ? END,
            status=CASE status WHEN 'processing' THEN 'interrupted' ELSE 'failed' END
         WHERE status='processing'
            OR (status='queued' AND NOT EXISTS(SELECT 1 FROM note_recordings r WHERE r.note_id=note_speaker_jobs.note_id))
         RETURNING note_id",
    )
    .bind(INTERRUPTED)
    .bind(UNATTACHED)
    .fetch_all(&mut *tx)
    .await?;
    for id in settled {
        crate::summary::enqueue_automatic(&mut tx, id).await?;
    }
    tx.commit().await
}

/// Start the oldest queued job whose note is not in Trash and has its
/// recording attached. Stop attaches the recording just after it saves the
/// note, so a job without one is waiting, not failed.
pub async fn claim(pool: &SqlitePool) -> Result<Option<Job>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE note_speaker_jobs SET status='processing', error=NULL
         WHERE note_id=(
            SELECT j.note_id FROM note_speaker_jobs j
            JOIN notes n ON n.id=j.note_id
            JOIN note_recordings r ON r.note_id=j.note_id
            WHERE j.status='queued' AND n.trashed_at IS NULL
            ORDER BY j.created_at,j.note_id LIMIT 1)
         RETURNING note_id,generation,recognize",
    )
    .fetch_optional(pool)
    .await
}

/// Whether `job` is still the one to finish: not dismissed, retried or
/// trashed since it was claimed.
pub async fn active(pool: &SqlitePool, job: &Job) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM note_speaker_jobs WHERE note_id=? AND generation=? AND status='processing')",
    )
    .bind(job.note_id)
    .bind(job.generation)
    .fetch_one(pool)
    .await
}

/// The job could not label the note. The note is as it was, and gets its
/// summary. Stop also calls this, with the job still queued, when the
/// recording could not be attached.
pub async fn fail(
    pool: &SqlitePool,
    id: i64,
    generation: i64,
    error: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let changed = sqlx::query(
        "UPDATE note_speaker_jobs SET status='failed',error=? WHERE note_id=? AND generation=? AND status IN ('queued','processing')",
    )
    .bind(error)
    .bind(id)
    .bind(generation)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if changed == 1 {
        crate::summary::enqueue_automatic(&mut tx, id).await?;
    }
    tx.commit().await
}

/// The recording could not be attached to the note Stop just saved.
pub async fn fail_unattached(pool: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
    fail(pool, id, 1, UNATTACHED).await
}

/// Write the labelled transcript and finish the job, together.
///
/// `before` is the transcript the labels were worked out from. A note whose
/// transcript has changed since is left alone: the labels belong to passages
/// that may no longer be there.
///
/// # Errors
///
/// A sentence for the note, when nothing was written.
pub async fn finish(
    pool: &SqlitePool,
    job: &Job,
    before: &str,
    after: &str,
    detail: Option<&str>,
    samples: &crate::known_voices::Analysis,
) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let changed = sqlx::query(
        "UPDATE note_speaker_jobs SET status='succeeded',error=NULL
         WHERE note_id=? AND generation=? AND status='processing'
           AND EXISTS(SELECT 1 FROM notes n WHERE n.id=note_speaker_jobs.note_id AND n.transcript=? AND n.trashed_at IS NULL)",
    )
    .bind(job.note_id)
    .bind(job.generation)
    .bind(before)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?
    .rows_affected();
    if changed != 1 {
        return Err(
            "The transcript changed while its speakers were analysed, so no labels were applied."
                .into(),
        );
    }
    sqlx::query("UPDATE notes SET transcript=?,updated_at=CURRENT_TIMESTAMP WHERE id=?")
        .bind(after)
        .bind(job.note_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(detail) = detail {
        sqlx::query(
            "UPDATE notes SET recording_details=CASE WHEN trim(COALESCE(recording_details,''))='' THEN ?1 ELSE recording_details || char(10) || char(10) || ?1 END WHERE id=?2",
        )
        .bind(format!("({detail})"))
        .bind(job.note_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    }
    if let Some(policy) = crate::known_voices::RELEASE_POLICY {
        crate::notes::persist_voices(&mut tx, job.note_id, samples, policy)
            .await
            .map_err(|e| e.to_string())?;
    }
    // A summary queued by an earlier failure has not started: it reads the
    // labelled transcript. Progress it saved on an earlier attempt was made
    // from the unlabelled one, and its steps are kept by position, not by
    // what they read, so it goes. One already made is shown as out of date.
    sqlx::query(
        "UPDATE note_summary_jobs SET source_transcript=?,chunks_json='{}',completed=0,total=0 WHERE note_id=? AND status='queued'",
    )
    .bind(after)
    .bind(job.note_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    crate::summary::enqueue_automatic(&mut tx, job.note_id)
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())
}

/// Queue the job again, or for the first time on a note saved without one.
///
/// # Errors
///
/// A sentence, when the note cannot be labelled as it is.
pub async fn retry(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    // Take the writer lock before reading what the decision rests on.
    sqlx::query("UPDATE note_speaker_jobs SET error=error WHERE note_id=?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    let (note_type, transcript, trashed, attached): (Option<String>, Option<String>, bool, bool) = sqlx::query_as(
        "SELECT note_type,transcript,trashed_at IS NOT NULL,EXISTS(SELECT 1 FROM note_recordings r WHERE r.note_id=notes.id) FROM notes WHERE id=?",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| e.to_string())?
    .ok_or("This note no longer exists.")?;
    if trashed {
        return Err("This note is in Trash. Restore it to add speaker labels.".into());
    }
    if note_type.as_deref() != Some("meeting") {
        return Err("Speaker labels are added to meeting notes only.".into());
    }
    if transcript.is_none_or(|t| t.trim().is_empty() || t.trim() == "[]") {
        return Err("This note has no transcript to label.".into());
    }
    if !attached {
        return Err(
            "This note's recording is gone, and speaker labels are worked out from it.".into(),
        );
    }
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM note_speaker_jobs WHERE note_id=?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    if status
        .as_deref()
        .is_some_and(|s| matches!(s, "queued" | "processing"))
    {
        return Err("Speaker labels are already being added.".into());
    }
    sqlx::query(
        "INSERT INTO note_speaker_jobs(note_id,status) VALUES (?,'queued')
         ON CONFLICT(note_id) DO UPDATE SET generation=generation+1,status='queued',error=NULL,created_at=CURRENT_TIMESTAMP",
    )
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())
}

/// Continue without speaker labels: stop a job that is waiting or running,
/// or put aside one that failed. The note gets its summary.
///
/// # Errors
///
/// A sentence, when there is nothing to dismiss.
pub async fn dismiss(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let changed = dismiss_on(&mut tx, id).await.map_err(|e| e.to_string())?;
    if !changed {
        return Err("This note has no speaker labels waiting.".into());
    }
    crate::summary::enqueue_automatic(&mut tx, id)
        .await
        .map_err(|e| e.to_string())?;
    tx.commit().await.map_err(|e| e.to_string())
}

async fn dismiss_on(connection: &mut SqliteConnection, id: i64) -> Result<bool, sqlx::Error> {
    Ok(sqlx::query(
        "UPDATE note_speaker_jobs SET status='dismissed',generation=generation+1,error=NULL
         WHERE note_id=? AND status IN ('queued','processing','failed','interrupted')",
    )
    .bind(id)
    .execute(connection)
    .await?
    .rows_affected()
        == 1)
}

/// Moving a note to Trash stops its job in the same transaction. Its
/// recording goes with the note when Trash is emptied, not before, so the
/// deletion the meeting asked for is dropped.
pub(crate) async fn trash_on(
    connection: &mut SqliteConnection,
    id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE note_speaker_jobs SET delete_recording=0 WHERE note_id=?")
        .bind(id)
        .execute(&mut *connection)
        .await?;
    sqlx::query(
        "UPDATE note_speaker_jobs SET status='dismissed',generation=generation+1,error=NULL
         WHERE note_id=? AND status IN ('queued','processing')",
    )
    .bind(id)
    .execute(connection)
    .await?;
    Ok(())
}

/// A note back from Trash gets the automatic summary its speaker labels
/// were holding back: Trash stopped the job without queueing one, since
/// nothing works on a note in Trash. Nothing here starts the labels again;
/// the note offers that. A note with no speaker job, or one still waiting,
/// is left as it is, and so is one that already has a summary job.
pub async fn restored(pool: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let settled: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM note_speaker_jobs WHERE note_id=? AND status NOT IN ('queued','processing')",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    if settled.is_some() {
        crate::summary::enqueue_automatic(&mut tx, id).await?;
    }
    tx.commit().await
}

pub async fn state(pool: &SqlitePool, id: i64) -> Result<Option<SpeakerLabelState>, String> {
    let row: Option<(String, Option<String>, bool)> = sqlx::query_as(
        "SELECT j.status,j.error,
                n.trashed_at IS NULL AND EXISTS(SELECT 1 FROM note_recordings r WHERE r.note_id=j.note_id)
         FROM note_speaker_jobs j JOIN notes n ON n.id=j.note_id WHERE j.note_id=?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(row.map(|(status, error, labellable)| SpeakerLabelState {
        can_retry: labellable && matches!(status.as_str(), "failed" | "interrupted" | "dismissed"),
        status,
        error,
    }))
}

/// Notes whose recording was to be deleted once saved, and whose job is now
/// out of the way. The request stays until [`removal_done`], so a deletion
/// that could not be handed over is tried again at the next sweep.
pub async fn removals_due(pool: &SqlitePool) -> Result<Vec<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT note_id FROM note_speaker_jobs WHERE delete_recording=1 AND status IN ('succeeded','dismissed') ORDER BY note_id",
    )
    .fetch_all(pool)
    .await
}

/// The deletion has been handed to `recording_state`, which holds it from here.
pub async fn removal_done(pool: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE note_speaker_jobs SET delete_recording=0 WHERE note_id=?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::notes::{self, NewNote};
    use crate::summary::{SummarySettings, SETTINGS_KEY};

    const PLAIN: &str = r#"[{"text":"Hello.","timestamp":0.0,"endTimestamp":2.0}]"#;
    const LABELLED: &str =
        r#"[{"text":"Hello.","timestamp":0.0,"endTimestamp":2.0,"speaker":"speaker_0"}]"#;
    const REQUEST: Request = Request {
        recognize: true,
        delete_recording: false,
    };

    async fn fixture(request: Request) -> (kw_storage::Db, i64) {
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        kw_storage::settings::set(
            db.pool(),
            SETTINGS_KEY,
            &SummarySettings {
                enabled: true,
                provider: "local".into(),
                model: "fixture".into(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let id = notes::create_with_voices(
            db.pool(),
            &NewNote {
                title: "Meeting".into(),
                note_type: Some("meeting".into()),
                transcript: Some(PLAIN.into()),
                ..Default::default()
            },
            Some(("meeting.wav", 2.0)),
            &crate::known_voices::Analysis::default(),
            Some(request),
        )
        .await
        .unwrap();
        (db, id)
    }

    async fn status(db: &kw_storage::Db, id: i64) -> String {
        state(db.pool(), id).await.unwrap().unwrap().status
    }

    async fn summary_jobs(db: &kw_storage::Db) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM note_summary_jobs")
            .fetch_one(db.pool())
            .await
            .unwrap()
    }

    async fn transcript(db: &kw_storage::Db, id: i64) -> String {
        notes::get(db.pool(), id)
            .await
            .unwrap()
            .unwrap()
            .transcript
            .unwrap()
    }

    #[tokio::test]
    async fn a_note_saved_with_a_job_waits_for_it_before_its_summary() {
        let (db, id) = fixture(REQUEST).await;
        assert_eq!(status(&db, id).await, "queued");
        assert_eq!(summary_jobs(&db).await, 0);
        assert_eq!(transcript(&db, id).await, PLAIN);

        let job = claim(db.pool()).await.unwrap().unwrap();
        assert_eq!((job.note_id, job.recognize), (id, true));
        assert!(claim(db.pool()).await.unwrap().is_none());
        finish(
            db.pool(),
            &job,
            PLAIN,
            LABELLED,
            Some("1 of 2 passages carry no speaker."),
            &crate::known_voices::Analysis::default(),
        )
        .await
        .unwrap();

        assert_eq!(status(&db, id).await, "succeeded");
        assert_eq!(transcript(&db, id).await, LABELLED);
        let (source, details): (String, String) = sqlx::query_as(
            "SELECT j.source_transcript,n.recording_details FROM note_summary_jobs j JOIN notes n ON n.id=j.note_id",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(source, LABELLED);
        assert_eq!(details, "(1 of 2 passages carry no speaker.)");
    }

    #[tokio::test]
    async fn a_job_running_when_the_application_ended_is_interrupted_and_not_restarted() {
        let (db, id) = fixture(REQUEST).await;
        claim(db.pool()).await.unwrap().unwrap();

        recover(db.pool()).await.unwrap();

        let found = state(db.pool(), id).await.unwrap().unwrap();
        assert_eq!(found.status, "interrupted");
        assert!(found.can_retry);
        assert!(found.error.unwrap().contains("saved"));
        assert!(claim(db.pool()).await.unwrap().is_none());
        // The note is whole without labels, so it is summarised as it is.
        assert_eq!(summary_jobs(&db).await, 1);
        assert_eq!(transcript(&db, id).await, PLAIN);
    }

    #[tokio::test]
    async fn a_failure_keeps_the_note_and_a_retry_labels_it() {
        let (db, id) = fixture(REQUEST).await;
        let job = claim(db.pool()).await.unwrap().unwrap();
        fail(db.pool(), id, job.generation, "The diarizer exited.")
            .await
            .unwrap();
        let found = state(db.pool(), id).await.unwrap().unwrap();
        assert_eq!(
            (
                found.status.as_str(),
                found.error.as_deref(),
                found.can_retry
            ),
            ("failed", Some("The diarizer exited."), true)
        );
        assert_eq!(summary_jobs(&db).await, 1);

        retry(db.pool(), id).await.unwrap();
        assert!(retry(db.pool(), id).await.is_err(), "already queued");
        let again = claim(db.pool()).await.unwrap().unwrap();
        // The first run's late answer is not this run's.
        assert!(!active(db.pool(), &job).await.unwrap());
        assert!(
            finish(db.pool(), &job, PLAIN, LABELLED, None, &Default::default())
                .await
                .is_err()
        );
        finish(
            db.pool(),
            &again,
            PLAIN,
            LABELLED,
            None,
            &Default::default(),
        )
        .await
        .unwrap();
        assert_eq!(transcript(&db, id).await, LABELLED);
        // The summary queued by the failure had not started: it reads the labels.
        let source: String = sqlx::query_scalar("SELECT source_transcript FROM note_summary_jobs")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(source, LABELLED);
    }

    #[tokio::test]
    async fn labels_worked_out_from_a_transcript_that_changed_are_not_applied() {
        let (db, id) = fixture(REQUEST).await;
        let job = claim(db.pool()).await.unwrap().unwrap();
        sqlx::query("UPDATE notes SET transcript='[]' WHERE id=?")
            .bind(id)
            .execute(db.pool())
            .await
            .unwrap();
        let error = finish(db.pool(), &job, PLAIN, LABELLED, None, &Default::default())
            .await
            .unwrap_err();
        assert!(error.contains("changed"));
        assert_eq!(transcript(&db, id).await, "[]");
        assert_eq!(status(&db, id).await, "processing");
    }

    #[tokio::test]
    async fn continuing_without_labels_stops_the_job_and_summarises_the_note() {
        let (db, id) = fixture(REQUEST).await;
        let job = claim(db.pool()).await.unwrap().unwrap();
        dismiss(db.pool(), id).await.unwrap();
        assert_eq!(status(&db, id).await, "dismissed");
        assert!(!active(db.pool(), &job).await.unwrap());
        assert_eq!(summary_jobs(&db).await, 1);
        assert!(dismiss(db.pool(), id).await.is_err());
        // Labels can still be asked for while the recording is there.
        retry(db.pool(), id).await.unwrap();
        assert_eq!(status(&db, id).await, "queued");
    }

    #[tokio::test]
    async fn the_recording_is_deleted_only_once_the_job_is_out_of_the_way() {
        let (db, id) = fixture(Request {
            recognize: false,
            delete_recording: true,
        })
        .await;
        assert!(removals_due(db.pool()).await.unwrap().is_empty());
        let job = claim(db.pool()).await.unwrap().unwrap();
        fail(db.pool(), id, job.generation, "failed").await.unwrap();
        assert!(
            removals_due(db.pool()).await.unwrap().is_empty(),
            "a failed job keeps the recording it would be retried from"
        );
        dismiss(db.pool(), id).await.unwrap();
        assert_eq!(removals_due(db.pool()).await.unwrap(), vec![id]);
        removal_done(db.pool(), id).await.unwrap();
        assert!(removals_due(db.pool()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_job_waits_for_its_recording_and_fails_at_launch_without_one() {
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        let id = notes::create_with_voices(
            db.pool(),
            &NewNote {
                note_type: Some("meeting".into()),
                transcript: Some(PLAIN.into()),
                ..Default::default()
            },
            None,
            &Default::default(),
            Some(REQUEST),
        )
        .await
        .unwrap();
        assert!(claim(db.pool()).await.unwrap().is_none());
        assert_eq!(status(&db, id).await, "queued");
        assert!(retry(db.pool(), id).await.is_err());

        recover(db.pool()).await.unwrap();
        let found = state(db.pool(), id).await.unwrap().unwrap();
        assert_eq!(found.status, "failed");
        assert!(!found.can_retry);

        crate::recordings::attach(db.pool(), id, "meeting.wav", Some(2.0), None)
            .await
            .unwrap();
        retry(db.pool(), id).await.unwrap();
        assert_eq!(claim(db.pool()).await.unwrap().unwrap().note_id, id);
    }

    #[tokio::test]
    async fn a_note_in_trash_is_not_labelled_and_keeps_its_recording() {
        let (db, id) = fixture(Request {
            recognize: false,
            delete_recording: true,
        })
        .await;
        crate::trash::trash(db.pool(), id).await.unwrap();
        assert!(claim(db.pool()).await.unwrap().is_none());
        assert_eq!(status(&db, id).await, "dismissed");
        assert!(removals_due(db.pool()).await.unwrap().is_empty());
        assert!(retry(db.pool(), id).await.is_err());
        assert_eq!(summary_jobs(&db).await, 0);
    }

    #[tokio::test]
    async fn a_note_restored_from_trash_gets_the_summary_its_labels_held_back() {
        let (db, id) = fixture(REQUEST).await;
        crate::trash::trash(db.pool(), id).await.unwrap();
        assert_eq!(summary_jobs(&db).await, 0);

        assert!(crate::trash::restore(db.pool(), id).await.unwrap());

        assert_eq!(summary_jobs(&db).await, 1);
        // The labels are not started again; the note offers them.
        let found = state(db.pool(), id).await.unwrap().unwrap();
        assert_eq!(
            (found.status.as_str(), found.can_retry),
            ("dismissed", true)
        );
        assert!(claim(db.pool()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_restore_still_restores_when_its_summary_cannot_be_queued() {
        let (db, id) = fixture(REQUEST).await;
        crate::trash::trash(db.pool(), id).await.unwrap();
        sqlx::query("CREATE TRIGGER refuse_summary BEFORE INSERT ON note_summary_jobs BEGIN SELECT RAISE(ABORT, 'fixture failure'); END")
            .execute(db.pool())
            .await
            .unwrap();

        assert!(crate::trash::restore(db.pool(), id).await.unwrap());

        assert!(!crate::trash::is_trashed(db.pool(), id).await.unwrap());
        assert_eq!(summary_jobs(&db).await, 0);
    }

    #[tokio::test]
    async fn a_restore_with_summaries_off_or_no_speaker_job_queues_nothing() {
        // Off: the fixture's switch is turned off before the note comes back.
        let (db, id) = fixture(REQUEST).await;
        crate::trash::trash(db.pool(), id).await.unwrap();
        kw_storage::settings::set(db.pool(), SETTINGS_KEY, &SummarySettings::default())
            .await
            .unwrap();
        assert!(crate::trash::restore(db.pool(), id).await.unwrap());
        assert_eq!(summary_jobs(&db).await, 0);

        // No speaker job: a summary Trash cancelled stays cancelled, as before.
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        let plain = notes::create(db.pool(), &NewNote::default()).await.unwrap();
        crate::trash::trash(db.pool(), plain).await.unwrap();
        assert!(crate::trash::restore(db.pool(), plain).await.unwrap());
        assert_eq!(summary_jobs(&db).await, 0);
    }

    #[tokio::test]
    async fn labels_drop_the_progress_a_waiting_summary_made_without_them() {
        let (db, id) = fixture(REQUEST).await;
        let job = claim(db.pool()).await.unwrap().unwrap();
        fail(db.pool(), id, job.generation, "failed").await.unwrap();
        // The summary the failure queued was tried once and saved a step.
        sqlx::query(
            "UPDATE note_summary_jobs SET chunks_json='{\"extract-0\":{}}',completed=1,total=3",
        )
        .execute(db.pool())
        .await
        .unwrap();

        retry(db.pool(), id).await.unwrap();
        let again = claim(db.pool()).await.unwrap().unwrap();
        finish(
            db.pool(),
            &again,
            PLAIN,
            LABELLED,
            None,
            &Default::default(),
        )
        .await
        .unwrap();

        let (source, chunks, completed, total): (String, String, i64, i64) = sqlx::query_as(
            "SELECT source_transcript,chunks_json,completed,total FROM note_summary_jobs",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(
            (source.as_str(), chunks.as_str(), completed, total),
            (LABELLED, "{}", 0, 0)
        );
    }

    #[tokio::test]
    async fn a_recording_s_deletion_stays_asked_for_until_it_is_handed_over() {
        let (db, id) = fixture(Request {
            recognize: false,
            delete_recording: true,
        })
        .await;
        let job = claim(db.pool()).await.unwrap().unwrap();
        finish(db.pool(), &job, PLAIN, LABELLED, None, &Default::default())
            .await
            .unwrap();
        // Reading what is due does not use it up: only the hand-over does.
        assert_eq!(removals_due(db.pool()).await.unwrap(), vec![id]);
        assert_eq!(removals_due(db.pool()).await.unwrap(), vec![id]);
        removal_done(db.pool(), id).await.unwrap();
        assert!(removals_due(db.pool()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn labels_can_be_added_to_a_meeting_saved_without_a_job() {
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        let id = notes::create_with_voices(
            db.pool(),
            &NewNote {
                note_type: Some("meeting".into()),
                transcript: Some(PLAIN.into()),
                ..Default::default()
            },
            Some(("meeting.wav", 2.0)),
            &Default::default(),
            None,
        )
        .await
        .unwrap();
        assert!(state(db.pool(), id).await.unwrap().is_none());
        retry(db.pool(), id).await.unwrap();
        let job = claim(db.pool()).await.unwrap().unwrap();
        assert!(!job.recognize);
    }
}
