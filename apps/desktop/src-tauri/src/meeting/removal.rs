//! Deleting a meeting's recording once its transcript is saved (KNEG-409).
//!
//! One switch under Settings > Meetings, `meetings.deleteRecordings`, off by
//! default. A meeting reads it once, at Start, into its plan
//! (`MeetingPlan::delete_recording`), which the session holds and which is
//! written beside the recording; Stop and recovery act on the plan, so turning
//! the switch while a meeting records changes the next meeting, not this one.
//! The key and its two commands are in `meeting.rs`, beside the other meeting
//! settings, where `tools/settings-audit` reads them.
//!
//! **When.** Only once a note is saved with a whole transcript: not a note of
//! typed notes alone, and not the running transcript saved because the
//! whole-recording pass failed. The recording is the only way to a better
//! transcript, so then it is kept, and the outcome says why
//! ([`recording_removal`]).
//!
//! **How.** The recording is attached to the note exactly as it is when the
//! switch is off, and only then deleted with
//! [`kw_notes_core::recordings::delete_attached`], which removes the
//! attachment and the file in one transaction. A crash before that leaves the
//! recording attached, so it is never offered under Interrupted and cannot
//! become a second note.

use std::path::Path;

use kw_storage::Db;
use serde::Serialize;
use ts_rs::TS;

/// What the note's recording details say once its recording is deleted.
const DELETED_DETAILS: &str = "The recording was deleted after transcribing (Settings › Meetings).";

/// What became of a saved meeting's recording, when it was to be deleted.
///
/// `None` in the outcome where it was not asked to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum RecordingRemoval {
    /// Deleted; the transcript is in the note.
    Deleted,
    /// Kept, because the saved transcript may be incomplete: the running one
    /// from during the meeting, or none at all.
    KeptIncomplete,
    /// Deletion was tried and failed, so the recording is still attached to
    /// the note. The outcome carries why.
    DeleteFailed,
}

/// Whether to delete a recording whose note was saved: `delete` is the
/// meeting's plan, `transcribed` whether the note has a transcript, and
/// `from_partials` whether that transcript is the running one saved because
/// the whole-recording pass failed. `Deleted` here means "delete it"; the
/// caller reports `DeleteFailed` if that fails.
pub(super) const fn recording_removal(
    delete: bool,
    transcribed: bool,
    from_partials: bool,
) -> Option<RecordingRemoval> {
    if !delete {
        None
    } else if transcribed && !from_partials {
        Some(RecordingRemoval::Deleted)
    } else {
        Some(RecordingRemoval::KeptIncomplete)
    }
}

/// Carry out `removal` for the recording at `recording`, attached to
/// `note_id`: delete it when it says `Deleted`, and say what happened, with
/// the error when deletion failed.
pub(super) async fn remove_recording(
    db: &Db,
    note_id: i64,
    recording: &Path,
    removal: Option<RecordingRemoval>,
) -> (Option<RecordingRemoval>, Option<String>) {
    if removal != Some(RecordingRemoval::Deleted) {
        return (removal, None);
    }
    let directory = recording
        .parent()
        .map_or_else(crate::recordings::recording_dir, Path::to_path_buf);
    match kw_notes_core::recordings::delete_attached(
        db.pool(),
        note_id,
        &directory,
        DELETED_DETAILS,
    )
    .await
    {
        Ok(true) => (Some(RecordingRemoval::Deleted), None),
        // Attached a moment ago, so not reachable unless something detached
        // it in between; nothing was deleted, so there is nothing to say.
        Ok(false) => {
            tracing::warn!(
                note_id,
                "the meeting recording was no longer attached when it was to be deleted"
            );
            (None, None)
        }
        Err(error) => {
            tracing::error!(note_id, %error, "could not delete the meeting recording");
            (Some(RecordingRemoval::DeleteFailed), Some(error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recording_is_deleted_only_when_a_whole_transcript_was_saved() {
        use RecordingRemoval::{Deleted, KeptIncomplete};
        // (asked to delete, transcribed, from the live partials) -> removal
        let table = [
            ((false, true, false), None),
            ((false, true, true), None),
            ((false, false, false), None),
            ((false, false, true), None),
            ((true, true, false), Some(Deleted)),
            // The running transcript, saved because the whole-recording pass
            // failed: the recording is the only way to a better one.
            ((true, true, true), Some(KeptIncomplete)),
            // Only typed notes: nothing was transcribed from it at all.
            ((true, false, false), Some(KeptIncomplete)),
            ((true, false, true), Some(KeptIncomplete)),
        ];
        for ((delete, transcribed, from_partials), expected) in table {
            assert_eq!(
                recording_removal(delete, transcribed, from_partials),
                expected,
                "delete {delete}, transcribed {transcribed}, from partials {from_partials}"
            );
        }
    }
}
