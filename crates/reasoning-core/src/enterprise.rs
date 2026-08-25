//! Bedrock, Azure OpenAI, and Vertex AI: three request shapes, one interface.
//!
//! `chat.rs` used to return `ShapeNotImplemented` for all three, which was the
//! honest answer while it was true. It is no longer true.
//!
//! The three are genuinely different, and lumping them together is what upstream
//! did with a single `enterpriseProvider` implementation:
//!
//! | | Auth | Body | Endpoint |
//! | --- | --- | --- | --- |
//! | **Bedrock** | SigV4 over the request | Converse: `system` is separate, content is a list of blocks | region in the host, model in the path |
//! | **Azure** | `api-key` header | OpenAI chat-completions | the org's own resource host, deployment in the path, `api-version` as a **query parameter** |
//! | **Vertex** | OAuth2 bearer, from a service-account JWT | OpenAI chat-completions | region in the host, project in the path |
//!
//! Two of the three speak the OpenAI body and one does not, so `Shaped` carries
//! the body it built rather than assuming.
//!
//! # What is verified and what is not
//!
//! **Verified:** SigV4 against AWS's published vectors (see [`crate::sigv4`]),
//! every endpoint and body shape against the fixtures below, and the Converse
//! response parser against real response shapes.
//!
//! **Not verified:** a live call to any of the three. That needs AWS, Azure or
//! GCP organisation credentials, which this project does not have. Recorded in
//! `docs/PLATFORM_SUPPORT.md` rather than left to be assumed.

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::chat::ChatError;
use crate::params::RequestOptions;
use crate::sigv4;

/// Everything an enterprise connector needs, as the user supplies it.
///
/// One value per provider rather than a bag of optional fields: Bedrock without
/// a region and Azure without a deployment are not "partly configured", they are
/// unconfigured, and an enum makes that unrepresentable rather than checked.
///
/// Stored as JSON in a single OS-credential-store slot. Not in SQLite — some of
/// these fields are secrets, and a struct where two fields are secret and three
/// are not is a struct that gets split across two stores and reassembled wrongly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
// `rename_all` renames the variants; `rename_all_fields` renames what is
// inside them. Without the second, this would have crossed the IPC boundary
// as `access_key_id` while every other struct in the application sends
// camelCase -- which is precisely the drift the generated types exist to
// catch, and which they did catch here.
#[serde(
    tag = "provider",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
#[ts(optional_fields = nullable)]
pub enum EnterpriseConfig {
    Bedrock {
        /// e.g. `us-east-1`. Part of the host *and* of the signature scope.
        region: String,
        access_key_id: String,
        secret_access_key: String,
        /// Present for temporary credentials, absent for long-lived ones.
        #[serde(default)]
        session_token: Option<String>,
    },
    Azure {
        /// The resource endpoint, e.g. `https://my-org.openai.azure.com`.
        endpoint: String,
        /// The deployment name. **This selects the model**, which is why the
        /// registry gives Azure no model list — the name is the org's own.
        deployment: String,
        /// e.g. `2024-10-21`. A query parameter, not a header; Azure rejects a
        /// request without it rather than defaulting.
        api_version: String,
        api_key: String,
    },
    Vertex {
        project_id: String,
        /// e.g. `europe-west4`, or `global`.
        region: String,
        /// The service-account JSON key, verbatim as downloaded.
        service_account_json: String,
    },
}

