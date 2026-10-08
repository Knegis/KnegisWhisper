import { test } from "node:test";
import assert from "node:assert/strict";
import { acknowledgeNote, noteHasPerson } from "../src/lib/noteList.js";
import type { NoteListItem } from "../src/lib/ipc.js";
const note = (id: number, updatedAt: string, trashedAt: string | null = null): NoteListItem => ({ id, title: `Note ${id}`, updatedAt, trashedAt, noteType: "personal", folderId: null, participants: null, audioDurationSeconds: null, createdAt: null, hasTranscript: false });
test("committed metadata replaces the entry and keeps newest-first ordering and the cap", () => {
  assert.deepEqual(acknowledgeNote([note(1, "2026-01-01"), note(2, "2026-01-02")], note(1, "2026-01-03"), 1, 2).map(item => item.id), [1, 2]);
  assert.deepEqual(acknowledgeNote([note(1, "2026-01-01"), note(2, "2026-01-02")], note(3, "2026-01-03"), 3, 2).map(item => item.id), [3, 2]);
});
test("a missing or trashed note is removed rather than kept as an editable entry", () => {
  for (const saved of [null, note(1, "2026-01-03", "2026-01-03")]) {
    assert.deepEqual(acknowledgeNote([note(1, "2026-01-01"), note(2, "2026-01-02")], saved, 1, 100).map(item => item.id), [2]);
  }
});

test("person matching keeps imported case and whitespace rules without adding unrelated notes", () => {
  assert.ok(noteHasPerson({ ...note(1, "today"), participants: JSON.stringify([{email:" Anna@Example.com\t"}]) }, "anna@example.com"));
  assert.ok(!noteHasPerson({ ...note(1, "today"), participants: "not JSON" }, "anna@example.com"));
});
