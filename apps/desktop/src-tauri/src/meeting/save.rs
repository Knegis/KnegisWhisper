//! Compose and save stopped, discarded, and recovered meetings.
use super::*;

/// What a finished meeting's note takes from its session, besides the
/// transcript: the title, the plan it was started with, and what was typed.
#[derive(Debug)]
pub(super) struct SaveAs {
    pub(super) title: String,
    pub(super) plan: MeetingPlan,
    pub(super) notes: String,
    /// The model chosen for this meeting, for its whole-recording pass.
    pub(super) whole_pass_model: Option<String>,
}

/// Save a meeting's note in the folder its plan chose: the one place every
/// way a meeting is saved creates its note (KNEG-365). The second value is
/// whether the chosen folder was left out because it no longer exists.
///
/// **The save never fails because of the folder.** `notes.folder_id`
/// references `folders`, and the foreign key is enforced, so a folder deleted
/// since Start would refuse the note outright. The folder is looked up first,
/// and one deleted between that lookup and the insert is the same refusal a
/// step later, so the insert is tried once more with no folder.
///
/// `recording` saves the note and attaches its WAV in one transaction, as
/// recovery needs (`create_meeting_with_recording`). Stop attaches after the
/// note is saved instead, so a failed attachment never costs the note.
pub(super) async fn create_meeting_note(
    db: &Db,
    note: kw_notes_core::notes::NewNote,
    plan: &MeetingPlan,
    recording: Option<(&str, f64)>,
) -> Result<(i64, bool), kw_storage::StorageError> {
    create_meeting_note_labelling(db, note, plan, recording, None).await
}

/// [`create_meeting_note`], queueing the note's speaker labels with it when
/// `speakers` asks for them (`kw_notes_core::speaker_labels`).
pub(super) async fn create_meeting_note_labelling(
    db: &Db,
    mut note: kw_notes_core::notes::NewNote,
    plan: &MeetingPlan,
    recording: Option<(&str, f64)>,
    speakers: Option<kw_notes_core::speaker_labels::Request>,
) -> Result<(i64, bool), kw_storage::StorageError> {
    // Recognition results are saved by the speaker-label job, not here.
    let samples = &kw_notes_core::known_voices::Analysis::default();
    note.calendar_event_id = plan.calendar.as_ref().map(|event| event.event_id.clone());
    if note.participants.is_none() {
        if let Some(event) = plan
            .calendar
            .as_ref()
            .filter(|event| !event.participants.is_empty())
        {
            // Participants contain only optional strings, so encoding has no
            // fallible provider-specific values, just as for the transcript.
            note.participants = serde_json::to_string(&event.participants).ok();
        }
    }
    note.folder_id = match kw_notes_core::notes::existing_folder(db.pool(), plan.folder_id).await {
        Ok(found) => found,
        // Unknown rather than gone: the insert below says which.
        Err(error) => {
            tracing::warn!(%error, "could not look up the meeting's folder");
            plan.folder_id
        }
    };
    let id = match kw_notes_core::notes::create_with_voices(
        db.pool(),
        &note,
        recording,
        samples,
        speakers,
    )
    .await
    {
        Err(error) if note.folder_id.is_some() && error.is_foreign_key_violation() => {
            tracing::warn!(%error, "the meeting's folder was deleted as its note was saved");
            note.folder_id = None;
            kw_notes_core::notes::create_with_voices(db.pool(), &note, recording, samples, speakers)
                .await?
        }
        result => result?,
    };
    Ok((id, plan.folder_id.is_some() && note.folder_id.is_none()))
}

