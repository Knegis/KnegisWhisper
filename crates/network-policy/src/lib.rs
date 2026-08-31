//! The only outbound-network egress point in the workspace.
//!
//! Every request must declare a [`RequestPurpose`]. There is no default and no
//! unlabelled path: a call site that cannot name why it is reaching the network
//! does not compile.
//!
//! This crate owns the only [`reqwest::Client`] that any part of the desktop
//! application's own egress path constructs. That claim is narrower than "no
//! other crate declares `reqwest`" — several do (`calendar-core`,
//! `local-api`, `reasoning-core`, `sidecar`, `transcription-core`,
//! `provider-probe`, and both `tools/knegiswhisper*` binaries), but only to
//! chain onto the [`reqwest::RequestBuilder`] that [`NetworkPolicy::post`],
//! [`NetworkPolicy::get`] and [`NetworkPolicy::loopback`] hand back (headers,
//! multipart bodies, methods, response and error types) — none of them
//! construct a `Client` of their own to reach the network with. Corrected by
//! Task 19: this doc comment previously claimed no other crate declared the
//! dependency at all, which was already false before this run and stayed
//! false afterwards. Two real exceptions exist and both are outside this
//! egress path rather than gaps in it: `crates/calendar-core/tests/oauth_flow.rs`
//! and `crates/local-api/tests/api.rs` each build a bare client as a test
//! double confined to their own test binary, and `tools/knegiswhisper`, a
//! separate CLI process, builds one to talk to the local API over loopback —
//! which `docs/NETWORK_BOUNDARY.md`'s "Loopback is not egress" section holds
//! is not one of the seven purposes in the first place, because nothing
//! leaves the machine.
//!
//! No React component has any HTTP capability at all — the frontend asks
//! Rust, and Rust decides.
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

    #[error("download exceeded its pinned {limit}-byte limit")]
    DownloadTooLarge { limit: u64 },
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
                // Trailing dots come off here as well as in `is_blocked`.
                // Without it the two sides normalise differently: an entry
                // written with a trailing DNS root dot would never match an
                // incoming host, because the incoming one has already had its
                // dot stripped. No entry has one today -- every line in
                // `config/forbidden-domains.json` is written by this project
                // -- so this closes a footgun for whoever next edits that
                // file rather than a live hole. An asymmetry that costs
                // nothing to remove is not worth keeping to save a line.
                .map(|h| h.trim_end_matches('.').to_ascii_lowercase())
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
    // A trailing "." is the DNS root and resolves identically to the name
    // without it -- a blocked host with one appended is still that host -- so
    // it must be stripped before comparison or it silently exits both
    // branches below. `trim_end_matches` rather than a single `strip_suffix`,
    // for the same reason as the bracket-trimming in `is_private_host`: an
    // extra dot must not be a second, narrower bypass of the same kind.
    let host = host.trim_end_matches('.').to_ascii_lowercase();
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

    // Url::host_str keeps IPv6 literals bracketed. A trailing "." is the DNS
    // root and resolves identically to the name without it -- `localhost.`
    // and `nas.local.` are `localhost` and `nas.local` as far as any resolver
    // is concerned -- so it has to come off before any of the checks below,
    // or a single dot defeats every one of them at once: it is not equal to
    // "localhost", it does not end in ".local", it is not a valid IP literal,
    // and it still contains a "." so the single-label fallback does not fire
    // either. `trim_end_matches` rather than a single `strip_suffix`, so that
    // `localhost..` is not left classified as public just because it carries
    // one dot more than a single strip removes.
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let host = host.trim_end_matches('.').to_ascii_lowercase();

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
/// Split out of [`is_private_host`] so an IPv4-embedded IPv6 address can be
/// unwrapped to the IPv4 address it actually is and re-checked against the
/// same IPv4 rules, rather than against a second, looser notion of "private"
/// that only understands the IPv6 ranges. `::ffff:169.254.169.254` is exactly
/// `169.254.169.254` wearing an IPv6 costume, and the policy must see through
/// it.
fn is_private_ip(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;

    match ip {
        IpAddr::V4(v4) => is_private_ipv4(v4),
        IpAddr::V6(v6) => {
            // `is_loopback` and `is_unspecified` are checked before `to_ipv4`
            // on purpose. `to_ipv4` recognises two embeddings: the IPv4-mapped
            // form (`::ffff:a.b.c.d`) and the deprecated IPv4-compatible form
            // (`::a.b.c.d`, a 96-bit zero prefix with the IPv4 address in the
            // low 32 bits). `::` and `::1` both have a 96-bit zero prefix, so
            // read as IPv4-compatible they would decode to `0.0.0.0` and
            // `0.0.0.1` -- an IPv4 reading of addresses that are natively IPv6
            // loopback and unspecified, not an IPv4 host wearing IPv6
            // notation. Resolving the unambiguous IPv6 meaning first avoids
            // that reinterpretation; every other 96-bit-zero-prefixed address
            // is genuinely someone's IPv4 host and `to_ipv4` is exactly right
            // for it.
            if v6.is_loopback() || v6.is_unspecified() {
                return true;
            }

            // Every RFC-defined embedding, not just the two the standard
            // library's `to_ipv4` knows. See [`embedded_ipv4s`].
            if embedded_ipv4s(v6)
                .into_iter()
                .flatten()
                .any(is_private_ipv4)
            {
                return true;
            }

            let first = v6.segments().first().copied().unwrap_or(0);
            // fc00::/7 unique-local, fe80::/10 link-local. Native IPv6
            // ranges, not IPv4 embeddings, so there is no "mapped form" of
            // these to also account for.
            first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80
        }
    }
}

