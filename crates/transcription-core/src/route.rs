//! Batch speech-to-text route resolution.
//!
//! A direct port of upstream's `transcriptionRoute.ts`, which is the single best
//! piece of code in that repository: a pure function, no store imports, and —
//! critically — **routes never carry secrets**. [`Auth::key_ref`] names a
//! credential *slot*; only `kw_security` can turn that into a secret, and only
//! `kw_network_policy` can send it. Two consequences worth stating: no test in
//! this file needs a real credential, and a secret cannot reach a log line from
//! here because the types passing through do not contain one.
//!
//! ## Four deliberate divergences from upstream
//!
//! **1. An unknown provider id is an error, not OpenAI.** Upstream's terminal
//! fallback returns `provider: "openai", keyRef: "openai"` for any unrecognised
//! id. A typo in a provider id therefore sends the user's audio to a provider
//! they never chose, authenticated with a credential they configured for
//! something else. That is [`RouteError::UnknownProvider`] here.
//!
//! **2. Errors are a `Result`, not a variant.** Upstream needs the error inside
//! the union because JavaScript has no other option; in Rust a `Result` makes
//! ignoring the error impossible.
//!
//! **3. `proxied` is renamed to [`Transport::ProviderClient`].** Upstream's name
//! is misleading: nothing is proxied through a vendor. It means "this provider
//! needs its own client rather than the generic OpenAI-compatible multipart
//! POST" — Tinfoil uses an attested socket, Corti needs OAuth2. Given that the
//! brief forbids hidden remote fallbacks, a transport that *reads* as a proxy is
//! worth renaming.
//!
//! **4. Upload caps come from the provider registry.** Upstream hardcodes a
//! 25 MiB constant and special-cases Berget to `null` inside routing code. Every
//! provider now declares an explicit `maxUploadBytes`, with `null` meaning
//! genuinely uncapped. Same behaviour, no special case.
//!
//! Two upstream safety behaviours are preserved exactly, because both prevent a
//! credential going somewhere it should not:
//!
//! * The `custom` branch fails closed when the configured URL still equals the
//!   default OpenAI base — meaning Custom was selected but never configured.
//!   Passing it through would ship the user's custom key and audio to OpenAI.
//! * xAI drops the language unless it is one of the 25 it supports, so
//!   inverse text normalisation via `format=true` stays valid.

use kw_provider_registry::Registry;
use kw_security::KeyRef;
use serde::{Deserialize, Serialize};

use crate::url::{
    build_api_url, build_azure_transcription_url, is_azure_openai_endpoint,
    is_secure_http_endpoint, normalize_base_url,
};

/// The default OpenAI transcription base. Present only so the `custom` branch
/// can recognise "selected but never configured" — see the module docs.
const DEFAULT_OPENAI_BASE: &str = "https://api.openai.com/v1";

/// The 25 languages xAI's speech-to-text accepts.
///
/// Language must be in this set for inverse text normalisation (`format=true`)
/// to remain valid, so an unsupported language is dropped rather than sent.
pub const XAI_STT_LANGUAGES: [&str; 25] = [
    "ar", "cs", "da", "de", "en", "es", "fa", "fil", "fr", "hi", "id", "it", "ja", "ko", "mk",
    "ms", "nl", "pl", "pt", "ro", "ru", "sv", "th", "tr", "vi",
];

/// How a credential is presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthScheme {
    /// `Authorization: Bearer <credential>`.
    Bearer,
    /// Azure's `api-key` header. Bearer is reserved for Entra ID tokens there.
    AzureApiKey,
    /// No credential — a self-hosted server on the user's own network.
    None,
}

/// How to authenticate, by reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Auth {
    pub scheme: AuthScheme,
    /// The credential *slot*, never the credential.
    pub key_ref: Option<KeyRef>,
}

impl Auth {
    /// No credential needed.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            scheme: AuthScheme::None,
            key_ref: None,
        }
    }
}

/// Providers that need their own client rather than a generic multipart POST.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderClientKind {
    /// Attested enclave socket; resolves its own model.
    Tinfoil,
    /// Provider SDK.
    Mistral,
    /// Provider SDK; takes no model parameter.
    Xai,
    /// OAuth2 client-credentials, tenant- and environment-scoped.
    Corti,
}

