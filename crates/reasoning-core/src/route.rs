//! Deciding which reasoning backend handles a request.
//!
//! ## The bug this fixes
//!
//! Upstream's `chatRouting.js` classifies against a hardcoded set:
//!
//! ```js
//! if (!CLOUD_CHAT_PROVIDERS.has(provider)) {
//!   return { kind: "local", baseUrl: "", apiKey: "" };
//! }
//! ```
//!
//! Anything the set does not name is classified as a **local model**. That is
//! not a harmless mislabel: a "local" route goes to a llama.cpp server on
//! loopback, so a cloud provider missing from the set produces a request to
//! `127.0.0.1` that fails with a connection error — and the user is told their
//! local model is broken when in fact their cloud provider was never consulted.
//!
//! This is exactly what happened to Berget: it was a fully configured cloud
//! provider that the BYOK commit had to add to `CLOUD_CHAT_PROVIDERS` to fix.
//! Adding an entry fixes the instance; the class of bug survives, and returns
//! with the next provider.
//!
//! Here the cloud set is **derived from the provider registry**, so a registry
//! entry is a cloud provider by construction, and an id in neither the registry
//! nor the local set is [`ChatRouteError::UnknownProvider`] rather than a
//! misdirected request to loopback.

use kw_provider_registry::Registry;
use kw_security::KeyRef;
use serde::{Deserialize, Serialize};

use kw_transcription_core::url::{is_secure_http_endpoint, normalize_base_url};

/// Provider ids that mean "a server on this machine or this network".
///
/// Not registry-derived because these have no credential and no fixed base URL —
/// they are a *kind* of destination rather than a provider.
pub const LOCAL_PROVIDERS: [&str; 2] = ["local", "lan"];

/// Where a reasoning request goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ChatRoute {
    /// An endpoint the user typed. Wins over everything, including a stale
    /// enterprise provider id left in settings — it is a declared intent.
    SelfHosted {
        base_url: String,
        /// A slot, if the user configured a credential for it. Self-hosted
        /// servers commonly need none.
        key_ref: Option<KeyRef>,
    },

    /// A llama.cpp server this application manages.
    Local,

    /// A cloud provider from the registry.
    Provider {
        id: String,
        base_url: String,
        key_ref: KeyRef,
    },

    /// Bedrock, Azure, or Vertex: organisation credentials, different request
    /// shapes, and no single base URL.
    Enterprise { id: String },
}

/// Why a route could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChatRouteError {
    /// The fix for the misclassification described in the module documentation.
    #[error(
        "{id:?} is not a reasoning provider this application knows. Nothing was sent. \
         (Upstream would have classified this as a local model and sent the request to \
         loopback, which fails in a way that blames the local model.)"
    )]
    UnknownProvider { id: String },

    #[error("the self-hosted URL {url:?} is invalid or unsupported: {reason}")]
    SelfHostedInvalid { url: String, reason: String },

    #[error("the registry gives {id} no base URL, so it cannot be reached directly")]
    NoBaseUrl { id: String },

    #[error("the registry names credential slot {slot:?}, which is not a valid slot name")]
    InvalidSlot { slot: String },
}

/// The settings a chat route is resolved from.
///
/// Chat settings only. Upstream's comment is worth keeping: this must never
/// consult the dictation-cleanup mode or endpoint, because the two are
/// separately configurable and mixing them silently sends a chat request to a
/// cleanup endpoint.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChatSettings {
    pub provider: Option<String>,
    /// An explicit self-hosted or LAN URL.
    pub self_hosted_url: Option<String>,
    /// Whether the provider is an enterprise one.
    pub enterprise: bool,
}