/// A stopped meeting's note, and what is to become of its recording: what
/// `keep` saves once its transcript is chosen, apart so it can be tested
/// without a running meeting. `gaps` is the live transcript's
/// ([`MeetingTranscript::gaps`]), `live_failure` why the live transcription
/// stopped, when it did, and `made_by` where `saved` was transcribed. `None`
/// when there is nothing to keep.
pub(super) fn compose_stopped(
    save: &SaveAs,
    finished: &Finished,
    saved: &SavedTranscript,
    gaps: u32,
    live_failure: Option<&str>,
    made_by: Option<&kw_notes_core::TranscribedBy>,
) -> Option<(kw_notes_core::notes::NewNote, Option<RecordingRemoval>)> {
    let segments = &saved.segments;
    let from_partials = saved.from_partials;
    let mut recording_details = String::new();
    if from_partials && !segments.is_empty() {
        recording_details.push_str(
            "\n\n(The whole-recording pass did not complete, so this is the running \
             transcript from during the meeting rather than a single pass over the \
             finished audio.)",
        );
    }
    if let Some(reason) = live_failure.filter(|_| !segments.is_empty()) {
        // Said in the note rather than only in a log, for the same reason
        // `from_partials` is: a transcript that came about differently from
        // how the reader expects should say so. This one is the good case --
        // the whole-recording pass ran afterwards over the complete audio,
        // so nothing is missing from what follows.
        let recovery = if from_partials {
            "The whole-recording recovery also failed; the saved live transcript may be incomplete."
        } else {
            "This note is the whole-recording pass, which ran afterwards over the complete audio."
        };
        recording_details.push_str(&format!(
            "\n\n(The live transcription stopped early: {reason} {recovery})"
        ));
    }
    if let Some(note) = &saved.speaker_note {
        // Only ever says something went differently from what the setting
        // asked for. A run that produced labels adds nothing here -- the
        // labels are visible in the note itself, and a sentence announcing
        // that a feature worked is noise.
        recording_details.push_str(&format!("\n\n({note})"));
    }
    if gaps > 0 && from_partials {
        // Two causes, and `MeetingTranscript::gaps` is one number that does
        // not distinguish them: the local model falling behind the meeting,
        // or a session past the limit on how much transcript is retained.
        // Naming both is honest where naming one was accurate until
        // KNEG-109 gave the count a second source.
        recording_details.push_str(&format!(
            "\n\n({gaps} stretch(es) of audio were never transcribed: either the transcriber \
             could not keep up with the meeting, or the meeting ran past the limit on how \
             much transcript one session keeps.)"
        ));
    }
    append_delivery_facts(
        &mut recording_details,
        finished.mic_delivered,
        finished.system_delivered,
    );

    if segments.is_empty() {
        recording_details.push_str("\n\nNothing was transcribed from the recording; this note holds only what was typed during the meeting.");
    }

    let removal = removal::recording_removal(
        save.plan.delete_recording,
        !segments.is_empty(),
        from_partials,
    );

    let note = compose_note(
        save.title.clone(),
        save.notes.as_str(),
        segments,
        recording_details.trim(),
        finished.duration_seconds,
        made_by,
    )?;
    Some((note, removal))
}

/// What a stopped meeting's save made of its note and its recording.
#[derive(Debug)]
pub(super) struct Stopped {
    pub(super) note_id: i64,
    pub(super) folder_missing: bool,
    pub(super) recording_attached: bool,
    pub(super) recording_removal: Option<RecordingRemoval>,
    pub(super) recording_removal_error: Option<String>,
    /// Speaker labels are queued for the note. A recording that was to be
    /// deleted is deleted once they are done, so `recording_removal` is
    /// `None` until then.
    pub(super) speaker_labels_pending: bool,
}

/// Save a stopped meeting's note, attach its recording, then tidy beside it:
/// the tail of `keep`, apart so it can be tested over a temporary directory.
///
/// In that order, and each step only after the one before it worked. The
/// typed notes' sidecar goes once the note holds them. The plan goes only
/// once the recording is attached: a recording left with no note is offered
/// under Interrupted, and its plan names, files and deletes the note
/// recovered from it. Deletion (`removal`) comes last, after the attachment,
/// so a crash before it leaves a recording a note owns rather than one
/// offered for recovery a second time.
///
/// With `label_speakers`, the note's speaker labels are queued with it and
/// the deletion waits for them: they are worked out from the recording.
#[cfg(test)]
pub(super) async fn save_stopped(
    db: &Db,
    note: kw_notes_core::notes::NewNote,
    plan: &MeetingPlan,
    recording: &std::path::Path,
    duration_seconds: f64,
    removal: Option<RecordingRemoval>,
) -> Result<Stopped, kw_storage::StorageError> {
    save_stopped_labelling(db, note, plan, recording, duration_seconds, removal, false).await
}

