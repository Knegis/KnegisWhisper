# Network Boundary

KnegisWhisper is local-first. It has no hosted backend, no account, no license check, no
telemetry, and no vendor API key. **Nothing is sent anywhere unless the user configured it or
asked for it.**

## The rule

Every outbound request must declare exactly one purpose. There is no default and no unlabelled
path.

| Purpose | Example |
| --- | --- |
| `user-configured-ai-provider` | A reasoning call to the provider whose key the user entered. |
| `user-configured-transcription-provider` | A transcription call to a user-configured endpoint. |
| `user-configured-calendar-provider` | Google or Microsoft Graph, with the user's own OAuth grant. |
| `user-triggered-model-download` | Fetching a GGUF or GGML the user chose to install. |
| `user-triggered-media-import` | yt-dlp fetching a URL the user pasted. |
| `own-github-releases-updater` | Optional update check against this project's own releases. |
| `explicitly-configured-self-hosted-endpoint` | A self-hosted transcription or inference server the user pointed us at. |

The list is mirrored in `config/forbidden-domains.json` under `allowedEgressPurposes` and in the
`RequestPurpose` enum in `crates/network-policy`.

## How it is enforced

**Structurally, not by review.** `crates/network-policy` owns the only `reqwest::Client` in the
workspace, and constructing a request requires a `RequestPurpose`. A call site that cannot name
its purpose does not compile.

**The frontend gets no network capability at all.** No `http:` permission in any Tauri
capability file, and a strict CSP. React components cannot make outbound requests; they ask Rust,
which decides. This is a deliberate inversion of Pluely, which ships `"csp": null` and a
capability granting `http:default` over `http://**` and `https://**`.

**No startup request.** Launching the application performs no network I/O. The update check, if
enabled, is user-initiated or scheduled — never a condition of starting.

**No hidden fallback.** If a provider fails, the operation fails. Nothing is silently retried
against a different provider, and no local failure escalates to a remote service. Ambiguous
provider dispatch is an error, not a guess (see `PORTING_MATRIX.md`).

## The scanner

`config/forbidden-domains.json` drives a scanner that fails the build if a forbidden host or
identifier appears outside an allowlisted path.

**Scanned:** Rust, TypeScript, JavaScript, `tauri.conf.json`, capability files, `Cargo.toml`,
`package.json`, `.env.example`, `config/`, sidecar definitions, **built frontend assets**, and
**`.github/workflows/`**.

Those last two are in scope because of a real defect found in the upstream audit: OpenWhispr's
CI workflows still default `VITE_AUTH_URL` to `https://auth.openwhispr.com`, and the main
process's `getAuthUrl()` has no guard against it. A `src/`-only scan sees nothing; a CI-built
binary carries the vendor host. **The scanner ships with a negative test asserting it catches
exactly that pattern**, so we know it is looking where it claims to.

The scanner also blocks two build-time couplings that no host blocklist would catch:
`OpenWhispr/whisper.cpp` and `OpenWhispr/openwhispr/releases`. Depending on a vendor's GitHub
org for binaries is a vendor dependency even though no vendor domain appears in the code.

**Allowlisted paths:** `docs/**`, `NOTICE`, `LICENSE*`, the three `config/*.json` files that must
name vendor services in order to declare them removed, and test fixtures. Documenting what was
removed is not the same as depending on it.

## Third-party hosts we do talk to

Only with the user's own credentials, and only for the purposes above. Grouped as the audit found
them:

**Inference and transcription** — `api.openai.com`, `api.anthropic.com`,
`generativelanguage.googleapis.com`, `api.groq.com`, `api.berget.ai`, `openrouter.ai`,
`api.x.ai`, `api.mistral.ai`, `api.{eu,us}.corti.app`, `inference.tinfoil.sh` (plus the Tinfoil
attestation hosts), `streaming.assemblyai.com`, `api.deepgram.com`, Azure OpenAI deployments,
AWS Bedrock, and Google Vertex AI.

**Model and binary downloads** — `huggingface.co`, `github.com` release assets.

**Update check** — `api.github.com`, for this project's own release listing
only, and only when a person explicitly asks (`updater.rs`). Never on
startup, never on a timer.

**Calendars** — `accounts.google.com`, `oauth2.googleapis.com`, `www.googleapis.com`,
`login.microsoftonline.com`, `graph.microsoft.com`.

**Media import** — whatever host the user's pasted URL resolves to, subject to the SSRF guards
below.

## SSRF guards for URL import

Ported from `urlAudioDownloader.js` and non-negotiable: HTTPS only; DNS resolution followed by
rejection of private and link-local addresses; a 500 MB download cap; at most 3 redirects; a
6-hour duration cap; and a 30-second stall timeout.

## The local API is inbound, and not egress either

`crates/local-api` *listens*; it does not send. It is in this document anyway
because it is the other half of the same boundary — the surface through which
this application's data can leave the process.

