#!/usr/bin/env node
/**
 * Cargo feature enablement gate.
 *
 * Fails the build when a Cargo feature is declared and no CI invocation ever
 * turns it on -- because code behind such a feature is not merely untested, it
 * is never compiled, and so it can rot without anything going red.
 *
 * **Why this exists.** KNEG-147. `2a5af33` added
 * `every_import_command_enforces_the_dialog_grant_at_the_ipc_boundary`, which
 * drives real IPC requests through Tauri's mock webview and asserts all three
 * import commands refuse an ungranted path. Good coverage of a boundary that
 * had been untestable. Its body was `#[cfg(feature = "tauri-test")]`, and that
 * feature was declared in exactly one line of one Cargo.toml and named by
 * nothing else in the repository -- no workflow, no script, no document. So the
 * test never ran, and the closing note on the issue it was written for said the
 * gap was "permanently covered".
 *
 * Two consequences, and the second is the worse one. The assertions did not
 * run. And because the module was not compiled at all, `cargo clippy
 * --all-targets` did not see it either, so the test could have stopped
 * compiling without anything noticing. That is how feature-gated coverage
 * usually dies: not with a failure, but by being quietly dropped from the build
 * graph.
 *
 * This is the same shape as `tools/settings-audit/scan.mjs`, one layer down.
 * There, a setting the backend honoured that no screen could change; here, a
 * test the repository declared that no invocation could run. In both cases the
 * artefact existed, had a passing suite around it, and was unreachable -- and
 * in both cases the suite was no help, because a suite reports on what it ran.
 *
 * **What it checks:**
 *
 *   A. Every feature declared in a `[features]` table in the workspace is
 *      named by at least one CI invocation -- `--features`, `-F`, or
 *      `--all-features` -- or is declared exempt in
 *      `config/cargo-features.json` with a reason. A feature named only by a
 *      commented-out line does not count; see [`runLines`] for why that
 *      distinction is load-bearing in this repository.
 *   B. Every exemption names a feature that still exists. The manifest does not
 *      get to accumulate fiction either.
 *   C. Every feature a CI invocation names is really declared by the package
 *      that invocation selects. Cargo already refuses an unknown feature for a
 *      selected package, so this catches the case Cargo does not: a `-p` and a
 *      `--features` that have drifted apart.
 *
 * **What it does not check.** That the feature's code is *correct*, or that the
 * invocation enabling it asserts anything -- a step that compiles a test and
 * then filters it all out would satisfy rule A. It does not see implicit
 * features from optional dependencies, which have no `[features]` entry; those
 * are enabled by depending on them, not by a flag, so they cannot go dark the
 * way an explicit feature can. And it says nothing about whether CI runs at
 * all: `.github/workflows/ci.yml` has had its `push` and `pull_request`
 * triggers commented out by Actions budget since before this gate existed, so
 * this checks the declared gate list, not the last run.
 *
 * Usage:
 *   node tools/feature-audit/scan.mjs [--json]
 */

import { readFileSync, existsSync, readdirSync, statSync } from "node:fs";
import { join, dirname, resolve, relative } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");

export const MANIFEST = "config/cargo-features.json";
export const WORKFLOW_DIR = ".github/workflows";

/** Directories that never hold a Cargo.toml worth reading. */
const SKIP = new Set(["target", "node_modules", ".git", "dist", "coverage"]);

/**
 * Every Cargo.toml in the tree, not only the workspace members. A feature on a
 * non-member package is still a feature that can go dark, and reading the
 * member list would mean re-implementing glob resolution to find out less.
 */
export function cargoManifests(root = REPO_ROOT) {
  const found = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir)) {
      if (SKIP.has(entry)) continue;
      const path = join(dir, entry);
      let stats;
      try {
        stats = statSync(path);
      } catch {
        continue; // a broken symlink is not a manifest
      }
      if (stats.isDirectory()) walk(path);
      else if (entry === "Cargo.toml") found.push(relative(root, path).split("\\").join("/"));
    }
  };
  walk(root);
  return found.sort();
}

/**
 * The package name and declared feature names from one Cargo.toml.
 *
 * Deliberately a small hand-rolled reader rather than a TOML dependency: the
 * two things wanted are `name` under `[package]` and the keys of `[features]`,
 * both of which are one `key = value` per line in every manifest here.
 * `default` is skipped -- it is always enabled, so it can never be the thing
 * this gate is looking for.
 */
