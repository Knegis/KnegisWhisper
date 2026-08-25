//! The bearer token that guards the local API.
//!
//! This is the credential that stands between anything running as this user and
//! the notes, transcripts, and dictionary in the database. It is not a
//! convenience: on a shared or multi-user machine, and against any other process
//! that can reach loopback, it is the whole access control.
//!
//! Three properties, each with a test:
//!
//! - **Generated from the operating system's randomness**, 32 bytes, per run.
//!   Never derived from a machine identifier, a path, or a clock — those are
//!   guessable, and the brief forbids device fingerprinting besides.
//! - **Compared in constant time.** A byte-by-byte comparison that returns early
//!   leaks the length of the shared prefix, and a local attacker can measure it
//!   as many times as they like.
//! - **Never printed.** `Debug` shows the length and nothing else, so a token
//!   cannot reach a log through a struct that happens to derive `Debug`.

use std::fmt;

use subtle::ConstantTimeEq;

/// How many random bytes a token carries. 256 bits, matching upstream's
/// `randomBytes(32)`.
pub const TOKEN_BYTES: usize = 32;

/// Length of the hex form.
pub const TOKEN_HEX_LEN: usize = TOKEN_BYTES * 2;

/// A bearer token for the local API.
#[derive(Clone, PartialEq, Eq)]
pub struct LocalApiToken {
    hex: String,
}

impl LocalApiToken {
    /// Generate a fresh token from the operating system's randomness.
    ///
    /// A new one every run, so a token recovered from a stale discovery file is
    /// useless rather than a way in.
    pub fn generate() -> Result<Self, TokenError> {
        let mut bytes = [0_u8; TOKEN_BYTES];
        getrandom::getrandom(&mut bytes).map_err(|source| TokenError::Randomness {
            detail: source.to_string(),
        })?;
        Ok(Self { hex: hex(&bytes) })
    }

    /// Accept a token read back from a discovery file.
    ///
    /// Validated on the way in, so a truncated or hand-edited file is refused
    /// rather than becoming a short token that is easier to guess.
    pub fn parse(value: &str) -> Result<Self, TokenError> {
        if value.len() != TOKEN_HEX_LEN {
            return Err(TokenError::Length {
                found: value.len(),
                expected: TOKEN_HEX_LEN,
            });
        }
        if !value.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(TokenError::NotHex);
        }
        Ok(Self {
            hex: value.to_ascii_lowercase(),
        })
    }

    /// The token, for writing to the discovery file or sending as a header.
    ///
    /// Named `expose` for the same reason [`crate::Secret::expose`] is: every
    /// call site that puts this somewhere is easy to find.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.hex
    }

    /// The full `Authorization` header value a client should send.
    #[must_use]
    pub fn header_value(&self) -> String {
        format!("Bearer {}", self.hex)
    }

    /// Whether an `Authorization` header matches, compared in constant time.
    ///
    /// Takes the whole header rather than an extracted token so there is no
    /// place to get the parsing wrong and compare the wrong thing. A missing
    /// prefix, wrong case in the scheme, or wrong length all fail — but they
    /// fail *after* a constant-time comparison against a padded buffer, so the
    /// time taken does not reveal which.
    #[must_use]
    pub fn matches_header(&self, header: &str) -> bool {
        let expected = self.header_value();

        // Compare against a buffer of the expected length either way. Returning
        // early on a length mismatch would turn "how long is the token" into a
        // timing question, and a local attacker can ask it repeatedly.
        let mut candidate = vec![0_u8; expected.len()];
        let provided = header.as_bytes();
        let copied = provided.len().min(candidate.len());
        if let Some(slice) = candidate.get_mut(..copied) {
            if let Some(source) = provided.get(..copied) {
                slice.copy_from_slice(source);
            }
        }

        let same_length = provided.len() == expected.len();
        let same_bytes = candidate.ct_eq(expected.as_bytes()).unwrap_u8() == 1;

        // Non-short-circuiting `&`, so the byte comparison always runs and the
        // length check cannot skip it.
        same_length & same_bytes
    }
}

impl fmt::Debug for LocalApiToken {
    /// Never the value.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LocalApiToken({} hex chars, redacted)", self.hex.len())
    }
}

/// Why a token could not be made or read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TokenError {
    #[error(
        "the operating system would not provide randomness: {detail}. The local API cannot be \
         started without it."
    )]
    Randomness { detail: String },

    #[error("a local API token is {expected} hex characters; found {found}")]
    Length { found: usize, expected: usize },

    #[error("a local API token must be hex")]
    NotHex,
}