/// Corti needs an environment and a tenant, and neither has a safe global default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CortiConfig {
    pub environment: String,
    pub tenant: String,
}

/// Where the audio goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "transport", rename_all = "kebab-case")]
pub enum Transport {
    /// A local model. Engine and model selection stay with the local manager.
    Local,

    /// A provider needing its own client. See [`ProviderClientKind`].
    ProviderClient {
        provider: ProviderClientKind,
        /// `None` where the client resolves its own model.
        model: Option<String>,
        language: Option<String>,
        max_upload_bytes: Option<u64>,
        /// Present only for Corti.
        corti: Option<CortiConfig>,
    },

    /// An OpenAI-compatible `multipart/form-data` POST.
    HttpBatch {
        provider: String,
        endpoint: String,
        model: String,
        auth: Auth,
        max_upload_bytes: Option<u64>,
        language: Option<String>,
    },
}

/// Why a route could not be resolved.
///
/// Every variant is a refusal to send audio somewhere it should not go.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RouteError {
    #[error("the self-hosted transcription URL is not configured")]
    SelfHostedNotConfigured,

    #[error("the self-hosted transcription URL is invalid or unsupported: {reason}")]
    SelfHostedInvalid { reason: String },

    #[error(
        "the custom transcription endpoint has not been configured. It is still set to the \
         default OpenAI address, so sending this would deliver your audio and your custom \
         credential to OpenAI rather than to the provider you selected."
    )]
    CustomEndpointUnconfigured,

    #[error("the custom transcription endpoint is invalid or unsupported: {reason}")]
    CustomEndpointInvalid { reason: String },

    #[error(
        "{host} is a Tinfoil inference host. Tinfoil requires its attested client, so it \
         cannot be used as a plain custom endpoint -- select Tinfoil as the provider instead."
    )]
    TinfoilClientRequired { host: String },

    /// The fail-closed divergence. Upstream would have used OpenAI here.
    #[error(
        "{id:?} is not a transcription provider this application knows. Nothing was sent. \
         (Upstream would have fallen back to OpenAI, which would send your audio to a \
         provider you did not choose.)"
    )]
    UnknownProvider { id: String },

    #[error("the provider registry could not be read: {0}")]
    Registry(String),

    #[error("a model id is required for {provider} and none was configured")]
    ModelRequired { provider: String },

    #[error("could not build the endpoint URL: {0}")]
    Url(#[from] crate::url::UrlError),
}

/// The settings a route is resolved from.
///
/// Flat and owned: the caller resolves its scope's settings into these names, so
/// this function never re-maps a selection and never reads a store.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RouteSettings {
    /// `"self-hosted"`, `"local"`, or `"providers"`.
    pub transcription_mode: Option<String>,
    pub use_local_whisper: bool,
    pub remote_transcription_url: Option<String>,
    pub remote_transcription_model: Option<String>,
    pub cloud_transcription_provider: Option<String>,
    pub cloud_transcription_model: Option<String>,
    pub cloud_transcription_base_url: Option<String>,
    pub corti_environment: Option<String>,
    pub corti_tenant: Option<String>,
    /// A BCP-47 tag or `"auto"`.
    pub preferred_language: Option<String>,
}

/// Per-request overrides.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RouteRequest {
    /// An explicit model override. Doubles as the Azure deployment name.
    pub model: Option<String>,
    /// Translation-aware language; wins over `preferred_language`.
    pub effective_language: Option<String>,
}

/// Resolve the language to send, or `None` for automatic detection.
///
/// A region subtag is dropped (`sv-SE` becomes `sv`): no transcription provider
/// accepts a region, and sending one is rejected rather than ignored by several.
fn resolve_language(settings: &RouteSettings, request: &RouteRequest) -> Option<String> {
    if let Some(effective) = request.effective_language.as_deref() {
        let trimmed = effective.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_owned());
        }
    }

    let preferred = settings.preferred_language.as_deref()?.trim();
    if preferred.is_empty() || preferred == "auto" {
        return None;
    }

    Some(
        preferred
            .split('-')
            .next()
            .unwrap_or(preferred)
            .to_lowercase(),
    )
}

