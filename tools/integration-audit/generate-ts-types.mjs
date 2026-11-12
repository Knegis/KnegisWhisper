#!/usr/bin/env node
/**
 * Generates apps/desktop/src/generated/providers.ts from
 * config/provider-registry.json.
 *
 * The Rust side embeds the same JSON via include_str!, so provider and model
 * identity is declared exactly once and both languages get it typed. This is
 * what makes the brief's "do not manually duplicate provider lists across
 * multiple files" structural rather than a review rule.
 *
 * Run: node tools/integration-audit/generate-ts-types.mjs [--check]
 */

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const SRC = join(REPO_ROOT, "config/provider-registry.json");
const OUT = join(REPO_ROOT, "apps/desktop/src/generated/providers.ts");

const registry = JSON.parse(readFileSync(SRC, "utf8"));

/** Emit a readonly tuple of string literals plus its union type. */
function idUnion(typeName, constName, ids, doc) {
  const items = ids.map((id) => JSON.stringify(id)).join(", ");
  return [
    `/** ${doc} */`,
    `export const ${constName} = [${items}] as const;`,
    `export type ${typeName} = (typeof ${constName})[number];`,
  ].join("\n");
}

const transcriptionIds = registry.transcription.map((p) => p.id);
const reasoningIds = registry.reasoning.map((p) => p.id);
const enterpriseIds = registry.enterprise.map((p) => p.id);
const localLlmIds = registry.localLlm.map((p) => p.id);
const whisperIds = registry.localAsr.whisper.map((m) => m.id);
const sherpaIds = registry.localAsr.sherpa.map((m) => m.id);
const diarizationIds = registry.localAsr.diarization.map((m) => m.id);

// Credential slots come from the registry's own list, not from the provider
// lists. Deriving them from providers gave nine; there are fifteen, because
// `openrouter` has no pinned models and the five custom endpoints have no
// provider entry. In registry order, which is upstream's secretKeys order, so
// the screen renders provider slots before custom ones without sorting for it.
const credentialSlots = registry.credentials.map((c) => c.slot);

const header = `/**
 * GENERATED FILE — DO NOT EDIT.
 *
 * Source:    config/provider-registry.json
 * Generator: tools/integration-audit/generate-ts-types.mjs
 * Baseline:  ${registry.derivedFrom.baselineCommit}
 *
 * Regenerate with:
 *   node tools/integration-audit/generate-ts-types.mjs
 *
 * The Rust side reads the same JSON through crates/provider-registry, so a
 * provider or model id is declared exactly once for the whole project.
 */
`;

