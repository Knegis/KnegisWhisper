/**
 * `acceleratorLabel` tells a failure from a binding that works and is worth
 * a note (Ruling 18).
 *
 * Settings > Shortcuts renders `problem` in `.row-detail.warn`, which is red
 * because every string reaching it is a failure. The shipped Windows default,
 * `Control+Super`, is a hooked modifier-only chord that works -- and it used to
 * come back as a `problem`, so the default row was red while Dictation said
 * Ready. These pin which cases are failures and which are notes.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { acceleratorLabel } from "../src/lib/labels.js";

const hooked = { state: "available", mechanism: "WH_KEYBOARD_LL low-level hook" } as const;
const noHook = { state: "unsupported", reason: "No keyboard hook exists on this platform." } as const;

test("a standard accelerator has nothing to say", () => {
  assert.deepEqual(acceleratorLabel({ kind: "standard", accelerator: "Control+Shift+D" }, hooked), {
    text: "Control+Shift+D",
    problem: null,
    note: null,
  });
});

test("a hooked modifier-only chord works: a note, never a problem", () => {
  const label = acceleratorLabel(
    { kind: "modifier-only", accelerator: "Control+Super", modifiers: ["Control", "Super"] },
    hooked,
  );
  assert.equal(label.text, "Control+Super");
  assert.equal(label.problem, null);
  assert.ok(label.note);
  // One plain sentence on screen; the mechanism is for Learn more.
  assert.equal(label.note.text, "Works on this computer.");
  assert.doesNotMatch(label.note.text, /WH_KEYBOARD_LL/);
  assert.match(label.note.more, /WH_KEYBOARD_LL/);
});

test("a modifier-only chord with no hook is a failure, and says why", () => {
  const label = acceleratorLabel(
    { kind: "modifier-only", accelerator: "Control+Super", modifiers: ["Control", "Super"] },
    noHook,
  );
  assert.equal(label.note, null);
  assert.ok(label.problem);
  assert.match(label.problem, /does nothing on this computer/);
  assert.match(label.problem, /No keyboard hook exists on this platform\./);
});

test("a watched mouse button works, and the note says what the click still does", () => {
  const label = acceleratorLabel(
    { kind: "mouse-button", accelerator: "Mouse4", alsoDoes: "Back" },
    hooked,
  );
  assert.equal(label.problem, null);
  assert.ok(label.note);
  // What the click still does is the part to know before pressing it, so it
  // is on screen; only the mechanism is folded.
  assert.match(label.note.text, /^Works on this computer/);
  assert.match(label.note.text, /Back/);
  assert.doesNotMatch(label.note.text, /WH_KEYBOARD_LL/);
  assert.match(label.note.more, /WH_KEYBOARD_LL/);
});

test("a mouse button with no hook is a failure", () => {
  const label = acceleratorLabel(
    { kind: "mouse-button", accelerator: "Mouse4", alsoDoes: "Back" },
    noHook,
  );
  assert.equal(label.note, null);
  assert.ok(label.problem);
  assert.match(label.problem, /does nothing on this computer/);
});

test("an invalid accelerator is a failure carrying its own reason", () => {
  assert.deepEqual(
    acceleratorLabel({ kind: "invalid", input: "Control+Nope", reason: "Nope is not a key." }, hooked),
    { text: "Control+Nope", problem: "Nope is not a key.", note: null },
  );
});

test("with the hook not known yet, nothing is called a failure", () => {
  const label = acceleratorLabel({
    kind: "modifier-only",
    accelerator: "Control+Super",
    modifiers: ["Control", "Super"],
  });
  assert.equal(label.problem, null);
  assert.ok(label.note);
  assert.match(label.note.text, /still being checked/);
});