impl EnterpriseConfig {
    /// The registry id this configures.
    #[must_use]
    pub const fn provider(&self) -> &'static str {
        match self {
            Self::Bedrock { .. } => "bedrock",
            Self::Azure { .. } => "azure",
            Self::Vertex { .. } => "vertex",
        }
    }

    /// Reject a configuration with a field missing, naming the field.
    ///
    /// Called before storing, so an incomplete configuration is refused where
    /// the user is looking at it rather than at the end of their next dictation.
    pub fn validate(&self) -> Result<(), String> {
        let blank = |name: &str, value: &str| -> Result<(), String> {
            if value.trim().is_empty() {
                Err(format!("{name} is required"))
            } else {
                Ok(())
            }
        };

        match self {
            Self::Bedrock {
                region,
                access_key_id,
                secret_access_key,
                ..
            } => {
                blank("region", region)?;
                blank("access key id", access_key_id)?;
                blank("secret access key", secret_access_key)
            }
            Self::Azure {
                endpoint,
                deployment,
                api_version,
                api_key,
            } => {
                blank("endpoint", endpoint)?;
                if !endpoint.starts_with("https://") {
                    return Err("the endpoint must be https".to_owned());
                }
                blank("deployment", deployment)?;
                blank("api version", api_version)?;
                blank("api key", api_key)
            }
            Self::Vertex {
                project_id,
                region,
                service_account_json,
            } => {
                blank("project id", project_id)?;
                blank("region", region)?;
                blank("service account JSON", service_account_json)?;
                // Parsed here rather than at send time: a pasted key that is not
                // JSON is a mistake the user can fix now.
                let key: ServiceAccount = serde_json::from_str(service_account_json)
                    .map_err(|e| format!("the service account JSON could not be read: {e}"))?;
                blank("client_email in the service account", &key.client_email)?;
                blank("private_key in the service account", &key.private_key)
            }
        }
    }

    /// Which fields are filled, for a screen that must never read a value back.
    ///
    /// Returns names and whether each is set, never contents. The Azure endpoint
    /// and the Bedrock region are not secrets, but they are returned as booleans
    /// too — a status call that returns some fields and redacts others invites
    /// the next person to add one to the wrong list.
    #[must_use]
    pub fn filled_fields(&self) -> Vec<(&'static str, bool)> {
        let set = |s: &str| !s.trim().is_empty();
        match self {
            Self::Bedrock {
                region,
                access_key_id,
                secret_access_key,
                session_token,
            } => vec![
                ("region", set(region)),
                ("accessKeyId", set(access_key_id)),
                ("secretAccessKey", set(secret_access_key)),
                ("sessionToken", session_token.as_deref().is_some_and(set)),
            ],
            Self::Azure {
                endpoint,
                deployment,
                api_version,
                api_key,
            } => vec![
                ("endpoint", set(endpoint)),
                ("deployment", set(deployment)),
                ("apiVersion", set(api_version)),
                ("apiKey", set(api_key)),
            ],
            Self::Vertex {
                project_id,
                region,
                service_account_json,
            } => vec![
                ("projectId", set(project_id)),
                ("region", set(region)),
                ("serviceAccountJson", set(service_account_json)),
            ],
        }
    }
}

/// The fields of a Google service-account key this uses.
#[derive(Debug, Clone, Deserialize)]
pub struct ServiceAccount {
    pub client_email: String,
    /// PEM, PKCS#8.
    pub private_key: String,
    #[serde(default = "default_token_uri")]
    pub token_uri: String,
}

fn default_token_uri() -> String {
    "https://oauth2.googleapis.com/token".to_owned()
}

/// A request ready to send: where, what, and the headers it must carry.
#[derive(Debug, Clone)]
pub struct Shaped {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Whether the response is Converse-shaped rather than OpenAI-shaped.
    pub converse_response: bool,
}

/// The OpenAI chat-completions body, as Azure and Vertex both expect it.
fn openai_body(model: &str, messages: &[Value], options: &RequestOptions) -> Value {
    let mut body = json!({
        "model": model,
        "messages": messages,
        "max_tokens": options.max_tokens,
    });

    if let (Some(temperature), Some(map)) = (options.temperature, body.as_object_mut()) {
        map.insert("temperature".to_owned(), json!(temperature));
    }

    body
}

/// The Bedrock Converse body.
///
/// Three differences from OpenAI that each produce a 400 if missed:
///
/// 1. **System messages are not messages.** They go in a top-level `system`
///    array; leaving one in `messages` is rejected with an unhelpful error.
/// 2. **Content is a list of blocks**, so `"content": "hello"` becomes
///    `"content": [{"text": "hello"}]`.
/// 3. **Generation settings live in `inferenceConfig`**, not at the top level,
///    and the token cap is `maxTokens` rather than `max_tokens`.
#[must_use]
pub fn converse_body(messages: &[Value], options: &RequestOptions) -> Value {
    let text_of = |m: &Value| -> String {
        m.get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };

    let system: Vec<Value> = messages
        .iter()
        .filter(|m| m.get("role").and_then(Value::as_str) == Some("system"))
        .map(|m| json!({ "text": text_of(m) }))
        .collect();

    let turns: Vec<Value> = messages
        .iter()
        .filter(|m| m.get("role").and_then(Value::as_str) != Some("system"))
        .map(|m| {
            json!({
                "role": m.get("role").and_then(Value::as_str).unwrap_or("user"),
                "content": [{ "text": text_of(m) }],
            })
        })
        .collect();

    let mut inference = json!({ "maxTokens": options.max_tokens });
    if let (Some(temperature), Some(map)) = (options.temperature, inference.as_object_mut()) {
        map.insert("temperature".to_owned(), json!(temperature));
    }

    let mut body = json!({
        "messages": turns,
        "inferenceConfig": inference,
    });

    // Omitted entirely rather than sent empty: Bedrock rejects `"system": []`
    // on some models and accepts its absence on all of them.
    if !system.is_empty() {
        if let Some(map) = body.as_object_mut() {
            map.insert("system".to_owned(), json!(system));
        }
    }

    body
}

