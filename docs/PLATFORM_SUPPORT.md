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

## Capture exclusion

Reported as one of four states, never a boolean:

| State | Meaning |
| --- | --- |
| `active` | Exclusion applied **and positively verified**. |
| `unsupported` | The platform or compositor cannot do it. |
| `failed` | The attempt returned an error. |
| `unknown` | Applied, but the result cannot be read back. |

| Platform | Expected state | Mechanism |
| --- | --- | --- |
| Windows 10 2004+ | `unknown` | `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`. The call reports success, but there is no documented getter that confirms the affinity took effect, so we report `unknown` rather than `active`. Some capture paths bypass it regardless. |
| Windows < 10 2004 | `unsupported` | API unavailable. |
| macOS | `unknown` | `NSWindow.sharingType = NSWindowSharingNone`. Same reasoning. |
| Linux (all) | `unsupported` | No-op. Neither X11 nor the Wayland compositors expose per-window capture exclusion. |

Reporting `unknown` where Pluely's README promises "absolute stealth" is deliberate. Pluely's
entire mechanism is the same `contentProtected` flag, it never verifies the result, and it is a
no-op on Linux — yet the README carries no Linux caveat. We would rather under-claim.

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

Push-to-talk on Linux needs read access to `/dev/input`; without it the app reports the
limitation and falls back to toggle mode rather than silently failing. Windows does the same when
the key-listener helper is missing.

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
