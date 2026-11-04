import { test } from "node:test";
import assert from "node:assert/strict";
import { dictationStatus } from "../src/lib/dictationNotice.js";

const base = {
  recording: false,
  busyLabel: null,
  ready: true,
  microphone: "System default",
  microphoneMissing: false,
  elapsed: "00:00",
};

test("ready says dictation is ready and names the microphone, without claiming the microphone was tested", () => {
  const status = dictationStatus(base);
  assert.equal(status.tone, "ready");
  assert.equal(status.notice.label, "Dictation");
  assert.equal(status.notice.value, "Ready · System default");
  assert.equal(status.tile, "ready");
  // The readiness check reads the route, the shortcut and the insert path. It
  // never opens the microphone, so nothing here may say the microphone is ready.
  assert.doesNotMatch(status.notice.label, /microphone/i);
});

test("needs setup never says ready", () => {
  const status = dictationStatus({ ...base, ready: false });
  assert.equal(status.tone, "setup");
  assert.equal(status.notice.value, "Needs setup");
  assert.equal(status.tile, "needs setup");
  assert.doesNotMatch(status.notice.value, /ready/i);
});

test("recording outranks readiness, and the ticking clock stays out of the announced words", () => {
  const status = dictationStatus({ ...base, recording: true, ready: false, elapsed: "01:05" });
  assert.equal(status.tone, "recording");
  assert.equal(status.notice.label, "Recording");
  assert.equal(status.notice.value, "the overlay is showing");
  assert.equal(status.notice.extra, "01:05");
  assert.equal(status.tile, "recording 01:05");
});

test("busy speaks for the session, whatever readiness says", () => {
  const status = dictationStatus({ ...base, busyLabel: "Transcribing…" });
  assert.equal(status.tone, "busy");
  assert.equal(status.notice.value, "Transcribing…");
  assert.equal(status.notice.extra, undefined);
  assert.equal(status.tile, "transcribing…");
});

test("a chosen microphone that is unplugged reads as the default dictation falls back to", () => {
  const status = dictationStatus({ ...base, microphone: "USB headset (not connected)", microphoneMissing: true });
  assert.equal(status.notice.value, "Ready · System default");
});
