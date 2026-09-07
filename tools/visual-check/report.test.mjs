/** Verifies the compact summary table and readable per-check report details. */
import test from "node:test";
import assert from "node:assert/strict";
import {
  addCheck,
  addEvent,
  createReport,
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
