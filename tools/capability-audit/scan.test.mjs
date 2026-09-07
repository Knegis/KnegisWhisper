#!/usr/bin/env node
/**
 * Self-tests for the Tauri window capability coverage gate.
 *
 * The real tree being clean proves only that today's inputs happen to agree.
 * These cases make each rule fail on purpose, including KNEG-168: a gate made
 * to catch a discarded denied `listen()` promise must not itself be allowed to
 * pass merely because it no longer recognises the bad shape.
 *
 * Usage: node --test tools/capability-audit/scan.test.mjs
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { collect, matchesWindowPattern, violations } from "./scan.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const labels = ["main", "overlay", "meeting", "update"].map((label) => ({
  label,
  source: label === "main" ? "tauri.conf.json" : "Rust pub const LABEL",
}));
const allCovered = [
  { file: "capabilities/default.json", identifier: "default", windows: ["main", "overlay"], permissions: ["core:default"] },
  { file: "capabilities/notifications.json", identifier: "notifications", windows: ["meeting", "update"], permissions: ["core:event:default"] },
];
const noBuilders = [];

const rules = (found) => found.map(({ rule }) => rule);

test("the real tree is clean, and all parser inventories are live", () => {
  const real = collect(REPO_ROOT);
  assert.deepEqual(real.labels.map(({ label }) => label).sort(), ["main", "meeting", "overlay", "update"]);
  assert.equal(new Set(real.labels.map(({ source }) => source)).size, 2);
  assert.equal(real.capabilities.length, 2);
  // Three production panels plus the import and recording-deletion mock webviews.
  assert.equal(real.builders.length, 5);
  assert.deepEqual(
    real.builders.filter(({ kind }) => kind === "literal").map(({ file, arg }) => ({ file, arg })),
    [
      { file: "apps/desktop/src-tauri/src/media.rs", arg: "main" },
      { file: "apps/desktop/src-tauri/src/recording_attach.rs", arg: "main" },
    ],
  );
  assert.deepEqual(violations(real), []);
});

test("KNEG-168 regression: meeting and update have no capability", () => {
  const found = violations({
    labels,
    capabilities: [allCovered[0]],
    builders: noBuilders,
    exempt: {},
  });
  assert.deepEqual(
    found.filter(({ rule }) => rule.startsWith("A.")).map(({ subject }) => subject).sort(),
    ["meeting", "update"]
  );
});

test("all four current labels are covered by the two capabilities", () => {
  assert.deepEqual(violations({ labels, capabilities: allCovered, builders: noBuilders, exempt: {} }), []);
});

test("rule B rejects a capability entry for a window that does not exist", () => {
  const found = violations({
    labels,
    capabilities: [...allCovered, { file: "capabilities/stale.json", identifier: "stale", windows: ["gone"], permissions: [] }],
    builders: noBuilders,
    exempt: {},
  });
  assert.deepEqual(rules(found), ["B. capability names no created window"]);
  assert.equal(found[0].subject, "stale:gone");
});

test("rule C rejects a stale exemption and ignores an absent manifest", () => {
  const stale = violations({ labels, capabilities: allCovered, builders: noBuilders, exempt: { gone: "old exception" } });
  assert.deepEqual(rules(stale), ["C. exemption names no created window"]);
  assert.deepEqual(violations({ labels, capabilities: allCovered, builders: noBuilders, exempt: {} }), []);
});

test("rule D rejects an unresolvable builder and accepts both legitimate forms", () => {
  const good = [
    { file: "src/overlay.rs", line: 87, arg: "LABEL", kind: "same-file LABEL" },
    { file: "src/media.rs", line: 1509, arg: "main", kind: "literal" },
  ];
  assert.deepEqual(violations({ labels, capabilities: allCovered, builders: good, exempt: {} }), []);

  const found = violations({
    labels,
    capabilities: allCovered,
    builders: [...good, { file: "src/future.rs", line: 12, arg: "window_label()", kind: "unresolved" }],
    exempt: {},
  });
  assert.deepEqual(rules(found), ["D. window construction has no resolvable label"]);
  assert.equal(found[0].subject, "src/future.rs:12");
});

test("an exempt label does not trip rule A", () => {
  const found = violations({
    labels,
    capabilities: [allCovered[0]],
    builders: noBuilders,
    exempt: { meeting: "deliberately permissionless" },
  });
  assert.deepEqual(found.map(({ subject }) => subject), ["update"]);
});

test("window matching supports plain labels, star globs, and literal regex characters", () => {
  assert.equal(matchesWindowPattern("meeting", "meeting"), true);
  assert.equal(matchesWindowPattern("panel-*", "panel-a"), true);
  assert.equal(matchesWindowPattern("panel-*", "panel-"), true);
  assert.equal(matchesWindowPattern("panel.*", "panelxanything"), false);
  assert.equal(matchesWindowPattern("panel.*", "panel.anything"), true);
});
