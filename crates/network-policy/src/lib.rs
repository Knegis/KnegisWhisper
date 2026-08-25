//! The only outbound-network egress point in the workspace.
//!
//! Every request must declare a [`RequestPurpose`]. There is no default and no
//! unlabelled path: a call site that cannot name why it is reaching the network
//! does not compile.
//!
//! This crate owns the only [`reqwest::Client`] in the workspace. No other crate
//! declares `reqwest` as a dependency, and no React component has any HTTP
//! capability at all — the frontend asks Rust, and Rust decides.
//!
//! See `docs/NETWORK_BOUNDARY.md`.

use std::collections::BTreeSet;
use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

/// Why a request is being made. Mirrors `allowedEgressPurposes` in
/// `config/forbidden-domains.json`.
///
/// Adding a variant here is a deliberate widening of the network boundary and
/// should be accompanied by a change to that file and to
/// `docs/NETWORK_BOUNDARY.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RequestPurpose {
    /// A reasoning or assistant call to a provider the user configured.
    UserConfiguredAiProvider,
    /// A transcription call to a provider the user configured.
    UserConfiguredTranscriptionProvider,
    /// Google Calendar or Microsoft Graph, under the user's own OAuth grant.
    UserConfiguredCalendarProvider,
    /// Fetching a model the user chose to install.
    UserTriggeredModelDownload,
    /// Importing media from a URL the user supplied.
    UserTriggeredMediaImport,
    /// Optional update check against this project's own GitHub releases.
    OwnGithubReleasesUpdater,
    /// A self-hosted endpoint the user explicitly pointed us at.
    ExplicitlyConfiguredSelfHostedEndpoint,
}

/// Every purpose, so callers can enumerate the boundary without restating it.
///
/// A test asserts this matches `allowedEgressPurposes` in
/// `config/forbidden-domains.json`, which is what stops the enum, the config,
/// and `docs/NETWORK_BOUNDARY.md` from drifting apart.
pub const ALL_PURPOSES: [RequestPurpose; 7] = [
    RequestPurpose::UserConfiguredAiProvider,
    RequestPurpose::UserConfiguredTranscriptionProvider,
    RequestPurpose::UserConfiguredCalendarProvider,
    RequestPurpose::UserTriggeredModelDownload,
    RequestPurpose::UserTriggeredMediaImport,
    RequestPurpose::OwnGithubReleasesUpdater,
    RequestPurpose::ExplicitlyConfiguredSelfHostedEndpoint,
];

impl RequestPurpose {
    /// Stable identifier used in logs and diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UserConfiguredAiProvider => "user-configured-ai-provider",
            Self::UserConfiguredTranscriptionProvider => "user-configured-transcription-provider",
            Self::UserConfiguredCalendarProvider => "user-configured-calendar-provider",
            Self::UserTriggeredModelDownload => "user-triggered-model-download",
            Self::UserTriggeredMediaImport => "user-triggered-media-import",
            Self::OwnGithubReleasesUpdater => "own-github-releases-updater",
            Self::ExplicitlyConfiguredSelfHostedEndpoint => {
                "explicitly-configured-self-hosted-endpoint"
            }
        }
    }

    /// Whether this purpose may target a plain-HTTP endpoint.
    ///
    /// Only a self-hosted endpoint may, and only on a loopback address — a user
    /// running a local inference server should not be forced to terminate TLS,
    /// but nothing else gets an exemption.
    #[must_use]
    pub const fn allows_plaintext_loopback(self) -> bool {
        matches!(self, Self::ExplicitlyConfiguredSelfHostedEndpoint)
    }
}

