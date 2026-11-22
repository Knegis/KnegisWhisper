//! Adding speaker labels to a meeting note that is already saved.
//!
//! The rules -- when a job is queued, what settles it, when the summary and
//! the recording's deletion follow -- are in `kw_notes_core::speaker_labels`.
//! This is the thin half: it runs the diarizer over the note's recording,
//! writes the labels, and owns the three commands the note's status line uses.
//!
//! Stop and Recover used to do this before saving the note, so a process that
//! died during the minutes it takes left no note. Here the worst a failure can
//! cost is the labels, and the note says so and offers Retry.

use std::time::Duration;

use kw_meeting_core::diarization::Turn;
use kw_notes_core::speaker_labels::{self as jobs, Job, SpeakerLabelState};
use kw_notes_core::transcript::Segment;
use tauri::{AppHandle, Emitter, State};

use crate::state::AppState;

/// Emitted with a note's id whenever its speaker labels change state.
pub const EVENT: &str = "note-speaker-labels-changed";

/// Attach `turns` to `segments`, and say how many passages no voice covered.
///
/// A passage runs to its own end, or else to the next passage from the same
/// source, or else to `duration`: a live transcript records only where each
/// passage starts. A name somebody typed is left alone.
pub(crate) fn apply_turns(segments: &mut [Segment], turns: &[Turn], duration: f64) -> usize {
    let spans: Vec<(f64, f64)> = segments
        .iter()
        .enumerate()
        .map(|(index, saved)| {
            let start = saved.timestamp.unwrap_or_default();
            let end = saved
                .end_timestamp
                .or_else(|| {
                    segments
                        .iter()
                        .skip(index + 1)
                        .find(|next| next.source == saved.source)
                        .and_then(|next| next.timestamp)
                })
                .unwrap_or(duration)
                .max(start);
            (start, end)
        })
        .collect();
    let assigned = kw_meeting_core::diarization::assign(turns, &spans);

    let mut unattributed = 0_usize;
    for ((saved, speaker), (start, end)) in segments.iter_mut().zip(assigned).zip(spans) {
        saved.end_timestamp.get_or_insert(end);
        let Some(speaker) = speaker else {
            unattributed = unattributed.saturating_add(1);
            continue;
        };
        if saved.speaker_name.is_some() && !saved.speaker_is_placeholder {
            continue;
        }
        saved.speaker = Some(format!("speaker_{speaker}"));
        // A generated placeholder, and flagged as one so a name somebody
        // types later wins over it. `Speaker 1` is not a name and this is
        // the field that says so.
        saved.speaker_name = Some(
            Turn {
                start_seconds: start,
                end_seconds: end,
                speaker,
            }
            .label(),
        );
        saved.speaker_is_placeholder = true;
    }
    unattributed
}

/// Said on the note because a reader seeing most passages labelled and some
/// not would otherwise read the blanks as a person who said nothing.
pub(crate) fn unattributed_sentence(unattributed: usize, total: usize) -> String {
    format!(
        "{unattributed} of {total} passages carry no speaker: the diarization pass found no \
         voice covering them."
    )
}

/// The saved transcript with `turns` attached, and the sentence for the note
/// when some of it stayed unlabelled.
fn label(
    transcript: &str,
    turns: &[Turn],
    duration: f64,
) -> Result<(String, Option<String>), String> {
    let mut segments: Vec<Segment> = serde_json::from_str(transcript)
        .map_err(|_| "The saved transcript could not be read.".to_owned())?;
    let unattributed = apply_turns(&mut segments, turns, duration);
    let detail = (unattributed > 0).then(|| unattributed_sentence(unattributed, segments.len()));
    let labelled = serde_json::to_string(&segments).map_err(|e| e.to_string())?;
    Ok((labelled, detail))
}

