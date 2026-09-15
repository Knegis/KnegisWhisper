import assert from "node:assert/strict";
import test from "node:test";
import { ShortcutCapture } from "../src/lib/shortcutCapture.js";

test("captures a standard chord only after all keys are released", () => {
  const c = new ShortcutCapture(false);
  c.down("ControlLeft"); c.down("ShiftRight");
  assert.equal(c.down("KeyD").preview, "Control+Shift+D");
  assert.equal(c.up("KeyD"), null);
  assert.equal(c.up("ControlLeft"), null);
  assert.equal(c.up("ShiftRight"), "Control+Shift+D");
});
test("modifier-only capture can preserve sides and ignores repeats", () => {
  const c = new ShortcutCapture(true);
  c.down("ControlRight"); c.down("ShiftRight"); c.down("ShiftRight");
  assert.equal(c.up("ControlRight"), null);
  assert.equal(c.up("ShiftRight"), "RightControl+RightShift");
});
test("unsupported and multi-key chords cannot silently become a different shortcut", () => {
  const c = new ShortcutCapture(false);
  c.down("KeyA"); assert.ok(c.down("KeyB").error);
  c.up("KeyA"); assert.equal(c.up("KeyB"), null);
  assert.ok(c.down("Unidentified").error); assert.equal(c.up("Unidentified"), null);
  c.down("Space"); assert.equal(c.up("Space"), "Space");
});
test("side buttons, digits, and navigation keys retain parser names", () => {
  for (const [code, name] of [["Mouse4", "Mouse4"], ["Digit1", "1"], ["ArrowLeft", "ArrowLeft"], ["Numpad1", "Numpad1"]]) {
    const c = new ShortcutCapture(false);
    c.down(code!); assert.equal(c.up(code!), name);
  }
});
