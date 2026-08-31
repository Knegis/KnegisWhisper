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

/**
 * Every credential slot, with what it is for.
 *
 * `baseline.secretKeys` lists fifteen; deriving the set from the provider lists
 * instead yields nine, because six slots belong to no single provider:
 * `openrouter` (a model aggregator upstream pinned no model list for) and five
 * per-feature custom endpoints. `config/integration-manifest.json` has always
 * listed all fifteen, so the registry was quietly the smaller of two sources of
 * truth for the same thing — and a Providers screen would have had to invent the
 * missing six, which is exactly the duplicated provider list the registry
 * exists to prevent.
 *
 * `kind` is the distinction that matters to a UI:
 *
 * - `provider`        — a named service. A key on its own is enough to try it.
 * - `custom-endpoint` — a user-supplied OpenAI-compatible endpoint for one
 *                       feature. A key on its own configures nothing; the base
 *                       URL lives with that feature's own settings, so the
 *                       screen must say so rather than implying the key is all
 *                       that is needed.
 *
 * A slot in the baseline and absent here is a hard error, following
 * `UPLOAD_CAPS`: defaulting would let a new upstream key appear in the UI with
 * no one having decided what it is.
 */
const CREDENTIAL_SLOTS = {
  openai: { name: "OpenAI", kind: "provider" },
  anthropic: { name: "Anthropic", kind: "provider" },
  gemini: { name: "Google Gemini", kind: "provider" },
  groq: { name: "Groq", kind: "provider" },
  berget: { name: "Berget AI", kind: "provider" },
  xai: { name: "xAI", kind: "provider" },
  mistral: { name: "Mistral", kind: "provider" },
  tinfoil: { name: "Tinfoil", kind: "provider" },
  corti: { name: "Corti", kind: "provider" },
  // No models are pinned for OpenRouter — upstream treated it as a pass-through
  // aggregator — so it appears in no provider list while still needing a key.
  openrouter: { name: "OpenRouter", kind: "provider" },
  "note-formatting-custom": {
    name: "Note formatting",
    kind: "custom-endpoint",
    feature: "note-formatting",
  },
  "translation-custom": {
    name: "Translation",
    kind: "custom-endpoint",
    feature: "translation",
  },
  "dictation-agent-custom": {
    name: "Dictation agent",
    kind: "custom-endpoint",
    feature: "dictation-agent",
  },
  "dictation-agent-vision-custom": {
    name: "Dictation agent (vision)",
    kind: "custom-endpoint",
    feature: "dictation-agent-vision",
  },
  "chat-agent-custom": {
    name: "Chat agent",
    kind: "custom-endpoint",
    feature: "chat-agent",
  },
  // One slot each, holding the whole multi-field configuration as JSON. Not on
  // the Providers screen, which stores a single secret per slot -- these are
  // configured on Settings > Intelligence > Enterprise, which has the fields.
  bedrock: { name: "AWS Bedrock", kind: "enterprise", feature: "bedrock" },
  azure: { name: "Azure OpenAI", kind: "enterprise", feature: "azure" },
  vertex: { name: "GCP Vertex AI", kind: "enterprise", feature: "vertex" },
  // Meeting-streaming providers whose upstream client minted a realtime token
  // from the vendor's hosted token-broker rather than asking the user for a
  // key -- see OWN_CREDENTIAL_SLOTS below. A plain BYOK slot each, same shape
  // as any other `provider` credential.
  assemblyai: { name: "AssemblyAI", kind: "provider" },
  deepgram: { name: "Deepgram", kind: "provider" },
};

