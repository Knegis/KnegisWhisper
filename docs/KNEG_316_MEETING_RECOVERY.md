# Interrupted meeting recovery (KNEG-316)

An interrupted meeting can leave a WAV in the application's `meetings` directory
without a `note_recordings` row. Personal notes may also be in the matching
`.notes.md` sidecar, and the name and folder chosen before Start are in the
matching `.meeting.json` (KNEG-365), written as the meeting starts. The
**Interrupted** section of the Meetings page's list panel lists these files
while no meeting is running, by that name when it has one, and offers recovery
one recording at a time. Nothing is transcribed or saved until the user presses
**Recover** on a row.

Recovery checks that the WAV is still a regular, unattached file in the owned
directory. A WAV left open by `WavWriter` has zero lengths in its header; the
recovery action validates its exact 16 kHz mono PCM format and fills in those
lengths without changing sample bytes. It also accepts a WAV whose header was
finished before the app stopped. A different or damaged format is refused, with
its file and notes sidecar left in place.

The existing whole-file local engine transcribes the WAV; optional speaker
labels follow the normal meeting setting. The note's body comes only from the
typed sidecar. A failed transcription still saves nonempty typed notes, with a
recording detail that names the failure. The note takes the meeting's name, or
is named for when the recording started (read from its file name), and is
saved in the chosen folder; a folder deleted since is left out, and the outcome
says so. An unreadable `.meeting.json` is ignored, with a warning in the log.
The note and WAV attachment commit in one transaction, so a failed save leaves
neither a duplicate note nor an attachment claim. Both sidecars are removed
only after that commit. If there are neither typed notes nor transcribed
segments, recovery leaves the WAV available for a later attempt. Recovered
notes say that audio may end abruptly and that the original capture-source
measurements are unavailable.

A meeting that ends without a note owning its WAV -- the recording could not
be closed, or nothing was transcribed and nothing typed -- keeps its
`.meeting.json`, so the note recovered from that WAV is still named and filed.

New recording names include a UUID, and `WavWriter::create` refuses an existing
path, so starting another meeting cannot overwrite an orphan from a prior run.

## Validation

Local automated checks cover LF migration bytes (KNEG-323), streamed WAV header
repair and refusal of foreign audio, no overwrite of an interrupted WAV,
unattached-file discovery, atomic note/attachment rollback on duplicate keys,
and lifecycle reservation during recovery. A disposable native launch was
attempted on 2026-09-28 with a synthetic unfinished WAV and sidecar, both by
launching the debug binary and through `tauri:dev`. The process opened a window
but stalled before `Db::open`; the isolated database was never created and
WebView2 did not expose its debug port. The processes and Vite server were
stopped. The Meetings page's Recover button and the recovery path remain to be
exercised in a functioning native session.
