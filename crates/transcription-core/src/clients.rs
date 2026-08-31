//! The providers that need their own request shape, not a generic POST.
//!
//! `executor.rs` refused all four `Transport::ProviderClient` providers with
//! `ClientNotImplemented`, which was honest and is now only partly true. Three
//! are implemented here. The fourth is Tinfoil, and it stays refused on purpose
//! — see [`why_tinfoil_is_refused`].
//!
//! Each of the three differs from the OpenAI-compatible multipart POST in a way
//! that produces a 400 or a wrong result if it is treated as the same thing:
//!
//! | | Endpoint | Auth | The trap |
//! | --- | --- | --- | --- |
//! | **Mistral** | `api.mistral.ai/v1/audio/transcriptions` | bearer | the model id is required and its own |
//! | **xAI** | `api.x.ai/v1/audio/transcriptions` | bearer | **takes no `model`**, and `format=true` is only valid for the 25 languages it knows |
//! | **Corti** | tenant- and environment-scoped host | **OAuth2 client credentials**, exchanged first | two requests, and a primary language is required |
//!
//! # What is verified
//!
//! Request shaping and response reading, against the fixtures below. **No live
//! call**, because each needs a key this project does not have. Recorded in
//! `docs/PLATFORM_SUPPORT.md` per provider rather than left to be assumed.

use kw_network_policy::RequestPurpose;
use kw_security::Secret;
use serde_json::Value;

use crate::executor::TranscriptionError;
use crate::route::{CortiConfig, ProviderClientKind};

/// A request one of these clients wants made.
///
/// Built without touching the network so every shaping rule is a unit test.
/// `multipart` carries the form fields; the audio part is added by the sender,
/// which is the only place that holds the bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapedRequest {
    pub provider: &'static str,
    pub endpoint: String,
    /// Form fields besides `file`, in a stable order so a fixture can pin them.
    pub fields: Vec<(String, String)>,
    /// Headers besides authorization.
    pub headers: Vec<(String, String)>,
    pub purpose: RequestPurpose,
}

// There is deliberately no language list in this file. `route::resolve` drops a
// language outside xAI's twenty-five before the request reaches here, so by this
// point `language` is already known-good — and a second copy of that list would
// be one more thing to keep in step with the first.

/// Why Tinfoil is still refused, in one place so the message cannot drift.
///
/// Tinfoil's whole proposition is an *attested* enclave: the client fetches an
/// attestation document, verifies it against a hardware root of trust and a
/// published measurement, and only then sends the audio. A plain HTTPS POST to
/// the same host would transcribe correctly and provide **none** of that — and
/// would appear in the UI as "Tinfoil", which is a claim about confidentiality
/// the request did not earn.
///
/// `route.rs` already refuses a Tinfoil host configured as a plain custom
/// endpoint, with `RouteError::TinfoilClientRequired`, for exactly this reason.
/// Implementing the client here as an unattested POST would defeat that refusal
/// from the inside.
///
/// So: still `ClientNotImplemented`, and this is the reason rather than an
/// absence of effort. What it needs is enclave attestation verification —
/// certificate-chain and measurement checks against a hardware root — which is
/// a security-critical component that cannot be written against fixtures alone.
#[must_use]
pub const fn why_tinfoil_is_refused() -> &'static str {
    "Tinfoil requires verifying an enclave attestation before any audio is sent. A plain HTTPS \
     request would transcribe and would provide none of the confidentiality the name implies, so \
     it is refused rather than approximated."
}

/// Why Mistral cannot be shaped without a model.
const MISTRAL_NEEDS_A_MODEL: &str =
    "a model id is required and the route did not resolve one. There is no default worth \
     inventing: sending a guess produces a 422 that reads as a broken integration.";

/// Why Corti cannot be shaped without both of its scoping values.
const CORTI_NEEDS_A_TENANT: &str =
    "an environment and a tenant are required, and neither has a safe default. Omitting the \
     tenant reaches a real endpoint under the wrong account, which is worse than an error.";

