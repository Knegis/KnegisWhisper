#!/usr/bin/env node
/**
 * Generates config/upstream-integration-baseline.json — the frozen record of the
 * OpenWhispr integration surface that KnegisWhisper claims parity with.
 *
 * Run this ONCE per baseline bump, never in CI. CI reads the generated JSON and
 * never needs the upstream checkout.
 *
 * Upstream source files are read as TEXT and parsed with targeted regexes rather
 * than imported, so we never execute upstream code (several of these modules pull
 * in Electron). Every extractor asserts an expected count, so upstream drift fails
 * loudly instead of silently producing a shorter baseline.
 *
 * Usage:
 *   node tools/integration-audit/generate-baseline.mjs [--upstream <path>] [--check]
 *
 *   --check  Regenerate in memory and diff against the committed baseline;
 *            exit 1 on mismatch. Used when bumping the baseline deliberately.
 */

import { readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(HERE, "..", "..");

const DEFAULT_UPSTREAM = "C:/Users/AxelKarlsson/Dyve/Dyve Internal/openwhispr";

// ---------------------------------------------------------------------------
// Expected counts. These are assertions, not documentation: if upstream changes
// shape, the generator must fail rather than emit a quietly-truncated baseline.
// Sourced from the Phase 0 audit of c4000db9.
// ---------------------------------------------------------------------------
const EXPECT = {
  transcriptionProviders: { providers: 7, models: 12 },
  cloudProviders: { providers: 7, models: 40 },
  enterpriseProviders: { providers: 3, models: 9 },
  localProviders: { providers: 6, models: 35 },
  whisperModels: 6,
  parakeetModels: 4,
  diarizationModels: 2,
  secretKeys: 15,
  meetingStreamingProviders: 5,
  inferenceProviderRegistry: 15,
  cloudChatProviders: 9,
};

const args = process.argv.slice(2);
const flag = (name) => {
  const i = args.indexOf(name);
  return i === -1 ? null : args[i + 1] ?? true;
};
const UPSTREAM = resolve(String(flag("--upstream") ?? DEFAULT_UPSTREAM));
const CHECK_ONLY = args.includes("--check");

function die(msg) {
  console.error(`generate-baseline: ${msg}`);
  process.exit(1);
}

function readUpstream(relPath) {
  const p = join(UPSTREAM, relPath);
  if (!existsSync(p)) die(`missing upstream file: ${p}`);
  return readFileSync(p, "utf8");
}

function assertCount(label, actual, expected) {
  if (actual !== expected) {
    die(
      `${label}: expected ${expected}, found ${actual}. ` +
        `Upstream shape changed — review the diff, then update EXPECT deliberately.`
    );
  }
}

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------
function gitProvenance() {
  const git = (...a) =>
    execFileSync("git", ["-C", UPSTREAM, ...a], { encoding: "utf8" }).trim();
  let dirty = true;
  try {
    dirty = git("status", "--porcelain").length > 0;
  } catch {
    /* leave dirty=true */
  }
  return {
    repo: "OpenWhispr/openwhispr (via axelalbertkarlsson fork)",
    commit: git("rev-parse", "HEAD"),
    branch: git("rev-parse", "--abbrev-ref", "HEAD"),
    subject: git("log", "-1", "--format=%s"),
    upstreamForkPoint: git("rev-parse", "HEAD^"),
    worktreeDirty: dirty,
    note:
      "Parity baseline is this fork commit, not the brief's 858e824 (absent from " +
      "this clone). The ~3-commit delta to 858e824 touches no provider or registry " +
      "code. HEAD^ is pristine upstream main, available as a cross-check.",
  };
}

// ---------------------------------------------------------------------------
// Model registry — structured JSON, read directly
// ---------------------------------------------------------------------------
function extractRegistry() {
  const raw = JSON.parse(readUpstream("src/models/modelRegistryData.json"));

  const providerArray = (key) => {
    const arr = raw[key];
    if (!Array.isArray(arr)) die(`${key} is not an array`);
    const models = arr.reduce((n, p) => n + (p.models?.length ?? 0), 0);
    assertCount(`${key}.providers`, arr.length, EXPECT[key].providers);
    assertCount(`${key}.models`, models, EXPECT[key].models);
    return arr.map((p) => ({
      id: p.id,
      name: p.name,
      ...(p.baseUrl !== undefined ? { baseUrl: p.baseUrl } : {}),
      ...(p.batchModel !== undefined ? { batchModel: p.batchModel } : {}),
      ...(p.allowCustomModelId !== undefined
        ? { allowCustomModelId: p.allowCustomModelId }
        : {}),
      models: (p.models ?? []).map((m) => ({ ...m })),
    }));
  };

  const modelMap = (key) => {
    const obj = raw[key];
    if (!obj || typeof obj !== "object") die(`${key} is not an object`);
    assertCount(key, Object.keys(obj).length, EXPECT[key]);
    return Object.fromEntries(
      Object.keys(obj)
        .sort()
        .map((k) => [k, { ...obj[k] }])
    );
  };

  return {
    transcriptionProviders: providerArray("transcriptionProviders"),
    cloudProviders: providerArray("cloudProviders"),
    enterpriseProviders: providerArray("enterpriseProviders"),
    localProviders: providerArray("localProviders"),
    whisperModels: modelMap("whisperModels"),
    parakeetModels: modelMap("parakeetModels"),
    diarizationModels: modelMap("diarizationModels"),
    // openwhisprCloudModels is deliberately NOT carried into the baseline:
    // it has zero readers upstream and is a vendor-service artifact. Recorded
    // in the manifest as removed-vendor-service instead.
    droppedFromBaseline: {
      openwhisprCloudModels: {
        reason: "removed-vendor-service",
        readers: 0,
        detail:
          "Hosted tiering vestige. grep across src/ test/ scripts/ finds only " +
          "the JSON definition itself. Absent from the ModelRegistryData interface.",
      },
    },
  };
}

// ---------------------------------------------------------------------------
// Secret keys — pure-data CommonJS, but parsed as text for consistency
// ---------------------------------------------------------------------------
function extractSecretKeys() {
  const src = readUpstream("src/config/secretKeys.js");
  const entries = [];
  // Match each { base: "...", env: "...", get: "...", save: "...", storeKey: "..." }
  const re =
    /base:\s*"([^"]+)"\s*,\s*env:\s*"([^"]+)"\s*,\s*get:\s*"([^"]+)"\s*,\s*save:\s*"([^"]+)"\s*,\s*storeKey:\s*"([^"]+)"/g;
  for (const m of src.matchAll(re)) {
    entries.push({ base: m[1], env: m[2], get: m[3], save: m[4], storeKey: m[5] });
  }
  assertCount("secretKeys", entries.length, EXPECT.secretKeys);
  return entries;
}

