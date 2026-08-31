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
import { readFileSync, existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { violations, hasTargetOnDisk } from "./verify-parity.mjs";

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
    // The same predicate the CLI passes. Without it every test below would run
    // with the `() => false` default, and the documentation rule would be
    // measured against something the gate never does.
    hasTarget: (target) => hasTargetOnDisk(target),
  };
}

/** The manifest's one `kind: "documentation"` entry. */
function aDocumentationKey(manifest) {
  const key = Object.keys(manifest.entries).find(
    (k) => manifest.entries[k].kind === "documentation"
  );
  assert.ok(key, "the manifest should hold at least one documentation entry");
  return key;
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
    // It used to name a real surface directly (`Settings > Meetings`, later
    // `Notes > Upload`), and each time that screen got built the test started
    // passing vacuously: setting `reachable: true` on an entry behind a
    // *built* surface is not a violation, so it asserted nothing. Finding an
    // unbuilt surface fixed it once, but Task 12 built the last one -- there
    // is now no real surface left unbuilt to find. A synthetic surface this
    // rule has never seen closes that gap for good: it does not matter which
    // real surfaces exist or how many of them are built.
    const key = aUserConfigurableKey(input.manifest);
    input.surfaces.surfaces["Test > Not Built"] = {
      surfaceId: "test.not-built",
      component: "apps/desktop/src/views/NoSuchComponentEverWritten.tsx",
      built: false,
    };
    input.manifest.entries[key].ui = "Test > Not Built";
    input.manifest.entries[key].reachable = true;
  });

  assert.deepEqual(rulesOf(found), new Set(["unreachable-surface"]));
});

test("a surface built out of a component that does not exist is caught", () => {
  const found = newViolations((input) => {
    // A synthetic surface rather than a real one named by hand: `Notes >
    // Upload` used to be the one surface still unbuilt, so mutating its
    // `built` flag alone was enough to make its already-real, marker-less
    // component path phantom. Task 12 built it, so every real surface's
    // component now exists and carries its marker -- this rule still needs
    // exercising, but only a surface invented for the purpose, pointed at a
    // file nobody has written, still proves it.
    input.surfaces.surfaces["Test > Phantom"] = {
      surfaceId: "test.phantom",
      component: "apps/desktop/src/views/NoSuchComponentEverWritten.tsx",
      built: true,
    };
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

/* ------------------------------------------------- documentation targets */

test("a documentation entry claiming ported docs that are not there is caught", () => {
  // The hole KNEG-86 was opened for. `developer:agent-skills` carried
  // `status: "ported"` against `docs/agent-skills/` for a twenty-task run, and
  // that directory has never existed. Naming a target was the whole check.
  const found = newViolations((input) => {
    const key = aDocumentationKey(input.manifest);
    input.manifest.entries[key].status = "ported";
    input.manifest.entries[key].target = "docs/a-directory-nobody-wrote/";
  });

  assert.deepEqual(rulesOf(found), new Set(["documentation-target-missing"]));
});

test("a documentation entry whose docs really are there passes", () => {
  // The other half: the rule must not fire on a claim that is true, or it is a
  // rule people route around. `docs/` exists and is not empty.
  const found = newViolations((input) => {
    const key = aDocumentationKey(input.manifest);
    input.manifest.entries[key].status = "ported";
    input.manifest.entries[key].target = "docs/";
  });

  assert.deepEqual(found, []);
});

test("a documentation entry that does not claim the docs exist is left alone", () => {
  // `temporarily-deferred` says the work has not been done, which is an honest
  // thing to say and not a violation. This is the state the entry was in
  // between 2026-08-27 and 2026-08-31, and the reason the committed tree passed
  // throughout: the rule fires on the *claim*, not on the absence.
  //
  // The deferral block goes in with it. Without one a different rule fires --
  // `incomplete-deferral` -- and this test would pass or fail for the wrong
  // reason.
  const found = newViolations((input) => {
    const key = aDocumentationKey(input.manifest);
    const entry = input.manifest.entries[key];
    entry.status = "temporarily-deferred";
    entry.target = "docs/a-directory-nobody-wrote/";
    entry.deferral = {
      why: "nobody has written it",
      dependencies: ["somebody to write it"],
      interface: "none -- documentation only",
      expectedPhase: 7,
    };
  });

  assert.deepEqual(found, []);
});

test("a documentation entry still has to name a target at all", () => {
  // The rule that was already there, kept: an entry with no target names
  // nowhere for the docs to be, which no status check can rescue.
  const found = newViolations((input) => {
    const key = aDocumentationKey(input.manifest);
    delete input.manifest.entries[key].target;
  });

  assert.deepEqual(rulesOf(found), new Set(["incomplete-documentation"]));
});

test("hasTargetOnDisk refuses a missing path and an empty directory", () => {
  // An empty `docs/agent-skills/` satisfies "the rewritten documentation lives
  // here" no better than a missing one, and is the same hole one commit later.
  const root = mkdtempSync(join(tmpdir(), "parity-target-"));
  try {
    mkdirSync(join(root, "empty"));
    mkdirSync(join(root, "full"));
    writeFileSync(join(root, "full", "SKILL.md"), "# something\n");
    writeFileSync(join(root, "a-file.md"), "# something\n");

    assert.equal(hasTargetOnDisk("missing/", root), false, "a path that is not there");
    assert.equal(hasTargetOnDisk("empty/", root), false, "a directory with nothing in it");
    assert.equal(hasTargetOnDisk("full/", root), true, "a directory with a file in it");
    assert.equal(hasTargetOnDisk("a-file.md", root), true, "a file that exists");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("violations refuses every documentation target when nobody injects a reader", () => {
  // The default fails closed. A caller that forgets `hasTarget` gets a loud
  // failure rather than a silent pass, which is the direction that matters for
  // a check whose whole job is catching a claim about something absent.
  const input = inputs();
  const key = aDocumentationKey(input.manifest);
  input.manifest.entries[key].status = "ported";
  input.manifest.entries[key].target = "docs/";
  delete input.hasTarget;

  const found = violations(input).filter((v) => v.rule === "documentation-target-missing");
  assert.equal(found.length, 1, "the default must not wave a target through");
});
