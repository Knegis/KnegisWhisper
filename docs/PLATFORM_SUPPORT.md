# Platform Support

This document reports **capability, not intent**. A row says "works" only where that has been
observed. Where the operating system or compositor prevents something, it says so.

We do not claim the privacy overlay is undetectable, and we do not report capture exclusion as
active without evidence.

## Verification status

| Platform | Build | Runtime verification |
| --- | --- | --- |
| Windows 11 x64 | local + CI | **primary — verified by hand** |
| macOS (Apple Silicon) | CI compile only | ⚠️ **unverified — no hardware available** |
| macOS (Intel) | CI compile only | ⚠️ **unverified — no hardware available** |
| Linux X11 | CI compile only | ⚠️ unverified |
| Linux GNOME Wayland | CI compile only | ⚠️ unverified |
| Linux KDE Wayland | CI compile only | ⚠️ unverified |
| Linux Hyprland | CI compile only | ⚠️ unverified |

Development happens on a Windows-only machine with a single Rust target
(`x86_64-pc-windows-msvc`). macOS-native code — the `tauri-nspanel` non-activating panel,
EventKit calendar access, and CoreAudio process taps — is written behind traits and
compile-checked in CI, but **nothing has run it**. It stays marked unverified here until someone
runs it on hardware. Treat any claim to the contrary in this table as a bug in the table.

## Providers verified against a live service

Platform support is one axis; whether a provider has ever actually answered is another, and it is
just as easy to claim falsely. Same rule: a row says "verified" only where a real request came back.

| Provider group | Request shape | Live call |
| --- | --- | --- |
| Transcription — OpenAI, Groq, Berget | `HttpBatch`, tested against recorded fixtures | ⚠️ needs a key; unverified |
| Transcription — Mistral, xAI, Corti | their own clients, tested against fixtures | ⚠️ needs a key; unverified |
| Transcription — Tinfoil | **declined, not unimplemented** — an unattested request would claim Tinfoil's confidentiality without providing it | — |
| Reasoning — the seven cloud providers | OpenAI-compatible, tested against fixtures | ⚠️ needs a key; unverified |
| **Enterprise — AWS Bedrock** | Converse body + **SigV4 signing verified against AWS's own published vectors** | ⚠️ **unverified — no AWS organisation credentials** |
| **Enterprise — Azure OpenAI** | deployment path, `api-version` query parameter, `api-key` header | ⚠️ **unverified — no Azure organisation credentials** |
| **Enterprise — GCP Vertex AI** | service-account JWT → OAuth2 exchange, OpenAI-compatible endpoint | ⚠️ **unverified — no GCP organisation credentials** |

The Bedrock row is the strongest claim of the three and still not a verified one. AWS publishes
signing test vectors — a request, its canonical form, its string-to-sign, its derived key, and the
expected signature — and `crates/reasoning-core/src/sigv4.rs` reproduces all five, including
through the public `sign` entry point. That establishes the *signature* is right. It does not
establish that Bedrock accepts the request, that the Converse body is shaped the way a particular
model wants it, or that the endpoint is reachable from here. Those need an account.

`crates/reasoning-core/src/sigv4.rs` deliberately omits `x-amz-content-sha256`, which S3 requires
and Bedrock does not, so that `sign` itself can be checked against AWS's vector rather than only
its internals. A signing routine nobody can check against an outside answer is one you have to
take on trust.

## Capture exclusion

Reported as one of four states, never a boolean:

| State | Meaning |
| --- | --- |
| `active` | Exclusion applied **and positively verified**. |
| `unsupported` | The platform or compositor cannot do it. |
| `failed` | The attempt returned an error. |
| `unknown` | Applied, but the result cannot be read back. |

| Platform | State | Mechanism |
| --- | --- | --- |
| Windows 10 2004+ | `active` | `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`, then **`GetWindowDisplayAffinity` read back** and compared. `active` is reported only when the read-back matches. **Measured 2026-09-07** with `crates/platform/examples/capture_probe.rs`: GDI `BitBlt`, Windows.Graphics.Capture monitor capture and DXGI Desktop Duplication all show the desktop where the excluded window is while an unexcluded control window stays visible (3 of 3 `EFFECTIVE`), and Windows.Graphics.Capture window capture of the excluded window delivers no frames. One machine, Windows 11 10.0.26200. |
| Windows < 10 2004 | `failed` | `WDA_EXCLUDEFROMCAPTURE` is unavailable; the call fails and the OS error is reported verbatim. |
| macOS | `unsupported` | Not implemented. `NSWindow.sharingType = NSWindowSharingNone` is the intended mechanism and it is readable, so this can become `active` with evidence — but it cannot be built or run from the current development machine, and an unverified claim about a privacy feature is worse than an admitted gap. |
| Linux (all) | `unsupported` | There is no such facility. Neither X11 nor any current Wayland protocol lets a client ask to be omitted from a screen capture. |

### A correction to the Phase 0 audit

The audit recorded that `SetWindowDisplayAffinity` has "no documented getter" and concluded
Windows would have to report `unknown`. That was wrong: **`GetWindowDisplayAffinity` exists, and
has since Windows 7.** Windows therefore reports `active` with real read-back evidence, and
`unknown` is now reserved for the case where the set succeeds but the read fails.

Be precise about what the read-back proves. It proves **the flag is set on the window**. It does
not prove that a particular screen recorder is excluded — that depends on which capture API the
recorder uses, and no API reports that from inside this process. `Evidence::ReadBack` names the
API it consulted so the claim stays attached to its source, and the settings screen says exactly
this rather than showing a bare green check.

What the read-back cannot say, a probe can measure from outside the process: `capture_probe`
(`cargo run -p kw-platform --example capture_probe`) draws an excluded window beside an unexcluded
control and looks at what each recorder API actually delivers. On 2026-09-07 all three delivered the
desktop behind the excluded window and the control intact, so on this machine the flag does what the
settings screen hopes it does. That is a measurement of three APIs on one Windows build, not a
guarantee about every recorder; the read-back stays the only claim the application makes about
itself. `docs/BENCH_CHECK.md` § 2026-09-07 (later) has the rows.

Under-claiming here is deliberate. Pluely's entire mechanism is Tauri's `contentProtected` flag,
it never verifies the result, and it is a no-op on Linux — yet its README promises "absolute
stealth" and carries no Linux caveat.

## Non-activating overlay

Whether the overlay declines focus. It matters functionally, not cosmetically: an overlay that
takes focus moves the caret out of whatever the user is dictating into.

| Platform | State | Mechanism |
| --- | --- | --- |
| Windows | `active` | `WS_EX_NOACTIVATE` set via `SetWindowLongPtrW`, then read back from `GWL_EXSTYLE`. The declarative `focused(false)` alone does not stop a click from activating the window. |
| macOS | `unsupported` | Not implemented. `NSWindowStyleMaskNonActivatingPanel` via `tauri-nspanel` is the intended mechanism. |
| Linux | `unsupported` | Not implemented. |

Pluely does not implement this on Windows at all — its attempt is present in `window.rs` but
commented out — so there is no reference implementation to copy.

## Privacy overlay

| Capability | Windows | macOS | Linux X11 | Linux Wayland |
| --- | --- | --- | --- | --- |
| Transparent, undecorated | yes | private API, compile-verified only — see below | yes | compositor-dependent |
| Non-activating (no focus steal) | `WS_EX_NOACTIVATE` + `WM_MOUSEACTIVATE` — **new work** | `NSWindowStyleMaskNonActivatingPanel` via `tauri-nspanel` — unverified | window-type hint | compositor-dependent |
| Always-on-top | `"pop-up-menu"` level | `"floating"` level 1 | `"screen-saver"` level | GNOME: `"floating"` |
| Visible on all workspaces | not applicable | `CanJoinAllSpaces` | yes | compositor-dependent |
| Visible over full-screen apps | partial | `FullScreenAuxiliary` | partial | compositor-dependent |
| Click-through | `set_ignore_cursor_events` | supported | supported | `forward` is ignored — see below |
| Taskbar/Dock hiding | `skipTaskbar` | `ActivationPolicy::Accessory` | yes | compositor-dependent |
| Saved position, multi-monitor | yes | yes | yes | yes |

Two notes carried over from the upstream audit:

- **Windows click-through forwarding is unreliable.** OpenWhispr sets
  `setIgnoreMouseEvents(false)` permanently on Windows for this reason. We follow suit rather
  than expose a toggle that does not work.
- **Linux ignores the `forward` flag**, so click-through cannot be toggled per-region there.
  OpenWhispr skips the toggle entirely on Linux.

And one that is this project's own, recorded 2026-09-08 (KNEG-221):

