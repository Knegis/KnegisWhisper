//! Sending a chat-completions request, and the dictation cleanup that uses it.
//!
//! Cleanup is the reason this crate exists in the dictation slice: a raw
//! transcript has no punctuation worth speaking of, keeps every "um", and
//! renders proper nouns however the acoustic model felt. A single deterministic
//! pass over it is the difference between dictation being usable and being a
//! demo.
//!
//! The prompt is deliberately narrow, and the reason is a real failure mode:
//! given latitude, a model will *answer* a dictated question rather than
//! transcribe it. Someone dictating "should we move the meeting to Tuesday" into
//! an email wants that sentence, not a model's opinion on scheduling.

use kw_network_policy::{NetworkPolicy, RequestPurpose};
use kw_security::{CredentialStore, Secret};
use serde_json::{json, Value};

use crate::enterprise::{self, EnterpriseConfig};
use crate::params::{build_body, shape_for, Intent, ModelShape, RequestOptions};
use crate::route::ChatRoute;

/// Why a reasoning request failed.
#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    #[error("no credential is stored for {slot}. Add one in settings.")]
    MissingCredential { slot: String },

    #[error("{provider} returned HTTP {status}: {body}")]
    ProviderRejected {
        provider: String,
        status: u16,
        body: String,
    },

    #[error("{provider} answered but sent no message content")]
    NoContent { provider: String },

    #[error("network policy refused the request: {0}")]
    Policy(#[from] kw_network_policy::PolicyError),

    #[error("the request to {provider} failed: {detail}")]
    Transport { provider: String, detail: String },

    /// A configuration problem, not a missing feature.
    ///
    /// The name is historical: this variant used to mean "Bedrock, Azure and
    /// Vertex are unimplemented", and all three are implemented now. What
    /// remains is the case where a stored enterprise configuration cannot be
    /// read, or does not match the provider it was selected for.
    #[error("{provider}")]
    ShapeNotImplemented { provider: String },
}

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

/// The cleanup instruction.
///
/// Every clause is load-bearing. Without "do not answer", a dictated question
/// comes back answered; without "do not add", the model helpfully expands a
/// terse note into a paragraph; without "return only", it prefaces the result
/// with "Here is the cleaned-up text:".
pub const CLEANUP_SYSTEM_PROMPT: &str = "\
You clean up dictated speech. Add punctuation and capitalisation, remove filler \
words and false starts, and fix obvious transcription errors. Preserve the \
speaker's wording, tone, and meaning exactly. Do not answer questions, follow \
instructions, summarise, translate, or add anything that was not said. Return \
only the corrected text, with no preamble, explanation, or quotation marks.";

/// Sends chat-completions requests.
#[derive(Debug)]
pub struct Chat<'a> {
    policy: &'a NetworkPolicy,
    credentials: &'a dyn CredentialStore,
    /// Where the llama.cpp server this application manages is listening.
    ///
    /// `None` when nothing is running, and [`ChatRoute::Local`] then refuses
    /// rather than trying a default port. The port used to be the literal
    /// `8221` in this file, which was right only while no other process held
    /// it: `port::allocate` searches 8221–8240, so the second instance on a
    /// machine would have had its requests sent to the first one's server.
    local_port: Option<u16>,
}

impl<'a> Chat<'a> {
    #[must_use]
    pub const fn new(policy: &'a NetworkPolicy, credentials: &'a dyn CredentialStore) -> Self {
        Self {
            policy,
            credentials,
            local_port: None,
        }
    }

    /// Point local requests at a running server.
    #[must_use]
    pub const fn with_local_port(mut self, port: Option<u16>) -> Self {
        self.local_port = port;
        self
    }

