import { test } from "node:test";
import assert from "node:assert/strict";
import { meetingStatus } from "../src/lib/meetingNotice.js";
import { recentPicture } from "../src/lib/meetingView.js";

const base = {
  state: "idle" as const,
  saved: null,
  ready: true,
  microphone: "System default",
  microphoneMissing: false,
  elapsed: null,
  destination: "Sent to Deepgram",
};

test("idle and ready says the meeting can start and names the microphone, without claiming it was tested", () => {
  const status = meetingStatus(base);
  assert.equal(status.tone, "ready");
  assert.equal(status.notice.label, "Meeting");
  assert.equal(status.notice.value, "Ready · System default");
  assert.equal(status.tile, "ready");
});

test("a chosen microphone that is unplugged reads as the default a meeting falls back to", () => {
  const status = meetingStatus({ ...base, microphone: "USB headset (not connected)", microphoneMissing: true });
  assert.equal(status.notice.value, "Ready · System default");
});

test("needs setup never says ready", () => {
  const status = meetingStatus({ ...base, ready: false });
  assert.equal(status.tone, "setup");
  assert.equal(status.notice.value, "Needs setup");
  assert.doesNotMatch(status.tile, /ready/);
});

test("recording says where the audio goes, and keeps the clock out of the announced words", () => {
  const status = meetingStatus({ ...base, state: "recording", elapsed: "00:01:32" });
  assert.equal(status.tone, "recording");
  assert.equal(status.notice.label, "Recording");
  assert.equal(status.notice.value, "Sent to Deepgram");
  assert.equal(status.notice.extra, "00:01:32");
  assert.equal(status.tile, "recording");
});

test("a live transcript that stopped is still recording, and says what stopped", () => {
  const status = meetingStatus({ ...base, state: "recording-without-transcript", elapsed: "00:01:32" });
  assert.equal(status.tone, "recording");
  assert.equal(status.notice.value, "the live transcript has stopped");
});

test("stalled never reads as recording: nothing is being captured", () => {
  const status = meetingStatus({ ...base, state: "stalled", elapsed: "00:01:32" });
  assert.equal(status.tone, "setup");
  assert.equal(status.notice.label, "Stopped unexpectedly");
  assert.equal(status.notice.value, "nothing is being recorded");
  assert.doesNotMatch(status.tile, /^recording/);
});

test("finishing is work under way, and a page opened during it has no clock to show", () => {
  const status = meetingStatus({ ...base, state: "finishing", elapsed: null });
  assert.equal(status.tone, "busy");
  assert.equal(status.notice.label, "Finishing");
  assert.equal(status.notice.extra, undefined);
  assert.equal(status.tile, "finishing the meeting…");
});

test("saved shows the outcome's own sentence, and only while nothing else is running", () => {
  const saved = "Saved as a note (31 min).";
  const status = meetingStatus({ ...base, saved });
  assert.equal(status.tone, "ready");
  assert.equal(status.notice.label, "Saved");
  assert.equal(status.notice.value, saved);
  assert.equal(status.tile, "saved");
  assert.equal(meetingStatus({ ...base, saved, state: "recording", elapsed: "00:00:01" }).notice.label, "Recording");
});

test("a recent meeting keeps its picture, and neighbouring meetings differ", () => {
  assert.equal(recentPicture(7), recentPicture(7));
  assert.notEqual(recentPicture(7), recentPicture(8));
  assert.deepEqual([0, 1, 2].map(recentPicture), ["dusk", "streak", "orb"]);
  // An id is never negative today, but a picture must still be one of the three.
  assert.ok(["dusk", "streak", "orb"].includes(recentPicture(-4)));
});