/**
 * Slots this project needs that upstream's `secretKeys` does not have.
 *
 * Two different reasons, in one list because both are "ours, not upstream's":
 *
 * - `bedrock`/`azure`/`vertex` — upstream stored enterprise credentials as
 *   loose settings fields, an AWS secret access key in the same store as a
 *   window position. Here each connector gets one credential-store slot
 *   holding its whole configuration as JSON, because splitting it would put
 *   the secret half in the keychain and the region/deployment half in
 *   SQLite, and a struct reassembled from two stores is a struct that gets
 *   reassembled wrongly.
 *
 * - `assemblyai`/`deepgram` — upstream's meeting client never asked the user
 *   for either key. It minted a short-lived realtime token from the vendor's
 *   hosted token-broker (recorded as `vendor:openwhispr-realtime-token-broker`,
 *   `removed-vendor-service`, in the integration manifest), so
 *   `baseline.secretKeys` has no entry for either provider. A direct client
 *   needs its own key, and this is where it is stored.
 *
 * Listed separately from `CREDENTIAL_SLOTS` so the distinction stays visible:
 * everything above came from upstream, everything here is ours.
 */
const OWN_CREDENTIAL_SLOTS = [
  { base: "bedrock", env: "AWS_BEDROCK_CONFIG" },
  { base: "azure", env: "AZURE_OPENAI_CONFIG" },
  { base: "vertex", env: "GCP_VERTEX_CONFIG" },
  { base: "assemblyai", env: "ASSEMBLYAI_API_KEY" },
  { base: "deepgram", env: "DEEPGRAM_API_KEY" },
];

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

/**
 * Measured byte counts for the local-LLM GGUFs, replacing upstream's declared
 * ones.
 *
 * Same reasoning as `SHERPA_PINS` below -- project-supplied data with nowhere
 * else to live, because the baseline is a frozen record of what upstream had and
 * must not gain fields we invented. The difference is that here the field
 * already exists and upstream's value for it is *wrong*, so this table overrides
 * rather than adds.
 *
 * **What was wrong.** Upstream derived `sizeBytes` from its own `size` label --
 * a base-10 "GB" figure -- by multiplying by 2^30 instead of 10^9. Eighteen of
 * the thirty-five models were therefore 6.7-7.9% larger than the file Hugging
 * Face serves, and `download_gguf` deleted any finished download more than 5%
 * from the declared size. Those eighteen could not be installed at all: 101 GB
 * of successful downloads, thrown away on arrival. Over the eighteen,
 * declared / measured averages 1.0738 and 2^30 / 10^9 is 1.073741824. The full
 * arithmetic is in `docs/ENGINEERING_LOG.md`, 2026-08-31.
 *
 * **These are measurements, taken on 2026-08-31**, from the `x-linked-size`
 * response header on an HTTP HEAD to
 * `https://huggingface.co/{hfRepo}/resolve/main/{fileName}`. Hugging Face serves
 * these through LFS, so that header carries the true object size while
 * `content-length` describes the 302 that points at it. Four were cross-checked
 * against `/api/models/{hfRepo}/tree/main`, which reports the same `lfs.size`.
 *
 * **They are not bytes on disk after a download**, which is the stronger
 * standard `tools/sidecar-pin/pin.mjs` holds itself to and the reason that tool
 * can also pin a SHA-256. The difference is a cost decision, not a preference:
 * these 39 objects total 202 GB, and downloading all of them to confirm a length
 * the publisher already states is not justified for a size pin. So read this
 * table as "Hugging Face says the object is this long", never as "these bytes
 * were seen" -- and do not add a `sha256` here on the strength of it. A digest
 * belongs to bytes somebody actually hashed.
 *
 * A model here is held to its size *exactly* rather than within 5%; see
 * `download_gguf` in `apps/desktop/src-tauri/src/local_llm.rs`. A model absent
 * from this table keeps upstream's declared figure and the tolerance check, so
 * adding a model without measuring it is degraded rather than broken.
 *
 * Never hand-write an entry: re-measure. The `#[ignore]`d
 * `the_measured_sizes_still_match_hugging_face` test in `local_llm.rs` is that
 * re-measurement, and it is what turns a silent upstream re-upload into a
 * failure somebody sees.
 */
