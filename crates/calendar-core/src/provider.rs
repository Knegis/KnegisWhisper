//! Which provider, which tables, and which endpoints.
//!
//! Endpoints are configuration, not constants baked into the flow.
//! [`ProviderEndpoints::google`] and [`ProviderEndpoints::microsoft`] are
//! defaults, not the only values this type can hold: a test builds one that
//! points at a stub server on loopback instead, which is the only way this
//! flow can be exercised end to end without a real client id.

use kw_security::Secret;

/// A provider-typed desktop OAuth client. Google's desktop secret is a public
/// build value, used only by token grants; Microsoft is always a public client.
#[derive(Clone)]
pub enum OAuthClientConfig {
    Google {
        client_id: String,
        client_secret: Option<Secret>,
    },
    Microsoft {
        client_id: String,
    },
}

impl OAuthClientConfig {
    #[must_use]
    pub fn client_id(&self) -> &str {
        match self {
            Self::Google { client_id, .. } | Self::Microsoft { client_id } => client_id,
        }
    }

    #[must_use]
    pub const fn kind(&self) -> CalendarProviderKind {
        match self {
            Self::Google { .. } => CalendarProviderKind::Google,
            Self::Microsoft { .. } => CalendarProviderKind::Microsoft,
        }
    }

    #[must_use]
    pub const fn client_secret(&self) -> Option<&Secret> {
        match self {
            Self::Google { client_secret, .. } => client_secret.as_ref(),
            Self::Microsoft { .. } => None,
        }
    }
}

impl std::fmt::Debug for OAuthClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthClientConfig")
            .field("kind", &self.kind())
            .field("client_id", &self.client_id())
            .field("client_secret", &self.client_secret().map(|_| "<redacted>"))
            .finish()
    }
}

/// Which calendar provider an account belongs to.
///
/// Deliberately just Google and Microsoft. Apple/EventKit is not an OAuth
/// flow at all -- it is local platform access -- and belongs to the crate
/// that does event sync, not this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CalendarProviderKind {
    Google,
    Microsoft,
}

impl CalendarProviderKind {
    /// The prefix used when deriving a credential-store slot for an account
    /// of this provider. See `crate::credentials::token_ref_for`.
    #[must_use]
    pub const fn slot_prefix(self) -> &'static str {
        match self {
            Self::Google => "calendar-google",
            Self::Microsoft => "calendar-microsoft",
        }
    }

    /// The table in `crates/storage/migrations/0001_initial.sql` that carries
    /// this provider's `token_ref`.
    #[must_use]
    pub const fn token_table(self) -> &'static str {
        match self {
            Self::Google => "google_calendar_tokens",
            Self::Microsoft => "microsoft_calendar_tokens",
        }
    }

    /// The column in that table holding the account's address.
    #[must_use]
    pub const fn email_column(self) -> &'static str {
        match self {
            Self::Google => "google_email",
            Self::Microsoft => "microsoft_email",
        }
    }
}

/// Where a provider's authorize and token endpoints live, and what to ask
/// for. Every field is public: a test builds one from scratch, pointed at a
/// stub server, rather than being forced through [`ProviderEndpoints::google`]
/// or [`ProviderEndpoints::microsoft`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderEndpoints {
    pub authorize_url: String,
    pub token_url: String,
    pub scopes: Vec<String>,
    /// Provider-specific authorize query parameters beyond the standard
    /// OAuth2/PKCE set. Google needs `access_type=offline` and
    /// `prompt=consent` to hand back a refresh token on every grant;
    /// Microsoft returns one when `offline_access` is scoped, and uses
    /// `prompt=select_account` so SSO cannot silently re-link an account.
    pub extra_authorize_params: Vec<(String, String)>,
}

impl ProviderEndpoints {
    /// Google's real endpoints, with calendar read-only scope.
    #[must_use]
    pub fn google() -> Self {
        Self {
            authorize_url: "https://accounts.google.com/o/oauth2/v2/auth".to_owned(),
            token_url: "https://oauth2.googleapis.com/token".to_owned(),
            scopes: vec![
                "https://www.googleapis.com/auth/calendar.calendarlist.readonly".to_owned(),
                "https://www.googleapis.com/auth/calendar.events.readonly".to_owned(),
            ],
            extra_authorize_params: vec![
                ("access_type".to_owned(), "offline".to_owned()),
                ("prompt".to_owned(), "consent".to_owned()),
            ],
        }
    }

    /// Microsoft Graph's real endpoints (`common` tenant), with calendar
    /// read-only scope and `offline_access` so a refresh token is issued.
    #[must_use]
    pub fn microsoft() -> Self {
        Self {
            authorize_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize"
                .to_owned(),
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token".to_owned(),
            scopes: vec![
                "offline_access".to_owned(),
                "https://graph.microsoft.com/Calendars.Read".to_owned(),
            ],
            extra_authorize_params: vec![("prompt".to_owned(), "select_account".to_owned())],
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

    #[test]
    fn each_kind_names_the_table_the_schema_already_declares() {
        assert_eq!(
            CalendarProviderKind::Google.token_table(),
            "google_calendar_tokens"
        );
        assert_eq!(
            CalendarProviderKind::Microsoft.token_table(),
            "microsoft_calendar_tokens"
        );
    }

    #[test]
    fn slot_prefixes_are_legal_keyref_material() {
        for kind in [
            CalendarProviderKind::Google,
            CalendarProviderKind::Microsoft,
        ] {
            let prefix = kind.slot_prefix();
            assert!(kw_security::KeyRef::new(prefix).is_some(), "{prefix}");
        }
    }

    #[test]
    fn google_and_microsoft_defaults_use_https() {
        assert!(ProviderEndpoints::google()
            .authorize_url
            .starts_with("https://"));
        assert!(ProviderEndpoints::google()
            .token_url
            .starts_with("https://"));
        assert!(ProviderEndpoints::microsoft()
            .authorize_url
            .starts_with("https://"));
        assert!(ProviderEndpoints::microsoft()
            .token_url
            .starts_with("https://"));
    }

    #[test]
    fn a_test_can_build_endpoints_that_point_anywhere() {
        // The point of every field being public: nothing here is hardcoded
        // into the flow.
        let stub = ProviderEndpoints {
            authorize_url: "http://127.0.0.1:9/authorize".to_owned(),
            token_url: "http://127.0.0.1:9/token".to_owned(),
            scopes: vec!["test.scope".to_owned()],
            extra_authorize_params: Vec::new(),
        };
        assert_eq!(stub.scopes, vec!["test.scope".to_owned()]);
    }
}