Three properties hold it:

- **Loopback only.** `serve` takes an already-bound listener and refuses one
  whose address is not a loopback address. There is no configuration option to
  widen it. Upstream binds `127.0.0.1` and then checks each request's remote
  address as a second line of defence; here a non-loopback listener never reaches
  the router, so a future edit that widens the bind fails at startup rather than
  quietly exposing everything.
- **Authenticated with a fresh token.** 256 bits from the operating system's
  randomness, per run, compared in constant time. Never derived from a machine
  identifier — that would be the device fingerprinting the brief forbids, and it
  would be guessable.
- **Discoverable only through a file whose permissions are read back.**
  `~/.knegiswhisper/local-api.json` is `0600` on Unix, verified rather than
  assumed. Windows has no `chmod`, so it reports what it actually has — the
  user's account and administrators, inherited from the profile directory —
  rather than claiming owner-only.

An unauthenticated request gets `401` whatever the path, including paths that do
not exist, so the surface cannot be mapped by anyone who can reach the port.

## Loopback is not egress

Talking to a sidecar this application started — `whisper-server`, and later `llama-server` — is
not one of the seven purposes, because nothing leaves the machine. `NetworkPolicy::loopback`
takes a **port and a path, never a URL**, and builds the address itself. There is no parameter
that could aim it at another host and no way for a later change to widen it quietly, which is
what makes "not egress" a property of the signature rather than a claim in a comment. The path is
parsed rather than trusted, so one carrying a scheme or an authority is refused.

The sidecars themselves are bound to `127.0.0.1` by `kw_sidecar::port`, which also takes no
address, and `whisper-server` is passed `--host 127.0.0.1` explicitly. These servers have no
authentication of any kind; a bind to `0.0.0.0` would be an open transcription endpoint on the
network.

## Redirects are followed, one validated hop at a time

The client is built with `redirect::Policy::none()`, because a silently followed redirect is how
an allowed host becomes a disallowed one. But downloads genuinely need redirects: a GitHub
release asset answers with a 302 to a storage host, so refusing to follow means refusing to
download.

So they are followed *visibly*. Every hop is validated against the same purpose before it is
requested, the chain is bounded at five, and the hops taken are returned so a report can say
where the bytes actually came from rather than implying they came from the URL that was asked
for. A redirect to plaintext, to a blocked vendor host, to a private or link-local address (over
either scheme, since the fix described under "Plaintext HTTP, and HTTPS to a private address"),
or to a scheme the purpose does not allow is an error naming both ends.

## What `user-triggered-model-download` covers

Both the model and the runtime that executes it. Installing local transcription is one action
from the user's point of view, and it needs a `whisper-server` binary as well as a GGML model
file; splitting them would say the boundary is about file types rather than about reasons, and
the seven-purpose count is a stated constraint.

The executable half is more dangerous, and that is answered by a stronger requirement rather than
a separate purpose: `kw_sidecar::Asset` cannot be constructed without a pinned SHA-256, so a
binary only ever reaches this purpose when its exact bytes were decided at review time. Models
are held to a weaker check — size, with a tolerance, unless a digest happens to be known — and
which check applied is reported to the caller rather than implied, because wrong weights produce
bad transcripts while wrong code produces a compromised machine.

## Plaintext HTTP, and HTTPS to a private address

Only one purpose may reach a private, loopback or link-local host at all:
`explicitly-configured-self-hosted-endpoint`. Private means, for an IPv4 address: loopback, the
RFC 1918 ranges, link-local (`169.254.0.0/16`, which includes the `169.254.169.254` cloud-metadata
address with nothing metadata-specific required to catch it), `0.0.0.0/8` in full, and
`100.64.0.0/10` (the shared address space carrier-grade NAT uses, and a standard SSRF target). For
an IPv6 address: loopback (`::1`), unspecified (`::`), unique-local (`fc00::/7`), and link-local
(`fe80::/10`) — native IPv6 ranges, with no "mapped" IPv4 form, unlike the next case. And
separately: any of the IPv4 cases above **embedded in an IPv6 literal**, either as
`::ffff:a.b.c.d` (IPv4-mapped) or as `::a.b.c.d` (IPv4-compatible, deprecated but still decodable)
— `::ffff:127.0.0.1` and `::127.0.0.1` are both `127.0.0.1` wearing an IPv6 costume, and this
predicate looks through the costume rather than treating it as a separate, IPv6-native address.
`.local`, `.internal`, `.home.arpa`, and single-label hostnames count as private too, and a
trailing DNS-root dot (`localhost.`, `nas.local.`) is stripped before any of these checks run, so
it changes none of their answers. Everything else is not private, and no purpose other than the
self-hosted one gets the exemption at all. That purpose may reach a private host over plain HTTP
**or** over HTTPS; every other purpose is refused a private target under either scheme. Tests
assert all of the above, one case per form.

