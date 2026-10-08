import { test } from "node:test";
import assert from "node:assert/strict";
import {
  REDUCED_MOTION_INTERVAL_MS,
  STALE_MS,
  levelAt,
  segmentsLit,
  showDelay,
} from "../src/lib/levelMeter.js";

test("a level lights its share of the segments, rounded", () => {
  assert.equal(segmentsLit(0, 10), 0);
  assert.equal(segmentsLit(1, 10), 10);
  assert.equal(segmentsLit(0.75, 8), 6);
  assert.equal(segmentsLit(0.5, 10), 5);
  // Under half a segment lights nothing: the floor of the meter is silence.
  assert.equal(segmentsLit(0.04, 10), 0);
  assert.equal(segmentsLit(0.06, 10), 1);
});

test("a level outside 0..1, or not a number, is clamped or silent", () => {
  assert.equal(segmentsLit(1.4, 10), 10);
  assert.equal(segmentsLit(-0.2, 10), 0);
  assert.equal(segmentsLit(Number.NaN, 10), 0);
  assert.equal(segmentsLit(0.5, 0), 0, "a meter with no segments lights none");
});

test("the last level heard is shown until it is 400 ms old", () => {
  assert.equal(STALE_MS, 400);
  const heard = { level: 0.75, at: 1_000 };
  assert.equal(levelAt(heard, 1_000), 0.75);
  assert.equal(levelAt(heard, 1_399), 0.75);
  assert.equal(levelAt(heard, 1_400), 0, "stale: the capture went quiet or went away");
  assert.equal(levelAt(null, 1_000), 0, "nothing heard yet");
});

test("a reading outside 0..1, or not a number, shows clamped or as silence", () => {
  assert.equal(levelAt({ level: 2, at: 0 }, 0), 1);
  assert.equal(levelAt({ level: -1, at: 0 }, 0), 0);
  assert.equal(levelAt({ level: Number.NaN, at: 0 }, 0), 0);
});

test("without reduced motion every reading is shown at once", () => {
  assert.equal(showDelay(null, 0, false), 0);
  assert.equal(showDelay(1_000, 1_010, false), 0);
});

test("under reduced motion a meter changes at most four times a second", () => {
  assert.equal(REDUCED_MOTION_INTERVAL_MS, 250);
  assert.equal(showDelay(null, 1_000, true), 0, "the first reading is shown at once");
  assert.equal(showDelay(1_000, 1_100, true), 150, "the rest of the quarter second");
  assert.equal(showDelay(1_000, 1_250, true), 0);
  assert.equal(showDelay(1_000, 1_900, true), 0);
});
