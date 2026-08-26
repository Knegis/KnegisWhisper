//! Mapping a calendar account to a credential-store slot, and the bundle
//! format stored under it.
//!
//! The refresh token is what the brief means by "credential storage for
//! calendars", but the access token travels with it in the same secret: the
//! `expires_at` column in `google_calendar_tokens` / `microsoft_calendar_tokens`
//! tracks the access token's expiry, and there is nowhere else in this
//! design to keep the access token itself between calls. One
//! [`kw_security::CredentialStore`] entry, holding both, means
//! `crate::flow::ensure_fresh_access_token` can return the cached access
//! token without a network call at all while it is still valid, and only
//! reaches for the refresh grant once it has expired.

use std::fmt;

use kw_security::{KeyRef, Secret};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::provider::CalendarProviderKind;

/// Derive the credential-store slot for one calendar account.
///
/// An email address is not a legal [`KeyRef`] -- `@` and `.` fall outside
/// `[a-z0-9-]` -- and slugifying it lossily risks two different addresses
/// landing on the same slot (`a.b@x` and `a@b.x` would both become
/// `a-b-x`). Hashing avoids both problems: the result is deterministic, so
/// the same account always resolves to the same slot (this is what lets
/// `ensure_fresh_access_token` find, on a later call, exactly what
/// `store_tokens` wrote), and collision-free for any realistic input.
#[must_use]
pub fn token_ref_for(kind: CalendarProviderKind, account_email: &str) -> Option<KeyRef> {
    let normalized = account_email.trim().to_ascii_lowercase();
    let digest = Sha256::digest(normalized.as_bytes());
    KeyRef::new(&format!("{}-{}", kind.slot_prefix(), hex_lower(&digest)))
}

fn hex_lower(bytes: &[u8]) -> String {
    use fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// Why a stored bundle could not be produced or read back.
#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("could not encode the credential bundle: {0}")]
    Encode(#[source] serde_json::Error),

    #[error("could not decode the stored credential bundle: {0}")]
    Decode(#[source] serde_json::Error),
}

/// The access and refresh token, together, exactly as stored under one
/// [`KeyRef`]. Never printed with real values -- see the manual `Debug` impl.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct TokenBundle {
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
}

impl fmt::Debug for TokenBundle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenBundle")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .finish()
    }
}

pub(crate) fn encode_bundle(bundle: &TokenBundle) -> Result<Secret, BundleError> {
    serde_json::to_string(bundle)
        .map(Secret::new)
        .map_err(BundleError::Encode)
}

pub(crate) fn decode_bundle(secret: &Secret) -> Result<TokenBundle, BundleError> {
    serde_json::from_str(secret.expose()).map_err(BundleError::Decode)
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
    fn the_same_account_always_derives_the_same_slot() {
        let a = token_ref_for(CalendarProviderKind::Google, "user@example.com").unwrap();
        let b = token_ref_for(CalendarProviderKind::Google, "user@example.com").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn case_and_surrounding_whitespace_do_not_change_the_slot() {
        let a = token_ref_for(CalendarProviderKind::Google, "User@Example.com").unwrap();
        let b = token_ref_for(CalendarProviderKind::Google, "  user@example.com  ").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn different_accounts_derive_different_slots() {
        let a = token_ref_for(CalendarProviderKind::Google, "alice@example.com").unwrap();
        let b = token_ref_for(CalendarProviderKind::Google, "bob@example.com").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn the_same_account_on_different_providers_derives_different_slots() {
        let google = token_ref_for(CalendarProviderKind::Google, "user@example.com").unwrap();
        let microsoft = token_ref_for(CalendarProviderKind::Microsoft, "user@example.com").unwrap();
        assert_ne!(google, microsoft);
    }

    #[test]
    fn every_derived_slot_is_a_legal_keyref() {
        // Belt and braces: token_ref_for returns Option specifically because
        // KeyRef::new can refuse, so prove the derivation never actually does.
        for kind in [
            CalendarProviderKind::Google,
            CalendarProviderKind::Microsoft,
        ] {
            for email in ["a@b.com", "A.Long.Name+tag@sub.example.co.uk", "x@y"] {
                assert!(token_ref_for(kind, email).is_some(), "{kind:?} {email}");
            }
        }
    }

    #[test]
    fn a_bundle_round_trips_through_encode_and_decode() {
        let bundle = TokenBundle {
            access_token: "access-xyz".to_owned(),
            refresh_token: "refresh-xyz".to_owned(),
        };
        let secret = encode_bundle(&bundle).unwrap();
        let decoded = decode_bundle(&secret).unwrap();
        assert_eq!(decoded.access_token, "access-xyz");
        assert_eq!(decoded.refresh_token, "refresh-xyz");
    }

    #[test]
    fn a_bundle_never_shows_its_tokens_in_debug_output() {
        let bundle = TokenBundle {
            access_token: "access-xyz".to_owned(),
            refresh_token: "refresh-xyz".to_owned(),
        };
        let rendered = format!("{bundle:?}");
        assert!(!rendered.contains("access-xyz"));
        assert!(!rendered.contains("refresh-xyz"));
        assert!(rendered.contains("redacted"));
    }

    #[test]
    fn decoding_garbage_is_an_error_not_a_panic() {
        let secret = Secret::new("not json".to_owned());
        assert!(decode_bundle(&secret).is_err());
    }
}
