import { test } from "node:test";
import assert from "node:assert/strict";
import { noteMeta, parseParticipants } from "../src/lib/noteMeta.js";
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
  audioDurationSeconds: 1847, createdAt: "2026-08-26T09:00:00Z", updatedAt: "2026-08-26T09:31:00Z",
  ...overrides,
});
const folders: Folder[] = [
  { id: 1, name: "Inbox", isDefault: true, sortOrder: 0 },
  { id: 3, name: "Internal", isDefault: false, sortOrder: 2 },
];

test("participants are their display names, in stored order", () => {
  assert.deepEqual(parseParticipants(FIXTURE_101), ["Anna Lindqvist", "Ravi Menon"]);
});

test("no participants, or participants that will not parse, are none", () => {
  assert.deepEqual(parseParticipants(null), []);
  assert.deepEqual(parseParticipants("not json"), []);
});

test("a participant with only an email is shown by the email", () => {
  assert.deepEqual(parseParticipants(JSON.stringify([{ email: "jonas@nordbank.example" }])), ["jonas@nordbank.example"]);
  assert.deepEqual(parseParticipants(JSON.stringify([{ displayName: null, email: "jonas@nordbank.example" }])), ["jonas@nordbank.example"]);
});

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

test("participants come through noteMeta too", () => {
  assert.deepEqual(noteMeta(note(), folders).participants, ["Anna Lindqvist", "Ravi Menon"]);
  assert.deepEqual(noteMeta(note({ participants: null }), folders).participants, []);
});