/// Build the Bedrock request, signed.
///
/// `timestamp` is passed in rather than read from the clock so the shape is a
/// pure function and the tests below are not time-dependent.
pub fn shape_bedrock(
    config: &EnterpriseConfig,
    model: &str,
    messages: &[Value],
    options: &RequestOptions,
    timestamp: &str,
) -> Result<Shaped, ChatError> {
    let EnterpriseConfig::Bedrock {
        region,
        access_key_id,
        secret_access_key,
        session_token,
    } = config
    else {
        return Err(mismatch("bedrock", config));
    };

    let body = serde_json::to_vec(&converse_body(messages, options)).unwrap_or_default();
    let host = format!("bedrock-runtime.{region}.amazonaws.com");
    // The model id appears in the path and may contain characters that need
    // encoding — `anthropic.claude-3-5-sonnet-20241022-v2:0` has a colon.
    let path = format!("/model/{}/converse", encode_path_segment(model));

    let signed = sigv4::sign(
        &sigv4::Request {
            method: "POST",
            path: &path,
            query: "",
            headers: vec![
                ("host".to_owned(), host.clone()),
                ("content-type".to_owned(), "application/json".to_owned()),
            ],
            body: &body,
        },
        &sigv4::Scope {
            region,
            service: "bedrock",
            timestamp,
        },
        &sigv4::Credentials {
            access_key_id: access_key_id.clone(),
            secret_access_key: secret_access_key.clone(),
            session_token: session_token.clone(),
        },
    );

    Ok(Shaped {
        url: format!("https://{host}{path}"),
        headers: signed.headers,
        body,
        converse_response: true,
    })
}

/// Percent-encode one path segment, leaving the unreserved set alone.
///
/// Written out because the colon in a Bedrock model id must survive as `%3A` in
/// the *signed* path, and the signature is computed over exactly the string sent.
fn encode_path_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// Build the Azure OpenAI request.
///
/// The deployment name is the model. `api-version` is a query parameter, and
/// authentication is the `api-key` header rather than a bearer token — both are
/// easy to get wrong in a way that reads as a credential problem.
pub fn shape_azure(
    config: &EnterpriseConfig,
    messages: &[Value],
    options: &RequestOptions,
) -> Result<Shaped, ChatError> {
    let EnterpriseConfig::Azure {
        endpoint,
        deployment,
        api_version,
        api_key,
    } = config
    else {
        return Err(mismatch("azure", config));
    };

    let base = endpoint.trim_end_matches('/');
    let url = format!(
        "{base}/openai/deployments/{}/chat/completions?api-version={}",
        encode_path_segment(deployment),
        encode_path_segment(api_version)
    );

    // The deployment already selects the model, and Azure ignores `model` in the
    // body. Sending the deployment name keeps the body valid OpenAI JSON without
    // implying a model id the org may not use.
    let body = serde_json::to_vec(&openai_body(deployment, messages, options)).unwrap_or_default();

    Ok(Shaped {
        url,
        headers: vec![
            ("content-type".to_owned(), "application/json".to_owned()),
            ("api-key".to_owned(), api_key.clone()),
        ],
        body,
        converse_response: false,
    })
}

