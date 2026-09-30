import { test } from "node:test";
import assert from "node:assert/strict";
import { lastDictation } from "../src/lib/lastDictation.js";
import type { Transcription } from "../src/generated/ipc-types.js";

// Every time in UTC, so nothing here depends on the machine's time zone.
const at = (minute: number) => new Date(Date.UTC(2026, 8, 30, 10, minute));
const stored = (minute: number) => at(minute).toISOString();

const row = (over: Partial<Transcription>): Transcription => ({
  id: 1,
  text: "Send the notes to Jonas.",
  rawText: null,
  createdAt: stored(0),
  hasAudio: false,
  audioDurationMs: null,
  provider: null,
  model: null,
  status: "completed",
  errorMessage: null,
  errorCode: null,
  routeKind: null,
  ...over,
});

test("with history off, a Discard after an insert keeps the inserted text", () => {
  // A cancelled outcome carries no text and, with history off, writes no row.
  const last = lastDictation([], { text: "Book the room.", historyId: null, at: at(5) }, null);
  assert.deepEqual(last, { text: "Book the room.", at: at(5), raw: null, typed: true });
});

test("with history off, a failure after an insert keeps the inserted text", () => {
  const last = lastDictation([], { text: "Book the room.", historyId: null, at: at(5) }, { historyId: null, at: at(6) });
  assert.equal(last?.text, "Book the room.");
  assert.equal(last?.typed, true);
});

test("with history off, the session's insert beats a row saved before history went off", () => {
  const history = [row({ id: 3, text: "An older dictation.", createdAt: stored(1) })];
  const last = lastDictation(history, { text: "Book the room.", historyId: null, at: at(5) }, null);
  assert.equal(last?.text, "Book the room.");
});

test("a row saved after the session's insert is the newer one", () => {
  // History switched back on after an unsaved insert, then a paste failed.
  const history = [row({ id: 4, text: "Pasted nowhere.", status: "failed", createdAt: stored(9) })];
  const last = lastDictation(history, { text: "Book the room.", historyId: null, at: at(5) }, null);
  assert.equal(last?.text, "Pasted nowhere.");
  assert.equal(last?.typed, false);
});

test("a failure whose row kept the text shows that text, marked as not typed", () => {
  // Only the paste failed: `fail` stores the cleaned text on the row.
  const history = [
    row({ id: 7, text: "Pasted nowhere.", status: "failed", createdAt: stored(8) }),
    row({ id: 6, text: "Book the room.", createdAt: stored(5) }),
  ];
  const last = lastDictation(history, { text: "Book the room.", historyId: 6, at: at(5) }, { historyId: 7, at: at(8) });
  assert.deepEqual(last, { text: "Pasted nowhere.", at: at(8), raw: null, typed: false });
});

test("a Discard after a failure that kept its text leaves that row showing", () => {
  const history = [
    row({ id: 7, text: "Pasted nowhere.", status: "failed", createdAt: stored(8) }),
    row({ id: 6, text: "Book the room.", createdAt: stored(5) }),
  ];
  const last = lastDictation(history, { text: "Book the room.", historyId: 6, at: at(5) }, null);
  assert.equal(last?.text, "Pasted nowhere.");
  assert.equal(last?.typed, false);
});

test("a failure with no text falls back to the session's insert", () => {
  const history = [
    row({ id: 7, text: "", status: "failed", errorMessage: "401 Unauthorized", createdAt: stored(8) }),
    row({ id: 6, text: "Book the room.", createdAt: stored(5) }),
  ];
  const last = lastDictation(history, { text: "Book the room.", historyId: 6, at: at(5) }, { historyId: 7, at: at(8) });
  assert.deepEqual(last, { text: "Book the room.", at: at(5), raw: null, typed: true });
});

test("the insert's own row supplies the text before cleanup", () => {
  const history = [row({ id: 6, text: "Book the room.", rawText: "book the room", createdAt: stored(5) })];
  const last = lastDictation(history, { text: "Book the room.", historyId: 6, at: at(5) }, null);
  assert.equal(last?.raw, "book the room");
});

test("with nothing from this session, the newest row that has text", () => {
  const history = [
    row({ id: 9, text: "", status: "failed", createdAt: stored(9) }),
    row({ id: 8, text: "From yesterday.", rawText: "from yesterday", createdAt: stored(2) }),
  ];
  assert.deepEqual(lastDictation(history, null, null), {
    text: "From yesterday.",
    at: at(2),
    raw: "from yesterday",
    typed: true,
  });
});

test("nothing dictated anywhere is null", () => {
  assert.equal(lastDictation([], null, null), null);
  assert.equal(lastDictation([row({ text: "", status: "failed" })], null, { historyId: 1, at: at(1) }), null);
});