export function declaredFeatures(source) {
  let table = null;
  let name = null;
  const features = [];
  for (const raw of source.split(/\r?\n/)) {
    const line = raw.trim();
    if (line.startsWith("#") || line === "") continue;
    if (line.startsWith("[")) {
      table = line.replace(/^\[+|\]+.*$/g, "");
      continue;
    }
    const eq = line.indexOf("=");
    if (eq < 0) continue;
    const key = line.slice(0, eq).trim().replace(/^"|"$/g, "");
    if (table === "package" && key === "name") {
      name = line
        .slice(eq + 1)
        .trim()
        .replace(/^"|"$/g, "");
    } else if (table === "features" && key !== "default") {
      features.push(key);
    }
  }
  return { name, features };
}

/**
 * The lines of a workflow that could actually run a command.
 *
 * A full-line comment is dropped, and that is the rule this gate turns on. The
 * failure being prevented is a feature nothing enables; a commented-out step
 * enables nothing, so counting it would make the gate agree that a disabled
 * check is a check. This repository has the live example: `ci.yml`'s own `push`
 * and `pull_request` triggers are commented out, and the engineering log is
 * explicit that nothing is being checked while they are. A gate that read
 * comments would call the defect clean the moment someone wrote the fix into a
 * comment.
 *
 * A trailing comment is cut too, by [`stripComment`]. That case looks contrived
 * -- `run: # cargo test ...` -- but the first draft of this gate left it in and
 * its own self-test walked straight into it: commenting out the command while
 * leaving `run:` at the start of the line kept the flag visible, so the gate
 * called a disabled step an enabled one.
 */
export function runLines(source) {
  return source
    .split(/\r?\n/)
    .filter((line) => !line.trim().startsWith("#"))
    .map(stripComment);
}

/**
 * `line` up to its first `#` that is not inside quotes.
 *
 * The quote tracking is what keeps this from cutting a `#` inside a shell
 * string. It is approximate -- an unbalanced quote on one line swallows the
 * rest of it -- and the direction of that error is the reason it is acceptable:
 * over-stripping loses an enablement, which fails rule A loudly. Under-
 * stripping would count a comment as a live invocation, which is the silent
 * pass this whole gate exists to prevent.
 */
export function stripComment(line) {
  let quote = null;
  for (let i = 0; i < line.length; i += 1) {
    const c = line[i];
    if (quote !== null) {
      if (c === quote) quote = null;
    } else if (c === '"' || c === "'") {
      quote = c;
    } else if (c === "#") {
      return line.slice(0, i);
    }
  }
  return line;
}

/**
 * Feature enablements found in workflow text, as `{ features, pkg, all, line }`.
 *
 * `--features a,b`, `--features=a,b`, `-F a` and `--all-features` are all
 * recognised, as is the `package/feature` spelling, which is the only one that
 * works for a workspace-wide invocation.
 */