The exemption covers the whole private range rather than only loopback because a model server on
another machine on the user's own network is an ordinary setup, and refusing it would mean the
only way to use one is to put a certificate on it. That is a real trade — audio and any
configured credential cross the local network in the clear when the endpoint is plain HTTP — and
it belongs to the user, for an endpoint they typed in themselves. The same reasoning is why CGNAT
was added to the private set rather than left alone: the self-hosted purpose stays free to reach
one deliberately, so widening what every other purpose is refused costs nothing real.

**The purpose is a property of the destination, not the feature.** A self-hosted whisper server
is reached with `explicitly-configured-self-hosted-endpoint`, not
`user-configured-transcription-provider`, even though the request is a transcription. Getting
that backwards is not cosmetic: it was a real bug, caught by a test, in which a local whisper or
llama.cpp server could never be reached because the policy layer refused the scheme before
sending, and the error blamed the scheme rather than the configuration.

`is_private_host` lives in `crates/network-policy` and route resolution delegates to it, so the
security layer and the routing layer cannot disagree about what counts as private. They did
disagree once — route resolution accepted the private range while the policy layer accepted only
loopback — which meant an endpoint the settings screen accepted was refused at send time.

### Three corrections this document did not previously mention

**1. The HTTPS and WSS arms ran no host check at all.** Until this was fixed, `validate`'s
`https` arm ran no host check whatsoever, for any purpose: `https://127.0.0.1/`,
`https://169.254.169.254/` and `https://[::1]/` were all accepted for every one of the seven
purposes, and because redirects are re-validated through that same function (see below), a
redirect to one of those addresses was *followed*, not refused. A valid TLS certificate says
nothing about whether a host sits on the public internet or on loopback, and the code was
treating "https" as proof of the latter. `wss` had the identical gap in `validate_websocket`.
Both arms now run the same `is_private_host` check the `http`/`ws` arms already ran, gated by the
same one purpose. This closes the gap without widening or narrowing what a self-hosted endpoint
may do: it could already reach a private host in the clear, so allowing it to do the same over
TLS is not a new exposure, and every other purpose loses only a capability nothing legitimate was
using — every call site that reaches a private address at all already does so under
`explicitly-configured-self-hosted-endpoint`.

**2. `is_private_host` itself missed IPv4-embedded IPv6 and the trailing DNS-root dot.**
These are bugs in the predicate, independent of finding 1: `is_private_host` is also consulted by
the plaintext arms and by `transcription-core`'s route-resolution layer, so both misses were live
before the HTTPS/WSS fix, just reachable only through the plaintext exemption. `::ffff:127.0.0.1`
and `::127.0.0.1` read as ordinary, non-loopback, non-unique-local, non-link-local IPv6 addresses
under the old logic, because none of `is_loopback`, `is_unspecified`, or the unique-local/
link-local bitmasks look at an embedded IPv4 address. And a trailing dot — `localhost.` —
defeated every branch in the function simultaneously: not equal to `"localhost"`, not ending in
`.local`, not a valid IP literal, and still containing a `.` so the single-label fallback never
fired either. The same trailing-dot miss existed in the vendor blocklist check (`is_blocked`):
`openwhispr.com.` matched neither the exact-match nor the suffix-match branch, so it was not
blocked. Both are fixed by normalising the trailing dot away before any comparison runs.

**3. Before finding 1, a predicate miss meant fail-closed; after it, the same miss means
fail-open.** This is why 2 was worth fixing at all, and why any future gap in `is_private_host`
now matters more than it used to. Before the HTTPS/WSS fix, a false negative in `is_private_host`
meant a plaintext request to that address was refused (the plaintext arm only *allows* what the
predicate says is private, so a miss loses permission, not gains it). After the fix, the same
false negative means an HTTPS or WSS request to that address is *allowed*, because the private
check is what refuses it. `is_private_host` did not change what it was for; what changed is that
a gap in it now bypasses a control rather than only breaking a convenience.

**What none of this stops: DNS rebinding.** The check runs against the literal host in the URL at
validation time, before any connection is attempted. It has no view of DNS. A hostname that
resolves to a public address when validated and to `127.0.0.1` or a link-local address when
`reqwest` actually connects — "DNS rebinding" — passes this check and is not caught by anything
else in this crate. Closing that would need a check against the address actually dialled, at
connect time, which is a materially different and harder problem than the ones the corrections
above address, and is not implemented here.

## Self-hosted and no-auth endpoints

Users may configure endpoints requiring bearer tokens, Azure `api-key` headers, or no
authentication at all. A self-hosted route is emitted with `auth: { scheme: "none" }` and
`sizeCapBytes: null`, and it takes precedence over a stale local-transcription preference. It must
still pass endpoint validation — the check that a "custom" endpoint has not been left at the
default OpenAI base URL is load-bearing, because without it the user's key and audio would go to
OpenAI instead of their own server.
