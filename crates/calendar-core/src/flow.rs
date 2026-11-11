//! Orchestrating one calendar account through authorize -> exchange -> store
//! -> refresh.
//!
//! Four steps, each independently testable, glued together here:
//!
//! 1. [`begin_authorization`] builds the URL to open in the user's browser.
//! 2. The caller runs a [`crate::redirect::LoopbackReceiver`] and gets back a
//!    [`crate::redirect::CallbackParams`] -- this crate does not do that step
//!    itself, since the receiver's own errors (a timeout, a malformed
//!    request) are a different concern from authorization's.
//! 3. [`complete_authorization`] checks `state` in constant time and, only if
//!    it matches, exchanges the code for tokens.
//! 4. [`store_tokens`] puts the refresh token in the credential store and a
//!    `token_ref` in the database; [`ensure_fresh_access_token`] returns a
//!    usable access token later, refreshing it first if it has expired.

use kw_security::{CredentialError, CredentialStore, KeyRef, Secret};
use sqlx::SqlitePool;

use crate::credentials::{decode_bundle, encode_bundle, token_ref_for, BundleError, TokenBundle};
use crate::exchange::{
    exchange_authorization_code, refresh_access_token, ExchangeError, TokenSender, TokenSet,
};
use crate::pkce::{self, Pkce, PkceError};
use crate::provider::{CalendarProviderKind, OAuthClientConfig, ProviderEndpoints};
use crate::redirect::CallbackParams;
use crate::tokens_repo;

/// How much earlier than the recorded expiry an access token is treated as
/// stale, so a caller does not race an expiry that lands mid-request.
const EXPIRY_SKEW_SECONDS: i64 = 60;

