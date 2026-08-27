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

URL import checks the pasted URL once, before yt-dlp ever runs: scheme, the vendor blocklist, and a
literal private-or-link-local host, under its own purpose. Corrected by Task 19 — the previous
sentence here ("DNS resolution followed by private and link-local address rejection") described
upstream's design, not this crate's, which resolves no names at all. Fetching the URL is yt-dlp's
own job as a separate process, bounded by a 500 MB filesize cap and a bounded socket timeout it
enforces on itself, not by `kw_network_policy`. See `docs/NETWORK_BOUNDARY.md`'s "SSRF guards for
URL import" section for the full accounting, including what this does not stop (DNS rebinding).

## Updates

Optional and user-initiated only. No check runs at startup or on a timer;
`updater.rs`'s only caller is the "Check for updates" button on
Settings > Diagnostics. See `docs/NETWORK_BOUNDARY.md`.

The bundler can produce signed update artifacts
(`bundle.createUpdaterArtifacts` in `apps/desktop/src-tauri/tauri.conf.json`,
turned on), but this repository ships no keypair and generates none as part
of its own tooling or CI. A maintainer who wants signed, verifiable updates
makes one themselves:

1. From `apps/desktop`, run
   `npx tauri signer generate -w ~/.tauri/knegiswhisper.key` (any path
   outside this repository; `-p` sets a password instead of an interactive
   prompt). This writes a private key to that path and prints the matching
   public key to the terminal.
2. Paste the printed public key into `plugins.updater.pubkey` in
   `apps/desktop/src-tauri/tauri.conf.json` and commit that change. The
   public key is not a secret — it is what a client checks a downloaded
   update's signature against, the same role a TLS certificate's public half
   plays.
3. Add the private key file's contents as the repository secret
   `TAURI_SIGNING_PRIVATE_KEY` (repository Settings > Secrets and variables >
   Actions). If step 1 set a password, also add
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. `.github/workflows/release.yml`
   reads both as environment variables on its `tauri build` step and signs
   updater artifacts only when they are present; without them, that workflow
   builds unsigned bundles, which is the state this repository is in today.
4. **The private key must never be committed, and must never be generated on
   a machine this project's CI or tooling controls.** It belongs nowhere in
   this repository, in `Cargo.lock`, or in any workflow file — only in the
   repository secret and, before that, on whatever machine ran step 1.

Until a keypair exists, `plugins.updater.pubkey` stays empty, and a check
reports that state explicitly (`UpdateCheckResult::NoPublicKeyConfigured`)
rather than either extreme: checking anyway would mean trusting a downloaded
update on the strength of an HTTPS connection to GitHub alone, with nothing
that verifies the bytes came from this project's own signing key; silently
reporting nothing would look identical to "no update exists" for a reason
that has nothing to do with whether one does.

This feature does not depend on the `tauri-plugin-updater` crate. Every
request it makes stays behind `kw_network_policy::NetworkPolicy`, tagged
`RequestPurpose::OwnGithubReleasesUpdater` like every other network call in
this application, rather than through that plugin's own HTTP client — see
`crates/network-policy/src/lib.rs`'s module doc on why this crate stays the
only one that builds a `reqwest::Client`. `plugins.updater.pubkey` is still
read from the conventional location the plugin itself would use, so a
keypair generated the standard way drops in without translation.

Installing an update is not implemented. A check reports that a newer
release exists and where; nothing downloads or runs it without a further,
separate, explicit action, which is not part of this project today.

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
  readable by anyone the profile directory lets in. That fact still stands; closing it means an
  explicit owner-only DACL through `SetNamedSecurityInfoW` with `PROTECTED_DACL_SECURITY_INFORMATION`,
  then reading the DACL back and reporting `OwnerOnly` only if it matches — the discipline the Unix
  path already uses. Not done yet: `discovery::restrict_file` is still a no-op on Windows.
  **Corrected 2026-08-27:** this paragraph used to go on to say that `Permissions::is_as_expected`
  returned `true` for `InheritedFromProfile` and so `Integrations > Local API` showed no warning and
  a sentence naming trustees nothing had read — true when written, and fixed by Task 14, which is
  before this correction. The desktop layer now has its own `DiscoveryPermissions::is_confirmed()`,
  which — unlike `kw_local_api::Permissions::is_as_expected()`, which still returns `true` for
  `InheritedFromProfile` and is no longer read by anything user-facing — treats only `OwnerOnly` as
  confirmed. Both the startup log warning and the two settings screens (`Integrations > Local API`
  and `Integrations > CLI`) go through that one decision and now warn on `InheritedFromProfile`,
  rendering "Not independently restricted" with a caveat naming administrators by name. The
  unmet-requirement fact above is what that warning is about; only the claim that the application
  hid it was wrong, and only that claim has been removed here.

The MCP server talks to the desktop application through this authenticated local API. Streamable
HTTP is available but disabled by default; stdio is the normal transport. Destructive MCP tools
require an explicit confirmation field.

## Webview

Strict CSP, no `unsafe-eval`. Capabilities are per-window and least-privilege: no shell, no
arbitrary filesystem access, no process spawning, no direct database or credential access, no
HTTP. Command payloads are typed and validated at the boundary; paths and URLs are validated
before use.

**`Notes > Upload`'s file pickers are a narrower exception, worth naming precisely rather than
folding into the paragraph above.** `pick_import_file` opens a real OS dialog and returns the
chosen path *to the renderer*, which holds it in React state and passes it as a plain string to a
separate command — `media_import_audio`, `media_import_video`, or `import_openwhispr` — on a later
click. What holds: the renderer has no independent way to *obtain* a path, since there is no
filesystem or dialog capability in `capabilities/default.json`; only a real dialog produces one.
What does not hold: unlike `note_export_to_file`, which fuses picking and writing into one atomic
command specifically so the frontend never sees or supplies a path at all, these three commands
accept whatever path string they are called with and read it — nothing confirms it is the one
`pick_import_file` returned rather than one invented outright. **Concretely:** a compromised
renderer, already in this document's threat model, can call `media_import_audio` directly with an
arbitrary local path with no dialog involved; with a cloud transcription provider configured, that
file's contents then leave the machine. Not yet closed. Two hardening directions, neither chosen
yet: fuse picking and importing into one atomic command the way export already does (in tension
with `Notes > Upload`'s pick-then-review-then-confirm flow, which export's single-click save does
not need); or a capability restricting which paths these three commands will accept, independent
of whatever the renderer claims. See `docs/BENCH_CHECK.md`'s Task 12 entry.

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
