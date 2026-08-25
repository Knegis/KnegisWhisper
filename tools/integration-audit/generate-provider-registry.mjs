#!/usr/bin/env node
/**
 * Derives config/provider-registry.json from the frozen upstream baseline.
 *
 * The baseline is a *record* of what upstream had. This registry is the
 * *authority* for what KnegisWhisper ships: it is what the Rust and TypeScript
 * sides both read, so a provider id or model id exists in exactly one place.
 *
 * Two deliberate normalisations are applied, both decided during the Phase 0
 * audit and recorded in docs/INTEGRATION_PARITY.md:
 *
 * 1. Every transcription provider declares an explicit `maxUploadBytes`.
 *    Upstream expresses the cap as a magic `sizeCapBytes: null` special case for
 *    Berget inside routing code, while OpenAI and Groq are capped at 25 MB
 *    elsewhere. Making it a per-provider field removes the special case without
 *    changing behaviour.
 *
 * 2. `openwhisprCloudModels` is not carried over. It had zero readers upstream
 *    and is a hosted-tiering artifact; the manifest records it as
 *    removed-vendor-service.
 *
 * Run: node tools/integration-audit/generate-provider-registry.mjs [--check]
 */

import { readFileSync, writeFileSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const BASELINE = join(REPO_ROOT, "config/upstream-integration-baseline.json");
const OUT = join(REPO_ROOT, "config/provider-registry.json");

/** 25 MiB — upstream's BYOK_FILE_SIZE_LIMIT and its proxied-path cap. */
const BYOK_UPLOAD_CAP = 25 * 1024 * 1024;

/**
 * Upload caps per transcription provider.
 *
 * `null` means genuinely uncapped, which is only Berget: upstream deliberately
 * routes it with `sizeCapBytes: null`. Recording that as data rather than a
 * branch is the whole point of this normalisation.
 */
const UPLOAD_CAPS = {
  openai: BYOK_UPLOAD_CAP,
  groq: BYOK_UPLOAD_CAP,
  berget: null,
  xai: BYOK_UPLOAD_CAP,
  mistral: BYOK_UPLOAD_CAP,
  corti: BYOK_UPLOAD_CAP,
  tinfoil: BYOK_UPLOAD_CAP,
};

/** Which credential slot each provider authenticates with. */
const AUTH_SCHEMES = {
  // Providers reached over an attested or proxied client rather than a plain
  // bearer request. Recorded so dispatch never has to infer it.
  tinfoil: "attested",
  corti: "oauth2-client-credentials",
};

function die(msg) {
  console.error(`generate-provider-registry: ${msg}`);
  process.exit(1);
}

const baseline = JSON.parse(readFileSync(BASELINE, "utf8"));
const R = baseline.registry;

// --- transcription -----------------------------------------------------------
const transcription = R.transcriptionProviders.map((p) => {
  if (!(p.id in UPLOAD_CAPS)) {
    die(
      `transcription provider "${p.id}" has no declared upload cap. Add it to ` +
        `UPLOAD_CAPS deliberately -- defaulting would reintroduce the magic ` +
        `value this normalisation removes.`
    );
  }
  return {
    id: p.id,
    name: p.name,
    baseUrl: p.baseUrl ?? null,
    authScheme: AUTH_SCHEMES[p.id] ?? "bearer",
    credentialSlot: p.id,
    allowCustomModelId: p.allowCustomModelId === true,
    maxUploadBytes: UPLOAD_CAPS[p.id],
    ...(p.batchModel !== undefined ? { batchModel: p.batchModel } : {}),
    models: p.models.map((m) => ({
      id: m.id,
      name: m.name,
      streaming: m.streaming === true,
    })),
  };
});

// --- cloud reasoning ---------------------------------------------------------
const reasoning = R.cloudProviders.map((p) => ({
  id: p.id,
  name: p.name,
  authScheme: AUTH_SCHEMES[p.id] ?? "bearer",
  credentialSlot: p.id,
  allowCustomModelId: p.allowCustomModelId === true,
  models: p.models.map((m) => ({
    id: m.id,
    name: m.name,
    tokenParam: m.tokenParam ?? "max_tokens",
    supportsTemperature: m.supportsTemperature === true,
    supportsThinking: m.supportsThinking === true,
    supportsVision: m.supportsVision === true,
  })),
}));

// --- enterprise --------------------------------------------------------------
const enterprise = R.enterpriseProviders.map((p) => ({
  id: p.id,
  name: p.name,
  // Credentials are multi-field (region/profile/key, endpoint/deployment, ...)
  // rather than a single bearer token, so dispatch must not assume one slot.
  credentialShape: `${p.id}-multi-field`,
  allowCustomModelId: p.allowCustomModelId === true,
  deploymentNameDriven: p.models.length === 0,
  models: p.models.map((m) => ({ id: m.id, name: m.name })),
}));

// --- local llama.cpp ---------------------------------------------------------
const localLlm = R.localProviders.map((p) => ({
  id: p.id,
  name: p.name,
  baseUrl: p.baseUrl ?? null,
  models: p.models.map((m) => ({
    id: m.id,
    name: m.name,
    fileName: m.fileName,
    hfRepo: m.hfRepo,
    sizeBytes: m.sizeBytes ?? null,
    quantization: m.quantization ?? null,
    contextLength: m.contextLength ?? null,
    recommended: m.recommended === true,
    supportsThinking: m.supportsThinking === true,
    // Speculative-decoding drafter, present on the Gemma QAT models only.
    draft:
      m.draftHfRepo === undefined
        ? null
        : {
            hfRepo: m.draftHfRepo,
            fileName: m.draftFileName,
            sizeBytes: m.draftSizeBytes ?? null,
          },
  })),
}));

// --- local ASR ---------------------------------------------------------------
const whisper = Object.entries(R.whisperModels).map(([id, m]) => ({
  id,
  name: m.name,
  fileName: m.fileName,
  downloadUrl: m.downloadUrl,
  sizeMb: m.sizeMb ?? null,
  expectedSizeBytes: m.expectedSizeBytes ?? null,
  recommended: m.recommended === true,
}));

const sherpa = Object.entries(R.parakeetModels).map(([id, m]) => ({
  id,
  name: m.name,
  downloadUrl: m.downloadUrl,
  extractDir: m.extractDir,
  sizeMb: m.sizeMb ?? null,
  language: m.language ?? null,
  supportedLanguages: m.supportedLanguages ?? null,
  // "online" means a streaming-capable runtime; "offline" is batch only.
  streaming: m.runtime === "online",
}));

const registry = {
  $comment:
    "GENERATED by tools/integration-audit/generate-provider-registry.mjs from " +
    "config/upstream-integration-baseline.json. Do not hand-edit. This is the " +
    "single source of truth for provider and model identity: both the Rust " +
    "crates and the TypeScript frontend read it, so no id is ever written twice.",
  schemaVersion: 1,
  derivedFrom: {
    baselineCommit: baseline.provenance.commit,
    normalisations: [
      "every transcription provider declares an explicit maxUploadBytes",
      "openwhisprCloudModels dropped as removed-vendor-service",
    ],
  },
  transcription,
  reasoning,
  enterprise,
  localLlm,
  localAsr: { whisper, sherpa },
};

const rendered = JSON.stringify(registry, null, 2) + "\n";

if (process.argv.includes("--check")) {
  let committed;
  try {
    committed = readFileSync(OUT, "utf8");
  } catch {
    die(`--check: ${OUT} does not exist`);
  }
  if (committed !== rendered) {
    die("--check: committed provider registry is stale; regenerate and commit");
  }
  console.log("generate-provider-registry --check: up to date.");
  process.exit(0);
}

writeFileSync(OUT, rendered, "utf8");

const counts = {
  transcription: `${transcription.length} providers / ${transcription.reduce((n, p) => n + p.models.length, 0)} models`,
  reasoning: `${reasoning.length} providers / ${reasoning.reduce((n, p) => n + p.models.length, 0)} models`,
  enterprise: `${enterprise.length} providers / ${enterprise.reduce((n, p) => n + p.models.length, 0)} models`,
  localLlm: `${localLlm.length} providers / ${localLlm.reduce((n, p) => n + p.models.length, 0)} models`,
  localAsr: `${whisper.length} whisper / ${sherpa.length} sherpa`,
};
console.log(`wrote ${OUT}`);
for (const [k, v] of Object.entries(counts)) {
  console.log(`  ${k.padEnd(14)} ${v}`);
}