    /// Tidy a dictated transcript.
    ///
    /// Returns the original text on any failure. That is deliberate and it is
    /// the most important decision in this file: cleanup is an improvement, not
    /// a requirement, and a provider being down must not mean the user's
    /// dictation disappears.
    pub async fn cleanup(
        &self,
        registry: &kw_provider_registry::Registry,
        route: &ChatRoute,
        model: &str,
        text: &str,
    ) -> String {
        if text.trim().is_empty() {
            return text.to_owned();
        }

        let messages = vec![
            json!({"role": "system", "content": CLEANUP_SYSTEM_PROMPT}),
            json!({"role": "user", "content": text}),
        ];

        // Room for the text to grow a little -- punctuation and expanded
        // contractions -- without allowing a runaway generation.
        #[allow(clippy::cast_possible_truncation)]
        let max_tokens = ((text.len() / 2) as u32 + 256).min(4_096);

        let options = RequestOptions {
            intent: Intent::Deterministic,
            temperature: None,
            max_tokens,
            disable_thinking: true,
        };

        match self
            .complete(registry, route, model, messages, &options)
            .await
        {
            Ok(cleaned) if !cleaned.trim().is_empty() => cleaned,
            Ok(_) => {
                tracing::warn!("cleanup returned nothing; keeping the raw transcript");
                text.to_owned()
            }
            Err(err) => {
                // The transcript is worth more than the tidying.
                tracing::warn!(error = %err, "cleanup failed; keeping the raw transcript");
                text.to_owned()
            }
        }
    }

