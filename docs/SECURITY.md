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
fetch anything, are unaffected — and that local path must be one a file dialog returned, which the
Webview section below describes. The full accounting, and what re-opening it requires, is in
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
3. **Rebuild before checking whether step 2 worked.** `updater.rs` reads the
   key from the *compiled-in* configuration, not from the file on disk:
   `lib.rs` builds the app with `tauri::generate_context!()`, which embeds
   `tauri.conf.json` at compile time, and `configured_public_key` reads that
   embedded copy through `app.config()`. So an application that was already
   built — an installed bundle, or a `cargo run` from before the edit — keeps
   reporting `NoPublicKeyConfigured` no matter what the file now says, and
   there is nothing on screen to distinguish that from having pasted the key
   wrongly. This step is written out because the procedure used to go straight
   from step 2 to the CI secret, and a maintainer following it had no way to
   tell a stale binary from a bad key (KNEG-43).

   The rebuild itself needs no special flag and no `cargo clean`: the
   desktop crate's build script emits
   `cargo:rerun-if-changed=apps/desktop/src-tauri/tauri.conf.json`, so an
   ordinary `cargo build`, `cargo run` or `npx tauri dev` recompiles the
   context after the edit. It is the *skipping* of a build that is the trap,
   not the build being hard to trigger.
4. Add the private key file's contents as the repository secret
   `TAURI_SIGNING_PRIVATE_KEY` (repository Settings > Secrets and variables >
   Actions). If step 1 set a password, also add
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. `.github/workflows/release.yml`
   reads both as environment variables on its `tauri build` step and signs
   updater artifacts only when they are present; without them, that workflow
   builds unsigned bundles, which is the state this repository is in today.
5. **The private key must never be committed, and must never be generated on
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

**The verdict is read when it is asked for, not remembered from the write.** `local_api_status`
and `cli_status` both call `discovery::inspect`, which re-reads the file's access list and the
directory's on every call — two `stat` calls on Unix, two `GetNamedSecurityInfoW` calls on Windows.
Something with write access to the parent that adds a trustee afterwards is reported the next time
a screen asks, rather than at the next launch (KNEG-110).

It **re-reads and never re-applies**, deliberately. Silently repairing an access list somebody
changed on purpose is its own kind of surprise, and a status screen has to be able to describe a
bad state without altering it. So the application will tell you the token file has been widened; it
will not quietly narrow it again.

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
before use. The capability files themselves are enabled by name in `tauri.conf.json`'s
`security.capabilities`; a file that list does not name is parsed and ignored, which is what
KNEG-171 was, and `tools/capability-audit/scan.mjs` fails when the list and the directory disagree.

The `recording` URI scheme serves the application-owned WAV attached to a note, addressed only by
note id. `recordings::serve` looks up that id and `recordings::resolve` accepts only a plain file
name and resolves it inside the app-owned recordings directory, so no path crosses the IPC boundary
in either direction. `recordings::response_for` answers byte-range requests for media seeking; the
handler refuses keys that are not plain file names, notes with no recording row, and every method
except `GET` and `HEAD`.

**`Notes > Upload`'s file pickers take a different shape from the rest, worth naming precisely
rather than folding into the paragraph above.** `pick_import_file` opens a real OS dialog and
returns the chosen path *to the renderer*, which holds it in React state and passes it as a plain
string to a separate command — `media_import_audio`, `media_import_video`, or `import_openwhispr` —
on a later click. That is not the shape `note_export_to_file` uses, which fuses picking and writing
into one atomic command so the frontend never sees or supplies a path at all; fusing pick and
import the same way would fight this screen's pick-then-review-then-confirm flow, which a
single-click save does not need. So the path does cross the boundary, and what makes that safe is a
capability rather than the shape: **`commands::DialogGrants` records the canonical form of every
path `pick_import_file` returns, and those three commands — the only three in the application that
accept a path at all — refuse anything that set does not hold.** A compromised renderer calling
`media_import_audio` directly with an arbitrary local path is refused before the file is opened,
which is what stops that file's contents from reaching a configured cloud transcription provider.

Four properties of that set, each a deliberate choice. It is keyed on `std::fs::canonicalize`
output, not on the string the renderer sent, because `pick_import_file` returns
`path.display().to_string()` and `display()` is lossy — the cost is that a path whose name does not
survive lossy conversion cannot be imported, which was already true in practice and is now an
explicit refusal. It lasts until the application exits, because single-use would break
retry-after-failure, which the screen relies on today. It is not time-bounded, because the review
step can legitimately sit open across a sidecar install of tens of megabytes. And it is bounded by
count (32) so a renderer calling the picker in a loop cannot grow it without limit.

**What is asserted and what is not.** Unit tests in `apps/desktop/src-tauri/src/media.rs` cover the
refusal, the retry, eviction past the bound, and an ungranted credential-file path. Only a real
call against the running application proves the policy is consulted on this path; that check is
still open as item 1 under `docs/BENCH_CHECK.md`'s Task 12 heading.

## Sidecars and downloaded models

Explicitly allowlisted; the frontend cannot spawn a process. Sidecar downloads are pinned by
version and verified by digest — `kw_sidecar::manifest::Asset` cannot represent an executable
without one, so an unpinned binary does not deserialize rather than skipping verification.
Temporary files are created with restrictive permissions, and temporary audio is deleted
automatically.

**The digest that was checked is now written down** (2026-09-01, KNEG-113). Until then it was
verified against the download and then forgotten, so an installed sidecar could be proved to exist
and never to be the one this build pins — the machine this was written on ran a `whisper` sidecar
a week behind its own manifest and nothing would ever have said so. `install` writes a
`.kw-install.json` into the directory it promotes, and `kw_sidecar::record::freshness` compares it
against the pinned digest.