/// Build the Vertex AI request, given an access token already obtained.
///
/// The token is a separate step ([`vertex_access_token`]) because getting one is
/// itself a network call, and this function stays pure so its endpoint shaping
/// is testable.
pub fn shape_vertex(
    config: &EnterpriseConfig,
    model: &str,
    messages: &[Value],
    options: &RequestOptions,
    access_token: &str,
) -> Result<Shaped, ChatError> {
    let EnterpriseConfig::Vertex {
        project_id, region, ..
    } = config
    else {
        return Err(mismatch("vertex", config));
    };

    // `global` is a real Vertex region with its own host, not a placeholder.
    let host = if region == "global" {
        "aiplatform.googleapis.com".to_owned()
    } else {
        format!("{region}-aiplatform.googleapis.com")
    };

    let url = format!(
        "https://{host}/v1/projects/{}/locations/{}/endpoints/openapi/chat/completions",
        encode_path_segment(project_id),
        encode_path_segment(region)
    );

    // Vertex's OpenAI-compatible endpoint requires the publisher prefix.
    let qualified = if model.contains('/') {
        model.to_owned()
    } else {
        format!("google/{model}")
    };

    let body = serde_json::to_vec(&openai_body(&qualified, messages, options)).unwrap_or_default();

    Ok(Shaped {
        url,
        headers: vec![
            ("content-type".to_owned(), "application/json".to_owned()),
            ("authorization".to_owned(), format!("Bearer {access_token}")),
        ],
        body,
        converse_response: false,
    })
}

fn mismatch(expected: &str, got: &EnterpriseConfig) -> ChatError {
    ChatError::ShapeNotImplemented {
        provider: format!(
            "{expected} was selected but the stored configuration is for {}",
            got.provider()
        ),
    }
}

/* ------------------------------------------------------------ vertex tokens */

/// A signed JWT assertion, ready to exchange for an access token.
///
/// Google's service-account flow: build a JWT claiming the scope you want, sign
/// it RS256 with the account's private key, and POST it to the token endpoint.
/// The assertion is short-lived by design — one hour is the maximum Google
/// accepts, and this uses it.
///
/// `issued_at` is passed in so the function is pure and testable.
pub fn vertex_assertion(account: &ServiceAccount, issued_at: i64) -> Result<String, ChatError> {
    use rsa::pkcs1v15::SigningKey;
    use rsa::pkcs8::DecodePrivateKey as _;
    use rsa::signature::{SignatureEncoding as _, Signer as _};
    use sha2::Sha256;

    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;

    let header = b64.encode(json!({ "alg": "RS256", "typ": "JWT" }).to_string());
    let claims = b64.encode(
        json!({
            "iss": account.client_email,
            "scope": "https://www.googleapis.com/auth/cloud-platform",
            "aud": account.token_uri,
            "iat": issued_at,
            "exp": issued_at + 3600,
        })
        .to_string(),
    );
    let signing_input = format!("{header}.{claims}");

    let key = rsa::RsaPrivateKey::from_pkcs8_pem(&account.private_key).map_err(|e| {
        ChatError::ShapeNotImplemented {
            provider: format!("the Vertex service-account private key could not be read: {e}"),
        }
    })?;

    let signature = SigningKey::<Sha256>::new(key).sign(signing_input.as_bytes());

    Ok(format!(
        "{signing_input}.{}",
        b64.encode(signature.to_bytes())
    ))
}

/// The form body that exchanges an assertion for an access token.
#[must_use]
pub fn vertex_token_request(assertion: &str) -> String {
    format!(
        "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion={assertion}"
    )
}

/// Read an access token out of Google's token response.
///
/// A 2xx with no token is a failure, like everywhere else in this workspace.
pub fn interpret_token(status: u16, body: &str) -> Result<String, ChatError> {
    if !(200..300).contains(&status) {
        return Err(ChatError::ProviderRejected {
            provider: "vertex (token exchange)".to_owned(),
            status,
            body: body.chars().take(400).collect(),
        });
    }

    serde_json::from_str::<Value>(body)
        .ok()
        .as_ref()
        .and_then(|v| v.get("access_token"))
        .and_then(Value::as_str)
        .filter(|t| !t.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ChatError::NoContent {
            provider: "vertex (token exchange)".to_owned(),
        })
}

/* ------------------------------------------------------- converse responses */

