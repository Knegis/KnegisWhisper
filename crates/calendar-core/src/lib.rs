//! Loopback PKCE OAuth for Google Calendar and Microsoft Graph.
//!
//! Upstream ships a Google OAuth client secret in the built application.
//! In an open-source build that secret is not a secret at all -- anyone can
//! extract it and impersonate the application -- so this crate never embeds
//! one, and never embeds a client id of its own either. Instead it implements
//! the loopback flow for a public client (RFC 8252, RFC 7636): the user
//! registers their own OAuth client in Google Cloud or Azure and supplies the
//! client id, this crate proves the token exchange comes from the process
//! that started it using a PKCE verifier instead of a secret, and a one-shot
//! listener on `127.0.0.1` receives the redirect because a client with no
//! server component has nowhere else to receive one. That is a deliberate
//! trade of setup friction for the absence of an embedded credential.
//!
//! Everything else follows from that one decision:
//!
//! * [`redirect::LoopbackReceiver`] is a bare `tokio::net::TcpListener`, not a
//!   router -- one connection, one static page, closed. Loopback only, never
//!   `0.0.0.0`.
//! * [`pkce`] generates the verifier and challenge from `getrandom`, S256
//!   only, and compares the `state` parameter in constant time.
//! * [`exchange::NetworkPolicySender`] is the only way production code may
//!   reach a token endpoint: every request goes through
//!   `kw_network_policy::NetworkPolicy` under
//!   `RequestPurpose::UserConfiguredCalendarProvider`.
//! * [`flow::store_tokens`] puts the refresh token in the OS credential store
//!   under a `token_ref`, and writes only that slot name -- never a token --
//!   into `google_calendar_tokens` / `microsoft_calendar_tokens`
//!   (`crates/storage/migrations/0001_initial.sql`, which this crate adds no
//!   migration to). `kw_storage::assert_no_credential_columns` is the
//!   backstop that would catch a regression, not the reason this is safe.
//!
//! Task 8 builds calendar event sync, the Tauri commands, and the settings
//! screen on top of this. This crate stops at: build the authorize URL, run
//! the loopback receiver, exchange the code, store the refresh token, and
//! mint a fresh access token on demand. There is no Apple/EventKit support
//! here -- that is local platform access, not an OAuth flow, and belongs to
//! whatever does event sync.

pub mod credentials;
pub mod exchange;
pub mod flow;
pub mod pkce;
pub mod provider;
pub mod redirect;
pub mod tokens_repo;

pub use crate::exchange::{ExchangeError, NetworkPolicySender, TokenSender, TokenSet};
pub use crate::flow::{
    begin_authorization, complete_authorization, ensure_fresh_access_token, store_tokens,
    CalendarError, PendingAuthorization,
};
pub use crate::pkce::{constant_time_eq, generate_state, Pkce, PkceError};
pub use crate::provider::{CalendarProviderKind, ProviderEndpoints};
pub use crate::redirect::{CallbackParams, LoopbackReceiver, RedirectError};
pub use crate::tokens_repo::TokenRow;
