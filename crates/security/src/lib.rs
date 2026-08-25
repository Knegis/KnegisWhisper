//! Credential storage and secret hygiene.
//!
//! Secrets live in the operating system's credential store — macOS Keychain,
//! Windows Credential Manager, Linux Secret Service — one entry per key. Never
//! in SQLite, never in `localStorage`, never in a `.env` file.
//!
//! The central idea is the [`KeyRef`]: route resolution and provider dispatch
//! deal only in *references* to credentials, and only this crate can turn a
//! reference into a secret. Two things follow. A test never needs a real
//! credential, and a secret cannot reach a log line by accident, because the
//! types that travel through the rest of the app do not contain one.
//!
//! See `docs/SECURITY.md`.

use std::fmt;

/// Service name under which every credential is filed.
pub const SERVICE: &str = "com.knegis.knegiswhisper";

/// A reference to a credential, safe to put in a route, a log line, or an error.
///
/// Deliberately not `Deserialize`: a `KeyRef` should be produced by the provider
/// registry from a known provider id, not conjured from frontend input. That
/// keeps the set of reachable credential slots bounded by the registry.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
pub struct KeyRef(String);

impl KeyRef {
    /// Create a reference to the credential slot for `slot`.
    ///
    /// Returns `None` for a slot name that is empty or contains characters
    /// outside `[a-z0-9-]`, so a slot name can never be used to escape the
    /// credential namespace.
    #[must_use]
    pub fn new(slot: &str) -> Option<Self> {
        let ok = !slot.is_empty()
            && slot.len() <= 128
            && slot
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        ok.then(|| Self(slot.to_owned()))
    }

    #[must_use]
    pub fn slot(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for KeyRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A secret value.
///
/// `Debug` and `Display` are deliberately redacted, so a secret that ends up in
/// a `tracing` field or an error chain prints as `<redacted>` rather than
/// leaking. Reading the real value requires calling [`Secret::expose`], which is
/// easy to grep for in review.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// Yield the underlying value. Every call site is a place to check that the
    /// secret is going somewhere legitimate.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Failure modes of the credential store.
#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("no credential stored for {0}")]
    NotFound(KeyRef),

    #[error("credential slot name is not valid")]
    InvalidSlot,

    /// The platform has no usable credential store. On Linux this means no
    /// Secret Service provider is running; we say so rather than silently
    /// falling back to something weaker.
    #[error("no OS credential store is available: {detail}")]
    NoBackend { detail: String },

    #[error("credential store error: {0}")]
    Backend(String),
}

/// Read and write credentials.
///
/// A trait so tests can substitute an in-memory store without touching the
/// developer's real keychain.
pub trait CredentialStore: Send + Sync + fmt::Debug {
    fn get(&self, key: &KeyRef) -> Result<Secret, CredentialError>;
    fn set(&self, key: &KeyRef, secret: &Secret) -> Result<(), CredentialError>;
    fn delete(&self, key: &KeyRef) -> Result<(), CredentialError>;

    /// Whether a credential exists, without reading it.
    fn contains(&self, key: &KeyRef) -> bool {
        matches!(self.get(key), Ok(s) if !s.is_empty())
    }
}

/// The real store, backed by the OS keychain.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsCredentialStore;

impl OsCredentialStore {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    fn entry(key: &KeyRef) -> Result<keyring::Entry, CredentialError> {
        keyring::Entry::new(SERVICE, key.slot()).map_err(|e| match e {
            keyring::Error::PlatformFailure(inner) => CredentialError::NoBackend {
                detail: inner.to_string(),
            },
            other => CredentialError::Backend(other.to_string()),
        })
    }
}

impl CredentialStore for OsCredentialStore {
    fn get(&self, key: &KeyRef) -> Result<Secret, CredentialError> {
        match Self::entry(key)?.get_password() {
            Ok(value) => Ok(Secret::new(value)),
            Err(keyring::Error::NoEntry) => Err(CredentialError::NotFound(key.clone())),
            Err(keyring::Error::PlatformFailure(inner)) => Err(CredentialError::NoBackend {
                detail: inner.to_string(),
            }),
            Err(other) => Err(CredentialError::Backend(other.to_string())),
        }
    }