export function enablements(lines) {
  const found = [];
  for (const line of lines) {
    const pkg = /(?:^|\s)(?:-p|--package)[= ]([A-Za-z0-9_-]+)/.exec(line)?.[1] ?? null;
    if (/(?:^|\s)--all-features(?:\s|$)/.test(line)) {
      found.push({ features: [], pkg, all: true, line: line.trim() });
      continue;
    }
    const flags = [...line.matchAll(/(?:^|\s)(?:--features|-F)[= ]("[^"]+"|'[^']+'|[^\s]+)/g)];
    if (flags.length === 0) continue;
    const features = [];
    for (const flag of flags) {
      for (const spec of flag[1].replace(/^["']|["']$/g, "").split(/[,\s]+/)) {
        if (spec !== "") features.push(spec);
      }
    }
    found.push({ features, pkg, all: false, line: line.trim() });
  }
  return found;
}

/** The bare feature name from a `package/feature` or plain spec. */
export function featureName(spec) {
  const slash = spec.indexOf("/");
  return slash < 0 ? spec : spec.slice(slash + 1);
}

/** The package named by a `package/feature` spec, or null for a plain one. */
export function featurePackage(spec) {
  const slash = spec.indexOf("/");
  return slash < 0 ? null : spec.slice(0, slash);
}

/** Whether one enablement turns on `feature` for `pkgName`. */
export function enables(e, feature, pkgName) {
  if (e.all) return e.pkg === null || e.pkg === pkgName;
  return e.features.some(
    (spec) =>
      featureName(spec) === feature &&
      (featurePackage(spec) === null || featurePackage(spec) === pkgName) &&
      (e.pkg === null || e.pkg === pkgName)
  );
}

export function violations({ packages, enabled, exempt }) {
  const found = [];

  // Rule A. The whole point: a declared feature no invocation names.
  for (const pkg of packages) {
    for (const feature of pkg.features) {
      const reason = exempt[feature] ?? exempt[`${pkg.name}/${feature}`];
      if (reason !== undefined) continue;
      if (enabled.some((e) => enables(e, feature, pkg.name))) continue;
      found.push({
        rule: "A. declared and never enabled",
        subject: `${pkg.name}/${feature}`,
        detail:
          `declared in ${pkg.manifest} and named by no CI invocation. Code behind it ` +
          `is never compiled, so it can stop building without anything failing. Add a ` +
          `step passing --features ${feature}, or declare it exempt in ${MANIFEST} ` +
          `with the reason.`,
      });
    }
  }

  const declared = new Set();
  for (const pkg of packages) {
    for (const feature of pkg.features) {
      declared.add(feature);
      declared.add(`${pkg.name}/${feature}`);
    }
  }

  // Rule B. An exemption for a feature since deleted or renamed reads as a live
  // decision and is not one.
  for (const [key, reason] of Object.entries(exempt)) {
    if (declared.has(key)) continue;
    found.push({
      rule: "B. exemption names no feature",
      subject: key,
      detail: `${MANIFEST} exempts it (${JSON.stringify(
        reason
      )}) and no [features] table declares it.`,
    });
  }

  // Rule C. Cargo refuses an unknown feature for a selected package; it does
  // not notice a -p and a --features that have drifted apart.
  for (const e of enabled) {
    if (e.all) continue;
    for (const spec of e.features) {
      const name = featureName(spec);
      const owner = featurePackage(spec) ?? e.pkg;
      if (owner === null) {
        if (declared.has(name)) continue;
      } else {
        const pkg = packages.find((p) => p.name === owner);
        if (pkg === undefined) continue; // not a workspace package; not ours to judge
        if (pkg.features.includes(name)) continue;
      }
      found.push({
        rule: "C. enabled feature is not declared",
        subject: owner === null ? name : `${owner}/${name}`,
        detail: `enabled by \`${e.line}\` and declared by no package in the workspace.`,
      });
    }
  }

  return found;
}

export function workflows(root = REPO_ROOT) {
  const dir = join(root, WORKFLOW_DIR);
  if (!existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((f) => f.endsWith(".yml") || f.endsWith(".yaml"))
    .sort()
    .map((f) => ({ file: `${WORKFLOW_DIR}/${f}`, source: readFileSync(join(dir, f), "utf8") }));
}

export function collect(root = REPO_ROOT) {
  const packages = [];
  for (const manifest of cargoManifests(root)) {
    const { name, features } = declaredFeatures(readFileSync(join(root, manifest), "utf8"));
    if (name === null || features.length === 0) continue;
    packages.push({ name, features, manifest });
  }
  const enabled = [];
  for (const { file, source } of workflows(root)) {
    for (const e of enablements(runLines(source))) enabled.push({ ...e, file });
  }
  const path = join(root, MANIFEST);
  const exempt = existsSync(path) ? (JSON.parse(readFileSync(path, "utf8")).exempt ?? {}) : {};
  return { packages, enabled, exempt };
}

function main() {
  const { packages, enabled, exempt } = collect();
  const found = violations({ packages, enabled, exempt });
  const total = packages.reduce((n, p) => n + p.features.length, 0);

  if (process.argv.includes("--json")) {
    console.log(JSON.stringify({ packages, enabled, exempt, violations: found }, null, 2));
    process.exit(found.length === 0 ? 0 : 1);
  }

  if (found.length > 0) {
    console.error(
      `cargo features FAILED — ${found.length} violation(s) across ${total} feature(s)\n`
    );
    const grouped = {};
    for (const v of found) (grouped[v.rule] ??= []).push(v);
    for (const [rule, vs] of Object.entries(grouped)) {
      console.error(`  ${rule} (${vs.length})`);
      for (const v of vs) console.error(`    ${v.subject}: ${v.detail}`);
      console.error("");
    }
    process.exit(1);
  }

  // The counts are the point, not decoration. "No violations" over zero
  // features parsed would be this gate committing the very failure it exists to
  // catch, so the numbers that would make it vacuous are printed every run.
  console.log(
    `cargo features OK — ${total} declared across ${packages.length} package(s), ` +
      `${enabled.length} enabling invocation(s), ${Object.keys(exempt).length} exempt`
  );
  for (const pkg of packages) {
    for (const feature of pkg.features) {
      const label = `${pkg.name}/${feature}`.padEnd(34);
      const reason = exempt[feature] ?? exempt[`${pkg.name}/${feature}`];
      if (reason !== undefined) {
        console.log(`  ${label} exempt — ${reason}`);
        continue;
      }
      const by = enabled.find((e) => enables(e, feature, pkg.name));
      console.log(`  ${label} ${by.file}`);
    }
  }
  process.exit(0);
}

if (resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  main();
}