const types = `
export type AuthScheme =
  | "bearer"
  | "azure-api-key"
  | "oauth2-client-credentials"
  | "attested"
  | "none";

/** What a credential slot is for. Mirrors \`provider_registry::CredentialKind\`. */
export type CredentialKind = "provider" | "custom-endpoint" | "enterprise";

/**
 * One slot in the OS credential store. Mirrors
 * \`provider_registry::CredentialSlot\`.
 *
 * \`kind\` is the distinction a screen has to make. A \`provider\` key is enough on
 * its own to try that service; an \`enterprise\` slot holds a whole multi-field
 * configuration as JSON and is set on its own screen; a \`custom-endpoint\` key
 * configures nothing
 * without the base URL that lives with the feature it belongs to, so a screen
 * storing one must say so rather than implying the key finished the job.
 */
export interface CredentialSlotSpec {
  readonly slot: CredentialSlot;
  readonly name: string;
  readonly kind: CredentialKind;
  /** Upstream's environment-variable name, for the OpenWhispr importer. */
  readonly env: string;
  /** \`transcription\`, \`reasoning\`, or neither. Derived, never restated. */
  readonly usedFor: readonly string[];
  /** Which feature a custom endpoint belongs to. */
  readonly feature?: string;
}

export interface TranscriptionModel {
  readonly id: string;
  readonly name: string;
  /**
   * Whether this model supports realtime streaming.
   *
   * A model property, not a provider one: only four models across all seven
   * providers declare it, and Berget offers three models of which none do.
   */
  readonly streaming: boolean;
}

export interface TranscriptionProvider {
  readonly id: TranscriptionProviderId;
  readonly name: string;
  readonly baseUrl: string | null;
  readonly authScheme: AuthScheme;
  readonly credentialSlot: CredentialSlot;
  readonly allowCustomModelId: boolean;
  /** \`null\` means genuinely uncapped. Explicit on every provider. */
  readonly maxUploadBytes: number | null;
  readonly batchModel?: string;
  readonly models: readonly TranscriptionModel[];
}

export interface ReasoningModel {
  readonly id: string;
  readonly name: string;
  /** \`max_tokens\` or \`max_completion_tokens\`; providers disagree. */
  readonly tokenParam: string;
  readonly supportsTemperature: boolean;
  readonly supportsThinking: boolean;
  readonly supportsVision: boolean;
}

export interface ReasoningProvider {
  readonly id: ReasoningProviderId;
  readonly name: string;
  readonly authScheme: AuthScheme;
  readonly credentialSlot: CredentialSlot;
  readonly allowCustomModelId: boolean;
  readonly models: readonly ReasoningModel[];
}

export interface EnterpriseProvider {
  readonly id: EnterpriseProviderId;
  readonly name: string;
  /** Enterprise credentials are multi-field, so there is no single slot. */
  readonly credentialShape: string;
  readonly allowCustomModelId: boolean;
  /** Azure ships no model list; the deployment name selects the model. */
  readonly deploymentNameDriven: boolean;
  readonly models: readonly { readonly id: string; readonly name: string }[];
}

/** A speculative-decoding drafter downloaded alongside the main GGUF. */
export interface DraftModel {
  readonly hfRepo: string;
  /** On the same terms as \`LocalLlmModel.revision\`. */
  readonly revision: string;
  readonly fileName: string;
  readonly sizeBytes: number | null;
  readonly sizeMeasured: boolean;
  /** On the same terms as \`LocalLlmModel.sha256\`. */
  readonly sha256: string | null;
  readonly sha256Measured: boolean;
}

export interface LocalLlmModel {
  readonly id: string;
  readonly name: string;
  readonly fileName: string;
  readonly hfRepo: string;
  /**
   * The commit the GGUF is downloaded from — a 40-character sha, never
   * \`main\`.
   *
   * A branch pointer moves when the publisher pushes, which would move the
   * bytes out from under the digest committed beside them and fail an install
   * for a reason that has nothing to do with the user. Measured into
   * \`HF_REVISIONS\` by \`node tools/model-pin/pin.mjs --revisions\`.
   */
  readonly revision: string;
  readonly sizeBytes: number | null;
  /**
   * Whether \`sizeBytes\` was measured against the publisher or copied from
   * upstream's declaration. Upstream declared a size for every model and was
   * wrong about eighteen of them, so a non-null size is not on its own a
   * trustworthy one; the installer holds a measured size to exact equality and
   * a declared one to 5%.
   */
  readonly sizeMeasured: boolean;
  /**
   * The object's SHA-256, and whether this project measured it.
   *
   * Non-null on all thirty-nine published objects. \`sha256Measured\` is the
   * part that matters: **true** means the bytes were downloaded and hashed
   * here, **false** means this is the publisher's stated \`lfs.oid\` — a real
   * digest of the object, but one delivered by the same host that serves the
   * bytes, so it catches corruption and re-uploads and not a compromised
   * huggingface.co. Seven of the thirty-nine are measured, one per publishing
   * organisation. Anything shown to a person about how a model was verified
   * must read this flag and not just the digest.
   */
  readonly sha256: string | null;
  readonly sha256Measured: boolean;
  readonly quantization: string | null;
  readonly contextLength: number | null;
  readonly recommended: boolean;
  readonly supportsThinking: boolean;
  readonly draft: DraftModel | null;
}

export interface LocalLlmProvider {
  readonly id: LocalLlmProviderId;
  readonly name: string;
  readonly baseUrl: string | null;
  readonly models: readonly LocalLlmModel[];
}

export interface WhisperModel {
  readonly id: WhisperModelId;
  readonly name: string;
  readonly fileName: string;
  readonly downloadUrl: string;
  /** A display estimate, rounded. This is the figure the screen shows. */
  readonly sizeMb: number | null;
  /**
   * The model file's pinned digest and its expected byte count. Non-null on
   * all six shipped models, both measured from a real download; a null
   * \`sha256\` would mean a model added to the registry before anybody
   * downloaded it, which \`models::integrity_for\` degrades to a size check
   * rather than hiding.
   *
   * With a digest, \`expectedSizeBytes\` is the measured length — what the
   * integrity check is handed, so it has to be bytes that were there. Without
   * one it is upstream's rounded declaration, held to a 5% tolerance. It is
   * never \`sizeMb\` scaled.
   */
  readonly sha256: string | null;
  readonly expectedSizeBytes: number | null;
  readonly recommended: boolean;
}

export interface ModelFile {
  readonly path: string;
  readonly url: string;
  readonly sha256: string;
  readonly expectedSizeBytes: number;
}

export interface SherpaModel {
  readonly files: readonly ModelFile[];
  readonly id: SherpaModelId;
  readonly name: string;
  readonly downloadUrl: string;
  readonly extractDir: string;
  /** A display estimate of the *extracted* footprint, not the download. */
  readonly sizeMb: number | null;
  /**
   * The archive's pinned digest and exact compressed size, and a ceiling on
   * what it may extract to. Nullable in the type because sherpa_models::install
   * refuses an unpinned model rather than downloading something
   * unauthenticated -- but as of 2026-09-01 all four carry all three: the two
   * Nemotron streaming models were the last unpinned pair and both were
   * measured that day.
   */
  readonly sha256: string | null;
  readonly expectedSizeBytes: number | null;
  readonly maxExtractedSizeBytes: number | null;
  readonly language: string | null;
  readonly supportedLanguages: readonly string[] | null;
  /** True for the Nemotron streaming runtimes; Parakeet models are batch. */
  readonly streaming: boolean;
}

/** Which half of a speaker-diarization run a model does. */
export type DiarizationRole = "segmentation" | "embedding";

/**
 * One of the two models a speaker-diarization run needs.
 *
 * Not a SherpaModel despite coming from the same publisher: only one of the two
 * is an archive, neither declares a language the way an ASR model does, and
 * neither is streaming.
 */
export interface DiarizationModel {
  readonly files: readonly ModelFile[];
  readonly id: DiarizationModelId;
  readonly name: string;
  readonly downloadUrl: string;
  /** Null for the embedding model, a single .onnx with nothing to extract. */
  readonly extractDir: string | null;
  /** Null for the segmentation model, which is an archive. Exactly one of
   *  this and extractDir is set. */
  readonly fileName: string | null;
  readonly sizeMb: number | null;
  readonly role: DiarizationRole | null;
  readonly sha256: string | null;
  readonly expectedSizeBytes: number | null;
  /** Null for the embedding model: nothing extracts it. */
  readonly maxExtractedSizeBytes: number | null;
}

export interface ProviderRegistry {
  readonly credentials: readonly CredentialSlotSpec[];
  readonly transcription: readonly TranscriptionProvider[];
  readonly reasoning: readonly ReasoningProvider[];
  readonly enterprise: readonly EnterpriseProvider[];
  readonly localLlm: readonly LocalLlmProvider[];
  readonly localAsr: {
    readonly whisper: readonly WhisperModel[];
    readonly sherpa: readonly SherpaModel[];
    readonly diarization: readonly DiarizationModel[];
  };
}
`;

