# Interrupted meeting recovery (KNEG-316)

An interrupted meeting can leave a WAV in the application's `meetings` directory
without a `note_recordings` row. Personal notes may also be in the matching
`.notes.md` sidecar, and the name and folder chosen before Start are in the
matching `.meeting.json` (KNEG-365), written as the meeting starts, with
whether the recording is deleted once its transcript is saved (KNEG-409). The
**Interrupted** section of the Meetings page's list panel lists these files
while no meeting is running, by that name when it has one, and offers recovery
one recording at a time. Nothing is transcribed or saved until the user presses
**Recover** on a row.

A note in Trash (KNEG-401) still owns its WAV -- Restore brings the recording back
with it -- so that WAV is attached, not interrupted, and is never offered here. It is
removed only when the note is deleted forever or Trash is emptied, and only inside the
transaction that deletes the note's rows, before it commits
(`recordings::empty_trash`, the order `delete_attached` uses): a file that can't be
removed rolls the deletion back, so no WAV is ever left that no note owns, to come back
here as an interrupted meeting.

Recovery checks that the WAV is still a regular, unattached file in the owned
directory. A WAV left open by `WavWriter` has zero lengths in its header; the
recovery action validates its exact 16 kHz mono PCM format and fills in those
lengths without changing sample bytes. It also accepts a WAV whose header was
finished before the app stopped. A different or damaged format is refused, with
its file and notes sidecar left in place.

