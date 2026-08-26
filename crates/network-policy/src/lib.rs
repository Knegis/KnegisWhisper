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

    /// Whether this purpose may target a private, loopback or link-local host
    /// at all -- over plain HTTP, or over HTTPS.
    ///
    /// Only a self-hosted endpoint may. A user running whisper.cpp or llama.cpp
    /// on their own machine, or on a box on their own network, should not be
    /// forced to terminate TLS to use it; nothing else gets an exemption. The
    /// name is about plaintext because that was the only place this used to
    /// matter -- `validate`'s HTTPS arm did not consult it at all, so every
    /// purpose could already reach a private host over HTTPS. That was the gap:
    /// a valid certificate says nothing about whether the host is internal, and
    /// this predicate is now the one place both the plaintext and the TLS arm
    /// ask "is this purpose allowed near a private host in the first place."
    ///
    /// The exemption covers the whole private range rather than only loopback,
    /// because a self-hosted model server on another machine on the user's LAN
    /// is an ordinary setup, and refusing it would mean the only way to use one
    /// is to put a certificate on it. That is a real trade -- audio and any
    /// configured credential cross the local network in the clear when the
    /// endpoint is plain HTTP -- and it is the user's to make, for an endpoint
    /// they typed in themselves.
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

    #[error(
        "{host:?} is a private, loopback or link-local address, and https to one is only permitted for an explicitly configured self-hosted endpoint"
    )]
    PrivateHttpsTarget { host: String },

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
///
/// This is a check on the literal address in the URL, at the time the URL is
/// validated. It is not a check on where a hostname resolves at connect time,
/// and it cannot be: `reqwest` resolves DNS itself, after this function has
/// already returned. A public-looking hostname that a DNS answer later points
/// at `127.0.0.1` or `169.254.169.254` -- "DNS rebinding" -- is not something
/// this function, or anything else in this crate, detects. That would need a
/// check at connect time against the address actually dialled, which this
/// crate does not implement.
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
        return is_private_ip(ip);
    }

    // A single-label hostname has no public DNS resolution, so it can only be a
    // machine on the local network.
    !host.contains('.')
}

/// Whether a parsed IP address is on a private network.
///
/// Split out of [`is_private_host`] so an IPv4-mapped IPv6 address
/// (`::ffff:a.b.c.d`) can be unwrapped to the IPv4 address it actually is and
/// re-checked against the same IPv4 rules, rather than against a second,
/// looser notion of "private" that only understands the IPv6 ranges.
/// `::ffff:169.254.169.254` is exactly `169.254.169.254` wearing an IPv6
/// costume, and the policy must see through it.
fn is_private_ip(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;

    match ip {
        IpAddr::V4(v4) => is_private_ipv4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_private_ipv4(mapped);
            }
            let first = v6.segments().first().copied().unwrap_or(0);
            v6.is_loopback()
                || v6.is_unspecified()
                // fc00::/7 unique-local, fe80::/10 link-local.
                || first & 0xfe00 == 0xfc00
                || first & 0xffc0 == 0xfe80
        }
    }
}

