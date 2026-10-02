/** Verifies the compact summary table and readable per-check report details. */
import test from "node:test";
import assert from "node:assert/strict";
import {
  addCheck,
  addEvent,
  createReport,
  failedChecks,
  renderMarkdown,
} from "./report.mjs";

test("report keeps structured records and makes a readable summary", () => {
  const report = createReport({ remBasis: "14px" });
  addCheck(report, {
    id: "1",
    claim: "Pinned",
    measured: { left: 0, nested: { width: 182 } },
    expectation: "left 0",
    matches: true,
    screenshots: ["rail.png"],
    note: "Measured after scrolling.",
  });
  addEvent(report, { type: "watch" });

  const markdown = renderMarkdown(report);
  assert.equal(report.checks[0].note, "Measured after scrolling.");
  assert.match(markdown, /\| ID \| Claim \| Matches \| Expectation \|/);
  assert.doesNotMatch(markdown, /\| ID \| Claim \| Matches \| Measured/);
  assert.match(markdown, /### 1\n\n```json\n[\s\S]*"nested"/);
  assert.match(markdown, /Measured after scrolling\./);
  assert.match(markdown, /Screenshots: rail\.png/);
  assert.match(markdown, /## Observations/);
});

test("only a check measured false fails the run (KNEG-389)", () => {
  const report = createReport();
  addCheck(report, { id: "1", claim: "a", measured: {}, expectation: "", matches: true });
  addCheck(report, { id: "2", claim: "b", measured: {}, expectation: "", matches: null });
  assert.deepEqual(failedChecks(report), []);
  addCheck(report, { id: "3", claim: "c", measured: {}, expectation: "", matches: false });
  addCheck(report, { id: "4", claim: "d", measured: {}, expectation: "", matches: false });
  assert.deepEqual(failedChecks(report), ["3", "4"]);
});
