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

    /// Whether this purpose may target a plain-HTTP endpoint on a private host.
    ///
    /// Only a self-hosted endpoint may. A user running whisper.cpp or llama.cpp
    /// on their own machine, or on a box on their own network, should not be
    /// forced to terminate TLS to use it; nothing else gets an exemption.
    ///
    /// The exemption covers the whole private range rather than only loopback,
    /// because a self-hosted model server on another machine on the user's LAN
    /// is an ordinary setup, and refusing it would mean the only way to use one
    /// is to put a certificate on it. That is a real trade -- audio and any
    /// configured credential cross the local network in the clear -- and it is
    /// the user's to make, for an endpoint they typed in themselves.
    #[must_use]
    pub const fn allows_plaintext_private(self) -> bool {
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

    #[error(
        "scheme {scheme:?} is not permitted; only https is, or http to a private address          for an explicitly configured self-hosted endpoint"
    )]
    SchemeNotPermitted { scheme: String },

    #[error("host {host:?} is a blocked vendor service and must never be contacted")]
    BlockedVendorHost { host: String },

    #[error(
        "http is only permitted to a private address, and {host:?} is not one.          Use https, or point this at a machine on your own network."
    )]
    PlaintextNonPrivate { host: String },

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
            // domain-scanner:allow-next-line the parse-failure fallback must name the hosts it blocks
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

/// Whether `host` is on a private network, and therefore whether plaintext to
/// it stays inside the user's own control.
///
/// Public because it is the *security* predicate: route resolution and this
/// layer must agree about what counts as private, and the only way to guarantee
/// that is for both to call the same function. They previously disagreed --
/// route resolution allowed the whole private range while this layer allowed
/// only loopback -- which meant a self-hosted endpoint the settings screen
/// accepted was refused at send time.
#[must_use]
pub fn is_private_host(host: &str) -> bool {
    use std::net::IpAddr;

    // Url::host_str keeps IPv6 literals bracketed.
    let host = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();

    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    // mDNS and the conventional private suffixes.
    if host.ends_with(".local") || host.ends_with(".internal") || host.ends_with(".home.arpa") {
        return true;
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => {
                v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
            }
            IpAddr::V6(v6) => {
                let first = v6.segments().first().copied().unwrap_or(0);
                v6.is_loopback()
                    || v6.is_unspecified()
                    // fc00::/7 unique-local, fe80::/10 link-local.
                    || first & 0xfe00 == 0xfc00
                    || first & 0xffc0 == 0xfe80
            }
        };
    }

    // A single-label hostname has no public DNS resolution, so it can only be a
    // machine on the local network.
    !host.contains('.')
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
        "http" if purpose.allows_plaintext_private() => {
            if is_private_host(host) {
                Ok(parsed)
            } else {
                Err(PolicyError::PlaintextNonPrivate {
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
    fn allows_plaintext_to_a_private_host_and_only_for_self_hosted() {
        // Loopback: whisper.cpp or llama.cpp on this machine.
        assert!(validate("http://127.0.0.1:8080/v1", SELF_HOSTED).is_ok());
        assert!(validate("http://localhost:8080/v1", SELF_HOSTED).is_ok());
        assert!(validate("http://[::1]:8080/v1", SELF_HOSTED).is_ok());

        // The rest of the private range: a model server on the user's own LAN.
        // Refusing these would mean the only way to use one is to put a
        // certificate on it.
        assert!(validate("http://192.168.1.10:8080/v1", SELF_HOSTED).is_ok());
        assert!(validate("http://10.0.0.5:8080/v1", SELF_HOSTED).is_ok());
        assert!(validate("http://172.16.4.4/v1", SELF_HOSTED).is_ok());
        assert!(validate("http://whisper-box:8178/v1", SELF_HOSTED).is_ok());
        assert!(validate("http://nas.local/v1", SELF_HOSTED).is_ok());

        // A public host is still refused, even for a self-hosted purpose.
        for public in [
            "http://transcribe.example.com/v1",
            "http://8.8.8.8/v1",
            "http://172.32.0.1/v1",
        ] {
            assert!(
                matches!(
                    validate(public, SELF_HOSTED),
                    Err(PolicyError::PlaintextNonPrivate { .. })
                ),
                "{public} should be refused"
            );
        }
    }

    #[test]
    fn no_other_purpose_gets_the_plaintext_exemption() {
        // Widening the exemption to the whole private range makes it more
        // important, not less, that only one purpose has it.
        for purpose in ALL_PURPOSES {
            let permitted = purpose == RequestPurpose::ExplicitlyConfiguredSelfHostedEndpoint;
            assert_eq!(purpose.allows_plaintext_private(), permitted, "{purpose:?}");
            assert_eq!(
                validate("http://127.0.0.1:8080/v1", purpose).is_ok(),
                permitted,
                "{purpose:?}"
            );
        }
    }

    #[test]
    fn the_private_host_predicate_matches_what_route_resolution_expects() {
        // The two layers previously disagreed -- route resolution accepted the
        // private range, this layer accepted only loopback -- so a self-hosted
        // endpoint the settings screen accepted was refused at send time. One
        // function now serves both.
        for private in [
            "localhost",
            "127.0.0.1",
            "::1",
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.0.1",
            "169.254.1.1",
            "whisper-box",
            "nas.local",
            "db.internal",
        ] {
            assert!(is_private_host(private), "{private} should be private");
        }

        for public in [
            "api.openai.com",
            "8.8.8.8",
            "172.32.0.0",
            "11.0.0.1",
            "192.169.0.1",
            "example.com",
        ] {
            assert!(!is_private_host(public), "{public} should not be private");
        }
    }

    // Tests that name blocked vendor hosts live in tests/blocklist.rs, so this
    // file stays fully covered by the forbidden-domain scanner. Suppressing the
    // scanner across the crate that owns egress would blind it exactly where it
    // matters most.

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
        // domain-scanner:allow-next-line asserting a host is in the runtime blocklist
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
