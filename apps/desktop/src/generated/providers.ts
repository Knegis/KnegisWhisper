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
export const WHISPER_MODEL_IDS = ["base", "large", "medium", "small", "tiny", "turbo"] as const;
export type WhisperModelId = (typeof WHISPER_MODEL_IDS)[number];

/** Local sherpa-onnx models (Parakeet batch, Nemotron streaming). */
export const SHERPA_MODEL_IDS = ["nemotron-3.5-asr-streaming-0.6b", "nemotron-speech-streaming-en-0.6b", "parakeet-tdt-0.6b-v3", "parakeet-unified-en-0.6b"] as const;
export type SherpaModelId = (typeof SHERPA_MODEL_IDS)[number];

/** OS credential-store slots, in registry order. Twenty: twelve providers, five per-feature custom endpoints and three enterprise connectors. */
export const CREDENTIAL_SLOTS = ["openai", "anthropic", "gemini", "groq", "berget", "xai", "mistral", "openrouter", "tinfoil", "corti", "note-formatting-custom", "translation-custom", "dictation-agent-custom", "dictation-agent-vision-custom", "chat-agent-custom", "bedrock", "azure", "vertex", "assemblyai", "deepgram"] as const;
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
  readonly fileName: string;
  readonly sizeBytes: number | null;
  readonly sizeMeasured: boolean;
}