/// Why a request was refused before it left the process.
#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
    #[error("url could not be parsed: {0}")]
    InvalidUrl(#[from] url::ParseError),

    #[error("url has no host")]
    MissingHost,

    #[error("scheme {scheme:?} is not permitted; only https is, or http on loopback for a self-hosted endpoint")]
    SchemeNotPermitted { scheme: String },

    #[error("host {host:?} is a blocked vendor service and must never be contacted")]
    BlockedVendorHost { host: String },

    #[error("http is only permitted on a loopback address, and {host:?} is not one")]
    PlaintextNonLoopback { host: String },

    #[error("failed to build the http client: {0}")]
    Client(#[from] reqwest::Error),
}

/// Vendor hosts that must never be contacted, parsed once from
/// `config/forbidden-domains.json` so the runtime check and the CI scanner
/// cannot drift apart.
fn blocked_hosts() -> &'static BTreeSet<String> {
    static BLOCKED: OnceLock<BTreeSet<String>> = OnceLock::new();
    BLOCKED.get_or_init(|| {
        const CONFIG: &str = include_str!("../../../config/forbidden-domains.json");

        #[derive(Deserialize)]
        struct Config {
            #[serde(rename = "runtimeBlockedHosts")]
            runtime_blocked_hosts: Vec<String>,
        }

        // A malformed config must not silently disable the blocklist. Falling
        // back to an empty set would turn a typo into a policy hole, so fall
        // back to the hosts we know matter most.
        match serde_json::from_str::<Config>(CONFIG) {
            Ok(cfg) => cfg
                .runtime_blocked_hosts
                .into_iter()
                .map(|h| h.to_ascii_lowercase())
                .collect(),
            Err(_) => ["openwhispr.com", "pluely.com"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        }
    })
}

/// True if `host` is, or is a subdomain of, a blocked vendor host.
fn is_blocked(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    blocked_hosts().iter().any(|blocked| {
        host == *blocked
            || host
                .strip_suffix(blocked.as_str())
                .is_some_and(|prefix| prefix.ends_with('.'))
    })
}

fn is_loopback(host: &str) -> bool {
    use std::net::IpAddr;
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    // Url::host_str keeps IPv6 literals bracketed.
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    bare.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Validate a URL against the network boundary without sending anything.
///
/// Exposed separately so settings screens can reject a bad endpoint at the point
/// the user types it, using exactly the rules the request path will apply.
pub fn validate(url: &str, purpose: RequestPurpose) -> Result<Url, PolicyError> {
    let parsed = Url::parse(url)?;
    let host = parsed.host_str().ok_or(PolicyError::MissingHost)?;

    if is_blocked(host) {
        return Err(PolicyError::BlockedVendorHost {
            host: host.to_owned(),
        });
    }

    match parsed.scheme() {
        "https" => Ok(parsed),
        "http" if purpose.allows_plaintext_loopback() => {
            if is_loopback(host) {
                Ok(parsed)
            } else {
                Err(PolicyError::PlaintextNonLoopback {
                    host: host.to_owned(),
                })
            }
        }
        scheme => Err(PolicyError::SchemeNotPermitted {
            scheme: scheme.to_owned(),
        }),
    }
}

/// The workspace's single HTTP client.
#[derive(Debug, Clone)]
pub struct NetworkPolicy {
    client: reqwest::Client,
}

impl NetworkPolicy {
    /// Build the client.
    ///
    /// No cookie store and no redirect-following by default: cookies would let a
    /// provider correlate requests across sessions, and silent redirects are how
    /// an allowed host becomes a disallowed one.
    pub fn new() -> Result<Self, PolicyError> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("KnegisWhisper/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .build()?;
        Ok(Self { client })
    }

    /// Begin a request. The URL is validated against `purpose` before a
    /// builder is handed back, so an invalid target fails here rather than on
    /// send.
    pub fn request(
        &self,
        purpose: RequestPurpose,
        method: reqwest::Method,
        url: &str,
    ) -> Result<reqwest::RequestBuilder, PolicyError> {
        let validated = validate(url, purpose)?;
        tracing::debug!(
            purpose = purpose.as_str(),
            host = validated.host_str().unwrap_or("<none>"),
            "egress"
        );
        Ok(self.client.request(method, validated))
    }

    /// Convenience wrapper for `GET`.
    pub fn get(
        &self,
        purpose: RequestPurpose,
        url: &str,
    ) -> Result<reqwest::RequestBuilder, PolicyError> {
        self.request(purpose, reqwest::Method::GET, url)
    }

    /// Convenience wrapper for `POST`.
    pub fn post(
        &self,
        purpose: RequestPurpose,
        url: &str,
    ) -> Result<reqwest::RequestBuilder, PolicyError> {
        self.request(purpose, reqwest::Method::POST, url)
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

    const AI: RequestPurpose = RequestPurpose::UserConfiguredAiProvider;
    const SELF_HOSTED: RequestPurpose = RequestPurpose::ExplicitlyConfiguredSelfHostedEndpoint;

    #[test]
    fn allows_a_normal_provider_over_https() {
        assert!(validate("https://api.openai.com/v1/chat/completions", AI).is_ok());
    }

    #[test]
    fn rejects_plaintext_for_a_cloud_provider() {
        assert!(matches!(
            validate("http://api.openai.com/v1", AI),
            Err(PolicyError::SchemeNotPermitted { .. })
        ));
    }

    #[test]
    fn allows_plaintext_only_on_loopback_and_only_for_self_hosted() {
        assert!(validate("http://127.0.0.1:8080/v1", SELF_HOSTED).is_ok());
        assert!(validate("http://localhost:8080/v1", SELF_HOSTED).is_ok());
        assert!(validate("http://[::1]:8080/v1", SELF_HOSTED).is_ok());
        assert!(matches!(
            validate("http://192.168.1.10:8080/v1", SELF_HOSTED),
            Err(PolicyError::PlaintextNonLoopback { .. })
        ));
    }

    #[test]
    fn blocks_vendor_hosts_for_every_purpose() {
        for url in [
            "https://api.openwhispr.com/api/v1/notes",
            "https://auth.openwhispr.com/session",
            "https://mcp.openwhispr.com/mcp",
            "https://pluely.com/api/update",
            "https://app.posthog.com/capture",
        ] {
            assert!(
                matches!(
                    validate(url, AI),
                    Err(PolicyError::BlockedVendorHost { .. })
                ),
                "expected {url} to be blocked"
            );
        }
    }

    #[test]
    fn blocks_subdomains_of_vendor_hosts_but_not_lookalikes() {
        assert!(matches!(
            validate("https://anything.openwhispr.com/x", AI),
            Err(PolicyError::BlockedVendorHost { .. })
        ));
        // A suffix match must not catch a different registrable domain that
        // merely ends with the same letters.
        assert!(validate("https://notopenwhispr.com/x", AI).is_ok());
        assert!(validate("https://openwhispr.com.example.org/x", AI).is_ok());
    }

    #[test]
    fn rejects_non_http_schemes() {
        for url in [
            "file:///etc/passwd",
            "ftp://example.com/x",
            "ws://example.com",
        ] {
            assert!(
                matches!(
                    validate(url, AI),
                    Err(PolicyError::SchemeNotPermitted { .. } | PolicyError::MissingHost)
                ),
                "expected {url} to be rejected"
            );
        }
    }

    #[test]
    fn blocklist_is_loaded_from_config_not_the_fallback() {
        // If include_str!/serde ever break, the fallback set has 2 entries. The
        // real config has more, so this asserts we are reading the config.
        assert!(
            blocked_hosts().len() > 2,
            "blocklist looks like the parse-failure fallback: {:?}",
            blocked_hosts()
        );
        assert!(blocked_hosts().contains("mcp.openwhispr.com"));
    }

    #[test]
    fn purpose_strings_match_the_config_vocabulary() {
        const CONFIG: &str = include_str!("../../../config/forbidden-domains.json");
        let cfg: serde_json::Value =
            serde_json::from_str(CONFIG).expect("config/forbidden-domains.json must be valid json");
        let allowed: Vec<&str> = cfg["allowedEgressPurposes"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        assert_eq!(
            ALL_PURPOSES.len(),
            allowed.len(),
            "purpose count drifted from config"
        );
        for p in ALL_PURPOSES {
            assert!(
                allowed.contains(&p.as_str()),
                "{} is not in allowedEgressPurposes",
                p.as_str()
            );
        }
    }
}
