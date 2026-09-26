import { test } from "node:test";
import assert from "node:assert/strict";
import { placeTooltip, TOOLTIP_GAP, TOOLTIP_MARGIN } from "../src/lib/tooltipPlacement.js";

const viewport = { width: 1000, height: 700 };
const tip = { width: 80, height: 28 };

test("sits centred below the anchor, arrow at the anchor's centre", () => {
  const placed = placeTooltip({ left: 460, top: 100, width: 40, height: 30 }, tip, viewport);
  assert.equal(placed.side, "below");
  assert.equal(placed.top, 130 + TOOLTIP_GAP);
  assert.equal(placed.left, 440);
  assert.equal(placed.arrowLeft, 40);
});

test("flips above when there is no room below", () => {
  const placed = placeTooltip({ left: 460, top: 660, width: 40, height: 30 }, tip, viewport);
  assert.equal(placed.side, "above");
  assert.equal(placed.top, 660 - TOOLTIP_GAP - tip.height);
  assert.equal(placed.arrowLeft, 40);
});

test("stays inside the right edge and the arrow keeps pointing at the anchor", () => {
  const placed = placeTooltip({ left: 970, top: 100, width: 24, height: 24 }, tip, viewport);
  assert.equal(placed.left, viewport.width - TOOLTIP_MARGIN - tip.width);
  assert.equal(placed.arrowLeft, 982 - placed.left);
});

test("stays inside the left edge", () => {
  const placed = placeTooltip({ left: 2, top: 100, width: 24, height: 24 }, tip, viewport);
  assert.equal(placed.left, TOOLTIP_MARGIN);
  assert.equal(placed.arrowLeft, 14 - TOOLTIP_MARGIN);
});

test("the arrow never leaves the box", () => {
  const placed = placeTooltip({ left: 0, top: 100, width: 4, height: 24 }, { width: 200, height: 28 }, viewport);
  assert.equal(placed.left, TOOLTIP_MARGIN);
  assert.ok(placed.arrowLeft >= 8, `arrow at ${placed.arrowLeft}`);
});

test("below wins when neither side fits", () => {
  const placed = placeTooltip({ left: 460, top: 300, width: 40, height: 30 }, { width: 80, height: 600 }, viewport);
  assert.equal(placed.side, "below");
});
