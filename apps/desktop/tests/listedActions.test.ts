import { test } from "node:test";
import assert from "node:assert/strict";
import { listedActions } from "../src/lib/labels.js";
import type { HotkeyAction } from "../src/lib/ipc.js";

const binding = (action: HotkeyAction) => ({ action, mode: "toggle" as const });
const all = (["dictation", "chat-agent", "voice-agent", "translation", "meeting"] as const).map(binding);

test("an action nothing handles is not listed (KNEG-571)", () => {
  // What Rust reports today: every action but dictation.
  const unhandled = (["chat-agent", "voice-agent", "translation", "meeting"] as const).map((action) => ({ action, reason: "not built yet" }));
  assert.deepEqual(listedActions(all, unhandled).map((b) => b.action), ["dictation"]);
});

test("an action appears by itself once it is handled, in the order Rust sent", () => {
  const unhandled = [{ action: "chat-agent" as const }, { action: "translation" as const }];
  assert.deepEqual(listedActions(all, unhandled).map((b) => b.action), ["dictation", "voice-agent", "meeting"]);
  assert.deepEqual(listedActions(all, []).map((b) => b.action), all.map((b) => b.action));
});

test("the bindings it returns are the ones it was given", () => {
  const [first] = listedActions(all, []);
  assert.equal(first, all[0]);
});
