import { test } from "node:test";
import assert from "node:assert/strict";
import { hasStoredTranscript, noteMeta, transcribedLine } from "../src/lib/noteMeta.js";
import { parseStoredTime } from "../src/lib/dayGroups.js";
import type { Folder, Note } from "../src/generated/ipc-types.js";

// Fixture 101's participants, exactly as `lib/devFixtures.ts` stores them.
const FIXTURE_101 = JSON.stringify([
  { displayName: "Anna Lindqvist", email: "anna@example.com" },
  { displayName: "Ravi Menon", email: "ravi@example.com" },
]);

const note = (overrides: Partial<Note> = {}): Note => ({
  id: 101, title: "Weekly sync — platform", content: "", recordingDetails: null, noteType: "meeting",
  enhancedContent: null, folderId: 3, transcript: null, participants: FIXTURE_101,
  audioDurationSeconds: 1847, transcribedBy: null, createdAt: "2026-08-26T09:00:00Z", updatedAt: "2026-08-26T09:31:00Z",
  ...overrides,
});
const folders: Folder[] = [
  { id: 1, name: "Inbox", isDefault: true, sortOrder: 0 },
  { id: 3, name: "Internal", isDefault: false, sortOrder: 2 },
];

// A note's people are read by `lib/people.ts` (KNEG-362), and tested in `people.test.ts`.

test("the folder is named by the note's folderId, and no folder is null", () => {
  assert.equal(noteMeta(note(), folders).folder, "Internal");
  assert.equal(noteMeta(note({ folderId: null }), folders).folder, null);
  // A folder deleted since the list was read: unfiled, as `groupNotesByFolder` treats it.
  assert.equal(noteMeta(note({ folderId: 99 }), folders).folder, null);
});

test("the duration is whole minutes, rounded", () => {
  assert.equal(noteMeta(note(), folders).minutes, Math.round(1847 / 60));
  assert.equal(noteMeta(note(), folders).minutes, 31);
  assert.equal(noteMeta(note({ audioDurationSeconds: null }), folders).minutes, null);
});

test("the date is the stored creation time, read as parseStoredTime reads it", () => {
  assert.deepEqual(noteMeta(note(), folders).date, parseStoredTime("2026-08-26T09:00:00Z"));
  assert.equal(noteMeta(note({ createdAt: null }), folders).date, null);
});

// As Rust's `TranscribedBy` serialises it (`crates/notes-core/src/notes.rs`).
const ON_THIS_COMPUTER = JSON.stringify({ provider: "local", name: "this computer", onThisComputer: true });

test("a transcript made on this computer says so", () => {
  assert.equal(transcribedLine(ON_THIS_COMPUTER), "Transcribed on this computer");
  assert.equal(
    transcribedLine(JSON.stringify({ provider: "local-streaming", name: "this computer", onThisComputer: true })),
    "Transcribed on this computer",
  );
});

test("live captions from elsewhere are named beside it", () => {
  // The ruling on KNEG-368: the bare line could read as "nothing left this computer".
  assert.equal(
    transcribedLine(JSON.stringify({ provider: "local", name: "this computer", onThisComputer: true, liveName: "Deepgram Realtime" })),
    "Transcribed on this computer · live captions by Deepgram Realtime",
  );
  assert.equal(
    transcribedLine(JSON.stringify({ provider: "local", name: "this computer", onThisComputer: true, liveName: "  " })),
    "Transcribed on this computer",
  );
});

test("a transcript made elsewhere names who made it", () => {
  assert.equal(transcribedLine(JSON.stringify({ provider: "openai", name: "OpenAI", onThisComputer: false })), "Transcribed by OpenAI");
  assert.equal(
    transcribedLine(JSON.stringify({ provider: "self-hosted", name: "your self-hosted server", onThisComputer: false })),
    "Transcribed by your self-hosted server",
  );
  assert.equal(
    transcribedLine(JSON.stringify({ provider: "deepgram-realtime", name: "Deepgram Realtime", onThisComputer: false })),
    "Transcribed by Deepgram Realtime",
  );
});

test("no record, or one that will not read, says nothing", () => {
  for (const json of [
    null,
    "",
    "not json",
    "[]",
    "null",
    JSON.stringify({ provider: "openai", onThisComputer: false }),
    JSON.stringify({ provider: "openai", name: "  ", onThisComputer: false }),
    JSON.stringify({ provider: "openai", name: "OpenAI" }),
    JSON.stringify({ provider: "openai", name: "OpenAI", onThisComputer: "no" }),
  ]) {
    assert.equal(transcribedLine(json), null, String(json));
  }
});

test("the line comes through noteMeta too", () => {
  assert.equal(noteMeta(note({ transcribedBy: ON_THIS_COMPUTER }), folders).transcribed, "Transcribed on this computer");
  assert.equal(noteMeta(note(), folders).transcribed, null);
});

test("a note has a transcript when its stored array has a row, and not otherwise", () => {
  // KNEG-377: a wide window gave a plain note a 340px column saying "This
  // note has no transcript." Read from the note, so the layout is right
  // before the rows load rather than jumping after.
  assert.equal(hasStoredTranscript(null), false);
  assert.equal(hasStoredTranscript(""), false);
  assert.equal(hasStoredTranscript("[]"), false);
  assert.equal(hasStoredTranscript("not json"), false);
  assert.equal(hasStoredTranscript('{"text":"not an array"}'), false);
  assert.equal(hasStoredTranscript(JSON.stringify([{ speaker: "Anna", text: "Two more days." }])), true);
});
