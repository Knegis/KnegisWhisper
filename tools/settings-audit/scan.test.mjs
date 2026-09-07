#!/usr/bin/env node
/**
 * Self-tests for the settings-key gate.
 *
 * These exist for the reason `tools/domain-scanner/scan.test.mjs` and
 * `tools/integration-audit/verify-parity.test.mjs` exist, and the lesson is
 * recorded in docs/ENGINEERING_LOG.md: **a check that matches nothing passes
 * trivially.** "The committed tree is clean" and "the rules work" are different
 * claims, and a green gate only ever demonstrates the first.
 *
 * That warning is sharper here than anywhere else in this repository. This gate
 * was written because three settings existed that nothing shipped could change,
 * and all three had passing unit tests -- a test sets the value directly, so it
 * exercises the branch and never asks whether anybody can reach it. A gate
 * against that failure which was itself only ever run green would be the same
 * mistake a third time.
 *
 * Every case starts from the **real committed inputs** and mutates one thing.
 * The first two reconstruct KNEG-99 as it actually stood: `history.enabled`
 * honoured before every write, with no command able to change it.
 *
 * Usage: node --test tools/settings-audit/scan.test.mjs
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  violations,
  backendSources,
  resolveKey,
  normalise,
  ipcWrapper,
  usesWrapper,
} from "./scan.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const read = (p) => JSON.parse(readFileSync(join(REPO_ROOT, p), "utf8"));
const readSource = (relative) => {
  const path = join(REPO_ROOT, relative);
  return existsSync(path) ? readFileSync(path, "utf8") : null;
};

/** The real inputs, cloned so a test can mutate them freely. */
function inputs() {
  const backend = backendSources(REPO_ROOT);
  return {
    manifest: read("config/settings-keys.json"),
    backend,
    lib: backend["lib.rs"],
    surfaces: read("config/ui-surfaces.json"),
    ipc: readFileSync(join(REPO_ROOT, "apps/desktop/src/lib/ipc.ts"), "utf8"),
    readSource,
  };
}

const rules = (found) => [...new Set(found.map((v) => v.rule))].sort();

/* ------------------------------------------------------------ the clean tree */

test("the committed tree passes", () => {
  assert.deepEqual(violations(inputs()), []);
});

/* -------------------------------------------- KNEG-99, reconstructed exactly */

test("a key the backend honours and nothing declares is caught", () => {
  const i = inputs();
  delete i.manifest.keys["history.enabled"];
  const found = violations(i);
  assert.ok(
    found.some((v) => v.rule === "unlisted-key" && v.key === "history.enabled"),
    `expected unlisted-key for history.enabled, got ${JSON.stringify(rules(found))}`
  );
});

test("a declared key with no command that writes it is caught", () => {
  const i = inputs();
  // The state KNEG-99 was actually in: the key read before every history row,
  // and settable only by editing SQLite by hand. `undeclared-writer` fires
  // alongside because the command does exist today; what matters is that the
  // key cannot pass with no writer named.
  delete i.manifest.keys["history.enabled"].writtenBy;
  const found = violations(i);
  assert.ok(
    found.some((v) => v.rule === "unwritable-key" && v.key === "history.enabled"),
    `expected unwritable-key, got ${JSON.stringify(rules(found))}`
  );
});

/* ------------------------------------------------- the scanner fails closed */

test("a key expression the scanner cannot resolve fails rather than being skipped", () => {
  const i = inputs();
  i.backend["dictation.rs"] = i.backend["dictation.rs"].replace(
    "get_or(self.db.pool(), HISTORY_ENABLED_KEY, true)",
    "get_or(self.db.pool(), whatever_this_returns(), true)"
  );
  const found = violations(i);
  assert.ok(
    found.some((v) => v.rule === "unresolvable-key-expression"),
    `a shape the scanner does not understand must be loud, got ${JSON.stringify(rules(found))}`
  );
});

test("removing the hotkey family's declaration does not silently drop five keys", () => {
  const i = inputs();
  delete i.manifest.expressions[".settings_key()"];
  const found = violations(i);
  assert.ok(
    found.some((v) => v.rule === "unresolvable-key-expression"),
    "the derived family must stop resolving loudly, not quietly"
  );
  // And the five keys it produced now have no call site, which is the second
  // half of the same fact rather than a separate defect.
  assert.ok(found.some((v) => v.rule === "phantom-key" && v.key === "hotkey.dictation"));
});

