/**
 * GENERATED FILE — DO NOT EDIT.
 *
 * Source:    config/provider-registry.json
 * Generator: tools/integration-audit/generate-ts-types.mjs
 * Baseline:  c4000db9802d696341f1d239c1d81dc6d1ee233e
 *
 * Regenerate with:
 *   node tools/integration-audit/generate-ts-types.mjs
 *
 * The Rust side reads the same JSON through crates/provider-registry, so a
 * provider or model id is declared exactly once for the whole project.
 */


/** Transcription providers, in registry order. */
export const TRANSCRIPTION_PROVIDER_IDS = ["openai", "groq", "berget", "xai", "mistral", "corti", "tinfoil"] as const;
export type TranscriptionProviderId = (typeof TRANSCRIPTION_PROVIDER_IDS)[number];

/** Cloud reasoning providers, in registry order. */
export const REASONING_PROVIDER_IDS = ["openai", "anthropic", "gemini", "groq", "tinfoil", "berget", "corti"] as const;
export type ReasoningProviderId = (typeof REASONING_PROVIDER_IDS)[number];

/** Enterprise connectors, configured with organisation credentials. */
export const ENTERPRISE_PROVIDER_IDS = ["bedrock", "azure", "vertex"] as const;
export type EnterpriseProviderId = (typeof ENTERPRISE_PROVIDER_IDS)[number];

/** Local llama.cpp model families. */
export const LOCAL_LLM_PROVIDER_IDS = ["qwen", "mistral", "llama", "openai-oss", "gemma", "liquidai"] as const;
export type LocalLlmProviderId = (typeof LOCAL_LLM_PROVIDER_IDS)[number];

/** Local whisper.cpp models. */
export const WHISPER_MODEL_IDS = ["base", "large", "medium", "small", "tiny", "turbo", "kb-whisper-tiny", "kb-whisper-base"] as const;
export type WhisperModelId = (typeof WHISPER_MODEL_IDS)[number];

/** Local sherpa-onnx models (Parakeet batch, Nemotron streaming). */
export const SHERPA_MODEL_IDS = ["nemotron-3.5-asr-streaming-0.6b", "nemotron-speech-streaming-en-0.6b", "parakeet-tdt-0.6b-v3", "parakeet-unified-en-0.6b"] as const;
export type SherpaModelId = (typeof SHERPA_MODEL_IDS)[number];

/** The two models a speaker-diarization run needs, one of each role. */
export const DIARIZATION_MODEL_IDS = ["3dspeaker-campplus-en", "sherpa-onnx-pyannote-segmentation-3-0"] as const;
export type DiarizationModelId = (typeof DIARIZATION_MODEL_IDS)[number];

/** OS credential-store slots, in registry order. Twenty: twelve providers, five per-feature custom endpoints and three enterprise connectors. */
export const CREDENTIAL_SLOTS = ["openai", "anthropic", "gemini", "groq", "berget", "xai", "mistral", "openrouter", "tinfoil", "corti", "note-formatting-custom", "translation-custom", "dictation-agent-custom", "dictation-agent-vision-custom", "chat-agent-custom", "bedrock", "azure", "vertex", "assemblyai", "deepgram", "custom"] as const;
export type CredentialSlot = (typeof CREDENTIAL_SLOTS)[number];

export type AuthScheme =
  | "bearer"
  | "azure-api-key"
  | "oauth2-client-credentials"
  | "attested"
  | "none";

/** What a credential slot is for. Mirrors `provider_registry::CredentialKind`. */
export type CredentialKind = "provider" | "custom-endpoint" | "enterprise";

/**
 * One slot in the OS credential store. Mirrors
 * `provider_registry::CredentialSlot`.
 *
 * `kind` is the distinction a screen has to make. A `provider` key is enough on
 * its own to try that service; an `enterprise` slot holds a whole multi-field
 * configuration as JSON and is set on its own screen; a `custom-endpoint` key
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
  /** `transcription`, `reasoning`, or neither. Derived, never restated. */
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
  /** `null` means genuinely uncapped. Explicit on every provider. */
  readonly maxUploadBytes: number | null;
  readonly batchModel?: string;
  readonly models: readonly TranscriptionModel[];
}

export interface ReasoningModel {
  readonly id: string;
  readonly name: string;
  /** `max_tokens` or `max_completion_tokens`; providers disagree. */
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
  /** On the same terms as `LocalLlmModel.revision`. */
  readonly revision: string;
  readonly fileName: string;
  readonly sizeBytes: number | null;
  readonly sizeMeasured: boolean;
  /** On the same terms as `LocalLlmModel.sha256`. */
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
   * `main`.
   *
   * A branch pointer moves when the publisher pushes, which would move the
   * bytes out from under the digest committed beside them and fail an install
   * for a reason that has nothing to do with the user. Measured into
   * `HF_REVISIONS` by `node tools/model-pin/pin.mjs --revisions`.
   */
  readonly revision: string;
  readonly sizeBytes: number | null;
  /**
   * Whether `sizeBytes` was measured against the publisher or copied from
   * upstream's declaration. Upstream declared a size for every model and was
   * wrong about eighteen of them, so a non-null size is not on its own a
   * trustworthy one; the installer holds a measured size to exact equality and
   * a declared one to 5%.
   */
  readonly sizeMeasured: boolean;
  /**
   * The object's SHA-256, and whether this project measured it.
   *
   * Non-null on all thirty-nine published objects. `sha256Measured` is the
   * part that matters: **true** means the bytes were downloaded and hashed
   * here, **false** means this is the publisher's stated `lfs.oid` — a real
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
   * `sha256` would mean a model added to the registry before anybody
   * downloaded it, which `models::integrity_for` degrades to a size check
   * rather than hiding.
   *
   * With a digest, `expectedSizeBytes` is the measured length — what the
   * integrity check is handed, so it has to be bytes that were there. Without
   * one it is upstream's rounded declaration, held to a 5% tolerance. It is
   * never `sizeMb` scaled.
   */
  readonly sha256: string | null;
  readonly expectedSizeBytes: number | null;
  readonly recommended: boolean;
}

