//! PKCE (RFC 7636) verifier/challenge generation, the CSRF `state` parameter,
//! and the constant-time comparison the redirect handler uses on it.
//!
//! S256 only. `plain` exists in the RFC for a client that cannot compute
//! SHA-256, which is not a constraint this application has, and shipping it
//! anyway would just be a second way for the verifier to end up sent in the
//! clear.

use std::fmt;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use kw_security::Secret;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Bytes of randomness behind the verifier. 32 bytes encodes to 43 base64url
/// characters, at RFC 7636's minimum length.
const VERIFIER_BYTES: usize = 32;

/// Bytes of randomness behind the `state` parameter.
const STATE_BYTES: usize = 32;

/// Why PKCE material could not be generated.
#[derive(Debug, thiserror::Error)]
pub enum PkceError {
    #[error(
        "the operating system would not provide randomness: {detail}. Authorization cannot \
         start without it."
    )]
    Randomness { detail: String },
}

/// A freshly generated PKCE verifier and its S256 challenge.
///
/// The verifier is the secret half: it never leaves this process except
/// inside the token-exchange request body, sent directly to the token
/// endpoint over the request the network boundary validates. The challenge is
/// the public half, sent in the authorize URL, and safe to log or print.
pub struct Pkce {
    verifier: Secret,
    challenge: String,
}

impl Pkce {
    /// Generate a new verifier from the operating system's randomness, and
    /// derive its S256 challenge.
    pub fn generate() -> Result<Self, PkceError> {
        let verifier = random_urlsafe(VERIFIER_BYTES)?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Ok(Self {
            verifier: Secret::new(verifier),
            challenge,
        })
    }

    #[must_use]
    pub fn verifier(&self) -> &Secret {
        &self.verifier
    }

    #[must_use]
    pub fn challenge(&self) -> &str {
        &self.challenge
    }
}

impl fmt::Debug for Pkce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pkce")
            .field("verifier", &self.verifier)
            .field("challenge", &self.challenge)
            .finish()
    }
}

/// Generate the CSRF `state` parameter: high-entropy and unrelated to the
/// verifier, so leaking one never helps guess the other.
pub fn generate_state() -> Result<String, PkceError> {
    random_urlsafe(STATE_BYTES)
}

fn random_urlsafe(len: usize) -> Result<String, PkceError> {
    let mut bytes = vec![0_u8; len];
    getrandom::getrandom(&mut bytes).map_err(|source| PkceError::Randomness {
        detail: source.to_string(),
    })?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Compare two `state` values in constant time.
///
/// A byte-by-byte comparison that returns early on the first mismatch, or on a
/// length mismatch, leaks how much of the expected value a guess got right --
/// and the redirect receiver is reachable by anything on the same machine that
/// can win the race to the loopback port. Both the length check and the byte
/// comparison always run, combined with a non-short-circuiting `&`.
#[must_use]
pub fn constant_time_eq(expected: &str, provided: &str) -> bool {
    let expected = expected.as_bytes();
    let provided = provided.as_bytes();

    let mut candidate = vec![0_u8; expected.len()];
    let copied = provided.len().min(candidate.len());
    if let Some(slice) = candidate.get_mut(..copied) {
        if let Some(source) = provided.get(..copied) {
            slice.copy_from_slice(source);
        }
    }

    let same_length = provided.len() == expected.len();
    let same_bytes = candidate.ct_eq(expected).unwrap_u8() == 1;

    same_length & same_bytes
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
    fn a_generated_verifier_meets_rfc_7636s_minimum_length() {
        let pkce = Pkce::generate().unwrap();
        assert!(pkce.verifier().expose().len() >= 43);
        assert!(pkce.verifier().expose().len() <= 128);
    }

    #[test]
    fn the_challenge_is_the_sha256_of_the_verifier_not_the_verifier_itself() {
        let pkce = Pkce::generate().unwrap();
        assert_ne!(pkce.challenge(), pkce.verifier().expose());

        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(pkce.verifier().expose().as_bytes()));
        assert_eq!(pkce.challenge(), expected);
    }

    #[test]
    fn two_generated_pairs_differ() {
        let a = Pkce::generate().unwrap();
        let b = Pkce::generate().unwrap();
        assert_ne!(a.verifier().expose(), b.verifier().expose());
        assert_ne!(a.challenge(), b.challenge());
    }

    #[test]
    fn the_verifier_never_appears_in_debug_output() {
        let pkce = Pkce::generate().unwrap();
        let rendered = format!("{pkce:?}");
        assert!(!rendered.contains(pkce.verifier().expose()));
        assert!(rendered.contains("redacted"));
        // The challenge is public and fine to show.
        assert!(rendered.contains(pkce.challenge()));
    }

    #[test]
    fn state_values_are_url_safe_and_high_entropy() {
        let a = generate_state().unwrap();
        let b = generate_state().unwrap();
        assert_ne!(a, b);
        assert!(a.len() >= 32);
        assert!(a
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn matching_state_compares_equal() {
        let state = generate_state().unwrap();
        assert!(constant_time_eq(&state, &state));
    }

    #[test]
    fn a_wrong_state_of_the_same_length_is_refused() {
        let a = generate_state().unwrap();
        let b = generate_state().unwrap();
        assert!(!constant_time_eq(&a, &b));
    }

    #[test]
    fn a_wrong_length_is_refused_without_panicking() {
        let state = generate_state().unwrap();
        assert!(!constant_time_eq(&state, ""));
        assert!(!constant_time_eq(&state, &format!("{state}extra")));
        assert!(!constant_time_eq(&state, &state[..state.len() - 1]));
        assert!(!constant_time_eq("", &state));
    }

    #[test]
    fn empty_matches_empty() {
        assert!(constant_time_eq("", ""));
    }
}
