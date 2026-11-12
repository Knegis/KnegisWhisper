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
use kw_security::{CredentialStore, KeyRef, Secret};
use serde::Serialize;

use crate::local::{Inference, LocalTranscriber};
use crate::route::{Auth, AuthScheme, Transport};

/// A finished transcription.
// No `Eq`: a segment's timings are `f64`. Nothing compares two transcripts for
// total equality, and `PartialEq` is what the tests use.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Transcript {
    pub text: String,
    /// Which provider produced it, for the history row.
    pub provider: String,
    /// Which model, where the provider reports or was told one.
    pub model: Option<String>,
    /// The transcript divided into timed utterances, when the engine timed it.
    ///
    /// Empty for every provider that answers with prose and for a local engine
    /// that was not asked for timings, which is all of them but media import.
    /// An empty list means *no timings were reported* -- never "the recording
    /// was one utterance", which is the claim a single synthesised segment
    /// spanning the whole file would make.
    pub segments: Vec<crate::local::TimedSegment>,
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

    #[error("{0}")]
    Credential(#[from] kw_security::CredentialError),

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

    /// A provider this build will not send to, with the reason.
    ///
    /// The advice used to be baked in here -- "choose a provider that uses the
    /// standard API" -- and it was written when the answer was the same for all
    /// four of these providers. It is not: three have clients now, and the
    /// fourth is refused *deliberately* rather than pending, so a message
    /// calling it unimplemented would send someone looking for a feature nobody
    /// intends to add in that form. The layer that knows why supplies the reason.
    #[error("{provider}: {reason}")]
    ClientNotImplemented { provider: String, reason: String },

    // The remedy is deliberately not baked in here. This layer knows the server
    // is not answering; it does not know whether a model is installed, which
    // model was chosen, or whether a provider is configured -- and it used to
    // tell the user to "install a local model in settings" while a perfectly
    // good model sat installed, which sent a real bench check looking in the
    // wrong place five times. An error states what it observed; whoever has the
    // facts supplies the advice.
    #[error("local transcription is not available: {reason}")]
    LocalNotAvailable { reason: String },

    #[error("the local model produced nothing usable from that recording")]
    LocalEmpty,

    #[error("the local model server refused the request: {detail}")]
    LocalFailed { detail: String },
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
pub struct Executor<'a> {
    policy: &'a NetworkPolicy,
    credentials: &'a dyn CredentialStore,
    /// Present only while a local model server is installed and running.
    ///
    /// `Option` rather than a stub that errors, so "no local model" is a state
    /// the caller can see and report before the user records anything, instead
    /// of a failure they discover after speaking.
    local: Option<&'a dyn LocalTranscriber>,
    local_prompt: Option<&'a str>,
}

impl std::fmt::Debug for Executor<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Executor")
            .field("has_local", &self.local.is_some())
            .finish_non_exhaustive()
    }
}

impl<'a> Executor<'a> {
    #[must_use]
    pub const fn new(policy: &'a NetworkPolicy, credentials: &'a dyn CredentialStore) -> Self {
        Self {
            policy,
            credentials,
            local: None,
            local_prompt: None,
        }
    }

    /// The same executor, able to transcribe locally.
    #[must_use]
    pub const fn with_local(mut self, local: &'a dyn LocalTranscriber) -> Self {
        self.local = Some(local);
        self
    }

    /// Apply a prepared prompt only to local dictation; cloud requests ignore it.
    #[must_use]
    pub const fn with_local_prompt(mut self, prompt: Option<&'a str>) -> Self {
        self.local_prompt = prompt;
        self
    }