fn hex(bytes: &[u8]) -> String {
    use fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
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

    #[test]
    fn a_generated_token_is_256_bits_of_hex() {
        let token = LocalApiToken::generate().unwrap();
        assert_eq!(token.expose().len(), TOKEN_HEX_LEN);
        assert!(token.expose().chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn two_tokens_differ() {
        // Not a randomness test — a guard against a constant, a counter, or
        // anything derived from the machine sneaking in here.
        let a = LocalApiToken::generate().unwrap();
        let b = LocalApiToken::generate().unwrap();
        assert_ne!(a.expose(), b.expose());
    }

    #[test]
    fn a_token_never_appears_in_its_debug_output() {
        // A token reaches a log through any struct that derives Debug and holds
        // one, so this is the only place that can prevent it.
        let token = LocalApiToken::generate().unwrap();
        let rendered = format!("{token:?}");
        assert!(!rendered.contains(token.expose()), "{rendered}");
        assert!(rendered.contains("redacted"));

        // And through a struct that contains it.
        #[derive(Debug)]
        struct Config {
            token: LocalApiToken,
        }
        let config = Config {
            token: token.clone(),
        };
        assert!(!format!("{config:?}").contains(config.token.expose()));
    }

    #[test]
    fn the_right_header_matches() {
        let token = LocalApiToken::generate().unwrap();
        assert!(token.matches_header(&token.header_value()));
        assert!(token.matches_header(&format!("Bearer {}", token.expose())));
    }

    #[test]
    fn a_wrong_token_of_the_right_length_does_not_match() {
        let token = LocalApiToken::generate().unwrap();
        let other = LocalApiToken::generate().unwrap();
        assert!(!token.matches_header(&other.header_value()));
    }

    #[test]
    fn a_correct_prefix_is_not_enough() {
        // The case a length check plus an early-returning comparison would let
        // an attacker walk one byte at a time.
        let token = LocalApiToken::generate().unwrap();
        let mut truncated = token.header_value();
        truncated.pop();
        truncated.push('0');
        // One-in-sixteen chance the last character was already '0'.
        if truncated != token.header_value() {
            assert!(!token.matches_header(&truncated));
        }
    }

    #[test]
    fn an_empty_or_missing_header_does_not_match() {
        let token = LocalApiToken::generate().unwrap();
        for header in ["", "Bearer", "Bearer ", "Basic abc", token.expose()] {
            assert!(!token.matches_header(header), "{header:?} must not match");
        }
    }

    #[test]
    fn the_scheme_is_case_sensitive_and_exact() {
        // Not because the HTTP spec demands it, but because accepting variants
        // means more than one string is a valid credential.
        let token = LocalApiToken::generate().unwrap();
        assert!(!token.matches_header(&format!("bearer {}", token.expose())));
        assert!(!token.matches_header(&format!("BEARER {}", token.expose())));
        assert!(!token.matches_header(&format!("Bearer  {}", token.expose())));
    }

    #[test]
    fn a_much_longer_header_does_not_match_and_does_not_panic() {
        // The padded-buffer comparison has to handle a header longer than the
        // expected value without indexing past the end.
        let token = LocalApiToken::generate().unwrap();
        let long = format!("Bearer {}", "a".repeat(10_000));
        assert!(!token.matches_header(&long));
    }

    #[test]
    fn a_token_round_trips_through_the_discovery_file_form() {
        let token = LocalApiToken::generate().unwrap();
        let parsed = LocalApiToken::parse(token.expose()).unwrap();
        assert!(parsed.matches_header(&token.header_value()));
    }

    #[test]
    fn a_truncated_or_edited_token_is_refused_on_the_way_in() {
        // Otherwise a hand-edited discovery file becomes a short token, which is
        // easier to guess than a missing one.
        let token = LocalApiToken::generate().unwrap();
        assert!(matches!(
            LocalApiToken::parse(&token.expose()[..40]),
            Err(TokenError::Length { .. })
        ));
        assert!(matches!(
            LocalApiToken::parse(&"z".repeat(TOKEN_HEX_LEN)),
            Err(TokenError::NotHex)
        ));
        assert!(LocalApiToken::parse("").is_err());
    }

    #[test]
    fn an_uppercase_token_from_a_file_still_works() {
        // Normalised on the way in, so there is exactly one spelling to compare.
        let token = LocalApiToken::generate().unwrap();
        let parsed = LocalApiToken::parse(&token.expose().to_uppercase()).unwrap();
        assert_eq!(parsed.expose(), token.expose());
    }
}