/* ---------------------------------------------------- the writer must be real */

test("the generic settings_set cannot be claimed as a writer", () => {
  const i = inputs();
  i.manifest.keys["history.enabled"].writtenBy = ["settings_set"];
  const found = violations(i);
  assert.ok(
    found.some((v) => v.rule === "generic-writer" && v.key === "history.enabled"),
    `a command that writes every key is evidence for none, got ${JSON.stringify(rules(found))}`
  );
});

test("a writer that is not a command is caught", () => {
  const i = inputs();
  i.manifest.keys["history.enabled"].writtenBy = ["history_set_enabled_someday"];
  const found = violations(i);
  assert.ok(found.some((v) => v.rule === "phantom-command"));
});

test("a command generate_handler! does not register is caught", () => {
  const i = inputs();
  const before = i.lib;
  i.lib = i.lib.replace(/commands::history_set_enabled,\r?\n/, "");
  assert.notEqual(i.lib, before, "the mutation must remove the command on LF and CRLF checkouts");
  i.backend["lib.rs"] = i.lib;
  const found = violations(i);
  assert.ok(
    found.some((v) => v.rule === "unregistered-command" && v.key === "history.enabled"),
    `an unregistered command is unreachable from the renderer, got ${JSON.stringify(rules(found))}`
  );
});

test("a writer that does not write this key is caught", () => {
  const i = inputs();
  // A real, registered command -- but one that writes a different key. Without
  // this rule the manifest would be a second place to write an unchecked name.
  i.manifest.keys["history.enabled"].writtenBy = ["meeting_set_language"];
  const found = violations(i);
  assert.ok(
    found.some((v) => v.rule === "command-does-not-write-the-key" && v.key === "history.enabled"),
    `expected the body check to bite, got ${JSON.stringify(rules(found))}`
  );
});

test("a writer the manifest forgot is caught", () => {
  const i = inputs();
  i.manifest.keys["transcription.route"].writtenBy = ["dictation_use_local"];
  const found = violations(i);
  assert.ok(
    found.some((v) => v.rule === "undeclared-writer" && v.key === "transcription.route"),
    `dictation_set_cloud_provider also writes it, got ${JSON.stringify(rules(found))}`
  );
});

test("a key called internal that a command does write is caught", () => {
  const i = inputs();
  delete i.manifest.keys["history.enabled"].writtenBy;
  delete i.manifest.keys["history.enabled"].ui;
  delete i.manifest.keys["history.enabled"].reachable;
  i.manifest.keys["history.enabled"].internal = "nobody sets this";
  const found = violations(i);
  assert.ok(
    found.some((v) => v.rule === "internal-key-has-a-writer"),
    `calling a key internal must not be a way to opt out, got ${JSON.stringify(rules(found))}`
  );
});

/* ------------------------------------------------------------- reachability */

test("reachability must be declared, not defaulted", () => {
  const i = inputs();
  delete i.manifest.keys["history.enabled"].reachable;
  assert.ok(violations(i).some((v) => v.rule === "missing-reachability"));
});

test("a screen that never calls the writer is not a way in", () => {
  const i = inputs();
  // Integrations > MCP is built and real, and its component calls nothing that
  // writes this key -- the shape meetings.detect was in until KNEG-114.
  i.manifest.keys["history.enabled"].ui = "Integrations > MCP";
  const found = violations(i);
  assert.ok(
    found.some((v) => v.rule === "unreachable-writer" && v.key === "history.enabled"),
    `naming a screen must not be enough, got ${JSON.stringify(rules(found))}`
  );
});

test("importing a wrapper is not using it", () => {
  // The mutation KNEG-114's fix was checked against, and the gate survived it
  // until the import statements were stripped: replacing the call while leaving
  // `setMeetingDetectLists` in the import list kept `Settings > Meetings`
  // looking like a way in.
  const component = [
    `import { setMeetingDetectLists } from "../lib/ipc";`,
    `export function Screen() {`,
    `  return null;`,
    `}`,
  ].join("\n");
  assert.equal(usesWrapper(component, "setMeetingDetectLists"), false);
});

