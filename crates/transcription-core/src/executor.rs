//! Sending audio to whichever endpoint the router chose.
//!
//! This is the only place a transcription credential is resolved, and it happens
//! at the last possible moment: a [`crate::route::Transport`] carries a slot
//! name, this module turns it into a [`Secret`] and hands it straight to
//! `kw_network_policy`. Nothing in between holds it.
//!
//! Three rules are enforced here rather than left to callers.
//!
//! **The upload cap is checked before the request, not after.** A 30 MB file
//! sent to a 25 MiB endpoint wastes the upload and returns an error that reads
//! like a server problem. Checking first produces an error that says what to do.
//!
//! **A 2xx is not a success.** As in `kw_provider_probe`, a 200 with no `text`
//! field means the route worked and the model did not. Reporting that as an
//! empty transcript would silently insert nothing at the user's cursor and look
//! like a microphone problem.
//!
//! **Nothing here logs audio, a transcript, or a credential.** Byte counts and
//! provider ids only.

use kw_network_policy::{NetworkPolicy, RequestPurpose};
use kw_security::{CredentialStore, Secret};
use serde::Serialize;

use crate::route::{Auth, AuthScheme, Transport};

/// A finished transcription.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Transcript {
    pub text: String,
    /// Which provider produced it, for the history row.
    pub provider: String,
    /// Which model, where the provider reports or was told one.
    pub model: Option<String>,
}

/// Why a transcription failed.
#[derive(Debug, thiserror::Error)]
pub enum TranscriptionError {
    #[error(
        "the recording is {actual_bytes} bytes and {provider} accepts at most {limit_bytes}. \
         Record a shorter passage, or choose a provider without that limit."
    )]
    TooLarge {
        provider: String,
        actual_bytes: u64,
        limit_bytes: u64,
    },

    #[error("no credential is stored for {slot}. Add one in settings.")]
    MissingCredential { slot: String },

    #[error("{provider} returned HTTP {status}: {body}")]
    ProviderRejected {
        provider: String,
        status: u16,
        body: String,
    },

    #[error(
        "{provider} answered successfully but sent no transcript. The request reached the \
         provider and the model produced nothing usable."
    )]
    NoTranscript { provider: String },

    #[error("network policy refused the request: {0}")]
    Policy(#[from] kw_network_policy::PolicyError),

    #[error("the request to {provider} failed: {detail}")]
    Transport { provider: String, detail: String },

    #[error(
        "{provider} needs its own client, which is not implemented yet. Choose a provider \
         that uses the standard API, or use a local model."
    )]
    ClientNotImplemented { provider: String },

    #[error("local transcription is not wired up yet")]
    LocalNotImplemented,
}

/// How much of a provider's error body may be reported.
const BODY_LIMIT: usize = 400;

fn truncate(mut s: String) -> String {
    if s.chars().count() <= BODY_LIMIT {
        return s;
    }
    let cut = s.char_indices().nth(BODY_LIMIT).map_or(s.len(), |(i, _)| i);
    s.truncate(cut);
    s.push('…');
    s
}

/// Everything one batch request needs, gathered so the send function has a
/// signature a reader can hold in their head.
#[derive(Debug)]
struct BatchRequest<'a> {
    provider: &'a str,
    endpoint: &'a str,
    model: &'a str,
    language: Option<&'a str>,
    auth: &'a Auth,
    /// Resolved at the call site and borrowed here, so it is never stored.
    credential: Option<&'a Secret>,
}

/// Executes a resolved route.
#[derive(Debug)]
pub struct Executor<'a> {
    policy: &'a NetworkPolicy,
    credentials: &'a dyn CredentialStore,
}

impl<'a> Executor<'a> {
    #[must_use]
    pub const fn new(policy: &'a NetworkPolicy, credentials: &'a dyn CredentialStore) -> Self {
        Self {
            policy,
            credentials,
        }
    }