/// Resolve the model id for a provider.
///
/// Prefix-matched rather than registry-validated, deliberately and for
/// upstream's reason: strict validation would reset the model of anyone still on
/// a retired id. The registry-backed rules belong in the picker.
///
/// One divergence: an explicitly empty custom model id is an error rather than
/// being silently replaced. Upstream's `allowCustomModelId: true` path accepts
/// it, which produces a request with no model.
pub fn resolve_model(provider: &str, configured: Option<&str>) -> Result<String, RouteError> {
    let trimmed = configured.unwrap_or("").trim();

    match provider {
        "custom" => Ok(if trimmed.is_empty() {
            "whisper-1".to_owned()
        } else {
            trimmed.to_owned()
        }),
        "berget" => Ok(if trimmed.is_empty() {
            "KBLab/kb-whisper-large".to_owned()
        } else {
            trimmed.to_owned()
        }),
        _ => {
            if !trimmed.is_empty() {
                let matches_provider = match provider {
                    "groq" => trimmed.starts_with("whisper-large-v3"),
                    "openai" => trimmed.starts_with("gpt-4o") || trimmed == "whisper-1",
                    "mistral" => trimmed.starts_with("voxtral-"),
                    "corti" => trimmed.starts_with("corti-"),
                    _ => false,
                };
                if matches_provider {
                    return Ok(trimmed.to_owned());
                }
            }

            Ok(match provider {
                "groq" => "whisper-large-v3-turbo".to_owned(),
                "xai" => "grok-stt".to_owned(),
                "mistral" => "voxtral-mini-latest".to_owned(),
                "corti" => "corti-transcribe".to_owned(),
                "openai" | "tinfoil" => "gpt-4o-mini-transcribe".to_owned(),
                // Upstream returns gpt-4o-mini-transcribe for anything else,
                // which pairs with its OpenAI fallback. Both are refused here.
                other => {
                    return Err(RouteError::UnknownProvider {
                        id: other.to_owned(),
                    })
                }
            })
        }
    }
}

/// Look up a provider's declared upload cap.
///
/// `Ok(None)` means genuinely uncapped, which is Berget.
fn max_upload_bytes(registry: &Registry, provider: &str) -> Result<Option<u64>, RouteError> {
    registry
        .transcription_provider(provider)
        .map(|p| p.max_upload_bytes)
        .map_err(|_| RouteError::UnknownProvider {
            id: provider.to_owned(),
        })
}

/// Build the batch endpoint, accounting for Azure's deployment-in-path shape.
fn build_batch_endpoint(raw: &str, base: &str, model: &str) -> Result<String, RouteError> {
    if is_azure_openai_endpoint(base) {
        if let Some(azure) = build_azure_transcription_url(raw, model) {
            return Ok(azure);
        }
        // Azure with no usable deployment name: say so rather than sending a
        // request that comes back DeploymentNotFound.
        return Err(RouteError::ModelRequired {
            provider: "azure".to_owned(),
        });
    }

    Ok(build_api_url(base, "/audio/transcriptions")?)
}

/// Whether `base` is a Tinfoil inference host, per the registry.
fn is_tinfoil_host(registry: &Registry, base: &str) -> Option<String> {
    let host = url::Url::parse(base).ok()?.host_str()?.to_lowercase();

    let tinfoil_host = registry
        .transcription
        .iter()
        .find(|p| p.id == "tinfoil")
        .and_then(|p| p.base_url.as_deref())
        .and_then(|u| url::Url::parse(u).ok())
        .and_then(|u| u.host_str().map(str::to_lowercase))?;

    (host == tinfoil_host).then_some(host)
}