/// The IPv4 half of [`is_private_ip`], shared by a plain IPv4 address and an
/// IPv4-mapped IPv6 one. `is_link_local` alone already covers
/// `169.254.169.254`, the cloud-metadata address most SSRF exploits target --
/// it falls inside `169.254.0.0/16` like every other link-local address, with
/// nothing metadata-specific about it.
fn is_private_ipv4(v4: std::net::Ipv4Addr) -> bool {
    v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
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
        // HTTPS is permitted to any host that is not on the vendor blocklist --
        // except a private, loopback or link-local one, which only the
        // self-hosted purpose may reach. Without this check `https://127.0.0.1/`
        // and `https://169.254.169.254/` were both reachable under every
        // purpose: TLS says nothing about where a host sits on the network, and
        // treating a valid certificate as proof the target is not internal is
        // exactly the gap SSRF exploits.
        "https" if is_private_host(host) && !purpose.allows_plaintext_private() => {
            Err(PolicyError::PrivateHttpsTarget {
                host: host.to_owned(),
            })
        }
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

/// Validate a WebSocket URL against the same boundary.
///
/// A separate function rather than two more arms in [`validate`], because the two
/// answer different questions and a caller must not be able to hand a `wss://`
/// URL to the HTTP client by accident. `validate` stays the HTTP gate and refuses
/// `wss` exactly as it did.
///
/// The rules are the same rules. `wss` is checked as `https` and `ws` as `http`,
/// so the vendor blocklist, the private-address exemption and the
/// plaintext refusal all apply unchanged — there is one set of rules about where
/// audio may go, not one per transport.
///
/// Meeting audio travels under [`RequestPurpose::UserConfiguredTranscriptionProvider`].
/// The brief fixes the number of purposes at seven, and a socket carrying audio
/// to a provider the user configured is that purpose whatever the scheme.
pub fn validate_websocket(url: &str, purpose: RequestPurpose) -> Result<Url, PolicyError> {
    let parsed = Url::parse(url)?;
    let host = parsed.host_str().ok_or(PolicyError::MissingHost)?;

    if is_blocked(host) {
        return Err(PolicyError::BlockedVendorHost {
            host: host.to_owned(),
        });
    }

    match parsed.scheme() {
        // Mirrors the "https" arm of `validate`: the two are one set of rules,
        // not one per transport, so a socket to a private address is refused
        // under the same rule a request would be.
        "wss" if is_private_host(host) && !purpose.allows_plaintext_private() => {
            Err(PolicyError::PrivateHttpsTarget {
                host: host.to_owned(),
            })
        }
        "wss" => Ok(parsed),
        "ws" if purpose.allows_plaintext_private() => {
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

    /// Fetch a small text payload -- a release manifest, not a bundle.
    ///
    /// Kept inside this crate, the same way [`Self::download_to_file`] is, so a
    /// caller checking for an update never needs `reqwest` in scope just to read
    /// the response. This stays the one crate that names it.
    ///
    /// Unlike `download_to_file` this does not follow redirects: a release
    /// *asset* genuinely gets one (GitHub answers with a 302 to a storage
    /// host), but a small JSON manifest is not expected to, and a scheme this
    /// purpose does not allow, or a host the redirect chain wanders onto, would
    /// be exactly the downgrade this crate exists to refuse. A redirect
    /// therefore surfaces as an ordinary `HttpStatus` error rather than being
    /// followed silently or hidden.
    pub async fn get_text(
        &self,
        purpose: RequestPurpose,
        url: &str,
    ) -> Result<String, PolicyError> {
        let response = self.get(purpose, url)?.send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(PolicyError::HttpStatus {
                url: url.to_owned(),
                status: status.as_u16(),
            });
        }
        Ok(response.text().await?)
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
    const TRANSCRIPTION: RequestPurpose = RequestPurpose::UserConfiguredTranscriptionProvider;
    const SELF_HOSTED: RequestPurpose = RequestPurpose::ExplicitlyConfiguredSelfHostedEndpoint;
    const DOWNLOAD: RequestPurpose = RequestPurpose::UserTriggeredModelDownload;
    const UPDATER: RequestPurpose = RequestPurpose::OwnGithubReleasesUpdater;

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

    // -------------------------------------------------------------- get_text

    #[tokio::test]
    async fn a_text_fetch_target_is_validated_before_any_socket_is_opened() {
        // Nothing is listening on this port, and the updater purpose does not
        // allow plaintext at all -- so a scheme refusal proves the validation
        // ran before any connection was attempted.
        let policy = NetworkPolicy::new().unwrap();
        let err = policy
            .get_text(
                UPDATER,
                "http://127.0.0.1:9/repos/owner/repo/releases/latest",
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, PolicyError::SchemeNotPermitted { .. }),
            "expected a policy refusal, got {err}"
        );
    }

    /// A minimal HTTP/1.1 server for one request, so `get_text`'s success and
    /// failure paths can be exercised without a real host. It does not parse
    /// the request at all -- reading enough bytes to let the client finish
    /// writing is all a single-request test needs.
    async fn respond_once(status_line: &'static str, body: &'static [u8]) -> std::net::SocketAddr {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            // Best-effort: enough to drain the request so the client's write
            // does not block on us, not a real parse.
            let _ = socket.read(&mut buf).await;

            let response = format!(
                "{status_line}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.write_all(body).await;
            let _ = socket.shutdown().await;
        });

        addr
    }

    #[tokio::test]
    async fn a_small_text_payload_is_fetched_and_returned() {
        let body = br#"{"tag_name":"v9.9.9","html_url":"https://example.invalid/releases/v9.9.9"}"#;
        let addr = respond_once("HTTP/1.1 200 OK", body).await;

        let policy = NetworkPolicy::new().unwrap();
        // Loopback, plaintext: only the self-hosted purpose may reach it, but
        // the mechanism under test -- reading a small body back as text -- has
        // nothing to do with which purpose validated the URL.
        let text = policy
            .get_text(SELF_HOSTED, &format!("http://{addr}/release.json"))
            .await
            .unwrap();
        assert_eq!(text, String::from_utf8_lossy(body));
    }

    #[tokio::test]
    async fn a_non_success_status_is_reported_rather_than_returned_as_text() {
        let addr = respond_once("HTTP/1.1 404 Not Found", b"not found").await;

        let policy = NetworkPolicy::new().unwrap();
        let err = policy
            .get_text(SELF_HOSTED, &format!("http://{addr}/missing"))
            .await
            .unwrap_err();
        assert!(
            matches!(err, PolicyError::HttpStatus { status: 404, .. }),
            "expected a 404 to be reported, got {err}"
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

    #[test]
    fn ipv4_mapped_ipv6_addresses_are_recognised_as_private() {
        // `::ffff:a.b.c.d` is `a.b.c.d` wearing an IPv6 costume. `is_loopback`,
        // `is_unspecified` and the unique-local/link-local bitmasks all miss
        // it -- none of them look at the embedded IPv4 address -- so this is a
        // distinct check, not a stronger version of the existing ones.
        for private in [
            "::ffff:127.0.0.1",       // loopback
            "[::ffff:127.0.0.1]",     // bracketed, as `Url::host_str` returns it
            "::ffff:169.254.169.254", // the cloud-metadata address specifically
            "::ffff:10.0.0.5",        // RFC 1918
            "::ffff:192.168.1.1",     // RFC 1918
            "::ffff:0.0.0.0",         // unspecified
        ] {
            assert!(is_private_host(private), "{private} should be private");
        }

        for public in ["::ffff:8.8.8.8", "::ffff:1.1.1.1"] {
            assert!(!is_private_host(public), "{public} should not be private");
        }
    }

    #[test]
    fn ipv6_unique_local_and_link_local_ranges_are_private() {
        for private in [
            "fc00::1",           // unique-local, fc00::/7
            "fdff:ffff:ffff::1", // unique-local, top of the range
            "fe80::1",           // link-local, fe80::/10
            "febf::1",           // link-local, top of the range
        ] {
            assert!(is_private_host(private), "{private} should be private");
        }

        // Outside both ranges: a documentation-range address, and one just
        // past the fe80::/10 slice.
        for public in ["2001:db8::1", "fec0::1"] {
            assert!(!is_private_host(public), "{public} should not be private");
        }
    }

    #[test]
    fn https_to_a_private_host_is_refused_except_for_self_hosted() {
        // This is the gap Task 20 exists to close: the "https" arm used to run
        // no host check at all, so every one of these reached every purpose,
        // and a redirect to one (see `next_hop` below) was followed rather
        // than refused.
        for private in [
            "https://127.0.0.1/",
            "https://[::1]/",
            // The classic cloud-metadata SSRF target.
            "https://169.254.169.254/latest/meta-data/",
            "https://10.0.0.5/",
            "https://192.168.1.10/",
            "https://172.16.4.4/",
            "https://0.0.0.0/",
            "https://whisper-box/",
            "https://nas.local/",
            // Decimal, octal and hex IPv4 -- normalised to dotted-quad by URL
            // parsing before `is_private_host` ever runs.
            "https://2130706433/", // 127.0.0.1 as a 32-bit integer
            "https://0177.0.0.1/", // 127 in octal
            "https://0x7f.0.0.1/", // 127 in hex
            // IPv4-mapped IPv6.
            "https://[::ffff:127.0.0.1]/",
            "https://[::ffff:169.254.169.254]/",
        ] {
            for purpose in ALL_PURPOSES {
                if purpose == SELF_HOSTED {
                    continue;
                }
                assert!(
                    matches!(
                        validate(private, purpose),
                        Err(PolicyError::PrivateHttpsTarget { .. })
                    ),
                    "{private} should be refused over https for {purpose:?}"
                );
            }
        }
    }

    #[test]
    fn https_to_a_private_host_is_allowed_for_self_hosted() {
        // The fix must not make every self-hosted https endpoint stop
        // working: a user who put a certificate on their own LAN server must
        // still be able to reach it.
        for private in [
            "https://127.0.0.1:8443/v1",
            "https://192.168.1.10:8443/v1",
            "https://[::1]:8443/v1",
        ] {
            assert!(
                validate(private, SELF_HOSTED).is_ok(),
                "{private} should still be reachable for the self-hosted purpose"
            );
        }
    }

    #[test]
    fn https_to_a_public_host_is_unaffected() {
        // The common case: nearly every real call is exactly this, and the
        // fix must leave it alone.
        for public in [
            "https://api.openai.com/v1",
            "https://8.8.8.8/",
            "https://172.32.0.1/",
        ] {
            assert!(validate(public, AI).is_ok(), "{public} should be allowed");
        }
    }

    #[test]
    fn a_redirect_to_a_private_https_host_is_refused_for_a_non_self_hosted_purpose() {
        // The concrete SSRF this task exists to close: a redirect chain that
        // starts at an allowed public host and ends at loopback or link-local
        // metadata, entirely over https, used to be followed -- `next_hop`
        // re-validates through the same `validate` whose https arm did not
        // check the host at all.
        for target in [
            "https://127.0.0.1/secret",
            "https://169.254.169.254/latest/meta-data/",
        ] {
            let err = next_hop(RELEASE, target, DOWNLOAD).unwrap_err();
            assert!(
                matches!(err, PolicyError::RedirectNotPermitted { .. }),
                "{target}: {err:?}"
            );
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

    /* -------------------------------------------------------- websockets */

    #[test]
    fn a_websocket_to_a_provider_is_allowed_over_tls() {
        assert!(validate_websocket("wss://api.openai.com/v1/realtime", AI).is_ok());
        assert!(validate_websocket(
            "wss://api.deepgram.com/v1/listen?model=nova-3",
            TRANSCRIPTION
        )
        .is_ok());
    }

    #[test]
    fn the_http_gate_still_refuses_a_websocket_url() {
        // The reason this is a second function and not two more arms. A caller
        // that hands a wss:// URL to the HTTP client has made a mistake, and the
        // gate should say so rather than quietly accepting a scheme reqwest
        // cannot use.
        let err = validate("wss://api.openai.com/v1/realtime", AI).unwrap_err();
        match err {
            PolicyError::SchemeNotPermitted { scheme } => assert_eq!(scheme, "wss"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_websocket_gate_refuses_an_http_url_for_the_same_reason() {
        for url in [
            "https://api.openai.com/v1/realtime",
            "http://api.openai.com/v1/realtime",
        ] {
            let err = validate_websocket(url, AI).unwrap_err();
            assert!(
                matches!(err, PolicyError::SchemeNotPermitted { .. }),
                "{url}: {err:?}"
            );
        }
    }

    #[test]
    fn plaintext_websockets_follow_the_same_rule_as_plaintext_http() {
        // One set of rules about where audio may go, not one per transport.
        assert!(validate_websocket("ws://127.0.0.1:8221/stream", SELF_HOSTED).is_ok());
        assert!(validate_websocket("ws://192.168.1.20:9000/stream", SELF_HOSTED).is_ok());

        // A public host in the clear is refused, exactly as over http.
        let err = validate_websocket("ws://api.deepgram.com/v1/listen", SELF_HOSTED).unwrap_err();
        assert!(
            matches!(err, PolicyError::PlaintextNonPrivate { .. }),
            "{err:?}"
        );

        // And plaintext is only for the self-hosted purpose.
        let err = validate_websocket("ws://127.0.0.1:8221/stream", TRANSCRIPTION).unwrap_err();
        assert!(
            matches!(err, PolicyError::SchemeNotPermitted { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn wss_to_a_private_host_is_refused_except_for_self_hosted() {
        // Mirrors the https fix in `validate`: the "wss" arm used to run no
        // host check either, so meeting audio over a websocket could be
        // pointed at loopback or link-local space under any purpose.
        for private in [
            "wss://127.0.0.1/stream",
            "wss://[::1]/stream",
            "wss://169.254.169.254/stream",
            "wss://[::ffff:127.0.0.1]/stream",
        ] {
            let err = validate_websocket(private, TRANSCRIPTION).unwrap_err();
            assert!(
                matches!(err, PolicyError::PrivateHttpsTarget { .. }),
                "{private}: {err:?}"
            );
        }

        // The self-hosted purpose keeps working over wss, exactly as it does
        // over ws and over https.
        assert!(validate_websocket("wss://127.0.0.1:8221/stream", SELF_HOSTED).is_ok());
        assert!(validate_websocket("wss://192.168.1.20:9000/stream", SELF_HOSTED).is_ok());
    }

    #[test]
    fn a_blocked_vendor_host_is_blocked_over_a_websocket_too() {
        // The whole point of sharing the rules. A vendor service the brief
        // removes must not be reachable by changing the scheme.
        for url in [
            // domain-scanner:allow-next-line a negative test must name the host it asserts is blocked
            "wss://api.openwhispr.com/realtime",
            // domain-scanner:allow-next-line a negative test must name the host it asserts is blocked
            "wss://mcp.openwhispr.com/stream",
        ] {
            let err = validate_websocket(url, TRANSCRIPTION).unwrap_err();
            assert!(
                matches!(err, PolicyError::BlockedVendorHost { .. }),
                "{url}: {err:?}"
            );
        }
    }

    #[test]
    fn a_malformed_websocket_url_is_refused_rather_than_attempted() {
        for url in ["wss://", "realtime", "not a url", ""] {
            let err = validate_websocket(url, TRANSCRIPTION).unwrap_err();
            assert!(
                matches!(err, PolicyError::InvalidUrl(_) | PolicyError::MissingHost),
                "{url:?}: {err:?}"
            );
        }
    }

    #[test]
    fn an_empty_authority_makes_the_first_path_segment_the_host() {
        // Recorded because it surprised me while writing the test above:
        // `wss:///realtime` does not have an empty host, it has a host called
        // `realtime`. The `url` crate resolves the authority that way and
        // `validate` has always behaved the same for `https:///x`.
        //
        // A single-label host is exactly what `is_private_host` treats as
        // private -- it has no public DNS resolution -- so proving the host
        // resolution quirk now needs a purpose allowed near a private host.
        let parsed = validate_websocket("wss:///realtime", SELF_HOSTED).unwrap();
        assert_eq!(parsed.host_str(), Some("realtime"));
        assert_eq!(parsed.path(), "/");

        // For a purpose not allowed near a private host, that same
        // single-label host is now refused rather than silently accepted.
        // Before this task, the obvious-looking `.unwrap()` above would have
        // succeeded for every purpose, `TRANSCRIPTION` included -- which is
        // the gap this task closes, showing up in an unexpected corner.
        let err = validate_websocket("wss:///realtime", TRANSCRIPTION).unwrap_err();
        assert!(
            matches!(err, PolicyError::PrivateHttpsTarget { .. }),
            "{err:?}"
        );

        // And the blocklist still applies to a host that arrives this way.
        // domain-scanner:allow-next-line a negative test must name the host it asserts is blocked
        let err = validate_websocket("wss:///api.openwhispr.com", TRANSCRIPTION).unwrap_err();
        assert!(
            matches!(err, PolicyError::BlockedVendorHost { .. }),
            "{err:?}"
        );
    }
}