    fn set(&self, key: &KeyRef, secret: &Secret) -> Result<(), CredentialError> {
        Self::entry(key)?
            .set_password(secret.expose())
            .map_err(|e| CredentialError::Backend(e.to_string()))
    }

    fn delete(&self, key: &KeyRef) -> Result<(), CredentialError> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(other) => Err(CredentialError::Backend(other.to_string())),
        }
    }
}

/// An in-memory store for tests. Never used in the application.
#[derive(Debug, Default)]
pub struct MemoryCredentialStore {
    inner: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl MemoryCredentialStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Recover from a poisoned lock rather than propagating a panic. A poisoned
    /// mutex in a credential cache is not worth taking the process down for.
    fn lock(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<String, String>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl CredentialStore for MemoryCredentialStore {
    fn get(&self, key: &KeyRef) -> Result<Secret, CredentialError> {
        self.lock()
            .get(key.slot())
            .map(|v| Secret::new(v.clone()))
            .ok_or_else(|| CredentialError::NotFound(key.clone()))
    }

    fn set(&self, key: &KeyRef, secret: &Secret) -> Result<(), CredentialError> {
        self.lock()
            .insert(key.slot().to_owned(), secret.expose().to_owned());
        Ok(())
    }

    fn delete(&self, key: &KeyRef) -> Result<(), CredentialError> {
        self.lock().remove(key.slot());
        Ok(())
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

    fn key(s: &str) -> KeyRef {
        KeyRef::new(s).expect("test slot must be valid")
    }

    #[test]
    fn accepts_the_slot_names_the_registry_produces() {
        for slot in [
            "openai",
            "anthropic",
            "berget",
            "note-formatting-custom",
            "dictation-agent-vision-custom",
        ] {
            assert!(KeyRef::new(slot).is_some(), "{slot} should be valid");
        }
    }

    #[test]
    fn rejects_slot_names_that_could_escape_the_namespace() {
        for slot in [
            "",
            "Openai",              // uppercase
            "open ai",             // space
            "openai/../anthropic", // traversal
            "openai\0",            // nul
            "openai:extra",        // separator
        ] {
            assert!(KeyRef::new(slot).is_none(), "{slot:?} should be rejected");
        }
        assert!(KeyRef::new(&"a".repeat(129)).is_none(), "over-long slot");
    }

    #[test]
    fn secret_never_prints_its_value() {
        let s = Secret::new("sk-do-not-log-me".to_owned());
        assert_eq!(format!("{s}"), "<redacted>");
        assert_eq!(format!("{s:?}"), "Secret(<redacted>)");
        assert!(!format!("{s} {s:?}").contains("do-not-log-me"));
        // The value is still reachable deliberately.
        assert_eq!(s.expose(), "sk-do-not-log-me");
    }

    #[test]
    fn secret_treats_whitespace_as_empty() {
        assert!(Secret::new(String::new()).is_empty());
        assert!(Secret::new("   \n".to_owned()).is_empty());
        assert!(!Secret::new("x".to_owned()).is_empty());
    }

    #[test]
    fn memory_store_round_trips() {
        let store = MemoryCredentialStore::new();
        let k = key("openai");

        assert!(matches!(store.get(&k), Err(CredentialError::NotFound(_))));
        assert!(!store.contains(&k));

        store.set(&k, &Secret::new("sk-test".to_owned())).unwrap();
        assert_eq!(store.get(&k).unwrap().expose(), "sk-test");
        assert!(store.contains(&k));

        store.delete(&k).unwrap();
        assert!(matches!(store.get(&k), Err(CredentialError::NotFound(_))));
    }

    #[test]
    fn deleting_a_missing_credential_is_not_an_error() {
        let store = MemoryCredentialStore::new();
        assert!(store.delete(&key("never-stored")).is_ok());
    }

    #[test]
    fn contains_is_false_for_a_blank_credential() {
        let store = MemoryCredentialStore::new();
        let k = key("blank");
        store.set(&k, &Secret::new("  ".to_owned())).unwrap();
        assert!(
            !store.contains(&k),
            "a blank credential is not a credential"
        );
    }

    #[test]
    fn error_display_does_not_leak_a_secret() {
        let e = CredentialError::NotFound(key("openai"));
        let rendered = e.to_string();
        assert!(rendered.contains("openai"), "the slot name is safe to show");
        assert!(!rendered.contains("sk-"));
    }
}
