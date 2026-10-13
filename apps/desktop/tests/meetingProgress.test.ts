import assert from "node:assert/strict";
import test from "node:test";
import { newerProgress, processingClock, FINALISATION_LABELS } from "../src/lib/meetingProgress.js";
import type { FinalisationUpdate } from "../src/lib/ipc.js";
const initial: FinalisationUpdate = { revision: 3, progress: { operationId: 10, kind: "stop", stage: "finishing-live-transcript", elapsedSeconds: 12 } };
test("old events and old snapshots cannot replace a later stage or cleared operation", () => {
  assert.equal(newerProgress(initial, { ...initial, revision: 2 }), false);
  assert.equal(newerProgress(initial, { revision: 4, progress: null }), true);
  assert.equal(newerProgress({ revision: 4, progress: null }, initial), false);
  assert.equal(newerProgress({ revision: 4, progress: null }, { ...initial, revision: 4 }), false);
});
test("a same-revision snapshot can advance elapsed time, never move it backwards", () => {
  assert.equal(newerProgress(initial, { ...initial, progress: { ...initial.progress!, elapsedSeconds: 13 } }), true);
  assert.equal(newerProgress(initial, { ...initial, progress: { ...initial.progress!, elapsedSeconds: 11 } }), false);
  assert.equal(newerProgress(initial, { ...initial, progress: { ...initial.progress!, operationId: 9, elapsedSeconds: 13 } }), false);
});
test("stage labels cover real stages and the clock is elapsed processing time", () => {
  assert.equal(Object.keys(FINALISATION_LABELS).length, 5);
  assert.equal(processingClock(61.9), "Processing 1:01");
  assert.equal(processingClock(3601), "Processing 1:00:01");
});
