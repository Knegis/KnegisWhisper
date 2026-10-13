import test from "node:test";
import assert from "node:assert/strict";
import { SKIP_SECONDS, markFractions, skipTarget } from "../src/lib/playerControls.js";

test("the skip step is fifteen seconds", () => {
  assert.equal(SKIP_SECONDS, 15);
});

test("skipTarget moves by delta", () => {
  assert.equal(skipTarget(30, 15, 100), 45);
  assert.equal(skipTarget(30, -15, 100), 15);
});

test("skipTarget clamps to the start and to the duration", () => {
  assert.equal(skipTarget(5, -15, 100), 0);
  assert.equal(skipTarget(95, 15, 100), 100);
});

test("skipTarget with an unknown duration only clamps at zero", () => {
  assert.equal(skipTarget(5, -15, NaN), 0);
  assert.equal(skipTarget(5, 15, Infinity), 20);
  assert.equal(skipTarget(5, 15, 0), 20);
  assert.equal(skipTarget(5, 15, -3), 20);
});

test("skipTarget treats a non-finite current time as zero", () => {
  assert.equal(skipTarget(NaN, 15, 100), 15);
  assert.equal(skipTarget(Infinity, -15, 100), 0);
});

test("markFractions maps seconds to fractions of the duration", () => {
  assert.deepEqual(markFractions([25, 50], 100), [0.25, 0.5]);
});

test("markFractions drops out-of-range and NaN marks, sorts and dedupes", () => {
  assert.deepEqual(markFractions([75, 0, NaN, 25, 100, 150, -5, 25, Infinity], 100), [0.25, 0.75]);
});

test("markFractions is empty without a usable duration", () => {
  assert.deepEqual(markFractions([1, 2], 0), []);
  assert.deepEqual(markFractions([1, 2], NaN), []);
  assert.deepEqual(markFractions([1, 2], Infinity), []);
  assert.deepEqual(markFractions([1, 2], -4), []);
});
