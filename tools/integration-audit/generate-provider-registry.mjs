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
 * Integrity pins for the local-LLM GGUFs: a length *and* a digest for every one
 * of the 39 published objects, plus a per-entry flag saying which of the two
 * standards each digest meets.
 *
 * Same reasoning as `SHERPA_PINS` and `WHISPER_PINS` below for why the table
 * lives here -- the baseline is a frozen record of what upstream had and must
 * not gain fields we invented, and `config/provider-registry.json` is
 * generated, so a pin hand-edited into it dies at the next regeneration. The
 * difference is that `sizeBytes` already existed and upstream's value for it was
 * *wrong*, so this table both overrides and adds.
 *
 * **What was wrong about the sizes.** Upstream derived `sizeBytes` from its own
 * `size` label -- a base-10 "GB" figure -- by multiplying by 2^30 instead of
 * 10^9. Eighteen of the thirty-five models were therefore 6.7-7.9% larger than
 * the file Hugging Face serves, and `download_gguf` deleted any finished
 * download more than 5% from the declared size. Those eighteen could not be
 * installed at all: 101 GB of successful downloads, thrown away on arrival.
 * Over the eighteen, declared / measured averages 1.0738 and 2^30 / 10^9 is
 * 1.073741824. The full arithmetic is in `docs/ENGINEERING_LOG.md`, 2026-08-31.
 *
 * ## Two standards, and every row says which one it meets
 *
 * `sha256Measured: true` -- and `draftSha256Measured` for a drafter -- means the
 * object was downloaded in full and hashed by this project. **Seven objects
 * carry it, one from each publishing organisation in the table.** Every other
 * row is the publisher's *stated* digest, which is not the same claim and must
 * never be read as one.
 *
 * **A stated digest is `lfs.oid`**, from
 * `https://huggingface.co/api/models/{hfRepo}/tree/main?expand=true&recursive=true`.
 * (`expand=true` is what adds the `lfs` block; `recursive=true` is what finds an
 * object a publisher put in a subdirectory, which several do for multi-part
 * quantisations.) It is a
 * true SHA-256 of the object rather than a checksum of convenience: git-lfs is
 * content-addressed on exactly that hash, so the pointer file committed in the
 * repository *is* the digest. One call per repository returns every object's oid
 * and length together, which is why a 39-row table cost 36 requests rather than
 * the 202 GB of downloading that kept it from existing.
 *
 * **What a stated digest defends against.** It turns "any file of 6 169 341 984
 * bytes passes" into "only this content passes" -- so a corrupted transfer, a
 * truncation the length happened to survive, and an accidental re-upload all
 * fail where they used to install. That is the whole of KNEG-45's complaint and
 * it is worth having.
 *
 * **What it does not defend against: a compromised huggingface.co**, because one
 * host would serve both the bytes and the digest they are checked against. A
 * digest measured once and committed to *this* repository does, and that is why
 * `config/sidecars.json`'s rule is "a pin is only a pin if it was measured" --
 * and why this table carries a flag rather than a comment implying all 39 rows
 * are equal.
 *
 * **On the prohibition this replaces.** The previous version of this comment
 * said: "do not add a `sha256` here on the strength of it. A digest belongs to
 * bytes somebody actually hashed." It was written when the only alternative on
 * offer was deriving a digest from a single download, which pins whatever
 * arrived while looking like a guarantee. It still forbids that. An oid is a
 * different object -- not derived from a download at all, stated by the
 * publisher before any transfer, and therefore testable against one.
 *
 * **It was tested.** On 2026-08-31 the smallest object from each of the seven
 * organisations here -- bartowski, Qwen, mradermacher, ggml-org, google, unsloth
 * and LiquidAI, 23.6 GiB in total -- was downloaded in full by
 * `tools/model-pin/pin.mjs`, hashed on the way to disk, and its length taken
 * from `stat`. All seven digests and all seven lengths matched the stated ones
 * exactly. `ggerganov/whisper.cpp` had already agreed six-for-six over 6.9 GiB
 * when `WHISPER_PINS` was measured, which is where the lead came from. Thirteen
 * objects, eight organisations, no disagreement. That is evidence the metadata
 * is *accurate*; it is not evidence it is *trustworthy under attack*, which is
 * the paragraph above.
 *
 * ## The lengths
 *
 * `sizeBytes` came from the `x-linked-size` response header on an HTTP HEAD to
 * `https://huggingface.co/{hfRepo}/resolve/main/{fileName}` on 2026-08-31.
 * Hugging Face serves these through LFS, so that header carries the true object
 * size while `content-length` describes the 302 that points at it. All 39 were
 * re-read as `lfs.size` from the tree API above in the same pass that produced
 * the digests, and all 39 agreed -- one instrument confirming another, not an
 * independent measurement, since both are Hugging Face's own metadata. Seven are
 * now bytes on disk, from the run described above.
 *
 * A model here is held to its size *exactly* rather than within 5%, and to its
 * digest unconditionally; see `download_gguf` in
 * `apps/desktop/src-tauri/src/local_llm.rs`. A model absent from this table
 * keeps upstream's declared figure, the tolerance check, and no digest at all,
 * so adding a model without measuring it is degraded rather than broken.
 *
 * Never hand-write an entry: re-measure. Two `#[ignore]`d tests in
 * `local_llm.rs` are that re-measurement --
 * `the_measured_sizes_still_match_hugging_face` for the lengths and
 * `the_stated_digests_still_match_hugging_face` for the digests -- and
 * `node tools/model-pin/pin.mjs --verify-stated`, which the weekly
 * `pin-verify.yml` runs, is the same check outside the Rust test harness. Any of
 * the three turns a silent upstream re-upload into a failure somebody sees.
 */