/// Every IPv4 address folded inside an IPv6 one, by any RFC-defined embedding.
///
/// At most two, which is why the return type is a fixed array rather than a
/// `Vec`: only Teredo carries two IPv4 addresses in one address.
///
/// **Why the whole family at once.** `Ipv6Addr::to_ipv4` recognises two forms.
/// The rest were noticed one at a time -- IPv4-translated during the SSRF
/// review, NAT64 and 6to4 raised unprompted by the re-reviewer -- and each was
/// accepted on the same reasoning, that the prefix is unrouted on an ordinary
/// host so a `connect()` to it fails rather than arriving. That reasoning is
/// about the *host's* configuration, and it stops holding the moment a machine
/// has 6to4 or a NAT64 resolver, which is not a thing this process can see. The
/// recorded decision (KNEG-93) was that if this family is ever closed it must be
/// closed as a family, by enumerating the RFCs rather than by adding a prefix
/// each time somebody spots one. This is that enumeration.
///
/// **Why every one of these reaches the IPv4 address it carries.** In each
/// form the embedded address is the delivery target: a dual-stack kernel
/// delivers an IPv4-mapped address directly; a NAT64 gateway translates and
/// forwards; a 6to4 or Teredo tunnel encapsulates the packet and sends it to
/// the embedded IPv4. So unwrapping and re-checking against the IPv4 rules is
/// not a courtesy -- it is the only reading that describes where the bytes go.
///
/// | form | prefix | RFC |
/// | --- | --- | --- |
/// | IPv4-mapped | `::ffff:0:0/96` | 4291 §2.5.5.2 |
/// | IPv4-compatible (deprecated) | `::/96` | 4291 §2.5.5.1 |
/// | IPv4-translated (SIIT) | `::ffff:0:0:0/96` | 2765 |
/// | NAT64, well-known prefix | `64:ff9b::/96` | 6052 §2.1 |
/// | NAT64, local-use prefix | `64:ff9b:1::/48` | 8215, laid out by 6052 §2.2 |
/// | 6to4 | `2002::/16` | 3056 |
/// | Teredo | `2001:0::/32` | 4380 |
///
/// **What this still cannot enumerate**, and it is a real limit rather than an
/// oversight: RFC 6052 also permits a *network-specific* NAT64 prefix of any
/// value at /32, /40, /48, /56 or /64. Only the well-known and local-use
/// prefixes have fixed values, so a site running NAT64 on its own prefix is
/// invisible here. Nothing short of reading the host's resolver configuration
/// would see it, which is the same class of problem as DNS rebinding and is out
/// of scope for the same reason.
fn embedded_ipv4s(v6: std::net::Ipv6Addr) -> [Option<std::net::Ipv4Addr>; 2] {
    use std::net::Ipv4Addr;

    let s = v6.segments();
    let o = v6.octets();
    let v4 = |a: u8, b: u8, c: u8, d: u8| Some(Ipv4Addr::new(a, b, c, d));
    let low32 = || v4(o[12], o[13], o[14], o[15]);
    let none = [None, None];

    // `::ffff:0:0/96` mapped, and `::/96` compatible. Both put the address in
    // the low 32 bits behind an all-zero prefix; the mapped form's marker is
    // the `ffff` in segment 5. `::` and `::1` are resolved by the caller
    // before this runs, so a zero prefix here is genuinely somebody's IPv4.
    if s[0] == 0
        && s[1] == 0
        && s[2] == 0
        && s[3] == 0
        && s[4] == 0
        && (s[5] == 0xffff || s[5] == 0)
    {
        return [low32(), None];
    }

    // `::ffff:0:0:0/96` IPv4-translated, the SIIT source form: the `ffff` sits
    // one segment earlier than the mapped form, with a zero segment after it.
    if s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0xffff && s[4] == 0 && s[5] == 0 {
        return [low32(), None];
    }

    if s[0] == 0x0064 && s[1] == 0xff9b {
        // `64:ff9b::/96`, the well-known NAT64 prefix: address in the low 32.
        if s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0 {
            return [low32(), None];
        }
        // `64:ff9b:1::/48`, the local-use prefix, laid out by RFC 6052 §2.2:
        // for a /48 the address is split around the reserved u-octet, sixteen
        // bits at 48..64 and sixteen more at 72..88. Getting this wrong in the
        // obvious way -- reading four contiguous octets -- would decode a
        // different address than the gateway will.
        if s[2] == 0x0001 {
            return [v4(o[6], o[7], o[9], o[10]), None];
        }
    }

    // `2002::/16` 6to4: the IPv4 of the tunnel endpoint is bits 16..48.
    if s[0] == 0x2002 {
        return [v4(o[2], o[3], o[4], o[5]), None];
    }

    // `2001:0::/32` Teredo, and the only form carrying two addresses: the
    // server at bits 32..64 in the clear, and the client at bits 96..128
    // obfuscated by XOR with all-ones. Both are checked, because a packet to a
    // Teredo address involves both hosts and either being private is a reason
    // to refuse.
    if s[0] == 0x2001 && s[1] == 0x0000 {
        let server = v4(o[4], o[5], o[6], o[7]);
        let client = v4(!o[12], !o[13], !o[14], !o[15]);
        return [server, client];
    }

    none
}

