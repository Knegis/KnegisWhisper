/** A saved transcript must not imply that its recording can be played. */
export function meetingRecordingWarning(recordingAttached: boolean): string | null {
  return recordingAttached
    ? null
    : "Recording attachment failed, so playback is unavailable for this note. " +
        "Your note and recording were kept. Open the note, choose “Attach recording…”, " +
        "and select the recording at the path shown here to restore playback.";
}