/// Shape the request for one provider, or say why it cannot be shaped.
pub fn shape(
    provider: ProviderClientKind,
    model: Option<&str>,
    language: Option<&str>,
    corti: Option<&CortiConfig>,
) -> Result<ShapedRequest, TranscriptionError> {
    match provider {
        ProviderClientKind::Tinfoil => Err(TranscriptionError::ClientNotImplemented {
            provider: "tinfoil".to_owned(),
            reason: why_tinfoil_is_refused().to_owned(),
        }),

        ProviderClientKind::Mistral => {
            // A model is required and there is no default worth inventing: the
            // registry lists Mistral's own ids, and sending a guess produces a
            // 422 that reads as a broken integration.
            let model = model.filter(|m| !m.trim().is_empty()).ok_or_else(|| {
                TranscriptionError::ClientNotImplemented {
                    provider: "mistral".to_owned(),
                    reason: MISTRAL_NEEDS_A_MODEL.to_owned(),
                }
            })?;

            let mut fields = vec![("model".to_owned(), model.to_owned())];
            if let Some(language) = language {
                fields.push(("language".to_owned(), language.to_owned()));
            }

            Ok(ShapedRequest {
                provider: "mistral",
                endpoint: "https://api.mistral.ai/v1/audio/transcriptions".to_owned(),
                fields,
                headers: Vec::new(),
                purpose: RequestPurpose::UserConfiguredTranscriptionProvider,
            })
        }

        ProviderClientKind::Xai => {
            // **No `model` field.** xAI's speech endpoint takes none, and
            // sending one is rejected rather than ignored. The route sets
            // `model: Some("grok-stt")` for its own bookkeeping; it is
            // deliberately not forwarded.
            let mut fields = Vec::new();

            if let Some(language) = language {
                // Inverse text normalisation. Only valid for a language xAI
                // knows, and `route::resolve` has already dropped anything
                // outside its 25 — so a language present here is a language
                // `format=true` is safe with.
                fields.push(("language".to_owned(), language.to_owned()));
                fields.push(("format".to_owned(), "true".to_owned()));
            }

            Ok(ShapedRequest {
                provider: "xai",
                endpoint: "https://api.x.ai/v1/audio/transcriptions".to_owned(),
                fields,
                headers: Vec::new(),
                purpose: RequestPurpose::UserConfiguredTranscriptionProvider,
            })
        }

        ProviderClientKind::Corti => {
            let config = corti.ok_or_else(|| TranscriptionError::ClientNotImplemented {
                provider: "corti".to_owned(),
                reason: CORTI_NEEDS_A_TENANT.to_owned(),
            })?;

            if !is_valid_corti_environment(&config.environment) || config.tenant.trim().is_empty() {
                return Err(TranscriptionError::ClientNotImplemented {
                    provider: "corti".to_owned(),
                    reason: "the stored environment is not a DNS label or the tenant is blank"
                        .to_owned(),
                });
            }

            let model = model.filter(|m| !m.trim().is_empty()).unwrap_or("premier");

            // Corti requires a concrete primary language; `route::resolve`
            // defaults it to English rather than letting the service guess.
            let language = language.unwrap_or("en");

            Ok(ShapedRequest {
                provider: "corti",
                endpoint: format!(
                    "https://api.{}.corti.app/v2/transcribe",
                    config.environment.trim()
                ),
                fields: vec![
                    ("model".to_owned(), model.to_owned()),
                    ("primaryLanguage".to_owned(), language.to_owned()),
                ],
                // The tenant scopes the request and is not part of the path, so
                // omitting it reaches a real endpoint with the wrong account.
                headers: vec![("Tenant-Name".to_owned(), config.tenant.trim().to_owned())],
                purpose: RequestPurpose::UserConfiguredTranscriptionProvider,
            })
        }
    }
}

/* ------------------------------------------------------------ corti tokens */