- **macOS transparency is a private API, and the row above used to say plain "yes".** It said so
  while the desktop application did not compile on macOS at all. `WebviewWindowBuilder::transparent`
  is `#[cfg(any(not(target_os = "macos"), feature = "macos-private-api"))]` in tauri 2.11.5, so the
  three transparent panels — the meeting notification, the dictation overlay and the update
  notification — were `E0599: no method named transparent` there while compiling cleanly on Windows
  and Linux. Both halves are now set: the `macos-private-api` feature in
  `apps/desktop/src-tauri/Cargo.toml` (what the compiler needs, and what CI's `cargo` invocations
  see) and `app.macOSPrivateApi` in `tauri.conf.json` (what the Tauri CLI derives its feature list
  from, and what the runtime reads). `tools/capability-audit/scan.mjs` rule F fails when a
  `.transparent(...)` call is not matched by both, which is what makes the defect visible from a
  machine that cannot compile for macOS.

  **The cost, stated plainly: the bundle is ineligible for the Mac App Store while this is on.**
  Accepted rather than worked around, because the alternative is an opaque rectangle. The indicator
  rule at the top of `MeetingNotification.tsx` and `Overlay.tsx` is that the person at the keyboard
  always sees that a microphone is open; a non-transparent always-on-top panel is a worse product
  than one that cannot be sold through Apple's store, and this project ships its own installers
  (`docs/BUILD_PLAN.md` chunk J) rather than through the store.

  **Still compile-verified only.** Nothing has run on macOS — that is KNEG-59, and it covers the
  `tauri-nspanel` row above for the same reason. A transparent window that paints an opaque
  background is a separate defect class (KNEG-163 on Windows), and rule F cannot see it.

## Audio capture

| Capability | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Microphone | `cpal` | `cpal` | `cpal` |
| System audio | WASAPI process loopback | CoreAudio process taps | PipeWire via xdg-desktop-portal |
| System audio floor | — | **macOS 14.4+** for `cidre` process taps; 14.2+ for the helper-based tap | PipeWire, or PulseAudio via `pipewire-pulse` |
| Acoustic echo cancellation | WebRTC APM sidecar | same | same |

The macOS version floor matters. Pluely declares `minimumSystemVersion: "10.13"` while using an
API that needs 14.4 — a hard runtime failure rather than graceful degradation. We gate on the
OS version and report `unsupported` below the floor.

Linux system audio works through the desktop portal. Without a portal implementation, or on a
PulseAudio-only system without `pipewire-pulse`, it reports `unsupported`.

## System audio

What the speakers are playing, for meeting transcription. Reported by enumerating real devices, so
the answer names the device that would be captured rather than describing the platform.

| Platform | Mechanism | State |
| --- | --- | --- |
| Windows | WASAPI loopback on the default output device, through `cpal` | **verified** — 97 920 samples at 48 kHz stereo, peak 1.0, captured from a real playback |
| Linux | a PulseAudio or PipeWire `.monitor` source, which `cpal` lists as an ordinary input | ⚠️ written, **unverified** — reported as `unverified: true` so no screen can present it as tested |
| macOS | none available | ⚠️ **unsupported**, and it says why: `AudioHardwareCreateProcessTap` (14.4+) or a user-installed virtual device, neither exposed by `cpal` |

`cargo run -p kw-audio-core --example system_audio_probe` is the check. It enumerates, opens a real
stream, and reports whether the callback fired, whether it produced silence, and what the mixer did
with it — three different facts that a single "it works" would collapse.

## Meeting transcription providers

Four cloud providers, one local model, and Tinfoil's deliberate refusal. Three different
epistemic states sit inside the "unverified" claims below, and they are not interchangeable:

1. **Reused from tested code.** Corti's OAuth2 token exchange reuses
   `kw_transcription_core::clients::corti_token_endpoint`/`corti_token_request`/`interpret_token`,
   already implemented and tested for batch Corti transcription. Not believed -- known to work
   against that shape.
2. **Read from vendor documentation, unverified against a live call.** AssemblyAI's and Deepgram's
   wire shapes, and most of OpenAI Realtime's, are clean-room builds against public documentation
   with no live socket to check them against yet. Wrong only if that documentation is wrong.
3. **Invented, with no vendor source at all.** Corti's *socket* wire -- the endpoint path, the
   message envelope, and most of the session-config block -- has no documentation anywhere to be
   wrong about; its own implementer said so directly. This is a strictly weaker position than (2),
   and the table and prose below keep it separate rather than folding it into the same "unverified"
   word.

**What a provider failure now does, for all five clients.** As of 2026-08-31 a provider that
reports a fault -- an error frame, a close frame with a code and a reason, a dead transport, or a
first frame this client cannot parse -- ends the session, and Stop reports the provider's own
sentence rather than an empty transcript. That is a claim about *this application's* wiring only.
It is covered by tests in each socket module that record a fault the way the reader task would; it
is **not** evidence about any provider's behaviour, and for the three clients below with no key it
has never been exercised by a real refusal. A provider that reports nothing and simply produces no
transcript is still indistinguishable from a quiet room, and the failure is still invisible *while*
the meeting runs.

| Provider | Client | Verified |
| --- | --- | --- |
| Local windowed model | repeated `/inference` passes over the local `whisper-server` | ⚠️ built, never run by a person |
| OpenAI Realtime | two WebSockets, one per capture | ✅ **Verified live, 2026-08-28.** A real meeting transcribed, microphone and system audio, against the GA API. Five live attempts were needed first: the client had been written against the beta shape, which OpenAI has retired — see `docs/BENCH_CHECK.md` |
| AssemblyAI Realtime | two WebSockets, one per capture | ⚠️ request shaping and the event grammar against fixtures, and most of the wire shape **believed rather than confirmed** -- see below. **No live call and no key for this provider exists on any machine this project has touched.** |
| Deepgram Realtime | two WebSockets, one per capture | ⚠️ request shaping and the event grammar against fixtures, and most of the wire shape **believed rather than confirmed** -- see below. **No live call and no Deepgram key exists on any machine this project has touched.** |
| Corti Realtime | two WebSockets, one per capture, authenticated by an OAuth2 client-credentials exchange first | ⚠️ request shaping and the event grammar against fixtures. Two different epistemic states, not one: the **token exchange is reused from working, tested code**, not believed -- `kw_transcription_core::clients::corti_token_endpoint`/`corti_token_request`/`interpret_token` already exist and are already tested for batch Corti transcription. The **socket wire, by contrast, was invented with no vendor source at all** -- weaker than "believed," see below. **No live call and no Corti credential exists on any machine this project has touched.** |
| Tinfoil Realtime | — | **declined.** An unattested socket would present as Tinfoil while providing none of the confidentiality the name means. Same reason as the batch client |

Three things about the OpenAI client are unlike the others and each is a place a
live call would tell us something a fixture cannot:

- **24 kHz only**, where every other provider takes 16. The meeting pipeline
  resamples per session because of it; it used to hardcode 16 kHz, so this client
  could never have connected at all.
- **A sixty-minute server cap.** A longer meeting rebuilds both sockets two
  minutes early, so the gap falls where the client chooses rather than
  mid-sentence, and `MeetingTranscript::reconnects` counts it. This is the only
  provider where that path is reachable, and nothing but a call over an hour long
  exercises it.
- **Two sockets, one per capture.** The provider labels no speakers, so
  attribution comes from which capture the audio came from. One socket would make
  `AudioSource` on every event a guess and would hand the server's voice-activity
  detection two overlapping speakers.

The AssemblyAI client is a clean-room build: no vendor source for this wire exists
anywhere in this repository to check against, so most of it is **believed from
public documentation, not confirmed**. `crates/meeting-core/src/assemblyai.rs`'s
module documentation carries the full list; the highest-risk item is repeated here
because it bears on a capability claim rather than only on plumbing:

- **Whether a real-time turn carries a per-speaker label at all.** The capability
  table declares `speaker_labels: true`, and this client reads a `speaker` field
  from each turn if the wire carries one. If it does not, every event from this
  provider reports `speaker: None` — the same degradation OpenAI Realtime already
  reports honestly, and `AudioSource` still attributes the transcript correctly
  either way. This is a safe failure mode, not a silent one, but it means the
  `speaker_labels: true` row should be read as "believed" until a live session
  confirms it.
- The endpoint path, the query parameter names (`sample_rate`, `encoding`,
  `language_code`, `keyterms_prompt`), the `Authorization` header carrying the
  raw key with no `Bearer` prefix, and the `Begin`/`Turn`/`Termination`/`Error`
  message envelope are all likewise unverified against a live socket.