/// Why authorization, storage, or refresh failed.
#[derive(thiserror::Error)]
pub enum CalendarError {
    #[error("could not generate pkce material: {0}")]
    Pkce(#[from] PkceError),

    #[error("could not build the authorize url: {0}")]
    InvalidProviderUrl(#[from] url::ParseError),

    #[error("the redirect did not carry an authorization code")]
    MissingCallbackParams,

    #[error("the provider would not complete authorization: {error}")]
    AuthorizationDenied {
        error: String,
        description: Option<String>,
    },

    #[error("the calendar connection belongs to a different OAuth client; reconnect required")]
    ReconnectRequired,

    #[error("the state parameter did not match; refusing to continue")]
    StateMismatch,

    #[error("token exchange failed: {0}")]
    Exchange(#[from] ExchangeError),

    #[error("the derived credential slot name was not a valid KeyRef")]
    InvalidSlot,

    #[error("credential store error: {0}")]
    Credential(#[from] CredentialError),

    #[error("the stored credential could not be read back: {0}")]
    Bundle(#[from] BundleError),

    #[error("database error: {0}")]
    Storage(#[from] sqlx::Error),

    #[error("the provider did not return a refresh token; cannot store this connection")]
    NoRefreshToken,

    #[error("no calendar token is on file for {account}")]
    NotConnected { account: String },
}

impl std::fmt::Debug for CalendarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}

/// An authorization in progress: the URL to open, and the secret material
/// needed to complete it once the redirect comes back.
pub struct PendingAuthorization {
    pub authorize_url: String,
    pub redirect_uri: String,
    state: String,
    verifier: Secret,
}

impl std::fmt::Debug for PendingAuthorization {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingAuthorization")
            .field("authorize_url", &self.authorize_url)
            .field("redirect_uri", &self.redirect_uri)
            .field("state", &self.state)
            .field("verifier", &self.verifier)
            .finish()
    }
}

/// Build the URL to open in the user's browser, and the PKCE/state material
/// needed to validate and complete the redirect that follows.
///
/// The caller resolves the app-owned or custom client. Its secret is never
/// included in this URL; only token endpoint grants may use it.
pub fn begin_authorization(
    provider: &ProviderEndpoints,
    client: &OAuthClientConfig,
    redirect_uri: &str,
) -> Result<PendingAuthorization, CalendarError> {
    let pkce = Pkce::generate()?;
    let state = pkce::generate_state()?;

    let mut url = url::Url::parse(&provider.authorize_url)?;
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("client_id", client.client_id())
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", &provider.scopes.join(" "))
            .append_pair("code_challenge", pkce.challenge())
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state);
        for (key, value) in &provider.extra_authorize_params {
            query.append_pair(key, value);
        }
    }

    Ok(PendingAuthorization {
        authorize_url: url.to_string(),
        redirect_uri: redirect_uri.to_owned(),
        state,
        verifier: pkce.verifier().clone(),
    })
}

/// Validate the redirect against `pending` and exchange the code for tokens.
///
/// The state check happens first, in constant time, before anything else --
/// a mismatch is refused without attempting the exchange at all.
pub async fn complete_authorization(
    pending: &PendingAuthorization,
    callback: &CallbackParams,
    provider: &ProviderEndpoints,
    client: &OAuthClientConfig,
    sender: &dyn TokenSender,
) -> Result<TokenSet, CalendarError> {
    let Some(state) = &callback.state else {
        return Err(CalendarError::MissingCallbackParams);
    };
    if !pkce::constant_time_eq(&pending.state, state) {
        return Err(CalendarError::StateMismatch);
    }

    if let Some(error) = &callback.error {
        return Err(CalendarError::AuthorizationDenied {
            error: error.clone(),
            description: callback.error_description.clone(),
        });
    }

    let Some(code) = &callback.code else {
        return Err(CalendarError::MissingCallbackParams);
    };

    let now = now_epoch();
    let tokens = exchange_authorization_code(
        sender,
        &provider.token_url,
        client,
        &pending.redirect_uri,
        &Secret::new(code.clone()),
        &pending.verifier,
        now,
    )
    .await?;
    Ok(tokens)
}

/// Store a freshly exchanged token set: the refresh token (bundled with the
/// access token) goes to the credential store, and only a `token_ref` plus
/// its expiry and scope go to the database.
pub async fn store_tokens(
    pool: &SqlitePool,
    creds: &dyn CredentialStore,
    kind: CalendarProviderKind,
    account_email: &str,
    tokens: &TokenSet,
    client_id: &str,
) -> Result<KeyRef, CalendarError> {
    let Some(refresh_token) = &tokens.refresh_token else {
        return Err(CalendarError::NoRefreshToken);
    };
    let slot = token_ref_for(kind, account_email).ok_or(CalendarError::InvalidSlot)?;

    let bundle = TokenBundle {
        access_token: tokens.access_token.expose().to_owned(),
        refresh_token: refresh_token.expose().to_owned(),
        client_id: Some(client_id.to_owned()),
    };
    let secret = encode_bundle(&bundle)?;
    creds.set(&slot, &secret)?;

    tokens_repo::upsert(
        pool,
        kind,
        account_email,
        slot.slot(),
        tokens.expires_at,
        &tokens.scope,
    )
    .await?;

    Ok(slot)
}

/// Return a usable access token for `account_email`, refreshing it first if
/// the cached one has expired (or is close enough to it).
///
/// Never makes a network call when the cached access token is still fresh:
/// the only thing persisted between calls is the credential-store bundle and
/// the database's `expires_at`, and both are checked before reaching for the
/// refresh grant. A missing or different issuing client requires reconnecting
/// before even a still-valid cached token may be returned.
pub async fn ensure_fresh_access_token(
    pool: &SqlitePool,
    creds: &dyn CredentialStore,
    sender: &dyn TokenSender,
    provider: &ProviderEndpoints,
    client: &OAuthClientConfig,
    kind: CalendarProviderKind,
    account_email: &str,
) -> Result<Secret, CalendarError> {
    let row = tokens_repo::read(pool, kind, account_email)
        .await?
        .ok_or_else(|| CalendarError::NotConnected {
            account: account_email.to_owned(),
        })?;
    let slot = KeyRef::new(&row.token_ref).ok_or(CalendarError::InvalidSlot)?;
    let secret = creds.get(&slot)?;
    let bundle = decode_bundle(&secret)?;
    if bundle.client_id.as_deref() != Some(client.client_id()) {
        return Err(CalendarError::ReconnectRequired);
    }

    let now = now_epoch();
    if row.expires_at > now + EXPIRY_SKEW_SECONDS {
        return Ok(Secret::new(bundle.access_token));
    }

    let refreshed = refresh_access_token(
        sender,
        &provider.token_url,
        client,
        &Secret::new(bundle.refresh_token.clone()),
        &row.scope,
        now,
    )
    .await?;

    let new_refresh_token = refreshed
        .refresh_token
        .as_ref()
        .map(|s| s.expose().to_owned())
        .unwrap_or(bundle.refresh_token);

    let new_bundle = TokenBundle {
        access_token: refreshed.access_token.expose().to_owned(),
        refresh_token: new_refresh_token,
        client_id: Some(client.client_id().to_owned()),
    };
    let new_secret = encode_bundle(&new_bundle)?;
    creds.set(&slot, &new_secret)?;

    tokens_repo::update_after_refresh(
        pool,
        kind,
        account_email,
        refreshed.expires_at,
        &refreshed.scope,
    )
    .await?;

    Ok(Secret::new(new_bundle.access_token))
}

fn now_epoch() -> i64 {
    chrono::Utc::now().timestamp()
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
    use async_trait::async_trait;
    use kw_security::MemoryCredentialStore;
    use kw_storage::Db;

    struct RejectingSender;

    #[async_trait]
    impl TokenSender for RejectingSender {
        async fn post_form(
            &self,
            _url: &str,
            _form: &[(&str, &str)],
        ) -> Result<String, ExchangeError> {
            panic!("the exchange must not be attempted for a refused redirect");
        }
    }

    fn client() -> OAuthClientConfig {
        OAuthClientConfig::Google {
            client_id: "client-123".to_owned(),
            client_secret: Some(Secret::new("private-client-value".to_owned())),
        }
    }

    fn provider() -> ProviderEndpoints {
        ProviderEndpoints {
            authorize_url: "https://example.invalid/authorize".to_owned(),
            token_url: "https://example.invalid/token".to_owned(),
            scopes: vec!["calendar.readonly".to_owned()],
            extra_authorize_params: Vec::new(),
        }
    }

    #[test]
    fn the_authorize_url_carries_pkce_and_no_client_secret() {
        let pending =
            begin_authorization(&provider(), &client(), "http://127.0.0.1:9/callback").unwrap();

        let url = url::Url::parse(&pending.authorize_url).unwrap();
        let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(pairs.get("client_id"), Some(&"client-123".to_owned()));
        assert_eq!(pairs.get("code_challenge_method"), Some(&"S256".to_owned()));
        assert!(pairs.contains_key("code_challenge"));
        assert!(pairs.contains_key("state"));
        assert!(!pairs.contains_key("client_secret"));
        assert!(!pending.authorize_url.contains("client_secret"));
        assert!(!pending.authorize_url.contains("private-client-value"));
    }

    #[test]
    fn pendings_debug_output_never_shows_the_verifier() {
        let pending =
            begin_authorization(&provider(), &client(), "http://127.0.0.1:9/callback").unwrap();
        let rendered = format!("{pending:?}");
        assert!(rendered.contains("redacted"));
    }

    #[tokio::test]
    async fn a_mismatched_state_is_refused_before_any_exchange_is_attempted() {
        let pending =
            begin_authorization(&provider(), &client(), "http://127.0.0.1:9/callback").unwrap();

        let callback = CallbackParams {
            code: Some("some-code".to_owned()),
            state: Some("definitely-not-the-real-state".to_owned()),
            error: None,
            error_description: None,
        };

        let err = complete_authorization(
            &pending,
            &callback,
            &provider(),
            &client(),
            &RejectingSender,
        )
        .await
        .unwrap_err();
        assert!(matches!(err, CalendarError::StateMismatch));
    }

    #[tokio::test]
    async fn a_denied_authorization_is_refused_before_any_exchange_is_attempted() {
        let pending =
            begin_authorization(&provider(), &client(), "http://127.0.0.1:9/callback").unwrap();

        let callback = CallbackParams {
            code: None,
            state: Some(pending.state.clone()),
            error: Some("access_denied".to_owned()),
            error_description: Some("private description".to_owned()),
        };

        let err = complete_authorization(
            &pending,
            &callback,
            &provider(),
            &client(),
            &RejectingSender,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, CalendarError::AuthorizationDenied { ref error, .. } if error == "access_denied")
        );
    }

    #[tokio::test]
    async fn storing_without_a_refresh_token_is_refused() {
        let db = Db::open_in_memory().await.unwrap();
        let creds = MemoryCredentialStore::new();
        let tokens = TokenSet {
            access_token: Secret::new("access-only".to_owned()),
            refresh_token: None,
            expires_at: 1_000,
            scope: "calendar.readonly".to_owned(),
        };

        let err = store_tokens(
            db.pool(),
            &creds,
            CalendarProviderKind::Google,
            "user@example.com",
            &tokens,
            "client-123",
        )
        .await
        .unwrap_err();
        assert!(matches!(err, CalendarError::NoRefreshToken));
    }

    #[tokio::test]
    async fn ensure_fresh_access_token_reports_a_never_connected_account_honestly() {
        let db = Db::open_in_memory().await.unwrap();
        let creds = MemoryCredentialStore::new();

        let err = ensure_fresh_access_token(
            db.pool(),
            &creds,
            &RejectingSender,
            &provider(),
            &client(),
            CalendarProviderKind::Google,
            "never-connected@example.com",
        )
        .await
        .unwrap_err();
        assert!(matches!(err, CalendarError::NotConnected { .. }));
    }

    #[tokio::test]
    async fn a_still_fresh_access_token_is_returned_without_a_refresh_call() {
        let db = Db::open_in_memory().await.unwrap();
        let creds = MemoryCredentialStore::new();
        let tokens = TokenSet {
            access_token: Secret::new("still-good".to_owned()),
            refresh_token: Some(Secret::new("refresh-1".to_owned())),
            expires_at: now_epoch() + 3_600,
            scope: "calendar.readonly".to_owned(),
        };
        store_tokens(
            db.pool(),
            &creds,
            CalendarProviderKind::Google,
            "user@example.com",
            &tokens,
            "client-123",
        )
        .await
        .unwrap();

        // RejectingSender panics if the exchange is ever attempted, so a
        // clean return here proves no refresh call was made.
        let access = ensure_fresh_access_token(
            db.pool(),
            &creds,
            &RejectingSender,
            &provider(),
            &client(),
            CalendarProviderKind::Google,
            "user@example.com",
        )
        .await
        .unwrap();
        assert_eq!(access.expose(), "still-good");
    }

    #[tokio::test]
    async fn callback_state_precedes_provider_errors_and_missing_code() {
        let pending =
            begin_authorization(&provider(), &client(), "http://127.0.0.1/callback").unwrap();
        for state in [None, Some("wrong".to_owned()), Some(pending.state.clone())] {
            let callback = CallbackParams {
                code: None,
                state: state.clone(),
                error: Some("access_denied".into()),
                error_description: Some("private-description".into()),
            };
            let error = complete_authorization(
                &pending,
                &callback,
                &provider(),
                &client(),
                &RejectingSender,
            )
            .await
            .unwrap_err();
            assert!(!format!("{callback:?} {error:?} {error}").contains("private-description"));
            match state {
                None => assert!(matches!(error, CalendarError::MissingCallbackParams)),
                Some(state) if state != pending.state => {
                    assert!(matches!(error, CalendarError::StateMismatch))
                }
                Some(_) => assert!(
                    matches!(error, CalendarError::AuthorizationDenied { error, description } if error == "access_denied" && description.as_deref() == Some("private-description"))
                ),
            }
        }
        let callback = CallbackParams {
            code: None,
            state: Some(pending.state.clone()),
            error: None,
            error_description: None,
        };
        assert!(matches!(
            complete_authorization(
                &pending,
                &callback,
                &provider(),
                &client(),
                &RejectingSender
            )
            .await,
            Err(CalendarError::MissingCallbackParams)
        ));
    }

    #[tokio::test]
    async fn unbound_or_other_client_grants_require_reconnect_even_while_fresh() {
        for binding in [None, Some("other-client".to_owned())] {
            for expires_at in [1, now_epoch() + 3600] {
                let db = Db::open_in_memory().await.unwrap();
                let creds = MemoryCredentialStore::new();
                let slot = token_ref_for(CalendarProviderKind::Google, "user@example.com").unwrap();
                creds
                    .set(
                        &slot,
                        &encode_bundle(&TokenBundle {
                            access_token: "access".into(),
                            refresh_token: "refresh".into(),
                            client_id: binding.clone(),
                        })
                        .unwrap(),
                    )
                    .unwrap();
                tokens_repo::upsert(
                    db.pool(),
                    CalendarProviderKind::Google,
                    "user@example.com",
                    slot.slot(),
                    expires_at,
                    "scope",
                )
                .await
                .unwrap();
                let error = ensure_fresh_access_token(
                    db.pool(),
                    &creds,
                    &RejectingSender,
                    &provider(),
                    &client(),
                    CalendarProviderKind::Google,
                    "user@example.com",
                )
                .await
                .unwrap_err();
                assert!(matches!(error, CalendarError::ReconnectRequired));
            }
        }
    }

    #[test]
    fn provider_authorize_parameters_are_scoped_and_never_contain_the_secret() {
        for endpoints in [ProviderEndpoints::google(), ProviderEndpoints::microsoft()] {
            let pending =
                begin_authorization(&endpoints, &client(), "http://127.0.0.1/callback").unwrap();
            let url = url::Url::parse(&pending.authorize_url).unwrap();
            let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
            assert!(!pending.authorize_url.contains("private-client-value"));
            assert!(!params.contains_key("client_secret"));
            if endpoints == ProviderEndpoints::google() {
                assert_eq!(params["scope"], "https://www.googleapis.com/auth/calendar.calendarlist.readonly https://www.googleapis.com/auth/calendar.events.readonly");
                assert_eq!(params["access_type"], "offline");
                assert_eq!(params["prompt"], "consent");
            } else {
                assert_eq!(params["prompt"], "select_account");
                assert_eq!(
                    params["scope"],
                    "offline_access https://graph.microsoft.com/Calendars.Read"
                );
            }
        }
    }
}
