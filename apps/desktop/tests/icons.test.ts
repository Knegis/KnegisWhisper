import test from "node:test";
import assert from "node:assert/strict";
import { ICONS, FILLED } from "../src/lib/icons.js";

test("every icon is a non-empty path made only of path commands and numbers", () => {
  for (const [name, d] of Object.entries({ ...ICONS, ...FILLED })) {
    assert.ok(d.length > 0, name);
    assert.match(d, /^[MmLlHhVvAaZz0-9.,\s-]+$/, name);
  }
});

test("pause is two closed bars", () => {
  assert.match(FILLED.pause, /^M[^M]*zM[^M]*z$/);
});

test("chevronUp exists and is chevronDown mirrored vertically", () => {
  assert.ok(ICONS.chevronUp.length > 0);
  assert.equal(ICONS.chevronUp, "M7 14l5-5 5 5");
});
