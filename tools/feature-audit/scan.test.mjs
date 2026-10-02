#!/usr/bin/env node
/**
 * Self-tests for the Cargo feature enablement gate.
 *
 * These exist for the reason the other gates' self-tests exist, recorded in
 * docs/ENGINEERING_LOG.md: **a check that matches nothing passes trivially.**
 * "The committed tree is clean" and "the rules work" are different claims, and
 * a green gate only ever demonstrates the first.
 *
 * The warning is sharper here than anywhere. This gate exists because a test
 * was compiled out of every build and nothing objected -- green because nothing
 * ran. A gate against *that* which was itself only ever seen green would be the
 * same mistake one level up, and the mistake would be invisible in exactly the
 * same way.
 *
 * So every case starts from the **real committed inputs** and mutates one
 * thing. The first two reconstruct KNEG-147 as it actually stood: the feature
 * declared in `apps/desktop/src-tauri/Cargo.toml` and named by no invocation
 * anywhere. `the_real_tree_is_clean_and_not_vacuously_so` is the control -- it
 * asserts the parsers found something before it accepts that they found no
 * violations.
 *
 * Usage: node --test tools/feature-audit/scan.test.mjs
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import {
  collect,
  cargoManifests,
  violations,
  declaredFeatures,
  enablements,
  runLines,
  stripComment,
  enables,
  featureName,
  featurePackage,
  workflows,
  MANIFEST,
} from "./scan.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const CI = ".github/workflows/ci.yml";
const THE_STEP = "cargo test --profile ci -p knegiswhisper --lib --features tauri-test";

const ciSource = () => readFileSync(join(REPO_ROOT, CI), "utf8");

/** The real inputs, with the workflow text swapped for a mutated copy. */
function withCi(source) {
  const real = collect(REPO_ROOT);
  const enabled = enablements(runLines(source)).map((e) => ({ ...e, file: CI }));
  return { ...real, enabled };
}

const rules = (found) => found.map((v) => v.rule).sort();

