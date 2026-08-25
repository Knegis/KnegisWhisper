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
    /// Fetching a local model the user chose to install, or the runtime that
    /// executes it.
    ///
    /// Both halves are one action from the user's point of view -- they asked
    /// for local transcription, and it needs a `whisper-server` binary and a
    /// GGML model file. Splitting them into two purposes would say the boundary
    /// is about file types rather than about reasons, and the brief fixes the
    /// number of purposes at seven.
    ///
    /// The extra danger of the executable half is answered by a stronger
    /// requirement rather than a different purpose: `kw_sidecar` cannot
    /// represent an executable without a pinned SHA-256, so a binary reaches
    /// this purpose only when the bytes were decided at review time.
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

    #[error(
        "{url} redirected more than {limit} times. A redirect chain that long is a          misconfiguration or a loop, not a download."
    )]
    TooManyRedirects { url: String, limit: u8 },

    #[error("{url} returned {status} with no Location header to follow")]
    RedirectWithoutLocation { url: String, status: u16 },

    #[error(
        "{url} redirected to {to}, which this purpose does not permit: {reason}.          Following it would turn an allowed host into a disallowed one."
    )]
    RedirectNotPermitted {
        url: String,
        to: String,
        reason: String,
    },

    #[error("{url} returned {status}")]
    HttpStatus { url: String, status: u16 },

    #[error("{path:?} is not a usable loopback path: {reason}")]
    NotALoopbackPath { path: String, reason: &'static str },

    #[error("could not write {path}: {source}")]
    Write {
        path: String,
        #[source]
        source: std::io::Error,
    },
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

    /// Talk to a sidecar this application started, on loopback.
    ///
    /// Deliberately *not* one of the seven purposes, because this is not egress:
    /// nothing leaves the machine. `whisper-server` and `llama-server` are
    /// processes we spawned, bound to `127.0.0.1` by `crate::port`, and a
    /// request to one is closer to an IPC call than to a network request.
    ///
    /// What keeps that claim honest is the signature. It takes a port and a
    /// path, never a URL, and builds the address itself -- so there is no way to
    /// aim this at another host, and no way for a later change to quietly widen
    /// it. The path is parsed rather than trusted, so a path carrying an
    /// authority or control characters is refused instead of producing a request
    /// somewhere unintended.
    pub fn loopback(
        &self,
        method: reqwest::Method,
        port: u16,
        path: &str,
    ) -> Result<reqwest::RequestBuilder, PolicyError> {
        if !path.starts_with('/') {
            return Err(PolicyError::NotALoopbackPath {
                path: path.to_owned(),
                reason: "a loopback path has to start with /",
            });
        }
        if path.contains("://") {
            return Err(PolicyError::NotALoopbackPath {
                path: path.to_owned(),
                reason: "a loopback path cannot carry a scheme",
            });
        }

        let base = format!("http://{}:{port}", std::net::Ipv4Addr::LOCALHOST);
        let url = Url::parse(&base)
            .and_then(|base| base.join(path))
            .map_err(PolicyError::InvalidUrl)?;

        // The host is written by us, so this can only fail if `join` replaced
        // the authority -- which is exactly the case worth refusing rather than
        // sending.
        if url.host_str() != Some("127.0.0.1") || url.port() != Some(port) {
            return Err(PolicyError::NotALoopbackPath {
                path: path.to_owned(),
                reason: "that path would change the host or port",
            });
        }

        Ok(self.client.request(method, url))
    }

    /// Stream a URL to a file, following redirects one validated hop at a time.
    ///
    /// The client is built with `Policy::none()` because a silently followed
    /// redirect is how an allowed host becomes a disallowed one. But a download
    /// genuinely needs redirects: a GitHub release asset answers with a 302 to
    /// a storage host, so refusing to follow would mean refusing to download.
    ///
    /// The answer is to follow *visibly*. Every hop is validated against the
    /// same purpose before it is requested, the chain is bounded, and the hops
    /// are returned so the caller can report where the bytes actually came
    /// from. A redirect to a blocked vendor host, to plaintext, or to a scheme
    /// this purpose does not allow is an error naming both ends.
    ///
    /// The body is written as it arrives rather than buffered: these files run
    /// to gigabytes.
    pub async fn download_to_file(
        &self,
        purpose: RequestPurpose,
        url: &str,
        destination: &std::path::Path,
        // `+ Send` because this callback is held across awaits: without it the
        // returned future is not `Send` and cannot be spawned or used in a
        // Tauri command.
        progress: &mut (dyn FnMut(u64, Option<u64>) + Send),
    ) -> Result<Download, PolicyError> {
        use tokio::io::AsyncWriteExt as _;

        const MAX_HOPS: u8 = 5;

        let mut current = url.to_owned();
        let mut hops: Vec<RedirectHop> = Vec::new();

        let response = loop {
            let response = self
                .request(purpose, reqwest::Method::GET, &current)?
                .send()
                .await?;

            let status = response.status();
            if !status.is_redirection() {
                if !status.is_success() {
                    return Err(PolicyError::HttpStatus {
                        url: current,
                        status: status.as_u16(),
                    });
                }
                break response;
            }

            if hops.len() as u8 >= MAX_HOPS {
                return Err(PolicyError::TooManyRedirects {
                    url: url.to_owned(),
                    limit: MAX_HOPS,
                });
            }

            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| PolicyError::RedirectWithoutLocation {
                    url: current.clone(),
                    status: status.as_u16(),
                })?;

            let next = next_hop(&current, location, purpose)?;

            tracing::debug!(from = %current, to = %next, "following a validated redirect");
            hops.push(RedirectHop {
                from: current.clone(),
                to: next.clone(),
            });
            current = next;
        };

        let total = response.content_length();
        let mut file = tokio::fs::File::create(destination)
            .await
            .map_err(|source| PolicyError::Write {
                path: destination.display().to_string(),
                source,
            })?;

        let mut written: u64 = 0;
        let mut stream = response;
        while let Some(chunk) = stream.chunk().await? {
            file.write_all(&chunk)
                .await
                .map_err(|source| PolicyError::Write {
                    path: destination.display().to_string(),
                    source,
                })?;
            written += chunk.len() as u64;
            progress(written, total);
        }

        file.flush().await.map_err(|source| PolicyError::Write {
            path: destination.display().to_string(),
            source,
        })?;

        Ok(Download {
            bytes: written,
            final_url: current,
            hops,
        })
    }
}

