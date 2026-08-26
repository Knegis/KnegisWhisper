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
| Windows 10 2004+ | `active` | `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`, then **`GetWindowDisplayAffinity` read back** and compared. `active` is reported only when the read-back matches. |
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
| Transparent, undecorated | yes | yes | yes | compositor-dependent |
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

| Provider | Client | Verified |
| --- | --- | --- |
| Local windowed model | repeated `/inference` passes over the local `whisper-server` | ⚠️ built, never run by a person |
| OpenAI Realtime | two WebSockets, one per capture | ⚠️ request shaping and the event grammar against fixtures. **No live call.** A key exists on this machine and has not been used |
| AssemblyAI Realtime | two WebSockets, one per capture | ⚠️ request shaping and the event grammar against fixtures, and most of the wire shape **believed rather than confirmed** -- see below. **No live call and no key for this provider exists on any machine this project has touched.** |
| Deepgram Realtime | — | not written |
| Corti Realtime | — | not written |
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
| Microsoft Teams | `ms-teams.exe`, `teams.exe`, `msteams`, `teams` | — | **`ms-teams.exe` seen to match** (2026-08-26, 628 processes). New Teams runs the call inside its own process, so there is no in-call signal |
| Zoom | `zoom.exe`, `zoom`, `zoom.us` | `cpthost.exe`, `caphost` | ⚠️ written, not seen to match |
| Webex | `webex.exe`, `webexhost.exe`, `ciscocollabhost.exe` | `webexmta.exe` | ⚠️ written, not seen to match |
| FaceTime | `facetime` | — | ⚠️ macOS only, compiled out elsewhere, unverified |
| Anything else | the user's own allowlist | — | works by construction |

**Only an in-call process offers to record**, and that is a measurement rather
than a preference. Teams was found running with no call in progress, and Teams
starts with Windows on most corporate machines — so offering on "the client is
open" would put a window on screen at every launch, and the feature would be
switched off the same day. Teams and FaceTime are therefore reported on
`Settings > Meetings` and never prompted for, which is a limit worth stating
rather than leaving somebody to discover.

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
| Standard accelerators | yes | yes | yes | gsettings + D-Bus | KGlobalAccel | `hyprctl` |
| Modifier-only (`Control+Super`) | native hook | native hook | limited | native slot | native slot | native slot |
| Right-side modifiers | native hook | native hook | limited | limited | limited | limited |
| Globe / `Fn` | not applicable | native hook | not applicable | not applicable | not applicable | not applicable |
| Mouse buttons 4/5 | native hook | native hook | limited | limited | limited | limited |
| Push-to-talk | native hook per key | native hook | `/dev/input` access required | portal-dependent | portal-dependent | portal-dependent |

The default hotkey is `Control+Super`, which is modifier-only and therefore invisible to the
cross-platform API — hence the native hooks. On X11 and GNOME the effective default is adjusted
because modifier-only combinations do not work there.

**What is built, as against what the table says is possible:**

| | State |
| --- | --- |
| Windows modifier-only | **verified.** One `WH_KEYBOARD_LL` hook on one thread, in-process, not one child per key. `cargo run -p kw-platform --example keyhook_probe` installs it, injects two presses, and asserts it fires exactly twice. |
| Windows right-side modifiers | **Built and unreachable.** `keys.rs` expresses `RightControl+Super`, because a low-level hook reports the sided virtual keys and the cross-platform API does not — but no accelerator a user can type gets there. `hotkey::MODIFIERS` has no sided spellings, so `classify` reads `RightControl` as *the key* and hands `RightControl+Super` to the system register, the one mechanism that cannot tell the sides apart; and `RightControl+RightShift` comes back `Invalid` — "it names 2 non-modifier keys" — which blames the user's spelling for a missing alias. See [BENCH_CHECK.md](BENCH_CHECK.md). |
| Windows mouse buttons, push-to-talk | Not built. Both need a second hook (`WH_MOUSE_LL`) or key-up routing, and neither has a consumer yet. |
| macOS, Linux | Not built, and the reason is reported rather than the capability being silently absent: macOS needs a `CGEventTap` and the Accessibility permission that goes with it, and Wayland's global-shortcuts portal takes only chords the compositor can register. |

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

**A modifier-only chord fires on the second modifier going down**, which is what makes it feel
immediate and also means a key pressed *afterwards* cannot call it back. The documented rule that an
ordinary key disqualifies a modifier-only chord catches a key already held — somebody mid-word — not
one pressed a moment later. The consequence measured on 2026-08-26: on a layout where AltGr is
left-Control plus right-Alt (Nordic, German, and others), binding `Control+Alt` starts a dictation
every time the user types `@`. The shipped default, `Control+Super`, is unaffected, and nothing
warns anyone who chooses `Control+Alt`.

Push-to-talk on Linux needs read access to `/dev/input`; without it the app reports the
limitation and falls back to toggle mode rather than silently failing.

## Text insertion

| Capability | Windows | macOS | Linux X11 | Linux Wayland |
| --- | --- | --- | --- | --- |
| Insert at cursor | SendInput | CGEvent ⌘V | XTEST / `xdotool` | portal RemoteDesktop, `ydotool`, `wtype` |
| Restore foreground app | by HWND | by PID | by window id | compositor-dependent |
| Preserve clipboard | yes | yes | yes + PRIMARY selection | yes + PRIMARY selection |
| Permission required | none | **Accessibility** | `/dev/uinput` for `ydotool` | portal grant |

Without macOS Accessibility permission, dictated text is copied to the clipboard but not
inserted, and the user is told so. On Wayland, insertion depends on a portal implementation being
present; where it is not, the app reports `unsupported` rather than appearing to work.

## Calendars

| Provider | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Google | yes | yes | yes |
| Microsoft 365 | yes | yes | yes |
| Apple | not applicable | EventKit — unverified | not applicable |

Calendar access is optional and is never required for meeting transcription.

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

## Known environment constraints

- **Non-ASCII or space characters in the user data path** crash the native whisper and parakeet
  binaries. Upstream works around this by relocating the model cache to `%ProgramData%`; we keep
  that fallback. It will bite on any Windows account with a non-ASCII username.
- **`tauri-nspanel` is an unpinned git branch dependency** upstream. We pin it to a revision;
  a branch move would otherwise break macOS builds with no version change.