/// Resolve a reasoning route.
pub fn resolve_chat(
    registry: &Registry,
    settings: &ChatSettings,
) -> Result<ChatRoute, ChatRouteError> {
    // A URL the user typed is a declared route and wins over everything.
    let self_hosted = settings.self_hosted_url.as_deref().unwrap_or("").trim();
    if !self_hosted.is_empty() {
        let base =
            normalize_base_url(self_hosted).ok_or_else(|| ChatRouteError::SelfHostedInvalid {
                url: self_hosted.to_owned(),
                reason: "it is not a URL".to_owned(),
            })?;

        if !is_secure_http_endpoint(&base) {
            return Err(ChatRouteError::SelfHostedInvalid {
                url: base,
                reason: "it uses plaintext HTTP to a public host".to_owned(),
            });
        }

        return Ok(ChatRoute::SelfHosted {
            base_url: base,
            key_ref: KeyRef::new("custom"),
        });
    }

    let provider = settings
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .unwrap_or("openai");

    if settings.enterprise {
        // Validated against the registry rather than trusted: an enterprise flag
        // with an unknown id must not become a request to nowhere.
        if !registry.enterprise.iter().any(|e| e.id == provider) {
            return Err(ChatRouteError::UnknownProvider {
                id: provider.to_owned(),
            });
        }
        return Ok(ChatRoute::Enterprise {
            id: provider.to_owned(),
        });
    }

    if LOCAL_PROVIDERS.contains(&provider) {
        return Ok(ChatRoute::Local);
    }

    // Derived from the registry, not a hardcoded set. This is the fix.
    let Ok(entry) = registry.reasoning_provider(provider) else {
        return Err(ChatRouteError::UnknownProvider {
            id: provider.to_owned(),
        });
    };

    // A reasoning provider's base URL comes from its transcription twin where it
    // has one, since the registry stores base URLs once.
    let base_url = registry
        .transcription
        .iter()
        .find(|t| t.id == provider)
        .and_then(|t| t.base_url.clone())
        .or_else(|| reasoning_base_url(provider))
        .ok_or_else(|| ChatRouteError::NoBaseUrl {
            id: provider.to_owned(),
        })?;

    let key_ref =
        KeyRef::new(&entry.credential_slot).ok_or_else(|| ChatRouteError::InvalidSlot {
            slot: entry.credential_slot.clone(),
        })?;

    Ok(ChatRoute::Provider {
        id: provider.to_owned(),
        base_url,
        key_ref,
    })
}

/// Base URLs for reasoning-only providers, which have no transcription entry.
///
/// A small table rather than registry data because these providers appear only
/// on the reasoning side; when one of them gains transcription support the
/// registry entry becomes authoritative and this stops being consulted.
fn reasoning_base_url(provider: &str) -> Option<String> {
    Some(
        match provider {
            "anthropic" => "https://api.anthropic.com/v1",
            "gemini" => "https://generativelanguage.googleapis.com/v1beta/openai",
            "openrouter" => "https://openrouter.ai/api/v1",
            _ => return None,
        }
        .to_owned(),
    )
}