    /// Send a completion and return the message content.
    pub async fn complete(
        &self,
        registry: &kw_provider_registry::Registry,
        route: &ChatRoute,
        model: &str,
        messages: Vec<Value>,
        options: &RequestOptions,
    ) -> Result<String, ChatError> {
        // The purpose is a property of the destination, not the feature. A
        // self-hosted or local server is "an endpoint the user pointed us at",
        // and only that purpose may use plaintext HTTP to a private address --
        // so getting this wrong makes a local llama.cpp server unreachable, with
        // the policy layer refusing the scheme before anything is sent.
        let (provider, base_url, key_ref, shape, purpose) = match route {
            ChatRoute::Provider {
                id,
                base_url,
                key_ref,
            } => (
                id.clone(),
                base_url.clone(),
                Some(key_ref.clone()),
                shape_for(registry, id, model),
                RequestPurpose::UserConfiguredAiProvider,
            ),

            ChatRoute::SelfHosted { base_url, key_ref } => (
                "self-hosted".to_owned(),
                base_url.clone(),
                key_ref.clone(),
                // A server the user runs is llama.cpp-shaped until told otherwise.
                ModelShape::default(),
                RequestPurpose::ExplicitlyConfiguredSelfHostedEndpoint,
            ),

            ChatRoute::Local => {
                // Refused rather than guessed. A request to a port nothing is
                // listening on fails with a connection error that reads as a
                // network problem, and the real answer is "no server is
                // running" — which the caller can act on.
                let port = self.local_port.ok_or_else(|| ChatError::Transport {
                    provider: "local".to_owned(),
                    detail: "the local text-generation server is not running".to_owned(),
                })?;

                (
                    "local".to_owned(),
                    format!("http://127.0.0.1:{port}/v1"),
                    None,
                    ModelShape::default(),
                    RequestPurpose::ExplicitlyConfiguredSelfHostedEndpoint,
                )
            }

            ChatRoute::Enterprise { id } => {
                // Its own signing, its own body, and for Bedrock its own
                // response shape. Nothing below this point applies.
                return self.send_enterprise(id, model, &messages, options).await;
            }
        };

        let credential = match &key_ref {
            Some(slot) => match self.credentials.get(slot) {
                Ok(secret) if !secret.is_empty() => Some(secret),
                // A self-hosted server legitimately needs none; a cloud
                // provider does not.
                _ if matches!(route, ChatRoute::SelfHosted { .. } | ChatRoute::Local) => None,
                _ => {
                    return Err(ChatError::MissingCredential {
                        slot: slot.slot().to_owned(),
                    })
                }
            },
            None => None,
        };

        let body = build_body(&shape, model, messages, options);
        let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));

        self.post(&provider, &endpoint, purpose, credential.as_ref(), &body)
            .await
    }

    /// Send to Bedrock, Azure or Vertex.
    ///
    /// The configuration lives in the OS credential store under the provider's
    /// own slot, as one JSON document. Not split across settings and the
    /// keychain: a struct where two fields are secret and three are not is a
    /// struct that gets reassembled wrongly, and the whole thing is small.
    async fn send_enterprise(
        &self,
        id: &str,
        model: &str,
        messages: &[Value],
        options: &RequestOptions,
    ) -> Result<String, ChatError> {
        let slot = kw_security::KeyRef::new(id).ok_or_else(|| ChatError::MissingCredential {
            slot: id.to_owned(),
        })?;

        let stored = self
            .credentials
            .get(&slot)
            .ok()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ChatError::MissingCredential {
                slot: id.to_owned(),
            })?;

        let config: EnterpriseConfig =
            serde_json::from_str(stored.expose()).map_err(|e| ChatError::ShapeNotImplemented {
                provider: format!("the stored {id} configuration could not be read: {e}"),
            })?;
        config
            .validate()
            .map_err(|detail| ChatError::ShapeNotImplemented {
                provider: format!("the stored {id} configuration is invalid: {detail}"),
            })?;

        let shaped = match &config {
            EnterpriseConfig::Bedrock { .. } => {
                // SigV4 covers a timestamp, and AWS rejects one more than five
                // minutes from its own clock. Read here rather than passed in,
                // because this is the one place it is not a test.
                let now = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
                enterprise::shape_bedrock(&config, model, messages, options, &now)?
            }
            EnterpriseConfig::Azure { .. } => enterprise::shape_azure(&config, messages, options)?,
            EnterpriseConfig::Vertex { .. } => {
                let token = self.vertex_access_token(&config).await?;
                enterprise::shape_vertex(&config, model, messages, options, &token)?
            }
        };

        let mut outbound = self
            .policy
            .post(RequestPurpose::UserConfiguredAiProvider, &shaped.url)?;
        for (name, value) in &shaped.headers {
            // `host` is set by the transport from the URL. Sending it again
            // would duplicate the header, and it is covered by the SigV4
            // signature either way.
            if name != "host" {
                outbound = outbound.header(name, value);
            }
        }

        tracing::debug!(provider = id, "sending an enterprise completion request");

        let response =
            outbound
                .body(shaped.body)
                .send()
                .await
                .map_err(|e| ChatError::Transport {
                    provider: id.to_owned(),
                    detail: e.to_string(),
                })?;

        let status = response.status().as_u16();
        let text = response.text().await.map_err(|e| ChatError::Transport {
            provider: id.to_owned(),
            detail: e.to_string(),
        })?;

        if shaped.converse_response {
            enterprise::interpret_converse(status, &text)
        } else {
            interpret_completion(id, status, &text)
        }
    }

    /// Exchange a service-account assertion for a Vertex access token.
    ///
    /// A network call of its own, against Google's token endpoint rather than
    /// the model endpoint, and under the same purpose: it is one action the user
    /// configured. Not cached — an hour of validity against a dictation that
    /// happens once a minute is not worth a cache that has to be invalidated
    /// whenever the configuration changes.
    async fn vertex_access_token(&self, config: &EnterpriseConfig) -> Result<String, ChatError> {
        let EnterpriseConfig::Vertex {
            service_account_json,
            ..
        } = config
        else {
            return Err(ChatError::ShapeNotImplemented {
                provider: "vertex was selected with another provider's configuration".to_owned(),
            });
        };

        let account: enterprise::ServiceAccount = serde_json::from_str(service_account_json)
            .map_err(|e| ChatError::ShapeNotImplemented {
                provider: format!("the Vertex service account could not be read: {e}"),
            })?;

        if account.token_uri != enterprise::VERTEX_TOKEN_URI {
            return Err(ChatError::ShapeNotImplemented {
                provider: "the Vertex token endpoint is not Google's OAuth endpoint".to_owned(),
            });
        }

        let assertion = enterprise::vertex_assertion(&account, chrono::Utc::now().timestamp())?;

        let response = self
            .policy
            .post(RequestPurpose::UserConfiguredAiProvider, &account.token_uri)?
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(enterprise::vertex_token_request(&assertion))
            .send()
            .await
            .map_err(|e| ChatError::Transport {
                provider: "vertex (token exchange)".to_owned(),
                detail: e.to_string(),
            })?;

        let status = response.status().as_u16();
        let text = response.text().await.map_err(|e| ChatError::Transport {
            provider: "vertex (token exchange)".to_owned(),
            detail: e.to_string(),
        })?;

        enterprise::interpret_token(status, &text)
    }

    async fn post(
        &self,
        provider: &str,
        endpoint: &str,
        purpose: RequestPurpose,
        credential: Option<&Secret>,
        body: &Value,
    ) -> Result<String, ChatError> {
        // Never the prompt, never the transcript. The transcript is the user's
        // speech and the brief forbids logging it.
        tracing::debug!(provider, "sending a completion request");

        let mut outbound = self.policy.post(purpose, endpoint)?;

        if let Some(secret) = credential {
            outbound = outbound.bearer_auth(secret.expose());
        }

        let response = outbound
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|e| ChatError::Transport {
                provider: provider.to_owned(),
                detail: e.to_string(),
            })?;

        let status = response.status().as_u16();
        let text = response.text().await.map_err(|e| ChatError::Transport {
            provider: provider.to_owned(),
            detail: e.to_string(),
        })?;

        interpret_completion(provider, status, &text)
    }
}