    /// Whether local transcription would work right now.
    #[must_use]
    pub const fn has_local(&self) -> bool {
        self.local.is_some()
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
            Transport::Local { languages } => self.transcribe_locally(wav, languages).await,

            Transport::ProviderClient {
                provider,
                model,
                language,
                max_upload_bytes,
                corti,
            } => {
                // Three of the four have a client now. The fourth is Tinfoil and
                // it is refused on purpose -- see
                // `clients::why_tinfoil_is_refused`. Falling back to a generic
                // POST for any of them would send audio to an endpoint that does
                // not exist for it, and falling back to *another provider* is
                // exactly what the brief forbids.
                let shaped = crate::clients::shape(
                    *provider,
                    model.as_deref(),
                    language.as_deref(),
                    corti.as_ref(),
                )?;

                self.check_size(shaped.provider, *max_upload_bytes, wav.len())?;
                self.post_client(&shaped, model.as_deref(), wav).await
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

                let credential = self.resolve_credential(auth, endpoint)?;

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

    /// Transcribe with the running local model server.
    async fn transcribe_locally(
        &self,
        wav: Vec<u8>,
        languages: &[String],
    ) -> Result<Transcript, TranscriptionError> {
        let Some(local) = self.local else {
            return Err(TranscriptionError::LocalNotAvailable {
                reason: "no local model server is running".to_owned(),
            });
        };

        // No size check: there is no upload. The recording length cap belongs to
        // capture, where it can stop the recording rather than discard it.
        match local
            .transcribe_languages(wav, languages, self.local_prompt)
            .await
            .map_err(|detail| TranscriptionError::LocalFailed { detail })?
        {
            Inference::Text(text) => Ok(Transcript {
                text,
                provider: "local".to_owned(),
                model: local.model_name(),
                segments: Vec::new(),
            }),
            Inference::Timed { text, segments } => Ok(Transcript {
                text,
                provider: "local".to_owned(),
                model: local.model_name(),
                segments,
            }),
            Inference::Empty => Err(TranscriptionError::LocalEmpty),
            Inference::Rejected { status, body } => Err(TranscriptionError::LocalFailed {
                detail: format!("HTTP {status}: {body}"),
            }),
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
    fn resolve_credential(
        &self,
        auth: &Auth,
        endpoint: &str,
    ) -> Result<Option<Secret>, TranscriptionError> {
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
            kw_security::endpoint_credential::get_for_endpoint(self.credentials, slot, endpoint)
                .map_err(|err| {
                    if slot.slot() == "custom" {
                        TranscriptionError::Credential(err)
                    } else {
                        TranscriptionError::MissingCredential {
                            slot: slot.slot().to_owned(),
                        }
                    }
                })?;

        if secret.is_empty() {
            return Err(TranscriptionError::MissingCredential {
                slot: slot.slot().to_owned(),
            });
        }

        Ok(Some(secret))
    }

    /// Send a request one of the provider clients shaped.
    ///
    /// Separate from `post_multipart` because the differences are real: the
    /// fields are per-provider rather than the OpenAI three, there may be extra
    /// headers, and Corti needs a token fetched first. Sharing one function
    /// would mean a parameter for each of those, which is how a shared function
    /// becomes two functions with a flag.
    async fn post_client(
        &self,
        shaped: &crate::clients::ShapedRequest,
        model: Option<&str>,
        wav: Vec<u8>,
    ) -> Result<Transcript, TranscriptionError> {
        // Byte count only. Never the audio, never the transcript.
        tracing::info!(
            provider = shaped.provider,
            bytes = wav.len(),
            "sending audio for transcription"
        );

        let slot =
            KeyRef::new(shaped.provider).ok_or_else(|| TranscriptionError::MissingCredential {
                slot: shaped.provider.to_owned(),
            })?;
        let stored = self
            .credentials
            .get(&slot)
            .ok()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| TranscriptionError::MissingCredential {
                slot: shaped.provider.to_owned(),
            })?;

        // Corti's stored secret is a client secret, not a bearer token, so it is
        // exchanged first. The other two send it directly.
        let bearer = if shaped.provider == "corti" {
            self.corti_token(shaped, &stored).await?
        } else {
            stored
        };

        let part = reqwest::multipart::Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| TranscriptionError::Transport {
                provider: shaped.provider.to_owned(),
                detail: e.to_string(),
            })?;

        let mut form = reqwest::multipart::Form::new().part("file", part);
        for (name, value) in &shaped.fields {
            form = form.text(name.clone(), value.clone());
        }

        let mut request = self.policy.post(shaped.purpose, &shaped.endpoint)?;
        for (name, value) in &shaped.headers {
            request = request.header(name, value);
        }
        request = request.bearer_auth(bearer.expose());

        let response =
            request
                .multipart(form)
                .send()
                .await
                .map_err(|e| TranscriptionError::Transport {
                    provider: shaped.provider.to_owned(),
                    detail: e.to_string(),
                })?;

        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|e| TranscriptionError::Transport {
                provider: shaped.provider.to_owned(),
                detail: e.to_string(),
            })?;

        crate::clients::interpret(shaped.provider, model, status, &body)
    }

    /// Exchange Corti client credentials for an access token.
    ///
    /// A request of its own, under the same purpose: it is one action the user
    /// configured. The client id is the tenant header the shaped request already
    /// carries, which is why this takes the whole request rather than two strings.
    async fn corti_token(
        &self,
        shaped: &crate::clients::ShapedRequest,
        client_secret: &Secret,
    ) -> Result<Secret, TranscriptionError> {
        let client_id = shaped
            .headers
            .iter()
            .find(|(name, _)| name == "Tenant-Name")
            .map(|(_, value)| value.as_str())
            .ok_or_else(|| TranscriptionError::ClientNotImplemented {
                provider: "corti".to_owned(),
                reason: "the shaped request carries no tenant, so there is nothing to \
                         authenticate as"
                    .to_owned(),
            })?;

        // The environment is in the endpoint host, which the client built.
        let environment = shaped
            .endpoint
            .strip_prefix("https://api.")
            .and_then(|rest| rest.split('.').next())
            .unwrap_or("eu");

        let form = crate::clients::corti_token_request(client_id, client_secret);

        let response = self
            .policy
            .post(
                shaped.purpose,
                &crate::clients::corti_token_endpoint(environment)?,
            )?
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(form.expose().to_owned())
            .send()
            .await
            .map_err(|e| TranscriptionError::Transport {
                provider: "corti (token exchange)".to_owned(),
                detail: e.to_string(),
            })?;

        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|e| TranscriptionError::Transport {
                provider: "corti (token exchange)".to_owned(),
                detail: e.to_string(),
            })?;

        crate::clients::interpret_token(status, &body)
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
        // No timings: every batch provider on this path answers with prose. A
        // provider that reports segments would be a reason to read them here,
        // not a reason to invent one.
        segments: Vec::new(),
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

    #[derive(Debug)]
    struct LanguageProbe(Option<String>);
    #[async_trait::async_trait]
    impl LocalTranscriber for LanguageProbe {
        async fn transcribe(
            &self,
            _wav: Vec<u8>,
            language: Option<&str>,
            _prompt: Option<&str>,
        ) -> Result<Inference, String> {
            assert_eq!(language, self.0.as_deref());
            Ok(Inference::Text("Hej".to_owned()))
        }
    }
    #[tokio::test]
    async fn local_language_reaches_the_decoder_including_auto() {
        let policy = NetworkPolicy::new().unwrap();
        let credentials = MemoryCredentialStore::new();
        for language in [None, Some("sv".to_owned()), Some("en".to_owned())] {
            let local = LanguageProbe(language.clone());
            let executor = Executor::new(&policy, &credentials).with_local(&local);
            let transcript = executor
                .transcribe(
                    &Transport::Local {
                        languages: language.into_iter().collect(),
                    },
                    vec![],
                )
                .await
                .unwrap();
            assert_eq!(transcript.text, "Hej");
        }
    }

    #[derive(Debug)]
    struct PromptProbe;
    #[async_trait::async_trait]
    impl LocalTranscriber for PromptProbe {
        async fn transcribe(
            &self,
            _wav: Vec<u8>,
            language: Option<&str>,
            prompt: Option<&str>,
        ) -> Result<Inference, String> {
            assert_eq!(language, Some("sv"));
            assert_eq!(prompt, Some("Dyve, KnegisWhisper"));
            Ok(Inference::Text("Hej".into()))
        }
    }
    #[tokio::test]
    async fn executor_forwards_the_prepared_local_dictation_prompt() {
        let policy = NetworkPolicy::new().unwrap();
        let credentials = MemoryCredentialStore::new();
        let local = PromptProbe;
        let executor = Executor::new(&policy, &credentials)
            .with_local(&local)
            .with_local_prompt(Some("Dyve, KnegisWhisper"));
        assert!(!format!("{executor:?}").contains("KnegisWhisper"));
        executor
            .transcribe(
                &Transport::Local {
                    languages: vec!["sv".into()],
                },
                vec![],
            )
            .await
            .unwrap();
    }

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

        let resolved = executor
            .resolve_credential(&Auth::none(), "http://127.0.0.1/v1")
            .unwrap();
        assert!(
            resolved.is_none(),
            "a self-hosted server needs no credential"
        );
    }