/// Providers that are reachable as cloud providers, derived from the registry.
///
/// Exposed so a settings screen offers exactly what can be routed — the
/// mismatch between the two is what produced the upstream bug.
#[must_use]
pub fn cloud_chat_providers(registry: &Registry) -> Vec<&str> {
    registry
        .reasoning
        .iter()
        .map(|p| p.id.as_str())
        .filter(|id| !LOCAL_PROVIDERS.contains(id))
        .collect()
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

    fn registry() -> &'static Registry {
        kw_provider_registry::registry().expect("the shipped registry must parse")
    }

    fn with_provider(id: &str) -> ChatSettings {
        ChatSettings {
            provider: Some(id.to_owned()),
            ..Default::default()
        }
    }

    /* ------------------------------------------------------- the actual bug */

    #[test]
    fn an_unknown_provider_is_an_error_not_a_local_model() {
        // The bug: upstream returns kind:"local" here, so the request goes to a
        // llama.cpp server on loopback and fails in a way that blames the local
        // model rather than the unrecognised provider id.
        for bogus in ["berget-ai", "OpenAI", "claude", "not-a-provider"] {
            match resolve_chat(registry(), &with_provider(bogus)) {
                Err(ChatRouteError::UnknownProvider { id }) => assert_eq!(id, bogus),
                other => panic!("{bogus:?} should have failed closed, got {other:?}"),
            }
        }
    }

    #[test]
    fn berget_the_provider_that_exposed_the_bug_now_routes_correctly() {
        // It is a registry entry, so it is a cloud provider by construction --
        // there is no separate list it can be missing from.
        match resolve_chat(registry(), &with_provider("berget")).unwrap() {
            ChatRoute::Provider {
                id,
                base_url,
                key_ref,
            } => {
                assert_eq!(id, "berget");
                assert!(base_url.contains("berget"), "{base_url}");
                assert_eq!(key_ref.slot(), "berget");
            }
            other => panic!("berget must not be classified as local: {other:?}"),
        }
    }

    #[test]
    fn the_cloud_set_is_derived_so_it_cannot_drift_from_the_registry() {
        let derived = cloud_chat_providers(registry());
        // Every reasoning provider is routable, which is the invariant that
        // makes the hardcoded-set bug impossible.
        for id in &derived {
            let route = resolve_chat(registry(), &with_provider(id));
            assert!(
                route.is_ok(),
                "{id} is offered but does not route: {route:?}"
            );
        }
        assert!(derived.contains(&"berget"), "{derived:?}");
        assert!(derived.contains(&"openai"));
        assert!(!derived.contains(&"local"));
    }

    /* ------------------------------------------------------------- dispatch */

    #[test]
    fn an_explicit_self_hosted_url_wins_over_everything() {
        // Including a stale enterprise flag, which upstream also gets right.
        let settings = ChatSettings {
            provider: Some("bedrock".to_owned()),
            self_hosted_url: Some("http://127.0.0.1:8221/v1".to_owned()),
            enterprise: true,
        };
        match resolve_chat(registry(), &settings).unwrap() {
            ChatRoute::SelfHosted { base_url, .. } => {
                assert_eq!(base_url, "http://127.0.0.1:8221/v1");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_plaintext_self_hosted_url_to_a_public_host_is_refused() {
        let settings = ChatSettings {
            self_hosted_url: Some("http://llm.example.com/v1".to_owned()),
            ..Default::default()
        };
        assert!(matches!(
            resolve_chat(registry(), &settings),
            Err(ChatRouteError::SelfHostedInvalid { .. })
        ));
    }

    #[test]
    fn a_private_plaintext_self_hosted_url_is_allowed() {
        // The normal case: llama.cpp on loopback, or a box on the LAN.
        for url in [
            "http://127.0.0.1:8221/v1",
            "http://localhost:8221/v1",
            "http://192.168.1.9:8080/v1",
        ] {
            let settings = ChatSettings {
                self_hosted_url: Some(url.to_owned()),
                ..Default::default()
            };
            assert!(resolve_chat(registry(), &settings).is_ok(), "{url}");
        }
    }

    #[test]
    fn the_local_provider_ids_route_locally() {
        for id in LOCAL_PROVIDERS {
            assert_eq!(
                resolve_chat(registry(), &with_provider(id)).unwrap(),
                ChatRoute::Local
            );
        }
    }

    #[test]
    fn an_enterprise_provider_is_validated_against_the_registry() {
        let bedrock = ChatSettings {
            provider: Some("bedrock".to_owned()),
            enterprise: true,
            ..Default::default()
        };
        match resolve_chat(registry(), &bedrock).unwrap() {
            ChatRoute::Enterprise { id } => assert_eq!(id, "bedrock"),
            other => panic!("{other:?}"),
        }

        // An enterprise flag with a nonsense id must not become a request to
        // nowhere.
        let bogus = ChatSettings {
            provider: Some("not-an-enterprise".to_owned()),
            enterprise: true,
            ..Default::default()
        };
        assert!(matches!(
            resolve_chat(registry(), &bogus),
            Err(ChatRouteError::UnknownProvider { .. })
        ));
    }

    #[test]
    fn no_configuration_defaults_to_openai() {
        match resolve_chat(registry(), &ChatSettings::default()).unwrap() {
            ChatRoute::Provider { id, .. } => assert_eq!(id, "openai"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn reasoning_only_providers_still_get_a_base_url() {
        // anthropic and gemini have no transcription entry, so they come from
        // the small reasoning-only table.
        for id in ["anthropic", "gemini"] {
            match resolve_chat(registry(), &with_provider(id)).unwrap() {
                ChatRoute::Provider { base_url, .. } => {
                    assert!(base_url.starts_with("https://"), "{id}: {base_url}");
                }
                other => panic!("{id}: {other:?}"),
            }
        }
    }

    /* -------------------------------------------------------------- secrecy */

    #[test]
    fn a_resolved_route_carries_a_slot_and_never_a_credential() {
        for id in cloud_chat_providers(registry()) {
            let route = resolve_chat(registry(), &with_provider(id)).unwrap();
            let json = serde_json::to_string(&route).unwrap();
            for needle in ["sk-", "gsk_", "Bearer ", "apiKey"] {
                assert!(!json.contains(needle), "{id}: {needle} in {json}");
            }
        }
    }

    #[test]
    fn every_resolved_slot_is_one_the_registry_declares() {
        let slots = registry().credential_slots();
        for id in cloud_chat_providers(registry()) {
            if let Ok(ChatRoute::Provider { key_ref, .. }) =
                resolve_chat(registry(), &with_provider(id))
            {
                assert!(
                    slots.contains(key_ref.slot()),
                    "{id} resolved to undeclared slot {:?}",
                    key_ref.slot()
                );
            }
        }
    }
}