/// Turn a completion response into content or an error.
///
/// Separate so the rules are tested without a network. The rule that matters is
/// the same one as everywhere else in this workspace: a 2xx with no content is a
/// failure, not empty content.
pub fn interpret_completion(provider: &str, status: u16, body: &str) -> Result<String, ChatError> {
    if !(200..300).contains(&status) {
        return Err(ChatError::ProviderRejected {
            provider: provider.to_owned(),
            status,
            body: truncate(body.to_owned()),
        });
    }

    let parsed: Value = serde_json::from_str(body).map_err(|_| ChatError::NoContent {
        provider: provider.to_owned(),
    })?;

    let content = parsed
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();

    if content.trim().is_empty() {
        return Err(ChatError::NoContent {
            provider: provider.to_owned(),
        });
    }

    Ok(content.trim().to_owned())
}

/// Whether a finish reason means the output was cut off at the token cap.
///
/// Providers disagree on the spelling, so both are checked.
#[must_use]
pub fn is_truncated(finish_reason: Option<&str>) -> bool {
    matches!(finish_reason, Some("length" | "max_tokens"))
}

/// Extract the finish reason, if the provider sent one.
#[must_use]
pub fn finish_reason(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body)
        .ok()?
        .get("choices")?
        .get(0)?
        .get("finish_reason")?
        .as_str()
        .map(str::to_owned)
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

    fn registry() -> &'static kw_provider_registry::Registry {
        kw_provider_registry::registry().expect("the shipped registry must parse")
    }

    /* ---------------------------------------------- a 2xx is not a success */

    #[test]
    fn a_completion_with_content_succeeds_and_is_trimmed() {
        let out = interpret_completion(
            "openai",
            200,
            r#"{"choices":[{"message":{"content":"  Hello there.  "}}]}"#,
        )
        .unwrap();
        assert_eq!(out, "Hello there.");
    }

    #[test]
    fn a_2xx_with_no_content_is_a_failure() {
        for body in [
            r#"{"choices":[{"message":{"content":""}}]}"#,
            r#"{"choices":[{"message":{"content":"   "}}]}"#,
            r#"{"choices":[]}"#,
            r#"{}"#,
            "<html>gateway</html>",
        ] {
            assert!(
                matches!(
                    interpret_completion("openai", 200, body),
                    Err(ChatError::NoContent { .. })
                ),
                "{body}"
            );
        }
    }

    #[test]
    fn a_rejection_carries_the_status_and_a_bounded_body() {
        match interpret_completion("groq", 400, &"x".repeat(5_000)) {
            Err(ChatError::ProviderRejected { status, body, .. }) => {
                assert_eq!(status, 400);
                assert!(body.chars().count() <= BODY_LIMIT + 1);
            }
            other => panic!("{other:?}"),
        }
    }

    /* -------------------------------------------------------- truncation */

    #[test]
    fn both_spellings_of_a_truncated_finish_are_recognised() {
        assert!(is_truncated(Some("length")));
        assert!(is_truncated(Some("max_tokens")));
        assert!(!is_truncated(Some("stop")));
        assert!(!is_truncated(None));
    }

    #[test]
    fn the_finish_reason_is_extracted_when_present() {
        let body = r#"{"choices":[{"finish_reason":"length","message":{"content":"x"}}]}"#;
        assert_eq!(finish_reason(body).as_deref(), Some("length"));
        assert_eq!(finish_reason(r#"{"choices":[{}]}"#), None);
        assert_eq!(finish_reason("nonsense"), None);
    }

    /* ------------------------------------------------- cleanup never loses text */

    #[tokio::test]
    async fn cleanup_returns_the_original_when_the_provider_is_unreachable() {
        // The most important behaviour in this file. Cleanup is an improvement,
        // not a requirement; a provider being down must not lose the dictation.
        let policy = NetworkPolicy::new().unwrap();
        let creds = MemoryCredentialStore::new();
        let chat = Chat::new(&policy, &creds);

        // No credential stored, so the request cannot even be attempted.
        let route = ChatRoute::Provider {
            id: "openai".to_owned(),
            base_url: "https://api.openai.com/v1".to_owned(),
            key_ref: KeyRef::new("openai").unwrap(),
        };

        let original = "um so i think we should ship it";
        let out = chat
            .cleanup(registry(), &route, "gpt-4o-mini", original)
            .await;
        assert_eq!(
            out, original,
            "the transcript must survive a cleanup failure"
        );
    }

    #[tokio::test]
    async fn cleanup_of_empty_text_does_not_send_a_request() {
        let policy = NetworkPolicy::new().unwrap();
        let creds = MemoryCredentialStore::new();
        let chat = Chat::new(&policy, &creds);
        let route = ChatRoute::Local;

        for text in ["", "   ", "\n"] {
            assert_eq!(chat.cleanup(registry(), &route, "m", text).await, text);
        }
    }

    /* -------------------------------------------------------------- the prompt */

    #[test]
    fn the_cleanup_prompt_forbids_the_model_answering_the_dictation() {
        // Without this the model answers a dictated question instead of
        // transcribing it, which is the failure that makes cleanup unusable.
        let prompt = CLEANUP_SYSTEM_PROMPT.to_lowercase();
        assert!(prompt.contains("do not answer"), "{prompt}");
        assert!(prompt.contains("return only"), "{prompt}");
        assert!(prompt.contains("preserve"), "{prompt}");
        assert!(prompt.contains("not said") || prompt.contains("add anything"));
    }

    /* --------------------------------------------------------------- routing */

    /// An unconfigured enterprise provider is a missing credential.
    ///
    /// It used to be `ShapeNotImplemented`, which was the honest answer while
    /// the three shapes were unwritten. They are written now, so the only thing
    /// missing is the configuration — and saying "not implemented" for that
    /// would send someone looking for a feature rather than for a settings page.
    #[tokio::test]
    async fn an_unconfigured_enterprise_provider_says_the_configuration_is_missing() {
        let policy = NetworkPolicy::new().unwrap();
        let creds = MemoryCredentialStore::new();
        let chat = Chat::new(&policy, &creds);

        let err = chat
            .complete(
                registry(),
                &ChatRoute::Enterprise {
                    id: "bedrock".to_owned(),
                },
                "anthropic.claude-v2",
                vec![json!({"role": "user", "content": "hi"})],
                &RequestOptions::default(),
            )
            .await
            .unwrap_err();

        match err {
            ChatError::MissingCredential { slot } => assert_eq!(slot, "bedrock"),
            other => panic!("expected a missing configuration, got {other:?}"),
        }
    }

    /// A stored configuration for the wrong provider is refused, not coerced.
    ///
    /// The case that matters: someone configures Azure, switches the provider to
    /// Bedrock, and the Azure key is still in the Bedrock slot. Sending an Azure
    /// key to AWS would be a credential in a request nobody intended.
    #[tokio::test]
    async fn a_configuration_for_another_provider_is_refused() {
        let policy = NetworkPolicy::new().unwrap();
        let creds = MemoryCredentialStore::new();
        let slot = KeyRef::new("bedrock").unwrap();
        creds
            .set(
                &slot,
                &kw_security::Secret::new(
                    serde_json::to_string(&crate::enterprise::EnterpriseConfig::Azure {
                        endpoint: "https://x.openai.azure.com".to_owned(),
                        deployment: "d".to_owned(),
                        api_version: "2024-10-21".to_owned(),
                        api_key: "k".to_owned(),
                    })
                    .unwrap(),
                ),
            )
            .unwrap();

        let err = Chat::new(&policy, &creds)
            .complete(
                registry(),
                &ChatRoute::Enterprise {
                    id: "bedrock".to_owned(),
                },
                "amazon.nova-lite-v1:0",
                vec![json!({"role": "user", "content": "hi"})],
                &RequestOptions::default(),
            )
            .await
            .unwrap_err();

        assert!(
            format!("{err}").contains("azure"),
            "the refusal must name what was actually stored: {err}"
        );
    }

    /// A stored blob that is not a configuration is refused before any request.
    #[tokio::test]
    async fn an_unreadable_configuration_is_refused_before_anything_is_sent() {
        let policy = NetworkPolicy::new().unwrap();
        let creds = MemoryCredentialStore::new();
        let slot = KeyRef::new("azure").unwrap();
        creds
            .set(&slot, &kw_security::Secret::new("not json".to_owned()))
            .unwrap();

        let err = Chat::new(&policy, &creds)
            .complete(
                registry(),
                &ChatRoute::Enterprise {
                    id: "azure".to_owned(),
                },
                "whatever",
                vec![json!({"role": "user", "content": "hi"})],
                &RequestOptions::default(),
            )
            .await
            .unwrap_err();

        match err {
            ChatError::ShapeNotImplemented { provider } => {
                assert!(provider.contains("could not be read"), "{provider}");
            }
            other => panic!("expected a configuration error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_cloud_provider_without_a_credential_is_an_error() {
        let policy = NetworkPolicy::new().unwrap();
        let creds = MemoryCredentialStore::new();
        let chat = Chat::new(&policy, &creds);

        let err = chat
            .complete(
                registry(),
                &ChatRoute::Provider {
                    id: "openai".to_owned(),
                    base_url: "https://api.openai.com/v1".to_owned(),
                    key_ref: KeyRef::new("openai").unwrap(),
                },
                "gpt-4o-mini",
                vec![json!({"role": "user", "content": "hi"})],
                &RequestOptions::default(),
            )
            .await
            .unwrap_err();

        assert!(matches!(err, ChatError::MissingCredential { .. }));
    }

    #[tokio::test]
    async fn a_self_hosted_route_without_a_credential_is_not_an_error() {
        // llama.cpp and LM Studio need no key, so requiring one would make
        // self-hosting impossible.
        let policy = NetworkPolicy::new().unwrap();
        let creds = MemoryCredentialStore::new();
        let chat = Chat::new(&policy, &creds);

        let err = chat
            .complete(
                registry(),
                &ChatRoute::SelfHosted {
                    base_url: "http://127.0.0.1:9/v1".to_owned(),
                    key_ref: KeyRef::new("custom"),
                },
                "local-model",
                vec![json!({"role": "user", "content": "hi"})],
                &RequestOptions::default(),
            )
            .await
            .unwrap_err();

        // It got as far as trying to connect, which is the point.
        assert!(
            matches!(err, ChatError::Transport { .. }),
            "expected a connection failure rather than a credential one, got {err:?}"
        );
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
        let chat = Chat::new(&policy, &creds);

        let err = chat
            .complete(
                registry(),
                &ChatRoute::SelfHosted {
                    // domain-scanner:allow-next-line asserting the runtime blocklist refuses a vendor host
                    base_url: "https://api.openwhispr.com/v1".to_owned(),
                    key_ref: KeyRef::new("custom"),
                },
                "m",
                vec![json!({"role": "user", "content": "hi"})],
                &RequestOptions::default(),
            )
            .await
            .unwrap_err();

        assert!(matches!(err, ChatError::Policy(_)), "{err:?}");
    }

    #[tokio::test]
    async fn a_local_llama_server_is_not_refused_by_the_policy_layer() {
        // The bug this guards: with the AI-provider purpose the policy layer
        // refuses http:// outright, so the local model route can never work and
        // the error blames the scheme rather than the missing server.
        let policy = NetworkPolicy::new().unwrap();
        let creds = MemoryCredentialStore::new();
        let chat = Chat::new(&policy, &creds);

        let err = chat
            .complete(
                registry(),
                &ChatRoute::Local,
                "qwen3-4b",
                vec![json!({"role": "user", "content": "hi"})],
                &RequestOptions::default(),
            )
            .await
            .unwrap_err();

        match err {
            ChatError::Transport { .. } => {}
            other => panic!("expected a connection failure, not a policy refusal: {other:?}"),
        }
    }

    #[test]
    fn no_error_message_contains_a_credential() {
        let errors = [
            ChatError::MissingCredential {
                slot: "openai".to_owned(),
            },
            ChatError::ProviderRejected {
                provider: "openai".to_owned(),
                status: 401,
                body: "invalid key".to_owned(),
            },
            ChatError::NoContent {
                provider: "groq".to_owned(),
            },
        ];
        for err in errors {
            let rendered = err.to_string();
            assert!(!rendered.contains("sk-"), "{rendered}");
            assert!(!rendered.contains("Bearer "), "{rendered}");
        }
    }
}