const data = {
  credentials: registry.credentials,
  transcription: registry.transcription,
  reasoning: registry.reasoning,
  enterprise: registry.enterprise,
  localLlm: registry.localLlm,
  localAsr: registry.localAsr,
};

const helpers = `
/**
 * Look up a transcription provider, or \`undefined\`.
 *
 * Callers must handle \`undefined\` rather than substituting a default. Upstream
 * defaulted an unrecognised provider to OpenAI with the OpenAI credential, which
 * meant a typo silently sent the user's audio to a provider they never chose.
 */
export function findTranscriptionProvider(
  id: string,
): TranscriptionProvider | undefined {
  return PROVIDER_REGISTRY.transcription.find((p) => p.id === id);
}

export function findReasoningProvider(id: string): ReasoningProvider | undefined {
  return PROVIDER_REGISTRY.reasoning.find((p) => p.id === id);
}

/**
 * Look up a credential slot, or \`undefined\`.
 *
 * Callers must handle \`undefined\`. A screen that invented a slot name would be
 * writing a secret to a key nothing reads, which looks exactly like success.
 */
export function findCredentialSlot(slot: string): CredentialSlotSpec | undefined {
  return PROVIDER_REGISTRY.credentials.find((c) => c.slot === slot);
}

/** Every provider/model pair that supports realtime streaming transcription. */
export function streamingTranscriptionModels(): readonly {
  readonly providerId: TranscriptionProviderId;
  readonly modelId: string;
}[] {
  return PROVIDER_REGISTRY.transcription.flatMap((p) =>
    p.models
      .filter((m) => m.streaming)
      .map((m) => ({ providerId: p.id, modelId: m.id })),
  );
}
`;

