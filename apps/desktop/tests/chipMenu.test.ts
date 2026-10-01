import { test } from "node:test";
import assert from "node:assert/strict";
import { chosenOption, menuGroups, nextIndex, openIndex, type MenuOption } from "../src/lib/chipMenu.js";

const option = (id: string, more: Partial<MenuOption> = {}): MenuOption => ({
  id,
  label: id,
  group: null,
  current: false,
  disabled: false,
  reason: null,
  ...more,
});

const three = [option("a"), option("b", { disabled: true, reason: "needs a key" }), option("c")];

test("ArrowDown and ArrowUp move one row, and greyed rows are not skipped", () => {
  assert.equal(nextIndex(three, 0, "ArrowDown"), 1);
  assert.equal(nextIndex(three, 2, "ArrowUp"), 1);
});

test("the arrows stop at the ends rather than wrapping", () => {
  assert.equal(nextIndex(three, 2, "ArrowDown"), 2);
  assert.equal(nextIndex(three, 0, "ArrowUp"), 0);
});

test("Home and End go to the first and the last row", () => {
  assert.equal(nextIndex(three, 1, "Home"), 0);
  assert.equal(nextIndex(three, 1, "End"), 2);
});

test("the footer is the row after the last option", () => {
  assert.equal(nextIndex(three, 2, "ArrowDown", true), 3);
  assert.equal(nextIndex(three, 0, "End", true), 3);
  assert.equal(nextIndex(three, 3, "ArrowUp", true), 2);
  assert.equal(nextIndex(three, 3, "Home", true), 0);
});

test("from no row, ArrowDown goes to the first and ArrowUp to the last", () => {
  assert.equal(nextIndex(three, -1, "ArrowDown"), 0);
  assert.equal(nextIndex(three, -1, "ArrowUp"), 2);
});

test("a key that does not move, or a menu with no rows, gives null", () => {
  assert.equal(nextIndex(three, 0, "Enter"), null);
  assert.equal(nextIndex(three, 0, "a"), null);
  assert.equal(nextIndex([], -1, "ArrowDown"), null);
});

test("opening lands on the current option, or on the first row when none is current", () => {
  assert.equal(openIndex([option("a"), option("b", { current: true }), option("c")]), 1);
  assert.equal(openIndex(three), 0);
  assert.equal(openIndex([]), 0);
});

test("a greyed row is focusable but cannot be chosen", () => {
  assert.equal(chosenOption(three, 1), null);
  assert.equal(chosenOption(three, 0)?.id, "a");
});

test("the current option can be chosen again, and the footer's index is no option", () => {
  const withCurrent = [option("a", { current: true }), option("b")];
  assert.equal(chosenOption(withCurrent, 0)?.id, "a");
  assert.equal(chosenOption(withCurrent, 2), null);
  assert.equal(chosenOption(withCurrent, -1), null);
});

test("groups are runs of options in the order given, keeping each option's index", () => {
  const groups = menuGroups([
    option("default"),
    option("whisper", { group: "On this computer" }),
    option("parakeet", { group: "On this computer" }),
    option("openai", { group: "Cloud" }),
  ]);
  assert.deepEqual(
    groups.map((g) => [g.group, g.items.map((item) => [item.option.id, item.index])]),
    [
      [null, [["default", 0]]],
      ["On this computer", [["whisper", 1], ["parakeet", 2]]],
      ["Cloud", [["openai", 3]]],
    ],
  );
});

test("a group named again after another one starts a new run, so the rows stay in keyboard order", () => {
  const groups = menuGroups([option("a", { group: "X" }), option("b", { group: "Y" }), option("c", { group: "X" })]);
  assert.deepEqual(groups.map((g) => g.group), ["X", "Y", "X"]);
  assert.equal(groups[2]?.items[0]?.index, 2);
});