    /// Transcribe `wav` according to `transport`.
    ///
    /// `wav` is 16-bit mono PCM in a WAV container; every provider here accepts
    /// that, so there is no per-provider transcoding to get wrong.
    pub async fn transcribe(
        &self,
        transport: &Transport,
        wav: Vec<u8>,
    ) -> Result<Transcript, TranscriptionError> {
        match transport {
            Transport::Local => Err(TranscriptionError::LocalNotImplemented),

            Transport::ProviderClient { provider, .. } => {
                // Honest refusal rather than a silent substitution. Falling back
                // to a generic POST here would send audio to an endpoint that
                // does not exist for these providers, and falling back to
                // *another provider* is exactly what the brief forbids.
                Err(TranscriptionError::ClientNotImplemented {
                    provider: format!("{provider:?}").to_lowercase(),
                })
            }

            Transport::HttpBatch {
                provider,
                endpoint,
                model,
                auth,
                max_upload_bytes,
                language,
            } => {
                self.check_size(provider, *max_upload_bytes, wav.len())?;

                let credential = self.resolve_credential(auth)?;

                self.post_multipart(
                    &BatchRequest {
                        provider,
                        endpoint,
                        model,
                        language: language.as_deref(),
                        auth,
                        credential: credential.as_ref(),
                    },
                    wav,
                )
                .await
            }
        }
    }

    /// Refuse an oversized recording before spending the upload on it.
    fn check_size(
        &self,
        provider: &str,
        limit: Option<u64>,
        actual: usize,
    ) -> Result<(), TranscriptionError> {
        let Some(limit) = limit else { return Ok(()) };
        let actual = actual as u64;

        if actual > limit {
            return Err(TranscriptionError::TooLarge {
                provider: provider.to_owned(),
                actual_bytes: actual,
                limit_bytes: limit,
            });
        }

        Ok(())
    }

    /// Turn the route's slot name into a secret. The last possible moment.
    fn resolve_credential(&self, auth: &Auth) -> Result<Option<Secret>, TranscriptionError> {
        if auth.scheme == AuthScheme::None {
            return Ok(None);
        }

        let slot = auth
            .key_ref
            .as_ref()
            .ok_or_else(|| TranscriptionError::MissingCredential {
                slot: "<unnamed>".to_owned(),
            })?;

        let secret =
            self.credentials
                .get(slot)
                .map_err(|_| TranscriptionError::MissingCredential {
                    slot: slot.slot().to_owned(),
                })?;

        if secret.is_empty() {
            return Err(TranscriptionError::MissingCredential {
                slot: slot.slot().to_owned(),
            });
        }

        Ok(Some(secret))
    }

    async fn post_multipart(
        &self,
        request: &BatchRequest<'_>,
        wav: Vec<u8>,
    ) -> Result<Transcript, TranscriptionError> {
        let BatchRequest {
            provider,
            endpoint,
            model,
            language,
            auth,
            credential,
        } = *request;

        // Byte count only. Never the audio, never the transcript.
        tracing::info!(
            provider,
            model,
            bytes = wav.len(),
            language = language.unwrap_or("auto"),
            "sending audio for transcription"
        );

        let part = reqwest::multipart::Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| TranscriptionError::Transport {
                provider: provider.to_owned(),
                detail: e.to_string(),
            })?;

        let mut form = reqwest::multipart::Form::new().part("file", part);
        if !model.is_empty() {
            form = form.text("model", model.to_owned());
        }
        if let Some(language) = language {
            form = form.text("language", language.to_owned());
        }

        let mut request = self.policy.post(purpose_for(provider), endpoint)?;

        // The only two lines in this crate that touch a credential.
        if let Some(secret) = credential {
            request = match auth.scheme {
                AuthScheme::Bearer => request.bearer_auth(secret.expose()),
                AuthScheme::AzureApiKey => request.header("api-key", secret.expose()),
                AuthScheme::None => request,
            };
        }

        let response =
            request
                .multipart(form)
                .send()
                .await
                .map_err(|e| TranscriptionError::Transport {
                    provider: provider.to_owned(),
                    detail: e.to_string(),
                })?;

        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|e| TranscriptionError::Transport {
                provider: provider.to_owned(),
                detail: e.to_string(),
            })?;

        interpret_response(provider, model, status, &body)
    }
}

/// The egress purpose for a provider.
///
/// A self-hosted server is not "a transcription provider the user configured" in
/// the boundary's terms -- it is an endpoint they pointed us at -- and only that
/// purpose may use plaintext HTTP to a private address. Getting this wrong is not
/// cosmetic: it makes a local whisper server unreachable, because the policy
/// layer refuses the scheme before the request is sent.
#[must_use]
pub fn purpose_for(provider: &str) -> RequestPurpose {
    match provider {
        "self-hosted" => RequestPurpose::ExplicitlyConfiguredSelfHostedEndpoint,
        // A custom endpoint may also be a machine on the user's own network, and
        // route resolution has already refused a plaintext public host.
        "custom" => RequestPurpose::ExplicitlyConfiguredSelfHostedEndpoint,
        _ => RequestPurpose::UserConfiguredTranscriptionProvider,
    }
}

