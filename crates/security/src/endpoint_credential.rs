//! The custom key and its allowed destinations are one OS-vault record.
//! Mutable routing settings cannot extend this authority. Only saving a newly
//! supplied key can do that; legacy unbound values require re-entry.

use crate::{CredentialError, CredentialStore, KeyRef, Secret};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundKey {
    version: u8,
    origins: Vec<String>,
    key: String,
}

fn origin(endpoint: &str) -> Result<String, CredentialError> {
    let url = url::Url::parse(endpoint).map_err(|_| CredentialError::InvalidEndpoint)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(CredentialError::InvalidEndpoint);
    }
    Ok(url.origin().ascii_serialization())
}

/// Bind a newly supplied key. Never call this on a value read from the vault:
/// doing so would let mutable settings re-authorize an existing secret.
pub fn bind_custom(secret: &Secret, endpoints: &[&str]) -> Result<Secret, CredentialError> {
    if endpoints.is_empty() || secret.is_empty() {
        return Err(CredentialError::InvalidEndpoint);
    }
    let mut origins = endpoints
        .iter()
        .map(|endpoint| origin(endpoint))
        .collect::<Result<Vec<_>, _>>()?;
    origins.sort();
    origins.dedup();
    let bound = BoundKey {
        version: 1,
        origins,
        key: secret.expose().to_owned(),
    };
    serde_json::to_string(&bound)
        .map(Secret::new)
        .map_err(|_| CredentialError::Backend("could not encode the custom key".into()))
}

/// Resolve a credential against the final request URL, before any network I/O.
/// Fixed provider slots retain their existing behavior.
pub fn get_for_endpoint(
    store: &dyn CredentialStore,
    slot: &KeyRef,
    endpoint: &str,
) -> Result<Secret, CredentialError> {
    let stored = store.get(slot)?;
    if slot.slot() != "custom" {
        return Ok(stored);
    }
    let bound: BoundKey =
        serde_json::from_str(stored.expose()).map_err(|_| CredentialError::UnapprovedEndpoint)?;
    let requested = origin(endpoint)?;
    if bound.version != 1 || !bound.origins.contains(&requested) || bound.key.trim().is_empty() {
        return Err(CredentialError::UnapprovedEndpoint);
    }
    Ok(Secret::new(bound.key))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::MemoryCredentialStore;

    #[test]
    fn origin_authority_includes_scheme_host_and_effective_port() {
        let store = MemoryCredentialStore::new();
        let slot = KeyRef::new("custom").unwrap();
        let supplied = Secret::new("synthetic-key".into());
        let bound = bind_custom(
            &supplied,
            &["https://EXAMPLE.test:443/v1", "http://[::1]:8181/v1"],
        )
        .unwrap();
        store.set(&slot, &bound).unwrap();
        for allowed in [
            "https://example.test/another/path?model=m",
            "http://[0:0:0:0:0:0:0:1]:8181/",
        ] {
            assert_eq!(get_for_endpoint(&store, &slot, allowed).unwrap(), supplied);
        }
        for denied in [
            "http://example.test/v1",
            "https://example.test:444/v1",
            "https://example.test.attacker.invalid/v1",
            "https://attacker.invalid/example.test",
            "https://example.test@attacker.invalid/v1",
            "https://name:pass@example.test/v1",
            "http://127.0.0.1:8181/v1",
            "file:///example.test",
            "not a URL",
        ] {
            assert!(get_for_endpoint(&store, &slot, denied).is_err(), "{denied}");
        }
        assert!(!format!("{bound:?}").contains("synthetic-key"));
    }

    #[test]
    fn unbound_or_malformed_values_never_gain_authority_from_settings() {
        let store = MemoryCredentialStore::new();
        let slot = KeyRef::new("custom").unwrap();
        for raw in [
            "legacy-key",
            "",
            "{}",
            r#"{"version":2,"origins":["https://example.test"],"key":"k"}"#,
        ] {
            store.set(&slot, &Secret::new(raw.into())).unwrap();
            assert!(matches!(
                get_for_endpoint(&store, &slot, "https://example.test/v1"),
                Err(CredentialError::UnapprovedEndpoint)
            ));
        }
        store.delete(&slot).unwrap();
        assert!(matches!(
            get_for_endpoint(&store, &slot, "https://example.test"),
            Err(CredentialError::NotFound(_))
        ));
        assert!(bind_custom(&Secret::new("key".into()), &[]).is_err());
        assert!(bind_custom(&Secret::new("key".into()), &["https://user@example.test"]).is_err());
    }

    #[test]
    fn a_fresh_key_that_looks_like_an_envelope_is_still_only_a_key() {
        let store = MemoryCredentialStore::new();
        let slot = KeyRef::new("custom").unwrap();
        let supplied = Secret::new(
            r#"{"version":1,"origins":["https://attacker.invalid"],"key":"inner"}"#.into(),
        );
        store
            .set(
                &slot,
                &bind_custom(&supplied, &["https://approved.test"]).unwrap(),
            )
            .unwrap();
        assert_eq!(
            get_for_endpoint(&store, &slot, "https://approved.test/v1").unwrap(),
            supplied
        );
        assert!(get_for_endpoint(&store, &slot, "https://attacker.invalid").is_err());
        let fixed = KeyRef::new("openai").unwrap();
        store
            .set(&fixed, &Secret::new("fixed-provider-key".into()))
            .unwrap();
        assert_eq!(
            get_for_endpoint(&store, &fixed, "https://api.openai.com")
                .unwrap()
                .expose(),
            "fixed-provider-key"
        );
    }
}
