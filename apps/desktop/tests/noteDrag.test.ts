import { test } from "node:test";
import assert from "node:assert/strict";
import { NOTE_DRAG_TYPE, carriesNote, draggedNoteId, folderChange } from "../src/lib/noteDrag.js";

test("a drag carries a note only when it names the note type", () => {
  assert.equal(carriesNote([NOTE_DRAG_TYPE]), true);
  assert.equal(carriesNote(["text/plain", NOTE_DRAG_TYPE]), true);
  assert.equal(carriesNote(["Files"]), false);
  assert.equal(carriesNote([]), false);
  assert.equal(carriesNote(undefined), false);
});

test("the dragged note id is read back as a number, or nothing", () => {
  const payload = (value: string) => ({ getData: (type: string) => (type === NOTE_DRAG_TYPE ? value : "") });
  assert.equal(draggedNoteId(payload("42")), 42);
  assert.equal(draggedNoteId(payload("")), null);
  assert.equal(draggedNoteId(payload("forty-two")), null);
  assert.equal(draggedNoteId(payload("4.2")), null);
  assert.equal(draggedNoteId({ getData: () => "" }), null);
});

test("a drop is a change only when the folder differs", () => {
  assert.equal(folderChange(3, 3), false);
  assert.equal(folderChange(null, null), false);
  assert.equal(folderChange(3, null), true);
  assert.equal(folderChange(null, 3), true);
  assert.equal(folderChange(3, 4), true);
});
