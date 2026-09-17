import assert from "node:assert/strict";
import test from "node:test";
import { initialHistory, navigationHistory as reduce, type NavigationHistory, type Destination } from "../src/lib/navigationHistory.js";
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