const LOCAL_LLM_PINS = {
  "qwen3.5-9b-q4_k_m": {
    sizeBytes: 6_169_341_984,
    sha256: "d784ce9eda1a5a7b51e8f705a9e6310844bf4f173654d115823c775fdea56d43",
  },
  "qwen3.5-4b-q4_k_m": {
    sizeBytes: 3_013_027_808,
    sha256: "13c16f426047e2de38cd075bdade4a7bcbc8c774384876f677740cda65f8a983",
  },
  "qwen3.5-2b-q4_k_m": {
    sizeBytes: 1_396_198_496,
    sha256: "57a1085840f497d764a7fc5d346922dbde961efb54cc792ea81d694fd846a1d8",
  },
  "qwen3-8b-q4_k_m": {
    sizeBytes: 5_027_783_488,
    sha256: "d98cdcbd03e17ce47681435b5150e34c1417f50b5c0019dd560e4882c5745785",
  },
  "qwen3-8b-q5_k_m": {
    sizeBytes: 5_851_112_224,
    sha256: "068bae163faa96ad48032daf4e071a6a28fe67d8dcc95367609c2ff165e52738",
  },
  "qwen3-4b-q4_k_m": {
    sizeBytes: 2_497_280_256,
    sha256: "7485fe6f11af29433bc51cab58009521f205840f5b4ae3a32fa7f92e8534fdf5",
  },
  "qwen3-1.7b-q8_0": {
    sizeBytes: 1_834_426_016,
    sha256: "061b54daade076b5d3362dac252678d17da8c68f07560be70818cace6590cb1a",
  },
  "qwen3-32b-q4_k_m": {
    sizeBytes: 19_762_149_024,
    sha256: "efd971561896866f0e910cce52761ca77b1b138090c7f15fe284676d57d1f689",
  },
  "qwen2.5-1.5b-instruct-q5_k_m": {
    sizeBytes: 1_285_494_304,
    sha256: "b46661073c18e5b56a41fa320975f866a00def1ff08feef4718e013258896f8c",
    sha256Measured: true,
  },
  "qwen2.5-3b-instruct-q5_k_m": {
    sizeBytes: 2_438_740_384,
    sha256: "2c63dde5f2c9ab1fd64d47dee2d34dade6ba9ff62442d1d20b5342310c982081",
  },
  "qwen2.5-7b-instruct-q4_k_m": {
    sizeBytes: 4_683_074_240,
    sha256: "65b8fcd92af6b4fefa935c625d1ac27ea29dcb6ee14589c55a8f115ceaaa1423",
  },
  "qwen2.5-7b-instruct-q5_k_m": {
    sizeBytes: 5_444_831_936,
    sha256: "2e998d7e181c8756c5ffc55231b9ee1cdc9d3acec4245d6e27d32bd8e738c474",
  },
  "mistral-nemo-12b-instruct-q4_k_m": {
    sizeBytes: 7_477_208_320,
    sha256: "2046c16c427f5bd6dfb2b08b6b74edcc6102e0e2129cebff38ef7aa746d26f31",
    sha256Measured: true,
  },
  "mistral-7b-instruct-v0.3-q4_k_m": {
    sizeBytes: 4_372_812_000,
    sha256: "1270d22c0fbb3d092fb725d4d96c457b7b687a5f5a715abe1e818da303e562b6",
  },
  "mistral-7b-instruct-v0.3-q5_k_m": {
    sizeBytes: 5_136_175_328,
    sha256: "0374072546aaf1257d1e34610edb11b691730d8ef7d44b5629693a2dd8e8d472",
  },
  "llama-3.2-1b-instruct-q4_k_m": {
    sizeBytes: 807_694_464,
    sha256: "6f85a640a97cf2bf5b8e764087b1e83da0fdb51d7c9fab7d0fece9385611df83",
  },
  "llama-3.2-3b-instruct-q4_k_m": {
    sizeBytes: 2_019_377_696,
    sha256: "6c1a2b41161032677be168d354123594c0e6e67d2b9227c84f296ad037c728ff",
  },
  "llama-3.1-8b-instruct-q4_k_m": {
    sizeBytes: 4_920_739_232,
    sha256: "7b064f5842bf9532c91456deda288a1b672397a54fa729aa665952863033557c",
  },
  "gpt-oss-20b-mxfp4": {
    sizeBytes: 12_109_566_624,
    sha256: "27cd6c432c7672cb812a92f611cf3ba7bbc35928262bb1e1253ff4ee6ae35901",
    sha256Measured: true,
  },
  "gemma-4-31b-it-q4_k_m": {
    sizeBytes: 19_598_489_952,
    sha256: "2959c1b0a4f792493bb0092846ea06aec54547e47be979c282634d938d501b75",
  },
  "gemma-4-31b-it-qat-q4_0": {
    sizeBytes: 17_651_001_568,
    sha256: "179cfb99212709597eae5929112cfca677e1bbf566178b479ae1da0c4772874b",
    draftSizeBytes: 279_955_968,
    draftSha256: "3a5e99fd8d0b23afb1fccd1ee0c9ebd1f571d00399c2dae2292d217feeec0f6b",
  },
  "gemma-4-26b-a4b-it-q4_k_m": {
    sizeBytes: 17_035_039_872,
    sha256: "a07f72221e8e3f77455ab0d7f7652d01a9f63c262b954aa6932a53275a0e895a",
  },
  "gemma-4-26b-a4b-it-qat-q4_0": {
    sizeBytes: 14_439_363_584,
    sha256: "3eca3b8f6d7baf218a7dd6bba5fb59a56ee25fe2d567b6f5f589b4f697eca51d",
    draftSizeBytes: 251_939_328,
    draftSha256: "7272d97595f0d4c74bd7b623492b7dbdaafd8b7c72f329a8270ba4eca68f768a",
  },
  "gemma-4-e4b-it-q4_k_m": {
    sizeBytes: 5_405_170_144,
    sha256: "d35a3aa7a6d47de237fd488bb6f7f3efd62908d828f85a63da0ee93acdbefe9f",
  },
  "gemma-4-e4b-it-qat-q4_0": {
    sizeBytes: 5_154_941_280,
    sha256: "676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee",
    draftSizeBytes: 59_678_016,
    draftSha256: "423074e537504b4f9ec5eafed5c639fac82c96631626efccacdd3c4039b20605",
  },
  "gemma-4-e2b-it-q4_k_m": {
    sizeBytes: 3_462_680_032,
    sha256: "923c4c86177d2ee173a7f5b4fa3d0ac65f5962ab15e6d6a5bc250aec4fd7bf7e",
  },
  "gemma-4-e2b-it-qat-q4_0": {
    sizeBytes: 3_349_516_256,
    sha256: "fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634",
    sha256Measured: true,
    draftSizeBytes: 59_235_648,
    draftSha256: "586f2460b909008640981ec34060aa864e03c144fbabfb3173c4335087e4aae0",
    draftSha256Measured: true,
  },
  "gemma-3-12b-it-q4_k_m": {
    sizeBytes: 7_300_575_264,
    sha256: "fc57f67efa46d711c346e587cbef7d049e95f3df8db2eb2271153343ef0acc7b",
  },
  "gemma-3-4b-it-q4_k_m": {
    sizeBytes: 2_489_758_112,
    sha256: "4996030242583a40aa151ff93f49ed787ac8c25e4120c3ae4588b2e2a7d1ae94",
  },
  "gemma-3-1b-it-q4_k_m": {
    sizeBytes: 806_058_496,
    sha256: "12bf0fff8815d5f73a3c9b586bd8fee8e7b248c935de70dec367679873d0f29d",
    sha256Measured: true,
  },
  "lfm2.5-1.2b-instruct-q4_k_m": {
    sizeBytes: 730_895_168,
    sha256: "b1b3de114215d9507409a662a501a631095a479a419584e8a2ded6304b19b4f5",
  },
  "lfm2.5-8b-a1b-q4_k_m": {
    sizeBytes: 5_155_564_768,
    sha256: "4923ec14f06b968b74d663e5949867d2d9c3bf13a20b8be1a9f9af39989b2bb0",
  },
  "lfm2-2.6b-q4_k_m": {
    sizeBytes: 1_563_668_704,
    sha256: "384bc877b6c37064982f96885bef69e4475919f5969218ed4e3b9399ae0340df",
  },
  "lfm2.5-350m-q8_0": {
    sizeBytes: 379_217_632,
    sha256: "be036a757295e550098b85e13f6af2735d0fa73b41e1156a40c7d8e8e32a5766",
  },
  "lfm2.5-230m-q8_0": {
    sizeBytes: 246_598_496,
    sha256: "855be85429300602eda72958547614703541b7d6dd965a8f8f6052b85a7aa935",
    sha256Measured: true,
  },
};