- Audio leaving as raw binary WebSocket frames (not JSON-plus-base64, the
  opposite of OpenAI Realtime's convention) is the one piece believed with more
  confidence, since it matches AssemblyAI's documented batch-upload convention.

Two sockets, one per capture, for the same reason as OpenAI Realtime: this
provider's speaker labels say who *within* a stream, never *which* stream, so
`AudioSource` still needs two connections to stay a fact rather than a guess.

The Deepgram client is a clean-room build in the same position as AssemblyAI's:
no vendor source for this wire exists anywhere in this repository, so most of it
is **believed from public documentation, not confirmed**.
`crates/meeting-core/src/deepgram.rs`'s module documentation carries the full
list; the items that bear on a capability claim rather than only on plumbing are
repeated here:

- **`token_rotation: true` describes upstream's minted temporary tokens, not
  this client.** Deepgram's own hosted service issues short-lived tokens that
  must be refreshed mid-session; this application never mints one, so the
  header carries the user's own stored API key and nothing here schedules a
  refresh. `crates/meeting-core/src/deepgram.rs` and `deepgram_socket.rs` both
  carry a test proving no rotation clock exists, with a comment saying why.
  Implementing a refresh loop that re-reads an unchanging key would be
  ceremony that looks like a safeguard and is not one.
- **Whether a `Results` message carries a per-word `speaker` field at all**, and
  whether reading the *first* word's speaker as representative of the whole
  message is a safe simplification. The capability table declares
  `speaker_labels: true`; if the wire does not carry the field, or splits it
  across the message differently than believed, every event from this provider
  degrades to `speaker: None` — the same safe degradation AssemblyAI and
  OpenAI Realtime already report, and `AudioSource` still attributes the
  transcript correctly either way.
- **Whether omitting a language and sending `detect_language=true` is in fact
  how this provider auto-detects**, as opposed to defaulting to a fixed
  language when neither is sent. `language_detection: true` is declared, and
  this client actively asks for detection rather than silently sending
  nothing and hoping the server's default matches the room.
- The endpoint path, the query parameter names (`encoding`, `sample_rate`,
  `channels`, `language`, `detect_language`, `diarize`, `interim_results`, and
  the repeated `keyterm` parameter itself), the `Authorization: Token <key>`
  header (a scheme prefix different from AssemblyAI's bare key and OpenAI's
  `Bearer`), and the `Results`/`Metadata`/`Error` message envelope are all
  likewise unverified against a live socket.
- Audio leaving as raw binary WebSocket frames, not JSON-plus-base64, is
  believed with more confidence than the rest, matching both AssemblyAI's
  assumed convention and Deepgram's documented one for raw linear PCM.

Two sockets, one per capture, for the same reason as AssemblyAI and OpenAI
Realtime: Deepgram's diarization says who *within* a stream, never *which*
stream, so `AudioSource` still needs two connections to stay a fact rather
than a guess.

The Corti client is the only one of the five where **two different claims
deserve two different confidence levels**, because it crosses the network
twice for two different reasons.

**The OAuth2 token exchange is reused from working, tested code, not
believed.** `kw_transcription_core::clients::corti_token_endpoint`,
`corti_token_request` and `interpret_token` already exist and are already
exercised by `Executor::corti_token` for batch Corti transcription
(`crates/transcription-core/src/executor.rs`). `crates/meeting-core/src/corti.rs`
wraps them rather than re-deriving them, and copies the one decision that
code already makes: the tenant name is the OAuth2 client id, and the secret
stored under the `corti` credential slot is the client secret. The exchange
runs once per meeting, before either socket opens, and both sockets carry the
same access token -- an OAuth2 client-credentials token is meant to be
presented on more than one request, and nothing in this project's own audit
of Corti suggests otherwise, unlike Deepgram's documented per-session minted
tokens.

**The socket wire was invented, which is a weaker claim than "believed" and
should not be read as the same thing as AssemblyAI's or Deepgram's
unconfirmed-but-documented wires.** Those two clients are wrong, if they are
wrong, because public vendor documentation was wrong or was misread -- there
is a source to check them against. Corti's socket has no such source: no
vendor documentation for its *streaming* wire exists anywhere in this
repository or was found while building it, and the person who wrote it said
so directly and unprompted rather than presenting a guess as a citation.
Endpoint path, message envelope, and most of the session-config block below
are this client's own invention, with one exception
(`configuration.keyterms`) that does have a textual anchor.
`crates/meeting-core/src/corti.rs`'s module documentation carries the full
list; repeated here because it bears on capability claims:

- The endpoint path, `wss://api.{environment}.corti.app/audio-bridge/v1/streams`.
  Only the host template -- `api.{environment}.corti.app` -- is reused from
  the tested batch endpoint; the path after it is invented.
- The `Authorization: Bearer <token>` scheme is the one item believed with
  more confidence than the rest of the socket wire: it is the standard OAuth2
  bearer convention (RFC 6750), not a Corti-specific guess.
- That the tenant travels on the socket handshake as a `Tenant-Name` header --
  the same header name the tested batch request uses, assumed here to apply
  to a WebSocket handshake, which it has never been tried against.
- The whole shape of the session configuration message beyond one thing:
  `configuration.keyterms` as the path for keyterms comes from this project's
  own audit (`docs/INTEGRATION_PARITY.md`), written before this client and
  independent of it -- the only piece of the streaming wire with a textual
  anchor in this repository. `primaryLanguage` (reused from the batch field
  name), and the `encoding`/`sampleRate`/`channels` fields, are this client's
  own construction.
- The incoming message envelope entirely -- a `type` field naming
  `"ready"`/`"transcript"`/`"error"`, and the fields `text`/`isFinal`/`speaker`
  -- is the lowest-confidence area in this module, with no textual anchor at
  all.

`language_detection: false` is declared for Corti, and it is the first
provider whose client actually reaches `MeetingRuntime::start`'s
`language_required` refusal in practice: every other client built so far
declares detection, so the refusal existed but nothing could exercise it end
to end until a real client existed to be denied a language. Tests in
`crates/meeting-core/src/capabilities.rs` and
`apps/desktop/src-tauri/src/meeting.rs` (`corti_refuses_to_start_naming_the_provider_and_the_field_when_no_language_is_set`)
assert it fires.

Two sockets, one per capture, for the same reason as every client before it:
a speaker label Corti's wire might carry says who *within* a stream, never
*which* stream.

## Meeting detection

Noticing that a meeting application is running, so recording can be *offered*.
Nothing here starts a recording: `crates/platform/src/meeting_detect.rs` cannot,
and `MeetingRuntime::start` has exactly one caller, the command a button invokes.

**The names were written, not ported.** Upstream's `meetingProcessDetector.js` is
not in this checkout, so the table is knowledge rather than a port, and each row
below says whether it has actually been seen to match. `cargo run -p kw-platform
--example meeting_detect_probe` is how a row moves from one column to the other.

| Application | Open | In a call | State |
| --- | --- | --- | --- |
| Microsoft Teams | `ms-teams.exe`, `teams.exe`, `msteams`, `teams` | *the microphone* | **both states seen** (2026-09-01). New Teams runs the call inside its own process, so there is no in-call process — it is detected by the capture session it holds |
| Zoom | `zoom.exe`, `zoom`, `zoom.us` | `cpthost.exe`, `caphost` | ⚠️ written, not seen to match |
| Webex | `webex.exe`, `webexhost.exe`, `ciscocollabhost.exe` | `webexmta.exe` | ⚠️ written, not seen to match |
| FaceTime | `facetime` | — | ⚠️ macOS only, compiled out elsewhere, unverified. Microphone activity cannot be read there, so it has no second signal either |
| Anything else | the user's own allowlist | *the microphone* | works by construction |

**Only a call in progress offers to record**, and which signal says so is
reported rather than flattened. There are two, and they are not equally strong:

- **A call-only process.** `CptHost.exe` is not started by opening Zoom.
- **A held microphone.** Real evidence, and weaker — Teams holds the device for
  its own audio test, and for a call the user placed that nobody joined.

The second exists because of a measurement taken on 2026-09-01 with
`cargo run -p kw-platform --example mic_users_probe`, on Windows, against one
Teams install:

| run | active capture endpoints | `ms-teams.exe` capturing |
| --- | --- | --- |
| Teams not running | 1 | — |
| Teams open, idle | 1 | **no** |
| Teams in a real call | 2 | **yes** |

Every run carried its own control — a second process opened a capture stream and
had to appear in the list — so the middle row is a measured absence rather than a
broken enumeration. The two middle rows differing is the whole basis for the
signal, and until they were taken Teams was permanently unofferable: the only
thing available was "the client is open", true from login on any machine where
Teams starts with Windows.

**The microphone can only strengthen a match, never create one.** A process
holding a capture device that is not a known client and is not on the user's own
list is not reported at all. Otherwise this application's own dictation would
offer to record itself.

**It is Windows-only.** `IAudioSessionManager2` is the mechanism and nothing
equivalent is read on Linux or macOS, so there the capture list is always empty
and Teams and FaceTime stay reported-but-never-prompted, exactly as before.
`Settings > Meetings` says which state this machine is in, because "Teams is
open and not in a call" and "Teams is open and nobody looked" are the same
sentence with opposite meanings.

Matching is on the **image name and exact**, not the command line and not by
substring. `kw_sidecar::reap` matches command lines because it must prove a
process is ours before signalling it; detection has the opposite job, and
`zoomboard.exe` is not Zoom.

`meeting-detect:generic` is the allow/blocklist and nothing else yet. Calendar
events need chunk F; microphone activity needs the `IAudioSessionManager2`
listener that is still unwritten.

## Global shortcuts

| Capability | Windows | macOS | Linux X11 | GNOME Wayland | KDE Wayland | Hyprland |
| --- | --- | --- | --- | --- | --- | --- |
| Standard accelerators | **verified** | plugin, unverified | plugin, unverified | plugin, unverified | plugin, unverified | plugin, unverified |
| Modifier-only (`Control+Super`) | **verified** — `WH_KEYBOARD_LL` | written — `CGEventTap`, compile-only | cannot | cannot | cannot | cannot |
| Right-side modifiers | built — same hook | written — same tap, compile-only | cannot | cannot | cannot | cannot |
| Globe / `Fn` | not built | not built — deliberately, see below | not built | not built | not built | not built |
| Mouse buttons 4/5 | **measured** — `WH_MOUSE_LL` | written — same tap, compile-only | cannot | cannot | cannot | cannot |
| Push-to-talk, hooked accelerators | **measured** — one hook, both edges | written — same tap, compile-only | cannot | cannot | cannot | cannot |
| Portal registration (`GlobalShortcuts`) | not applicable | not applicable | written, compile-only, no consumer | written, compile-only, no consumer | written, compile-only, no consumer | written, compile-only, no consumer |
| Push-to-talk, standard accelerators | believed, never run | believed, never run | believed, never run | believed, never run | believed, never run | believed, never run |

**Four answers in that table, and the distinctions are the point.** Rewritten 2026-09-08 for
KNEG-62, which added two of them.

- *Not built* — there is no code for it anywhere, on any platform.
- *Cannot* — no mechanism exists to build against. This is the honest answer for a modifier-only
  chord on Linux and is not a backlog item: X11 cannot observe one globally without grabbing the
  keyboard away from every other application, and Wayland has no interface for it at all. The
  portal is a *registration* API and its trigger grammar has no way to spell a chord with no key
  in it, so no compositor can grant one however willing it is.
- *Written, compile-only* — the code exists and has **never run**. macOS's `CGEventTap` and the
  Linux portal client are both in this state, and `cargo check --target aarch64-apple-darwin` /
  `--target x86_64-unknown-linux-gnu` against `kw-platform` is the whole of the evidence. Both
  cross-checks run from the Windows development machine, which is how they are known to compile at
  all without waiting for a dispatched CI job.
- *Unverified* — the code is cross-platform, compiles in CI, and has never been run there. This is
  what the plugin row means.

`keyhook::support()` therefore has three answers rather than two. Windows is unconditionally
`Available`; **macOS depends on a permission and is asked, not assumed** —
`CGPreflightListenEventAccess` decides, and a refusal names System Settings > Privacy & Security >
Accessibility rather than reporting that macOS cannot do this; everything else is `Unsupported`
with the *cannot* reason above.

**There is one registration mechanism *in use* and it is the same on every platform.** Everything
in the Standard-accelerators row goes through `tauri_plugin_global_shortcut` (`shortcuts.rs:19`),
and there is still no second path *taken* — no `gsettings`, no `KGlobalAccel` and no `hyprctl`.

A desktop-portal client now exists (`kw_platform::portal_shortcuts`, KNEG-62) and **nothing calls
it**, which is a deliberate half-step rather than an oversight: the module is compile-checked for
Linux and its accelerator-to-trigger vocabulary is unit-tested on every platform, but wiring it
into `shortcuts.rs` would change registration behaviour on a platform this project has never run
(KNEG-61), and the desktop crate cannot even be compile-checked for Linux from the development
machine — `coreaudio-sys` and the GTK `pkg-config` probes need the real SDKs. So the vocabulary is
in place for the wiring to be written against, and the wiring is filed separately. Until it lands,
a Wayland compositor's native shortcut slot is not something this application reaches for.

The default dictation hotkey is `Control+Super`, which is modifier-only and therefore invisible to
the cross-platform API — hence the native hook. **The default is not adjusted per platform.**
`HotkeyAction::default_accelerator` is a bare `match` on the action with no conditional compilation
in it: `grep -n "cfg(target_os" crates/platform/src/hotkey.rs apps/desktop/src-tauri/src/shortcuts.rs`
returns nothing, and `nothing_ships_holding_by_default` pins the value. Every platform ships
`Control+Super`. Where there is no hook it classifies as `ModifierOnly`, is not registered, and
`Settings > Dictation` says it does nothing and to add an ordinary key — rather than the shipped
default silently becoming something else.

**KNEG-62 asked for the opposite and it was declined, 2026-09-08.** That issue's description says
"on X11 and GNOME the effective default hotkey is adjusted because modifier-only combinations do
not work there". It is recorded here as a decision rather than as outstanding work, because the
rule above was settled deliberately after it was written and the reasoning still holds: a default
that differs per platform means the documentation, the settings screen and the user's memory of
what they configured can all disagree, and the failure it avoids — a dead key with no explanation —
is already avoided by saying so on screen. What KNEG-62 *did* change is that the sentence
`Settings > Dictation` shows on Linux now names the mechanism that is missing rather than implying
one could be added; see `portal_shortcuts::PortalRefusal::ModifierOnly`.

**Globe / `Fn` on macOS is the one row that could have been claimed and was not.** macOS reports it
through the same `flagsChanged` events as the eight real modifiers, so the tap sees it. It maps to
`Key::Other` like any ordinary key, because `keys::Key` has no `Fn` variant and the accelerator
vocabulary has no spelling for one — mapping it onto a spare modifier would have made this row read
as supported without any user being able to bind it.

**What is built, as against what the table says is possible:**

| | State |
| --- | --- |
| Windows modifier-only | **verified.** One `WH_KEYBOARD_LL` hook on one thread, in-process, not one child per key. `cargo run -p kw-platform --example keyhook_probe` installs it, injects two presses, and asserts it fires exactly twice. |
| Windows right-side modifiers | **Built and reachable** since 2026-08-31 (KNEG-35). `RightControl+Super` and `RightControl+RightShift` both classify as `ModifierOnly` and go to the keyboard hook, which is the only mechanism that can see a side at all. The spellings come from one table, `keys::SIDED_MODIFIERS`, read by `Chord::parse` and by `hotkey::classify`, so the set each accepts cannot drift from the other. A side combined with an *ordinary* key (`RightControl+D`) is refused, because neither mechanism can take it — the register cannot see the side and the hook only matches modifiers — and the refusal names both ways out. **Never driven from the screen:** the hook path is measured by `keyhook_probe` and the classifier by unit tests, and nobody has typed one into `Dictation > Shortcuts` (KNEG-47). |
| Windows mouse buttons 4/5 | **Built and measured** since 2026-09-01 (KNEG-64). A second hook, `WH_MOUSE_LL`, on the same thread as the keyboard one, installed only when a button is actually bound — that hook sits in the path of every mouse *move* on the machine, so it is not carried for nothing. `Mouse4` and `Mouse5` classify as `Accelerator::MouseButton`, a third classification rather than a modifier-only chord with an odd spelling. `cargo run -p kw-platform --example mousehook_probe` measures both halves: the raw encoding (the button number is in `mouseData`'s **high** word — 1 for `XBUTTON1`, 2 for `XBUTTON2` — while `SendInput` takes the same numbers in the *low* word) and the shipped hook firing once per press and once per release for the right index, with mouse movement and a wheel notch as the control. **Never driven from a real mouse:** every press in the probe is injected, and nobody has pressed a physical side button (KNEG-47's mouse twin). |
| Windows push-to-talk | **Built** since 2026-09-01 (KNEG-64), per action, and **off by default**. `hotkey.<action>.mode` is `toggle` or `hold`; the shipped value is `toggle` for all five and deliberately stays that way — holding is one of the candidate answers to KNEG-115, and shipping it as the default would settle that open question in passing. The hook path is measured (`keyhook_probe` reports two presses **and two releases** for two injected chords); the plugin path — push-to-talk on an ordinary accelerator like `Control+Shift+D` — routes `ShortcutState::Released` through the same decision and is **believed, not run**. |
| macOS | **Written 2026-09-08 (KNEG-62), never run.** A `CGEventTap` at `kCGHIDEventTap`, `ListenOnly`, on a dedicated `CFRunLoop` thread — the direct counterpart of the Windows hook thread, and it never swallows or rewrites an event for the same stated reason. The decision logic is not new: it reuses `keys::HookWatcher` unchanged, so what was written is plumbing plus two tables. **The load-bearing platform difference:** a modifier press is not a key event on macOS at all — pressing Control alone emits `kCGEventFlagsChanged` and no `keyDown` — so a tap watching only key events would see nothing whatever for `Control+Super`, which is the entire capability. And `CGEventGetFlags`'s public masks are *unsided*, so the side comes from the keycode and the direction from a tracker, not from the flags. `crates/platform/src/macos_input.rs` holds both tables with ten unit tests that **run on Windows** — deliberately outside the `cfg`, because there is no Mac and no probe here, so the one checkable part is placed where a check can reach it. Mutation-checked: swapping Option and Command, or shifting the mouse-button numbering by one, each fails the test that claims to catch it. The permission is *asked* (`CGPreflightListenEventAccess`), never assumed. |
| Linux | **Cannot**, for modifier-only chords, mouse buttons and push-to-talk on a hooked chord — see the four answers above; this is a missing mechanism, not missing work. For *ordinary* accelerators there is now a written `org.freedesktop.portal.GlobalShortcuts` client (`kw_platform::portal_shortcuts`, KNEG-62), **compile-only and with no consumer**. Its tested half is the accelerator-to-trigger vocabulary — `Super` becomes `LOGO`, modifiers are emitted in the grammar's order so two spellings of one chord cannot compete for the same keys, and modifier-only, sided and mouse accelerators are refused *before* the D-Bus call with a reason the user can act on. Its untested half is the D-Bus itself: no session bus, no `xdg-desktop-portal` and no compositor has answered any of it, so it is **believed, not confirmed**, in the same sense as the realtime meeting clients (KNEG-55/56/57). Needs xdg-desktop-portal 1.17 or newer. |

**An accelerator being live is only half of a working hotkey.** Of the five actions
`HotkeyAction` declares, exactly one — dictation — has anything behind it. The other four used to
register a genuine system shortcut, take the chord away from every other application, and do
nothing at all when pressed; the settings screen showed the accelerator with no problem beside it,
which is the interface agreeing that it worked. They are now not registered and not hooked, and the
screen names the action and says which feature is missing. `shortcuts::unhandled_reason` is the one
place that knows, and both the registration path and the dispatch path read it.

The same half-a-path fault reached the hook itself: it delivered the `hotkey` event and stopped, so
`Control+Super` — the default — announced itself in the interface and started no dictation. The
probe that verified the hook had verified that it *fires*, which is not the same claim. Both paths
now run one dispatch table.

The hook **does not consume** the keystroke. `Control+Super` still reaches the shell and Windows
still does whatever it does with it. Consuming it is possible and is deliberately not done: an
application that silently eats a system shortcut is indistinguishable from a broken keyboard.
`shortcuts::Registration` therefore distinguishes `Hooked` from `Registered`, because the two
behave differently in a way the user can notice.

The hook sees every keystroke on the machine, including into other applications. **Nothing is
logged.** What leaves the module is the index of a chord that matched, never a key.

**Modifier-only toggle chords activate on full release (KNEG-115, 2026-09-05).**
An exact chord arms a candidate; any additional key cancels it until every key is up.
`Control+Super` remains the default. Hold mode still begins on press and ends when
one chord key is released; standard registered accelerators and mouse buttons keep
their existing timing. Matcher and desktop dispatch tests cover this behavior;
a physical shortcut check remains separate from those tests.

The historical AltGr measurements below describe press matching. For toggle mode,
typing another key cancels activation, but tapping AltGr alone can still match
Control+Alt on release. Hold mode retains the measured press behavior.

**The AltGr consequence, measured 2026-08-26 and warned about since 2026-08-31 (KNEG-37).** Windows
has no AltGr virtual key: the keyboard layout synthesises **left Control plus right Alt**, so on a
Nordic, German, Polish or most other non-US layout, binding `Control+Alt` starts a dictation every
time the user types `@ $ € { [`. Nothing downstream can tell that apart from the chord being pressed
deliberately, so it is not fixable at the hook — what was missing was anyone saying so.

`Settings > Dictation`'s Shortcuts section now carries a caution per colliding binding, and
`hotkey_set` returns the same sentence the moment the hotkey is saved. Two measurements feed it,
both re-runnable with `cargo run -p kw-platform --example altgr_probe`:

- **Does this layout have AltGr?** `kw_platform::keyboard_layout` asks `VkKeyScanExW` whether any
  character needs Control and Alt together, and names the character it found rather than the locale.
  On the development machine: *"AltGr is present: `'@'` is typed with Control and Alt together."* A
  layout with none is left alone; a platform that cannot be asked warns anyway and says it could not
  ask, because silence on a layout that does have AltGr is the defect and a spare sentence is not.
- **Which chords collide?** `kw_platform::keys::altgr_collision` answers by driving the **real chord
  matcher** with the keys a real AltGr press sends. That is not a stylistic choice: a predicate
  reasoning about modifier names would have missed `Control+Shift`, which also fires — AltGr's left
  Control arrives while Shift is already held, completing the chord before the Alt is pressed.

A **sided** chord avoids it entirely, because AltGr is left Control and right Alt specifically:
`LeftControl+LeftAlt` and `RightControl+Alt` are both unaffected. Until KNEG-35 closed on
2026-08-31 no settings field accepted a sided spelling, so the caution stated the problem and
deliberately suggested nothing. It now names an escape — built from the chord that collided by
`keys::sided_escape`, and passed back through `altgr_collision` before it is offered, so a
suggestion that still fires on AltGr is never made.

**Standard accelerators are not warned about**, and that is a decision rather than an oversight.
`Control+Alt+D` goes to the system shortcut register rather than to the hook, and whether *that*
also sees AltGr as Control+Alt has never been measured by this project. Believed, widely, and
unverified — so not claimed on a screen.

**The shipped default is unaffected by AltGr and not by everything.** `Control+Super` survives every
AltGr keystroke, and it does fire while reaching for `Ctrl+Win+Left`, because a two-modifier chord
completes on the way to any longer chord that starts with the same two. That is general, affects the
default, and is tracked separately (KNEG-115) rather than folded in here.

### Mouse buttons: three of the five are refused, and that is the feature

Only `Mouse4` and `Mouse5` — the two side buttons — can be bound. Left, right and middle are refused
by name, with the reason on screen. The hook does not swallow the event, exactly as the keyboard
hook does not, so binding the left button would start the action on **every click in every
application** while the click still went through; an application that does that is indistinguishable
from a broken mouse. The two side buttons are the only ones with no universal meaning to destroy.

They are not *free*, either, and the settings screen says so rather than leaving it to be found:
because the click is not consumed, `Mouse4` still means Back and `Mouse5` still means Forward in
every browser on the machine. That sentence is why `Accelerator::MouseButton` is its own variant
rather than being folded into `ModifierOnly` — a hooked chord costs nothing elsewhere, and a hooked
mouse button costs whatever that button already did.

A mouse button cannot be combined with a modifier. `Control+Mouse4` is refused, and the refusal says
that no mechanism could hold it rather than implying a different spelling would: the shortcut
register cannot see a mouse button at all, and the hook's chord matcher only ever matches modifier
keys.

### Push-to-talk makes "insufficient speech" a routine outcome, not a rare one

This is the honest consequence and it belongs here rather than in a user's discovery. Under a
toggle, a recording ends when the user presses again, so it is almost always long enough to hold
speech. Under a hold it ends when the key comes up — and **a hold shorter than a syllable produces a
recording shorter than a syllable.** The audio gate then refuses to send it, correctly, and the
outcome reads *"Nothing was sent: insufficient speech."* Every mistimed tap, every accidental
prefix, every "I let go too early" ends that way. Someone who reads that line as a fault will go
looking for a fault that is not there.

It is also, for the same reason, part of why push-to-talk is a candidate answer to KNEG-115: an
accidental `Control+Super` on the way into `Ctrl+Win+Left` brackets a few hundred milliseconds
rather than bracketing everything the user does before the next accidental press. A toggle turns
that keystroke into a recording of the interval, transcribed and pasted; a hold turns it into an
empty recording that is discarded. **That trade is not made for anyone here.** The mode exists, its
default is unchanged, and KNEG-115 stays open.

The release edge is the whole of the mechanism: `keys::ChordWatcher::key_up` returns a `Fired` now
instead of `()`, and `shortcuts::act` is the single pure function that decides what a press or a
release means given the mode. `DictationRuntime::toggle` is unchanged — in `Hold` a press finds the
session idle and starts it, and a release finds it recording and finishes it, which is what `toggle`
already did.

One thing worth knowing about the implementation, because it is the kind of bug that only shows up
under a short hold: the press and the release are handled by **one spawned task**, not two. `toggle`
decides by reading the session state, and a press only sets `Recording` after the microphone is
actually open; two independently spawned tasks have no order between them, so a release arriving
during that window would find `Idle` and either leave the microphone open with nothing to close it
or start a second recording. The press therefore starts, waits on a per-press channel for the key to
come up, and finishes. That is reasoned rather than measured — the window exists and the design
removes it.

Two ways the release can fail to arrive, and they are handled differently. **Re-registering the
hotkeys** — which happens on every settings change — tears down the hook that was watching the key,
so `Hooks::install` ends any hold in progress before dropping it; the alternative is somebody who
changed a setting mid-hold being left recording with nothing to stop them. A **lost key-up** (a
locked session) is not detectable, so the recording runs to the maximum-length cap rather than for
ever, and `ChordWatcher::reset` deliberately does **not** synthesise a release: a watcher that has
lost track of the keyboard cannot also claim the key came up, and inventing one would stop a
dictation on a key the user is still holding.

**Push-to-talk off Windows is the plugin path only, and it has never been run.** On a hooked
accelerator — a modifier-only chord or a mouse button — push-to-talk needs the low-level hook, which
exists on Windows and nowhere else. On an ordinary accelerator it rides
`tauri-plugin-global-shortcut`'s release event through the same `shortcuts::act`, so it is
cross-platform code by construction; nothing in this repository has run it, on any platform
including Windows, and `register_one`'s own doc says so.

There is no `/dev/input` reader here and nothing falls back to toggle mode. `hotkey.<action>.mode`
is a stored per-action setting and no code consults the platform when reading it, so a `hold` on a
platform with no hook does not degrade to a toggle — the accelerator it applies to simply never
registers, and the screen says why.

## Text insertion

| Platform | Implementation state | What the application does |
| --- | --- | --- |
| Windows | **Built and verified** | Borrows the clipboard, sends `Ctrl+V` with `SendInput`, then restores the previous clipboard value only if the user has not replaced it in the meantime. The non-activating overlay leaves the target application focused; there is no separate foreground-window restore step. |
| macOS | **Not implemented** | `kw_platform::paste_keystroke()` returns `None`, so the application reports text insertion as unsupported. No `CGEvent` or Accessibility path is written. |
| Linux X11 | **Not implemented** | `kw_platform::paste_keystroke()` returns `None`. No XTEST or `xdotool` path is written. |
| Linux Wayland | **Not implemented** | `kw_platform::paste_keystroke()` returns `None`. No RemoteDesktop portal, `ydotool`, or `wtype` path is written. |

The clipboard-preservation algorithm is platform-neutral and unit-tested, but it is unreachable
off Windows until that platform has a real paste-keystroke backend. Candidate upstream mechanisms
remain recorded in `PORTING_MATRIX.md`; they are porting inputs, not claims about this build.

## Calendars

| Provider | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Google | yes ⚠️ | yes ⚠️ | yes ⚠️ |
| Microsoft 365 | yes ⚠️ | yes ⚠️ | yes ⚠️ |
| Apple | not applicable | not implemented | not applicable |

⚠️ The loopback PKCE flow, calendar listing and incremental event sync run on every platform, but
the `calendarList`/`events.list` (Google) and `/me/calendars`/`events/delta` (Microsoft) wire shapes
are covered by fixtures modelled on each vendor's public documentation — no client ID exists on this
machine, so no live call has been made. See the `calendar:google` and `calendar:microsoft` manifest
notes for the full accounting.

**Apple is "not implemented", not merely "unverified" — corrected by Task 19.** The two rows above
are unverified in the ordinary sense: real code, exercised against fixtures, waiting on a live
credential. Apple Calendar is a different claim entirely. `kw_calendar_core::apple`'s own module doc
says `AppleCalendarSource` is "deliberately unimplemented by anything in this crate" and that this is
true "on every platform this builds for today", including macOS — there is no macOS machine in this
development environment to write or verify an EventKit binding against, so nothing exists to be
unverified. `config/integration-manifest.json`'s `calendar:apple` entry carries `temporarily-deferred`
with a `deferral` block naming exactly this, corrected during Task 19's audit of every manifest status
that asserts a working implementation exists (it previously carried `platform-specific`, the same
false-implementation pattern `platform:aec` was already corrected for).

Calendar access is optional and is never required for meeting transcription.

## Local ASR: sherpa-onnx

`docs/BUILD_PLAN.md`'s chunk B4 recorded that an extracted sherpa model is a
directory of encoder/decoder/joiner/tokens files whose layout differs between
the Parakeet and Nemotron families, and that the CLI flags follow the layout
-- something that "cannot be written correctly from a release page". This is
the measurement, not a guess: `cargo run -p kw-transcription-core --example
sherpa_probe` installed the real pinned `sherpa-onnx-offline` CLI on Windows,
downloaded each pinned Parakeet archive in turn, and drove a real
transcription with each. `crates/transcription-core/tests/sherpa_dictation.rs`
pins the same finding behind a `--ignored` test against the production
`SherpaOffline` type (`cargo test -p kw-transcription-core --test
sherpa_dictation -- --ignored --nocapture`), so it stays true as the pinned
sherpa release moves rather than being true only on the day the probe ran.

**The measured directory layout.** Both pinned Parakeet archives --
`parakeet-unified-en-0.6b` and `parakeet-tdt-0.6b-v3` -- extract to a flat
directory, not a nested one:

```
sherpa-onnx-nemo-parakeet-unified-en-0.6b-int8-non-streaming/
  encoder.int8.onnx       (654,040,552 bytes)
  decoder.int8.onnx       (7,257,753 bytes)
  joiner.int8.onnx        (1,735,860 bytes)
  tokens.txt              (8,952 bytes)
  test_wavs/0.wav         (sample audio -- not part of the model)
  bias.md, explainability.md, privacy.md, safety.md
```

`parakeet-tdt-0.6b-v3` has the identical shape (`encoder.int8.onnx`,
`decoder.int8.onnx`, `joiner.int8.onnx`, `tokens.txt` at the top level, a
`test_wavs/` sample folder, no `.md` files). `kw_transcription_core::sherpa::
discover_transducer_files` looks for exactly these four files by name pattern,
directly inside the model directory, and refuses -- rather than guessing --
if one is missing or if more than one file could be it.

**The measured CLI contract.** `sherpa-onnx-offline --help`'s own usage block
for "(1) Transducer from icefall" is:

```
sherpa-onnx-offline \
  --tokens=/path/to/tokens.txt \
  --encoder=/path/to/encoder.onnx \
  --decoder=/path/to/decoder.onnx \
  --joiner=/path/to/joiner.onnx \
  --num-threads=1 \
  --decoding-method=greedy_search \
  /path/to/foo.wav [bar.wav foobar.wav ...]
```

which is what both probe runs actually invoked (`--num-threads` only, since
`kw_transcription_core::sherpa::build_args` omits it -- like whisper's own
`threads` field -- unless a caller chooses a value). A clean run prints one
line of JSON to stdout per input file, `{"text": "...", ...}`.
`sherpa::interpret` parses that shape the same way `local::interpret` parses
whisper-server's JSON. `timestamps` and `tokens` are read as of 2026-08-31 --
see **What the transducer reports** below. `ys_log_probs` is still unread
(nothing here reports confidence, and a threshold nobody measured would be
worse than silence) and `durations` and `words` come back empty.

There is **no persistent server for the offline CLI**: `sherpa-onnx-offline`
reads one WAV path and exits, unlike `whisper-server`'s HTTP endpoint on a
port. `SherpaOffline::transcribe` writes the audio to a temp file and spawns
a fresh child per request rather than holding a `port` the way `WhisperServer`
does.

### The streaming server, and a protocol that had to be measured

`sherpa-onnx-online-websocket-server` — a different binary out of the same
archive, already extracted beside the CLI — *is* a persistent server, and since
2026-09-01 it is the runtime behind the `local-streaming` meeting provider.
It is not an HTTP endpoint, so there is no `/health` to probe: readiness is a
real WebSocket handshake, because a bare TCP connect answers "something is
listening" and a half-initialised server answers that too.

**The wire format is undocumented and one documented fact about it is wrong.**
`crates/meeting-core/examples/sherpa_ws_probe.rs` measured both directions
against the real binaries, with the reference client reproducing the model's
own `test_wavs/trans.txt` first as the control:

| | measured |
| --- | --- |
| sample format | **`f32` little-endian** |
| frame | BINARY, 8000 samples, then a short tail |
| client end marker | TEXT `Done` |
| server end marker | TEXT `Done!`, and it never closes the socket |
| transcript `text` | **cumulative** within a segment — replace, never append |
| a malformed frame | **no reply at all** |

`sherpa-onnx-online-websocket-client --help` says *"16-bit samples"*. As `f32`
the values are speech inside ±1; as `i16` they are noise. A client written
from the documentation would connect, send, and transcribe nothing.

The last row decides how faults are reported. There is **no error frame**, so
`kw_meeting_core::fault::FaultKind::Reported` is unreachable for this provider
and a client claiming to surface "what the provider said was wrong" would be
describing a channel that does not exist. That is the inverse of the four
cloud clients, which is why it was measured rather than assumed.

**Verified on 2026-09-01:** a real meeting was recorded through it in the
application window and the words arrived **as they were spoken** — which is the
whole difference from the windowed provider beside it, and the reason this one
is allowed to use the word "streaming".

**What that run did not cover:** *Stop and transcribe*, and the note it saves.
`meeting.rs`'s `keep` runs a whole-file pass rather than keeping the live
partials, so words appearing on screen says nothing about what ends up in the
note.

**A real bug this measurement caught.** `config/provider-registry.json`'s
`sizeMb` for a sherpa model (631 for Parakeet Unified EN 0.6B) is **the
extracted model's footprint on disk, not the compressed `.tar.bz2` download**:
the real download was 501,350,460 bytes (~478 MB) and the real extracted
total is ~663 MB. An early version of `sherpa_models::install` checked the
*download* against `sizeMb` with the same 5% tolerance `models.rs` uses for
whisper models, and it failed every install -- a 25-30% gap does not fit a 5%
tolerance. The fix does not size-check the download at all; instead, after
extraction, `discover_transducer_files` has to find the real files, which is
a stronger and more honest check than a byte-count guess: a truncated or
corrupted archive either fails to extract (`tar`/`bzip2-rs` notice a
malformed stream) or extracts without a complete file set, and either way the
install is refused before the model is treated as ready.

**The transcript, compared to whisper on the same input.**
`tests/fixtures/speech-16k-mono.wav` (the fixture `local_dictation.rs` already
used for whisper) was transcribed by all three: whisper's pinned `tiny` model
said

> The Quick Brown Fox jumps over the lazy dog. This is a test of local transcription.

and **both** pinned Parakeet models said

> The quick brown fox jumps over the lazy dog. This is a test of local transcription.

-- identical words, differing only in whisper's mid-sentence capitalisation.
Real-time factor was 0.109-0.124 on this development machine (single thread),
against whisper `tiny`'s comparable speed. Quality was not materially worse
than whisper's on this input. One fixture is not a benchmark suite, and
nothing here claims Parakeet is whisper's equal on noisy audio, accents, or
long-form speech -- only that on the one real input this project has a
known-good transcript for, it was not worse.

**What is proven, and why that is not the same as reachable.**
`kw_transcription_core::sherpa::SherpaOffline` (the second `LocalTranscriber`)
and `sherpa_models::install` are real, tested code, exercised end to end
against the real pinned binary and both real pinned Parakeet archives. What
does **not** exist yet is the Tauri command layer and Settings UI wiring that
would let a user install and select a Parakeet model from Settings >
Transcription > Local the way they already can for a whisper model --
`apps/desktop/src-tauri/src/dictation.rs`'s own comment on `LocalTargets`
already names this as the anticipated next step ("the next one is the
sherpa server"), and `SettingsTranscription.tsx` names the gap directly to
the user rather than staying silent about it. So
`local-asr:parakeet-tdt-0.6b-v3` and `local-asr:parakeet-unified-en-0.6b`
stay `reachable: false` in `config/integration-manifest.json`: this
project's settled reading of `reachable` (the same one applied to
`calendar:apple`) is that a person must be able to configure the integration
through the named, *built* surface, and today nobody can pick a Parakeet
model from `Settings > Transcription > Local` no matter how well the
runtime underneath it works. The note on both entries was rewritten to
record the real, narrower gap -- a missing selector, not a missing runtime --
rather than the stale "blocked on the archive format" reason from before
this task.

**The two Nemotron models stay unreached, on purpose.** Both
(`nemotron-3.5-asr-streaming-0.6b`, `nemotron-speech-streaming-en-0.6b`) are
*streaming* models over sherpa's own bespoke WebSocket protocol
(`sherpa-onnx-online-websocket-server`, confirmed present in the CLI sidecar's
`bin/` directory -- see below), not the offline CLI this measurement drove.
Nobody has written or tested a client for that protocol, so
`config/integration-manifest.json` keeps both `reachable: false`, with the
note updated to say this specifically rather than repeating the now-false
claim that the archive format itself was the blocker (Task 17 fixed that).

**Installing the sherpa-onnx CLI extracts the whole `bin/` directory.**
Confirmed by the probe on Windows: 23 files, not just
`sherpa-onnx-offline.exe` -- `onnxruntime.dll`, `onnxruntime_providers_shared.dll`,
and 20 other `sherpa-onnx-*.exe` tools (keyword spotting, audio tagging,
denoising, TTS, the online and offline WebSocket servers, `sherpa-onnx.exe`
itself, and more). This is `install::extract`'s existing payload-directory
model doing what it always does -- installing everything beside the named
executable, the same as it does for `whisper-server`'s sibling `ggml`
libraries -- not a defect specific to sherpa. It is also why the Linux and
macOS archives are 230-385 MB against the Windows CLI's 24 MB: those targets
ship the "static" build (see `config/sidecars.json`'s `sherpa` entry), which
statically links `onnxruntime` into every one of those ~23 executables rather
than sharing one `onnxruntime.dll` beside them the way the Windows "shared"
build does.

### What the transducer reports

Measured 2026-08-31, `parakeet-unified-en-0.6b-int8`, the same 13.5 s of speech
the whisper timing measurement below used. Verbatim, with the three long arrays
elided:

```text
{"lang": "", "emotion": "", "event": "",
 "text": "The quick brown fox jumps over the lazy dog. Second sentence, ...",
 "timestamps": [0.64, 0.72, 0.72, 0.88, ...],            // 64 entries
 "durations": [],
 "tokens": [" The", " qu", "ick", " br", "own", ...],    // 64 entries
 "ys_log_probs": [...],                                  // 64 entries
 "words": []}
```

Three things the field names alone did not say, and each one changed the design:

1. **`timestamps` is per token, not per segment** — 64 for 64 tokens, and the
   tokens are subword pieces (`" qu"`, `"ick"`). There is **no segmentation in
   this output at all**, so `sherpa::cues` builds it. That is the same job
   whisper.cpp's *server* does for us from its own decoded timestamp tokens.
2. **`words` and `durations` are empty** for a transducer model.
   `--whisper-enable-segment-timestamps` and `--whisper-enable-token-timestamps`
   exist on this CLI but are Whisper-family options; Parakeet is a transducer and
   neither applies. So there is no flag that fills them in, and **only onsets are
   available — never offsets**.
3. **A sentence-final `"."` is timestamped at the end of the pause that follows
   it**, not where the speech stopped:

   ```text
   token   'y'   ' do'  'g'    '.'    ' Se'  'cond'
   onset   2.32  2.64   2.72   4.08   4.24   4.32
   ```

   The `"."` sits 1.36 s after `"g"` and 0.16 s before the next word, because the
   decoder emits it only once it has seen the silence. Ending a cue on it would
   hold every cue on screen through the whole pause — 1.2 s of dead air per cue.
   So a cue ends at its last token carrying a letter or digit.

Against the same audio, whisper's own segmenter reported
`0.00-3.00 / 3.68-9.44 / 10.08-12.88` and this rule gives
`0.64-2.72 / 4.24-9.12 / 10.56-12.40` — the same three utterances, each a
fraction of a second tighter, because an onset is not an offset.

**Every boundary is a reported onset.** Nothing is interpolated or derived from
the audio length, and a test asserts each cue's start and end appear verbatim in
`timestamps`. Cues split after sentence-final punctuation, or at a 200-character
backstop that exists because a transducer has no decode window the way whisper
does — and only ever at a word start, which is the protection `--split-on-word`
gives whisper, unconditional here.

## Local ASR: the health check, and a busy server

Measured 2026-08-31 by `crates/transcription-core/tests/local_dictation.rs`, which
now stops the real `whisper-server` and starts it again. The numbers matter
because `2fe5825` put this probe in front of **every** local dictation and media
import.

| what is probed | `HttpHealth::is_ready` |
| --- | --- |
| a live, idle server | ~1 ms |
| a live server **mid-transcription** | 5,235 probes, **0** reported not-ready, slowest **1.09 ms** |
| a port whose server was just stopped | **2.004 s** |
| a port that never had a server | **2.004 s** |

**A busy server answers.** This was the risk worth checking before trusting the
recovery path: `whisper-server` serves `GET /` concurrently with inference, so a
server that is working cannot be judged gone and have a second one started on top
of it. Probed 5,235 times during a real transcription without a single false
negative.

**A dead port costs the full two-second timeout, not a connection refused.** Both
the just-vacated and the never-used port took 2.004 s, so this is not a lingering
socket — a loopback connect to a closed port simply does not fail fast through
`NetworkPolicy::loopback` here. What that means in practice:

* A healthy dictation pays about a millisecond. Nothing to notice.
* A dictation or import that has to recover waits two seconds before the restart
  even begins, then waits for the model to load. That replaced *failing
  outright*, so it is still the better outcome — but "the health check is a
  loopback round trip" understates it on the path that matters.

The evidence would support a shorter timeout for a liveness probe specifically —
a busy server answers in about a millisecond, so 500 ms would be safe and would
make recovery four times snappier. Not changed: `HttpHealth`'s two seconds is
shared with `process::start`'s startup polling, where a model still loading needs
the patience, and splitting the two is a change nobody has needed yet.

## Local ASR: whisper-server timings

Measured on 2026-08-31, Windows 11 Pro 26200, against `ggml-tiny` and 13.5 s of
synthesised speech, by starting the pinned `whisper-server.exe` by hand on a
spare port and posting the same WAV to `/inference` under each configuration.

**The question.** A 3,401-second Swedish import exported as a single subtitle
cue at `00:00:00`, and the recorded suspicion was that `media.rs` failed to
divide it. That was wrong. The server had not been asked for any timings, so
there was nothing to divide.

| server flags | `response_format=json` | `response_format=verbose_json` |
| --- | --- | --- |
| `--no-timestamps` (as shipped until 2026-08-31) | 0.63 / 0.65 / 0.65 s | **one** segment for the whole recording, carrying neither `start` nor `end` |
| `--max-len 1000` | 0.66 / 0.66 / 0.70 s | 0.95 / 0.95 / 1.01 s, **one segment per utterance** with real `start` and `end` |

Three runs each, `curl -w %{time_total}`.

**What was ruled out.** Neither `split_on_word` nor `max_len` is honoured as a
request field: sending `max_len=0` and `split_on_word=true` as form fields
returned byte-identical responses to sending neither. They are start-time flags.
`--max-len 0` at start time also changes nothing, because whisper.cpp's server
reads `0` as `60` — which is where the sixty-character mid-utterance wrap that
`--no-timestamps` was ported to remove actually comes from. An explicit cap far
above what a 30-second decode window can hold removes the wrap by never
reaching it.

**What this fixed and what it did not.**

- Media import now asks for `verbose_json` (`WhisperServer::timed`) and gets one
  cue per utterance. Dictation and meetings still ask for `json`.
- Dictation's output is **unchanged**, which was checked rather than assumed: the
  `\n` that now separates utterances in `text` is collapsed by
  `local::interpret`, and the result is byte-identical to what `--no-timestamps`
  returned for the same audio.
- **A cloud provider still reports no timings**, so an import routed to one gets
  the single honest fallback cue the screen writes. Only the local whisper server
  is fixed.
- **`sherpa-onnx-offline` already prints `timestamps` and `words` on stdout** and
  `sherpa::interpret` does not read them — see the sherpa section above, which
  has listed those as unread fields since the probe. That is now a gap with a
  known cure rather than an unexamined one.

## Local ASR: the model pins, measured

Measured on 2026-08-31, Windows 11 Pro 26200, by `node tools/model-pin/pin.mjs
--url <downloadUrl> --id <id>` for each model in turn: the bytes were downloaded
in full, hashed on the way through to a file, and the length read from `stat`
after the stream closed. Never `content-length` — a truncated body agrees with
its own header and disagrees with itself. 6,966,596,962 bytes total, 6.49 GiB,
14 min 33 s of transfer.

| model | file | SHA-256 | measured bytes |
| --- | --- | --- | --- |
| `tiny` | `ggml-tiny.bin` | `be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21` | 77,691,713 |
| `base` | `ggml-base.bin` | `60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe` | 147,951,465 |
| `small` | `ggml-small.bin` | `1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b` | 487,601,967 |
| `medium` | `ggml-medium.bin` | `6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208` | 1,533,763,059 |
| `turbo` | `ggml-large-v3-turbo.bin` | `1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69` | 1,624,555,275 |
| `large` | `ggml-large-v3.bin` | `64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2` | 3,095,033,483 |

All served by `us.aws.cdn.hf.co`, which is where a `huggingface.co/.../resolve/…`
request redirects. The pins live in `WHISPER_PINS` in
`tools/integration-audit/generate-provider-registry.mjs` and reach the
application as `WhisperModel::sha256` / `expected_size_bytes`.

**`tiny` reproduced the pin that was already committed**, byte count and digest
both, from a tool that had not seen it. That is what makes the other five
trustworthy: the same run validated the instrument.

Both Parakeet archives were re-measured the same way and the same day —
487,170,055 bytes (464.6 MiB) and 501,350,460 bytes (478.1 MiB), digests
unchanged. Before this, only their lengths had been checked, against
`content-length`.

**What this proves and what it does not.** Every one of these eight digests
matches the bytes the host serves today. That is the check, run once, and it says
nothing about who built those bytes.

About tomorrow the two families now differ, and the difference is the whole of
KNEG-103. **The six whisper URLs name a commit** — `5359861c739e955e79d9a303bcbc70fb988958b1`,
measured into `HF_REVISIONS` on 2026-08-31 — so a push to `ggerganov/whisper.cpp`
cannot move the bytes these digests describe. **The two Parakeet archives sit in
k2-fsa's rolling `asr-models` release**, where an asset can be replaced under the
same name, and **no immutable URL exists for them** — that was measured rather
than assumed, and the measurements are in `ENGINEERING_LOG.md` under 2026-08-31
(KNEG-112). What they have instead is a digest stated by the host:

**GitHub's release API returns a **`digest`** and a **`size`** per asset**, one
request for all 498 in the release. On 2026-08-31 both stated digests and both
stated sizes matched the committed pins to the character — independent
corroboration of a measurement, from the publisher's own metadata, at no
bandwidth cost. `node tools/model-pin/pin.mjs --verify-stated` re-reads them
weekly beside the Hugging Face trees.

That is the same standard the thirty-two stated GGUF pins meet and it is
*weaker* than how these two were pinned, since their bytes were hashed here. So
agreement there means the asset has not been swapped, not that the bytes are
still what we hashed; only the weekly `--verify`, which re-downloads about a
gigabyte, proves the second thing. Having both is what separates "the pin is
stale" from "the download was damaged" in seconds rather than after a
gigabyte.

**Consequence for an install.** All six models are now held to
`kw_sidecar::integrity::verify`, so the *Verified by* column reads `digest` for
every row rather than `size` for five of them. `tiny` is the only one whose whole
round trip has been driven — `cargo test -p kw-transcription-core --test
local_dictation -- --ignored` downloads it, asserts `DigestMatched`, starts the
server and transcribes. The other five have never been installed by any route;
that is a by-hand item in [BENCH_CHECK.md](BENCH_CHECK.md) and KNEG-105.

## GPU acceleration

| Backend | Windows | macOS | Linux |
| --- | --- | --- | --- |
| CUDA (NVIDIA) | downloadable pack | not applicable | downloadable pack |
| Vulkan (AMD / Intel / NVIDIA) | downloadable pack | not applicable | downloadable pack |
| Metal | not applicable | native | not applicable |
| CPU fallback | always | always | always |

GPU packs are downloaded separately with a verified digest. First Vulkan start needs a long
timeout — shader compilation can take well over a minute. On failure the app falls back to CPU
and records why.

## Updates

`apps/desktop/src-tauri/src/updater.rs` checks this project's own GitHub
releases, opt-in and only when a person presses "Check for updates" on
Settings > Diagnostics -- never at startup, never on a timer. It has never
been exercised end to end, for one specific reason rather than a general
disclaimer: **no release has ever been published.**
`.github/workflows/release.yml` is `workflow_dispatch`-only and nobody has
run it, so `GET /repos/{owner}/{repo}/releases/latest` has only ever had one
possible answer, 404, on every platform.

Everything downstream of that fact is unverified in the ordinary sense of the
word -- believed rather than observed -- and each piece is marked at its
point of use rather than only here:

- **The response shape `updater.rs` parses** (`tag_name`, `html_url`) is
  GitHub's documented shape for that endpoint, not one this code has seen.
- **Whether `tauri build` tolerates `bundle.createUpdaterArtifacts: true`
  with no `TAURI_SIGNING_PRIVATE_KEY` present**, rather than refusing to run
  at all, has never been checked on any of the three CI runners. The
  workflow defends against the worse of the two possibilities by overriding
  the setting back off when the secret is absent, but which possibility is
  actually true is unconfirmed either way.
- **Whether a real GitHub release actually carries the `.sig` files the
  bundler would produce**, once a signing key exists, in a shape
  `actions/upload-artifact`'s glob and `softprops/action-gh-release`'s
  `files: bundles/**/*` both pick up -- untested, because no signing key and
  no release exist yet.
- **The version comparison itself** (`semver::Version`, with a published tag's
  leading `v` stripped) is exercised by unit tests against invented tags, not
  against a tag this project has actually cut.

None of this bears on whether the application works without an update check;
it bears only on whether that specific, opt-in feature would behave as
designed the first time somebody actually uses it. See `docs/SECURITY.md` for
the keypair procedure and `docs/COMPLETION_PLAN.md` phase 9 for what remains.

## Known environment constraints

- **A non-ASCII character in the path of a file the native ASR binaries open** stops them opening
  it. Both `whisper-server` and `sherpa-onnx-offline` exit 127; whisper says nothing at all past
  `loading model from ...`, sherpa says `Errors in config!` for a model and `Failed to read '...'`
  for audio. The model cache and the audio scratch directory relocate to `%ProgramData%` when the
  user data path is affected, which any Windows account with a non-ASCII username will be.

  **Measured 2026-08-31, and it is narrower than this section claimed until then.** The rule was
  carried from upstream as "non-ASCII **or space**", applied to the binaries' own directory as well
  as to the files they read. Three quarters of that was not true:

  | | a space in the path | a non-ASCII character |
  | --- | --- | --- |
  | the model file | loads, transcribes | **fails** |
  | the audio file | transcribes | **fails** |
  | the binary's own directory | runs | runs |

  `whisper-server` loads its sibling `ggml-cpu-*.dll` out of a directory named `Håkan Test`
  without complaint, and so do `sherpa-onnx-offline`, `llama-server` and `ffmpeg` — four out of
  four, so `sidecar_dir` no longer relocates at all. `ffmpeg` handles every case, including a
  non-ASCII input path, which is the media-import half of the same question.

  Re-runnable: `cargo run -p kw-storage --example native_path_probe`. **Still unmeasured:** a real
  Windows account whose *own name* is non-ASCII, as opposed to a directory underneath an ASCII one.
  The relocation is what that would exercise, and nobody has an account like it.
- **`tauri-nspanel` is an unpinned git branch dependency** upstream. We pin it to a revision;
  a branch move would otherwise break macOS builds with no version change.
