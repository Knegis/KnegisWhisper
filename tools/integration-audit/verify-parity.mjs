#!/usr/bin/env node
/**
 * Integration parity gate.
 *
 * Diffs config/integration-manifest.json against
 * config/upstream-integration-baseline.json and fails the build on any of the
 * six conditions the brief requires:
 *
 *   1. An upstream provider ID is missing from the manifest.
 *   2. An upstream integration disappears without an explicit status.
 *   3. A model-provider registry entry is dropped accidentally.
 *   4. A frontend integration exists without a backend implementation.
 *   5. A backend integration exists but cannot be configured in the UI.
 *   6. A manifest entry claims a status outside the declared vocabulary.
 *
 * ...plus a seventh, which is rule 5 applied to itself:
 *
 *   7. A UI surface is claimed but does not exist.
 *
 * Rule 5 only ever checked that the `ui` field was a non-empty string, so an
 * entry could name `Settings > Meetings` and pass while no such screen had been
 * written. Sixty-four entries did exactly that. A gate that cannot tell a screen
 * from a string is the same defect it exists to catch, one level up.
 *
 * Rule 7 resolves each `ui` value through config/ui-surfaces.json, which names
 * the component that renders it, and requires a `built` surface to exist and to
 * carry a `data-surface="<surfaceId>"` marker. The marker is what makes the
 * claim checkable from source with no toolchain, so this stays in the cheap
 * contracts job that gates the rest of CI.
 *
 * `built` and `reachable` are separate on purpose. `built` is a fact about a
 * component; `reachable` is the per-entry claim that a user can configure *that*
 * integration through it. Settings > Transcription is the case that forced the
 * split: the section exists and lists every provider, but nothing anywhere can
 * store an API key, so no provider is configurable from it.
 *
 * (Forbidden vendor endpoints are the separate concern of the domain scanner.)
 *
 * Reads only committed JSON and source files — never the upstream checkout — so
 * this runs in CI on any platform with no external dependencies.
 *
 * Usage: node tools/integration-audit/verify-parity.mjs
 * Exit:  0 = parity holds, 1 = one or more violations
 */

