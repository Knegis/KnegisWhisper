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

**URL import is disabled.** It is the one place a user-typed URL would reach the network, and
the boundary this application can enforce stops at the URL as typed: `validate_import_url` checks
scheme, the vendor blocklist and a literal private-or-link-local host before anything is spawned,
but the fetch belongs to the `yt-dlp` sidecar — its own DNS resolution, its own redirects, and its
own extractor-generated requests, none of them observable here. DNS rebinding defeats an up-front
literal-host check by construction. Rather than ship an SSRF surface with a guard that cannot cover
it, `media_import_url` is unregistered as a command, `Notes > Upload` offers no URL field, and
`media:url-import` is `reachable: false`. Audio and video import, which take a local path and never
fetch anything, are unaffected. The full accounting, and what re-opening it requires, is in
`docs/NETWORK_BOUNDARY.md`'s "SSRF guards for URL import" section.

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
by platform, and this document used to claim `0600` everywhere, which was false. The mechanism is
`kw_security::file_acl`, shared with `kw_sidecar::pid`, and it is **applied and then read back** on
both platforms — some filesystems ignore mode bits, a DACL on a network redirector does not behave
like one in a home directory, and a token in a file whose sharing was never checked is the same
class of claim as a capture-exclusion flag reported `active` without asking the window.

- **Unix:** `0600` on the file, `0700` on the containing directory, both read back.
- **Windows:** an explicit DACL granting **the process's token user**, `NT AUTHORITY\SYSTEM` and
  `BUILTIN\Administrators`, and nobody else, applied with
  `PROTECTED_DACL_SECURITY_INFORMATION` so the profile directory's entries stop being inherited,
  and then read back entry by entry through `GetNamedSecurityInfoW`. The containing directory gets
  the same three, inheritable.

  **Not owner-only, on purpose.** An administrator can take ownership of any object and re-grant
  themselves anything, so a DACL that excludes them is theatre that breaks backup and repair
  tooling for nothing. What a confirmed Windows result claims is narrower and true: the
  *unexpected* trustees are gone and the inheritance that handed them down is off, both verified.
  The settings label says an administrator can still read the token, because they can.

  The grant is derived from `OpenProcessToken`/`TokenUser` rather than from the object's owner. On
  a machine whose policy makes `BUILTIN\Administrators` the owner of what an elevated process
  creates, an owner-derived DACL would omit the user and a later non-elevated run could not read
  its own file. Ownership is never taken and the SACL is never touched.

  **Measured on the machine that found the defect (2026-08-31).** Before, and unchanged across
  three measurements since 2026-08-26, `~/.knegiswhisper/local-api.json` inherited Modify for two
  trustees that were neither the user nor administrators — see
  [BENCH_CHECK.md](BENCH_CHECK.md) finding 2 for the `icacls` output. After a real
  `discovery::write` into a directory under `%USERPROFILE%`:

  ```
  C:\Users\AxelKarlsson\.knegiswhisper-acltest2
      AzureAD\AxelKarlsson:(OI)(CI)(F)
      NT AUTHORITY\SYSTEM:(OI)(CI)(F)
      BUILTIN\Administrators:(OI)(CI)(F)

  C:\Users\AxelKarlsson\.knegiswhisper-acltest2\local-api.json
      AzureAD\AxelKarlsson:(F)
      NT AUTHORITY\SYSTEM:(F)
      BUILTIN\Administrators:(F)
  ```

  Three trustees, no `(I)` on any of them, on both. The application can still read the file it
  wrote.

Two orderings in `discovery::write` are load-bearing. The **directory is restricted before the
temporary file is created**, so the temporary inherits a protected list and the token's bytes never
touch disk under the loose one; the empty temporary is then restricted in its own right before the
token is written into it, which closes the same window on Unix, where a `0700` directory does not
stop a `0644` file inside it. And the **read-back happens after the rename**, against the path the
caller is told about — verifying the temporary described a path that no longer existed by the time
the verdict was returned. Measured: a rename within one directory keeps the file object and its
whole security descriptor, `SE_DACL_PROTECTED` included.

The directory's verdict gates the file's (`file_acl::combine`). A perfectly restricted file inside
a directory another user may delete children of can be deleted and replaced, and a client then
reads an attacker's port and token — for this file, worse than being readable.

**The verdict is a snapshot taken when the API starts, not a live reading.** `local_api_status`
reports what the write measured; nothing re-checks the DACL while the application runs. Something
with write access to the parent can add a trustee afterwards and the screen will not say so until
the next start. Recorded as a known limitation rather than fixed here.

