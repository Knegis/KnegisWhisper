import assert from "node:assert/strict";
import test from "node:test";
import { NAV, INNER_VIEWS, DEFAULT_VIEW, RAIL, SETTINGS_HOME, SETTINGS_PANEL, childrenOf, groupOf, isGroup, railOf, srSuffixOf } from "../src/lib/navigation.js";

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

test("the Settings panel lists both groups, Developer under its own heading", () => {
  assert.deepEqual(SETTINGS_PANEL, [
    { group: "settings", heading: null },
    { group: "developer", heading: "Developer" },
  ]);
  // Every group NAV has, in NAV's order, so a group added there cannot be
  // left out of the only panel that lists group children.
  assert.deepEqual(SETTINGS_PANEL.map((section) => section.group), NAV.filter(isGroup).map((group) => group.id));
  // The heading is the group's own label, not a second name for it.
  const developer = NAV.filter(isGroup).find((group) => group.id === "developer");
  assert.equal(SETTINGS_PANEL[1].heading, developer?.label);
});

test("SETTINGS_HOME is a Settings child, so the rail's Settings opens a screen", () => {
  const settings = NAV.filter(isGroup).find((group) => group.id === "settings");
  assert.ok(settings);
  assert.ok(childrenOf(settings).some((child) => child.id === SETTINGS_HOME));
  assert.equal(railOf(SETTINGS_HOME), "settings");
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
