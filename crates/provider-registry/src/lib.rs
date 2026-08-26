//! Provider and model registry.
//!
//! `config/provider-registry.json` is the single source of truth for provider
//! and model identity. This crate reads it; the TypeScript frontend reads the
//! same file through generated types. No provider id is written twice.
//!
//! That matters because upstream duplicated its provider list across four
//! places — `secretKeys.js`, the `BYOK_KEY_BRIDGES` array inlined in
//! `preload.js`, `PROVIDER_REGISTRY`, and `CLOUD_CHAT_PROVIDERS` — and kept them
//! in sync with a test. A missed entry there was a real bug: Berget fell through
//! `CLOUD_CHAT_PROVIDERS` and got silently classified as a local model.
//!
//! See `docs/ARCHITECTURE.md` and `docs/INTEGRATION_PARITY.md`.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use kw_security::KeyRef;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The registry data, embedded at compile time.
const REGISTRY_JSON: &str = include_str!("../../../config/provider-registry.json");

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("the embedded provider registry is malformed: {0}")]
    Malformed(String),

    #[error("no provider with id {0:?}")]
    UnknownProvider(String),

    #[error("provider {provider:?} has no model {model:?}")]
    UnknownModel { provider: String, model: String },
}

/// How a provider authenticates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthScheme {
    /// `Authorization: Bearer <key>`.
    Bearer,
    /// Azure's `api-key` header.
    AzureApiKey,
    /// A token minted by an OAuth2 client-credentials exchange.
    Oauth2ClientCredentials,
    /// Reached through an attested enclave client rather than a plain request.
    Attested,
    /// The endpoint requires no credential.
    None,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TranscriptionModel {
    pub id: String,
    pub name: String,
    /// Whether this model supports realtime streaming. Only four upstream models
    /// do, so this must not be assumed from the provider.
    pub streaming: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionProvider {
    pub id: String,
    pub name: String,
    pub base_url: Option<String>,
    pub auth_scheme: AuthScheme,
    pub credential_slot: String,
    pub allow_custom_model_id: bool,
    /// `None` means genuinely uncapped. Explicit on every provider so dispatch
    /// never has to special-case one — which is what upstream did for Berget.
    pub max_upload_bytes: Option<u64>,
    #[serde(default)]
    pub batch_model: Option<String>,
    pub models: Vec<TranscriptionModel>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningModel {
    pub id: String,
    pub name: String,
    /// `max_tokens` or `max_completion_tokens`; providers disagree.
    pub token_param: String,
    pub supports_temperature: bool,
    pub supports_thinking: bool,
    pub supports_vision: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningProvider {
    pub id: String,
    pub name: String,
    pub auth_scheme: AuthScheme,
    pub credential_slot: String,
    pub allow_custom_model_id: bool,
    pub models: Vec<ReasoningModel>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NamedModel {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnterpriseProvider {
    pub id: String,
    pub name: String,
    /// Enterprise credentials are multi-field (region/profile/key,
    /// endpoint/deployment/version), so there is no single credential slot.
    pub credential_shape: String,
    pub allow_custom_model_id: bool,
    /// Azure ships no model list; the deployment name selects the model.
    pub deployment_name_driven: bool,
    pub models: Vec<NamedModel>,
}

/// A speculative-decoding drafter downloaded alongside the main GGUF.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftModel {
    pub hf_repo: String,
    pub file_name: String,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalLlmModel {
    pub id: String,
    pub name: String,
    pub file_name: String,
    pub hf_repo: String,
    pub size_bytes: Option<u64>,
    pub quantization: Option<String>,
    pub context_length: Option<u32>,
    pub recommended: bool,
    pub supports_thinking: bool,
    pub draft: Option<DraftModel>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalLlmProvider {
    pub id: String,
    pub name: String,
    pub base_url: Option<String>,
    pub models: Vec<LocalLlmModel>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperModel {
    pub id: String,
    pub name: String,
    pub file_name: String,
    pub download_url: String,
    pub size_mb: Option<f64>,
    pub expected_size_bytes: Option<u64>,
    pub recommended: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SherpaModel {
    pub id: String,
    pub name: String,
    pub download_url: String,
    pub extract_dir: String,
    pub size_mb: Option<f64>,
    pub language: Option<String>,
    pub supported_languages: Option<Vec<String>>,
    /// True for the Nemotron streaming runtimes; the Parakeet models are batch.
    pub streaming: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LocalAsr {
    pub whisper: Vec<WhisperModel>,
    pub sherpa: Vec<SherpaModel>,
}

/// What a credential slot is for.
///
/// The distinction a UI has to make. A `Provider` key is enough on its own to
/// try that service; a `CustomEndpoint` key configures nothing without the base
/// URL that lives with the feature it belongs to, so a screen that stores one
/// must say so rather than implying the key was all that was needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum CredentialKind {
    /// A named service, e.g. OpenAI.
    Provider,
    /// A user-supplied OpenAI-compatible endpoint for one feature.
    CustomEndpoint,
    /// An enterprise connector whose whole multi-field configuration is stored
    /// as one JSON document. Configured on its own screen, not on Providers.
    Enterprise,
}

/// One slot in the OS credential store.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialSlot {
    /// The slot name, which is also the credential-store key.
    pub slot: String,
    /// A label for the settings screen.
    pub name: String,
    pub kind: CredentialKind,
    /// Upstream's environment-variable name.
    ///
    /// Recorded so the OpenWhispr importer can recognise a key stored under the
    /// old name. This application does not read the environment for secrets.
    pub env: String,
    /// Which parts of the application use this key: `transcription`,
    /// `reasoning`, or neither. Derived by the generator from the provider lists
    /// rather than restated, so it cannot fall out of step with them.
    pub used_for: Vec<String>,
    /// For a custom endpoint, which feature it belongs to.
    #[serde(default)]
    pub feature: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Registry {
    /// Every credential slot, including the eleven that belong to no cloud
    /// provider list: `openrouter`, the five custom endpoints, the three
    /// enterprise connectors, and `assemblyai`/`deepgram`, which exist for a
    /// meeting client rather than a transcription or reasoning one.
    ///
    /// Derived from upstream's `secretKeys` rather than from the provider lists
    /// below: `openrouter` has no pinned models and the five custom endpoints
    /// have no provider entry at all, so deriving the set from providers gave
    /// nine slots where the integration manifest had always listed fifteen.
    pub credentials: Vec<CredentialSlot>,
    pub transcription: Vec<TranscriptionProvider>,
    pub reasoning: Vec<ReasoningProvider>,
    pub enterprise: Vec<EnterpriseProvider>,
    pub local_llm: Vec<LocalLlmProvider>,
    pub local_asr: LocalAsr,
}

impl Registry {
    /// Look up a transcription provider, failing closed on an unknown id.
    ///
    /// Deliberately not falling back to a default: upstream's terminal fallback
    /// routed any unrecognised provider to OpenAI with the OpenAI credential,
    /// which meant a typo silently sent the user's audio to a provider they did
    /// not choose.
    pub fn transcription_provider(
        &self,
        id: &str,
    ) -> Result<&TranscriptionProvider, RegistryError> {
        self.transcription
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| RegistryError::UnknownProvider(id.to_owned()))
    }

    pub fn reasoning_provider(&self, id: &str) -> Result<&ReasoningProvider, RegistryError> {
        self.reasoning
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| RegistryError::UnknownProvider(id.to_owned()))
    }

    /// Resolve a transcription model, failing closed if it is not offered by
    /// this provider and the provider does not permit custom model ids.
    pub fn resolve_transcription_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<String, RegistryError> {
        let provider = self.transcription_provider(provider_id)?;
        if provider.models.iter().any(|m| m.id == model_id) {
            return Ok(model_id.to_owned());
        }
        if provider.allow_custom_model_id && !model_id.trim().is_empty() {
            return Ok(model_id.to_owned());
        }
        Err(RegistryError::UnknownModel {
            provider: provider_id.to_owned(),
            model: model_id.to_owned(),
        })
    }

    /// The credential slot a transcription provider authenticates with.
    pub fn transcription_key_ref(&self, provider_id: &str) -> Result<KeyRef, RegistryError> {
        let provider = self.transcription_provider(provider_id)?;
        KeyRef::new(&provider.credential_slot).ok_or_else(|| {
            RegistryError::Malformed(format!(
                "provider {provider_id:?} has an invalid credential slot {:?}",
                provider.credential_slot
            ))
        })
    }

    /// Every credential slot the application can store a key for.
    ///
    /// All twenty, not just the ones a transcription or reasoning provider
    /// names. It used to be derived from the provider lists, which meant
    /// `openrouter` and the five custom endpoints were declared in the
    /// integration manifest and invisible here, so a settings screen would
    /// have had to write those six ids out by hand.
    #[must_use]
    pub fn credential_slots(&self) -> BTreeSet<&str> {
        self.credentials.iter().map(|c| c.slot.as_str()).collect()
    }

    /// Look up one slot, failing closed on an unknown name.
    pub fn credential_slot(&self, slot: &str) -> Result<&CredentialSlot, RegistryError> {
        self.credentials
            .iter()
            .find(|c| c.slot == slot)
            .ok_or_else(|| RegistryError::UnknownProvider(slot.to_owned()))
    }

    /// Transcription models that support realtime streaming.
    #[must_use]
    pub fn streaming_transcription_models(&self) -> Vec<(&str, &str)> {
        self.transcription
            .iter()
            .flat_map(|p| {
                p.models
                    .iter()
                    .filter(|m| m.streaming)
                    .map(move |m| (p.id.as_str(), m.id.as_str()))
            })
            .collect()
    }
}

/// The embedded registry.
///
/// Returns a `Result` rather than panicking so a malformed registry surfaces as
/// an error the caller can report. A test asserts it parses, so in practice this
/// is `Ok` in any build that passed CI.
pub fn registry() -> Result<&'static Registry, &'static RegistryError> {
    static PARSED: OnceLock<Result<Registry, RegistryError>> = OnceLock::new();
    PARSED
        .get_or_init(|| {
            serde_json::from_str::<Registry>(REGISTRY_JSON)
                .map_err(|e| RegistryError::Malformed(e.to_string()))
        })
        .as_ref()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn reg() -> &'static Registry {
        registry().expect("the embedded provider registry must parse")
    }

    #[test]
    fn registry_parses() {
        // If this fails, config/provider-registry.json is malformed or the
        // structs have drifted from it. Everything else here depends on it.
        assert!(registry().is_ok());
    }

    #[test]
    fn counts_match_the_phase_0_audit() {
        let r = reg();
        assert_eq!(r.transcription.len(), 7, "transcription providers");
        assert_eq!(r.reasoning.len(), 7, "reasoning providers");
        assert_eq!(r.enterprise.len(), 3, "enterprise providers");
        assert_eq!(r.local_llm.len(), 6, "local llm providers");
        assert_eq!(r.local_asr.whisper.len(), 6, "whisper models");
        assert_eq!(r.local_asr.sherpa.len(), 4, "sherpa models");

        let models: usize = r
            .transcription
            .iter()
            .map(|p| p.models.len())
            .sum::<usize>()
            + r.reasoning.iter().map(|p| p.models.len()).sum::<usize>()
            + r.enterprise.iter().map(|p| p.models.len()).sum::<usize>()
            + r.local_llm.iter().map(|p| p.models.len()).sum::<usize>();
        assert_eq!(models, 96, "total provider-array models");
    }

    #[test]
    fn every_provider_the_brief_omitted_is_present() {
        let r = reg();
        for id in ["berget", "xai", "mistral"] {
            assert!(
                r.transcription_provider(id).is_ok(),
                "{id} must be a transcription provider"
            );
        }
        assert!(
            r.local_llm.iter().any(|p| p.id == "liquidai"),
            "liquidai must be a local llm provider"
        );
    }

    #[test]
    fn upload_caps_are_explicit_on_every_provider() {
        // The normalisation this registry exists to enforce: no provider relies
        // on a magic default. Berget is the only uncapped one, and that is data.
        let r = reg();
        let berget = r.transcription_provider("berget").unwrap();
        assert_eq!(berget.max_upload_bytes, None, "berget is uncapped upstream");

        for id in ["openai", "groq", "xai", "mistral", "corti", "tinfoil"] {
            let p = r.transcription_provider(id).unwrap();
            assert_eq!(
                p.max_upload_bytes,
                Some(25 * 1024 * 1024),
                "{id} should carry the 25 MiB BYOK cap"
            );
        }
    }

    #[test]
    fn unknown_provider_fails_closed_rather_than_defaulting_to_openai() {
        let r = reg();
        // Upstream returned provider:"openai" with keyRef:"openai" here, which
        // sent audio to a provider the user never chose.
        assert!(matches!(
            r.transcription_provider("totally-made-up"),
            Err(RegistryError::UnknownProvider(_))
        ));
        assert!(matches!(
            r.reasoning_provider(""),
            Err(RegistryError::UnknownProvider(_))
        ));
    }

    #[test]
    fn model_resolution_fails_closed_unless_custom_ids_are_allowed() {
        let r = reg();

        // A known model resolves.
        assert_eq!(
            r.resolve_transcription_model("openai", "whisper-1")
                .unwrap(),
            "whisper-1"
        );

        // OpenAI does not allow arbitrary model ids.
        assert!(matches!(
            r.resolve_transcription_model("openai", "not-a-real-model"),
            Err(RegistryError::UnknownModel { .. })
        ));

        // Berget does, which is a declared provider capability.
        assert!(
            r.transcription_provider("berget")
                .unwrap()
                .allow_custom_model_id
        );
        assert_eq!(
            r.resolve_transcription_model("berget", "KBLab/some-new-model")
                .unwrap(),
            "KBLab/some-new-model"
        );

        // But an empty id is never a valid custom id.
        assert!(matches!(
            r.resolve_transcription_model("berget", "   "),
            Err(RegistryError::UnknownModel { .. })
        ));
    }

    #[test]
    fn streaming_is_a_model_property_not_a_provider_one() {
        let r = reg();
        let mut streaming: Vec<&str> = r
            .streaming_transcription_models()
            .into_iter()
            .map(|(_, m)| m)
            .collect();
        streaming.sort_unstable();
        assert_eq!(
            streaming,
            [
                "corti-transcribe",
                "gpt-4o-mini-transcribe",
                "gpt-4o-transcribe",
                "voxtral-mini-4b-realtime"
            ],
            "exactly these four upstream models declare streaming"
        );

        // Berget offers three models and none of them stream, so a
        // provider-level assumption would be wrong.
        let berget = r.transcription_provider("berget").unwrap();
        assert_eq!(berget.models.len(), 3);
        assert!(berget.models.iter().all(|m| !m.streaming));
    }

    #[test]
    fn credential_slots_produce_valid_key_refs() {
        let r = reg();
        for p in &r.transcription {
            assert!(
                r.transcription_key_ref(&p.id).is_ok(),
                "provider {} has an unusable credential slot",
                p.id
            );
        }
        assert!(r.credential_slots().contains("berget"));
    }

    /// The mismatch this section exists to close.
    ///
    /// `config/integration-manifest.json` declares fifteen `credential:*`
    /// entries. The registry used to derive its slots from the provider lists
    /// and produce nine, so six slots existed in one source of truth and not
    /// the other — and any screen offering all fifteen would have had to name
    /// those six itself. This asserts the two agree.
    #[test]
    fn every_slot_upstream_had_is_declared_here() {
        let r = reg();
        let slots = r.credential_slots();

        // Fifteen from upstream's secretKeys, plus three this project adds for
        // the enterprise connectors -- upstream kept those in loose settings
        // fields, so there is nothing to inherit -- plus two more for
        // AssemblyAI and Deepgram meeting streaming, which upstream also never
        // needed: it minted a realtime token from a removed vendor service
        // instead of asking the user for a key.
        let non_enterprise = r
            .credentials
            .iter()
            .filter(|c| c.kind != CredentialKind::Enterprise)
            .count();
        assert_eq!(
            non_enterprise, 17,
            "expected the fifteen slots in the integration manifest plus assemblyai and \
             deepgram, got {slots:?}"
        );
        assert_eq!(slots.len(), 20, "{slots:?}");

        // The six that belong to no provider, and would therefore be invisible
        // if the set were derived from the provider lists.
        for orphan in [
            "openrouter",
            "note-formatting-custom",
            "translation-custom",
            "dictation-agent-custom",
            "dictation-agent-vision-custom",
            "chat-agent-custom",
        ] {
            assert!(slots.contains(orphan), "{orphan} is missing");
        }

        // And every slot must be a usable credential-store key.
        for c in &r.credentials {
            assert!(
                KeyRef::new(&c.slot).is_some(),
                "slot {:?} cannot be a credential-store key",
                c.slot
            );
        }
    }

    /// New relative to upstream, for the same reason the three enterprise
    /// slots are: `baseline.secretKeys` has neither, because upstream's
    /// meeting client minted a short-lived realtime token from the vendor's
    /// hosted token-broker rather than asking the user for a key. `usedFor`
    /// is empty rather than `["meeting-stream"]` -- that string is not a value
    /// `generate-provider-registry.mjs` derives, and widening its vocabulary
    /// to fit a label was rejected on purpose. The purpose lives in
    /// `config/integration-manifest.json`'s `credential:assemblyai` and
    /// `credential:deepgram` notes instead.
    #[test]
    fn assemblyai_and_deepgram_are_provider_slots_upstream_never_needed() {
        let r = reg();
        for (slot, env, name) in [
            ("assemblyai", "ASSEMBLYAI_API_KEY", "AssemblyAI"),
            ("deepgram", "DEEPGRAM_API_KEY", "Deepgram"),
        ] {
            let c = r
                .credential_slot(slot)
                .unwrap_or_else(|_| panic!("{slot} has no credential slot"));
            assert_eq!(c.kind, CredentialKind::Provider);
            assert_eq!(c.env, env);
            assert_eq!(c.name, name);
            assert!(c.feature.is_none());
            assert!(
                c.used_for.is_empty(),
                "{slot} belongs to no transcription or reasoning provider list"
            );
        }
    }

    /// A custom endpoint's key configures nothing on its own.
    ///
    /// The kind is what lets a screen say so. Without it the five custom slots
    /// look like five more providers, and storing a key for one would appear to
    /// have finished a job that has not started.
    #[test]
    fn a_custom_endpoint_is_distinguishable_from_a_provider() {
        let r = reg();

        let openai = r.credential_slot("openai").unwrap();
        assert_eq!(openai.kind, CredentialKind::Provider);
        assert!(openai.feature.is_none());
        assert_eq!(openai.used_for, ["transcription", "reasoning"]);

        let custom = r.credential_slot("chat-agent-custom").unwrap();
        assert_eq!(custom.kind, CredentialKind::CustomEndpoint);
        assert_eq!(custom.feature.as_deref(), Some("chat-agent"));
        assert!(
            custom.used_for.is_empty(),
            "a custom endpoint belongs to no provider list"
        );

        // OpenRouter is the awkward case: a real provider with no pinned models,
        // so it needs a key and appears in no provider list.
        let openrouter = r.credential_slot("openrouter").unwrap();
        assert_eq!(openrouter.kind, CredentialKind::Provider);
        assert!(openrouter.used_for.is_empty());
    }

    #[test]
    fn an_unknown_slot_fails_closed() {
        assert!(reg().credential_slot("not-a-slot").is_err());
    }

    /// Each enterprise connector has exactly one slot, named for it.
    ///
    /// The slot name is the provider id because `ChatRoute::Enterprise { id }`
    /// looks it up by that id. A mismatch would be a configuration stored where
    /// nothing reads it, which looks exactly like a configuration that works.
    #[test]
    fn every_enterprise_connector_has_a_slot_named_for_it() {
        let r = reg();
        for p in &r.enterprise {
            let slot = r
                .credential_slot(&p.id)
                .unwrap_or_else(|_| panic!("{} has no credential slot", p.id));
            assert_eq!(slot.kind, CredentialKind::Enterprise);
            assert_eq!(slot.feature.as_deref(), Some(p.id.as_str()));
            assert!(
                KeyRef::new(&slot.slot).is_some(),
                "{} is not a usable credential-store key",
                slot.slot
            );
        }
    }

    #[test]
    fn azure_is_deployment_name_driven_with_no_model_list() {
        let r = reg();
        let azure = r
            .enterprise
            .iter()
            .find(|p| p.id == "azure")
            .expect("azure must be present");
        assert!(azure.deployment_name_driven);
        assert!(azure.models.is_empty());
    }

    #[test]
    fn speculative_decoding_metadata_survived_the_port() {
        let r = reg();
        let drafters: Vec<&str> = r
            .local_llm
            .iter()
            .flat_map(|p| p.models.iter())
            .filter(|m| m.draft.is_some())
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(
            drafters.len(),
            4,
            "exactly four Gemma QAT models have drafters"
        );
        assert!(drafters.iter().all(|id| id.contains("gemma")));
    }

    #[test]
    fn nemotron_models_are_the_streaming_asr_runtimes() {
        let r = reg();
        let streaming: Vec<&str> = r
            .local_asr
            .sherpa
            .iter()
            .filter(|m| m.streaming)
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(streaming.len(), 2);
        assert!(streaming.iter().all(|id| id.contains("nemotron")));
    }
}