pub(super) async fn save_stopped_labelling(
    db: &Db,
    note: kw_notes_core::notes::NewNote,
    plan: &MeetingPlan,
    recording: &std::path::Path,
    duration_seconds: f64,
    removal: Option<RecordingRemoval>,
    label_speakers: bool,
) -> Result<Stopped, kw_storage::StorageError> {
    let speakers = label_speakers.then_some(kw_notes_core::speaker_labels::Request {
        recognize: true,
        delete_recording: removal == Some(RecordingRemoval::Deleted),
    });
    let (note_id, folder_missing) =
        create_meeting_note_labelling(db, note, plan, None, speakers).await?;
    let recording_attached =
        attach_meeting_recording(db, note_id, recording, duration_seconds).await;
    let (recording_removal, recording_removal_error) = if recording_attached {
        draft::remove_sidecars(recording);
        if speakers.is_some() {
            // The job holds the deletion until it has read the recording.
            (None, None)
        } else {
            removal::remove_recording(db, note_id, recording, removal).await
        }
    } else {
        draft::remove_sidecar(recording);
        if speakers.is_some() {
            // Nothing to work the labels out from: say so on the note now,
            // rather than leave it waiting for a recording it will not get.
            if let Err(error) =
                kw_notes_core::speaker_labels::fail_unattached(db.pool(), note_id).await
            {
                tracing::warn!(%error, note_id, "could not settle the note's speaker labels");
            }
        }
        (None, None)
    };
    Ok(Stopped {
        note_id,
        folder_missing,
        recording_attached,
        recording_removal,
        recording_removal_error,
        speaker_labels_pending: speakers.is_some() && recording_attached,
    })
}

/// Keep what was typed in a discarded meeting as a note of its own, named as
/// the meeting was or as its notes, and say which note, if one was saved. The
/// recorder has already deleted the recording, so the plan describes nothing
/// once this is done; the typed notes' sidecar goes only once the note holds
/// them. A folder deleted since Start is left out without a word: only Stop's
/// and Recover's outcomes say so (docs/NOTES_WORKSPACE.md).
pub(super) async fn save_discarded(
    db: &Db,
    recording: &std::path::Path,
    notes: &str,
    plan: &MeetingPlan,
    started_at: chrono::DateTime<chrono::Local>,
) -> Option<i64> {
    if notes.trim().is_empty() {
        draft::remove_sidecars(recording);
        return None;
    }
    let note = kw_notes_core::notes::NewNote {
        calendar_event_id: None,
        title: notes_only_title(plan.title.as_deref(), started_at),
        content: notes.trim_end().to_owned(),
        recording_details: Some("The recording was discarded before it was transcribed.".into()),
        note_type: Some("meeting".into()),
        folder_id: None,
        transcript: None,
        participants: None,
        audio_duration_seconds: None,
        transcribed_by: None,
    };
    match create_meeting_note(db, note, plan, None).await {
        Ok((id, _)) => {
            draft::remove_sidecars(recording);
            Some(id)
        }
        Err(error) => {
            tracing::error!(%error, "could not save typed notes from cancelled meeting");
            // The notes stay on disk, deliberately.
            draft::remove_plan(recording);
            None
        }
    }
}

/// Save a recovered meeting, named, filed and kept or deleted by the plan
/// beside its recording, then remove both sidecars. With no plan, the
/// recording is kept or deleted by today's setting. Its title falls back to
/// when the recording started, read from its name, and to now when the name
/// says nothing.
#[cfg(test)]
pub(super) async fn save_recovered(
    db: &Db,
    recording: &std::path::Path,
    storage_key: &str,
    notes: &str,
    segments: &[kw_notes_core::transcript::Segment],
    details: &str,
    duration: f64,
) -> Result<MeetingOutcome, String> {
    save_recovered_labelling(
        db,
        recording,
        storage_key,
        notes,
        segments,
        details,
        duration,
        false,
    )
    .await
}

