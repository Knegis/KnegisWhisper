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
 * (Forbidden vendor endpoints are the separate concern of the domain scanner.)
 *
 * Reads only the two committed JSON files — never the upstream checkout — so this
 * runs in CI on any platform with no external dependencies.
 *
 * Usage: node tools/integration-audit/verify-parity.mjs
 * Exit:  0 = parity holds, 1 = one or more violations
 */

import { readFileSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const read = (p) => JSON.parse(readFileSync(join(REPO_ROOT, p), "utf8"));

const baseline = read("config/upstream-integration-baseline.json");
const manifest = read("config/integration-manifest.json");

const VOCAB = new Set(manifest.statusVocabulary);
const entries = manifest.entries;

/** Statuses that assert a working implementation exists in this codebase. */
const ACTIVE = new Set([
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
const USER_CONFIGURABLE = new Set([
  "transcription",
  "reasoning",
  "enterprise",
  "local-llm",
  "local-asr",
  "meeting-stream",
  "credential",
  "calendar",
  "developer",
  "media",
]);

const violations = [];
const fail = (rule, key, detail) => violations.push({ rule, key, detail });

// --- 1 & 3: every baseline provider and model-family ID must be represented ---
const expectProvider = (manifestKey, label) => {
  if (!entries[manifestKey]) {
    fail("missing-upstream-provider", manifestKey, `${label} is in the baseline but has no manifest entry`);
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
      fail("dropped-registry-model", manifestKey, `${label} model "${m}" is in the baseline but not in the manifest entry`);
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
      checkModels(key, p.models.map((m) => m.id), label);
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
      fail("missing-upstream-provider", alias, `vendor inference provider "${p.id}" has no manifest entry`);
    } else if (e.status !== "removed-vendor-service") {
      fail("undeclared-removal", alias, `vendor inference provider "${p.id}" must be removed-vendor-service, found "${e.status}"`);
    }
  }
}

// --- 2, 4, 5, 6: per-entry invariants ---------------------------------------
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

  // Documentation entries carry no code, so backend/UI requirements do not apply.
  // They must instead name where the rewritten docs live.
  if (e.kind === "documentation") {
    if (!e.target) {
      fail("incomplete-documentation", key, 'kind "documentation" requires a target path');
    }
  } else if (ACTIVE.has(e.status)) {
    if (!e.backend) {
      fail("frontend-without-backend", key, `status "${e.status}" claims an implementation but names no backend`);
    }
    const prefix = key.split(":")[0];
    if (USER_CONFIGURABLE.has(prefix) && !e.ui) {
      fail("backend-without-ui", key, `status "${e.status}" is user-configurable but names no UI surface`);
    }
  }
}

// --- baseline pin ------------------------------------------------------------
if (manifest.baseline?.commit !== baseline.provenance?.commit) {
  fail(
    "baseline-drift",
    "manifest.baseline.commit",
    `manifest pins ${manifest.baseline?.commit} but the baseline is ${baseline.provenance?.commit}`
  );
}

// --- report ------------------------------------------------------------------
const total = Object.keys(entries).length;
if (violations.length === 0) {
  const byStatus = {};
  for (const e of Object.values(entries)) byStatus[e.status] = (byStatus[e.status] ?? 0) + 1;
  console.log(`integration parity OK — ${total} entries against baseline ${baseline.provenance.commit.slice(0, 12)}`);
  for (const [s, n] of Object.entries(byStatus).sort((a, b) => b[1] - a[1])) {
    console.log(`  ${s.padEnd(38)} ${n}`);
  }
  process.exit(0);
}

console.error(`integration parity FAILED — ${violations.length} violation(s) across ${total} entries\n`);
const grouped = {};
for (const v of violations) (grouped[v.rule] ??= []).push(v);
for (const [rule, vs] of Object.entries(grouped)) {
  console.error(`  ${rule} (${vs.length})`);
  for (const v of vs) console.error(`    ${v.key}: ${v.detail}`);
  console.error("");
}
process.exit(1);
