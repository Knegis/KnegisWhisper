import assert from "node:assert/strict";
import test from "node:test";
import { summaryProgressText } from "../src/lib/summaryProgress.js";

test("summary labels distinguish extraction, combination and saving, including resumed steps", () => {
  const processing = (completed: number, total: number) => summaryProgressText({ status: "processing", completed, total });
  assert.equal(processing(0, 0), "Preparing transcript");
  assert.equal(processing(0, 5), "Summarising transcript · part 1 of 3");
  assert.equal(processing(2, 5), "Summarising transcript · part 3 of 3");
  assert.equal(processing(3, 5), "Combining summary · 0 of 2 parts completed");
  assert.equal(processing(4, 5), "Combining summary · 1 of 2 parts completed");
  assert.equal(processing(5, 5), "Saving summary");
  assert.equal(processing(0, 1), "Summarising transcript · part 1 of 1");
  assert.equal(processing(1, 1), "Saving summary");
});
test("every durable job status has a label without a fabricated percentage", () => {
  for (const [status, expected] of [["queued", "Summary queued"], ["failed", "Summary failed"], ["interrupted", "Summary interrupted"], ["cancelled", "Summary cancelled"], ["succeeded", "Summary completed"]]) {
    assert.equal(summaryProgressText({ status: status!, completed: 0, total: 0 }), expected);
  }
  assert.equal(summaryProgressText({ status: null, completed: 0, total: 0 }), null);
});