import { readFileSync, existsSync, readdirSync, statSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const read = (p) => JSON.parse(readFileSync(join(REPO_ROOT, p), "utf8"));

/** Statuses that assert a working implementation exists in this codebase. */
export const ACTIVE = new Set([
  "ported",
  "ported-with-new-rust-implementation",
  "rewired-to-direct-byok",
  "replaced-with-local-equivalent",
  "platform-specific",
]);

/**
 * Key prefixes a user must be able to configure. Platform capabilities and
 * automatic detectors are deliberately excluded: they are not settings, and
 * demanding a UI surface for them would be noise rather than a real check.
 */
export const USER_CONFIGURABLE = new Set([
  "transcription",
  "reasoning",
  "enterprise",
  "local-llm",
  "local-asr",
  // Added 2026-09-01 with KNEG-73a. A model somebody has to choose to download
  // is as much a setting as an ASR model is, and it is neither of the two
  // things this set excludes -- not a platform capability, not an automatic
  // detector. While it was absent, the two diarization entries could move from
  // deferred to built with no screen anywhere and no gate would have asked.
  "diarization-model",
  "meeting-stream",
  "credential",
  "calendar",
  "developer",
  "media",
]);

/**
 * Every parity violation in one set of inputs.
 *
 * Pure and injectable so `verify-parity.test.mjs` can exercise the real rules
 * against constructed inputs. A gate whose rules are only ever run against a
 * passing tree proves nothing: "this tree is clean" and "the rules work" are
 * different claims, and only the second one keeps working after a refactor.
 *
 * @param {object} input
 * @param {object} input.baseline   config/upstream-integration-baseline.json
 * @param {object} input.manifest   config/integration-manifest.json
 * @param {object} input.surfaces   config/ui-surfaces.json
 * @param {(componentPath: string) => string | null} input.readComponent
 *   Source of a component, or null when there is no such file.
 * @param {(targetPath: string) => boolean} [input.hasTarget]
 *   Whether a documentation target exists and holds something. Injected for
 *   the same reason `readComponent` is: a rule that can only be run against
 *   the committed tree is a rule nobody can prove bites. Defaults to refusing
 *   every target, so a caller that forgets it gets a loud failure rather than
 *   a silent pass -- the direction that matters for a check whose whole job is
 *   catching a claim about something that is not there.
 * @returns {{rule: string, key: string, detail: string}[]}
 */
export function violations({
  baseline,
  manifest,
  surfaces,
  readComponent,
  hasTarget = () => false,
}) {
  const found = [];
  const fail = (rule, key, detail) => found.push({ rule, key, detail });

  const VOCAB = new Set(manifest.statusVocabulary);
  const entries = manifest.entries;
  const declaredSurfaces = surfaces.surfaces ?? {};

  // --- 1 & 3: every baseline provider and model-family ID must be represented ---
  const expectProvider = (manifestKey, label) => {
    if (!entries[manifestKey]) {
      fail(
        "missing-upstream-provider",
        manifestKey,
        `${label} is in the baseline but has no manifest entry`
      );
      return null;
    }
    return entries[manifestKey];
  };

  const checkModels = (manifestKey, upstreamModels, label) => {
    const e = entries[manifestKey];
    if (!e) return;
    const declared = new Set(e.models ?? []);
    for (const m of upstreamModels) {
      if (!declared.has(m)) {
        fail(
          "dropped-registry-model",
          manifestKey,
          `${label} model "${m}" is in the baseline but not in the manifest entry`
        );
      }
    }
  };

  const R = baseline.registry;
  const providerGroups = [
    ["transcriptionProviders", "transcription", "transcription provider"],
    ["cloudProviders", "reasoning", "cloud reasoning provider"],
    ["enterpriseProviders", "enterprise", "enterprise provider"],
    ["localProviders", "local-llm", "local LLM provider"],
  ];
  for (const [regKey, prefix, label] of providerGroups) {
    for (const p of R[regKey]) {
      const key = `${prefix}:${p.id}`;
      if (expectProvider(key, `${label} "${p.id}"`)) {
        checkModels(
          key,
          p.models.map((m) => m.id),
          label
        );
      }
    }
  }

  for (const mid of Object.keys(R.whisperModels)) {
    expectProvider(`local-asr:whisper-${mid}`, `whisper model "${mid}"`);
  }
  for (const mid of Object.keys(R.parakeetModels)) {
    expectProvider(`local-asr:${mid}`, `parakeet/nemotron model "${mid}"`);
  }
  for (const mid of Object.keys(R.diarizationModels)) {
    expectProvider(`diarization-model:${mid}`, `diarization model "${mid}"`);
  }
  for (const pid of baseline.meetingStreamingProviders) {
    expectProvider(`meeting-stream:${pid}`, `meeting streaming provider "${pid}"`);
  }
  for (const k of baseline.secretKeys) {
    expectProvider(`credential:${k.base}`, `secret key slot "${k.base}"`);
  }

  // The vendor-only inference providers must be explicitly accounted for, not merely absent.
  const VENDOR_INFERENCE = { openwhispr: "vendor:openwhispr-cloud-reasoning" };
  for (const p of baseline.inferenceProviderRegistry) {
    const alias = VENDOR_INFERENCE[p.id];
    if (alias) {
      const e = entries[alias];
      if (!e) {
        fail(
          "missing-upstream-provider",
          alias,
          `vendor inference provider "${p.id}" has no manifest entry`
        );
      } else if (e.status !== "removed-vendor-service") {
        fail(
          "undeclared-removal",
          alias,
          `vendor inference provider "${p.id}" must be removed-vendor-service, found "${e.status}"`
        );
      }
    }
  }

  // --- 7a: the surface registry itself must describe real components ---------
  for (const [name, surface] of Object.entries(declaredSurfaces)) {
    for (const field of ["surfaceId", "component"]) {
      if (!surface[field]) {
        fail("incomplete-surface", name, `surface is missing "${field}"`);
      }
    }
    if (typeof surface.built !== "boolean") {
      fail(
        "incomplete-surface",
        name,
        'surface must say whether it is "built"; absent is not the same as false'
      );
      continue;
    }
    if (!surface.built || !surface.component || !surface.surfaceId) continue;

    // A surface claiming to be built must be findable in the tree. Without
    // this the registry is just a second place to write the same unchecked
    // string.
    const source = readComponent(surface.component);
    if (source === null) {
      fail(
        "phantom-surface",
        name,
        `is marked built but ${surface.component} does not exist`
      );
    } else if (!source.includes(`data-surface="${surface.surfaceId}"`)) {
      fail(
        "phantom-surface",
        name,
        `is marked built but ${surface.component} does not render data-surface="${surface.surfaceId}"`
      );
    }
  }

  // --- 2, 4, 5, 6, 7b: per-entry invariants ---------------------------------
  for (const [key, e] of Object.entries(entries)) {
    if (!e.status) {
      fail("missing-status", key, "entry has no status");
      continue;
    }
    if (!VOCAB.has(e.status)) {
      fail("unknown-status", key, `status "${e.status}" is not in statusVocabulary`);
    }

    if (e.status === "temporarily-deferred") {
      const d = e.deferral;
      if (!d) {
        fail("incomplete-deferral", key, "temporarily-deferred requires a deferral block");
      } else {
        for (const field of ["why", "dependencies", "interface", "expectedPhase"]) {
          if (d[field] === undefined) {
            fail("incomplete-deferral", key, `deferral is missing "${field}"`);
          }
        }
        if (!e.upstream) {
          fail("incomplete-deferral", key, "deferral must name the upstream files that implement it");
        }
      }
    }

    if (e.status === "removed-vendor-service" && !e.detail) {
      fail("undeclared-removal", key, "removed-vendor-service requires a detail explaining what was removed");
    }

    // Documentation entries carry no code, so the backend and UI requirements
    // below do not apply to them. They must instead name where the rewritten
    // docs live -- and, if their status claims those docs exist, actually have
    // them there.
    //
    // The second half is KNEG-86. Naming a target was the whole of the check
    // for twenty tasks, and `developer:agent-skills` spent all of them claiming
    // `status: "ported"` against `docs/agent-skills/`, a directory that has
    // never existed. The exemption from the backend/UI rules was right; the
    // hole was that nothing took its place, so a documentation entry was the
    // one kind that could assert work was done with nothing checking.
    //
    // Existence *and* content: an empty directory satisfies "the rewritten docs
    // live here" no better than a missing one, and is the same hole one commit
    // later.
    if (e.kind === "documentation") {
      if (!e.target) {
        fail("incomplete-documentation", key, 'kind "documentation" requires a target path');
      } else if (ACTIVE.has(e.status) && !hasTarget(e.target)) {
        fail(
          "documentation-target-missing",
          key,
          `status "${e.status}" claims the rewritten documentation is at "${e.target}", ` +
            "which does not exist or is empty"
        );
      }
      continue;
    }

    if (!ACTIVE.has(e.status)) continue;

    if (!e.backend) {
      fail("frontend-without-backend", key, `status "${e.status}" claims an implementation but names no backend`);
    }

    const prefix = key.split(":")[0];
    if (!USER_CONFIGURABLE.has(prefix)) continue;

    if (!e.ui) {
      fail("backend-without-ui", key, `status "${e.status}" is user-configurable but names no UI surface`);
      continue;
    }

    const surface = declaredSurfaces[e.ui];
    if (!surface) {
      fail(
        "unknown-ui-surface",
        key,
        `names UI surface "${e.ui}", which config/ui-surfaces.json does not declare`
      );
    }

    // Absent is a violation, not a default. A new integration cannot be added
    // without someone stating whether a user can actually get to it.
    if (typeof e.reachable !== "boolean") {
      fail(
        "missing-reachability",
        key,
        'entry must declare "reachable": true or false; absent is not the same as false'
      );
    } else if (e.reachable && surface && !surface.built) {
      fail(
        "unreachable-surface",
        key,
        `claims to be reachable through "${e.ui}", which is not built`
      );
    }
  }

  // --- baseline pin ---------------------------------------------------------
  if (manifest.baseline?.commit !== baseline.provenance?.commit) {
    fail(
      "baseline-drift",
      "manifest.baseline.commit",
      `manifest pins ${manifest.baseline?.commit} but the baseline is ${baseline.provenance?.commit}`
    );
  }

  return found;
}

/** A component's source as committed, or null when there is no such file. */
function readComponentFromDisk(relative) {
  const path = join(REPO_ROOT, relative);
  return existsSync(path) ? readFileSync(path, "utf8") : null;
}

/**
 * Whether a documentation target exists on disk and holds something.
 *
 * A directory has to be non-empty, because a manifest entry saying the
 * rewritten documentation lives at `docs/agent-skills/` is not satisfied by an
 * empty `docs/agent-skills/`. A file only has to exist -- an empty one is a
 * different kind of wrong and not one this gate is placed to judge.
 */
export function hasTargetOnDisk(relative, root = REPO_ROOT) {
  const path = join(root, relative);
  if (!existsSync(path)) return false;
  return statSync(path).isDirectory() ? readdirSync(path).length > 0 : true;
}

function main() {
  const manifest = read("config/integration-manifest.json");
  const surfaces = read("config/ui-surfaces.json");
  const found = violations({
    baseline: read("config/upstream-integration-baseline.json"),
    manifest,
    surfaces,
    readComponent: readComponentFromDisk,
    hasTarget: (target) => hasTargetOnDisk(target),
  });

  const total = Object.keys(manifest.entries).length;

  if (found.length === 0) {
    const byStatus = {};
    for (const e of Object.values(manifest.entries)) {
      byStatus[e.status] = (byStatus[e.status] ?? 0) + 1;
    }
    const baseline = read("config/upstream-integration-baseline.json");
    console.log(
      `integration parity OK — ${total} entries against baseline ${baseline.provenance.commit.slice(0, 12)}`
    );
    for (const [s, n] of Object.entries(byStatus).sort((a, b) => b[1] - a[1])) {
      console.log(`  ${s.padEnd(38)} ${n}`);
    }

    // The backlog, printed on success rather than buried. These are not
    // violations -- they are honestly declared gaps, and this is the list of
    // what is left to build.
    const unreachable = Object.entries(manifest.entries).filter(([, e]) => e.reachable === false);
    if (unreachable.length > 0) {
      const bySurface = {};
      for (const [, e] of unreachable) (bySurface[e.ui] ??= []).push(e);
      console.log(`\n${unreachable.length} integrations are declared unreachable:`);
      for (const [ui, list] of Object.entries(bySurface).sort((a, b) => b[1].length - a[1].length)) {
        const built = surfaces.surfaces[ui]?.built ? "surface built" : "surface not built";
        console.log(`  ${ui.padEnd(36)} ${String(list.length).padStart(2)}  (${built})`);
      }
    }
    process.exit(0);
  }

  console.error(`integration parity FAILED — ${found.length} violation(s) across ${total} entries\n`);
  const grouped = {};
  for (const v of found) (grouped[v.rule] ??= []).push(v);
  for (const [rule, vs] of Object.entries(grouped)) {
    console.error(`  ${rule} (${vs.length})`);
    for (const v of vs) console.error(`    ${v.key}: ${v.detail}`);
    console.error("");
  }
  process.exit(1);
}

if (resolve(process.argv[1] ?? "") === fileURLToPath(import.meta.url)) {
  main();
}