const rendered = [
  header,
  idUnion(
    "TranscriptionProviderId",
    "TRANSCRIPTION_PROVIDER_IDS",
    transcriptionIds,
    "Transcription providers, in registry order.",
  ),
  idUnion(
    "ReasoningProviderId",
    "REASONING_PROVIDER_IDS",
    reasoningIds,
    "Cloud reasoning providers, in registry order.",
  ),
  idUnion(
    "EnterpriseProviderId",
    "ENTERPRISE_PROVIDER_IDS",
    enterpriseIds,
    "Enterprise connectors, configured with organisation credentials.",
  ),
  idUnion(
    "LocalLlmProviderId",
    "LOCAL_LLM_PROVIDER_IDS",
    localLlmIds,
    "Local llama.cpp model families.",
  ),
  idUnion("WhisperModelId", "WHISPER_MODEL_IDS", whisperIds, "Local whisper.cpp models."),
  idUnion(
    "SherpaModelId",
    "SHERPA_MODEL_IDS",
    sherpaIds,
    "Local sherpa-onnx models (Parakeet batch, Nemotron streaming).",
  ),
  idUnion(
    "DiarizationModelId",
    "DIARIZATION_MODEL_IDS",
    diarizationIds,
    "The two models a speaker-diarization run needs, one of each role.",
  ),
  idUnion(
    "CredentialSlot",
    "CREDENTIAL_SLOTS",
    credentialSlots,
    "OS credential-store slots, in registry order. Twenty-one: twelve providers, five per-feature custom endpoints, your own server's key (custom) and three enterprise connectors.",
  ),
  types.trim(),
  `/** The registry. Generated from config/provider-registry.json. */\nexport const PROVIDER_REGISTRY = ${JSON.stringify(
    data,
    null,
    2,
  )} as const satisfies ProviderRegistry;`,
  helpers.trim(),
  "",
].join("\n\n");

if (process.argv.includes("--check")) {
  let committed;
  try {
    committed = readFileSync(OUT, "utf8");
  } catch {
    console.error(`generate-ts-types --check: ${OUT} does not exist`);
    process.exit(1);
  }
  if (committed !== rendered) {
    console.error(
      "generate-ts-types --check: generated types are stale. Run\n" +
        "  node tools/integration-audit/generate-ts-types.mjs\n" +
        "and commit the result.",
    );
    process.exit(1);
  }
  console.log("generate-ts-types --check: up to date.");
  process.exit(0);
}

mkdirSync(dirname(OUT), { recursive: true });
writeFileSync(OUT, rendered, "utf8");
console.log(`wrote ${OUT}`);
console.log(`  ${transcriptionIds.length} transcription, ${reasoningIds.length} reasoning, ${enterpriseIds.length} enterprise, ${localLlmIds.length} local LLM providers`);
console.log(
  `  ${whisperIds.length} whisper, ${sherpaIds.length} sherpa, ` +
    `${diarizationIds.length} diarization models, ${credentialSlots.length} credential slots`
);
