/**
 * What the Meetings page says when a meeting ends (KNEG-354, item 5).
 *
 * The caveat ended with the recording's full path in plain view, and began
 * with the note's database id ("Saved as note 42"). The copy rules put paths
 * and ids behind Learn more; what happened, and anything missing from the
 * transcript, stays in view.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { meetingOutcomeLabel } from "../src/lib/labels.js";

const PATH = "C:\\Users\\asa\\AppData\\Roaming\\KnegisWhisper\\recordings\\meeting-2026-10-07.wav";

const saved = {
  outcome: "saved",
  noteId: 42,
  durationSeconds: 300,
  recordingPath: PATH,
  recordingAttached: true,
  gaps: 0,
  reconnects: 0,
  transcriptIsPartials: false,
  folderMissing: false,
  recordingRemoval: null,
  recordingRemovalError: null,
  speakerLabelsPending: false,
} as const;

test("a saved meeting says so plainly, with the note id and path behind Learn more", () => {
  const label = meetingOutcomeLabel(saved);
  assert.equal(label.text, "Saved as a note (5 min).");
  assert.equal(label.more, `Note 42. The recording is at ${PATH}.`);
});

// The note is saved before its speakers are analysed, so the meeting ends
// with a note and a sentence that more is coming, not with a wait.
test("a saved meeting whose speaker labels are still coming says so", () => {
  const label = meetingOutcomeLabel({ ...saved, speakerLabelsPending: true });
  assert.equal(label.text, "Saved as a note (5 min). Speaker labels are being added to the note.");
});

test("what is missing from a saved transcript stays in view", () => {
  const label = meetingOutcomeLabel({ ...saved, gaps: 2, transcriptIsPartials: true });
  assert.match(label.text, /^Saved as a note \(5 min\)\./);
  assert.match(label.text, /running one/);
  assert.match(label.text, /2 stretch\(es\) of audio were never transcribed/);
  assert.doesNotMatch(label.text, /AppData|\.wav/);
});

// KNEG-365: the folder chosen before Start was deleted before Stop, so Rust
// saved the note in no folder rather than refusing it. Said in view: the note
// is not where the chip said it would be.
test("a saved meeting whose folder was deleted says the note isn't in a folder", () => {
  const label = meetingOutcomeLabel({ ...saved, folderMissing: true });
  assert.equal(
    label.text,
    "Saved as a note (5 min). The folder you chose was deleted, so the note isn't in a folder.",
  );
  assert.equal(label.more, `Note 42. The recording is at ${PATH}.`);
  assert.doesNotMatch(meetingOutcomeLabel(saved).text, /folder/);
});

// KNEG-409: with Settings › Meetings' "Delete each recording once its
// transcript is saved" on, the outcome says what became of the recording.
test("a recording deleted after transcribing says so, and no path is given", () => {
  const label = meetingOutcomeLabel({ ...saved, recordingRemoval: "deleted" });
  assert.equal(label.text, "Saved as a note (5 min). The recording was deleted; the transcript is in the note.");
  assert.equal(label.more, "Note 42.");
  assert.doesNotMatch(`${label.text} ${label.more}`, /AppData|\.wav/);
});

test("a recording kept because the transcript may be incomplete says why", () => {
  const label = meetingOutcomeLabel({ ...saved, transcriptIsPartials: true, recordingRemoval: "kept-incomplete" });
  assert.match(label.text, /^Saved as a note \(5 min\)\. The whole-recording pass did not complete/);
  assert.match(label.text, / The recording was kept because the transcript may be incomplete\.$/);
  assert.equal(label.more, `Note 42. The recording is at ${PATH}.`);
});

test("a recording that couldn't be deleted says it is still attached, with why behind Learn more", () => {
  const error = `could not delete ${PATH}: Access is denied. (os error 5)`;
  const label = meetingOutcomeLabel({ ...saved, recordingRemoval: "delete-failed", recordingRemovalError: error });
  assert.equal(
    label.text,
    "Saved as a note (5 min). The recording couldn't be deleted, so it's still attached to the note.",
  );
  assert.equal(label.more, `Note 42. The recording is at ${PATH}. Deleting it failed: ${error}`);
  assert.doesNotMatch(label.text, /Access is denied/);
});

test("a recording nobody asked to delete is not mentioned", () => {
  assert.doesNotMatch(meetingOutcomeLabel(saved).text, /recording/i);
});

test("nothing transcribed keeps why in view and folds the path", () => {
  const label = meetingOutcomeLabel({
    outcome: "nothing-transcribed",
    recordingPath: PATH,
    mic: "silence",
    system: "not-captured",
    reconnects: 0,
  });
  assert.match(label.text, /^Nothing was transcribed\./);
  assert.doesNotMatch(label.text, /AppData|\.wav/);
  assert.equal(label.more, `The recording is at ${PATH}.`);
});

test("a discarded or failed meeting has nothing to fold", () => {
  assert.deepEqual(meetingOutcomeLabel({ outcome: "cancelled", noteId: null }), {
    text: "Discarded. The recording was deleted.",
    more: null,
  });
  assert.deepEqual(meetingOutcomeLabel({ outcome: "failed", message: "The provider closed the session." }), {
    text: "The provider closed the session.",
    more: null,
  });
});


test("A typed-only kept recording names the absence of transcription", () => {
  const label = meetingOutcomeLabel({ ...saved, recordingRemoval: "kept-untranscribed" });
  assert.match(label.text, /nothing was transcribed/);
  assert.doesNotMatch(label.text, /incomplete/);
});

test("A file-gone database failure offers cleanup and keeps the error without a replay path", () => {
  const label = meetingOutcomeLabel({ ...saved, recordingRemoval: "deleted-update-failed", recordingRemovalError: "database commit refused" });
  assert.match(label.text, /recording was deleted/);
  assert.match(label.text, /cleanup/);
  assert.doesNotMatch(label.text, /still attached/);
  assert.match(label.more ?? "", /database commit refused/);
  assert.doesNotMatch(label.more ?? "", /recording is at/);
});
