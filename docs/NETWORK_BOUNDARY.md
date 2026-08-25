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
for. A redirect to plaintext, to a blocked vendor host, or to a scheme the purpose does not allow
is an error naming both ends.

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

## Plaintext HTTP

HTTPS always, with one exemption: `explicitly-configured-self-hosted-endpoint` may use plain
HTTP **to a private address**. Loopback, the RFC 1918 ranges, link-local, `.local`, `.internal`,
`.home.arpa`, and single-label hostnames all count as private; everything else does not, and no
other purpose gets the exemption at all. A test asserts both halves of that.

The exemption covers the whole private range rather than only loopback because a model server on
another machine on the user's own network is an ordinary setup, and refusing it would mean the
only way to use one is to put a certificate on it. That is a real trade — audio and any
configured credential cross the local network in the clear — and it belongs to the user, for an
endpoint they typed in themselves.

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

## Self-hosted and no-auth endpoints

Users may configure endpoints requiring bearer tokens, Azure `api-key` headers, or no
authentication at all. A self-hosted route is emitted with `auth: { scheme: "none" }` and
`sizeCapBytes: null`, and it takes precedence over a stale local-transcription preference. It must
still pass endpoint validation — the check that a "custom" endpoint has not been left at the
default OpenAI base URL is load-bearing, because without it the user's key and audio would go to
OpenAI instead of their own server.
