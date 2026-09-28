import assert from "node:assert/strict";
import test from "node:test";
import { NAV, INNER_VIEWS, DEFAULT_VIEW, childrenOf, groupOf, isGroup, railIdOf } from "../src/lib/navigation.js";

const topLevel = ["dictation", "meetings", "notes"];
const groups = {
  settings: ["shortcuts", "meeting-settings", "vocabulary", "transcription", "intelligence", "local-llm", "providers", "calendar"],
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
  assert.equal(groupOf("meeting-settings"), "settings");
  assert.equal(groupOf("vocabulary"), "settings");
  assert.equal(groupOf("meetings"), null);
  assert.equal(groupOf("diagnostics"), "developer");
});

test("an inner view's id does not collide with any nav id", () => {
  const leaves = NAV.flatMap((entry) => isGroup(entry) ? [...childrenOf(entry)] : [entry]);
  const navIds = [...leaves.map((leaf) => leaf.id), ...NAV.filter(isGroup).map((group) => group.id)];
  for (const inner of INNER_VIEWS) {
    assert.ok(!navIds.includes(inner.id), `${inner.id} collides with a nav id`);
  }
});

test("railIdOf maps an inner view to its parent, and leaves a rail id alone", () => {
  assert.equal(railIdOf("notes-upload"), "notes");
  assert.equal(railIdOf("vocabulary"), "vocabulary");
});