// Keeps the existing recovery inputs together with whether to label speakers.
#[allow(clippy::too_many_arguments)]
pub(super) async fn save_recovered_labelling(
    db: &Db,
    recording: &std::path::Path,
    storage_key: &str,
    notes: &str,
    segments: &[kw_notes_core::transcript::Segment],
    details: &str,
    duration: f64,
    label_speakers: bool,
) -> Result<MeetingOutcome, String> {
    let plan = match draft::read_plan(recording) {
        Some(plan) => plan,
        None => MeetingPlan {
            delete_recording: delete_recordings(db).await,
            ..MeetingPlan::default()
        },
    };
    let started = recording_started_at(storage_key).unwrap_or_else(chrono::Local::now);
    // Recovery's transcript is always the whole-recording pass, over all the
    // audio there is: never the running one.
    let removal = removal::recording_removal(plan.delete_recording, !segments.is_empty(), false);
    let details = details.to_owned();
    // That pass is this computer's.
    let note = compose_note(
        note_title(plan.title.as_deref(), started),
        notes,
        segments,
        &details,
        duration,
        crate::transcribed_by::with_live_provider("local", plan.live_provider_id.as_deref())
            .as_ref(),
    )
    .ok_or("The recovered meeting had no content")?;
    // Known voices are not matched on a recovered meeting, as before.
    let speakers = label_speakers.then_some(kw_notes_core::speaker_labels::Request {
        recognize: false,
        delete_recording: removal == Some(RecordingRemoval::Deleted),
    });
    let (note_id, folder_missing) =
        create_meeting_note_labelling(db, note, &plan, Some((storage_key, duration)), speakers)
            .await
            .map_err(|error| {
                format!(
                    "Could not save the recovered meeting: {error}. The recording remains at {} and typed notes remain at {}",
                    recording.display(),
                    draft::sidecar_path(recording).display()
                )
            })?;
    draft::remove_sidecars(recording);
    let (recording_removal, recording_removal_error) = if speakers.is_some() {
        // The job holds the deletion until it has read the recording.
        (None, None)
    } else {
        removal::remove_recording(db, note_id, recording, removal).await
    };
    Ok(MeetingOutcome::Saved {
        note_id,
        speaker_labels_pending: speakers.is_some(),
        duration_seconds: duration,
        recording_path: recording.display().to_string(),
        recording_attached: true,
        gaps: 0,
        reconnects: 0,
        transcript_is_partials: false,
        folder_missing,
        recording_removal,
        recording_removal_error,
    })
}

/// A meeting's note, or `None` when there is nothing to keep. `transcribed_by`
/// is where `segments` were made (KNEG-368), and is kept only with them: a
/// note of typed notes alone has no transcript to say it of.
pub(super) fn compose_note(
    title: String,
    notes: &str,
    segments: &[kw_notes_core::transcript::Segment],
    recording_details: &str,
    duration_seconds: f64,
    transcribed_by: Option<&kw_notes_core::TranscribedBy>,
) -> Option<kw_notes_core::notes::NewNote> {
    if notes.trim().is_empty() && segments.is_empty() {
        return None;
    }
    let transcribed = !segments.is_empty();
    Some(kw_notes_core::notes::NewNote {
        calendar_event_id: None,
        title,
        content: notes.trim_end().to_owned(),
        recording_details: Some(recording_details.to_owned()),
        note_type: Some("meeting".into()),
        folder_id: None,
        transcript: transcribed
            .then(|| serde_json::to_string(segments).ok())
            .flatten(),
        participants: None,
        audio_duration_seconds: Some(duration_seconds),
        transcribed_by: transcribed_by
            .filter(|_| transcribed)
            .and_then(kw_notes_core::TranscribedBy::to_json),
    })
}

/// Preserve what each capture delivered in the note that outlives the session.
///
/// The whole-recording pass reads a mixed WAV, so its segments cannot recover
/// which source contributed. The recorder has already measured the two facts;
/// dropping them only on the successful path made a complete-looking note out
/// of a meeting where one source may have contributed nothing.
pub(super) fn append_delivery_facts(content: &mut String, mic: Delivered, system: Delivered) {
    content.push_str("\n\nRecording sources:\n- Microphone: ");
    content.push_str(mic.note_fact());
    content.push_str(".\n- System audio: ");
    content.push_str(system.note_fact());
    content.push('.');
}