test("a wrapper called by reference is using it", () => {
  // `IntegrationsApi.tsx` really does this, and requiring a parenthesis after
  // the name reported localApi.enabled as unreachable — a setting whose
  // screen has had a working button since it was written.
  const component = [
    `import { startLocalApi, stopLocalApi } from "../lib/ipc";`,
    `void guard(api.running ? stopLocalApi : startLocalApi);`,
  ].join("\n");
  assert.equal(usesWrapper(component, "startLocalApi"), true);
});

test("a surface the registry does not declare is caught", () => {
  const i = inputs();
  i.manifest.keys["history.enabled"].ui = "Settings > Somewhere";
  assert.ok(violations(i).some((v) => v.rule === "unknown-ui-surface"));
});

test("a surface that is not built cannot be a way in", () => {
  const i = inputs();
  i.surfaces.surfaces["Dictation > History"].built = false;
  const found = violations(i);
  assert.ok(
    found.some((v) => v.rule === "unbuilt-surface" && v.key === "history.enabled"),
    `expected unbuilt-surface, got ${JSON.stringify(rules(found))}`
  );
});

test("an unreachable key must say why", () => {
  // Declared here rather than borrowed from the tree. Every key is reachable
  // as of KNEG-114, so a test that deletes some real key's note would have
  // gone quietly green the moment the backlog emptied — which is the failure
  // this whole file exists to refuse.
  const i = inputs();
  i.manifest.keys["history.enabled"].reachable = false;
  delete i.manifest.keys["history.enabled"].note;
  assert.ok(violations(i).some((v) => v.rule === "incomplete-reachability"));
});

/* ---------------------------------------------- the manifest cannot drift */

test("a key nothing in the backend uses is caught", () => {
  const i = inputs();
  i.manifest.keys["history.wasEnabledOnce"] = {
    what: "a key that used to exist",
    writtenBy: ["history_set_enabled"],
    ui: "Dictation > History",
    reachable: true,
  };
  assert.ok(violations(i).some((v) => v.rule === "phantom-key"));
});

test("an expression nothing passes is caught", () => {
  const i = inputs();
  i.manifest.expressions["legacy_key()"] = { kind: "renderer-supplied", why: "it used to" };
  assert.ok(violations(i).some((v) => v.rule === "phantom-expression"));
});

test("a derived family must resolve to a function and a test that exist", () => {
  for (const [field, value] of [
    ["file", "crates/platform/src/hotkey_moved.rs"],
    ["fn", "settings_key_renamed"],
    ["test", "a_test_nobody_wrote"],
  ]) {
    const i = inputs();
    i.manifest.expressions[".settings_key()"].derivedFrom[field] = value;
    const found = violations(i);
    assert.ok(
      found.some((v) => v.rule === "phantom-derivation"),
      `a stale derivedFrom.${field} must be caught, got ${JSON.stringify(rules(found))}`
    );
  }
});

/* -------------------------------------------------- the resolver's own rules */

test("a receiver is dropped so one family is one expression", () => {
  assert.equal(normalise("&action.settings_key()"), ".settings_key()");
  assert.equal(normalise("&HotkeyAction::Dictation.settings_key()"), ".settings_key()");
  const path = "crate::dictation::ROUTE_SETTINGS_KEY";
  assert.equal(normalise(path), path, "a path is not a method call");
});

test("an unknown constant resolves to null rather than to nothing", () => {
  const resolver = { constants: new Map(), helpers: new Map(), expressions: {} };
  assert.equal(resolveKey("SOME_KEY", resolver), null);
  assert.deepEqual(resolveKey('"a.literal"', resolver), { kind: "literal", keys: ["a.literal"] });
});

test("the IPC wrapper for a command is found by its invoke, not by its name", () => {
  const ipc = readFileSync(join(REPO_ROOT, "apps/desktop/src/lib/ipc.ts"), "utf8");
  assert.equal(ipcWrapper("history_set_enabled", ipc), "setHistoryEnabled");
  assert.equal(ipcWrapper("meeting_set_detect_lists", ipc), "setMeetingDetectLists");
  assert.equal(ipcWrapper("no_such_command", ipc), null);
});