/// The IPv4 half of [`is_private_ip`], shared by a plain IPv4 address and an
/// IPv4-embedded IPv6 one.
///
/// `is_link_local` alone already covers `169.254.169.254`, the cloud-metadata
/// address most SSRF exploits target -- it falls inside `169.254.0.0/16` like
/// every other link-local address, with nothing metadata-specific about it.
///
/// Two ranges the standard-library predicates do not cover, by design or by
/// age, are added explicitly: `0.0.0.0/8` ("this network", not just the single
/// unspecified address `is_unspecified` checks), and `100.64.0.0/10`, the
/// shared address space carrier-grade NAT uses -- a standard SSRF target and
/// one the self-hosted purpose remains free to reach deliberately, so refusing
/// it elsewhere costs nothing real.
fn is_private_ipv4(v4: std::net::Ipv4Addr) -> bool {
    let octets = v4.octets();
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_unspecified()
        || octets[0] == 0
        || (octets[0] == 100 && octets[1] & 0xc0 == 0x40)
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
        self.download_to_file_inner(purpose, url, destination, None, progress)
            .await
    }

    /// Stream a URL to a file, aborting before more than `max_bytes` are
    /// written, and deleting the partial file if it aborts.
    ///
    /// `max_bytes` is a *pinned length*, not a guess. The two callers pass a
    /// figure somebody measured — `Asset::size_bytes` from
    /// `config/sidecars.json`, or `SherpaModel::expected_size_bytes` — so the
    /// bound is exact and the digest check that follows would have failed on
    /// anything else anyway. Do not pass a rounded or declared figure: an
    /// upstream re-upload one byte over the pin would then be reported as a
    /// refused download rather than as a changed asset. (That is the mistake
    /// KNEG-101 fixed one layer up, where `download_gguf` compared a finished
    /// download against a declared size that was 7.4% wrong for eighteen
    /// models.)
    ///
    /// **Two checks, and the first one often does not run.** `content-length`
    /// is compared against `max_bytes` before the file is created, so an
    /// over-long body is refused without a byte reaching disk. That check is
    /// skipped entirely when the response carries no `content-length` — a
    /// chunked response, or HTTP/1.0 with `Connection: close` — because
    /// `Option::zip` yields `None` and the comparison cannot be made. The
    /// running total in the streaming loop is what holds in that case, and it
    /// is the check that actually guarantees the bound: a `content-length`
    /// header is a claim by the server, and a body may be longer than its own
    /// header says.
    ///
    /// Neither check is a substitute for the digest. This bounds *how much
    /// disk a hostile or broken mirror can fill before it is rejected*, which
    /// is the window a digest cannot cover, because a digest can only be
    /// computed once the bytes are already written.
    pub async fn download_to_file_bounded(
        &self,
        purpose: RequestPurpose,
        url: &str,
        destination: &std::path::Path,
        max_bytes: u64,
        progress: &mut (dyn FnMut(u64, Option<u64>) + Send),
    ) -> Result<Download, PolicyError> {
        self.download_to_file_inner(purpose, url, destination, Some(max_bytes), progress)
            .await
    }

    async fn download_to_file_inner(
        &self,
        purpose: RequestPurpose,
        url: &str,
        destination: &std::path::Path,
        max_bytes: Option<u64>,
        progress: &mut (dyn FnMut(u64, Option<u64>) + Send),
    ) -> Result<Download, PolicyError> {
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
        // Checked before the file is created, so a body that declares itself
        // over the limit never puts a byte on disk. `Option::zip` yields `None`
        // when the response carries no `content-length` at all, which skips
        // this check rather than failing it -- see the doc comment on
        // `download_to_file_bounded`. The running total in `stream_to_file` is
        // what holds in that case, and it is the check that actually
        // guarantees the bound.
        if total.zip(max_bytes).is_some_and(|(total, max)| total > max) {
            return Err(PolicyError::DownloadTooLarge {
                limit: max_bytes.unwrap_or(0),
            });
        }

        match Self::stream_to_file(response, destination, max_bytes, total, progress).await {
            Ok(written) => Ok(Download {
                bytes: written,
                final_url: current,
                hops,
            }),
            Err(err) => {
                // Every failure after the file was created, not only a refused
                // ceiling: a truncated body from a dropped connection is just
                // as convincing to a later `is_installed` check as a refused
                // one, and neither is the file that was asked for.
                //
                // Best-effort. A failed removal must not replace the error
                // that caused it, which is the one the caller needs, so it is
                // logged and the original error is returned.
                match tokio::fs::remove_file(destination).await {
                    Ok(()) => {}
                    Err(remove) if remove.kind() == std::io::ErrorKind::NotFound => {}
                    Err(remove) => tracing::warn!(
                        path = %destination.display(),
                        error = %remove,
                        "could not remove the partial download"
                    ),
                }
                Err(err)
            }
        }
    }

    /// The body half of [`Self::download_to_file_inner`]: create the file,
    /// stream into it, and report how many bytes landed.
    ///
    /// Split out so that *every* way this can fail leaves through one cleanup
    /// path in the caller. Inline, each `?` was its own partial-file leak — the
    /// streaming size refusal, the mid-stream write error, and the chunk error
    /// from a dropped connection, three separate returns and no `remove_file`
    /// after any of them.
    ///
    /// The file is closed by the time this returns, in every arm, which is what
    /// makes the caller's `remove_file` work on Windows.
    async fn stream_to_file(
        response: reqwest::Response,
        destination: &std::path::Path,
        max_bytes: Option<u64>,
        total: Option<u64>,
        progress: &mut (dyn FnMut(u64, Option<u64>) + Send),
    ) -> Result<u64, PolicyError> {
        use tokio::io::AsyncWriteExt as _;

        let mut file = tokio::fs::File::create(destination)
            .await
            .map_err(|source| PolicyError::Write {
                path: destination.display().to_string(),
                source,
            })?;

        let mut written: u64 = 0;
        let mut stream = response;
        while let Some(chunk) = stream.chunk().await? {
            // Before the write, so the byte that would cross the limit is
            // never written -- the bound is on what reaches disk, not on what
            // is noticed afterwards.
            if max_bytes.is_some_and(|max| written.saturating_add(chunk.len() as u64) > max) {
                return Err(PolicyError::DownloadTooLarge {
                    limit: max_bytes.unwrap_or(0),
                });
            }
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

        Ok(written)
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

    #[test]
    fn a_media_import_redirect_to_a_private_address_is_refused() {
        // `UserTriggeredMediaImport` (media URL import, task 11) is the
        // purpose this task introduces real use of. A public host redirecting
        // to a link-local address is exactly the shape a cloud metadata-service
        // SSRF takes -- most such services (AWS, GCP, Azure) answer only on
        // plain HTTP, which is refused here for the same reason a model
        // download redirecting to plaintext is refused above.
        //
        // This proves the mechanism holds for this purpose; it does not, and
        // is not meant to, say anything about the yt-dlp child process media
        // import actually fetches through -- that process's own networking
        // does not run through this crate at all. See
        // `apps/desktop/src-tauri/src/media.rs`'s module doc.
        const MEDIA_IMPORT: RequestPurpose = RequestPurpose::UserTriggeredMediaImport;
        let err = next_hop(
            "https://cdn.example.com/clip.mp4",
            "http://169.254.169.254/latest/meta-data/",
            MEDIA_IMPORT,
        )
        .unwrap_err();
        assert!(
            matches!(err, PolicyError::RedirectNotPermitted { .. }),
            "{err}"
        );
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

    // --------------------------------------------------------- download bounds
    //
    // The first tests for `PolicyError::DownloadTooLarge`, which had none at
    // all: the bound was written, shipped, and used by one caller, and nothing
    // had ever exercised it.
    //
    // Served over loopback plaintext, so `SELF_HOSTED` is the purpose -- it is
    // the only one `allows_plaintext_private`. Which purpose validated the URL
    // has nothing to do with the mechanism under test; `DOWNLOAD` is the
    // purpose the real callers pass and it cannot reach a loopback server at
    // all, which is asserted separately above.

    /// A body of `size` bytes, and whether to declare its length.
    ///
    /// Two shapes, because the bound has two checks and only one of them can
    /// run at a time. `Declared` exercises the `content-length` pre-check;
    /// `Undeclared` omits the header, so `Option::zip` yields `None`, the
    /// pre-check is skipped, and the running total in the streaming loop is
    /// what has to refuse. Undeclared is not a contrived shape: a chunked
    /// response has no `content-length` either.
    async fn serve_body(size: usize, declare: bool) -> std::net::SocketAddr {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await;

            // Without `Content-Length`, `Connection: close` is what tells the
            // client the body ended -- it reads to EOF.
            let header = if declare {
                format!("HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n")
            } else {
                "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_owned()
            };
            let _ = socket.write_all(header.as_bytes()).await;
            // Written in chunks so the streaming check meets more than one.
            let chunk = vec![b'x'; 4096];
            let mut sent = 0;
            while sent < size {
                let take = std::cmp::min(chunk.len(), size - sent);
                if socket.write_all(&chunk[..take]).await.is_err() {
                    break;
                }
                sent += take;
            }
            let _ = socket.shutdown().await;
        });

        addr
    }

    #[tokio::test]
    async fn a_declared_length_over_the_bound_is_refused_before_the_file_is_created() {
        let addr = serve_body(64 * 1024, true).await;
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("asset");
        let policy = NetworkPolicy::new().unwrap();
        let mut seen = |_written: u64, _total: Option<u64>| {};

        let err = policy
            .download_to_file_bounded(
                SELF_HOSTED,
                &format!("http://{addr}/asset"),
                &destination,
                1024,
                &mut seen,
            )
            .await
            .unwrap_err();

        assert!(
            matches!(err, PolicyError::DownloadTooLarge { limit: 1024 }),
            "{err}"
        );
        assert!(
            !destination.exists(),
            "the content-length check runs before the file is created, so nothing should exist"
        );
    }

    #[tokio::test]
    async fn an_undeclared_body_over_the_bound_is_refused_mid_stream_and_the_partial_is_deleted() {
        // The check that actually guarantees the bound. With no
        // `content-length` the pre-check above cannot run at all, so this is
        // the whole protection -- and it is the path that used to `return`
        // after creating and partially filling the file, leaving a truncated
        // download on disk with no `remove_file`.
        let addr = serve_body(64 * 1024, false).await;
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("asset");
        let policy = NetworkPolicy::new().unwrap();
        let mut high_water = 0_u64;
        let mut seen = |written: u64, _total: Option<u64>| high_water = written;

        let err = policy
            .download_to_file_bounded(
                SELF_HOSTED,
                &format!("http://{addr}/asset"),
                &destination,
                8192,
                &mut seen,
            )
            .await
            .unwrap_err();

        assert!(
            matches!(err, PolicyError::DownloadTooLarge { limit: 8192 }),
            "{err}"
        );
        assert!(
            !destination.exists(),
            "a refused download must not leave a partial file behind"
        );
        assert!(
            high_water <= 8192,
            "the bound is on what reaches disk: {high_water} bytes were written"
        );
        // Not vacuous: a partial file really did exist before the refusal, so
        // the assertion above is about a `remove_file` that had work to do
        // rather than about a file that was never created.
        assert!(
            high_water > 0,
            "nothing was written at all, so this proves nothing about cleanup"
        );
    }

    #[tokio::test]
    async fn a_body_that_exactly_matches_its_bound_is_not_refused() {
        // A bound that refuses a legitimate install is worse than no bound, and
        // the real callers pass an *exact* pinned length -- `Asset::size_bytes`
        // and `SherpaModel::expected_size_bytes` -- so "exactly the limit" is
        // the normal case, not an edge one. Asserted for both shapes, because
        // the two checks are separate comparisons.
        for declare in [true, false] {
            let addr = serve_body(16 * 1024, declare).await;
            let dir = tempfile::tempdir().unwrap();
            let destination = dir.path().join("asset");
            let policy = NetworkPolicy::new().unwrap();
            let mut seen = |_written: u64, _total: Option<u64>| {};

            let download = policy
                .download_to_file_bounded(
                    SELF_HOSTED,
                    &format!("http://{addr}/asset"),
                    &destination,
                    16 * 1024,
                    &mut seen,
                )
                .await
                .unwrap_or_else(|err| panic!("declare={declare}: {err}"));

            assert_eq!(download.bytes, 16 * 1024, "declare={declare}");
            assert_eq!(
                std::fs::metadata(&destination).unwrap().len(),
                16 * 1024,
                "declare={declare}"
            );
        }
    }

    #[tokio::test]
    async fn an_unbounded_download_still_takes_a_body_of_any_length() {
        // `download_to_file` is `download_to_file_inner` with `None`, and the
        // saturating arithmetic in the streaming check must not turn that into
        // a refusal.
        let addr = serve_body(64 * 1024, true).await;
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("asset");
        let policy = NetworkPolicy::new().unwrap();
        let mut seen = |_written: u64, _total: Option<u64>| {};

        let download = policy
            .download_to_file(
                SELF_HOSTED,
                &format!("http://{addr}/asset"),
                &destination,
                &mut seen,
            )
            .await
            .unwrap();
        assert_eq!(download.bytes, 64 * 1024);
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
    fn cgnat_and_this_network_widen_what_self_hosted_may_reach_in_cleartext_too() {
        // Adding CGNAT and the rest of 0.0.0.0/8 to `is_private_host` does not
        // only restrict every other purpose over https/wss -- it also widens
        // what the self-hosted purpose may reach over plain HTTP. Before that
        // addition, both of these came back `PlaintextNonPrivate`, because
        // neither address was private under the old, narrower predicate; now
        // that they are private, the self-hosted purpose's existing plaintext
        // exemption covers them too. That is the intended shape of the
        // ruling, and this pins it as a stated property of the change rather
        // than a side effect nobody wrote down.
        assert!(validate("http://100.64.0.1/v1", SELF_HOSTED).is_ok());
        assert!(validate("http://0.1.2.3/v1", SELF_HOSTED).is_ok());
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
    fn a_trailing_dns_root_dot_does_not_evade_the_private_or_public_classification() {
        // `example.com.` and `example.com` resolve identically -- the trailing
        // dot is the DNS root, not part of the name -- so it must not change
        // the answer. Before this was fixed, a trailing dot defeated every
        // branch in `is_private_host` at once: not equal to "localhost", not
        // ending in ".local", not a valid IP literal, and still containing a
        // "." so the single-label fallback never fired either. That made
        // `https://localhost./` (and `https://nas.local./`, `https://
        // whisper-box./`) read as a public host under every purpose.
        for private in [
            "localhost.",
            "nas.local.",
            "whisper-box.",
            "db.internal.",
            // More than one trailing dot is not valid DNS syntax and such a
            // name is unresolvable, but the strip has to remove the whole
            // run rather than exactly one -- a single `strip_suffix` would
            // leave "localhost." un-stripped here and misclassify it public.
            "localhost..",
            "whisper-box..",
        ] {
            assert!(is_private_host(private), "{private} should be private");
        }

        for public in ["api.openai.com.", "example.com."] {
            assert!(!is_private_host(public), "{public} should not be private");
        }
    }

    #[test]
    fn ipv4_embedded_ipv6_addresses_are_recognised_as_private() {
        // `::ffff:a.b.c.d` (IPv4-mapped) and `::a.b.c.d` (IPv4-compatible,
        // deprecated but still a valid embedding) are both `a.b.c.d` wearing
        // an IPv6 costume. `is_loopback`, `is_unspecified` and the
        // unique-local/link-local bitmasks all miss both forms -- none of
        // them look at the embedded IPv4 address -- so this is a distinct
        // check, not a stronger version of the existing ones.
        for private in [
            "::ffff:127.0.0.1",       // mapped, loopback
            "[::ffff:127.0.0.1]",     // bracketed, as `Url::host_str` returns it
            "::ffff:169.254.169.254", // mapped, the cloud-metadata address specifically
            "::ffff:10.0.0.5",        // mapped, RFC 1918
            "::ffff:192.168.1.1",     // mapped, RFC 1918
            "::ffff:0.0.0.0",         // mapped, unspecified
            "::127.0.0.1",            // compatible, loopback
            "::169.254.169.254",      // compatible, the cloud-metadata address
            "::10.0.0.5",             // compatible, RFC 1918
        ] {
            assert!(is_private_host(private), "{private} should be private");
        }

        for public in ["::ffff:8.8.8.8", "::ffff:1.1.1.1", "::8.8.8.8"] {
            assert!(!is_private_host(public), "{public} should not be private");
        }

        // `::` and `::1` both have the 96-bit zero prefix `to_ipv4` looks for,
        // which would read them as `0.0.0.0` and `0.0.0.1` under the
        // IPv4-compatible interpretation -- but they are natively IPv6
        // unspecified and loopback, and must resolve as such, not be
        // reinterpreted as IPv4 hosts. Both are private either way, so this
        // pins the *reason*, not just the outcome.
        assert!(is_private_host("::"));
        assert!(is_private_host("::1"));
    }

    /// Every other RFC-defined way to fold an IPv4 address into 128 bits.
    ///
    /// Closed as a family rather than a prefix at a time, which is the decision
    /// KNEG-93 recorded: each of these had been noticed separately and accepted
    /// on the same reasoning -- that the prefix is unrouted on an ordinary host,
    /// so a `connect()` fails rather than arriving. That is a claim about the
    /// host's configuration, and it stops holding the moment a machine has 6to4
    /// or a NAT64 resolver.
    ///
    /// The addresses below are written out in hex rather than in the friendlier
    /// dotted forms on purpose. Two of these layouts are easy to get wrong in a
    /// way that still parses -- the NAT64 /48 split around the reserved octet,
    /// and Teredo's XOR-obfuscated client -- so the test states the bits and the
    /// implementation has to agree with them, rather than both being written
    /// from the same misreading.
    #[test]
    fn the_whole_ipv4_embedding_family_is_recognised_as_private() {
        // 169.254.169.254 is a9 fe a9 fe; 10.0.0.5 is 0a 00 00 05.
        for (private, why) in [
            // RFC 2765 SIIT, `::ffff:0:0:0/96`. The `ffff` sits one segment
            // earlier than the mapped form, with a zero segment after it.
            ("::ffff:0:0:a9fe:a9fe", "IPv4-translated, cloud metadata"),
            ("::ffff:0:0:a00:5", "IPv4-translated, RFC 1918"),
            // RFC 6052 §2.1, the well-known NAT64 prefix: low 32 bits.
            ("64:ff9b::a9fe:a9fe", "NAT64 well-known, cloud metadata"),
            ("64:ff9b::7f00:1", "NAT64 well-known, loopback"),
            // RFC 8215's local-use prefix, laid out by RFC 6052 §2.2: for a
            // /48 the address is split around the reserved u-octet, sixteen
            // bits at 48..64 and sixteen more at 72..88. Reading four
            // contiguous octets instead would decode a different address than
            // the gateway will.
            (
                "64:ff9b:1:a9fe:a9:fe00::",
                "NAT64 local-use /48, cloud metadata",
            ),
            ("64:ff9b:1:a00:0:500::", "NAT64 local-use /48, RFC 1918"),
            // RFC 3056 6to4: the tunnel endpoint's IPv4 at bits 16..48.
            ("2002:a9fe:a9fe::", "6to4, cloud metadata"),
            ("2002:a00:5::1", "6to4, RFC 1918, with a host part"),
            ("2002:7f00:1::", "6to4, loopback"),
            // RFC 4380 Teredo: the server in the clear at bits 32..64, the
            // client at bits 96..128 XORed with all-ones. Either being private
            // is a reason to refuse, because a packet involves both.
            ("2001:0:a9fe:a9fe:0:0:34ff:8ef6", "Teredo, private server"),
            (
                "2001:0:cb00:7101:0:0:f5ff:fffe",
                "Teredo, private client (10.0.0.1)",
            ),
        ] {
            assert!(
                is_private_host(private),
                "{private} should be private -- {why}"
            );
        }

        for (public, why) in [
            ("::ffff:0:0:808:808", "IPv4-translated, 8.8.8.8"),
            ("64:ff9b::808:808", "NAT64 well-known, 8.8.8.8"),
            ("64:ff9b:1:808:0:800::", "NAT64 local-use /48, 8.8.8.8"),
            ("2002:808:808::", "6to4, 8.8.8.8"),
            (
                "2001:0:cb00:7101:0:0:f7ff:f7f7",
                "Teredo, both halves public",
            ),
            // Not Teredo: the second segment is non-zero, so `2001::/16` at
            // large must not be swept up. Google's public resolver and the
            // documentation range both live there.
            (
                "2001:4860:4860::8888",
                "2001:4860::/32 is not the Teredo prefix",
            ),
            ("2001:db8::1", "the documentation range is not Teredo"),
            ("2606:4700:4700::1111", "an ordinary public IPv6 address"),
        ] {
            assert!(
                !is_private_host(public),
                "{public} should not be private -- {why}"
            );
        }
    }

    /// The decoded address is the one the packet reaches, not merely *an*
    /// address.
    ///
    /// `the_whole_ipv4_embedding_family_is_recognised_as_private` would pass on
    /// a layout that decoded the wrong four octets, as long as those octets
    /// happened to be private too -- and in a NAT64 /48 or a Teredo address
    /// there are plenty of zero octets nearby to decode by accident. This pins
    /// the exact value.
    #[test]
    fn each_embedding_decodes_the_address_the_packet_would_reach() {
        use std::net::{Ipv4Addr, Ipv6Addr};

        let metadata = Ipv4Addr::new(169, 254, 169, 254);
        let decoded = |literal: &str| -> Vec<Ipv4Addr> {
            literal
                .parse::<Ipv6Addr>()
                .map(embedded_ipv4s)
                .unwrap_or([None, None])
                .into_iter()
                .flatten()
                .collect()
        };

        assert_eq!(decoded("::ffff:a9fe:a9fe"), vec![metadata], "IPv4-mapped");
        assert_eq!(decoded("::a9fe:a9fe"), vec![metadata], "IPv4-compatible");
        assert_eq!(
            decoded("::ffff:0:0:a9fe:a9fe"),
            vec![metadata],
            "IPv4-translated"
        );
        assert_eq!(
            decoded("64:ff9b::a9fe:a9fe"),
            vec![metadata],
            "NAT64 well-known"
        );
        assert_eq!(
            decoded("64:ff9b:1:a9fe:a9:fe00::"),
            vec![metadata],
            "NAT64 local-use"
        );
        assert_eq!(decoded("2002:a9fe:a9fe::"), vec![metadata], "6to4");
        assert_eq!(
            decoded("2001:0:a9fe:a9fe:0:0:34ff:8ef6"),
            vec![metadata, Ipv4Addr::new(203, 0, 113, 9)],
            "Teredo carries two: the server in the clear, the client XORed"
        );

        // An address in none of the families decodes to nothing, rather than
        // to whatever its low 32 bits happen to be.
        assert!(
            decoded("2606:4700:4700::1111").is_empty(),
            "an ordinary address"
        );
        assert!(
            decoded("fe80::1").is_empty(),
            "link-local is native, not an embedding"
        );
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
    fn cgnat_and_the_whole_this_network_range_are_private() {
        // Both are standard SSRF targets that the standard-library predicates
        // do not cover: `is_unspecified` only matches the single address
        // `0.0.0.0`, not the rest of the reserved `0.0.0.0/8` block, and
        // nothing in `std` knows about `100.64.0.0/10` at all -- it postdates
        // the RFC 1918 ranges `is_private` checks. The self-hosted purpose
        // stays exempt, so a deliberately configured CGNAT address still
        // works; refusing it for every other purpose costs nothing real.
        for private in [
            "0.0.0.1",
            "0.1.2.3",
            "0.255.255.255",
            "100.64.0.0",
            "100.64.0.1",
            "100.100.100.100",
            "100.127.255.255",
        ] {
            assert!(is_private_host(private), "{private} should be private");
        }

        // Just outside the CGNAT range on either side.
        for public in ["100.63.255.255", "100.128.0.0", "99.64.0.1", "101.64.0.1"] {
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
            // A trailing DNS-root dot, which used to defeat every branch of
            // `is_private_host` at once.
            "https://localhost./",
            "https://nas.local./",
            // Decimal, octal and hex IPv4 -- normalised to dotted-quad by URL
            // parsing before `is_private_host` ever runs.
            "https://2130706433/", // 127.0.0.1 as a 32-bit integer
            "https://0177.0.0.1/", // 127 in octal
            "https://0x7f.0.0.1/", // 127 in hex
            // IPv4-mapped and IPv4-compatible IPv6.
            "https://[::ffff:127.0.0.1]/",
            "https://[::ffff:169.254.169.254]/",
            "https://[::127.0.0.1]/",
            // CGNAT and the rest of 0.0.0.0/8.
            "https://100.64.0.1/",
            "https://0.0.0.1/",
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

    /// A configured host with a trailing dot still matches.
    ///
    /// `is_blocked` strips the DNS root dot from its *input*; until KNEG-93 the
    /// loader did not strip it from the configured entries, so the two sides
    /// normalised differently and an entry written with a trailing dot would
    /// have matched nothing at all. No entry has one today -- every line in
    /// `config/forbidden-domains.json` is written by this project -- so this
    /// pins the symmetry rather than a live hole, which is the point: the next
    /// person to edit that file should not have to know.
    #[test]
    fn a_configured_host_is_normalised_the_same_way_an_incoming_one_is() {
        for host in blocked_hosts() {
            assert!(
                !host.ends_with('.'),
                "{host} keeps a trailing dot, which no incoming host ever has"
            );
            assert_eq!(
                *host,
                host.to_ascii_lowercase(),
                "{host} is not lowercased, so a lowercased input cannot match it"
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
        //
        // Looped across every purpose, exactly like
        // `https_to_a_private_host_is_refused_except_for_self_hosted`, rather
        // than pinned against one. The "wss" arm is a hand-written copy of the
        // "https" arm's guard rather than shared code, so the two can diverge
        // silently; a loop across `ALL_PURPOSES` is what would catch that
        // divergence if a future edit touched one arm and not the other.
        for private in [
            "wss://127.0.0.1/stream",
            "wss://[::1]/stream",
            "wss://169.254.169.254/stream",
            "wss://[::ffff:127.0.0.1]/stream",
            "wss://[::127.0.0.1]/stream",
            "wss://localhost./stream",
            "wss://100.64.0.1/stream",
        ] {
            for purpose in ALL_PURPOSES {
                if purpose == SELF_HOSTED {
                    continue;
                }
                assert!(
                    matches!(
                        validate_websocket(private, purpose),
                        Err(PolicyError::PrivateHttpsTarget { .. })
                    ),
                    "{private} should be refused over wss for {purpose:?}"
                );
            }
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
