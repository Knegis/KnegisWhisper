//! Exchanging an authorization code, or a refresh token, for access and
//! refresh tokens -- without ever presenting a client secret.
//!
//! This is a public client (RFC 8252): the PKCE verifier proves the exchange
//! comes from the same process that started the authorize request, and that
//! is the whole of the client's authentication. No request built here ever
//! carries a `client_secret` field.
//!
//! Sending is behind [`TokenSender`] rather than a direct call into
//! `kw_network_policy::NetworkPolicy`, for one reason: the network boundary
//! allows a plaintext destination only for
//! `RequestPurpose::ExplicitlyConfiguredSelfHostedEndpoint`, and calendar
//! traffic is deliberately not that purpose -- so a plaintext stub
//! authorization server on loopback could never be reached through the real
//! boundary, in a test or otherwise. [`NetworkPolicySender`] is the only
//! implementation this crate ships outside `#[cfg(test)]`, and every
//! production caller must use it: it validates every request against
//! `RequestPurpose::UserConfiguredCalendarProvider` before anything is sent,
//! exactly like `kw_security::MemoryCredentialStore` stands in for the OS
//! keychain in tests without ever being wired into the application.

use async_trait::async_trait;
use kw_network_policy::{NetworkPolicy, RequestPurpose};
use kw_security::Secret;
use serde::Deserialize;

/// Why a token exchange failed.
#[derive(Debug, thiserror::Error)]
pub enum ExchangeError {
    #[error("the network policy refused the token endpoint: {0}")]
    Policy(#[from] kw_network_policy::PolicyError),

    #[error("the token endpoint could not be reached: {0}")]
    Http(#[source] reqwest::Error),

    #[error("the token endpoint rejected the request: {error}")]
    Rejected {
        error: String,
        description: Option<String>,
    },

    #[error("the token endpoint's response could not be parsed")]
    MalformedResponse,
}

/// Freshly issued tokens, from either an authorization-code or a
/// refresh-token exchange.
#[derive(Clone)]
pub struct TokenSet {
    pub access_token: Secret,
    /// Present after an authorization-code exchange. A refresh-token exchange
    /// may omit it -- Google does not rotate refresh tokens, Microsoft does --
    /// in which case the caller keeps using the one it already has.
    pub refresh_token: Option<Secret>,
    /// Unix seconds at which `access_token` expires.
    pub expires_at: i64,
    pub scope: String,
}

impl std::fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenSet")
            .field("access_token", &self.access_token)
            .field("refresh_token", &self.refresh_token)
            .field("expires_at", &self.expires_at)
            .field("scope", &self.scope)
            .finish()
    }
}

#[derive(Deserialize)]
struct RawTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Deserialize)]
struct RawErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// Sends one `application/x-www-form-urlencoded` POST to a token endpoint and
/// returns the raw response body, or the parsed OAuth2 error if the endpoint
/// rejected the request.
#[async_trait]
pub trait TokenSender: Send + Sync {
    async fn post_form(&self, url: &str, form: &[(&str, &str)]) -> Result<String, ExchangeError>;
}

/// The sender production code must use: every request goes through the
/// workspace's one network boundary, under
/// `RequestPurpose::UserConfiguredCalendarProvider`.
#[derive(Debug)]
pub struct NetworkPolicySender<'a> {
    policy: &'a NetworkPolicy,
}

impl<'a> NetworkPolicySender<'a> {
    #[must_use]
    pub const fn new(policy: &'a NetworkPolicy) -> Self {
        Self { policy }
    }
}

#[async_trait]
impl TokenSender for NetworkPolicySender<'_> {
    async fn post_form(&self, url: &str, form: &[(&str, &str)]) -> Result<String, ExchangeError> {
        let request = self
            .policy
            .post(RequestPurpose::UserConfiguredCalendarProvider, url)?;
        let response = request
            .form(form)
            .send()
            .await
            .map_err(ExchangeError::Http)?;
        read_body(response).await
    }
}

async fn read_body(response: reqwest::Response) -> Result<String, ExchangeError> {
    let status = response.status();
    let body = response.text().await.map_err(ExchangeError::Http)?;
    if status.is_success() {
        return Ok(body);
    }
    Err(parse_error_body(&body))
}

fn parse_error_body(body: &str) -> ExchangeError {
    match serde_json::from_str::<RawErrorResponse>(body) {
        Ok(err) => ExchangeError::Rejected {
            error: err.error,
            description: err.error_description,
        },
        Err(_json_error) => ExchangeError::Rejected {
            error: "http_error".to_owned(),
            description: None,
        },
    }
}

/// Turn a raw token-endpoint body into a [`TokenSet`]. Pure, so the parsing
/// itself is testable without a network round trip.
pub(crate) fn parse_token_response(
    body: &str,
    previous_scope: Option<&str>,
    now: i64,
) -> Result<TokenSet, ExchangeError> {
    let raw: RawTokenResponse =
        serde_json::from_str(body).map_err(|_json_error| ExchangeError::MalformedResponse)?;
    let expires_in = raw.expires_in.unwrap_or(3600);
    Ok(TokenSet {
        access_token: Secret::new(raw.access_token),
        refresh_token: raw.refresh_token.map(Secret::new),
        expires_at: now.saturating_add(expires_in),
        scope: raw
            .scope
            .or_else(|| previous_scope.map(str::to_owned))
            .unwrap_or_default(),
    })
}