/// Run the job waiting longest, one at a time, for as long as the application
/// runs. A job the last run left unfinished is marked interrupted first and
/// is not started again here.
pub fn start(app: AppHandle, state: AppState) {
    tauri::async_runtime::spawn(async move {
        if let Err(error) = jobs::recover(state.db().pool()).await {
            tracing::error!(%error, "could not recover speaker label jobs");
            return;
        }
        settle_removals(state.db(), state.recording_directory()).await;
        loop {
            match jobs::claim(state.db().pool()).await {
                Ok(Some(job)) => {
                    let _ = app.emit(EVENT, job.note_id);
                    // Dropping `run` kills the diarizer, so Continue without
                    // and Trash stop it rather than wait it out. Only an
                    // answer that the job is no longer this one stops it: a
                    // status that could not be read is not a cancellation,
                    // and treating it as one left the job marked as running
                    // with nothing running it.
                    let stopped = async {
                        loop {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            if stop_running(jobs::active(state.db().pool(), &job).await) {
                                break;
                            }
                        }
                    };
                    let result = tokio::select! {
                        result = run(&state, &job) => Some(result),
                        () = stopped => None,
                    };
                    if let Some(Err(error)) = result {
                        tracing::warn!(note_id = job.note_id, %error, "speaker labels were not added");
                        if let Err(error) =
                            jobs::fail(state.db().pool(), job.note_id, job.generation, &error).await
                        {
                            tracing::error!(%error, "could not record a failed speaker label job");
                        }
                    }
                    settle_removals(state.db(), state.recording_directory()).await;
                    let _ = app.emit(EVENT, job.note_id);
                    // Its summary may have been queued with it.
                    let _ = app.emit(crate::summaries::SUMMARY_EVENT, job.note_id);
                }
                Ok(None) => tokio::time::sleep(Duration::from_millis(500)).await,
                Err(error) => {
                    tracing::warn!(%error, "could not read the speaker label queue");
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
            }
        }
    });
}

/// Whether what was read of a running job says to stop it.
fn stop_running<E: std::fmt::Display>(active: Result<bool, E>) -> bool {
    match active {
        Ok(active) => !active,
        Err(error) => {
            tracing::warn!(%error, "could not read a speaker label job's status; leaving it running");
            false
        }
    }
}

async fn run(state: &AppState, job: &Job) -> Result<(), String> {
    let pool = state.db().pool();
    let before = kw_notes_core::notes::get(pool, job.note_id)
        .await
        .map_err(|e| e.to_string())?
        .and_then(|note| note.transcript)
        .ok_or("This note has no transcript to label.")?;
    let recording = kw_notes_core::recordings::get(pool, job.note_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("This note's recording is gone, and speaker labels are worked out from it.")?;
    let path =
        kw_notes_core::recordings::resolve(state.recording_directory(), &recording.storage_key)
            .ok_or("Invalid recording name.")?;
    if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
        return Err(format!(
            "The recording is no longer at {}, and speaker labels are worked out from it.",
            path.display()
        ));
    }
    let duration = recording.duration_seconds.unwrap_or_default();

    let turns = match crate::diarization::turns(&path, duration).await {
        Ok(Some(turns)) => turns,
        Ok(None) => {
            return Err(
                "The speaker label models or their runtime are not installed. Install them \
                 under Settings > Meetings, then retry."
                    .into(),
            )
        }
        Err(error) => return Err(format!("Speaker labels could not be produced: {error}")),
    };
    let (after, mut detail) = label(&before, &turns, duration)?;
    let samples = match crate::known_voices::extract(state.db(), &path, &turns, job.recognize).await
    {
        Ok(samples) => {
            if samples.warning.is_some() {
                detail.clone_from(&samples.warning);
            }
            samples
        }
        Err(error) => {
            detail = Some(format!(
                "Known voices could not run: {error}. Speaker labels were preserved."
            ));
            kw_notes_core::known_voices::Analysis::default()
        }
    };
    jobs::finish(pool, job, &before, &after, detail.as_deref(), &samples).await
}

/// Delete the recordings whose meetings asked for that and whose labels are
/// now out of the way. From here the deletion is `recording_state`'s: it is
/// durable there, and a failure shows on the note with its own Retry.
async fn settle_removals(db: &kw_storage::Db, directory: &std::path::Path) {
    let due = match jobs::removals_due(db.pool()).await {
        Ok(due) => due,
        Err(error) => {
            tracing::warn!(%error, "could not read which recordings to delete");
            return;
        }
    };
    for id in due {
        match kw_notes_core::recordings::get(db.pool(), id).await {
            Ok(Some(recording)) => {
                match kw_notes_core::recording_state::establish(
                    db.pool(),
                    id,
                    &recording.storage_key,
                    "delete-pending",
                )
                .await
                {
                    Ok(()) => {
                        if let Err(error) =
                            kw_notes_core::recording_state::retry(db.pool(), id, directory).await
                        {
                            tracing::warn!(note_id = id, %error, "could not delete the recording");
                        }
                    }
                    Err(error) => {
                        // Not handed over, so the request stays where it is
                        // and the next sweep tries again.
                        tracing::warn!(note_id = id, %error, "could not request the recording's deletion");
                        continue;
                    }
                }
            }
            // No recording left to delete: the request has nothing to act on.
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(note_id = id, %error, "could not read the note's recording");
                continue;
            }
        }
        if let Err(error) = jobs::removal_done(db.pool(), id).await {
            tracing::warn!(note_id = id, %error, "could not clear the recording's deletion request");
        }
    }
}