/// Resolve a `Location` header against the URL that produced it, and check the
/// result is somewhere this purpose may go.
///
/// Extracted from the download loop so the decision can be tested directly.
/// The loop around it is plumbing; this is the part that has to be right.
fn next_hop(current: &str, location: &str, purpose: RequestPurpose) -> Result<String, PolicyError> {
    // Resolved against the current URL, so a relative Location works.
    let next = Url::parse(current)
        .and_then(|base| base.join(location))
        .map_err(PolicyError::InvalidUrl)?
        .to_string();

    // The point of following redirects by hand: the new target is checked, and
    // a refusal names both ends rather than reporting a bare scheme error from
    // a URL the caller never typed.
    if let Err(reason) = validate(&next, purpose) {
        return Err(PolicyError::RedirectNotPermitted {
            url: current.to_owned(),
            to: next,
            reason: reason.to_string(),
        });
    }

    Ok(next)
}

/// One redirect that was followed, after being validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedirectHop {
    pub from: String,
    pub to: String,
}

/// What a completed download did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    pub bytes: u64,
    /// Where the bytes actually came from, after redirects.
    pub final_url: String,
    /// Every hop taken, so a report can show the chain rather than implying
    /// the bytes came from the URL that was asked for.
    pub hops: Vec<RedirectHop>,
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
    const DOWNLOAD: RequestPurpose = RequestPurpose::UserTriggeredModelDownload;

    // ---------------------------------------------------------------- loopback

    /// The URL a loopback request would actually be sent to.
    fn loopback_url(port: u16, path: &str) -> Result<String, PolicyError> {
        let policy = NetworkPolicy::new()?;
        let request = policy.loopback(reqwest::Method::GET, port, path)?;
        Ok(request
            .build()
            .map_err(PolicyError::Client)?
            .url()
            .to_string())
    }

    #[test]
    fn a_sidecar_health_check_addresses_loopback() {
        assert_eq!(loopback_url(8178, "/").unwrap(), "http://127.0.0.1:8178/");
        assert_eq!(
            loopback_url(8178, "/inference").unwrap(),
            "http://127.0.0.1:8178/inference"
        );
    }

    #[test]
    fn a_path_that_tries_to_change_the_host_is_refused() {
        // The reason this takes a port and a path rather than a URL: there has
        // to be no way to aim it off-machine.
        for path in [
            "//evil.example.com/x",
            "/x/../../..//evil.example.com",
            "https://evil.example.com/x",
        ] {
            match loopback_url(8178, path) {
                Err(_) => {}
                Ok(url) => assert!(
                    url.starts_with("http://127.0.0.1:8178/"),
                    "{path:?} produced {url}"
                ),
            }
        }
    }

    #[test]
    fn a_path_without_a_leading_slash_is_refused() {
        let err = loopback_url(8178, "inference").unwrap_err();
        assert!(matches!(err, PolicyError::NotALoopbackPath { .. }));
    }

    #[test]
    fn a_path_carrying_a_scheme_is_refused() {
        let err = loopback_url(8178, "/redirect?to=http://evil.example.com").unwrap_err();
        assert!(matches!(err, PolicyError::NotALoopbackPath { .. }));
    }

    #[test]
    fn loopback_is_not_one_of_the_seven_purposes() {
        // It is not egress, so it must not appear in the enumerated boundary.
        // A test rather than a comment, because the count is a stated constraint.
        assert_eq!(ALL_PURPOSES.len(), 7);
    }

    // ------------------------------------------------------- redirect following

    const RELEASE: &str =
        "https://github.com/ggml-org/whisper.cpp/releases/download/b4938/whisper-bin-x64.zip";

    #[test]
    fn a_release_asset_redirect_to_storage_is_followed() {
        // The real shape: GitHub answers a release download with a 302 to a
        // storage host. Refusing to follow would mean refusing to download.
        let storage = "https://objects.githubusercontent.com/github-production-release-asset/1/2";
        assert_eq!(next_hop(RELEASE, storage, DOWNLOAD).unwrap(), storage);
    }

    #[test]
    fn a_relative_location_is_resolved_against_the_current_url() {
        let next = next_hop(RELEASE, "/other/path.zip", DOWNLOAD).unwrap();
        assert_eq!(next, "https://github.com/other/path.zip");
    }

    #[test]
    fn a_redirect_to_plaintext_is_refused_and_names_both_ends() {
        // The downgrade attack this manual following exists to catch.
        let err = next_hop(RELEASE, "http://example.com/x.zip", DOWNLOAD).unwrap_err();
        let message = err.to_string();
        assert!(matches!(err, PolicyError::RedirectNotPermitted { .. }));
        assert!(message.contains("github.com"), "the origin: {message}");
        assert!(
            message.contains("http://example.com"),
            "the target: {message}"
        );
        assert!(message.contains("allowed host into a disallowed one"));
    }

    #[test]
    fn a_redirect_to_a_blocked_vendor_host_is_refused() {
        // domain-scanner:allow-next-line a negative test must name the host it asserts is blocked
        let err = next_hop(RELEASE, "https://api.openwhispr.com/x.zip", DOWNLOAD).unwrap_err();
        assert!(matches!(err, PolicyError::RedirectNotPermitted { .. }));
    }

    #[test]
    fn a_redirect_to_a_non_http_scheme_is_refused() {
        for target in ["file:///etc/passwd", "ftp://example.com/x.zip"] {
            assert!(
                next_hop(RELEASE, target, DOWNLOAD).is_err(),
                "{target} must not be followed"
            );
        }
    }

    #[test]
    fn a_redirect_is_validated_against_the_purpose_it_started_with() {
        // A self-hosted endpoint may redirect within the private range; a model
        // download may not, because plaintext is not its to allow.
        let from = "http://192.168.1.10:8080/v1/audio/transcriptions";
        assert!(next_hop(from, "http://192.168.1.11:8080/v1", SELF_HOSTED).is_ok());
        assert!(next_hop(from, "http://192.168.1.11:8080/v1", DOWNLOAD).is_err());
    }

    #[tokio::test]
    async fn a_download_target_is_validated_before_any_socket_is_opened() {
        // Nothing is listening on this port. If the purpose check happened after
        // the connection attempt, the error would be a connection failure.
        let policy = NetworkPolicy::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut seen = |_written: u64, _total: Option<u64>| {};

        let err = policy
            .download_to_file(
                DOWNLOAD,
                "http://127.0.0.1:9/whisper.zip",
                &dir.path().join("out.zip"),
                &mut seen,
            )
            .await
            .unwrap_err();

        assert!(
            matches!(err, PolicyError::SchemeNotPermitted { .. }),
            "expected a policy refusal, got {err}"
        );
        assert!(
            !dir.path().join("out.zip").exists(),
            "a refused download must not leave a file behind"
        );
    }

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
