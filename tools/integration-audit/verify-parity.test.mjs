#!/usr/bin/env node
/**
 * Self-tests for the parity gate's UI-surface rules.
 *
 * These exist for the reason `tools/domain-scanner/scan.test.mjs` exists, and
 * the lesson is recorded in docs/ENGINEERING_LOG.md: **a check that matches
 * nothing passes trivially.** "The committed tree is clean" and "the rules
 * work" are different claims, and a green gate only ever demonstrates the
 * first. Rule 7 was added precisely because rule 5 had degenerated into a
 * non-empty-string test without anyone noticing, so leaving its replacement
 * unproven would repeat the mistake it was written to fix.
 *
 * Every case starts from the **real committed inputs** and mutates one thing.
 * Hand-built fixtures would drift from the files the gate actually reads, and a
 * fixture that no longer resembles the manifest cannot tell you the rule still
 * bites.
 *
 * Usage: node --test tools/integration-audit/verify-parity.test.mjs
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { violations } from "./verify-parity.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const read = (p) => JSON.parse(readFileSync(join(REPO_ROOT, p), "utf8"));

const readComponent = (relative) => {
  const path = join(REPO_ROOT, relative);
  return existsSync(path) ? readFileSync(path, "utf8") : null;
};

/** The real inputs, cloned so a test can mutate them freely. */
function inputs() {
  return {
    baseline: read("config/upstream-integration-baseline.json"),
    manifest: read("config/integration-manifest.json"),
    surfaces: read("config/ui-surfaces.json"),
    readComponent,
  };
}

const rulesOf = (found) => new Set(found.map((v) => v.rule));

/** The violations one mutation introduces, and nothing that was already there. */
function newViolations(mutate) {
  const before = rulesOf(violations(inputs()));
  const input = inputs();
  mutate(input);
  return violations(input).filter((v) => !before.has(v.rule));
}

/** An entry key that the UI rules apply to, chosen from the real manifest. */
function aUserConfigurableKey(manifest) {
  const key = Object.keys(manifest.entries).find(
    (k) => manifest.entries[k].ui && typeof manifest.entries[k].reachable === "boolean"
  );
  assert.ok(key, "the manifest should hold at least one entry with a ui and a reachable");
  return key;
}

test("the committed tree passes", () => {
  // Not the interesting assertion, but if this ever fails the others are
  // measuring the wrong thing.
  assert.deepEqual(violations(inputs()), []);
});

test("an entry naming a surface nobody declared is caught", () => {
  const found = newViolations((input) => {
    const key = aUserConfigurableKey(input.manifest);
    input.manifest.entries[key].ui = "Settings > Somewhere That Does Not Exist";
  });

  assert.deepEqual(rulesOf(found), new Set(["unknown-ui-surface"]));
});

test("an entry that does not say whether it is reachable is caught", () => {
  // Absent must not read as false. Someone adding an integration has to state
  // whether a user can get to it.
  const found = newViolations((input) => {
    const key = aUserConfigurableKey(input.manifest);
    delete input.manifest.entries[key].reachable;
  });

  assert.deepEqual(rulesOf(found), new Set(["missing-reachability"]));
});

test("claiming to be reachable through a surface that is not built is caught", () => {
  const found = newViolations((input) => {
    // Any surface that is not built yet will do, and the test finds one rather
    // than naming one. It used to name `Settings > Meetings`, and when that
    // screen was written this test started passing vacuously: setting
    // `reachable: true` on an entry behind a *built* surface is not a violation,
    // so it asserted nothing. Finding the surface -- and failing loudly if there
    // is none -- is what stops that happening again.
    const unbuilt = Object.entries(input.surfaces.surfaces)
      .filter(([, surface]) => !surface.built)
      .map(([name]) => name);

    const key = Object.keys(input.manifest.entries).find((k) =>
      unbuilt.includes(input.manifest.entries[k].ui)
    );
    assert.ok(
      key,
      `no entry names an unbuilt surface, so this test cannot check anything. ` +
        `Unbuilt: ${unbuilt.join(", ") || "none"}. Point it at a real case or delete it.`
    );
    input.manifest.entries[key].reachable = true;
  });

  assert.deepEqual(rulesOf(found), new Set(["unreachable-surface"]));
});

test("a surface built out of a component that does not exist is caught", () => {
  const found = newViolations((input) => {
    input.surfaces.surfaces["Notes > Upload"].built = true;
    // Its component path is a file nobody has written yet, which is the whole
    // point: `built: true` must not be assertable by editing one word.
  });

  assert.deepEqual(rulesOf(found), new Set(["phantom-surface"]));
});

test("a surface whose component exists but carries no marker is caught", () => {
  const found = newViolations((input) => {
    input.surfaces.surfaces["Notes > Upload"].built = true;
    // A real file, so `existsSync` is satisfied and only the marker is missing.
    input.surfaces.surfaces["Notes > Upload"].component = "apps/desktop/src/Overlay.tsx";
  });

  const rules = rulesOf(found);
  assert.deepEqual(rules, new Set(["phantom-surface"]));
  assert.match(found[0].detail, /does not render data-surface/);
});

test("removing a marker from a built surface is caught", () => {
  // The direction that matters in practice: someone moves a section between
  // files, the marker goes with it, and the manifest keeps claiming the screen.
  //
  // The file and the marker are read out of the surface registry rather than
  // written here. They were hardcoded to `App.tsx` and `notes.export` until the
  // 1037-line App.tsx was split into one component per surface, at which point
  // stripping a marker from App.tsx stopped proving anything and this test went
  // green while testing nothing -- the exact failure mode the file's own header
  // warns about.
  const surfaces = read("config/ui-surfaces.json").surfaces;
  const [, victim] = Object.entries(surfaces).find(([, s]) => s.built) ?? [];
  assert.ok(victim, "no surface is marked built, so there is nothing to strip");

  const found = newViolations((input) => {
    const stripped = readComponent(victim.component).replaceAll(
      `data-surface="${victim.surfaceId}"`,
      ""
    );
    input.readComponent = (p) =>
      p === victim.component ? stripped : readComponent(p);
  });

  assert.deepEqual(rulesOf(found), new Set(["phantom-surface"]));
});

test("a surface that does not say whether it is built is caught", () => {
  const found = newViolations((input) => {
    delete input.surfaces.surfaces["Integrations > API"].built;
  });

  const rules = rulesOf(found);
  assert.ok(rules.has("incomplete-surface"), [...rules].join(", "));
});

test("a surface missing its component or its id is caught", () => {
  for (const field of ["surfaceId", "component"]) {
    const found = newViolations((input) => {
      delete input.surfaces.surfaces["Integrations > API"][field];
    });
    const rules = rulesOf(found);
    assert.ok(rules.has("incomplete-surface"), `${field}: ${[...rules].join(", ")}`);
  }
});

test("the rules that were already there still bite", () => {
  // Rule 7 was bolted onto a working gate. These three assert the refactor did
  // not quietly drop what it was extending.
  const missingStatus = newViolations((input) => {
    delete input.manifest.entries["transcription:openai"].status;
  });
  assert.ok(rulesOf(missingStatus).has("missing-status"));

  const droppedProvider = newViolations((input) => {
    delete input.manifest.entries["transcription:openai"];
  });
  assert.ok(rulesOf(droppedProvider).has("missing-upstream-provider"));

  const drifted = newViolations((input) => {
    input.manifest.baseline.commit = "0000000000000000000000000000000000000000";
  });
  assert.ok(rulesOf(drifted).has("baseline-drift"));
});