const LOCAL_LLM_SIZES = {
  "qwen3.5-9b-q4_k_m": { sizeBytes: 6_169_341_984 },
  "qwen3.5-4b-q4_k_m": { sizeBytes: 3_013_027_808 },
  "qwen3.5-2b-q4_k_m": { sizeBytes: 1_396_198_496 },
  "qwen3-8b-q4_k_m": { sizeBytes: 5_027_783_488 },
  "qwen3-8b-q5_k_m": { sizeBytes: 5_851_112_224 },
  "qwen3-4b-q4_k_m": { sizeBytes: 2_497_280_256 },
  "qwen3-1.7b-q8_0": { sizeBytes: 1_834_426_016 },
  "qwen3-32b-q4_k_m": { sizeBytes: 19_762_149_024 },
  "qwen2.5-1.5b-instruct-q5_k_m": { sizeBytes: 1_285_494_304 },
  "qwen2.5-3b-instruct-q5_k_m": { sizeBytes: 2_438_740_384 },
  "qwen2.5-7b-instruct-q4_k_m": { sizeBytes: 4_683_074_240 },
  "qwen2.5-7b-instruct-q5_k_m": { sizeBytes: 5_444_831_936 },
  "mistral-nemo-12b-instruct-q4_k_m": { sizeBytes: 7_477_208_320 },
  "mistral-7b-instruct-v0.3-q4_k_m": { sizeBytes: 4_372_812_000 },
  "mistral-7b-instruct-v0.3-q5_k_m": { sizeBytes: 5_136_175_328 },
  "llama-3.2-1b-instruct-q4_k_m": { sizeBytes: 807_694_464 },
  "llama-3.2-3b-instruct-q4_k_m": { sizeBytes: 2_019_377_696 },
  "llama-3.1-8b-instruct-q4_k_m": { sizeBytes: 4_920_739_232 },
  "gpt-oss-20b-mxfp4": { sizeBytes: 12_109_566_624 },
  "gemma-4-31b-it-q4_k_m": { sizeBytes: 19_598_489_952 },
  "gemma-4-31b-it-qat-q4_0": {
    sizeBytes: 17_651_001_568,
    draftSizeBytes: 279_955_968,
  },
  "gemma-4-26b-a4b-it-q4_k_m": { sizeBytes: 17_035_039_872 },
  "gemma-4-26b-a4b-it-qat-q4_0": {
    sizeBytes: 14_439_363_584,
    draftSizeBytes: 251_939_328,
  },
  "gemma-4-e4b-it-q4_k_m": { sizeBytes: 5_405_170_144 },
  "gemma-4-e4b-it-qat-q4_0": {
    sizeBytes: 5_154_941_280,
    draftSizeBytes: 59_678_016,
  },
  "gemma-4-e2b-it-q4_k_m": { sizeBytes: 3_462_680_032 },
  "gemma-4-e2b-it-qat-q4_0": {
    sizeBytes: 3_349_516_256,
    draftSizeBytes: 59_235_648,
  },
  "gemma-3-12b-it-q4_k_m": { sizeBytes: 7_300_575_264 },
  "gemma-3-4b-it-q4_k_m": { sizeBytes: 2_489_758_112 },
  "gemma-3-1b-it-q4_k_m": { sizeBytes: 806_058_496 },
  "lfm2.5-1.2b-instruct-q4_k_m": { sizeBytes: 730_895_168 },
  "lfm2.5-8b-a1b-q4_k_m": { sizeBytes: 5_155_564_768 },
  "lfm2-2.6b-q4_k_m": { sizeBytes: 1_563_668_704 },
  "lfm2.5-350m-q8_0": { sizeBytes: 379_217_632 },
  "lfm2.5-230m-q8_0": { sizeBytes: 246_598_496 },
};