    /* -------------------------------------------------------- honest refusals */

    #[tokio::test]
    async fn custom_transcription_refuses_changed_origins_and_legacy_keys() {
        let policy = NetworkPolicy::new().unwrap();
        let creds = MemoryCredentialStore::new();
        let slot = KeyRef::new("custom").unwrap();
        let authorized = kw_security::endpoint_credential::bind_custom(
            &Secret::new("synthetic-key".into()),
            &["https://approved.example/v1"],
        )
        .unwrap();
        for stored in [authorized, Secret::new("legacy-key".into())] {
            creds.set(&slot, &stored).unwrap();
            for scheme in [AuthScheme::Bearer, AuthScheme::AzureApiKey] {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let transport = Transport::HttpBatch {
                    provider: "custom".into(),
                    endpoint: format!(
                        "http://{}/v1/audio/transcriptions",
                        listener.local_addr().unwrap()
                    ),
                    model: "m".into(),
                    auth: Auth {
                        scheme,
                        key_ref: Some(slot.clone()),
                    },
                    max_upload_bytes: None,
                    language: None,
                };
                let executor = Executor::new(&policy, &creds);
                tokio::select! {
                    biased;
                    _ = listener.accept() => panic!("custom key reached an unapproved transcription origin"),
                    result = executor.transcribe(&transport, b"synthetic audio".to_vec()) => assert!(matches!(result, Err(TranscriptionError::Credential(kw_security::CredentialError::UnapprovedEndpoint)))),
                }
            }
        }
    }