test("KNEG-226: worktree copies are excluded while ordinary manifests remain audited", (t) => {
  const root = mkdtempSync(join(tmpdir(), "kw-feature-audit-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const manifests = [
    "Cargo.toml",
    "apps/desktop/Cargo.toml",
    "crates/worktrees/Cargo.toml",
    "vendor/example/Cargo.toml",
    ".claude/worktrees/chip-menus/Cargo.toml",
    ".claude/worktrees/chip-menus/apps/desktop/Cargo.toml",
    ".worktrees/stale/apps/desktop/Cargo.toml",
    ".worktrees/stale/crates/obsolete/Cargo.toml",
  ];
  for (const manifest of manifests) {
    mkdirSync(dirname(join(root, manifest)), { recursive: true });
    writeFileSync(join(root, manifest), '[package]\nname = "example"\n');
  }
  // The selected root may itself be a linked worktree.
  writeFileSync(join(root, ".git"), "gitdir: /unused/worktree/metadata\n");
  // KNEG-379: Claude Code's `.claude/worktrees` is skipped by path, so a
  // `worktrees` directory elsewhere is still read.
  assert.deepEqual(cargoManifests(root), manifests.slice(0, 4));
});

test("the real tree is clean, and not vacuously so", () => {
  const real = collect(REPO_ROOT);

  // The control. Every assertion below about "no violations" is worth nothing
  // if the parsers returned empty, which is the failure this whole gate is
  // about. Assert the subject was reached before accepting the verdict.
  const total = real.packages.reduce((n, p) => n + p.features.length, 0);
  assert.ok(total > 0, "no [features] table was parsed at all; the walk found nothing");
  assert.ok(real.enabled.length > 0, "no enabling invocation was parsed; the workflow scan is dead");
  assert.ok(
    real.packages.some((p) => p.name === "knegiswhisper" && p.features.includes("tauri-test")),
    "the known feature was not found, so a clean result proves nothing"
  );

  assert.deepEqual(violations(real), []);
});

test("KNEG-147 reconstructed: the feature declared and named by no invocation", () => {
  const source = ciSource();
  assert.ok(source.includes(THE_STEP), "the step this test removes is not in ci.yml any more");

  const found = violations(withCi(source.replace(THE_STEP, "cargo test --workspace")));

  assert.deepEqual(rules(found), ["A. declared and never enabled"]);
  assert.equal(found[0].subject, "knegiswhisper/tauri-test");
  assert.match(found[0].detail, /never compiled/);
});

test("a commented-out step enables nothing, in either spelling", () => {
  // The load-bearing rule. This repository's own CI triggers are commented out,
  // and the log is explicit that nothing is being checked while they are. A gate
  // that counted comments would call the defect fixed the moment somebody wrote
  // the fix into one.
  //
  // Both spellings are checked because the first draft only handled the first,
  // and this test found it: `run: # cargo ...` left the flag on a line whose
  // leading character is `r`, so it still counted.
  const wholeLine = ciSource().replace(`        run: ${THE_STEP}`, `        # run: ${THE_STEP}`);
  assert.ok(wholeLine.includes(`# run: ${THE_STEP}`), "the whole-line mutation did not apply");
  const first = violations(withCi(wholeLine));
  assert.deepEqual(rules(first), ["A. declared and never enabled"]);
  assert.equal(first[0].subject, "knegiswhisper/tauri-test");

  const trailing = ciSource().replace(THE_STEP, `# ${THE_STEP}`);
  assert.ok(trailing.includes(`run: # ${THE_STEP}`), "the trailing mutation did not apply");
  const second = violations(withCi(trailing));
  assert.deepEqual(rules(second), ["A. declared and never enabled"]);
});

test("stripComment leaves a `#` that is inside quotes alone", () => {
  // The direction of this error matters: cutting a quoted `#` would lose a real
  // enablement and fail rule A loudly, which is survivable. Keeping an unquoted
  // one would count a comment as a live step, which is not.
  assert.equal(stripComment("run: cargo test # disabled"), "run: cargo test ");
  assert.equal(stripComment('run: sed "s/#//" f --features a'), 'run: sed "s/#//" f --features a');
  assert.equal(stripComment("        # run: cargo test"), "        ");
  assert.equal(stripComment("run: cargo test --features a"), "run: cargo test --features a");

  // And the quoted case survives end to end, not just in the helper.
  const parsed = enablements(runLines('        run: sed "s/#//" x && cargo test -F a'));
  assert.deepEqual(
    parsed.map((e) => e.features),
    [["a"]]
  );
});

test("an exemption with a reason satisfies rule A, and is reported rather than hidden", () => {
  const base = withCi(ciSource().replace(THE_STEP, "cargo test --workspace"));
  assert.equal(violations(base).length, 1, "the premise: unexempt, this is a violation");

  const exempt = { ...base.exempt, "tauri-test": "the mock runtime needs a Windows manifest CI cannot embed" };
  assert.deepEqual(violations({ ...base, exempt }), []);

  // And the bare name and the qualified name are both accepted spellings.
  const qualified = { ...base.exempt, "knegiswhisper/tauri-test": "same decision, written the other way" };
  assert.deepEqual(violations({ ...base, exempt: qualified }), []);
});

test("rule B: an exemption naming no feature is fiction", () => {
  const real = collect(REPO_ROOT);
  const found = violations({ ...real, exempt: { ...real.exempt, "long-gone": "renamed three waves ago" } });

  assert.deepEqual(rules(found), ["B. exemption names no feature"]);
  assert.equal(found[0].subject, "long-gone");
  assert.match(found[0].detail, new RegExp(MANIFEST.replace(/[/.]/g, "\\$&")));
});

test("rule C: a --features nobody declares", () => {
  const found = violations({
    packages: [{ name: "knegiswhisper", features: ["tauri-test"], manifest: "a/Cargo.toml" }],
    enabled: [
      { features: ["tauri-test"], pkg: "knegiswhisper", all: false, line: "real", file: CI },
      { features: ["typo-test"], pkg: "knegiswhisper", all: false, line: "typo", file: CI },
    ],
    exempt: {},
  });

  assert.deepEqual(rules(found), ["C. enabled feature is not declared"]);
  assert.equal(found[0].subject, "knegiswhisper/typo-test");
});

test("rule C: a -p and a --features that have drifted apart", () => {
  // Cargo catches an unknown feature for the selected package. It does not
  // catch a flag that still names a real feature -- of some other package.
  const packages = [
    { name: "knegiswhisper", features: ["tauri-test"], manifest: "a/Cargo.toml" },
    { name: "kw-storage", features: [], manifest: "b/Cargo.toml" },
  ];
  const drifted = {
    features: ["tauri-test"],
    pkg: "kw-storage",
    all: false,
    line: "cargo test -p kw-storage --features tauri-test",
    file: CI,
  };

  const found = violations({ packages, enabled: [drifted], exempt: {} });

  // Both halves fire, and both are true: kw-storage does not declare it, and
  // nothing now enables it for the package that does.
  assert.deepEqual(rules(found), [
    "A. declared and never enabled",
    "C. enabled feature is not declared",
  ]);
});

test("--all-features satisfies rule A, but only for the package it selects", () => {
  const packages = [
    { name: "knegiswhisper", features: ["tauri-test"], manifest: "a/Cargo.toml" },
  ];
  const workspaceWide = { features: [], pkg: null, all: true, line: "--all-features", file: CI };
  const elsewhere = {
    features: [],
    pkg: "kw-storage",
    all: true,
    line: "-p kw-storage --all-features",
    file: CI,
  };

  assert.deepEqual(violations({ packages, enabled: [workspaceWide], exempt: {} }), []);
  assert.deepEqual(rules(violations({ packages, enabled: [elsewhere], exempt: {} })), [
    "A. declared and never enabled",
  ]);
});

test("declaredFeatures reads the package name and skips `default`", () => {
  const source = readFileSync(join(REPO_ROOT, "apps/desktop/src-tauri/Cargo.toml"), "utf8");
  const { name, features } = declaredFeatures(source);

  assert.equal(name, "knegiswhisper");
  assert.deepEqual(features, ["tauri-test"]);

  const synthetic = declaredFeatures(
    ['[package]', 'name = "demo"', "", "[features]", 'default = ["a"]', 'a = []', "", "[lib]"].join(
      "\n"
    )
  );
  assert.deepEqual(synthetic, { name: "demo", features: ["a"] });

  // A `[features]` line inside a comment is not a declaration.
  const commented = declaredFeatures(
    ['[package]', 'name = "demo"', "[features]", "# ghost = []", 'real = []'].join("\n")
  );
  assert.deepEqual(commented.features, ["real"]);
});

test("enablements parses every spelling the flag has", () => {
  const parsed = enablements([
    "run: cargo test -p one --features a",
    "run: cargo test --features=b,c",
    "run: cargo test -F d",
    'run: cargo test --features "e f"',
    "run: cargo test --package two --all-features",
    "run: cargo test --workspace",
  ]);

  assert.deepEqual(
    parsed.map((e) => [e.pkg, e.all, e.features]),
    [
      ["one", false, ["a"]],
      [null, false, ["b", "c"]],
      [null, false, ["d"]],
      [null, false, ["e", "f"]],
      ["two", true, []],
    ]
  );
});

test("a package/feature spec is split, and matched against the owning package", () => {
  assert.equal(featureName("tauri/test"), "test");
  assert.equal(featurePackage("tauri/test"), "tauri");
  assert.equal(featureName("plain"), "plain");
  assert.equal(featurePackage("plain"), null);

  const spec = { features: ["knegiswhisper/tauri-test"], pkg: null, all: false, line: "x" };
  assert.equal(enables(spec, "tauri-test", "knegiswhisper"), true);
  assert.equal(enables(spec, "tauri-test", "kw-storage"), false);
});

test("every workflow in the tree is read, not only ci.yml", () => {
  // release.yml exists and is workflow_dispatch-only. If a feature were enabled
  // only there this gate would still find it, which is deliberate: this checks
  // the declared gate list, and says nothing about which triggers are live.
  const files = workflows(REPO_ROOT).map((w) => w.file);
  assert.ok(files.includes(CI), "ci.yml was not read");
  assert.ok(files.length > 0);
});
