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
} as const;

test("a saved meeting says so plainly, with the note id and path behind Learn more", () => {
  const label = meetingOutcomeLabel(saved);
  assert.equal(label.text, "Saved as a note (5 min).");
  assert.equal(label.more, `Note 42. The recording is at ${PATH}.`);
});

test("what is missing from a saved transcript stays in view", () => {
  const label = meetingOutcomeLabel({ ...saved, gaps: 2, transcriptIsPartials: true });
  assert.match(label.text, /^Saved as a note \(5 min\)\./);
  assert.match(label.text, /running one/);
  assert.match(label.text, /2 stretch\(es\) of audio were never transcribed/);
  assert.doesNotMatch(label.text, /AppData|\.wav/);
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