/// Read the message text out of a Bedrock Converse response.
///
/// Converse nests it three deep and returns content as a list of blocks, so the
/// OpenAI parser finds nothing and reports "answered but sent no content" — true
/// and useless. Blocks that are not text (a tool use, an image) are skipped
/// rather than rendered as their JSON.
pub fn interpret_converse(status: u16, body: &str) -> Result<String, ChatError> {
    if !(200..300).contains(&status) {
        return Err(ChatError::ProviderRejected {
            provider: "bedrock".to_owned(),
            status,
            body: body.chars().take(400).collect(),
        });
    }

    let parsed: Value = serde_json::from_str(body).map_err(|_| ChatError::NoContent {
        provider: "bedrock".to_owned(),
    })?;

    let text = parsed
        .get("output")
        .and_then(|o| o.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();

    if text.trim().is_empty() {
        return Err(ChatError::NoContent {
            provider: "bedrock".to_owned(),
        });
    }

    Ok(text.trim().to_owned())
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
    use crate::params::Intent;

    fn options() -> RequestOptions {
        RequestOptions {
            intent: Intent::Deterministic,
            temperature: None,
            max_tokens: 512,
            disable_thinking: true,
        }
    }

    fn messages() -> Vec<Value> {
        vec![
            json!({"role": "system", "content": "Be brief."}),
            json!({"role": "user", "content": "Hello."}),
        ]
    }

    fn bedrock() -> EnterpriseConfig {
        EnterpriseConfig::Bedrock {
            region: "eu-west-1".to_owned(),
            access_key_id: "AKIDEXAMPLE".to_owned(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_owned(),
            session_token: None,
        }
    }

    /* ------------------------------------------------------------- bedrock */

    /// System messages leave `messages` and content becomes blocks.
    #[test]
    fn the_converse_body_moves_the_system_prompt_out_of_the_turns() {
        let body = converse_body(&messages(), &options());

        assert_eq!(body["system"][0]["text"], "Be brief.");
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["messages"][0]["role"], "user");
        // A list of blocks, not a string.
        assert_eq!(body["messages"][0]["content"][0]["text"], "Hello.");
        assert!(body["messages"][0]["content"].is_array());
    }

    /// Generation settings live under `inferenceConfig`, spelled Bedrock's way.
    #[test]
    fn the_token_cap_is_inference_config_max_tokens() {
        let body = converse_body(&messages(), &options());
        assert_eq!(body["inferenceConfig"]["maxTokens"], 512);
        assert!(body.get("max_tokens").is_none());
        assert!(body.get("maxTokens").is_none());
    }

    /// An absent system prompt omits the key rather than sending an empty list.
    #[test]
    fn no_system_prompt_means_no_system_key() {
        let body = converse_body(&[json!({"role": "user", "content": "Hi."})], &options());
        assert!(body.get("system").is_none(), "{body}");
    }

    /// The endpoint carries the region and the encoded model id.
    #[test]
    fn the_bedrock_endpoint_encodes_the_model_id() {
        let shaped = shape_bedrock(
            &bedrock(),
            "anthropic.claude-3-5-sonnet-20241022-v2:0",
            &messages(),
            &options(),
            "20150830T123600Z",
        )
        .unwrap();

        assert_eq!(
            shaped.url,
            "https://bedrock-runtime.eu-west-1.amazonaws.com/model/\
             anthropic.claude-3-5-sonnet-20241022-v2%3A0/converse"
        );
        assert!(shaped.converse_response);
    }

    /// The request is signed, and the signature scopes to `bedrock` in-region.
    #[test]
    fn the_bedrock_request_is_signed_for_the_configured_region() {
        let shaped = shape_bedrock(
            &bedrock(),
            "amazon.nova-lite-v1:0",
            &messages(),
            &options(),
            "20150830T123600Z",
        )
        .unwrap();

        let authorization = shaped
            .headers
            .iter()
            .find(|(n, _)| n == "authorization")
            .map(|(_, v)| v.clone())
            .expect("signed");

        assert!(
            authorization
                .contains("Credential=AKIDEXAMPLE/20150830/eu-west-1/bedrock/aws4_request"),
            "{authorization}"
        );
        assert!(authorization.contains("SignedHeaders=content-type;host;x-amz-date"));
    }

    /// A configuration for the wrong provider is refused, not coerced.
    #[test]
    fn a_mismatched_configuration_is_refused() {
        let azure = EnterpriseConfig::Azure {
            endpoint: "https://x.openai.azure.com".to_owned(),
            deployment: "d".to_owned(),
            api_version: "2024-10-21".to_owned(),
            api_key: "k".to_owned(),
        };
        let err = shape_bedrock(&azure, "m", &messages(), &options(), "20150830T123600Z")
            .expect_err("must refuse");
        assert!(format!("{err}").contains("azure"), "{err}");
    }

    /// Converse responses are read from where Converse puts them.
    #[test]
    fn a_converse_response_is_read_from_its_own_shape() {
        let body = r#"{"output":{"message":{"role":"assistant",
            "content":[{"text":"  Hello there.  "}]}},"stopReason":"end_turn"}"#;
        assert_eq!(interpret_converse(200, body).unwrap(), "Hello there.");
    }

    /// Non-text blocks are skipped rather than rendered as JSON.
    #[test]
    fn non_text_content_blocks_are_skipped() {
        let body = r#"{"output":{"message":{"content":[
            {"toolUse":{"name":"x"}},{"text":"the answer"}]}}}"#;
        assert_eq!(interpret_converse(200, body).unwrap(), "the answer");
    }

    /// A 2xx with no text is a failure, as everywhere else.
    #[test]
    fn a_converse_response_with_no_text_is_a_failure() {
        let body = r#"{"output":{"message":{"content":[]}}}"#;
        assert!(matches!(
            interpret_converse(200, body),
            Err(ChatError::NoContent { .. })
        ));
    }

    /* --------------------------------------------------------------- azure */

    /// The deployment is in the path and `api-version` is a query parameter.
    ///
    /// Sending `api-version` as a header is the mistake that reads as a
    /// credential problem: Azure answers 401 rather than "you forgot a
    /// parameter".
    #[test]
    fn azure_puts_the_version_in_the_query_and_the_key_in_a_header() {
        let config = EnterpriseConfig::Azure {
            endpoint: "https://my-org.openai.azure.com/".to_owned(),
            deployment: "gpt-4o-prod".to_owned(),
            api_version: "2024-10-21".to_owned(),
            api_key: "secret".to_owned(),
        };
        let shaped = shape_azure(&config, &messages(), &options()).unwrap();

        assert_eq!(
            shaped.url,
            "https://my-org.openai.azure.com/openai/deployments/gpt-4o-prod/\
             chat/completions?api-version=2024-10-21"
        );
        // `api-key`, not `Authorization: Bearer`.
        assert!(shaped
            .headers
            .iter()
            .any(|(n, v)| n == "api-key" && v == "secret"));
        assert!(!shaped.headers.iter().any(|(n, _)| n == "authorization"));
        assert!(!shaped.converse_response);
    }

    /// The body stays OpenAI-shaped, with the deployment as the model.
    #[test]
    fn azure_sends_an_openai_body() {
        let config = EnterpriseConfig::Azure {
            endpoint: "https://x.openai.azure.com".to_owned(),
            deployment: "my-deployment".to_owned(),
            api_version: "2024-10-21".to_owned(),
            api_key: "k".to_owned(),
        };
        let shaped = shape_azure(&config, &messages(), &options()).unwrap();
        let body: Value = serde_json::from_slice(&shaped.body).unwrap();

        assert_eq!(body["model"], "my-deployment");
        assert_eq!(body["max_tokens"], 512);
        // System stays in `messages` here, unlike Converse.
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "Be brief.");
    }

    /* -------------------------------------------------------------- vertex */

    /// The region is in the host and the project is in the path.
    #[test]
    fn the_vertex_endpoint_carries_the_region_and_project() {
        let config = EnterpriseConfig::Vertex {
            project_id: "my-project".to_owned(),
            region: "europe-west4".to_owned(),
            service_account_json: "{}".to_owned(),
        };
        let shaped =
            shape_vertex(&config, "gemini-2.0-flash", &messages(), &options(), "tok").unwrap();

        assert_eq!(
            shaped.url,
            "https://europe-west4-aiplatform.googleapis.com/v1/projects/my-project/\
             locations/europe-west4/endpoints/openapi/chat/completions"
        );
        assert!(shaped
            .headers
            .iter()
            .any(|(n, v)| n == "authorization" && v == "Bearer tok"));
    }

    /// `global` is a region with its own host, not a prefix.
    #[test]
    fn the_global_region_has_its_own_host() {
        let config = EnterpriseConfig::Vertex {
            project_id: "p".to_owned(),
            region: "global".to_owned(),
            service_account_json: "{}".to_owned(),
        };
        let shaped =
            shape_vertex(&config, "gemini-2.0-flash", &messages(), &options(), "t").unwrap();
        assert!(
            shaped.url.starts_with("https://aiplatform.googleapis.com/"),
            "{}",
            shaped.url
        );
    }

    /// Vertex needs the publisher prefix, and does not get it twice.
    #[test]
    fn a_vertex_model_is_qualified_with_its_publisher_exactly_once() {
        let config = EnterpriseConfig::Vertex {
            project_id: "p".to_owned(),
            region: "us-central1".to_owned(),
            service_account_json: "{}".to_owned(),
        };

        let bare = shape_vertex(&config, "gemini-2.0-flash", &messages(), &options(), "t").unwrap();
        let body: Value = serde_json::from_slice(&bare.body).unwrap();
        assert_eq!(body["model"], "google/gemini-2.0-flash");

        let qualified =
            shape_vertex(&config, "meta/llama-3.1-70b", &messages(), &options(), "t").unwrap();
        let body: Value = serde_json::from_slice(&qualified.body).unwrap();
        assert_eq!(body["model"], "meta/llama-3.1-70b");
    }

    /// A token response without a token is a failure, not an empty token.
    #[test]
    fn a_token_exchange_with_no_token_is_a_failure() {
        assert!(matches!(
            interpret_token(200, r#"{"expires_in":3600}"#),
            Err(ChatError::NoContent { .. })
        ));
        assert!(matches!(
            interpret_token(200, r#"{"access_token":"  "}"#),
            Err(ChatError::NoContent { .. })
        ));
        assert_eq!(
            interpret_token(200, r#"{"access_token":"ya29.abc"}"#).unwrap(),
            "ya29.abc"
        );
        assert!(matches!(
            interpret_token(403, "denied"),
            Err(ChatError::ProviderRejected { status: 403, .. })
        ));
    }

    /// The grant type is the JWT-bearer one, form-encoded.
    #[test]
    fn the_token_request_uses_the_jwt_bearer_grant() {
        let form = vertex_token_request("HEADER.CLAIMS.SIG");
        assert!(form.contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer"));
        assert!(form.ends_with("assertion=HEADER.CLAIMS.SIG"));
    }

    /* ---------------------------------------------------------- validation */

    #[test]
    fn an_incomplete_configuration_names_the_missing_field() {
        let err = EnterpriseConfig::Bedrock {
            region: String::new(),
            access_key_id: "a".to_owned(),
            secret_access_key: "b".to_owned(),
            session_token: None,
        }
        .validate()
        .expect_err("must refuse");
        assert!(err.contains("region"), "{err}");

        let err = EnterpriseConfig::Azure {
            endpoint: "http://insecure.example.com".to_owned(),
            deployment: "d".to_owned(),
            api_version: "v".to_owned(),
            api_key: "k".to_owned(),
        }
        .validate()
        .expect_err("plaintext must be refused");
        assert!(err.contains("https"), "{err}");

        let err = EnterpriseConfig::Vertex {
            project_id: "p".to_owned(),
            region: "r".to_owned(),
            service_account_json: "not json".to_owned(),
        }
        .validate()
        .expect_err("must refuse");
        assert!(err.contains("service account JSON"), "{err}");
    }

    /// The status view says which fields are filled and never what they are.
    #[test]
    fn the_field_report_carries_no_values() {
        let fields = bedrock().filled_fields();
        assert_eq!(fields.len(), 4);
        assert!(fields
            .iter()
            .all(|(_, filled)| *filled || fields.iter().any(|(n, _)| *n == "sessionToken")));

        let printed = format!("{fields:?}");
        assert!(!printed.contains("wJalrXUtnFEMI"), "{printed}");
        assert!(!printed.contains("eu-west-1"), "{printed}");
    }

    /// Configurations round-trip through the credential store as JSON.
    #[test]
    fn a_configuration_round_trips_as_tagged_json() {
        let json = serde_json::to_string(&bedrock()).unwrap();
        assert!(json.contains(r#""provider":"bedrock""#), "{json}");
        let back: EnterpriseConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, bedrock());
    }
}