**Read it as a staleness check and not as a tamper check.** It proves the directory was produced by
extracting the archive with that digest. Nothing re-hashes an installed executable, and the record
is an ordinary file that anything able to write to the directory could rewrite — so it detects a
sidecar left behind by an older release, not one substituted by an attacker who already has write
access there. An install made before the record existed reports `Unknown`, which is neither
`Current` nor `Stale`, for the same reason `VerifiedBy::StatedDigest` is not `Digest`.

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

**A digest only bounds a transfer if it and the URL describe the same object.** Until 2026-08-31
every model URL ended `resolve/main`, a branch pointer: the bytes behind it changed whenever the
publisher pushed, so a digest measured on Monday would refuse Tuesday's install — the correct
refusal, for a reason that had nothing to do with the user or the download. All 45 Hugging Face
URLs now name a commit, measured into `HF_REVISIONS` by `node tools/model-pin/pin.mjs --revisions`
and held by four generator guards. This is **not** a stronger integrity claim; the digest is still
what proves the bytes. It removes a class of spurious failure and makes the URL and the digest
describe the same objects by construction.

**The six k2-fsa assets are the remaining exception, and it is now a measured one.** They are
GitHub release assets on rolling tags, where an asset can be replaced under the same name: the four
sherpa ASR archives in `asr-models`, and the two diarization models in `speaker-segmentation-models`
and `speaker-recongition-models`. There is no immutable URL to move them to: the versioned releases
carry library binaries and no models, the Hugging Face mirror holds unpacked files rather than the
archive, and a release-asset id is destroyed when an asset is replaced rather than surviving beside
the new one. What GitHub does offer is a **stated size per asset, and a stated digest for assets
uploaded from about mid-2025**. The two Parakeet archives' stated digests matched their committed
pins exactly on 2026-08-31 (KNEG-112), and all four ASR archives carry one. For those four a swap is
caught three ways: cheaply within a week by `--verify-stated`, by the weekly `--verify`
re-download, and unconditionally at install by the digest that was measured here.

**The two diarization models have no stated digest, and that costs less than it looks.** Both were
uploaded in October 2024, before GitHub stated digests, and the release API returns `digest: null`
for them (measured 2026-09-29). `--verify-stated` compares their size and prints a note. A swap
would still show there: a replaced asset is a new upload, new uploads carry a digest (measured the
same day: every asset in `asr-models` uploaded since 2025-06-30 has one, and every one in
`speaker-recongition-models` since 2025-08-14), and that digest would disagree with the pin. That
GitHub keeps computing them is believed, not guaranteed. What is lost is the publisher's
corroboration of the bytes as they stand today; the weekly `--verify` re-download and the
install-time digest check (`crates/meeting-core/src/diarization_models.rs`, which refuses an
unpinned model and removes a file that fails) hold that. Until 2026-09-29 the missing digest was
counted as a disagreement, which turned the weekly job red every week from 2026-09-07 for a reason
that could not clear (KNEG-358).

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

One gap is recorded rather than implied. Neither a whisper `.bin` nor a GGUF has a
`maxExtractedSizeBytes`, because neither is an archive: both are downloaded and never unpacked, so
the only thing an unverified one can produce is bad output.

`extract_tar_xz` used to hold the whole decompressed tar in memory before reading an entry, which
made a `.tar.xz` a memory bomb independent of any output ceiling. **Closed on 2026-08-31
(KNEG-106):** the decompressor runs on its own thread writing into a bounded channel, so the bytes
in flight are capped at one megabyte however large the archive decompresses to. What that does
*not* cover is `lzma-rs`'s own LZMA2 dictionary, whose size the xz header declares and which is
allocated inside the decompressor — a few megabytes for any real archive, and not attacker-chosen
here, because extraction only runs after a mandatory SHA-256 matched.

## Logging

Never logged: API keys, OAuth tokens, local API bearer tokens, raw clipboard contents, raw audio,
credential file contents, or full transcripts by default. Secrets are redacted at the logging
boundary rather than at each call site, so a new call site cannot forget.

## Recording indication

When recording is active the user gets a visible indicator. The application does not record
silently, and it will not conceal recording from the person using it — a design constraint, not a
setting.

## Observing other applications

On Windows the application enumerates **which processes currently hold an audio capture session**,
through `IAudioSessionManager2`, once per detection poll. It exists for one reason: new Microsoft
Teams runs a call inside `ms-teams.exe` and spawns no child process, so the process table cannot
tell a call from an open client and the microphone can.

What that reads, exactly, and what it does not:

- **Read:** a process id and image name per active capture session, on every active capture
  endpoint. Nothing else about those processes.
- **Not read:** no other application's audio. Enumerating sessions asks Windows for a state it
  already publishes to any process on the desktop; it does not attach to a stream, open a device,
  or observe a sample. Nothing here can hear another application.
- **Not stored and not logged.** The list is held in memory for the life of one poll and reaches
  only the meeting screen. It is not written to the database and no log line contains it.
- **Windows only.** Linux and macOS read nothing, and `Settings > Meetings` reports which of those
  this machine is — because "Teams is open and not in a call" and "Teams is open and nobody
  looked" are the same sentence with opposite meanings.

**It suggests; it never records.** A detection can only put an offer on screen. Nothing in this
path can start a recording, which is the same rule the process-based detectors already follow.

## What we deliberately do not do

No process hiding, no anti-forensics, no security-software evasion, no attempt to bypass
administrator controls, and no attempt to hide recording from the local user.

The privacy overlay is excluded from screen capture on a best-effort basis where the OS supports
it, and `PLATFORM_SUPPORT.md` reports honestly where it does not. We do not describe the
application as undetectable.
