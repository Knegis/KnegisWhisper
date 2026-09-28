import assert from "node:assert/strict";
import test from "node:test";
import { NAV, DEFAULT_VIEW, childrenOf, groupOf, isGroup } from "../src/lib/navigation.js";

const topLevel = ["dictation", "meetings", "notes", "vocabulary", "notes-upload"];
const groups = {
  settings: ["shortcuts", "providers", "transcription", "intelligence", "local-llm", "calendar"],
  developer: ["api", "cli", "mcp", "diagnostics"],
};

test("the rail retains the intended destinations, grouping, and order", () => {
  assert.deepEqual(NAV.filter((entry) => !isGroup(entry)).map((entry) => entry.id), topLevel);
  assert.deepEqual(NAV.filter(isGroup).map((group) => group.id), Object.keys(groups));
  for (const group of NAV.filter(isGroup)) {
    assert.deepEqual(childrenOf(group).map((child) => child.id), groups[group.id]);
    assert.ok(childrenOf(group).length > 0);
  }
});

test("every destination has one unique id, distinct from every group id", () => {
  const leaves = NAV.flatMap((entry) => isGroup(entry) ? [...childrenOf(entry)] : [entry]);
  assert.equal(leaves.length, 15);
  const ids = [...leaves.map((leaf) => leaf.id), ...NAV.filter(isGroup).map((group) => group.id)];
  assert.equal(new Set(ids).size, ids.length);
  assert.equal(DEFAULT_VIEW, "dictation");
});

test("group lookup opens the destination's actual group and leaves top-level views alone", () => {
  for (const entry of NAV) {
    if (isGroup(entry)) {
      for (const child of childrenOf(entry)) assert.equal(groupOf(child.id), entry.id);
    } else {
      assert.equal(groupOf(entry.id), null);
    }
  }
  assert.equal(groupOf("providers"), "settings");
  assert.equal(groupOf("shortcuts"), "settings");
  assert.equal(groupOf("diagnostics"), "developer");
});
