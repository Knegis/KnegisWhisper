import assert from "node:assert/strict";
import test from "node:test";
import { initialHistory, navigationHistory as reduce, type NavigationHistory, type Destination, settingsReturnDestination } from "../src/lib/navigationHistory.js";
const visit = (state: NavigationHistory, destination: Destination) => reduce(state, { type: "visit", destination });
test("back and forward retain destinations and stop at each boundary", () => {
  assert.equal(reduce(initialHistory, { type: "back" }), initialHistory);
  const second = visit(initialHistory, { view: "providers" });
  const back = reduce(second, { type: "back" });
  assert.deepEqual(back.entries[back.index], { view: "dictation" });
  assert.deepEqual(reduce(back, { type: "forward" }), second);
  assert.equal(reduce(second, { type: "forward" }), second);
});
test("new navigation after going back drops the forward branch; repeated pages do not", () => {
  const second = visit(initialHistory, { view: "providers" });
  const back = reduce(second, { type: "back" });
  assert.equal(visit(back, { view: "dictation" }), back);
  const branch = visit(back, { view: "vocabulary" });
  assert.deepEqual(branch.entries, [{ view: "dictation" }, { view: "vocabulary" }]);
});
test("default note selection fills the Notes entry and individual notes remain navigable", () => {
  const notes = visit(initialHistory, { view: "notes" });
  const first = visit(notes, { view: "notes", noteId: 1 });
  assert.equal(first.entries.length, 2);
  const second = visit(first, { view: "notes", noteId: 2 });
  const back = reduce(second, { type: "back" });
  assert.deepEqual(back.entries[back.index], { view: "notes", noteId: 1 });
  assert.equal(visit(back, { view: "notes" }), back);
  assert.deepEqual(reduce(back, { type: "forward" }), second);
});
// KNEG-406: a search hit in a meeting transcript opens the note at a row. The
// row travels in the destination, so Back and Forward show it again.
test("a row in the note already open replaces its entry, and choosing it again is a new request", () => {
  const open = visit(visit(initialHistory, { view: "notes" }), { view: "notes", noteId: 1 });
  const row = visit(open, { view: "notes", noteId: 1, transcriptRow: 4 });
  // No Back step to the same note without the row.
  assert.equal(row.entries.length, open.entries.length);
  assert.deepEqual(row.entries[row.index], { view: "notes", noteId: 1, transcriptRow: 4 });
  // The same hit again is a new object, so the page shows the row again.
  const again = visit(row, { view: "notes", noteId: 1, transcriptRow: 4 });
  assert.notEqual(again.entries[again.index], row.entries[row.index]);
  // A visit to the open note with no row is nothing new, and keeps the row.
  assert.equal(visit(row, { view: "notes", noteId: 1 }), row);
});
test("a row in another note is a new entry, and Back returns to the row", () => {
  const open = visit(visit(initialHistory, { view: "notes" }), { view: "notes", noteId: 1 });
  const row = visit(open, { view: "notes", noteId: 2, transcriptRow: 0 });
  assert.equal(row.entries.length, open.entries.length + 1);
  const away = visit(row, { view: "dictation" });
  const back = reduce(away, { type: "back" });
  assert.deepEqual(back.entries[back.index], { view: "notes", noteId: 2, transcriptRow: 0 });
});

// KNEG-548: return must follow the current branch, not a later task in Forward.
test("settings returns preserve task destinations through multiple settings screens", () => {
  for (const destination of [
    { view: "dictation" }, { view: "meetings" }, { view: "notes-upload" },
    { view: "notes", noteId: 42, transcriptRow: 7, summaryFocus: true },
  ] satisfies Destination[]) {
    const task = visit(initialHistory, destination);
    const settings = visit(visit(task, { view: "intelligence" }), { view: "local-llm" });
    assert.deepEqual(settingsReturnDestination(settings), destination);
    const returned = visit(settings, settingsReturnDestination(settings));
    assert.deepEqual(returned.entries[returned.index], destination);
    assert.equal(reduce(returned, { type: "back" }).entries[returned.index - 1]?.view, "local-llm");
    assert.deepEqual(settingsReturnDestination(task), destination);
  }
});
test("settings return ignores forward entries and falls back when history has no task", () => {
  const settings = visit(initialHistory, { view: "providers" });
  const later = visit(settings, { view: "meetings" });
  assert.deepEqual(settingsReturnDestination(reduce(later, { type: "back" })), { view: "dictation" });
  assert.deepEqual(settingsReturnDestination({ entries: [{ view: "diagnostics" }], index: 0 }), { view: "dictation" });
});