The existing whole-file local engine transcribes the WAV. Optional speaker
labels follow the normal meeting setting and are added after the note is saved,
as for Stop ([below](#speaker-labels-after-the-save)). The note's body comes only from the
typed sidecar. A failed transcription still saves nonempty typed notes, with a
recording detail that names the failure. The note takes the meeting's name, or
is named for when the recording started (read from its file name), and is
saved in the chosen folder; a folder deleted since is left out, and the outcome
says so. An unreadable `.meeting.json` is ignored, with a warning in the log.
The note and WAV attachment commit in one transaction, so a failed save leaves
neither a duplicate note nor an attachment claim. Both sidecars are removed
only after that commit. Then, if the `.meeting.json` says to delete the
recording -- or, with no `.meeting.json`, the setting says so now -- and
segments were transcribed, the attachment and the WAV are removed together,
once any speaker labels queued for the note have settled
(`recordings::delete_attached`), once a full WAL checkpoint has put the note on
disk; a recovery of typed notes alone keeps the WAV, and its recording details
end by saying it was kept because nothing was transcribed from it. A deletion
that fails is said there too ([notes workspace](NOTES_WORKSPACE.md)).
A `.meeting.json` written before KNEG-409 has no delete choice and keeps it. If
there are neither typed notes nor transcribed segments, recovery leaves the WAV
available for a later attempt. Recovered notes say that audio may end abruptly
and that the original capture-source measurements are unavailable.

A meeting that ends without a note owning its WAV -- the recording could not
be closed, nothing was transcribed and nothing typed, or the note was saved but
the WAV could not be attached to it -- keeps its `.meeting.json`, so the note
recovered from that WAV is still named, filed and kept or deleted as the
meeting chose. A discarded meeting's WAV is deleted by the recorder, so its
`.meeting.json` goes too, even when the typed notes beside it could not be
saved and stay on disk.

New recording names include a UUID, and `WavWriter::create` refuses an existing
path, so starting another meeting cannot overwrite an orphan from a prior run.

## A meeting that was cut short

Most recordings with no note are left on purpose: a quiet room keeps its
recording. A meeting the application never saw end -- it crashed or was closed
while the meeting ran or was being saved -- is told apart by an empty
`.unfinished` file beside the WAV, written at Start and removed when the meeting
ends in any way the application sees: saved, nothing transcribed, failed or
discarded. A recording made before the marker existed has none.

For a recording that still has the marker, the window says so on every screen
but Meetings -- "A meeting was cut short before it was saved. Its recording and
typed notes are kept on this computer." -- with **Recover in Meetings**. On
Meetings, interrupted recordings come first under Recent, ahead of saved
meetings, and read "Not a note yet · recording kept". Until 2026-11-17 they came
after up to twenty saved meetings, read "Not transcribed", and the page could
say "No meetings saved yet." above them; read from Notes, the meeting looked
deleted.

## Speaker labels after the save

Until 2026-11-17 Stop and Recover analysed speakers before saving the note.
That step takes about five minutes per hour of recording, and nothing about the
meeting was in the database until it finished, so a process that ended during
it left no note. A report of exactly that, from somebody else's Windows computer
on v0.2.0, is what changed it (KNEG-562). The cause of that crash is not known: the
diarizer is a separate process whose failure the code already survived, so the
application itself ended, and a release build then wrote no log.

Now the note is saved first, with its transcript unlabelled, and
`note_speaker_jobs` (migration 0015) holds a job that adds the labels:

- **Queued with the note**, in the same transaction, when speaker labels are on
  and the saved transcript is a whole one with timings and no speakers of its
  provider's own. The running transcript kept when the whole-recording pass
  fails is not labelled, as before.
- **Run one at a time by a background worker**, which starts the diarizer with
  no console window, a deadline of ten minutes plus the recording's length, and
  bounded output, and ends it if the job is dismissed or the note goes to Trash.
- **Applied only to the transcript they were worked out from.** A note whose
  transcript changed meanwhile is left alone and the job fails saying so.
- **Never restarted on its own.** A job that was running when the application
  ended is marked interrupted at the next launch. If analysing that recording
  is what ended the application, starting it again would end it again.
- **The note says what happened**, over its transcript: still being added,
  failed or interrupted with the reason under Details, with **Retry speaker
  labels** and **Continue without speaker labels**. A note that continued
  without them offers **Add speaker labels** while its recording is attached.
- **The summary and the recording's deletion wait for the job.** The automatic
  summary names speakers, so it is queued when the job succeeds, fails, is
  interrupted or is dismissed. A recording set to be deleted is deleted when the
  job succeeds or is dismissed, and kept while it is failed or interrupted. A
  deletion that could not be handed over stays asked for and is tried again.
- **Trash stops the job, and Restore brings the summary.** A note moved to Trash
  has its job dismissed with no summary queued, since nothing works on a note in
  Trash, and the deletion its meeting asked for is dropped: the recording goes
  when Trash is emptied. Restored, the note gets its automatic summary when
  summaries are on, and offers **Add speaker labels**. If the summary cannot be
  queued the note is restored all the same.

What still leaves no note is the application ending before the whole-recording
pass finishes. That is the case the section above is for.

The application's log is now also written to `logs\knegiswhisper.log` in its
data directory, with any panic, so the next such report has something to read
([security](SECURITY.md#logging)).

## Validation

Local automated checks cover LF migration bytes (KNEG-323), streamed WAV header
repair and refusal of foreign audio, no overwrite of an interrupted WAV,
unattached-file discovery, atomic note/attachment rollback on duplicate keys,
and lifecycle reservation during recovery. A disposable native launch was
attempted on 2026-09-28 with a synthetic unfinished WAV and sidecar, both by
launching the debug binary and through `tauri:dev`. The process opened a window
but stalled before `Db::open`; the isolated database was never created and
WebView2 did not expose its debug port. The processes and Vite server were
stopped.

The recovery path has since crossed real WebView IPC twice, both times in a
hidden native WebView with an in-memory database, a disposable profile and
synthetic transcription output, not a real crash or model:

- `meeting::native_recovery_test::recovered_provider_and_lightweight_notes_cross_native_ipc`
  recovered a meeting and checked its provenance, listing and update metadata
  ([KNEG-423/420 validation](KNEG_423_420_VALIDATION.md)).
- `native_meeting_progress_and_settings_ipc` ran a real recovery that reported
  Preparing recording, Transcribing recording and Saving note
  ([three-issues validation](KNEG_THREE_ISSUES_VALIDATION.md)).

Neither presses the Meetings page's Recover button, and no recording interrupted
by a real crash has been recovered. That by-hand step is item 34 of KNEG-378:
end the app's process mid-meeting, then press Recover under Interrupted.

### Speaker labels after the save (2026-11-17)

Automated, on Windows: the job's rules over a real SQLite database
(`kw_notes_core::speaker_labels`, fifteen tests: queue, claim, finish, fail,
retry, dismiss, launch recovery, Trash and Restore, deferred deletion, a waiting
summary's dropped progress); a stopped and a recovered
meeting saved with a queued job, an attached recording, no summary job and no
deletion, with no diarizer run (`meeting::tests`); the labels attached to a
saved transcript (`speaker_labels::tests`); the running-meeting marker
(`meeting::recovery::tests`).

Run once by hand: `the_installed_diarizer_labels_a_real_recording`, ignored by
default, started the installed sherpa diarizer the way the job does over the
6.3-second speech fixture and attached its one turn to a transcript. That
verifies the process launch and the labelling, and nothing further.

Seen in headless Edge against dev fixtures, not in the Tauri window: the note's
status line in each state, light and dark, at 1400 and 900 px; the window's
notice on Notes; the interrupted row first under Recent.

Not done: a real meeting stopped in the application with speaker labels on; the
application's process ended while the job runs, and while the whole-recording
pass runs; the log file and panic record in a release build. The reporter's
computer has not been examined, so whether their recording is still in the
`meetings` folder is unknown.