export interface SherpaModel {
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

/** The registry. Generated from config/provider-registry.json. */
export const PROVIDER_REGISTRY = {
  "credentials": [
    {
      "slot": "openai",
      "name": "OpenAI",
      "kind": "provider",
      "env": "OPENAI_API_KEY",
      "usedFor": [
        "transcription",
        "reasoning"
      ]
    },
    {
      "slot": "anthropic",
      "name": "Anthropic",
      "kind": "provider",
      "env": "ANTHROPIC_API_KEY",
      "usedFor": [
        "reasoning"
      ]
    },
    {
      "slot": "gemini",
      "name": "Google Gemini",
      "kind": "provider",
      "env": "GEMINI_API_KEY",
      "usedFor": [
        "reasoning"
      ]
    },
    {
      "slot": "groq",
      "name": "Groq",
      "kind": "provider",
      "env": "GROQ_API_KEY",
      "usedFor": [
        "transcription",
        "reasoning"
      ]
    },
    {
      "slot": "berget",
      "name": "Berget AI",
      "kind": "provider",
      "env": "BERGET_API_KEY",
      "usedFor": [
        "transcription",
        "reasoning"
      ]
    },
    {
      "slot": "xai",
      "name": "xAI",
      "kind": "provider",
      "env": "XAI_API_KEY",
      "usedFor": [
        "transcription"
      ]
    },
    {
      "slot": "mistral",
      "name": "Mistral",
      "kind": "provider",
      "env": "MISTRAL_API_KEY",
      "usedFor": [
        "transcription"
      ]
    },
    {
      "slot": "openrouter",
      "name": "OpenRouter",
      "kind": "provider",
      "env": "OPENROUTER_API_KEY",
      "usedFor": []
    },
    {
      "slot": "tinfoil",
      "name": "Tinfoil",
      "kind": "provider",
      "env": "TINFOIL_API_KEY",
      "usedFor": [
        "transcription",
        "reasoning"
      ]
    },
    {
      "slot": "corti",
      "name": "Corti",
      "kind": "provider",
      "env": "CORTI_API_KEY",
      "usedFor": [
        "transcription",
        "reasoning"
      ]
    },
    {
      "slot": "note-formatting-custom",
      "name": "Note formatting",
      "kind": "custom-endpoint",
      "env": "NOTE_FORMATTING_CUSTOM_API_KEY",
      "usedFor": [],
      "feature": "note-formatting"
    },
    {
      "slot": "translation-custom",
      "name": "Translation",
      "kind": "custom-endpoint",
      "env": "TRANSLATION_CUSTOM_API_KEY",
      "usedFor": [],
      "feature": "translation"
    },
    {
      "slot": "dictation-agent-custom",
      "name": "Dictation agent",
      "kind": "custom-endpoint",
      "env": "DICTATION_AGENT_CUSTOM_API_KEY",
      "usedFor": [],
      "feature": "dictation-agent"
    },
    {
      "slot": "dictation-agent-vision-custom",
      "name": "Dictation agent (vision)",
      "kind": "custom-endpoint",
      "env": "DICTATION_AGENT_VISION_CUSTOM_API_KEY",
      "usedFor": [],
      "feature": "dictation-agent-vision"
    },
    {
      "slot": "chat-agent-custom",
      "name": "Chat agent",
      "kind": "custom-endpoint",
      "env": "CHAT_AGENT_CUSTOM_API_KEY",
      "usedFor": [],
      "feature": "chat-agent"
    },
    {
      "slot": "bedrock",
      "name": "AWS Bedrock",
      "kind": "enterprise",
      "env": "AWS_BEDROCK_CONFIG",
      "usedFor": [],
      "feature": "bedrock"
    },
    {
      "slot": "azure",
      "name": "Azure OpenAI",
      "kind": "enterprise",
      "env": "AZURE_OPENAI_CONFIG",
      "usedFor": [],
      "feature": "azure"
    },
    {
      "slot": "vertex",
      "name": "GCP Vertex AI",
      "kind": "enterprise",
      "env": "GCP_VERTEX_CONFIG",
      "usedFor": [],
      "feature": "vertex"
    },
    {
      "slot": "assemblyai",
      "name": "AssemblyAI",
      "kind": "provider",
      "env": "ASSEMBLYAI_API_KEY",
      "usedFor": []
    },
    {
      "slot": "deepgram",
      "name": "Deepgram",
      "kind": "provider",
      "env": "DEEPGRAM_API_KEY",
      "usedFor": []
    },
    {
      "slot": "custom",
      "name": "Your own server",
      "kind": "custom-endpoint",
      "env": "CUSTOM_API_KEY",
      "usedFor": [
        "reasoning"
      ]
    }
  ],
  "transcription": [
    {
      "id": "openai",
      "name": "OpenAI",
      "baseUrl": "https://api.openai.com/v1",
      "authScheme": "bearer",
      "credentialSlot": "openai",
      "allowCustomModelId": false,
      "maxUploadBytes": 26214400,
      "models": [
        {
          "id": "gpt-4o-mini-transcribe",
          "name": "GPT-4o Mini Transcribe",
          "streaming": true
        },
        {
          "id": "gpt-4o-transcribe",
          "name": "GPT-4o Transcribe",
          "streaming": true
        },
        {
          "id": "whisper-1",
          "name": "Whisper",
          "streaming": false
        }
      ]
    },
    {
      "id": "groq",
      "name": "Groq",
      "baseUrl": "https://api.groq.com/openai/v1",
      "authScheme": "bearer",
      "credentialSlot": "groq",
      "allowCustomModelId": false,
      "maxUploadBytes": 26214400,
      "models": [
        {
          "id": "whisper-large-v3",
          "name": "Whisper Large v3",
          "streaming": false
        },
        {
          "id": "whisper-large-v3-turbo",
          "name": "Whisper Large v3 Turbo",
          "streaming": false
        }
      ]
    },
    {
      "id": "berget",
      "name": "Berget AI",
      "baseUrl": "https://api.berget.ai/v1",
      "authScheme": "bearer",
      "credentialSlot": "berget",
      "allowCustomModelId": true,
      "maxUploadBytes": null,
      "models": [
        {
          "id": "KBLab/kb-whisper-large",
          "name": "KB Whisper Large",
          "streaming": false
        },
        {
          "id": "NbAiLab/nb-whisper-large",
          "name": "NB Whisper Large",
          "streaming": false
        },
        {
          "id": "Systran/faster-whisper-large-v3",
          "name": "Whisper Large v3",
          "streaming": false
        }
      ]
    },
    {
      "id": "xai",
      "name": "xAI",
      "baseUrl": "https://api.x.ai/v1",
      "authScheme": "bearer",
      "credentialSlot": "xai",
      "allowCustomModelId": false,
      "maxUploadBytes": 26214400,
      "models": [
        {
          "id": "grok-stt",
          "name": "Grok STT",
          "streaming": false
        }
      ]
    },
    {
      "id": "mistral",
      "name": "Mistral",
      "baseUrl": "https://api.mistral.ai/v1",
      "authScheme": "bearer",
      "credentialSlot": "mistral",
      "allowCustomModelId": false,
      "maxUploadBytes": 26214400,
      "models": [
        {
          "id": "voxtral-mini-latest",
          "name": "Voxtral Mini",
          "streaming": false
        }
      ]
    },
    {
      "id": "corti",
      "name": "Corti",
      "baseUrl": "https://api.us.corti.app/v2",
      "authScheme": "oauth2-client-credentials",
      "credentialSlot": "corti",
      "allowCustomModelId": false,
      "maxUploadBytes": 26214400,
      "models": [
        {
          "id": "corti-transcribe",
          "name": "Corti Transcribe",
          "streaming": true
        }
      ]
    },
    {
      "id": "tinfoil",
      "name": "Tinfoil",
      "baseUrl": "https://inference.tinfoil.sh/v1",
      "authScheme": "attested",
      "credentialSlot": "tinfoil",
      "allowCustomModelId": false,
      "maxUploadBytes": 26214400,
      "batchModel": "voxtral-small-24b",
      "models": [
        {
          "id": "voxtral-mini-4b-realtime",
          "name": "Voxtral",
          "streaming": true
        }
      ]
    }
  ],
  "reasoning": [
    {
      "id": "openai",
      "name": "OpenAI",
      "authScheme": "bearer",
      "credentialSlot": "openai",
      "allowCustomModelId": false,
      "models": [
        {
          "id": "gpt-5.6-sol",
          "name": "GPT-5.6 Sol",
          "tokenParam": "max_completion_tokens",
          "supportsTemperature": false,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "gpt-5.6-terra",
          "name": "GPT-5.6 Terra",
          "tokenParam": "max_completion_tokens",
          "supportsTemperature": false,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "gpt-5.6-luna",
          "name": "GPT-5.6 Luna",
          "tokenParam": "max_completion_tokens",
          "supportsTemperature": false,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "gpt-5.5",
          "name": "GPT-5.5",
          "tokenParam": "max_completion_tokens",
          "supportsTemperature": false,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "gpt-5.2",
          "name": "GPT-5.2",
          "tokenParam": "max_completion_tokens",
          "supportsTemperature": false,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "gpt-5-mini",
          "name": "GPT-5 Mini",
          "tokenParam": "max_completion_tokens",
          "supportsTemperature": false,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "gpt-5-nano",
          "name": "GPT-5 Nano",
          "tokenParam": "max_completion_tokens",
          "supportsTemperature": false,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "gpt-4.1",
          "name": "GPT-4.1",
          "tokenParam": "max_completion_tokens",
          "supportsTemperature": true,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "gpt-4.1-mini",
          "name": "GPT-4.1 Mini",
          "tokenParam": "max_completion_tokens",
          "supportsTemperature": true,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "gpt-4.1-nano",
          "name": "GPT-4.1 Nano",
          "tokenParam": "max_completion_tokens",
          "supportsTemperature": true,
          "supportsThinking": false,
          "supportsVision": true
        }
      ]
    },
    {
      "id": "anthropic",
      "name": "Anthropic",
      "authScheme": "bearer",
      "credentialSlot": "anthropic",
      "allowCustomModelId": false,
      "models": [
        {
          "id": "claude-fable-5",
          "name": "Claude Fable 5",
          "tokenParam": "max_tokens",
          "supportsTemperature": false,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "claude-sonnet-5",
          "name": "Claude Sonnet 5",
          "tokenParam": "max_tokens",
          "supportsTemperature": false,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "claude-sonnet-4-6",
          "name": "Claude Sonnet 4.6",
          "tokenParam": "max_tokens",
          "supportsTemperature": true,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "claude-haiku-4-5",
          "name": "Claude Haiku 4.5",
          "tokenParam": "max_tokens",
          "supportsTemperature": true,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "claude-opus-5",
          "name": "Claude Opus 5",
          "tokenParam": "max_tokens",
          "supportsTemperature": false,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "claude-opus-4-8",
          "name": "Claude Opus 4.8",
          "tokenParam": "max_tokens",
          "supportsTemperature": false,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "claude-opus-4-7",
          "name": "Claude Opus 4.7",
          "tokenParam": "max_tokens",
          "supportsTemperature": false,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "claude-opus-4-6",
          "name": "Claude Opus 4.6",
          "tokenParam": "max_tokens",
          "supportsTemperature": true,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "claude-sonnet-4-5",
          "name": "Claude Sonnet 4.5",
          "tokenParam": "max_tokens",
          "supportsTemperature": true,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "claude-opus-4-5",
          "name": "Claude Opus 4.5",
          "tokenParam": "max_tokens",
          "supportsTemperature": true,
          "supportsThinking": false,
          "supportsVision": true
        }
      ]
    },
    {
      "id": "gemini",
      "name": "Google Gemini",
      "authScheme": "bearer",
      "credentialSlot": "gemini",
      "allowCustomModelId": false,
      "models": [
        {
          "id": "gemini-3.5-flash",
          "name": "Gemini 3.5 Flash",
          "tokenParam": "max_tokens",
          "supportsTemperature": false,
          "supportsThinking": true,
          "supportsVision": true
        },
        {
          "id": "gemini-3.5-flash-lite",
          "name": "Gemini 3.5 Flash Lite",
          "tokenParam": "max_tokens",
          "supportsTemperature": false,
          "supportsThinking": true,
          "supportsVision": true
        },
        {
          "id": "gemini-3.1-pro-preview",
          "name": "Gemini 3.1 Pro",
          "tokenParam": "max_tokens",
          "supportsTemperature": false,
          "supportsThinking": true,
          "supportsVision": true
        },
        {
          "id": "gemini-3.1-flash-lite",
          "name": "Gemini 3.1 Flash Lite",
          "tokenParam": "max_tokens",
          "supportsTemperature": false,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "gemini-3-flash-preview",
          "name": "Gemini 3 Flash",
          "tokenParam": "max_tokens",
          "supportsTemperature": false,
          "supportsThinking": true,
          "supportsVision": true
        },
        {
          "id": "gemini-2.5-flash-lite",
          "name": "Gemini 2.5 Flash Lite",
          "tokenParam": "max_tokens",
          "supportsTemperature": false,
          "supportsThinking": false,
          "supportsVision": true
        },
        {
          "id": "gemma-4-31b-it",
          "name": "Gemma 4 31B",
          "tokenParam": "max_tokens",
          "supportsTemperature": false,
          "supportsThinking": false,
          "supportsVision": false
        },
        {
          "id": "gemma-4-26b-a4b-it",
          "name": "Gemma 4 26B MoE",
          "tokenParam": "max_tokens",
          "supportsTemperature": false,
          "supportsThinking": false,
          "supportsVision": false
        }
      ]
    },
    {
      "id": "groq",
      "name": "Groq",
      "authScheme": "bearer",
      "credentialSlot": "groq",
      "allowCustomModelId": false,
      "models": [
        {
          "id": "openai/gpt-oss-120b",
          "name": "GPT-OSS 120B",
          "tokenParam": "max_tokens",
          "supportsTemperature": false,
          "supportsThinking": true,
          "supportsVision": false
        },
        {
          "id": "openai/gpt-oss-20b",
          "name": "GPT-OSS 20B",
          "tokenParam": "max_tokens",
          "supportsTemperature": false,
          "supportsThinking": true,
          "supportsVision": false
        },
        {
          "id": "groq/compound",
          "name": "Compound",
          "tokenParam": "max_tokens",
          "supportsTemperature": false,
          "supportsThinking": false,
          "supportsVision": false
        },
        {
          "id": "groq/compound-mini",
          "name": "Compound Mini",
          "tokenParam": "max_tokens",
          "supportsTemperature": false,
          "supportsThinking": false,
          "supportsVision": false
        }
      ]
    },
    {
      "id": "tinfoil",
      "name": "Tinfoil",
      "authScheme": "attested",
      "credentialSlot": "tinfoil",
      "allowCustomModelId": false,
      "models": [
        {
          "id": "glm-5-2",
          "name": "GLM-5.2",
          "tokenParam": "max_tokens",
          "supportsTemperature": true,
          "supportsThinking": true,
          "supportsVision": false
        },
        {
          "id": "gpt-oss-120b",
          "name": "GPT-OSS 120B",
          "tokenParam": "max_tokens",
          "supportsTemperature": true,
          "supportsThinking": true,
          "supportsVision": false
        },
        {
          "id": "gemma4-31b",
          "name": "Gemma 4 31B",
          "tokenParam": "max_tokens",
          "supportsTemperature": true,
          "supportsThinking": true,
          "supportsVision": false
        }
      ]
    },
    {
      "id": "berget",
      "name": "Berget AI",
      "authScheme": "bearer",
      "credentialSlot": "berget",
      "allowCustomModelId": true,
      "models": [
        {
          "id": "openai/gpt-oss-120b",
          "name": "GPT-OSS 120B",
          "tokenParam": "max_tokens",
          "supportsTemperature": true,
          "supportsThinking": false,
          "supportsVision": false
        }
      ]
    },
    {
      "id": "corti",
      "name": "Corti",
      "authScheme": "oauth2-client-credentials",
      "credentialSlot": "corti",
      "allowCustomModelId": false,
      "models": [
        {
          "id": "corti-s1-instant",
          "name": "Corti S1 Instant",
          "tokenParam": "max_tokens",
          "supportsTemperature": true,
          "supportsThinking": false,
          "supportsVision": false
        },
        {
          "id": "corti-s1",
          "name": "Corti S1",
          "tokenParam": "max_tokens",
          "supportsTemperature": true,
          "supportsThinking": false,
          "supportsVision": false
        },
        {
          "id": "corti-s1-mini-instant",
          "name": "Corti S1 Mini Instant",
          "tokenParam": "max_tokens",
          "supportsTemperature": true,
          "supportsThinking": false,
          "supportsVision": false
        },
        {
          "id": "corti-s1-mini",
          "name": "Corti S1 Mini",
          "tokenParam": "max_tokens",
          "supportsTemperature": true,
          "supportsThinking": false,
          "supportsVision": false
        }
      ]
    }
  ],
  "enterprise": [
    {
      "id": "bedrock",
      "name": "AWS Bedrock",
      "credentialShape": "bedrock-multi-field",
      "allowCustomModelId": true,
      "deploymentNameDriven": false,
      "models": [
        {
          "id": "us.anthropic.claude-fable-5",
          "name": "Claude Fable 5"
        },
        {
          "id": "us.anthropic.claude-haiku-4-5-20251001-v1:0",
          "name": "Claude Haiku 4.5"
        },
        {
          "id": "us.anthropic.claude-sonnet-5",
          "name": "Claude Sonnet 5"
        },
        {
          "id": "us.anthropic.claude-opus-4-8",
          "name": "Claude Opus 4.8"
        },
        {
          "id": "openai.gpt-oss-120b-1:0",
          "name": "GPT-OSS 120B"
        },
        {
          "id": "deepseek.v3.2",
          "name": "DeepSeek V3.2"
        },
        {
          "id": "qwen.qwen3-next-80b-a3b",
          "name": "Qwen3 Next 80B"
        }
      ]
    },
    {
      "id": "azure",
      "name": "Azure OpenAI",
      "credentialShape": "azure-multi-field",
      "allowCustomModelId": true,
      "deploymentNameDriven": true,
      "models": []
    },
    {
      "id": "vertex",
      "name": "GCP Vertex AI",
      "credentialShape": "vertex-multi-field",
      "allowCustomModelId": true,
      "deploymentNameDriven": false,
      "models": [
        {
          "id": "gemini-2.5-flash",
          "name": "Gemini 2.5 Flash"
        },
        {
          "id": "gemini-2.5-pro",
          "name": "Gemini 2.5 Pro"
        }
      ]
    }
  ],
  "localLlm": [
    {
      "id": "qwen",
      "name": "Qwen",
      "baseUrl": "https://huggingface.co",
      "models": [
        {
          "id": "qwen3.5-9b-q4_k_m",
          "name": "Qwen3.5 9B",
          "fileName": "Qwen_Qwen3.5-9B-Q4_K_M.gguf",
          "hfRepo": "bartowski/Qwen_Qwen3.5-9B-GGUF",
          "revision": "182be2fd6c7bc44887d88a91cb03ff009cc9f549",
          "sizeBytes": 6169341984,
          "sizeMeasured": true,
          "sha256": "d784ce9eda1a5a7b51e8f705a9e6310844bf4f173654d115823c775fdea56d43",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 262144,
          "recommended": true,
          "supportsThinking": true,
          "draft": null
        },
        {
          "id": "qwen3.5-4b-q4_k_m",
          "name": "Qwen3.5 4B",
          "fileName": "Qwen_Qwen3.5-4B-Q4_K_M.gguf",
          "hfRepo": "bartowski/Qwen_Qwen3.5-4B-GGUF",
          "revision": "4168f45a16a1290d65a4ec0fa312ae917a4c15d6",
          "sizeBytes": 3013027808,
          "sizeMeasured": true,
          "sha256": "13c16f426047e2de38cd075bdade4a7bcbc8c774384876f677740cda65f8a983",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 262144,
          "recommended": false,
          "supportsThinking": true,
          "draft": null
        },
        {
          "id": "qwen3.5-2b-q4_k_m",
          "name": "Qwen3.5 2B",
          "fileName": "Qwen_Qwen3.5-2B-Q4_K_M.gguf",
          "hfRepo": "bartowski/Qwen_Qwen3.5-2B-GGUF",
          "revision": "7d26695454df6de5fbcce2e58681e62dae06ce43",
          "sizeBytes": 1396198496,
          "sizeMeasured": true,
          "sha256": "57a1085840f497d764a7fc5d346922dbde961efb54cc792ea81d694fd846a1d8",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 262144,
          "recommended": false,
          "supportsThinking": true,
          "draft": null
        },
        {
          "id": "qwen3-8b-q4_k_m",
          "name": "Qwen3 8B",
          "fileName": "Qwen3-8B-Q4_K_M.gguf",
          "hfRepo": "Qwen/Qwen3-8B-GGUF",
          "revision": "7c41481f57cb95916b40956ab2f0b139b296d974",
          "sizeBytes": 5027783488,
          "sizeMeasured": true,
          "sha256": "d98cdcbd03e17ce47681435b5150e34c1417f50b5c0019dd560e4882c5745785",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 131072,
          "recommended": false,
          "supportsThinking": true,
          "draft": null
        },
        {
          "id": "qwen3-8b-q5_k_m",
          "name": "Qwen3 8B (Q5)",
          "fileName": "Qwen3-8B-Q5_K_M.gguf",
          "hfRepo": "Qwen/Qwen3-8B-GGUF",
          "revision": "7c41481f57cb95916b40956ab2f0b139b296d974",
          "sizeBytes": 5851112224,
          "sizeMeasured": true,
          "sha256": "068bae163faa96ad48032daf4e071a6a28fe67d8dcc95367609c2ff165e52738",
          "sha256Measured": false,
          "quantization": "q5_k_m",
          "contextLength": 131072,
          "recommended": false,
          "supportsThinking": true,
          "draft": null
        },
        {
          "id": "qwen3-4b-q4_k_m",
          "name": "Qwen3 4B",
          "fileName": "Qwen3-4B-Q4_K_M.gguf",
          "hfRepo": "Qwen/Qwen3-4B-GGUF",
          "revision": "bc640142c66e1fdd12af0bd68f40445458f3869b",
          "sizeBytes": 2497280256,
          "sizeMeasured": true,
          "sha256": "7485fe6f11af29433bc51cab58009521f205840f5b4ae3a32fa7f92e8534fdf5",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 131072,
          "recommended": false,
          "supportsThinking": true,
          "draft": null
        },
        {
          "id": "qwen3-1.7b-q8_0",
          "name": "Qwen3 1.7B",
          "fileName": "Qwen3-1.7B-Q8_0.gguf",
          "hfRepo": "Qwen/Qwen3-1.7B-GGUF",
          "revision": "90862c4b9d2787eaed51d12237eafdfe7c5f6077",
          "sizeBytes": 1834426016,
          "sizeMeasured": true,
          "sha256": "061b54daade076b5d3362dac252678d17da8c68f07560be70818cace6590cb1a",
          "sha256Measured": false,
          "quantization": "q8_0",
          "contextLength": 131072,
          "recommended": false,
          "supportsThinking": true,
          "draft": null
        },
        {
          "id": "qwen3-32b-q4_k_m",
          "name": "Qwen3 32B",
          "fileName": "Qwen3-32B-Q4_K_M.gguf",
          "hfRepo": "Qwen/Qwen3-32B-GGUF",
          "revision": "938a7432affaec9157f883a87164e2646ae17555",
          "sizeBytes": 19762149024,
          "sizeMeasured": true,
          "sha256": "efd971561896866f0e910cce52761ca77b1b138090c7f15fe284676d57d1f689",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 131072,
          "recommended": false,
          "supportsThinking": true,
          "draft": null
        },
        {
          "id": "qwen2.5-1.5b-instruct-q5_k_m",
          "name": "Qwen2.5 1.5B",
          "fileName": "qwen2.5-1.5b-instruct-q5_k_m.gguf",
          "hfRepo": "Qwen/Qwen2.5-1.5B-Instruct-GGUF",
          "revision": "91cad51170dc346986eccefdc2dd33a9da36ead9",
          "sizeBytes": 1285494304,
          "sizeMeasured": true,
          "sha256": "b46661073c18e5b56a41fa320975f866a00def1ff08feef4718e013258896f8c",
          "sha256Measured": true,
          "quantization": "q5_k_m",
          "contextLength": 32768,
          "recommended": false,
          "supportsThinking": false,
          "draft": null
        },
        {
          "id": "qwen2.5-3b-instruct-q5_k_m",
          "name": "Qwen2.5 3B",
          "fileName": "qwen2.5-3b-instruct-q5_k_m.gguf",
          "hfRepo": "Qwen/Qwen2.5-3B-Instruct-GGUF",
          "revision": "7dabda4d13d513e3e842b20f0d435c732f172cbe",
          "sizeBytes": 2438740384,
          "sizeMeasured": true,
          "sha256": "2c63dde5f2c9ab1fd64d47dee2d34dade6ba9ff62442d1d20b5342310c982081",
          "sha256Measured": false,
          "quantization": "q5_k_m",
          "contextLength": 32768,
          "recommended": false,
          "supportsThinking": false,
          "draft": null
        },
        {
          "id": "qwen2.5-7b-instruct-q4_k_m",
          "name": "Qwen2.5 7B",
          "fileName": "Qwen2.5-7B-Instruct-Q4_K_M.gguf",
          "hfRepo": "bartowski/Qwen2.5-7B-Instruct-GGUF",
          "revision": "8911e8a47f92bac19d6f5c64a2e2095bd2f7d031",
          "sizeBytes": 4683074240,
          "sizeMeasured": true,
          "sha256": "65b8fcd92af6b4fefa935c625d1ac27ea29dcb6ee14589c55a8f115ceaaa1423",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 128000,
          "recommended": false,
          "supportsThinking": false,
          "draft": null
        },
        {
          "id": "qwen2.5-7b-instruct-q5_k_m",
          "name": "Qwen2.5 7B (Q5)",
          "fileName": "Qwen2.5-7B-Instruct-Q5_K_M.gguf",
          "hfRepo": "bartowski/Qwen2.5-7B-Instruct-GGUF",
          "revision": "8911e8a47f92bac19d6f5c64a2e2095bd2f7d031",
          "sizeBytes": 5444831936,
          "sizeMeasured": true,
          "sha256": "2e998d7e181c8756c5ffc55231b9ee1cdc9d3acec4245d6e27d32bd8e738c474",
          "sha256Measured": false,
          "quantization": "q5_k_m",
          "contextLength": 128000,
          "recommended": false,
          "supportsThinking": false,
          "draft": null
        }
      ]
    },
    {
      "id": "mistral",
      "name": "Mistral",
      "baseUrl": "https://huggingface.co",
      "models": [
        {
          "id": "mistral-nemo-12b-instruct-q4_k_m",
          "name": "Mistral Nemo 12B",
          "fileName": "Mistral-Nemo-12B-Instruct.Q4_K_M.gguf",
          "hfRepo": "mradermacher/Mistral-Nemo-12B-Instruct-GGUF",
          "revision": "308fe5820f484b70e0169aad5b5553e15aabd5b9",
          "sizeBytes": 7477208320,
          "sizeMeasured": true,
          "sha256": "2046c16c427f5bd6dfb2b08b6b74edcc6102e0e2129cebff38ef7aa746d26f31",
          "sha256Measured": true,
          "quantization": "q4_k_m",
          "contextLength": 131072,
          "recommended": true,
          "supportsThinking": false,
          "draft": null
        },
        {
          "id": "mistral-7b-instruct-v0.3-q4_k_m",
          "name": "Mistral 7B Instruct v0.3",
          "fileName": "Mistral-7B-Instruct-v0.3-Q4_K_M.gguf",
          "hfRepo": "bartowski/Mistral-7B-Instruct-v0.3-GGUF",
          "revision": "61fd4167fff3ab01ee1cfe0da183fa27a944db48",
          "sizeBytes": 4372812000,
          "sizeMeasured": true,
          "sha256": "1270d22c0fbb3d092fb725d4d96c457b7b687a5f5a715abe1e818da303e562b6",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 32768,
          "recommended": false,
          "supportsThinking": false,
          "draft": null
        },
        {
          "id": "mistral-7b-instruct-v0.3-q5_k_m",
          "name": "Mistral 7B Instruct v0.3 (Q5)",
          "fileName": "Mistral-7B-Instruct-v0.3-Q5_K_M.gguf",
          "hfRepo": "bartowski/Mistral-7B-Instruct-v0.3-GGUF",
          "revision": "61fd4167fff3ab01ee1cfe0da183fa27a944db48",
          "sizeBytes": 5136175328,
          "sizeMeasured": true,
          "sha256": "0374072546aaf1257d1e34610edb11b691730d8ef7d44b5629693a2dd8e8d472",
          "sha256Measured": false,
          "quantization": "q5_k_m",
          "contextLength": 32768,
          "recommended": false,
          "supportsThinking": false,
          "draft": null
        }
      ]
    },
    {
      "id": "llama",
      "name": "Meta Llama",
      "baseUrl": "https://huggingface.co",
      "models": [
        {
          "id": "llama-3.2-1b-instruct-q4_k_m",
          "name": "Llama 3.2 1B",
          "fileName": "Llama-3.2-1B-Instruct-Q4_K_M.gguf",
          "hfRepo": "bartowski/Llama-3.2-1B-Instruct-GGUF",
          "revision": "067b946cf014b7c697f3654f621d577a3e3afd1c",
          "sizeBytes": 807694464,
          "sizeMeasured": true,
          "sha256": "6f85a640a97cf2bf5b8e764087b1e83da0fdb51d7c9fab7d0fece9385611df83",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 131072,
          "recommended": false,
          "supportsThinking": false,
          "draft": null
        },
        {
          "id": "llama-3.2-3b-instruct-q4_k_m",
          "name": "Llama 3.2 3B",
          "fileName": "Llama-3.2-3B-Instruct-Q4_K_M.gguf",
          "hfRepo": "bartowski/Llama-3.2-3B-Instruct-GGUF",
          "revision": "5ab33fa94d1d04e903623ae72c95d1696f09f9e8",
          "sizeBytes": 2019377696,
          "sizeMeasured": true,
          "sha256": "6c1a2b41161032677be168d354123594c0e6e67d2b9227c84f296ad037c728ff",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 131072,
          "recommended": true,
          "supportsThinking": false,
          "draft": null
        },
        {
          "id": "llama-3.1-8b-instruct-q4_k_m",
          "name": "Llama 3.1 8B",
          "fileName": "Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf",
          "hfRepo": "bartowski/Meta-Llama-3.1-8B-Instruct-GGUF",
          "revision": "bf5b95e96dac0462e2a09145ec66cae9a3f12067",
          "sizeBytes": 4920739232,
          "sizeMeasured": true,
          "sha256": "7b064f5842bf9532c91456deda288a1b672397a54fa729aa665952863033557c",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 131072,
          "recommended": false,
          "supportsThinking": false,
          "draft": null
        }
      ]
    },
    {
      "id": "openai-oss",
      "name": "OpenAI",
      "baseUrl": "https://huggingface.co",
      "models": [
        {
          "id": "gpt-oss-20b-mxfp4",
          "name": "GPT-OSS 20B",
          "fileName": "gpt-oss-20b-MXFP4.gguf",
          "hfRepo": "ggml-org/gpt-oss-20b-GGUF",
          "revision": "ef9b12f2ff56c69cf32153a02784e7a3c88bf524",
          "sizeBytes": 12109566624,
          "sizeMeasured": true,
          "sha256": "27cd6c432c7672cb812a92f611cf3ba7bbc35928262bb1e1253ff4ee6ae35901",
          "sha256Measured": true,
          "quantization": "mxfp4",
          "contextLength": 128000,
          "recommended": true,
          "supportsThinking": true,
          "draft": null
        }
      ]
    },
    {
      "id": "gemma",
      "name": "Gemma (Google)",
      "baseUrl": "https://huggingface.co",
      "models": [
        {
          "id": "gemma-4-31b-it-q4_k_m",
          "name": "Gemma 4 31B",
          "fileName": "google_gemma-4-31B-it-Q4_K_M.gguf",
          "hfRepo": "bartowski/google_gemma-4-31B-it-GGUF",
          "revision": "619aca7589c72be65dd968de8602ad23d68b5c14",
          "sizeBytes": 19598489952,
          "sizeMeasured": true,
          "sha256": "2959c1b0a4f792493bb0092846ea06aec54547e47be979c282634d938d501b75",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 262144,
          "recommended": false,
          "supportsThinking": false,
          "draft": null
        },
        {
          "id": "gemma-4-31b-it-qat-q4_0",
          "name": "Gemma 4 31B QAT",
          "fileName": "gemma-4-31B_q4_0-it.gguf",
          "hfRepo": "google/gemma-4-31B-it-qat-q4_0-gguf",
          "revision": "59dde24573e7e61570dba08b18a2e1fe246955ed",
          "sizeBytes": 17651001568,
          "sizeMeasured": true,
          "sha256": "179cfb99212709597eae5929112cfca677e1bbf566178b479ae1da0c4772874b",
          "sha256Measured": false,
          "quantization": "q4_0",
          "contextLength": 262144,
          "recommended": false,
          "supportsThinking": false,
          "draft": {
            "hfRepo": "unsloth/gemma-4-31B-it-qat-GGUF",
            "revision": "43cc1aeb31adf47ec06a854507ce552cd9862e6f",
            "fileName": "mtp-gemma-4-31B-it.gguf",
            "sizeBytes": 279955968,
            "sizeMeasured": true,
            "sha256": "3a5e99fd8d0b23afb1fccd1ee0c9ebd1f571d00399c2dae2292d217feeec0f6b",
            "sha256Measured": false
          }
        },
        {
          "id": "gemma-4-26b-a4b-it-q4_k_m",
          "name": "Gemma 4 26B MoE",
          "fileName": "google_gemma-4-26B-A4B-it-Q4_K_M.gguf",
          "hfRepo": "bartowski/google_gemma-4-26B-A4B-it-GGUF",
          "revision": "10f3b41bcf8d3047f4e136e7197ffc2dd1654c9d",
          "sizeBytes": 17035039872,
          "sizeMeasured": true,
          "sha256": "a07f72221e8e3f77455ab0d7f7652d01a9f63c262b954aa6932a53275a0e895a",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 262144,
          "recommended": false,
          "supportsThinking": false,
          "draft": null
        },
        {
          "id": "gemma-4-26b-a4b-it-qat-q4_0",
          "name": "Gemma 4 26B MoE QAT",
          "fileName": "gemma-4-26B_q4_0-it.gguf",
          "hfRepo": "google/gemma-4-26B-A4B-it-qat-q4_0-gguf",
          "revision": "d1c082be9cf3c8a514acf63b8761f4b41935842e",
          "sizeBytes": 14439363584,
          "sizeMeasured": true,
          "sha256": "3eca3b8f6d7baf218a7dd6bba5fb59a56ee25fe2d567b6f5f589b4f697eca51d",
          "sha256Measured": false,
          "quantization": "q4_0",
          "contextLength": 262144,
          "recommended": false,
          "supportsThinking": false,
          "draft": {
            "hfRepo": "unsloth/gemma-4-26B-A4B-it-qat-GGUF",
            "revision": "7b92b5b28818151e8669af2e45e88d6086f490dd",
            "fileName": "mtp-gemma-4-26B-A4B-it.gguf",
            "sizeBytes": 251939328,
            "sizeMeasured": true,
            "sha256": "7272d97595f0d4c74bd7b623492b7dbdaafd8b7c72f329a8270ba4eca68f768a",
            "sha256Measured": false
          }
        },
        {
          "id": "gemma-4-e4b-it-q4_k_m",
          "name": "Gemma 4 E4B",
          "fileName": "google_gemma-4-E4B-it-Q4_K_M.gguf",
          "hfRepo": "bartowski/google_gemma-4-E4B-it-GGUF",
          "revision": "029e94146666900b08caf49a3b47b413dfa8ec66",
          "sizeBytes": 5405170144,
          "sizeMeasured": true,
          "sha256": "d35a3aa7a6d47de237fd488bb6f7f3efd62908d828f85a63da0ee93acdbefe9f",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 262144,
          "recommended": false,
          "supportsThinking": false,
          "draft": null
        },
        {
          "id": "gemma-4-e4b-it-qat-q4_0",
          "name": "Gemma 4 E4B QAT",
          "fileName": "gemma-4-E4B_q4_0-it.gguf",
          "hfRepo": "google/gemma-4-E4B-it-qat-q4_0-gguf",
          "revision": "4b4a2c1d584be7264f87aac328a1bc739ce81b6c",
          "sizeBytes": 5154941280,
          "sizeMeasured": true,
          "sha256": "676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee",
          "sha256Measured": false,
          "quantization": "q4_0",
          "contextLength": 262144,
          "recommended": true,
          "supportsThinking": false,
          "draft": {
            "hfRepo": "unsloth/gemma-4-E4B-it-qat-GGUF",
            "revision": "8c5a9e4fd5482e2be20fe0bf013b4c262a8f4265",
            "fileName": "mtp-gemma-4-E4B-it.gguf",
            "sizeBytes": 59678016,
            "sizeMeasured": true,
            "sha256": "423074e537504b4f9ec5eafed5c639fac82c96631626efccacdd3c4039b20605",
            "sha256Measured": false
          }
        },
        {
          "id": "gemma-4-e2b-it-q4_k_m",
          "name": "Gemma 4 E2B",
          "fileName": "google_gemma-4-E2B-it-Q4_K_M.gguf",
          "hfRepo": "bartowski/google_gemma-4-E2B-it-GGUF",
          "revision": "81012ba3538e061d5ee003f11f25335b17f82e2d",
          "sizeBytes": 3462680032,
          "sizeMeasured": true,
          "sha256": "923c4c86177d2ee173a7f5b4fa3d0ac65f5962ab15e6d6a5bc250aec4fd7bf7e",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 262144,
          "recommended": false,
          "supportsThinking": false,
          "draft": null
        },
        {
          "id": "gemma-4-e2b-it-qat-q4_0",
          "name": "Gemma 4 E2B QAT",
          "fileName": "gemma-4-E2B_q4_0-it.gguf",
          "hfRepo": "google/gemma-4-E2B-it-qat-q4_0-gguf",
          "revision": "675cff42a74c774d6cb76f76d8eacb49b48c9b93",
          "sizeBytes": 3349516256,
          "sizeMeasured": true,
          "sha256": "fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634",
          "sha256Measured": true,
          "quantization": "q4_0",
          "contextLength": 262144,
          "recommended": false,
          "supportsThinking": false,
          "draft": {
            "hfRepo": "unsloth/gemma-4-E2B-it-qat-GGUF",
            "revision": "66a399f68ddd113b06dff02fca9523e55465d11d",
            "fileName": "mtp-gemma-4-E2B-it.gguf",
            "sizeBytes": 59235648,
            "sizeMeasured": true,
            "sha256": "586f2460b909008640981ec34060aa864e03c144fbabfb3173c4335087e4aae0",
            "sha256Measured": true
          }
        },
        {
          "id": "gemma-3-12b-it-q4_k_m",
          "name": "Gemma 3 12B",
          "fileName": "google_gemma-3-12b-it-Q4_K_M.gguf",
          "hfRepo": "bartowski/google_gemma-3-12b-it-GGUF",
          "revision": "648e3a36a77c8a9f12d86e741f9dcb9089c769c4",
          "sizeBytes": 7300575264,
          "sizeMeasured": true,
          "sha256": "fc57f67efa46d711c346e587cbef7d049e95f3df8db2eb2271153343ef0acc7b",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 131072,
          "recommended": false,
          "supportsThinking": false,
          "draft": null
        },
        {
          "id": "gemma-3-4b-it-q4_k_m",
          "name": "Gemma 3 4B",
          "fileName": "google_gemma-3-4b-it-Q4_K_M.gguf",
          "hfRepo": "bartowski/google_gemma-3-4b-it-GGUF",
          "revision": "71506238f970075ca85125cd749c28b1b0eee84e",
          "sizeBytes": 2489758112,
          "sizeMeasured": true,
          "sha256": "4996030242583a40aa151ff93f49ed787ac8c25e4120c3ae4588b2e2a7d1ae94",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 131072,
          "recommended": false,
          "supportsThinking": false,
          "draft": null
        },
        {
          "id": "gemma-3-1b-it-q4_k_m",
          "name": "Gemma 3 1B",
          "fileName": "google_gemma-3-1b-it-Q4_K_M.gguf",
          "hfRepo": "bartowski/google_gemma-3-1b-it-GGUF",
          "revision": "116f76234503685a98f572982177b11d44ec8ff1",
          "sizeBytes": 806058496,
          "sizeMeasured": true,
          "sha256": "12bf0fff8815d5f73a3c9b586bd8fee8e7b248c935de70dec367679873d0f29d",
          "sha256Measured": true,
          "quantization": "q4_k_m",
          "contextLength": 32768,
          "recommended": false,
          "supportsThinking": false,
          "draft": null
        }
      ]
    },
    {
      "id": "liquidai",
      "name": "Liquid AI",
      "baseUrl": "https://huggingface.co",
      "models": [
        {
          "id": "lfm2.5-1.2b-instruct-q4_k_m",
          "name": "LFM2.5 1.2B",
          "fileName": "LFM2.5-1.2B-Instruct-Q4_K_M.gguf",
          "hfRepo": "LiquidAI/LFM2.5-1.2B-Instruct-GGUF",
          "revision": "6767265158422fb8a19c62ceb45f16f05363615b",
          "sizeBytes": 730895168,
          "sizeMeasured": true,
          "sha256": "b1b3de114215d9507409a662a501a631095a479a419584e8a2ded6304b19b4f5",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 32768,
          "recommended": true,
          "supportsThinking": false,
          "draft": null
        },
        {
          "id": "lfm2.5-8b-a1b-q4_k_m",
          "name": "LFM2.5 8B MoE",
          "fileName": "LFM2.5-8B-A1B-Q4_K_M.gguf",
          "hfRepo": "LiquidAI/LFM2.5-8B-A1B-GGUF",
          "revision": "49c14831707011e64d70b2ebd8462ba08d608434",
          "sizeBytes": 5155564768,
          "sizeMeasured": true,
          "sha256": "4923ec14f06b968b74d663e5949867d2d9c3bf13a20b8be1a9f9af39989b2bb0",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 32768,
          "recommended": false,
          "supportsThinking": false,
          "draft": null
        },
        {
          "id": "lfm2-2.6b-q4_k_m",
          "name": "LFM2 2.6B",
          "fileName": "LFM2-2.6B-Q4_K_M.gguf",
          "hfRepo": "LiquidAI/LFM2-2.6B-GGUF",
          "revision": "a759abdc5955d4ca97763e5cb7ff3940589ba898",
          "sizeBytes": 1563668704,
          "sizeMeasured": true,
          "sha256": "384bc877b6c37064982f96885bef69e4475919f5969218ed4e3b9399ae0340df",
          "sha256Measured": false,
          "quantization": "q4_k_m",
          "contextLength": 32768,
          "recommended": false,
          "supportsThinking": false,
          "draft": null
        },
        {
          "id": "lfm2.5-350m-q8_0",
          "name": "LFM2.5 350M",
          "fileName": "LFM2.5-350M-Q8_0.gguf",
          "hfRepo": "LiquidAI/LFM2.5-350M-GGUF",
          "revision": "9969000761ce34de907bf20017cbfc3d52d6eaf9",
          "sizeBytes": 379217632,
          "sizeMeasured": true,
          "sha256": "be036a757295e550098b85e13f6af2735d0fa73b41e1156a40c7d8e8e32a5766",
          "sha256Measured": false,
          "quantization": "q8_0",
          "contextLength": 32768,
          "recommended": false,
          "supportsThinking": false,
          "draft": null
        },
        {
          "id": "lfm2.5-230m-q8_0",
          "name": "LFM2.5 230M",
          "fileName": "LFM2.5-230M-Q8_0.gguf",
          "hfRepo": "LiquidAI/LFM2.5-230M-GGUF",
          "revision": "cdf97bd8205908758f44aec508d68ac1aef98f5c",
          "sizeBytes": 246598496,
          "sizeMeasured": true,
          "sha256": "855be85429300602eda72958547614703541b7d6dd965a8f8f6052b85a7aa935",
          "sha256Measured": true,
          "quantization": "q8_0",
          "contextLength": 32768,
          "recommended": false,
          "supportsThinking": false,
          "draft": null
        }
      ]
    }
  ],
  "localAsr": {
    "whisper": [
      {
        "id": "base",
        "name": "Base",
        "fileName": "ggml-base.bin",
        "downloadUrl": "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base.bin",
        "sizeMb": 142,
        "sha256": "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        "expectedSizeBytes": 147951465,
        "recommended": true
      },
      {
        "id": "large",
        "name": "Large",
        "fileName": "ggml-large-v3.bin",
        "downloadUrl": "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-large-v3.bin",
        "sizeMb": 3000,
        "sha256": "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2",
        "expectedSizeBytes": 3095033483,
        "recommended": false
      },
      {
        "id": "medium",
        "name": "Medium",
        "fileName": "ggml-medium.bin",
        "downloadUrl": "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-medium.bin",
        "sizeMb": 1500,
        "sha256": "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208",
        "expectedSizeBytes": 1533763059,
        "recommended": false
      },
      {
        "id": "small",
        "name": "Small",
        "fileName": "ggml-small.bin",
        "downloadUrl": "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small.bin",
        "sizeMb": 466,
        "sha256": "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        "expectedSizeBytes": 487601967,
        "recommended": false
      },
      {
        "id": "tiny",
        "name": "Tiny",
        "fileName": "ggml-tiny.bin",
        "downloadUrl": "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-tiny.bin",
        "sizeMb": 75,
        "sha256": "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
        "expectedSizeBytes": 77691713,
        "recommended": false
      },
      {
        "id": "turbo",
        "name": "Turbo",
        "fileName": "ggml-large-v3-turbo.bin",
        "downloadUrl": "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-large-v3-turbo.bin",
        "sizeMb": 1600,
        "sha256": "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69",
        "expectedSizeBytes": 1624555275,
        "recommended": false
      },
      {
        "id": "kb-whisper-tiny",
        "name": "KB Whisper Tiny (Swedish, Q5)",
        "fileName": "ggml-kb-whisper-tiny-q5_0.bin",
        "downloadUrl": "https://huggingface.co/KBLab/kb-whisper-tiny/resolve/76d796af43a50fa34321efa562c9b9887a187463/ggml-model-q5_0.bin",
        "sizeMb": 30,
        "sha256": "98d46b7d23e5528d006e8a42e29eb0cb39b44bed94e1329f10f57d1fd15c658b",
        "expectedSizeBytes": 29875738,
        "recommended": false
      },
      {
        "id": "kb-whisper-base",
        "name": "KB Whisper Base (Swedish, Q5)",
        "fileName": "ggml-kb-whisper-base-q5_0.bin",
        "downloadUrl": "https://huggingface.co/KBLab/kb-whisper-base/resolve/1499d2d2f0c7ed545bd6f2eec85287cf8d8c8b38/ggml-model-q5_0.bin",
        "sizeMb": 55,
        "sha256": "aead29b356bca8840e72a8dc2286e2d69e6702639751a1e60cb3c8eacefec546",
        "expectedSizeBytes": 55295450,
        "recommended": false
      }
    ],
    "sherpa": [
      {
        "id": "nemotron-3.5-asr-streaming-0.6b",
        "name": "Nemotron 3.5 ASR Streaming 0.6B",
        "downloadUrl": "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemotron-3.5-asr-streaming-0.6b-560ms-int8-2026-06-11.tar.bz2",
        "extractDir": "sherpa-onnx-nemotron-3.5-asr-streaming-0.6b-560ms-int8-2026-06-11",
        "sizeMb": 650,
        "sha256": "c6bf5e0df765f9d5b43bc9e0536d4b4b3e7d40bdf5ecf13e45f134c51c05ae3a",
        "expectedSizeBytes": 475271763,
        "maxExtractedSizeBytes": 838860800,
        "language": "multilingual",
        "supportedLanguages": [
          "en",
          "es",
          "fr",
          "it",
          "pt",
          "nl",
          "de",
          "tr",
          "ru",
          "ar",
          "hi",
          "ja",
          "ko",
          "vi",
          "uk"
        ],
        "streaming": true
      },
      {
        "id": "nemotron-speech-streaming-en-0.6b",
        "name": "Nemotron Speech Streaming EN 0.6B",
        "downloadUrl": "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemotron-speech-streaming-en-0.6b-560ms-int8-2026-04-25.tar.bz2",
        "extractDir": "sherpa-onnx-nemotron-speech-streaming-en-0.6b-560ms-int8-2026-04-25",
        "sizeMb": 632,
        "sha256": "78e2b79fcf7271553a74402a76b771b09ea40117a39566a79f52235b23db6358",
        "expectedSizeBytes": 463945051,
        "maxExtractedSizeBytes": 786432000,
        "language": "en",
        "supportedLanguages": [
          "en"
        ],
        "streaming": true
      },
      {
        "id": "parakeet-tdt-0.6b-v3",
        "name": "Parakeet TDT 0.6B",
        "downloadUrl": "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2",
        "extractDir": "sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8",
        "sizeMb": 680,
        "sha256": "5793d0fd397c5778d2cf2126994d58e9d56b1be7c04d13c7a15bb1b4eafb16bf",
        "expectedSizeBytes": 487170055,
        "maxExtractedSizeBytes": 838860800,
        "language": "multilingual",
        "supportedLanguages": [
          "bg",
          "hr",
          "cs",
          "da",
          "nl",
          "en",
          "et",
          "fi",
          "fr",
          "de",
          "el",
          "hu",
          "it",
          "lv",
          "lt",
          "mt",
          "pl",
          "pt",
          "ro",
          "sk",
          "sl",
          "es",
          "sv",
          "ru",
          "uk"
        ],
        "streaming": false
      },
      {
        "id": "parakeet-unified-en-0.6b",
        "name": "Parakeet Unified EN 0.6B",
        "downloadUrl": "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-unified-en-0.6b-int8-non-streaming.tar.bz2",
        "extractDir": "sherpa-onnx-nemo-parakeet-unified-en-0.6b-int8-non-streaming",
        "sizeMb": 631,
        "sha256": "99f63605b3a85a54c250c0869670a687b7d6598a47bf2421515e1f839a76e150",
        "expectedSizeBytes": 501350460,
        "maxExtractedSizeBytes": 786432000,
        "language": "en",
        "supportedLanguages": [
          "en"
        ],
        "streaming": false
      }
    ],
    "diarization": [
      {
        "id": "3dspeaker-campplus-en",
        "name": "CAM++ Speaker Embedding (English)",
        "downloadUrl": "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx",
        "extractDir": null,
        "fileName": "3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx",
        "sizeMb": 28.2,
        "role": "embedding",
        "sha256": "357a834f702b80161e5b981182c038e18553c1f2ca752ed6cec2052365d4129b",
        "expectedSizeBytes": 29596978,
        "maxExtractedSizeBytes": null
      },
      {
        "id": "sherpa-onnx-pyannote-segmentation-3-0",
        "name": "Pyannote Segmentation 3.0",
        "downloadUrl": "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2",
        "extractDir": "sherpa-onnx-pyannote-segmentation-3-0",
        "fileName": null,
        "sizeMb": 6.6,
        "role": "segmentation",
        "sha256": "24615ee884c897d9d2ba09bb4d30da6bb1b15e685065962db5b02e76e4996488",
        "expectedSizeBytes": 6958444,
        "maxExtractedSizeBytes": 33554432
      }
    ]
  }
} as const satisfies ProviderRegistry;

/**
 * Look up a transcription provider, or `undefined`.
 *
 * Callers must handle `undefined` rather than substituting a default. Upstream
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
 * Look up a credential slot, or `undefined`.
 *
 * Callers must handle `undefined`. A screen that invented a slot name would be
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

