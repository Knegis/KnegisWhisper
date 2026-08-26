# Security

## Reporting

Open a private security advisory on the GitHub repository. Please do not file a public issue for
an unfixed vulnerability.

## Threat model

KnegisWhisper handles microphone audio, system audio, meeting transcripts, notes, and provider
credentials — all on the user's machine. The assets worth protecting are those credentials and
that content.

**In scope:** credential disclosure, unintended network egress, a malicious webview escaping its
capability set, path traversal through import or export, SSRF through URL import, and
unauthenticated access to the local API.

**Out of scope:** an attacker with administrator rights on the machine, or with the user's
unlocked OS credential store. We do not attempt to defend against the machine's owner, and we do
not hide from them.

## Credentials

Secrets live in the OS credential store — macOS Keychain, Windows Credential Manager, Linux
Secret Service — one entry per key under the service name `com.knegis.knegiswhisper`. Never in
SQLite, never in `localStorage`, never in a `.env` file.

On Linux without a Secret Service provider, the app reports the limitation and requires the user
to opt into a documented fallback rather than silently degrading to a weaker store.

Routes carry a **key reference**, not a key. Route resolution is a pure function with no access
to credentials at all; only `crates/security` can resolve a reference, and only
`crates/network-policy` can transmit the result. A test never needs a real credential.

Calendar OAuth refresh tokens go to the credential store too. Upstream keeps them in plaintext
SQLite columns; the importer relocates them and leaves a reference behind. Google's flow moves to
a **PKCE-only installed-app public client** — upstream ships a `client_secret` in the app
environment, which is not a secret in a distributed binary.

## Network

Every request declares a purpose; there is no unlabelled path. The webview has no HTTP capability
and no permission to reach arbitrary hosts. There is no startup request, no telemetry, and no
hidden fallback: a provider failure fails the operation rather than quietly trying somewhere else.
See `NETWORK_BOUNDARY.md`.

URL import keeps upstream's SSRF guards: HTTPS only, DNS resolution followed by private and
link-local address rejection, a 500 MB cap, at most 3 redirects, a 6-hour duration cap, and a
30-second stall timeout.

## Local API

Loopback only — never `0.0.0.0`, and not on the LAN by default. A 32-byte random bearer token
per start, compared with a constant-time comparison after a length check. Non-loopback peer
addresses are rejected even if the token matches. Request bodies are capped.

The discovery file carries the port and token and is deleted on shutdown. Its permissions differ
by platform, and this document used to claim `0600` everywhere, which was false:

- **Unix:** `0600`, set and then **read back** — some filesystems ignore mode bits, and a token in
  a world-readable file is worth knowing about before it is used. The containing directory is
  `0700`.
- **Windows:** *nothing is applied.* The file inherits whatever ACL the profile directory carries,
  and `Permissions::InheritedFromProfile` reports that rather than claiming owner-only. **That is
  an honest report of an unmet requirement, not a mechanism.** Measured on one real machine
  (2026-08-26) the inherited ACL granted Modify to a local group that was neither the user nor the
  administrators — see [BENCH_CHECK.md](BENCH_CHECK.md) — so on Windows, treat the token as
  readable by anyone the profile directory lets in. Closing it means an explicit owner-only DACL
  through `SetNamedSecurityInfo`, with the same read-back discipline the Unix path already uses.

The MCP server talks to the desktop application through this authenticated local API. Streamable
HTTP is available but disabled by default; stdio is the normal transport. Destructive MCP tools
require an explicit confirmation field.

## Webview

Strict CSP, no `unsafe-eval`. Capabilities are per-window and least-privilege: no shell, no
arbitrary filesystem access, no process spawning, no direct database or credential access, no
HTTP. Command payloads are typed and validated at the boundary; paths and URLs are validated
before use.

## Sidecars

Explicitly allowlisted; the frontend cannot spawn a process. Downloads are pinned by version and
verified by digest. Temporary files are created with restrictive permissions, and temporary audio
is deleted automatically.

## Logging

Never logged: API keys, OAuth tokens, local API bearer tokens, raw clipboard contents, raw audio,
credential file contents, or full transcripts by default. Secrets are redacted at the logging
boundary rather than at each call site, so a new call site cannot forget.

## Recording indication

When recording is active the user gets a visible indicator. The application does not record
silently, and it will not conceal recording from the person using it — a design constraint, not a
setting.

## What we deliberately do not do

No process hiding, no anti-forensics, no security-software evasion, no attempt to bypass
administrator controls, and no attempt to hide recording from the local user.

The privacy overlay is excluded from screen capture on a best-effort basis where the OS supports
it, and `PLATFORM_SUPPORT.md` reports honestly where it does not. We do not describe the
application as undetectable.