export interface LocalLlmModel {
  readonly id: string;
  readonly name: string;
  readonly fileName: string;
  readonly hfRepo: string;
  readonly sizeBytes: number | null;
  /**
   * Whether `sizeBytes` was measured against the publisher or copied from
   * upstream's declaration. Upstream declared a size for every model and was
   * wrong about eighteen of them, so a non-null size is not on its own a
   * trustworthy one; the installer holds a measured size to exact equality and
   * a declared one to 5%.
   */
  readonly sizeMeasured: boolean;
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
   * what it may extract to. Null on the two Nemotron streaming models, which
   * nothing can install: sherpa_models::install refuses an unpinned model
   * rather than downloading something unauthenticated.
   */
  readonly sha256: string | null;
  readonly expectedSizeBytes: number | null;
  readonly maxExtractedSizeBytes: number | null;
  readonly language: string | null;
  readonly supportedLanguages: readonly string[] | null;
  /** True for the Nemotron streaming runtimes; Parakeet models are batch. */
  readonly streaming: boolean;
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
          "sizeBytes": 6169341984,
          "sizeMeasured": true,
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
          "sizeBytes": 3013027808,
          "sizeMeasured": true,
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
          "sizeBytes": 1396198496,
          "sizeMeasured": true,
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
          "sizeBytes": 5027783488,
          "sizeMeasured": true,
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
          "sizeBytes": 5851112224,
          "sizeMeasured": true,
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
          "sizeBytes": 2497280256,
          "sizeMeasured": true,
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
          "sizeBytes": 1834426016,
          "sizeMeasured": true,
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
          "sizeBytes": 19762149024,
          "sizeMeasured": true,
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
          "sizeBytes": 1285494304,
          "sizeMeasured": true,
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
          "sizeBytes": 2438740384,
          "sizeMeasured": true,
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
          "sizeBytes": 4683074240,
          "sizeMeasured": true,
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
          "sizeBytes": 5444831936,
          "sizeMeasured": true,
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
          "sizeBytes": 7477208320,
          "sizeMeasured": true,
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
          "sizeBytes": 4372812000,
          "sizeMeasured": true,
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
          "sizeBytes": 5136175328,
          "sizeMeasured": true,
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
          "sizeBytes": 807694464,
          "sizeMeasured": true,
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
          "sizeBytes": 2019377696,
          "sizeMeasured": true,
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
          "sizeBytes": 4920739232,
          "sizeMeasured": true,
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
          "sizeBytes": 12109566624,
          "sizeMeasured": true,
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
          "sizeBytes": 19598489952,
          "sizeMeasured": true,
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
          "sizeBytes": 17651001568,
          "sizeMeasured": true,
          "quantization": "q4_0",
          "contextLength": 262144,
          "recommended": false,
          "supportsThinking": false,
          "draft": {
            "hfRepo": "unsloth/gemma-4-31B-it-qat-GGUF",
            "fileName": "mtp-gemma-4-31B-it.gguf",
            "sizeBytes": 279955968,
            "sizeMeasured": true
          }
        },
        {
          "id": "gemma-4-26b-a4b-it-q4_k_m",
          "name": "Gemma 4 26B MoE",
          "fileName": "google_gemma-4-26B-A4B-it-Q4_K_M.gguf",
          "hfRepo": "bartowski/google_gemma-4-26B-A4B-it-GGUF",
          "sizeBytes": 17035039872,
          "sizeMeasured": true,
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
          "sizeBytes": 14439363584,
          "sizeMeasured": true,
          "quantization": "q4_0",
          "contextLength": 262144,
          "recommended": false,
          "supportsThinking": false,
          "draft": {
            "hfRepo": "unsloth/gemma-4-26B-A4B-it-qat-GGUF",
            "fileName": "mtp-gemma-4-26B-A4B-it.gguf",
            "sizeBytes": 251939328,
            "sizeMeasured": true
          }
        },
        {
          "id": "gemma-4-e4b-it-q4_k_m",
          "name": "Gemma 4 E4B",
          "fileName": "google_gemma-4-E4B-it-Q4_K_M.gguf",
          "hfRepo": "bartowski/google_gemma-4-E4B-it-GGUF",
          "sizeBytes": 5405170144,
          "sizeMeasured": true,
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
          "sizeBytes": 5154941280,
          "sizeMeasured": true,
          "quantization": "q4_0",
          "contextLength": 262144,
          "recommended": true,
          "supportsThinking": false,
          "draft": {
            "hfRepo": "unsloth/gemma-4-E4B-it-qat-GGUF",
            "fileName": "mtp-gemma-4-E4B-it.gguf",
            "sizeBytes": 59678016,
            "sizeMeasured": true
          }
        },
        {
          "id": "gemma-4-e2b-it-q4_k_m",
          "name": "Gemma 4 E2B",
          "fileName": "google_gemma-4-E2B-it-Q4_K_M.gguf",
          "hfRepo": "bartowski/google_gemma-4-E2B-it-GGUF",
          "sizeBytes": 3462680032,
          "sizeMeasured": true,
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
          "sizeBytes": 3349516256,
          "sizeMeasured": true,
          "quantization": "q4_0",
          "contextLength": 262144,
          "recommended": false,
          "supportsThinking": false,
          "draft": {
            "hfRepo": "unsloth/gemma-4-E2B-it-qat-GGUF",
            "fileName": "mtp-gemma-4-E2B-it.gguf",
            "sizeBytes": 59235648,
            "sizeMeasured": true
          }
        },
        {
          "id": "gemma-3-12b-it-q4_k_m",
          "name": "Gemma 3 12B",
          "fileName": "google_gemma-3-12b-it-Q4_K_M.gguf",
          "hfRepo": "bartowski/google_gemma-3-12b-it-GGUF",
          "sizeBytes": 7300575264,
          "sizeMeasured": true,
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
          "sizeBytes": 2489758112,
          "sizeMeasured": true,
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
          "sizeBytes": 806058496,
          "sizeMeasured": true,
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
          "sizeBytes": 730895168,
          "sizeMeasured": true,
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
          "sizeBytes": 5155564768,
          "sizeMeasured": true,
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
          "sizeBytes": 1563668704,
          "sizeMeasured": true,
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
          "sizeBytes": 379217632,
          "sizeMeasured": true,
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
          "sizeBytes": 246598496,
          "sizeMeasured": true,
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
        "downloadUrl": "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        "sizeMb": 142,
        "sha256": "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        "expectedSizeBytes": 147951465,
        "recommended": true
      },
      {
        "id": "large",
        "name": "Large",
        "fileName": "ggml-large-v3.bin",
        "downloadUrl": "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin",
        "sizeMb": 3000,
        "sha256": "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2",
        "expectedSizeBytes": 3095033483,
        "recommended": false
      },
      {
        "id": "medium",
        "name": "Medium",
        "fileName": "ggml-medium.bin",
        "downloadUrl": "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin",
        "sizeMb": 1500,
        "sha256": "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208",
        "expectedSizeBytes": 1533763059,
        "recommended": false
      },
      {
        "id": "small",
        "name": "Small",
        "fileName": "ggml-small.bin",
        "downloadUrl": "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        "sizeMb": 466,
        "sha256": "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        "expectedSizeBytes": 487601967,
        "recommended": false
      },
      {
        "id": "tiny",
        "name": "Tiny",
        "fileName": "ggml-tiny.bin",
        "downloadUrl": "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        "sizeMb": 75,
        "sha256": "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
        "expectedSizeBytes": 77691713,
        "recommended": false
      },
      {
        "id": "turbo",
        "name": "Turbo",
        "fileName": "ggml-large-v3-turbo.bin",
        "downloadUrl": "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin",
        "sizeMb": 1600,
        "sha256": "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69",
        "expectedSizeBytes": 1624555275,
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
        "sha256": null,
        "expectedSizeBytes": null,
        "maxExtractedSizeBytes": null,
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
        "sha256": null,
        "expectedSizeBytes": null,
        "maxExtractedSizeBytes": null,
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