/// Whether an environment can safely occupy one DNS label below `corti.app`.
///
/// Keeping this deliberately narrower than a URL parser prevents delimiters,
/// percent-encoding, and subdomains from changing the authority of either
/// Corti endpoint.
#[must_use]
pub fn is_valid_corti_environment(environment: &str) -> bool {
    let environment = environment.trim();
    !environment.is_empty()
        && environment.len() <= 63
        && !environment.starts_with('-')
        && !environment.ends_with('-')
        && environment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// Corti's token endpoint for one environment.
#[must_use]
pub fn corti_token_endpoint(environment: &str) -> String {
    format!(
        "https://auth.{}.corti.app/realms/{}/protocol/openid-connect/token",
        environment.trim(),
        environment.trim()
    )
}

/// The form body that exchanges client credentials for an access token.
///
/// The secret is a client *secret*, not a bearer token, so it goes in the body
/// of a POST rather than an `Authorization` header — and this function returns a
/// `Secret` so the composed string cannot be logged by accident.
#[must_use]
pub fn corti_token_request(client_id: &str, client_secret: &Secret) -> Secret {
    Secret::new(format!(
        "grant_type=client_credentials&client_id={client_id}&client_secret={}",
        client_secret.expose()
    ))
}

/// Read an access token out of Corti's token response.
pub fn interpret_token(status: u16, body: &str) -> Result<Secret, TranscriptionError> {
    if !(200..300).contains(&status) {
        return Err(TranscriptionError::ProviderRejected {
            provider: "corti (token exchange)".to_owned(),
            status,
            body: body.chars().take(300).collect(),
        });
    }

    serde_json::from_str::<Value>(body)
        .ok()
        .as_ref()
        .and_then(|v| v.get("access_token"))
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
        .map(|t| Secret::new(t.to_owned()))
        .ok_or_else(|| TranscriptionError::NoTranscript {
            provider: "corti (token exchange)".to_owned(),
        })
}

/* ---------------------------------------------------------------- responses */

/// Read a transcript out of one of these providers' responses.
///
/// Mistral and xAI both answer with `text`, which `executor::interpret_response`
/// already handles. Corti does not: it returns a list of utterances, and reading
/// it with the `text` rule finds nothing and reports "the route worked and the
/// model did not" — true of the parser, not of the provider.
pub fn interpret(
    provider: &'static str,
    model: Option<&str>,
    status: u16,
    body: &str,
) -> Result<crate::executor::Transcript, TranscriptionError> {
    if provider != "corti" {
        return crate::executor::interpret_response(
            provider,
            model.unwrap_or_default(),
            status,
            body,
        );
    }

    if !(200..300).contains(&status) {
        return Err(TranscriptionError::ProviderRejected {
            provider: provider.to_owned(),
            status,
            body: body.chars().take(300).collect(),
        });
    }

    let parsed: Value =
        serde_json::from_str(body).map_err(|_| TranscriptionError::NoTranscript {
            provider: provider.to_owned(),
        })?;

    // `transcripts[].text`, joined. A single utterance is the common case and a
    // conversation is the reason the field is a list at all.
    let text = parsed
        .get("transcripts")
        .and_then(Value::as_array)
        .map(|utterances| {
            utterances
                .iter()
                .filter_map(|u| u.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_default();

    if text.trim().is_empty() {
        return Err(TranscriptionError::NoTranscript {
            provider: provider.to_owned(),
        });
    }

    Ok(crate::executor::Transcript {
        // Provider clients answer with prose. See `Transcript::segments`.
        segments: Vec::new(),
        text: text.trim().to_owned(),
        provider: provider.to_owned(),
        model: model.map(str::to_owned),
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

    fn corti() -> CortiConfig {
        CortiConfig {
            environment: "eu".to_owned(),
            tenant: "my-clinic".to_owned(),
        }
    }

    /* -------------------------------------------------------------- tinfoil */

    /// Tinfoil is refused, and the refusal explains itself.
    ///
    /// The most important test in this file. An unattested POST to a Tinfoil
    /// host would work, and would present as Tinfoil in the UI while providing
    /// none of what the name means. `route.rs` already refuses one configured as
    /// a custom endpoint; implementing it here would defeat that from inside.
    #[test]
    fn tinfoil_is_refused_and_says_why() {
        let err = shape(ProviderClientKind::Tinfoil, None, None, None).expect_err("must refuse");
        let message = format!("{err}");
        assert!(message.contains("attestation"), "{message}");
        assert!(
            !message.contains("not implemented yet") && !message.contains("coming"),
            "the refusal must not read as an unfinished feature: {message}"
        );
    }

    /* -------------------------------------------------------------- mistral */

    #[test]
    fn mistral_sends_its_own_model_and_the_language() {
        let shaped = shape(
            ProviderClientKind::Mistral,
            Some("voxtral-mini-latest"),
            Some("sv"),
            None,
        )
        .unwrap();

        assert_eq!(shaped.provider, "mistral");
        assert_eq!(
            shaped.endpoint,
            "https://api.mistral.ai/v1/audio/transcriptions"
        );
        assert_eq!(
            shaped.fields,
            vec![
                ("model".to_owned(), "voxtral-mini-latest".to_owned()),
                ("language".to_owned(), "sv".to_owned()),
            ]
        );
    }

    /// No model is a refusal, not a guess.
    #[test]
    fn mistral_without_a_model_is_refused_rather_than_defaulted() {
        for model in [None, Some(""), Some("   ")] {
            assert!(
                shape(ProviderClientKind::Mistral, model, None, None).is_err(),
                "{model:?} should not resolve"
            );
        }
    }

    /* ------------------------------------------------------------------ xai */

    /// xAI takes no `model`, and sending one is rejected rather than ignored.
    ///
    /// The route sets `model: Some("grok-stt")` for its own bookkeeping. This
    /// asserts it does not reach the wire.
    #[test]
    fn xai_sends_no_model_even_when_the_route_resolved_one() {
        let shaped = shape(ProviderClientKind::Xai, Some("grok-stt"), None, None).unwrap();

        assert_eq!(shaped.endpoint, "https://api.x.ai/v1/audio/transcriptions");
        assert!(
            !shaped.fields.iter().any(|(k, _)| k == "model"),
            "{:?}",
            shaped.fields
        );
    }

    /// `format=true` travels with a language and never without one.
    ///
    /// Inverse text normalisation is only valid for a language xAI knows, and
    /// `route::resolve` drops anything outside its 25 — so an absent language
    /// here means "one was dropped", and sending `format=true` anyway would ask
    /// for normalisation in a language the service cannot do it in.
    #[test]
    fn xai_asks_for_normalisation_only_alongside_a_language() {
        let with = shape(ProviderClientKind::Xai, None, Some("en"), None).unwrap();
        assert_eq!(
            with.fields,
            vec![
                ("language".to_owned(), "en".to_owned()),
                ("format".to_owned(), "true".to_owned()),
            ]
        );

        let without = shape(ProviderClientKind::Xai, None, None, None).unwrap();
        assert!(without.fields.is_empty(), "{:?}", without.fields);
    }

    /* ---------------------------------------------------------------- corti */

    /// The environment is in the host and the tenant is a header.
    #[test]
    fn corti_scopes_by_environment_and_tenant() {
        let shaped = shape(
            ProviderClientKind::Corti,
            Some("premier"),
            Some("sv"),
            Some(&corti()),
        )
        .unwrap();

        assert_eq!(shaped.endpoint, "https://api.eu.corti.app/v2/transcribe");
        assert_eq!(
            shaped.headers,
            vec![("Tenant-Name".to_owned(), "my-clinic".to_owned())]
        );
        assert_eq!(
            shaped.fields,
            vec![
                ("model".to_owned(), "premier".to_owned()),
                ("primaryLanguage".to_owned(), "sv".to_owned()),
            ]
        );
    }

    /// A blank environment or tenant is refused, not sent.
    ///
    /// Omitting the tenant reaches a real endpoint under the wrong account,
    /// which is worse than an error.
    #[test]
    fn corti_refuses_a_blank_environment_or_tenant() {
        for (environment, tenant) in [("", "t"), ("eu", ""), ("  ", "t"), ("eu", "  ")] {
            let config = CortiConfig {
                environment: environment.to_owned(),
                tenant: tenant.to_owned(),
            };
            assert!(
                shape(ProviderClientKind::Corti, None, None, Some(&config)).is_err(),
                "{environment:?}/{tenant:?} should not resolve"
            );
        }

        // And with no configuration at all.
        assert!(shape(ProviderClientKind::Corti, None, None, None).is_err());
    }

    #[test]
    fn corti_refuses_environments_that_can_change_the_url_authority() {
        for environment in [
            "x@attacker%2ecom#",
            "eu.corti.app",
            "-eu",
            "eu-",
            "eu/path",
            "eu:443",
        ] {
            let config = CortiConfig {
                environment: environment.to_owned(),
                tenant: "tenant".to_owned(),
            };
            assert!(
                shape(ProviderClientKind::Corti, None, None, Some(&config)).is_err(),
                "{environment:?} should not resolve"
            );
        }

        assert!(is_valid_corti_environment("eu"));
        assert!(is_valid_corti_environment("us-prod"));
    }

    /// A primary language is always sent, because Corti requires one.
    #[test]
    fn corti_always_sends_a_primary_language() {
        let shaped = shape(ProviderClientKind::Corti, None, None, Some(&corti())).unwrap();
        assert!(shaped
            .fields
            .iter()
            .any(|(k, v)| k == "primaryLanguage" && v == "en"));
    }

    #[test]
    fn the_corti_token_endpoint_is_scoped_to_the_environment() {
        assert_eq!(
            corti_token_endpoint("eu"),
            "https://auth.eu.corti.app/realms/eu/protocol/openid-connect/token"
        );
    }

    /// The token form carries the client secret in the body, not a header.
    #[test]
    fn the_corti_token_request_uses_client_credentials() {
        let form = corti_token_request("my-app", &Secret::new("shh".to_owned()));
        assert!(form.expose().contains("grant_type=client_credentials"));
        assert!(form.expose().contains("client_id=my-app"));
        assert!(form.expose().contains("client_secret=shh"));
    }

    /// The composed form is a `Secret`, so it cannot be logged by accident.
    #[test]
    fn the_token_form_does_not_print_itself() {
        let form = corti_token_request("my-app", &Secret::new("shh".to_owned()));
        assert!(!format!("{form:?}").contains("shh"), "{form:?}");
    }

    #[test]
    fn a_token_response_without_a_token_is_a_failure() {
        assert!(interpret_token(200, r#"{"expires_in":300}"#).is_err());
        assert!(interpret_token(200, r#"{"access_token":"  "}"#).is_err());
        assert!(interpret_token(401, "denied").is_err());
        assert_eq!(
            interpret_token(200, r#"{"access_token":"abc"}"#)
                .unwrap()
                .expose(),
            "abc"
        );
    }

    /* ------------------------------------------------------------ responses */

    /// Corti returns utterances, not a `text` field.
    ///
    /// Reading it with the OpenAI rule finds nothing and reports "answered but
    /// sent no transcript", which blames the provider for a parser bug.
    #[test]
    fn a_corti_response_is_read_from_its_own_shape() {
        let body = r#"{"transcripts":[
            {"text":"Patient reports "},{"text":"a headache."}]}"#;
        let transcript = interpret("corti", Some("premier"), 200, body).unwrap();
        assert_eq!(transcript.text, "Patient reports  a headache.");
        assert_eq!(transcript.provider, "corti");
        assert_eq!(transcript.model.as_deref(), Some("premier"));
    }

    /// An empty utterance list is a failure, not an empty transcript.
    #[test]
    fn a_corti_response_with_no_utterances_is_a_failure() {
        assert!(interpret("corti", None, 200, r#"{"transcripts":[]}"#).is_err());
        assert!(interpret("corti", None, 200, r#"{"other":1}"#).is_err());
    }

    /// Mistral and xAI go through the shared `text` reader.
    #[test]
    fn the_other_two_use_the_openai_response_rule() {
        let transcript = interpret(
            "mistral",
            Some("voxtral-mini-latest"),
            200,
            r#"{"text":"  hello  ","model":"voxtral-mini-2507"}"#,
        )
        .unwrap();
        assert_eq!(transcript.text, "hello");
        // What the provider says it used beats what we asked for.
        assert_eq!(transcript.model.as_deref(), Some("voxtral-mini-2507"));

        assert!(interpret("xai", None, 200, r#"{"other":1}"#).is_err());
        assert!(interpret("xai", None, 500, "boom").is_err());
    }

    /// Every client that is implemented uses the transcription purpose.
    ///
    /// Not `ExplicitlyConfiguredSelfHostedEndpoint`, which is the only purpose
    /// permitted to use plaintext HTTP to a private address — these are public
    /// hosts over TLS, and mislabelling one would widen the boundary.
    #[test]
    fn every_client_declares_the_transcription_purpose() {
        let shaped = [
            shape(ProviderClientKind::Mistral, Some("m"), None, None).unwrap(),
            shape(ProviderClientKind::Xai, None, None, None).unwrap(),
            shape(ProviderClientKind::Corti, None, None, Some(&corti())).unwrap(),
        ];
        for request in shaped {
            assert_eq!(
                request.purpose,
                RequestPurpose::UserConfiguredTranscriptionProvider,
                "{}",
                request.provider
            );
            assert!(
                request.endpoint.starts_with("https://"),
                "{}",
                request.endpoint
            );
        }
    }
}