    #[tokio::test]
    async fn approved_custom_transcription_retains_bearer_and_azure_headers() {
        use std::io::{Read as _, Write as _};
        let policy = NetworkPolicy::new().unwrap();
        let creds = MemoryCredentialStore::new();
        let slot = KeyRef::new("custom").unwrap();
        for (scheme, expected) in [
            (AuthScheme::Bearer, "authorization: bearer synthetic-key"),
            (AuthScheme::AzureApiKey, "api-key: synthetic-key"),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!(
                "http://{}/v1/audio/transcriptions",
                listener.local_addr().unwrap()
            );
            creds
                .set(
                    &slot,
                    &kw_security::endpoint_credential::bind_custom(
                        &Secret::new("synthetic-key".into()),
                        &[&endpoint],
                    )
                    .unwrap(),
                )
                .unwrap();
            let request = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut seen = Vec::new();
                let mut chunk = [0_u8; 4096];
                loop {
                    let read = stream.read(&mut chunk).unwrap();
                    if read == 0 {
                        break;
                    }
                    seen.extend_from_slice(chunk.get(..read).unwrap());
                    let text = String::from_utf8_lossy(&seen);
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let size = head
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|s| s.trim().parse::<usize>().unwrap())
                            })
                            .unwrap();
                        if body.len() >= size {
                            break;
                        }
                    }
                }
                let body = r#"{"text":"approved transcript"}"#;
                write!(stream, "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).unwrap();
                String::from_utf8(seen).unwrap()
            });
            let transport = Transport::HttpBatch {
                provider: "custom".into(),
                endpoint,
                model: "m".into(),
                auth: Auth {
                    scheme,
                    key_ref: Some(slot.clone()),
                },
                max_upload_bytes: None,
                language: None,
            };
            assert_eq!(
                Executor::new(&policy, &creds)
                    .transcribe(&transport, b"synthetic audio".to_vec())
                    .await
                    .unwrap()
                    .text,
                "approved transcript"
            );
            let seen = request.join().unwrap().to_ascii_lowercase();
            assert!(seen.contains(expected));
            assert!(seen.contains("synthetic audio"));
            assert!(!seen.contains("origins"));
        }
    }

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
            TranscriptionError::ClientNotImplemented { provider, reason } => {
                assert_eq!(provider, "tinfoil");
                // The refusal carries its reason now. Tinfoil is not pending; it
                // is refused because an unattested request would present as
                // Tinfoil while providing none of what the name means.
                assert!(reason.contains("attestation"), "{reason}");
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
                &kw_security::endpoint_credential::bind_custom(
                    &Secret::new("sk-x".to_owned()),
                    // domain-scanner:allow-next-line a bound fixture must still be refused by network policy
                    &["https://api.openwhispr.com"],
                )
                .unwrap(),
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