// ---------------------------------------------------------------------------
// Meeting streaming providers — object literal keys
// ---------------------------------------------------------------------------
function extractMeetingProviders() {
  const src = readUpstream("src/helpers/meetingStreamingProviders.js");
  const block = src.match(
    /const STREAMING_CLIENT_BY_PROVIDER\s*=\s*\{([\s\S]*?)\n\};/
  );
  if (!block) die("could not locate STREAMING_CLIENT_BY_PROVIDER");
  const ids = [...block[1].matchAll(/"([^"]+)"\s*:/g)].map((m) => m[1]);
  assertCount("meetingStreamingProviders", ids.length, EXPECT.meetingStreamingProviders);
  return ids.sort();
}

// ---------------------------------------------------------------------------
// Inference provider registry — TS object literal, unquoted keys
// ---------------------------------------------------------------------------
function extractInferenceRegistry() {
  const src = readUpstream("src/services/ai/inferenceProviders/index.ts");
  const block = src.match(/Object\.freeze\(\{([\s\S]*?)\n\}\);/);
  if (!block) die("could not locate PROVIDER_REGISTRY Object.freeze");
  const pairs = [...block[1].matchAll(/^\s*([A-Za-z][\w-]*)\s*:\s*(\w+)\s*,/gm)].map(
    (m) => ({ id: m[1], impl: m[2] })
  );
  assertCount("inferenceProviderRegistry", pairs.length, EXPECT.inferenceProviderRegistry);
  return pairs.sort((a, b) => a.id.localeCompare(b.id));
}