const localLlm = R.localProviders.map((p) => ({
  id: p.id,
  name: p.name,
  baseUrl: p.baseUrl ?? null,
  models: p.models.map((m) => {
    const measured = LOCAL_LLM_SIZES[m.id];
    return {
      id: m.id,
      name: m.name,
      fileName: m.fileName,
      hfRepo: m.hfRepo,
      sizeBytes: measured?.sizeBytes ?? m.sizeBytes ?? null,
      // Whether that number was measured against the publisher or copied from
      // upstream's declaration. The installer reads it to decide between an
      // exact comparison and a 5% tolerance, so it must not be inferred from
      // `sizeBytes` being non-null -- upstream declared a size for every model
      // and was wrong about eighteen of them.
      sizeMeasured: measured?.sizeBytes !== undefined,
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
              sizeBytes: measured?.draftSizeBytes ?? m.draftSizeBytes ?? null,
              sizeMeasured: measured?.draftSizeBytes !== undefined,
            },
    };
  }),
}));

// A pin for a model that does not exist is a typo that would otherwise go
// unnoticed until somebody wondered why the size was never checked exactly --
// the same failure mode SHERPA_PINS guards against, and the same guard.
const LOCAL_LLM_BY_ID = new Map(
  R.localProviders.flatMap((p) => p.models.map((m) => [m.id, m]))
);
for (const [id, pin] of Object.entries(LOCAL_LLM_SIZES)) {
  const model = LOCAL_LLM_BY_ID.get(id);
  if (!model) {
    die(
      `LOCAL_LLM_SIZES has an entry for "${id}", which is not a model in the ` +
        `baseline's localProviders. Fix the id, or remove the pin.`
    );
  }
  if (pin.draftSizeBytes !== undefined && model.draftHfRepo === undefined) {
    die(
      `LOCAL_LLM_SIZES pins a drafter size for "${id}", which has no drafter. ` +
        `A measurement with nothing to check is a measurement of the wrong file.`
    );
  }
}

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

/**
 * Integrity pins for the Parakeet archives, measured rather than declared.
 *
 * Project-supplied data with nowhere else to live. Upstream's baseline carries
 * `expectedSizeBytes` for every *whisper* model, which is why those pass
 * straight through below -- and nothing at all for `parakeetModels`, because
 * upstream never pinned them. The baseline is a frozen record of what upstream
 * had and must not gain fields we invented, so the pins belong here, the same
 * reasoning `OWN_CREDENTIAL_SLOTS` is built on.
 *
 * `f17a5fd` added these three fields straight into `config/provider-registry.json`,
 * which is *generated* by this script -- so running it deleted the security fix
 * and `--check` failed on a clean tree. Putting them here is what makes the pin
 * survive a regeneration.
 *
 * **`sha256` and `expectedSizeBytes` are measurements.** Both byte counts were
 * confirmed against the live release assets' `content-length` on 2026-08-31, and
 * `parakeet-unified`'s also matches the figure `docs/PLATFORM_SUPPORT.md`
 * recorded from a real install. Never hand-write either: re-measure.
 *
 * **`maxExtractedSizeBytes` is a judgement.** It is a decompression-bomb
 * ceiling, not a measurement -- the real extracted footprint of
 * `parakeet-unified` was measured at ~663 MB, and 750 MiB gives that headroom
 * without leaving the limit meaningless. Being wrong high costs protection;
 * being wrong low rejects a legitimate install, so it errs high.
 *
 * A model with no entry here cannot be installed at all:
 * `kw_transcription_core::sherpa_models::install` refuses with `MissingPin`
 * rather than downloading something unauthenticated. That is why the two
 * Nemotron streaming models are absent rather than pinned with placeholders --
 * they are `reachable: false` and nothing can install them yet, and a
 * placeholder digest would be a lie that only failed at the worst moment.
 */
const SHERPA_PINS = {
  "parakeet-tdt-0.6b-v3": {
    sha256: "5793d0fd397c5778d2cf2126994d58e9d56b1be7c04d13c7a15bb1b4eafb16bf",
    expectedSizeBytes: 487_170_055,
    maxExtractedSizeBytes: 800 * 1024 * 1024,
  },
  "parakeet-unified-en-0.6b": {
    sha256: "99f63605b3a85a54c250c0869670a687b7d6598a47bf2421515e1f839a76e150",
    expectedSizeBytes: 501_350_460,
    maxExtractedSizeBytes: 750 * 1024 * 1024,
  },
};