**Corrected 2026-08-27:** this section used to go on to say that `Permissions::is_as_expected`
returned `true` for `InheritedFromProfile` and so `Integrations > Local API` showed no warning and
a sentence naming trustees nothing had read — true when written, and fixed by Task 14. That helper
has since been deleted outright; `DiscoveryPermissions::is_confirmed()` is the one decision both
the startup warning and both settings screens go through. It draws the line at **checked versus
unchecked**, not strict versus lax: the Windows state passes because the list was read back, and
`InheritedFromProfile` — now reachable only on a target that is neither Unix nor Windows — does not,
because nothing looked.

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

## Sidecars and downloaded models

Explicitly allowlisted; the frontend cannot spawn a process. Sidecar downloads are pinned by
version and verified by digest — `kw_sidecar::manifest::Asset` cannot represent an executable
without one, so an unpinned binary does not deserialize rather than skipping verification.
Temporary files are created with restrictive permissions, and temporary audio is deleted
automatically.

### What is bounded, and what is ceilinged

A **bound** caps the download before the bytes reach disk. A **ceiling** caps what an archive may
write when it is unpacked. They answer different exposures, and neither is the digest.

The digest is the strongest check and it is also the *last* one: `kw_sidecar::integrity::verify`
cannot run until the file is on disk. So a hostile or broken mirror gets one window in which to
fill the disk with something that will be rejected afterwards, and the download bound is what
closes that window. The extraction ceiling then sits behind a digest that has already matched,
because a pinned archive can still be a decompression bomb if whoever pinned it was wrong.

| asset class | download bound | extraction ceiling |
| --- | --- | --- |
| Sidecar binaries (whisper, llama, ffmpeg, yt-dlp, sherpa) | `sizeBytes`, exact | `maxExtractedSizeBytes` |
| Parakeet/sherpa ASR models | `expectedSizeBytes`, exact | `maxExtractedSizeBytes` |
| Whisper `.bin` models | `expectedSizeBytes`, exact | none — not an archive |
| Local text-generation GGUFs | `sizeBytes` where measured; none where declared | none — not an archive |

**Every downloadable asset above is digest-checked as of 2026-08-31, but not all to the same
standard, and the difference is reported rather than averaged.** Sidecar binaries, whisper models
and Parakeet archives are pinned to bytes this project downloaded and hashed. Thirty-two of the
thirty-nine local-LLM GGUF objects are pinned to Hugging Face's stated `lfs.oid` instead: a real
SHA-256 of the object — git-lfs is content-addressed on it — delivered over the same channel as the
bytes. That catches a corrupted transfer, a truncation the length survived, and an accidental
re-upload; it does not catch a compromised huggingface.co, because one host would serve both halves
of the comparison. The remaining seven were downloaded and hashed here, one per publishing
organisation, which is how the stated metadata was tested: thirteen objects across eight
organisations, 30.08 GiB, no disagreement. `local_model::VerifiedBy` carries `StatedDigest` as a
state of its own and the screen prints a different sentence for it, so the weaker pin is never
displayed as the stronger one. Measuring all thirty-nine is 202 GB of downloading, which is the
cost decision this trade is made against (KNEG-45).

**A bound is only ever an exact figure.** Every bound above comes from a measurement:
`tools/sidecar-pin/pin.mjs` and `tools/model-pin/pin.mjs` take the length from `stat` after a full
download, never from `content-length`, because a truncated body agrees with its own header. A
*declared* length cannot bound a transfer — the true length may be on either side of it, so a
bound built from one refuses legitimate installs. That is not hypothetical: eighteen of
thirty-five local GGUFs were declared 6.7–7.9% larger than the file Hugging Face serves
(KNEG-101), and a bound on those figures would have refused all eighteen mid-transfer. The
eighteen are measured now; a model added to the registry before anybody measures it is downloaded
unbounded and checked against its declared size afterwards, which is degraded rather than broken.

**A ceiling is a judgement, and says so where it is written.** `maxExtractedSizeBytes` is the
measured extracted total plus headroom, not a measurement — `withHeadroom` in
`tools/sidecar-pin/pin.mjs` states the rule and `SHERPA_PINS` in
`tools/integration-audit/generate-provider-registry.mjs` records the reasoning for its two
hand-chosen figures. Headroom errs high on purpose: being wrong high costs protection, being wrong
low refuses a legitimate install.

Two gaps are recorded rather than implied. Neither has a `maxExtractedSizeBytes` because neither
is an archive: a whisper `.bin` and a GGUF are downloaded and never unpacked, so the only thing an
unverified one can produce is bad output. And `extract_tar_xz` holds the whole decompressed tar in
memory before reading an entry, so a `.tar.xz` is a memory bomb independent of any output ceiling
(KNEG-106, behind a mandatory digest, two pinned assets).

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