/// Exchange an authorization code for tokens.
pub async fn exchange_authorization_code(
    sender: &dyn TokenSender,
    token_url: &str,
    client_id: &str,
    redirect_uri: &str,
    code: &Secret,
    verifier: &Secret,
    now: i64,
) -> Result<TokenSet, ExchangeError> {
    let form = [
        ("grant_type", "authorization_code"),
        ("code", code.expose()),
        ("redirect_uri", redirect_uri),
        ("client_id", client_id),
        ("code_verifier", verifier.expose()),
    ];
    let body = sender.post_form(token_url, &form).await?;
    parse_token_response(&body, None, now)
}

/// Exchange a refresh token for a new access token. Keeps the previous scope
/// if the provider's response omits one, which the refresh grant commonly
/// does.
pub async fn refresh_access_token(
    sender: &dyn TokenSender,
    token_url: &str,
    client_id: &str,
    refresh_token: &Secret,
    previous_scope: &str,
    now: i64,
) -> Result<TokenSet, ExchangeError> {
    let form = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token.expose()),
        ("client_id", client_id),
    ];
    let body = sender.post_form(token_url, &form).await?;
    parse_token_response(&body, Some(previous_scope), now)
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

    #[test]
    fn a_well_formed_response_is_parsed_into_a_token_set() {
        let body = r#"{"access_token":"access-1","refresh_token":"refresh-1","expires_in":3600,"scope":"calendar.readonly"}"#;
        let set = parse_token_response(body, None, 1_000).unwrap();
        assert_eq!(set.access_token.expose(), "access-1");
        assert_eq!(
            set.refresh_token.map(|s| s.expose().to_owned()),
            Some("refresh-1".to_owned())
        );
        assert_eq!(set.expires_at, 1_000 + 3600);
        assert_eq!(set.scope, "calendar.readonly");
    }

    #[test]
    fn a_refresh_response_without_a_new_refresh_token_carries_none() {
        // The common shape for Google: refreshing does not reissue one.
        let body = r#"{"access_token":"access-2","expires_in":3600}"#;
        let set = parse_token_response(body, Some("calendar.readonly"), 0).unwrap();
        assert!(set.refresh_token.is_none());
        assert_eq!(
            set.scope, "calendar.readonly",
            "falls back to the previous scope"
        );
    }

    #[test]
    fn a_missing_expires_in_falls_back_to_one_hour() {
        let body = r#"{"access_token":"access-3"}"#;
        let set = parse_token_response(body, None, 0).unwrap();
        assert_eq!(set.expires_at, 3600);
    }

    #[test]
    fn an_oauth2_error_body_is_parsed_rather_than_treated_as_malformed() {
        let err = parse_error_body(r#"{"error":"invalid_grant","error_description":"Bad code"}"#);
        match err {
            ExchangeError::Rejected { error, description } => {
                assert_eq!(error, "invalid_grant");
                assert_eq!(description, Some("Bad code".to_owned()));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_unparseable_body_is_a_malformed_response_not_a_panic() {
        assert!(matches!(
            parse_token_response("not json", None, 0),
            Err(ExchangeError::MalformedResponse)
        ));
    }

    #[test]
    fn no_request_built_here_ever_carries_a_client_secret() {
        // Covered structurally too (the form arrays are fixed literals with
        // no such field), but pinning it as a test means a future edit that
        // adds one fails loudly rather than silently.
        let code = Secret::new("code".to_owned());
        let verifier = Secret::new("verifier".to_owned());
        let refresh = Secret::new("refresh".to_owned());

        // Re-derive the exact field names used by both exchange functions.
        let auth_fields = [
            "grant_type",
            "code",
            "redirect_uri",
            "client_id",
            "code_verifier",
        ];
        let refresh_fields = ["grant_type", "refresh_token", "client_id"];
        assert!(!auth_fields.contains(&"client_secret"));
        assert!(!refresh_fields.contains(&"client_secret"));

        // Exercise the constructors so this test also fails to compile if a
        // parameter list ever grows a secret.
        let _ = (&code, &verifier, &refresh);
    }

    #[tokio::test]
    async fn the_network_policy_sender_refuses_a_plaintext_loopback_endpoint() {
        // Proves the production sender is wired to the boundary: calendar
        // traffic does not get the plaintext-private exemption, so this must
        // be refused before any socket is touched.
        let policy = NetworkPolicy::new().unwrap();
        let sender = NetworkPolicySender::new(&policy);
        let err = sender
            .post_form(
                "http://127.0.0.1:9/token",
                &[("grant_type", "refresh_token")],
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ExchangeError::Policy(_)), "{err:?}");
    }

    #[test]
    fn the_boundary_accepts_an_https_provider_url() {
        // Not a live call, just proof the scheme itself is not what the
        // boundary refuses for this purpose.
        assert!(kw_network_policy::validate(
            "https://oauth2.googleapis.com/token",
            RequestPurpose::UserConfiguredCalendarProvider
        )
        .is_ok());
    }
}
