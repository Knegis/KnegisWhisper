/**
 * What a meeting note says about its speaker labels.
 *
 * A meeting whose speaker analysis ended the application used to leave no
 * note. The note is saved first now, and this line is how a failure of the
 * labels shows: in view, with a way to try again and a way to go on without.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { speakerLabelLine } from "../src/lib/speakerLabels.js";

test("a note that never asked for labels, or has them, says nothing", () => {
  assert.equal(speakerLabelLine(null), null);
  assert.equal(speakerLabelLine({ status: "succeeded", error: null, canRetry: false }), null);
});

test("labels being added say the note is already saved, and can be given up on", () => {
  for (const status of ["queued", "processing"]) {
    const line = speakerLabelLine({ status, error: null, canRetry: false });
    assert.equal(line?.kind, "working");
    assert.match(line?.text ?? "", /note and transcript are saved/);
    assert.equal(line?.retry, null);
    assert.equal(line?.dismiss, "Continue without speaker labels");
  }
});

test("a failure keeps its reason behind Details and offers both ways on", () => {
  const line = speakerLabelLine({ status: "failed", error: "the diarizer exited with code 127", canRetry: true });
  assert.equal(line?.kind, "failed");
  assert.equal(line?.text, "Speaker labels couldn't be added. The note and transcript are saved.");
  assert.equal(line?.detail, "the diarizer exited with code 127");
  assert.equal(line?.retry, "Retry speaker labels");
  assert.equal(line?.dismiss, "Continue without speaker labels");
});

test("labels cut short by the application ending say so, not that they failed", () => {
  const line = speakerLabelLine({ status: "interrupted", error: "Speaker labels were interrupted.", canRetry: true });
  assert.match(line?.text ?? "", /interrupted before they finished/);
  assert.equal(line?.retry, "Retry speaker labels");
});

test("a failure with no recording left offers only to continue", () => {
  const line = speakerLabelLine({ status: "failed", error: "The recording is gone.", canRetry: false });
  assert.equal(line?.retry, null);
  assert.equal(line?.dismiss, "Continue without speaker labels");
});

test("a note that went on without labels can still ask for them while its recording is there", () => {
  const offered = speakerLabelLine({ status: "dismissed", error: null, canRetry: true });
  assert.equal(offered?.kind, "offer");
  assert.equal(offered?.retry, "Add speaker labels");
  assert.equal(offered?.dismiss, null);
  assert.equal(speakerLabelLine({ status: "dismissed", error: null, canRetry: false }), null);
});