const sherpa = Object.entries(R.parakeetModels).map(([id, m]) => ({
  id,
  name: m.name,
  downloadUrl: m.downloadUrl,
  extractDir: m.extractDir,
  sizeMb: m.sizeMb ?? null,
  // Nulls rather than omitted keys, so the shape does not change between a
  // pinned model and an unpinned one -- the Rust reads `Option` for all three.
  sha256: SHERPA_PINS[id]?.sha256 ?? null,
  expectedSizeBytes: SHERPA_PINS[id]?.expectedSizeBytes ?? null,
  maxExtractedSizeBytes: SHERPA_PINS[id]?.maxExtractedSizeBytes ?? null,
  language: m.language ?? null,
  supportedLanguages: m.supportedLanguages ?? null,
  // "online" means a streaming-capable runtime; "offline" is batch only.
  streaming: m.runtime === "online",
}));

// A pin for a model that does not exist is a typo that would otherwise go
// unnoticed until somebody wondered why verification never ran.
for (const id of Object.keys(SHERPA_PINS)) {
  if (!(id in R.parakeetModels)) {
    die(
      `SHERPA_PINS has an entry for "${id}", which is not a model in the ` +
        `baseline's parakeetModels. Fix the id, or remove the pin.`
    );
  }
}

// --- credentials -------------------------------------------------------------
// Built last, because `usedFor` is derived from the lists above rather than
// restated: a provider that gains transcription support does not also need its
// credential entry edited.
const credentials = [...baseline.secretKeys, ...OWN_CREDENTIAL_SLOTS].map((k) => {
  const declared = CREDENTIAL_SLOTS[k.base];
  if (!declared) {
    die(
      `credential slot "${k.base}" is in the baseline's secretKeys and not in ` +
        `CREDENTIAL_SLOTS. Add it deliberately, with its kind -- defaulting ` +
        `would put a slot in front of the user that nobody decided about.`
    );
  }

  const usedFor = [];
  if (transcription.some((p) => p.credentialSlot === k.base)) {
    usedFor.push("transcription");
  }
  if (reasoning.some((p) => p.credentialSlot === k.base)) {
    usedFor.push("reasoning");
  }

  return {
    slot: k.base,
    name: declared.name,
    kind: declared.kind,
    // Upstream's environment-variable name. Kept because the OpenWhispr
    // importer needs to recognise a key it finds under the old name, not
    // because this application reads the environment.
    env: k.env,
    usedFor,
    ...(declared.feature !== undefined ? { feature: declared.feature } : {}),
  };
});

// Every slot a provider names must be declared. The reverse does not hold --
// six slots belong to no provider on purpose -- but a provider pointing at a
// slot that is not in the list would be unreachable with no way to notice.
for (const p of [...transcription, ...reasoning]) {
  if (!credentials.some((c) => c.slot === p.credentialSlot)) {
    die(
      `provider "${p.id}" authenticates with slot "${p.credentialSlot}", which ` +
        `is not in the credential list. The baseline's secretKeys is the source ` +
        `of that list, so either the slot is missing upstream or the provider ` +
        `id is wrong.`
    );
  }
}

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
      "credential slots come from secretKeys, not from the provider lists, so all fifteen are declared",
      "three enterprise slots are added on top: upstream kept those credentials in loose settings fields",
      "two meeting-provider slots are added on top: upstream minted their realtime tokens from a removed vendor service instead of asking for a key",
    ],
  },
  credentials,
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
  credentials: `${credentials.length} slots (${credentials.filter((c) => c.kind === "provider").length} provider, ${credentials.filter((c) => c.kind === "custom-endpoint").length} custom endpoint, ${credentials.filter((c) => c.kind === "enterprise").length} enterprise)`,
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
