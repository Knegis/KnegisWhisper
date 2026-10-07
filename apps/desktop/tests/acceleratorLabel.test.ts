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

import { acceleratorLabel, registrationFor } from "../src/lib/labels.js";

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

/* ------------------------------------------------------------------ KNEG-353
 * The platform having a hook is not the same as this accelerator being on it.
 * When the hook fails to start, Rust reports the chord `not-registrable` and
 * Dictation's readiness says the shortcut doesn't work -- and Settings, keyed
 * on the platform alone, said "Works on this computer." The accelerator's own
 * registration outranks the platform's answer.
 */

const chord = { kind: "modifier-only" as const, accelerator: "Control+Super", modifiers: ["Control", "Super"] };
const failedHook =
  "Control+Super needs the keyboard hook, which could not be started, so this shortcut does nothing right now. Restarting the app may fix it.";

test("a chord whose hook failed to start is a failure, though the platform has a hook", () => {
  const label = acceleratorLabel(chord, hooked, { state: "not-registrable", accelerator: "Control+Super", reason: failedHook });
  assert.equal(label.note, null);
  assert.equal(label.problem, failedHook);
});

test("a mouse button whose hook failed to start is a failure too", () => {
  const reason =
    "Mouse4 needs the mouse hook, which could not be started, so this button does nothing right now. Restarting the app may fix it.";
  const label = acceleratorLabel({ kind: "mouse-button", accelerator: "Mouse4", alsoDoes: "Back" }, hooked, {
    state: "not-registrable",
    accelerator: "Mouse4",
    reason,
  });
  assert.equal(label.note, null);
  assert.equal(label.problem, reason);
});

test("a chord on the hook keeps its works note", () => {
  const label = acceleratorLabel(chord, hooked, { state: "hooked", accelerator: "Control+Super" });
  assert.equal(label.problem, null);
  assert.equal(label.note?.text, "Works on this computer.");
});

test("a standard accelerator the platform refused is a failure carrying its reason", () => {
  const reason = "another application already uses it";
  assert.deepEqual(
    acceleratorLabel({ kind: "standard", accelerator: "Control+Shift+D" }, hooked, {
      state: "rejected",
      accelerator: "Control+Shift+D",
      reason,
    }),
    { text: "Control+Shift+D", problem: reason, note: null },
  );
});

test("an action nothing handles is left to the screen's own line", () => {
  // Settings already says why above the accelerators, in red; repeating it
  // under each one would say it twice.
  const label = acceleratorLabel({ kind: "standard", accelerator: "Control+Shift+Space" }, hooked, {
    state: "unhandled",
    accelerator: "Control+Shift+Space",
    reason: "The chat agent is not built yet.",
  });
  assert.equal(label.problem, null);
});

test("registrationFor finds this accelerator's result in this action's report", () => {
  const registered = { state: "registered", accelerator: "Control+Shift+D" } as const;
  const failed = { state: "not-registrable", accelerator: "Control+Super", reason: failedHook } as const;
  const reports = [
    { action: "chat-agent", results: [{ state: "hooked", accelerator: "Control+Super" }] },
    { action: "dictation", results: [registered, failed] },
  ] as const;
  assert.deepEqual(registrationFor([...reports], "dictation", chord), failed);
  assert.deepEqual(registrationFor([...reports], "dictation", { kind: "standard", accelerator: "Control+Shift+D" }), registered);
  assert.equal(registrationFor([...reports], "translation", chord), undefined);
  assert.equal(registrationFor([], "dictation", chord), undefined);
  const invalid = { state: "not-registrable", accelerator: "Control+Nope", reason: "Nope is not a key." } as const;
  assert.deepEqual(
    registrationFor([{ action: "dictation", results: [invalid] }], "dictation", {
      kind: "invalid",
      input: "Control+Nope",
      reason: "Nope is not a key.",
    }),
    invalid,
  );
});
