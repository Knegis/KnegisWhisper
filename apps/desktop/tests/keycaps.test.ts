/**
 * `keycapGroups` splits accelerators into the keys drawn as caps (KNEG-331).
 *
 * It takes the accelerators one by one rather than `acceleratorText`'s joined
 * string, because the comma is a key (`Control+,`) as well as the separator
 * that string uses.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { keycapGroups, keyName, shortcutText } from "../src/lib/keycaps.js";

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

test("on Windows the Super key is called what the keyboard calls it", () => {
  assert.equal(keyName("Super", "Win32"), "Win");
  assert.equal(keyName("LeftSuper", "Win32"), "Left Win");
  assert.equal(keyName("RightSuper", "Win32"), "Right Win");
});

test("every other key, and every other platform, keeps its name", () => {
  assert.equal(keyName("Control", "Win32"), "Control");
  assert.equal(keyName("D", "Win32"), "D");
  assert.equal(keyName("Super", "Linux x86_64"), "Super");
  assert.equal(keyName("LeftSuper", "Linux x86_64"), "LeftSuper");
});

test("a shortcut as words uses the same names, and leaves the comma key and the separator alone", () => {
  assert.equal(shortcutText("Control+Super", "Win32"), "Control+Win");
  assert.equal(shortcutText("Control+Super, Alt+F", "Win32"), "Control+Win, Alt+F");
  assert.equal(shortcutText("Control+,", "Win32"), "Control+,");
  assert.equal(shortcutText("Control+Super", "Linux x86_64"), "Control+Super");
});
