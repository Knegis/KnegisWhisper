/**
 * `keycapGroups` splits accelerators into the keys drawn as caps (KNEG-331).
 *
 * It takes the accelerators one by one rather than `acceleratorText`'s joined
 * string, because the comma is a key (`Control+,`) as well as the separator
 * that string uses.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { keycapGroups } from "../src/lib/keycaps.js";

test("one accelerator is one group, a cap per key", () => {
  assert.deepEqual(keycapGroups(["Control+Shift+D"]), [["Control", "Shift", "D"]]);
});

test("a second accelerator on the same action is its own group", () => {
  assert.deepEqual(keycapGroups(["Control+Shift+D", "Alt+F"]), [["Control", "Shift", "D"], ["Alt", "F"]]);
});

test("the comma key keeps its cap, wherever it falls in the list", () => {
  assert.deepEqual(keycapGroups(["Control+,", "Alt+F"]), [["Control", ","], ["Alt", "F"]]);
  assert.deepEqual(keycapGroups(["Alt+F", "Control+Shift+,"]), [["Alt", "F"], ["Control", "Shift", ","]]);
});

test("a key with no modifier is a single cap, and no accelerators is no caps", () => {
  assert.deepEqual(keycapGroups(["F9"]), [["F9"]]);
  assert.deepEqual(keycapGroups([]), []);
});
