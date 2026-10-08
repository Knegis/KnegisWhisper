import { test } from "node:test";
import assert from "node:assert/strict";
import { groupNotesByFolder } from "../src/lib/noteGroups.js";
import type { Folder, Note } from "../src/generated/ipc-types.js";

const note = (id: number, folderId: number | null): Note => ({
  id, title: `Note ${id}`, content: "", recordingDetails: null, noteType: "note", enhancedContent: null,
  folderId, transcript: null, participants: null, audioDurationSeconds: null, transcribedBy: null, createdAt: null, updatedAt: null,
});
const folder = (id: number, name: string): Folder => ({ id, name, isDefault: false, sortOrder: id });

test("unfiled notes keep their order, and every folder is listed in order", () => {
  const groups = groupNotesByFolder(
    [note(1, 2), note(2, null), note(3, 1), note(4, null), note(5, 2)],
    [folder(1, "Work"), folder(2, "Home")],
  );
  assert.deepEqual(groups.unfiled.map((n) => n.id), [2, 4]);
  assert.deepEqual(groups.folders.map((g) => [g.folder.name, g.notes.map((n) => n.id)]), [["Work", [3]], ["Home", [1, 5]]]);
});

test("a folder with no notes is still a group", () => {
  const groups = groupNotesByFolder([note(1, null)], [folder(7, "Empty")]);
  assert.deepEqual(groups.folders, [{ folder: folder(7, "Empty"), notes: [] }]);
});

test("a note whose folder is gone counts as unfiled", () => {
  const groups = groupNotesByFolder([note(1, 99)], [folder(1, "Work")]);
  assert.deepEqual(groups.unfiled.map((n) => n.id), [1]);
  assert.deepEqual(groups.folders[0]?.notes, []);
});

test("nothing in, nothing out", () => {
  assert.deepEqual(groupNotesByFolder([], []), { unfiled: [], folders: [] });
});
