import test from "node:test";
import assert from "node:assert/strict";
import { ICONS, FILLED } from "../src/lib/icons.js";

test("every icon is a non-empty path made only of path commands and numbers", () => {
  for (const [name, d] of Object.entries({ ...ICONS, ...FILLED })) {
    assert.ok(d.length > 0, name);
    assert.match(d, /^[MmLlHhVvAaZz0-9.,\s-]+$/, name);
  }
});
