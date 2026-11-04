import { test } from "node:test";
import assert from "node:assert/strict";
import { historyRow } from "../src/lib/historyRow.js";
import type { Transcription } from "../src/generated/ipc-types.js";

// Built from a local date, so the expected `HH:MM` holds in any time zone.
const at = new Date(2026, 8, 29, 16, 12);
const stored = at.toISOString().slice(0, 19).replace("T", " ");

const row = (over: Partial<Transcription>): Transcription => ({
  id: 1,
  text: "Send the notes to Jonas.",
  rawText: null,
  createdAt: stored,
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

test("a completed row is its text over its time", () => {
  const r = historyRow(row({}));
  assert.equal(r.title, "Send the notes to Jonas.");
  assert.equal(r.muted, false);
  assert.equal(r.meta, "16:12");
  assert.equal(r.tooltip, "Send the notes to Jonas.");
  assert.equal(r.copyText, "Send the notes to Jonas.");
  assert.equal(r.typed, true);
});

test("the time is apart from the rest of the meta, so it can be set in the pixel face", () => {
  const plain = historyRow(row({}));
  assert.equal(plain.time, "16:12");
  assert.equal(plain.detail, "");
  const failed = historyRow(row({ status: "failed", errorMessage: "refused", rawText: "send notes" }));
  assert.equal(failed.time, "16:12");
  assert.equal(failed.detail, "refused · Cleaned up");
  assert.equal(failed.meta, "16:12 · refused · Cleaned up");
  const undated = historyRow(row({ createdAt: "not a date", status: "failed", errorMessage: "refused" }));
  assert.equal(undated.time, null);
  assert.equal(undated.detail, "refused");
});

test("a row whose paste failed keeps its text, and says why in the meta", () => {
  // The shape `fail` writes when only the insert failed: text present, status failed.
  const r = historyRow(row({ status: "failed", errorMessage: "could not deliver the paste keystroke: refused" }));
  assert.equal(r.title, "Send the notes to Jonas.");
  assert.equal(r.muted, false);
  assert.equal(r.meta, "16:12 · could not deliver the paste keystroke: refused");
  assert.equal(r.tooltip, "Send the notes to Jonas.");
  assert.equal(r.copyText, "Send the notes to Jonas.");
  assert.equal(r.typed, false);
});

test("a failed row with no text reads Didn't work, and copies the reason", () => {
  const r = historyRow(row({ text: "", status: "failed", errorMessage: "401 Unauthorized" }));
  assert.equal(r.title, "Didn't work");
  assert.equal(r.muted, true);
  assert.equal(r.meta, "16:12 · 401 Unauthorized");
  assert.equal(r.tooltip, null);
  assert.equal(r.copyText, "401 Unauthorized");
});

test("a cleaned-up row says so, and the tooltip carries the text before cleanup and the provider", () => {
  const r = historyRow(row({ rawText: "send the notes to jonas", provider: "openai", model: "gpt-4o-transcribe" }));
  assert.equal(r.meta, "16:12 · Cleaned up");
  assert.equal(
    r.tooltip,
    "Send the notes to Jonas.\nBefore cleanup: send the notes to jonas\nTranscribed by OpenAI (gpt-4o-transcribe)",
  );
});

test("the provider is in the tooltip even without a pre-cleanup text, and a model is optional", () => {
  assert.equal(historyRow(row({ provider: "local" })).tooltip, "Send the notes to Jonas.\nTranscribed by local");
  assert.equal(historyRow(row({ text: "", status: "failed", provider: "groq" })).tooltip, "Transcribed by Groq");
});

test("a row tells a screen reader what pressing it copies", () => {
  assert.equal(historyRow(row({})).copyDescription, "Press to copy this dictation");
  // A paste failure kept its text, so pressing copies the text, not the reason.
  assert.equal(historyRow(row({ status: "failed", errorMessage: "refused" })).copyDescription, "Press to copy this dictation");
  assert.equal(
    historyRow(row({ text: "", status: "failed", errorMessage: "401 Unauthorized" })).copyDescription,
    "Press to copy why it didn't work",
  );
  // Nothing to copy, so nothing to promise.
  assert.equal(historyRow(row({ text: "", status: "failed", errorMessage: null })).copyDescription, null);
});

test("an undated row has no time in its meta", () => {
  assert.equal(historyRow(row({ createdAt: null })).meta, "");
  assert.equal(historyRow(row({ createdAt: null, rawText: "x" })).meta, "Cleaned up");
});
