import { test } from "node:test";
import assert from "node:assert/strict";
import { MENU_GAP, placeMenu } from "../src/lib/menuPlacement.js";
import { TOOLTIP_MARGIN } from "../src/lib/tooltipPlacement.js";

const viewport = { width: 1000, height: 720 };
const menu = { width: 280, height: 200 };

test("opens above a chip near the foot of the window, the control bar's usual case", () => {
  const placed = placeMenu({ left: 120, top: 560, width: 160, height: 30 }, menu, viewport);
  assert.equal(placed.side, "above");
  assert.equal(placed.left, 120);
  assert.equal(placed.top, 560 - MENU_GAP - menu.height);
  assert.equal(placed.maxHeight, 560 - MENU_GAP - TOOLTIP_MARGIN);
});

test("opens above where it would also fit below, so it does not cover the bar the chip is in", () => {
  // Dictation's microphone chip at 1000x720: the control bar below it has room for a short menu.
  const placed = placeMenu({ left: 382, top: 419, width: 152, height: 30 }, { width: 334, height: 146 }, viewport);
  assert.equal(placed.side, "above");
  assert.equal(placed.top, 419 - MENU_GAP - 146);
});

test("a menu that fits above exactly, margin included, still opens above", () => {
  const top = TOOLTIP_MARGIN + menu.height + MENU_GAP;
  const placed = placeMenu({ left: 120, top, width: 160, height: 30 }, menu, viewport);
  assert.equal(placed.side, "above");
  assert.equal(placed.top, TOOLTIP_MARGIN);
});

test("opens below, on the chip's left edge, where there is no room above", () => {
  const placed = placeMenu({ left: 120, top: 100, width: 160, height: 30 }, menu, viewport);
  assert.equal(placed.side, "below");
  assert.equal(placed.left, 120);
  assert.equal(placed.top, 130 + MENU_GAP);
  assert.equal(placed.maxHeight, viewport.height - TOOLTIP_MARGIN - placed.top);
});

test("is pushed back inside the right edge", () => {
  const placed = placeMenu({ left: 900, top: 560, width: 90, height: 30 }, menu, viewport);
  assert.equal(placed.left, viewport.width - TOOLTIP_MARGIN - menu.width);
});

test("is pushed back inside the left edge", () => {
  const placed = placeMenu({ left: 2, top: 560, width: 90, height: 30 }, menu, viewport);
  assert.equal(placed.left, TOOLTIP_MARGIN);
});

test("a menu wider than the window starts at the margin", () => {
  const placed = placeMenu({ left: 300, top: 560, width: 90, height: 30 }, { width: 1200, height: 200 }, viewport);
  assert.equal(placed.left, TOOLTIP_MARGIN);
});

test("where neither side has room, it takes the larger side and scrolls inside the window", () => {
  const tall = { width: 280, height: 600 };
  const nearTop = placeMenu({ left: 120, top: 300, width: 160, height: 30 }, tall, viewport);
  assert.equal(nearTop.side, "below");
  assert.equal(nearTop.top, 330 + MENU_GAP);
  assert.equal(nearTop.top + nearTop.maxHeight, viewport.height - TOOLTIP_MARGIN);

  const nearFoot = placeMenu({ left: 120, top: 450, width: 160, height: 30 }, tall, viewport);
  assert.equal(nearFoot.side, "above");
  assert.equal(nearFoot.top, TOOLTIP_MARGIN);
  assert.equal(nearFoot.top + nearFoot.maxHeight, 450 - MENU_GAP);
});