/// Turn a provider's response into a transcript or an error.
///
/// Separated out so every rule is testable without a network — which is where
/// the "a 2xx is not a success" rule lives.
pub fn interpret_response(
    provider: &str,
    model: &str,
    status: u16,
    body: &str,
) -> Result<Transcript, TranscriptionError> {
    if !(200..300).contains(&status) {
        return Err(TranscriptionError::ProviderRejected {
            provider: provider.to_owned(),
            status,
            body: truncate(body.to_owned()),
        });
    }

    let parsed: serde_json::Value =
        serde_json::from_str(body).map_err(|_| TranscriptionError::NoTranscript {
            provider: provider.to_owned(),
        })?;

    // The contract is a `text` field. Absent means the route worked and the
    // model did not, which must not be reported as an empty transcript.
    let Some(text) = parsed.get("text").and_then(serde_json::Value::as_str) else {
        return Err(TranscriptionError::NoTranscript {
            provider: provider.to_owned(),
        });
    };

    Ok(Transcript {
        text: text.trim().to_owned(),
        provider: provider.to_owned(),
        // Prefer what the provider says it used over what we asked for.
        model: parsed
            .get("model")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .or_else(|| (!model.is_empty()).then(|| model.to_owned())),
    })
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
    use kw_security::{KeyRef, MemoryCredentialStore};

    /* -------------------------------------------- a 2xx is not automatically ok */

    #[test]
    fn a_response_with_a_transcript_succeeds() {
        let t = interpret_response("openai", "whisper-1", 200, r#"{"text":"  hello there  "}"#)
            .unwrap();
        assert_eq!(t.text, "hello there", "surrounding whitespace is trimmed");
        assert_eq!(t.provider, "openai");
        assert_eq!(t.model.as_deref(), Some("whisper-1"));
    }

    #[test]
    fn a_2xx_without_a_text_field_is_an_error_not_an_empty_transcript() {
        // Reporting this as success would insert nothing at the cursor and look
        // like a microphone fault.
        for body in [
            r#"{}"#,
            r#"{"segments":[]}"#,
            r#"{"text":null}"#,
            "not json at all",
        ] {
            let err = interpret_response("openai", "m", 200, body).unwrap_err();
            assert!(
                matches!(err, TranscriptionError::NoTranscript { .. }),
                "{body} gave {err:?}"
            );
        }
    }

    #[test]
    fn an_empty_transcript_string_is_a_legitimate_result() {
        // Silence transcribes to "". Distinct from an absent field.
        let t = interpret_response("openai", "m", 200, r#"{"text":""}"#).unwrap();
        assert_eq!(t.text, "");
    }

    #[test]
    fn a_provider_error_carries_the_status_and_a_bounded_body() {
        let err = interpret_response("groq", "m", 429, r#"{"error":"slow down"}"#).unwrap_err();
        match err {
            TranscriptionError::ProviderRejected {
                provider,
                status,
                body,
            } => {
                assert_eq!(provider, "groq");
                assert_eq!(status, 429);
                assert!(body.contains("slow down"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_enormous_error_body_is_truncated() {
        let err = interpret_response("openai", "m", 500, &"x".repeat(10_000)).unwrap_err();
        match err {
            TranscriptionError::ProviderRejected { body, .. } => {
                assert!(body.chars().count() <= BODY_LIMIT + 1);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_provider_reported_model_wins_over_the_requested_one() {
        let t = interpret_response(
            "openai",
            "gpt-4o-mini-transcribe",
            200,
            r#"{"text":"hi","model":"gpt-4o-transcribe"}"#,
        )
        .unwrap();
        assert_eq!(t.model.as_deref(), Some("gpt-4o-transcribe"));
    }

    #[test]
    fn truncation_survives_multibyte_input() {
        let err = interpret_response("openai", "m", 500, &"åäö".repeat(5_000)).unwrap_err();
        assert!(matches!(err, TranscriptionError::ProviderRejected { .. }));
    }

    /* ---------------------------------------------------------- the size check */

    #[tokio::test]
    async fn an_oversized_recording_is_refused_before_it_is_uploaded() {
        let policy = NetworkPolicy::new().unwrap();
        let creds = MemoryCredentialStore::new();
        creds
            .set(
                &KeyRef::new("openai").unwrap(),
                &Secret::new("sk-test".to_owned()),
            )
            .unwrap();
        let executor = Executor::new(&policy, &creds);

        let transport = Transport::HttpBatch {
            provider: "openai".to_owned(),
            // A URL that would fail if reached -- the point is that it is not.
            endpoint: "https://api.openai.com/v1/audio/transcriptions".to_owned(),
            model: "whisper-1".to_owned(),
            auth: Auth {
                scheme: AuthScheme::Bearer,
                key_ref: KeyRef::new("openai"),
            },
            max_upload_bytes: Some(1_000),
            language: None,
        };

        let err = executor
            .transcribe(&transport, vec![0_u8; 2_000])
            .await
            .unwrap_err();
        match err {
            TranscriptionError::TooLarge {
                provider,
                actual_bytes,
                limit_bytes,
            } => {
                assert_eq!(provider, "openai");
                assert_eq!(actual_bytes, 2_000);
                assert_eq!(limit_bytes, 1_000);
            }
            other => panic!("expected TooLarge, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn an_uncapped_provider_accepts_any_size_without_a_local_refusal() {
        let policy = NetworkPolicy::new().unwrap();
        let creds = MemoryCredentialStore::new();
        let executor = Executor::new(&policy, &creds);

        // No credential stored, so it fails at the credential step -- which
        // proves the size check let it through.
        let transport = Transport::HttpBatch {
            provider: "berget".to_owned(),
            endpoint: "https://api.berget.ai/v1/audio/transcriptions".to_owned(),
            model: "KBLab/kb-whisper-large".to_owned(),
            auth: Auth {
                scheme: AuthScheme::Bearer,
                key_ref: KeyRef::new("berget"),
            },
            max_upload_bytes: None,
            language: None,
        };

        let err = executor
            .transcribe(&transport, vec![0_u8; 100_000_000])
            .await
            .unwrap_err();
        assert!(
            matches!(err, TranscriptionError::MissingCredential { .. }),
            "expected the size check to pass, got {err:?}"
        );
    }

    /* ------------------------------------------------------- credential handling */

    #[tokio::test]
    async fn a_missing_credential_is_named_but_not_guessed_at() {
        let policy = NetworkPolicy::new().unwrap();
        let creds = MemoryCredentialStore::new();
        let executor = Executor::new(&policy, &creds);

        let transport = Transport::HttpBatch {
            provider: "openai".to_owned(),
            endpoint: "https://api.openai.com/v1/audio/transcriptions".to_owned(),
            model: "whisper-1".to_owned(),
            auth: Auth {
                scheme: AuthScheme::Bearer,
                key_ref: KeyRef::new("openai"),
            },
            max_upload_bytes: Some(1_000_000),
            language: None,
        };

        match executor
            .transcribe(&transport, vec![0_u8; 10])
            .await
            .unwrap_err()
        {
            TranscriptionError::MissingCredential { slot } => assert_eq!(slot, "openai"),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_blank_stored_credential_is_treated_as_missing() {
        let policy = NetworkPolicy::new().unwrap();
        let creds = MemoryCredentialStore::new();
        creds
            .set(
                &KeyRef::new("openai").unwrap(),
                &Secret::new("   ".to_owned()),
            )
            .unwrap();
        let executor = Executor::new(&policy, &creds);

        let transport = Transport::HttpBatch {
            provider: "openai".to_owned(),
            endpoint: "https://api.openai.com/v1/audio/transcriptions".to_owned(),
            model: "whisper-1".to_owned(),
            auth: Auth {
                scheme: AuthScheme::Bearer,
                key_ref: KeyRef::new("openai"),
            },
            max_upload_bytes: Some(1_000_000),
            language: None,
        };
        assert!(matches!(
            executor.transcribe(&transport, vec![0_u8; 10]).await,
            Err(TranscriptionError::MissingCredential { .. })
        ));
    }

    #[test]
    fn a_route_needing_no_credential_resolves_to_none() {
        let policy = NetworkPolicy::new().unwrap();
        let creds = MemoryCredentialStore::new();
        let executor = Executor::new(&policy, &creds);

        let resolved = executor.resolve_credential(&Auth::none()).unwrap();
        assert!(
            resolved.is_none(),
            "a self-hosted server needs no credential"
        );
    }

    /* -------------------------------------------------------- honest refusals */

    #[tokio::test]
    async fn a_provider_needing_its_own_client_is_refused_rather_than_substituted() {
        let policy = NetworkPolicy::new().unwrap();
        let creds = MemoryCredentialStore::new();
        let executor = Executor::new(&policy, &creds);

        let transport = Transport::ProviderClient {
            provider: crate::route::ProviderClientKind::Tinfoil,
            model: None,
            language: None,
            max_upload_bytes: Some(1_000),
            corti: None,
        };

        match executor
            .transcribe(&transport, vec![0_u8; 10])
            .await
            .unwrap_err()
        {
            TranscriptionError::ClientNotImplemented { provider } => {
                assert_eq!(provider, "tinfoil");
            }
            other => panic!("expected an honest refusal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_blocked_vendor_host_is_refused_by_the_network_policy() {
        let policy = NetworkPolicy::new().unwrap();
        let creds = MemoryCredentialStore::new();
        creds
            .set(
                &KeyRef::new("custom").unwrap(),
                &Secret::new("sk-x".to_owned()),
            )
            .unwrap();
        let executor = Executor::new(&policy, &creds);

        let transport = Transport::HttpBatch {
            provider: "custom".to_owned(),
            // domain-scanner:allow-next-line asserting the runtime blocklist refuses a vendor host
            endpoint: "https://api.openwhispr.com/v1/audio/transcriptions".to_owned(),
            model: "whisper-1".to_owned(),
            auth: Auth {
                scheme: AuthScheme::Bearer,
                key_ref: KeyRef::new("custom"),
            },
            max_upload_bytes: Some(1_000_000),
            language: None,
        };

        match executor
            .transcribe(&transport, vec![0_u8; 10])
            .await
            .unwrap_err()
        {
            TranscriptionError::Policy(_) => {}
            other => panic!("expected the policy to refuse it, got {other:?}"),
        }
    }

    #[test]
    fn a_self_hosted_provider_gets_the_purpose_that_permits_plaintext() {
        // The bug this guards: with the transcription purpose, the policy layer
        // refuses http:// before sending, so a local whisper server is
        // unreachable and the error blames the scheme rather than the config.
        assert_eq!(
            purpose_for("self-hosted"),
            RequestPurpose::ExplicitlyConfiguredSelfHostedEndpoint
        );
        assert_eq!(
            purpose_for("custom"),
            RequestPurpose::ExplicitlyConfiguredSelfHostedEndpoint
        );
        for cloud in ["openai", "groq", "berget"] {
            assert_eq!(
                purpose_for(cloud),
                RequestPurpose::UserConfiguredTranscriptionProvider,
                "{cloud}"
            );
        }
    }

    #[tokio::test]
    async fn a_local_whisper_server_is_not_refused_by_the_policy_layer() {
        let policy = NetworkPolicy::new().unwrap();
        let creds = MemoryCredentialStore::new();
        let executor = Executor::new(&policy, &creds);

        // Port 9 discards, so this fails to connect -- which is exactly the
        // proof wanted: it got past the policy check and tried to send.
        let transport = Transport::HttpBatch {
            provider: "self-hosted".to_owned(),
            endpoint: "http://127.0.0.1:9/v1/audio/transcriptions".to_owned(),
            model: "whisper-large-v3".to_owned(),
            auth: Auth::none(),
            max_upload_bytes: None,
            language: None,
        };

        match executor
            .transcribe(&transport, vec![0_u8; 64])
            .await
            .unwrap_err()
        {
            TranscriptionError::Transport { .. } => {}
            other => panic!("expected a connection failure, not a policy refusal: {other:?}"),
        }
    }

    #[test]
    fn no_error_message_contains_a_credential() {
        let errors = [
            TranscriptionError::MissingCredential {
                slot: "openai".to_owned(),
            },
            TranscriptionError::ProviderRejected {
                provider: "openai".to_owned(),
                status: 401,
                body: "invalid key".to_owned(),
            },
            TranscriptionError::NoTranscript {
                provider: "groq".to_owned(),
            },
            TranscriptionError::TooLarge {
                provider: "openai".to_owned(),
                actual_bytes: 1,
                limit_bytes: 2,
            },
        ];
        for err in errors {
            let rendered = err.to_string();
            assert!(!rendered.contains("sk-"), "{rendered}");
            assert!(!rendered.contains("Bearer "), "{rendered}");
        }
    }
}