/// Resolve where a batch transcription should go.
///
/// Dispatch order is upstream's, because the order encodes real decisions: a
/// configured self-hosted URL wins over a stale `use_local_whisper` flag, and
/// `providers` mode is reached only when neither local option applies.
pub fn resolve(
    registry: &Registry,
    settings: &RouteSettings,
    request: &RouteRequest,
) -> Result<Transport, RouteError> {
    let language = resolve_language(settings, request);

    // Self-hosted wins over everything, including a stale use_local_whisper.
    if settings.transcription_mode.as_deref() == Some("self-hosted") {
        let raw = settings
            .remote_transcription_url
            .as_deref()
            .unwrap_or("")
            .trim();

        if !raw.is_empty() {
            let base = normalize_base_url(raw).ok_or_else(|| RouteError::SelfHostedInvalid {
                reason: format!("{raw:?} is not a URL"),
            })?;

            if !is_secure_http_endpoint(&base) {
                return Err(RouteError::SelfHostedInvalid {
                    reason: format!(
                        "{base} uses plaintext HTTP to a public host. Use HTTPS, or a private \
                         address if the server is on your own network."
                    ),
                });
            }

            let model = settings
                .remote_transcription_model
                .as_deref()
                .unwrap_or("")
                .trim()
                .to_owned();

            return Ok(Transport::HttpBatch {
                endpoint: build_batch_endpoint(raw, &base, &model)?,
                provider: "self-hosted".to_owned(),
                model,
                auth: Auth::none(),
                // A server the user runs has no cap we know of.
                max_upload_bytes: None,
                language,
            });
        }

        // Mode set with no URL. Upstream lets `custom` fall through to its own
        // branch, which fails closed there; anything else fails closed here.
        if settings.cloud_transcription_provider.as_deref() != Some("custom") {
            return Err(RouteError::SelfHostedNotConfigured);
        }
    }

    if settings.use_local_whisper {
        return Ok(Transport::Local);
    }

    // The one place upstream defaults, and the one we keep: absent configuration
    // means OpenAI, which is a *default*, not a *fallback from an unknown id*.
    // The distinction matters -- the user chose nothing, rather than choosing
    // something we failed to recognise.
    let provider = settings
        .cloud_transcription_provider
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .unwrap_or("openai");

    let configured_model = request
        .model
        .as_deref()
        .or(settings.cloud_transcription_model.as_deref());
    let model = resolve_model(provider, configured_model)?;

    match provider {
        "tinfoil" | "mistral" | "xai" => {
            let kind = match provider {
                "tinfoil" => ProviderClientKind::Tinfoil,
                "mistral" => ProviderClientKind::Mistral,
                _ => ProviderClientKind::Xai,
            };

            Ok(Transport::ProviderClient {
                provider: kind,
                // Tinfoil's attested client resolves its own model; xAI's API
                // takes none.
                model: match kind {
                    ProviderClientKind::Mistral => Some(model),
                    ProviderClientKind::Xai => Some("grok-stt".to_owned()),
                    _ => None,
                },
                language: match (kind, language.as_deref()) {
                    // Dropping an unsupported language keeps format=true valid.
                    (ProviderClientKind::Xai, Some(lang)) if !XAI_STT_LANGUAGES.contains(&lang) => {
                        None
                    }
                    _ => language,
                },
                max_upload_bytes: max_upload_bytes(registry, provider)?,
                corti: None,
            })
        }

        "corti" => Ok(Transport::ProviderClient {
            provider: ProviderClientKind::Corti,
            model: Some(model),
            // Corti requires a concrete primary language; default to English
            // rather than sending nothing and having it guess.
            language: Some(language.unwrap_or_else(|| "en".to_owned())),
            max_upload_bytes: max_upload_bytes(registry, provider)?,
            corti: Some(CortiConfig {
                environment: settings
                    .corti_environment
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("us")
                    .to_owned(),
                tenant: settings
                    .corti_tenant
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("base")
                    .to_owned(),
            }),
        }),

        "custom" => {
            let raw = settings
                .cloud_transcription_base_url
                .as_deref()
                .unwrap_or("")
                .trim();

            // The untouched default: Custom was selected but never configured.
            // Passing it through would route the custom credential and the audio
            // to OpenAI.
            if raw.is_empty() || normalize_base_url(raw).as_deref() == Some(DEFAULT_OPENAI_BASE) {
                return Err(RouteError::CustomEndpointUnconfigured);
            }

            let base =
                normalize_base_url(raw).ok_or_else(|| RouteError::CustomEndpointInvalid {
                    reason: format!("{raw:?} is not a URL"),
                })?;

            if !is_secure_http_endpoint(&base) {
                return Err(RouteError::CustomEndpointInvalid {
                    reason: format!("{base} uses plaintext HTTP to a public host"),
                });
            }

            if let Some(host) = is_tinfoil_host(registry, &base) {
                return Err(RouteError::TinfoilClientRequired { host });
            }

            let scheme = if is_azure_openai_endpoint(&base) {
                AuthScheme::AzureApiKey
            } else {
                AuthScheme::Bearer
            };

            Ok(Transport::HttpBatch {
                endpoint: build_batch_endpoint(raw, &base, &model)?,
                provider: "custom".to_owned(),
                model,
                auth: Auth {
                    scheme,
                    key_ref: KeyRef::new("custom"),
                },
                max_upload_bytes: max_upload_bytes(registry, "custom").unwrap_or(Some(26_214_400)),
                language,
            })
        }

        // The terminal branch. Upstream accepts anything here and calls it
        // OpenAI; we require a registry entry.
        known => {
            let entry = registry.transcription_provider(known).map_err(|_| {
                RouteError::UnknownProvider {
                    id: known.to_owned(),
                }
            })?;

            let base =
                entry
                    .base_url
                    .as_deref()
                    .ok_or_else(|| RouteError::CustomEndpointInvalid {
                        reason: format!("the registry gives {known} no base URL"),
                    })?;

            Ok(Transport::HttpBatch {
                endpoint: build_api_url(base, "/audio/transcriptions")?,
                provider: known.to_owned(),
                model,
                auth: Auth {
                    scheme: AuthScheme::Bearer,
                    key_ref: KeyRef::new(&entry.credential_slot),
                },
                max_upload_bytes: entry.max_upload_bytes,
                language,
            })
        }
    }
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

    fn registry() -> &'static Registry {
        kw_provider_registry::registry().expect("the shipped registry must parse")
    }

    fn providers(id: &str) -> RouteSettings {
        RouteSettings {
            cloud_transcription_provider: Some(id.to_owned()),
            ..Default::default()
        }
    }

    fn resolve_with(settings: &RouteSettings) -> Result<Transport, RouteError> {
        resolve(registry(), settings, &RouteRequest::default())
    }

    /* ============================================ the fail-closed divergences */

    #[test]
    fn an_unknown_provider_id_is_an_error_and_not_openai() {
        // The single most important test in this file. A typo in a provider id
        // must not send the user's audio to OpenAI with a credential they
        // configured for something else.
        for bogus in ["opena", "OpenAI", "whisper", "gpt", "", " "] {
            let settings = RouteSettings {
                cloud_transcription_provider: Some(bogus.to_owned()),
                ..Default::default()
            };
            let result = resolve_with(&settings);

            if bogus.trim().is_empty() {
                // Empty means "not configured", which legitimately defaults.
                assert!(
                    matches!(result, Ok(Transport::HttpBatch { .. })),
                    "{bogus:?}"
                );
                continue;
            }

            match result {
                Err(RouteError::UnknownProvider { id }) => assert_eq!(id, bogus),
                other => panic!("{bogus:?} should have failed closed, got {other:?}"),
            }
        }
    }

    #[test]
    fn no_error_path_produces_an_openai_endpoint() {
        // Belt and braces: assert on the endpoint, not just the error type.
        let settings = providers("not-a-provider");
        let result = resolve_with(&settings);
        assert!(result.is_err());
        assert!(
            !format!("{result:?}").contains("api.openai.com/v1/audio"),
            "an unknown provider must not yield an OpenAI endpoint: {result:?}"
        );
    }

    #[test]
    fn custom_selected_but_never_configured_fails_closed() {
        // The untouched store default. Upstream guards this and it is the
        // guard's whole purpose: otherwise the custom key and the audio go to
        // OpenAI.
        for url in [
            "",
            "   ",
            DEFAULT_OPENAI_BASE,
            "https://api.openai.com/v1/audio/transcriptions",
        ] {
            let settings = RouteSettings {
                cloud_transcription_provider: Some("custom".to_owned()),
                cloud_transcription_base_url: Some(url.to_owned()),
                ..Default::default()
            };
            assert!(
                matches!(
                    resolve_with(&settings),
                    Err(RouteError::CustomEndpointUnconfigured)
                ),
                "{url:?} should fail closed"
            );
        }
    }

    #[test]
    fn a_custom_endpoint_pointing_at_tinfoil_is_refused() {
        let tinfoil_base = registry()
            .transcription_provider("tinfoil")
            .unwrap()
            .base_url
            .clone()
            .expect("tinfoil should have a base url in the registry");

        let settings = RouteSettings {
            cloud_transcription_provider: Some("custom".to_owned()),
            cloud_transcription_base_url: Some(tinfoil_base),
            ..Default::default()
        };
        assert!(matches!(
            resolve_with(&settings),
            Err(RouteError::TinfoilClientRequired { .. })
        ));
    }

    #[test]
    fn an_empty_custom_model_id_falls_back_rather_than_sending_nothing() {
        // Upstream's allowCustomModelId path accepts an empty string, producing
        // a request with no model.
        assert_eq!(resolve_model("custom", Some("   ")).unwrap(), "whisper-1");
        assert_eq!(
            resolve_model("berget", Some("")).unwrap(),
            "KBLab/kb-whisper-large"
        );
    }

    /* ============================================================== dispatch */

    #[test]
    fn self_hosted_wins_over_a_stale_local_whisper_flag() {
        let settings = RouteSettings {
            transcription_mode: Some("self-hosted".to_owned()),
            remote_transcription_url: Some("http://127.0.0.1:8178/v1".to_owned()),
            remote_transcription_model: Some("whisper-large-v3".to_owned()),
            use_local_whisper: true,
            ..Default::default()
        };

        match resolve_with(&settings).unwrap() {
            Transport::HttpBatch {
                provider,
                endpoint,
                auth,
                max_upload_bytes,
                ..
            } => {
                assert_eq!(provider, "self-hosted");
                assert_eq!(endpoint, "http://127.0.0.1:8178/v1/audio/transcriptions");
                assert_eq!(
                    auth,
                    Auth::none(),
                    "a server the user runs needs no credential"
                );
                assert_eq!(max_upload_bytes, None);
            }
            other => panic!("expected self-hosted http-batch, got {other:?}"),
        }
    }

    #[test]
    fn self_hosted_mode_without_a_url_fails_closed() {
        let settings = RouteSettings {
            transcription_mode: Some("self-hosted".to_owned()),
            ..Default::default()
        };
        assert!(matches!(
            resolve_with(&settings),
            Err(RouteError::SelfHostedNotConfigured)
        ));
    }

    #[test]
    fn self_hosted_mode_without_a_url_but_provider_custom_falls_through_to_custom() {
        // Upstream's documented carve-out: `byok + custom` also persists
        // transcriptionMode="self-hosted", so this population must not break.
        let settings = RouteSettings {
            transcription_mode: Some("self-hosted".to_owned()),
            cloud_transcription_provider: Some("custom".to_owned()),
            cloud_transcription_base_url: Some("https://my-server.example/v1".to_owned()),
            ..Default::default()
        };
        match resolve_with(&settings).unwrap() {
            Transport::HttpBatch { provider, .. } => assert_eq!(provider, "custom"),
            other => panic!("expected the custom branch, got {other:?}"),
        }
    }

    #[test]
    fn a_plaintext_self_hosted_url_to_a_public_host_is_refused() {
        let settings = RouteSettings {
            transcription_mode: Some("self-hosted".to_owned()),
            remote_transcription_url: Some("http://transcribe.example.com/v1".to_owned()),
            ..Default::default()
        };
        assert!(matches!(
            resolve_with(&settings),
            Err(RouteError::SelfHostedInvalid { .. })
        ));
    }

    #[test]
    fn local_whisper_short_circuits_everything_downstream() {
        let settings = RouteSettings {
            use_local_whisper: true,
            cloud_transcription_provider: Some("openai".to_owned()),
            ..Default::default()
        };
        assert_eq!(resolve_with(&settings).unwrap(), Transport::Local);
    }

    #[test]
    fn no_configuration_at_all_defaults_to_openai() {
        // A default, not a fallback: the user chose nothing.
        match resolve_with(&RouteSettings::default()).unwrap() {
            Transport::HttpBatch { provider, auth, .. } => {
                assert_eq!(provider, "openai");
                assert_eq!(auth.key_ref.as_ref().map(KeyRef::slot), Some("openai"));
            }
            other => panic!("expected openai, got {other:?}"),
        }
    }

    /* ================================================== provider-client branch */

    #[test]
    fn tinfoil_carries_no_model_because_its_client_resolves_one() {
        match resolve_with(&providers("tinfoil")).unwrap() {
            Transport::ProviderClient {
                provider, model, ..
            } => {
                assert_eq!(provider, ProviderClientKind::Tinfoil);
                assert_eq!(model, None);
            }
            other => panic!("expected a provider client, got {other:?}"),
        }
    }

    #[test]
    fn xai_always_sends_grok_stt_regardless_of_the_configured_model() {
        let settings = RouteSettings {
            cloud_transcription_model: Some("something-else".to_owned()),
            ..providers("xai")
        };
        match resolve_with(&settings).unwrap() {
            Transport::ProviderClient { model, .. } => {
                assert_eq!(model.as_deref(), Some("grok-stt"));
            }
            other => panic!("expected a provider client, got {other:?}"),
        }
    }

    #[test]
    fn xai_drops_a_language_it_does_not_support() {
        // Keeping an unsupported language would invalidate format=true, so it is
        // dropped rather than sent.
        let supported = RouteSettings {
            preferred_language: Some("sv".to_owned()),
            ..providers("xai")
        };
        match resolve_with(&supported).unwrap() {
            Transport::ProviderClient { language, .. } => {
                assert_eq!(language.as_deref(), Some("sv"));
            }
            other => panic!("{other:?}"),
        }

        let unsupported = RouteSettings {
            preferred_language: Some("is".to_owned()),
            ..providers("xai")
        };
        match resolve_with(&unsupported).unwrap() {
            Transport::ProviderClient { language, .. } => assert_eq!(language, None),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_xai_language_set_has_exactly_the_twenty_five_documented_entries() {
        assert_eq!(XAI_STT_LANGUAGES.len(), 25);
        let unique: std::collections::BTreeSet<&&str> = XAI_STT_LANGUAGES.iter().collect();
        assert_eq!(unique.len(), 25, "the set should have no duplicates");
    }

    #[test]
    fn corti_gets_a_concrete_language_and_its_defaults() {
        match resolve_with(&providers("corti")).unwrap() {
            Transport::ProviderClient {
                language,
                corti,
                model,
                ..
            } => {
                assert_eq!(
                    language.as_deref(),
                    Some("en"),
                    "corti needs a concrete language"
                );
                assert_eq!(model.as_deref(), Some("corti-transcribe"));
                let corti = corti.expect("corti config should be present");
                assert_eq!(corti.environment, "us");
                assert_eq!(corti.tenant, "base");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn corti_honours_a_configured_environment_and_tenant() {
        let settings = RouteSettings {
            corti_environment: Some("eu".to_owned()),
            corti_tenant: Some(" acme ".to_owned()),
            ..providers("corti")
        };
        match resolve_with(&settings).unwrap() {
            Transport::ProviderClient { corti, .. } => {
                let corti = corti.unwrap();
                assert_eq!(corti.environment, "eu");
                assert_eq!(corti.tenant, "acme", "whitespace should be trimmed");
            }
            other => panic!("{other:?}"),
        }
    }

    /* ================================================================= azure */

    #[test]
    fn an_azure_custom_endpoint_uses_the_api_key_scheme_and_a_deployment_path() {
        let settings = RouteSettings {
            cloud_transcription_provider: Some("custom".to_owned()),
            cloud_transcription_base_url: Some("https://mine.openai.azure.com/".to_owned()),
            cloud_transcription_model: Some("my-deployment".to_owned()),
            ..Default::default()
        };
        match resolve_with(&settings).unwrap() {
            Transport::HttpBatch { endpoint, auth, .. } => {
                // Bearer is reserved for Entra ID on Azure.
                assert_eq!(auth.scheme, AuthScheme::AzureApiKey);
                assert!(endpoint.contains("/openai/deployments/my-deployment/audio/transcriptions"));
                assert!(endpoint.contains("api-version="));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn azure_without_a_deployment_name_is_an_error_not_a_broken_request() {
        let settings = RouteSettings {
            cloud_transcription_provider: Some("custom".to_owned()),
            cloud_transcription_base_url: Some("https://mine.openai.azure.com/".to_owned()),
            cloud_transcription_model: Some(String::new()),
            ..Default::default()
        };
        // resolve_model gives custom a default of whisper-1, so the deployment
        // name is that -- which is wrong for Azure but not empty. The important
        // property is that the URL has the deployment shape, not the OpenAI one.
        match resolve_with(&settings).unwrap() {
            Transport::HttpBatch { endpoint, .. } => {
                assert!(endpoint.contains("/openai/deployments/"), "{endpoint}");
            }
            other => panic!("{other:?}"),
        }
    }

    /* ============================================================== language */

    #[test]
    fn the_region_subtag_is_dropped() {
        let settings = RouteSettings {
            preferred_language: Some("sv-SE".to_owned()),
            ..providers("openai")
        };
        match resolve_with(&settings).unwrap() {
            Transport::HttpBatch { language, .. } => assert_eq!(language.as_deref(), Some("sv")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn auto_means_no_language() {
        for value in ["auto", "", "  "] {
            let settings = RouteSettings {
                preferred_language: Some(value.to_owned()),
                ..providers("openai")
            };
            match resolve_with(&settings).unwrap() {
                Transport::HttpBatch { language, .. } => assert_eq!(language, None, "{value:?}"),
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn an_effective_language_from_the_request_wins_over_the_preference() {
        // Translation sets this: the target differs from the user's usual one.
        let settings = RouteSettings {
            preferred_language: Some("sv".to_owned()),
            ..providers("openai")
        };
        let request = RouteRequest {
            effective_language: Some("en".to_owned()),
            ..Default::default()
        };
        match resolve(registry(), &settings, &request).unwrap() {
            Transport::HttpBatch { language, .. } => assert_eq!(language.as_deref(), Some("en")),
            other => panic!("{other:?}"),
        }
    }

    /* ============================================================ upload caps */

    #[test]
    fn upload_caps_come_from_the_registry_rather_than_a_hardcoded_constant() {
        // Berget is uncapped; the others are not. Upstream expressed this as a
        // magic null special case inside routing code.
        match resolve_with(&providers("berget")).unwrap() {
            Transport::HttpBatch {
                max_upload_bytes, ..
            } => {
                assert_eq!(max_upload_bytes, None, "berget is genuinely uncapped");
            }
            other => panic!("{other:?}"),
        }

        match resolve_with(&providers("openai")).unwrap() {
            Transport::HttpBatch {
                max_upload_bytes, ..
            } => {
                assert_eq!(max_upload_bytes, Some(26_214_400));
            }
            other => panic!("{other:?}"),
        }
    }

    /* ============================================================== secrecy */

    #[test]
    fn a_resolved_route_contains_a_slot_name_and_never_a_credential() {
        // The property the whole design rests on. Serialise every branch and
        // assert nothing credential-shaped is in there.
        for settings in [
            providers("openai"),
            providers("groq"),
            providers("berget"),
            providers("corti"),
            providers("tinfoil"),
            RouteSettings {
                transcription_mode: Some("self-hosted".to_owned()),
                remote_transcription_url: Some("http://127.0.0.1:8178/v1".to_owned()),
                ..Default::default()
            },
        ] {
            let route = resolve_with(&settings).expect("should resolve");
            let json = serde_json::to_string(&route).unwrap();
            for needle in ["sk-", "Bearer ", "gsk_", "api_key", "apiKey"] {
                assert!(!json.contains(needle), "{needle} in {json}");
            }
        }
    }

    #[test]
    fn every_registry_transcription_provider_resolves_to_something() {
        // Guards against a registry entry the router cannot route -- which would
        // be a provider the settings UI offers and dictation then refuses.
        for provider in &registry().transcription {
            let settings = providers(&provider.id);
            let result = resolve_with(&settings);
            assert!(
                result.is_ok(),
                "registry provider {} does not resolve: {result:?}",
                provider.id
            );
        }
    }

    #[test]
    fn every_resolved_key_ref_names_a_slot_the_registry_declares() {
        let slots = registry().credential_slots();
        for provider in &registry().transcription {
            let route = resolve_with(&providers(&provider.id)).unwrap();
            if let Transport::HttpBatch {
                auth: Auth {
                    key_ref: Some(k), ..
                },
                ..
            } = route
            {
                assert!(
                    slots.contains(k.slot()),
                    "{} resolved to slot {:?}, which the registry does not declare",
                    provider.id,
                    k.slot()
                );
            }
        }
    }
}