/* ------------------------------------------------------------------ commands */

/// What the note shows of its speaker labels; `None` when it never asked.
#[tauri::command]
pub async fn note_speaker_labels_get(
    state: State<'_, AppState>,
    id: i64,
) -> Result<Option<SpeakerLabelState>, String> {
    jobs::state(state.db().pool(), id).await
}

/// Try again, or add labels to a note that continued without them.
#[tauri::command]
pub async fn note_speaker_labels_retry(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<Option<SpeakerLabelState>, String> {
    jobs::retry(state.db().pool(), id).await?;
    let _ = app.emit(EVENT, id);
    jobs::state(state.db().pool(), id).await
}

/// Continue without speaker labels. A recording the meeting asked to delete
/// goes now, since nothing is waiting to read it.
#[tauri::command]
pub async fn note_speaker_labels_dismiss(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> Result<Option<SpeakerLabelState>, String> {
    jobs::dismiss(state.db().pool(), id).await?;
    settle_removals(state.db(), state.recording_directory()).await;
    let _ = app.emit(EVENT, id);
    let _ = app.emit(crate::summaries::SUMMARY_EVENT, id);
    jobs::state(state.db().pool(), id).await
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

    fn turn(start: f64, end: f64, speaker: u32) -> Turn {
        Turn {
            start_seconds: start,
            end_seconds: end,
            speaker,
        }
    }

    #[test]
    fn a_saved_transcript_gains_labels_and_keeps_everything_else() {
        let saved = r#"[{"text":"Hello.","timestamp":0.0,"endTimestamp":2.0,"words":[{"text":"Hello.","start":0.0,"end":1.0}]},{"text":"Hi.","timestamp":2.0,"endTimestamp":4.0}]"#;
        let (labelled, detail) =
            label(saved, &[turn(0.0, 2.0, 0), turn(2.0, 4.0, 1)], 4.0).unwrap();
        assert!(detail.is_none());
        let segments: Vec<Segment> = serde_json::from_str(&labelled).unwrap();
        assert_eq!(segments[0].speaker.as_deref(), Some("speaker_0"));
        assert_eq!(segments[1].speaker_name.as_deref(), Some("Speaker 2"));
        assert!(segments[1].speaker_is_placeholder);
        assert_eq!(segments[0].text, "Hello.");
        assert_eq!(segments[0].words.len(), 1);
    }

    #[test]
    fn a_live_transcript_s_passages_run_to_the_next_from_the_same_source() {
        // Only starts are saved for a live transcript. The microphone's first
        // passage runs to its second, not to the system passage between them.
        let saved = r#"[{"text":"a","timestamp":0.0,"source":"mic"},{"text":"b","timestamp":1.0,"source":"system"},{"text":"c","timestamp":6.0,"source":"mic"}]"#;
        let (labelled, detail) =
            label(saved, &[turn(2.0, 6.0, 0), turn(6.0, 9.0, 1)], 10.0).unwrap();
        let segments: Vec<Segment> = serde_json::from_str(&labelled).unwrap();
        assert_eq!(segments[0].end_timestamp, Some(6.0));
        assert_eq!(segments[0].speaker.as_deref(), Some("speaker_0"));
        assert_eq!(segments[2].end_timestamp, Some(10.0));
        assert_eq!(segments[2].speaker.as_deref(), Some("speaker_1"));
        assert!(detail.is_none());
    }

    #[test]
    fn passages_no_voice_covers_are_counted_and_a_typed_name_is_kept() {
        let saved = r#"[{"text":"a","timestamp":0.0,"endTimestamp":1.0,"speaker":"speaker_3","speakerName":"Axel"},{"text":"b","timestamp":5.0,"endTimestamp":6.0}]"#;
        let (labelled, detail) = label(saved, &[turn(0.0, 1.0, 0)], 6.0).unwrap();
        let segments: Vec<Segment> = serde_json::from_str(&labelled).unwrap();
        assert_eq!(segments[0].speaker_name.as_deref(), Some("Axel"));
        assert_eq!(segments[0].speaker.as_deref(), Some("speaker_3"));
        assert_eq!(segments[1].speaker, None);
        assert_eq!(detail.unwrap(), unattributed_sentence(1, 2));
    }

    #[test]
    fn a_transcript_that_cannot_be_read_is_an_error_not_an_empty_one() {
        assert!(label("not json", &[], 1.0).is_err());
    }

    /// The link the unit tests cannot reach: the installed diarizer, started
    /// the way the job starts it -- no window, bounded, with a deadline -- and
    /// its turns attached to a transcript. Needs the sherpa runtime and both
    /// speaker label models installed for this user, so it is run by hand:
    ///
    /// `cargo test -p knegiswhisper --lib --features tauri-test
    ///  the_installed_diarizer -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "needs the sherpa runtime and the speaker label models installed"]
    async fn the_installed_diarizer_labels_a_real_recording() {
        let wav = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../crates/transcription-core/tests/fixtures/speech-16k-mono.wav");
        let reader = hound::WavReader::open(&wav).unwrap();
        let duration = f64::from(reader.duration()) / f64::from(reader.spec().sample_rate);
        drop(reader);

        let turns = crate::diarization::turns(&wav, duration)
            .await
            .expect("the diarizer ran")
            .expect("the runtime and models are installed");
        println!("{duration:.1} s, {} turn(s): {turns:?}", turns.len());
        assert!(!turns.is_empty(), "the fixture is speech");

        let saved = format!(r#"[{{"text":"speech","timestamp":0.0,"endTimestamp":{duration}}}]"#);
        let (labelled, detail) = label(&saved, &turns, duration).unwrap();
        let segments: Vec<Segment> = serde_json::from_str(&labelled).unwrap();
        assert_eq!(segments[0].speaker.as_deref(), Some("speaker_0"));
        assert!(detail.is_none());
    }

    #[test]
    fn a_job_is_stopped_only_when_it_is_known_not_to_be_the_one_running() {
        assert!(stop_running(Ok::<_, sqlx::Error>(false)));
        assert!(!stop_running(Ok::<_, sqlx::Error>(true)));
        // A read that failed says nothing about the job.
        assert!(!stop_running(Err(sqlx::Error::PoolClosed)));
    }

    #[tokio::test]
    async fn a_deletion_that_cannot_be_handed_over_is_still_asked_for_afterwards() {
        let root = tempfile::tempdir().unwrap();
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        let wav = root.path().join("meeting.wav");
        tokio::fs::write(&wav, b"wav").await.unwrap();
        let id = kw_notes_core::notes::create_with_voices(
            db.pool(),
            &kw_notes_core::notes::NewNote {
                note_type: Some("meeting".into()),
                transcript: Some(r#"[{"text":"a","timestamp":0.0}]"#.into()),
                ..Default::default()
            },
            Some(("meeting.wav", 1.0)),
            &Default::default(),
            Some(jobs::Request {
                recognize: false,
                delete_recording: true,
            }),
        )
        .await
        .unwrap();
        jobs::dismiss(db.pool(), id).await.unwrap();
        sqlx::query("CREATE TRIGGER refuse_state BEFORE INSERT ON note_recording_state BEGIN SELECT RAISE(ABORT, 'fixture failure'); END")
            .execute(db.pool())
            .await
            .unwrap();

        settle_removals(&db, root.path()).await;
        assert!(wav.exists());
        assert_eq!(jobs::removals_due(db.pool()).await.unwrap(), vec![id]);

        // The database answers again: the same request is carried out.
        sqlx::query("DROP TRIGGER refuse_state")
            .execute(db.pool())
            .await
            .unwrap();
        settle_removals(&db, root.path()).await;
        assert!(!wav.exists());
        assert!(jobs::removals_due(db.pool()).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_recording_to_delete_goes_once_its_labels_are_dismissed_and_not_before() {
        let root = tempfile::tempdir().unwrap();
        let db = kw_storage::Db::open_in_memory().await.unwrap();
        let wav = root.path().join("meeting.wav");
        tokio::fs::write(&wav, b"wav").await.unwrap();
        let id = kw_notes_core::notes::create_with_voices(
            db.pool(),
            &kw_notes_core::notes::NewNote {
                note_type: Some("meeting".into()),
                transcript: Some(r#"[{"text":"a","timestamp":0.0}]"#.into()),
                ..Default::default()
            },
            Some(("meeting.wav", 1.0)),
            &Default::default(),
            Some(jobs::Request {
                recognize: false,
                delete_recording: true,
            }),
        )
        .await
        .unwrap();

        let job = jobs::claim(db.pool()).await.unwrap().unwrap();
        jobs::fail(db.pool(), id, job.generation, "The diarizer exited.")
            .await
            .unwrap();
        settle_removals(&db, root.path()).await;
        assert!(
            wav.exists(),
            "a failed job keeps the recording to retry from"
        );

        jobs::dismiss(db.pool(), id).await.unwrap();
        settle_removals(&db, root.path()).await;
        assert!(!wav.exists());
        let state = kw_notes_core::recording_state::get(db.pool(), id, root.path())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.status, "deleted");
        assert!(jobs::removals_due(db.pool()).await.unwrap().is_empty());
    }
}
