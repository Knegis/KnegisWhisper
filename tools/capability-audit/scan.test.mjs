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

import { collect, markEnabled, matchesWindowPattern, securityCapabilities, violations } from "./scan.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
// Historical four-window fixtures for KNEG-168/171.
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
  assert.deepEqual(real.labels.map(({ label }) => label).sort(), ["dictation-languages", "main", "meeting", "overlay", "update"]);
  assert.equal(new Set(real.labels.map(({ source }) => source)).size, 2);
  assert.equal(real.capabilities.length, 3);
  // KNEG-171: a file in capabilities/ is not a capability until security.capabilities names it.
  assert.deepEqual(real.capabilities.map(({ identifier, enabled }) => ({ identifier, enabled })), [
    { identifier: "default", enabled: true },
    { identifier: "notifications", enabled: true },
    { identifier: "titlebar", enabled: true },
  ]);
  // Four production panels plus the import and recording-deletion mock webviews.
  assert.equal(real.builders.length, 6);
  assert.deepEqual(
    real.builders.filter(({ kind }) => kind === "literal").map(({ file, arg }) => ({ file, arg })),
    [
      { file: "apps/desktop/src-tauri/src/media.rs", arg: "main" },
      { file: "apps/desktop/src-tauri/src/recording_attach.rs", arg: "main" },
    ],
  );
  // KNEG-221: the four panel windows are transparent, and both halves of the
  // macOS configuration that makes them compile there are set.
  assert.deepEqual(
    real.transparency.sites.map(({ file }) => file),
    [
      "apps/desktop/src-tauri/src/meeting_window.rs",
      "apps/desktop/src-tauri/src/overlay.rs",
      "apps/desktop/src-tauri/src/persistent_overlay.rs",
      "apps/desktop/src-tauri/src/update_window.rs",
    ],
  );
  assert.equal(real.transparency.cargoFeature, true);
  assert.equal(real.transparency.configFlag, true);
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

test("KNEG-171 regression: a capability file the allow-list does not name covers nothing", () => {
  const found = violations({
    labels,
    capabilities: [allCovered[0], { ...allCovered[1], enabled: false }],
    builders: noBuilders,
    exempt: {},
  });
  assert.deepEqual(
    found.filter(({ rule }) => rule.startsWith("A.")).map(({ subject }) => subject).sort(),
    ["meeting", "update"]
  );
  assert.deepEqual(
    found.filter(({ rule }) => rule.startsWith("E.")).map(({ subject, detail }) => [subject, /security\.capabilities/.test(detail)]),
    [["notifications", true]]
  );
});

test("the allow-list reading: unset or empty enables every file, a list enables only what it names", () => {
  const files = [
    { file: "capabilities/default.json", identifier: "default", windows: ["main"], permissions: [] },
    { file: "capabilities/notifications.json", identifier: "notifications", windows: ["meeting"], permissions: [] },
  ];
  const enabled = (marked) => marked.map(({ identifier, enabled }) => `${identifier}:${enabled}`);
  assert.deepEqual(enabled(markEnabled(files, null)), ["default:true", "notifications:true"]);
  assert.deepEqual(enabled(markEnabled(files, ["default"])), ["default:true", "notifications:false"]);
  assert.deepEqual(enabled(markEnabled(files, ["default", "notifications"])), ["default:true", "notifications:true"]);

  const inline = markEnabled(files, [
    "default",
    { identifier: "drag", windows: ["main"], permissions: ["core:window:allow-start-dragging"] },
  ]);
  assert.deepEqual(enabled(inline), ["default:true", "notifications:false", "drag:true"]);
  assert.match(inline[2].file, /tauri\.conf\.json/);

  assert.deepEqual(securityCapabilities(REPO_ROOT), ["default", "notifications", "titlebar"]);
  assert.equal(securityCapabilities(resolve(REPO_ROOT, "tools")), null);
});

test("historical four-window fixture is covered by its two capabilities", () => {
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

test("KNEG-221 regression: a transparent window with neither half of the macOS configuration", () => {
  const found = violations({
    labels,
    capabilities: allCovered,
    builders: noBuilders,
    exempt: {},
    transparency: { sites: [{ file: "src/meeting_window.rs", line: 53 }], cargoFeature: false, configFlag: false },
  });
  assert.deepEqual(rules(found), ["F. transparent window is not configured for macOS"]);
  assert.equal(found[0].subject, "src/meeting_window.rs:53");
  // The message has to name both edits, because enabling either alone leaves a
  // different failure: the feature alone builds but disagrees with the config
  // the Tauri CLI derives, the config alone does not reach `cargo build`.
  assert.match(found[0].detail, /macos-private-api/);
  assert.match(found[0].detail, /macOSPrivateApi/);
});

test("rule F names the half that is missing, and one site reports for all of them", () => {
  const sites = [
    { file: "src/meeting_window.rs", line: 53 },
    { file: "src/overlay.rs", line: 92 },
    { file: "src/update_window.rs", line: 58 },
  ];
  const featureOnly = violations({
    labels,
    capabilities: allCovered,
    builders: noBuilders,
    exempt: {},
    transparency: { sites, cargoFeature: true, configFlag: false },
  });
  assert.deepEqual(rules(featureOnly), ["F. transparent window is not configured for macOS"]);
  assert.equal(featureOnly.length, 1, "one violation for the configuration, not one per call site");
  assert.match(featureOnly[0].detail, /app\.macOSPrivateApi/);
  assert.doesNotMatch(featureOnly[0].detail, /Cargo\.toml/);

  const configOnly = violations({
    labels,
    capabilities: allCovered,
    builders: noBuilders,
    exempt: {},
    transparency: { sites, cargoFeature: false, configFlag: true },
  });
  assert.deepEqual(rules(configOnly), ["F. transparent window is not configured for macOS"]);
  assert.match(configOnly[0].detail, /Cargo\.toml/);
  assert.doesNotMatch(configOnly[0].detail, /app\.macOSPrivateApi/);
});

test("rule F is quiet when both halves are set, and when nothing is transparent", () => {
  const sites = [{ file: "src/overlay.rs", line: 92 }];
  const base = { labels, capabilities: allCovered, builders: noBuilders, exempt: {} };
  assert.deepEqual(violations({ ...base, transparency: { sites, cargoFeature: true, configFlag: true } }), []);
  // A tree with no transparent window needs neither, so the rule must not
  // demand a private API nothing asks for.
  assert.deepEqual(violations({ ...base, transparency: { sites: [], cargoFeature: false, configFlag: false } }), []);
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


test("language popup loses coverage when removed from notifications", () => {
  const real = collect(REPO_ROOT);
  real.capabilities = real.capabilities.map((c) => ({ ...c, windows: c.windows.filter((w) => w !== "dictation-languages") }));
  assert.ok(violations(real).some((v) => v.rule.startsWith("A.") && v.subject === "dictation-languages"));
});

test("a disabled titlebar capability is reported even though main has other coverage", () => {
  const real = collect(REPO_ROOT);
  real.capabilities = real.capabilities.map((c) => c.identifier === "titlebar" ? { ...c, enabled: false } : c);
  assert.ok(violations(real).some((v) => v.rule.startsWith("E.") && v.subject === "titlebar"));
});