const localLlm = R.localProviders.map((p) => ({
  id: p.id,
  name: p.name,
  baseUrl: p.baseUrl ?? null,
  models: p.models.map((m) => {
    const pin = LOCAL_LLM_PINS[m.id];
    return {
      id: m.id,
      name: m.name,
      fileName: m.fileName,
      hfRepo: m.hfRepo,
      sizeBytes: pin?.sizeBytes ?? m.sizeBytes ?? null,
      // Whether that number was measured against the publisher or copied from
      // upstream's declaration. The installer reads it to decide between an
      // exact comparison and a 5% tolerance, so it must not be inferred from
      // `sizeBytes` being non-null -- upstream declared a size for every model
      // and was wrong about eighteen of them.
      sizeMeasured: pin?.sizeBytes !== undefined,
      sha256: pin?.sha256 ?? null,
      // Whether that digest is bytes this project hashed or the publisher's
      // stated `lfs.oid`. Both are held to exact equality after a download, so
      // this changes nothing about the check -- it changes what may honestly be
      // *claimed* about a model that passed, which is the screen's problem and
      // not the installer's. Seven of the thirty-nine objects are measured;
      // `LOCAL_LLM_PINS` says why the distinction is not cosmetic.
      sha256Measured: pin?.sha256Measured === true,
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
              sizeBytes: pin?.draftSizeBytes ?? m.draftSizeBytes ?? null,
              sizeMeasured: pin?.draftSizeBytes !== undefined,
              sha256: pin?.draftSha256 ?? null,
              sha256Measured: pin?.draftSha256Measured === true,
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
for (const [id, pin] of Object.entries(LOCAL_LLM_PINS)) {
  const model = LOCAL_LLM_BY_ID.get(id);
  if (!model) {
    die(
      `LOCAL_LLM_PINS has an entry for "${id}", which is not a model in the ` +
        `baseline's localProviders. Fix the id, or remove the pin.`
    );
  }
  for (const [field, value] of Object.entries(pin)) {
    if (field.startsWith("draft") && model.draftHfRepo === undefined) {
      die(
        `LOCAL_LLM_PINS pins "${field}" for "${id}", which has no drafter. ` +
          `A measurement with nothing to check is a measurement of the wrong file.`
      );
    }
    // A digest is 64 lowercase hex characters or it is a typo, and a typo here
    // fails every install of that model with a message about corruption. The
    // shape is cheap to check here and expensive to diagnose in the field.
    if (/^(draft)?[Ss]ha256$/.test(field) && !/^[0-9a-f]{64}$/.test(value)) {
      die(
        `LOCAL_LLM_PINS."${id}".${field} is not 64 lowercase hex characters. ` +
          `Re-measure it; do not hand-correct it.`
      );
    }
    // A `Measured` flag with no digest to qualify claims the stronger standard
    // for nothing. The generator cannot produce that pair; a hand-edit can.
    if (field.endsWith("Sha256Measured") || field === "sha256Measured") {
      const digest = field === "sha256Measured" ? pin.sha256 : pin.draftSha256;
      if (value === true && digest === undefined) {
        die(
          `LOCAL_LLM_PINS."${id}".${field} is true, but there is no digest for ` +
            `it to describe.`
        );
      }
    }
  }
}

// --- local ASR ---------------------------------------------------------------

/**
 * Integrity pins for the six whisper models: a measured digest and the exact
 * byte count that goes with it.
 *
 * Here rather than in Rust, and here rather than in the baseline, for the
 * reasons `SHERPA_PINS` and `LOCAL_LLM_PINS` give: the baseline is a frozen
 * record of what upstream had and must not gain fields we invented, and
 * `config/provider-registry.json` is generated, so a pin hand-edited into it
 * dies at the next regeneration. What is new is the third case --
 * `crates/transcription-core/src/models.rs` used to keep a `PINNED` table of
 * its own. Three model families with pins in three places was incoherent
 * enough that `tools/model-pin/pin.mjs` had to **parse Rust** to verify one of
 * them. It reads this file's output now, as JSON, and `models.rs` reads
 * `sha256` off `kw_provider_registry::WhisperModel`.
 *
 * **These are bytes on disk.** On 2026-08-31 `tools/model-pin/pin.mjs`
 * downloaded all six in full -- 6,966,596,962 bytes, 6.49 GiB, in one run --
 * hashing each stream as it passed through to a file and taking the length from
 * `stat` afterwards, never from `content-length`, because a truncated body
 * agrees with its own header. That is the standard `tools/sidecar-pin/pin.mjs`
 * holds and the standard
 * `LOCAL_LLM_PINS` above holds for only seven of its thirty-nine objects; the
 * other thirty-two are the publisher's stated `lfs.oid` and stated object size,
 * held to exact equality but never hashed here. That table carries a per-row
 * `sha256Measured` flag for exactly this reason. This one needs no flag,
 * because all six of its rows are bytes on disk. Do not read one table as the
 * other.
 *
 * `tiny` is the control. Its digest had been committed in `models.rs` since
 * that file was written, from a download nobody had repeated; the tool
 * reproduced `be07e048…c6e1b21` and 77,691,713 bytes exactly, on a different
 * day through different code. That validates the measurement of the other five
 * at the same time as it re-validates the pin.
 *
 * **`expectedSizeBytes` here overrides the baseline's figure**, which is
 * rounded -- 78,000,000 for a file that is 77,691,713 bytes. That was free to
 * change: the field has exactly one reader, `models::integrity_for`, and the
 * settings screen renders `sizeMb` instead (see `SettingsTranscription.tsx`).
 * `models.rs` used to claim otherwise; the claim was wrong and is corrected
 * there. `sizeMb` is left rounded, because it is a display estimate and
 * displaying 77,691,713 bytes to a user helps nobody.
 *
 * **What this costs.** Every URL here is `resolve/main` on Hugging Face -- a
 * branch pointer, not content. Until now a re-upload had to move a file by more
 * than 5% to be noticed; now any change at all fails the install, loudly, and
 * the user cannot proceed. That is the same trade `LOCAL_LLM_PINS` took and it
 * is taken again deliberately: a silent pass means running weights nobody here
 * measured. `tools/model-pin/pin.mjs --verify` is what is meant to notice
 * first, and `.github/workflows/pin-verify.yml` runs it weekly. See KNEG-103.
 *
 * A model with no entry here keeps upstream's rounded figure, gets no digest,
 * and `integrity_for` degrades it to a size check with a 5% tolerance rather
 * than refusing to install it -- unlike a sherpa model, which cannot be
 * installed unpinned at all. The difference is deliberate: a whisper model is a
 * single file from a repository whose other five files were hashed, and a
 * sherpa model is an archive that gets extracted.
 *
 * Never hand-write an entry. Both numbers come out of
 * `node tools/model-pin/pin.mjs --url <downloadUrl> --id <id>`.
 */
const WHISPER_PINS = {
  tiny: {
    sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
    expectedSizeBytes: 77_691_713,
  },
  base: {
    sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
    expectedSizeBytes: 147_951_465,
  },
  small: {
    sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
    expectedSizeBytes: 487_601_967,
  },
  medium: {
    sha256: "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208",
    expectedSizeBytes: 1_533_763_059,
  },
  turbo: {
    sha256: "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69",
    expectedSizeBytes: 1_624_555_275,
  },
  large: {
    sha256: "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2",
    expectedSizeBytes: 3_095_033_483,
  },
};

// Same guard SHERPA_PINS and LOCAL_LLM_PINS carry, plus one they do not need:
// half a pin. `models::integrity_for` needs the digest and the length together
// -- `kw_sidecar::integrity::verify` takes both -- and a digest arriving without
// a length degrades silently to a size check on a model the screen would still
// be describing as digest-verified.
for (const [id, pin] of Object.entries(WHISPER_PINS)) {
  if (!(id in R.whisperModels)) {
    die(
      `WHISPER_PINS has an entry for "${id}", which is not a model in the ` +
        `baseline's whisperModels. Fix the id, or remove the pin.`
    );
  }
  if (!/^[0-9a-f]{64}$/.test(pin.sha256)) {
    die(
      `WHISPER_PINS' digest for "${id}" is not 64 lowercase hex characters. ` +
        `Sha256Digest::parse rejects it, so integrity_for would quietly fall ` +
        `back to a size check. Measure it with tools/model-pin/pin.mjs.`
    );
  }
  if (!Number.isInteger(pin.expectedSizeBytes) || pin.expectedSizeBytes <= 0) {
    die(
      `WHISPER_PINS pins a digest for "${id}" with no usable expectedSizeBytes. ` +
        `Both numbers are measured in the same run; do not commit one without ` +
        `the other.`
    );
  }
}

const whisper = Object.entries(R.whisperModels).map(([id, m]) => ({
  id,
  name: m.name,
  fileName: m.fileName,
  downloadUrl: m.downloadUrl,
  // The rounded display estimate. Deliberately left as upstream had it.
  sizeMb: m.sizeMb ?? null,
  sha256: WHISPER_PINS[id]?.sha256 ?? null,
  // The measured length where there is a pin, upstream's rounded declaration
  // where there is not. One or the other, and the pin always wins -- a digest
  // beside a rounded length would fail every install.
  expectedSizeBytes: WHISPER_PINS[id]?.expectedSizeBytes ?? m.expectedSizeBytes ?? null,
  recommended: m.recommended === true,
}));

/**
 * Integrity pins for the Parakeet archives, measured rather than declared.
 *
 * Project-supplied data with nowhere else to live. Upstream's baseline carries a
 * rounded `expectedSizeBytes` for every *whisper* model and nothing at all for
 * `parakeetModels`, because upstream never pinned them. The baseline is a frozen
 * record of what upstream had and must not gain fields we invented, so the pins
 * belong here, the same reasoning `OWN_CREDENTIAL_SLOTS` is built on -- and the
 * same reasoning `WHISPER_PINS` above now follows, which is why the whisper
 * figures no longer pass straight through either.
 *
 * `f17a5fd` added these three fields straight into `config/provider-registry.json`,
 * which is *generated* by this script -- so running it deleted the security fix
 * and `--check` failed on a clean tree. Putting them here is what makes the pin
 * survive a regeneration.
 *
 * **`sha256` and `expectedSizeBytes` are measurements, and both were reproduced
 * by a full download.** On 2026-08-31 `tools/model-pin/pin.mjs` fetched both
 * archives end to end -- 464.6 MiB and 478.1 MiB -- hashing as the bytes passed
 * through and taking the length from `stat` afterwards rather than from
 * `content-length`, and both digests and both byte counts came back identical to
 * what is committed below. `parakeet-unified`'s size also matches the figure
 * `docs/PLATFORM_SUPPORT.md` recorded from a real install.
 *
 * Read that for exactly what it is: **the committed digest matches the bytes the
 * host serves today.** It is not evidence about who built those bytes, and it
 * says nothing about tomorrow. Both URLs name an asset inside k2-fsa's rolling
 * `asr-models` release, which the publisher keeps adding to, and an asset can be
 * replaced under the same name -- so this is the check that would *notice* a
 * swap, run once, not a guarantee there will never be one. Never hand-write
 * either number: re-measure with `node tools/model-pin/pin.mjs --verify`.
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
