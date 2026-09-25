import { test } from "node:test";
import assert from "node:assert/strict";
import { LIVE_ROW_LIMIT, emptyLiveTranscript, foldTranscriptEvent, liveRows } from "../src/lib/liveTranscript.js";
import type { TranscriptEvent } from "../src/generated/ipc-types.js";

const event = (text: string, timestamp = 1, source: "mic" | "system" = "mic", isFinal = false): TranscriptEvent =>
  ({ text, timestamp, source, isFinal, speaker: null });

test("growth replaces the partial and keeps its first timestamp", () => {
  const first = foldTranscriptEvent(emptyLiveTranscript(), event("hello", 1));
  const grown = foldTranscriptEvent(first, event("hello world", 4));
  assert.equal(grown.rows.length, 0);
  assert.deepEqual(grown.partial.mic, { id: 0, source: "mic", startSeconds: 1, speaker: null, text: "hello world", committed: false });
  assert.equal(grown.nextId, 1);
});

test("non-extending text commits the old partial and opens a new one", () => {
  const state = foldTranscriptEvent(foldTranscriptEvent(emptyLiveTranscript(), event("hello")), event("another", 2));
  assert.deepEqual(state.rows.map(row => [row.text, row.committed]), [["hello", true]]);
  assert.equal(state.partial.mic?.text, "another");
});

test("final text commits and clears, whether extending or replacing", () => {
  const growing = foldTranscriptEvent(foldTranscriptEvent(emptyLiveTranscript(), event("hello")), event("hello world", 2, "mic", true));
  assert.deepEqual(growing.rows.map(row => row.text), ["hello world"]);
  assert.equal(growing.partial.mic, undefined);
  const replacing = foldTranscriptEvent(foldTranscriptEvent(emptyLiveTranscript(), event("hello")), event("goodbye", 2, "mic", true));
  assert.deepEqual(replacing.rows.map(row => row.text), ["hello", "goodbye"]);
  assert.equal(replacing.partial.mic, undefined);
});

test("mic and system partials fold independently and interleave by time", () => {
  let state = emptyLiveTranscript();
  state = foldTranscriptEvent(state, event("system", 3, "system"));
  state = foldTranscriptEvent(state, event("mic", 1));
  state = foldTranscriptEvent(state, event("system more", 4, "system", true));
  assert.deepEqual(liveRows(state).map(row => [row.source, row.startSeconds]), [["mic", 1], ["system", 3]]);
  assert.equal(state.partial.mic?.text, "mic");
});

test("blank text is ignored", () => {
  const state = emptyLiveTranscript();
  assert.equal(foldTranscriptEvent(state, event("  \n ")), state);
});

test("committed rows are capped by dropping the oldest", () => {
  let state = emptyLiveTranscript();
  for (let i = 0; i <= LIVE_ROW_LIMIT; i++) state = foldTranscriptEvent(state, event(String(i), i, "mic", true));
  assert.equal(state.rows.length, LIVE_ROW_LIMIT);
  assert.equal(state.rows[0]?.text, "1");
});

test("folding does not mutate its input", () => {
  const previous = foldTranscriptEvent(emptyLiveTranscript(), event("first"));
  const snapshot = structuredClone(previous);
  foldTranscriptEvent(previous, event("second", 2));
  assert.deepEqual(previous, snapshot);
});
