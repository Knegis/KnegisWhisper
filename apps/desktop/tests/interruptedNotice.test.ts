/**
 * The window's notice for a meeting that was cut short.
 *
 * A meeting whose application ended during Stop has no note, and the only
 * place that said its recording was still there was a row on the Meetings
 * page. Read from Notes, the meeting looked deleted.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { interruptedMeta, interruptedNotice } from "../src/lib/interruptedNotice.js";

const recording = (unfinished: boolean, hasNotes = false) => ({ storageKey: "20261117-090000-a.wav", hasNotes, title: null, unfinished });

test("nothing is raised when no meeting was cut short", () => {
  assert.equal(interruptedNotice([]), null);
});

test("a recording left with no note on purpose is not raised from every screen", () => {
  assert.equal(interruptedNotice([recording(false, true)]), null);
});

test("a meeting cut short says what is kept, typed notes included when there are some", () => {
  assert.equal(
    interruptedNotice([recording(true)]),
    "A meeting was cut short before it was saved. Its recording is kept on this computer.",
  );
  assert.equal(
    interruptedNotice([recording(true, true), recording(false)]),
    "A meeting was cut short before it was saved. Its recording and typed notes are kept on this computer.",
  );
});

test("several are counted", () => {
  assert.equal(
    interruptedNotice([recording(true), recording(true, true)]),
    "2 meetings were cut short before they were saved. Their recordings and typed notes are kept on this computer.",
  );
});

test("an interrupted row says it is kept, not that it was never transcribed", () => {
  assert.equal(interruptedMeta(recording(true)), "Not a note yet · recording kept");
  assert.equal(interruptedMeta(recording(false, true)), "Not a note yet · recording kept · typed notes kept");
});
