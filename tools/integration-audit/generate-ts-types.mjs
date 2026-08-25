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

// Credential slots, deduplicated across transcription and reasoning. A provider
// offering both (openai, groq, berget, corti, tinfoil) shares one slot.
const credentialSlots = [
  ...new Set([
    ...registry.transcription.map((p) => p.credentialSlot),
    ...registry.reasoning.map((p) => p.credentialSlot),
  ]),
].sort();

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
  readonly fileName: string;
  readonly sizeBytes: number | null;
}

export interface LocalLlmModel {
  readonly id: string;
  readonly name: string;
  readonly fileName: string;
  readonly hfRepo: string;
  readonly sizeBytes: number | null;
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
  readonly sizeMb: number | null;
  readonly expectedSizeBytes: number | null;
  readonly recommended: boolean;
}

export interface SherpaModel {
  readonly id: SherpaModelId;
  readonly name: string;
  readonly downloadUrl: string;
  readonly extractDir: string;
  readonly sizeMb: number | null;
  readonly language: string | null;
  readonly supportedLanguages: readonly string[] | null;
  /** True for the Nemotron streaming runtimes; Parakeet models are batch. */
  readonly streaming: boolean;
}

export interface ProviderRegistry {
  readonly transcription: readonly TranscriptionProvider[];
  readonly reasoning: readonly ReasoningProvider[];
  readonly enterprise: readonly EnterpriseProvider[];
  readonly localLlm: readonly LocalLlmProvider[];
  readonly localAsr: {
    readonly whisper: readonly WhisperModel[];
    readonly sherpa: readonly SherpaModel[];
  };
}
`;

const data = {
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
    "CredentialSlot",
    "CREDENTIAL_SLOTS",
    credentialSlots,
    "OS credential-store slots. A provider offering both transcription and reasoning shares one slot.",
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
console.log(`  ${whisperIds.length} whisper, ${sherpaIds.length} sherpa models, ${credentialSlots.length} credential slots`);
