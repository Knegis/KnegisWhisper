import assert from "node:assert/strict";
import test from "node:test";
import { eyebrowOf, NAV, INNER_VIEWS, DEFAULT_VIEW, RAIL, SETTINGS_PANEL, childrenOf, groupOf, isGroup, railOf, srSuffixOf } from "../src/lib/navigation.js";

const topLevel = ["dictation", "meetings", "notes"];
const groups = {
  settings: ["shortcuts", "meeting-settings", "vocabulary", "transcription", "intelligence", "local-llm", "providers", "calendar"],
  developer: ["api", "cli", "mcp", "diagnostics"],
};

test("the tree retains the intended destinations, grouping, and order", () => {
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

test("group lookup finds the destination's actual group and leaves top-level views alone", () => {
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

test("the rail is the three jobs and Settings, in that order", () => {
  assert.deepEqual(RAIL, ["dictation", "meetings", "notes", "settings"]);
});

test("railOf maps every view to the rail entry that reads as current", () => {
  assert.equal(railOf("notes-upload"), "notes");
  assert.equal(railOf("dictation"), "dictation");
  // A top-level leaf is its own rail entry.
  for (const entry of NAV) if (!isGroup(entry)) assert.equal(railOf(entry.id), entry.id);
  // Both groups live in the Settings panel, Developer included.
  for (const group of NAV.filter(isGroup)) {
    for (const child of childrenOf(group)) assert.equal(railOf(child.id), "settings", child.id);
  }
});

test("the Settings sheet puts every group screen on exactly one tile", () => {
  // Every screen of both NAV groups, once: a screen added to a group cannot be
  // left off the only place that lists group children, nor listed twice.
  const tiles: string[] = SETTINGS_PANEL.flatMap((section) => [...section.views]);
  const screens = NAV.filter(isGroup).flatMap((group) => childrenOf(group).map((child) => child.id));
  assert.deepEqual([...tiles].sort(), [...screens].sort());
  assert.equal(new Set(tiles).size, tiles.length);
  // The sheet's two groups are NAV's two groups: their labels, their screens, their order.
  const groups = NAV.filter(isGroup);
  assert.deepEqual(SETTINGS_PANEL.map((section) => section.heading), groups.map((group) => group.label.toLowerCase()));
  groups.forEach((group, index) => {
    assert.deepEqual(SETTINGS_PANEL[index]?.views, childrenOf(group).map((child) => child.id));
  });
});


test("the Settings child Meetings is named apart from the top-level Meetings, and only it", () => {
  const leaves = NAV.flatMap((entry) => isGroup(entry) ? [...childrenOf(entry)] : [entry]);
  const withSuffix = leaves.filter((leaf) => srSuffixOf(leaf) !== null);
  assert.deepEqual(withSuffix.map((leaf) => leaf.id), ["meeting-settings"]);
  const child = leaves.find((leaf) => leaf.id === "meeting-settings");
  assert.ok(child);
  // The visible label is unchanged, and the accessible name contains it.
  assert.equal(child.label, "Meetings");
  assert.equal(`${child.label}${srSuffixOf(child) ?? ""}`, "Meetings settings");
});

test("a page's eyebrow says which part of the application it belongs to", () => {
  assert.equal(eyebrowOf("transcription"), "settings");
  assert.equal(eyebrowOf("api"), "developer");
  assert.equal(eyebrowOf("notes-upload"), "notes");
  assert.equal(eyebrowOf("dictation"), null);
});
