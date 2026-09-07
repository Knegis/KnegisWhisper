import assert from "node:assert/strict";
import test from "node:test";
import { meetingRecordingWarning } from "../src/lib/meetingRecordingWarning.js";

test("attachment failure explains saved data and the recovery action", () => {
  const warning = meetingRecordingWarning(false);
  assert.ok(warning);
  assert.match(warning, /playback is unavailable/);
  assert.match(warning, /note and recording were kept/);
  assert.match(warning, /Attach recording…/);
  assert.match(warning, /select the recording/);
});

test("successful attachment does not show a playback warning", () => {
  assert.equal(meetingRecordingWarning(true), null);
});