// ---------------------------------------------------------------------------
// Cloud chat providers — Set literal
// ---------------------------------------------------------------------------
function extractCloudChatProviders() {
  const src = readUpstream("src/helpers/chatRouting.js");
  const block = src.match(/const CLOUD_CHAT_PROVIDERS\s*=\s*new Set\(\[([\s\S]*?)\]\);/);
  if (!block) die("could not locate CLOUD_CHAT_PROVIDERS");
  const ids = [...block[1].matchAll(/"([^"]+)"/g)].map((m) => m[1]);
  assertCount("cloudChatProviders", ids.length, EXPECT.cloudChatProviders);
  return ids.sort();
}

// ---------------------------------------------------------------------------
// Assemble
// ---------------------------------------------------------------------------
const registry = extractRegistry();

const baseline = {
  $comment:
    "GENERATED by tools/integration-audit/generate-baseline.mjs. Do not hand-edit. " +
    "This is the frozen upstream integration surface KnegisWhisper claims parity " +
    "with; config/integration-manifest.json is diffed against it in CI.",
  schemaVersion: 1,
  provenance: gitProvenance(),
  counts: {
    transcriptionProviders: registry.transcriptionProviders.length,
    transcriptionModels: registry.transcriptionProviders.reduce(
      (n, p) => n + p.models.length,
      0
    ),
    cloudProviders: registry.cloudProviders.length,
    cloudModels: registry.cloudProviders.reduce((n, p) => n + p.models.length, 0),
    enterpriseProviders: registry.enterpriseProviders.length,
    enterpriseModels: registry.enterpriseProviders.reduce(
      (n, p) => n + p.models.length,
      0
    ),
    localProviders: registry.localProviders.length,
    localModels: registry.localProviders.reduce((n, p) => n + p.models.length, 0),
    whisperModels: Object.keys(registry.whisperModels).length,
    parakeetModels: Object.keys(registry.parakeetModels).length,
    diarizationModels: Object.keys(registry.diarizationModels).length,
    secretKeys: EXPECT.secretKeys,
    meetingStreamingProviders: EXPECT.meetingStreamingProviders,
  },
  registry,
  secretKeys: extractSecretKeys(),
  meetingStreamingProviders: extractMeetingProviders(),
  inferenceProviderRegistry: extractInferenceRegistry(),
  cloudChatProviders: extractCloudChatProviders(),
};

const OUT = join(REPO_ROOT, "config", "upstream-integration-baseline.json");
const rendered = JSON.stringify(baseline, null, 2) + "\n";

if (CHECK_ONLY) {
  if (!existsSync(OUT)) die(`--check: ${OUT} does not exist yet`);
  const committed = readFileSync(OUT, "utf8");
  if (committed !== rendered) {
    console.error(
      "generate-baseline --check: committed baseline differs from regenerated output.\n" +
        "If this is a deliberate baseline bump, re-run without --check and commit."
    );
    process.exit(1);
  }
  console.log("generate-baseline --check: baseline is up to date.");
  process.exit(0);
}

mkdirSync(dirname(OUT), { recursive: true });
writeFileSync(OUT, rendered, "utf8");

const c = baseline.counts;
console.log(`wrote ${OUT}`);
console.log(`  upstream       ${baseline.provenance.commit.slice(0, 12)} (${baseline.provenance.branch})`);
console.log(`  providers      ${c.transcriptionProviders + c.cloudProviders + c.enterpriseProviders + c.localProviders}`);
console.log(`  models         ${c.transcriptionModels + c.cloudModels + c.enterpriseModels + c.localModels}`);
console.log(`  local ASR      ${c.whisperModels} whisper, ${c.parakeetModels} parakeet, ${c.diarizationModels} diarization`);
console.log(`  secret keys    ${c.secretKeys}`);
console.log(`  meeting stream ${c.meetingStreamingProviders}`);
