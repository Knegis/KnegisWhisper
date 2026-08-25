# Completing phases 0–6

A tracked plan, maintained in place. When something here becomes false, edit it rather than
appending a note. The reasoning behind individual choices lives in
[ENGINEERING_LOG.md](ENGINEERING_LOG.md); this file is about what is left to do.

## Where each phase stands

Status words mean something specific. **Complete** means it works and a test proves it.
**Unreachable** means it works and a test proves it, but no user can get to it. **Contract only**
means the types exist and the implementation does not.

| Phase | Status | Evidence |
| --- | --- | --- |
| 0 — Audit and contracts | Complete | `docs/UPSTREAM_AUDIT.md`, `config/integration-manifest.json`, the parity and domain gates |
| 1 — Tauri foundation | Complete | Tray, autostart, shortcuts, overlay, capture exclusion, credential store, FTS all real; `apps/desktop/src-tauri/src/` and `docs/PLATFORM_SUPPORT.md` |
| 2 — Dictation | **Unreachable** | `crates/transcription-core/tests/local_dictation.rs` transcribes real speech end to end in ~6 s; nothing in the app starts it |
| 3 — Reasoning | Complete except enterprise | `crates/reasoning-core/src/chat.rs:191` returns `ShapeNotImplemented` for Bedrock, Azure, Vertex |
| 4 — Meetings and notes | Notes complete; meetings contract only | `crates/notes-core/`; `crates/meeting-core/src/lib.rs` documents the five missing clients. No system audio exists — `grep -r wasapi crates/` is empty |
| 5 — Calendars | Not started | No `crates/calendar-core`, no OAuth flow |
| 6 — Local API and CLI | **Unreachable** | `crates/local-api/tests/api.rs` (20) and `tools/knegiswhisper/tests/end_to_end.rs` (16, driving the compiled binary); nothing in the app starts the server |

### Why two phases are "unreachable"

`apps/desktop/src-tauri/Cargo.toml` depends on 7 of the 14 workspace crates. It does not depend on
`app-core`, `audio-core`, `transcription-core`, `reasoning-core`, `notes-core`, `sidecar`, or
`local-api`. `shortcuts.rs:167` emits a hotkey event that nothing consumes.

That is the whole gap. It is a wiring problem, not a missing-feature problem, which is why it is
the first chunk.

## Three defects found while surveying

Each is fixed by chunk 1 rather than tracked separately.

1. **Two modules own one table.** `crates/storage/src/transcriptions.rs` and
   `crates/notes-core/src/history.rs` both read and write `transcriptions`, with two
   `Transcription` types and two `NewTranscription` types. The Tauri commands use the first, the
   local API the second. Neither is a superset: storage has `Status`, failure recording, `clear`
   and `count`; notes-core has cursor paging, a redacted `Debug`, and the dictionary.
2. **A live frontend fiction.** `apps/desktop/src/lib/ipc.ts` declares `Transcription` in
   camelCase, but the Rust struct carries no `rename_all`, so it sends `error_message` where the
   UI reads `errorMessage`. `App.tsx:333` therefore renders `(empty)` instead of the reason a
   dictation failed. Same class as the `electronPresent` bug in
   [ENGINEERING_LOG.md](ENGINEERING_LOG.md) — a field the backend never sends reads as
   `undefined` and renders something confident and wrong.
3. **A guarantee with nothing behind it.** `OverlayStatus::recording_indicator_visible`
   (`overlay.rs:38`) is a hardcoded `true`, because nothing records. The brief requires the local
   user to see when the microphone is live; wiring dictation is what turns that field into a fact.

## The work

### Chunk 1 — consolidate, then wire *(in progress)*

The chunk that turns phases 2 and 6 from unreachable into usable.

- **1a. One owner per table.** Merge into `storage`, not out of it: the schema and `settings.rs`
  already live there, and `notes-core` should stay about notes. `storage::transcriptions` gains
  cursor paging, the redacted `Debug`, `AudioForgotten`'s three outcomes, and
  `rename_all = "camelCase"` (fixing defect 2). The dictionary moves to a new
  `storage::dictionary`. `notes-core::history` is deleted and its callers repointed.
- **1b. Adapters for the six trait seams.** `crates/app-core/src/dictation.rs` defines `Recorder`,
  `Transcriber`, `Cleanup`, `Inserter`, `History`, `StateSink`, implemented today only by test
  fakes. Real ones go in `apps/desktop/src-tauri/src/dictation/`, reusing
  `audio_core::capture::Capture` (real cpal), `transcription_core::{route, Executor, WhisperServer}`,
  `reasoning_core::chat::cleanup`, and `platform::insertion::{TextInserter, SystemClipboard}`.
  The one genuinely missing piece of platform code is a `Keyboard` implementation:
  `platform::window::send_ctrl_v` exists, but nothing implements the trait.
- **1c. Sidecar and model lifecycle.** `AppState` gains the catalogue, the PID store, and the
  running server. Stale sidecars are reaped on boot before anything touches a port. Commands to
  check, install (with progress), start, and stop.
- **1d. Local API lifecycle.** Off unless a setting says otherwise. Start allocates from
  `PORT_RANGE`, serves, writes the discovery file and surfaces the `Permissions` value it returns
  verbatim; stop removes it.
- **1e. The hotkey does something.** `HotkeyAction::Dictation` drives the session. The overlay is
  shown for the whole recording, and `recording_indicator_visible` becomes a real read of that
  (fixing defect 3).
- **1f. Frontend.** Three sections — local transcription, dictation, local API — and only types
  Rust actually sends.

### Chunk 2 — close out phase 3

Bedrock, Azure, and Vertex request shapes. SigV4 signing and body shaping are unit-testable
against AWS's published test vectors, so this is **not blocked** — only a live call is.

### Chunk 3 — system audio (phase 4)

WASAPI loopback on Windows behind a trait; macOS (`cidre` process taps, 14.4+) and Linux
(PulseAudio monitor) written and marked unverified. Nothing exists today. Prerequisite for
meetings, and independent of the meeting clients.

### Chunk 4 — calendars (phase 5)

Loopback PKCE against Google and Microsoft, refresh tokens as `token_ref` into the OS credential
store — never the plaintext columns upstream used — and the UI. Buildable and testable against a
stub authorisation server; only the real round trip needs client IDs.

### Chunk 5 — meeting clients (phase 4)

Blocked. See below.

## Blocked, and what unblocks it

Nothing here blocks anything in chunks 1–4.

| Item | Needs | What is *not* blocked by it |
| --- | --- | --- |
| Anything macOS-native | A Mac, or someone with one | It compiles in CI and ships marked unverified |
| macOS `whisper-server` | A macOS CI job publishing under this project's own releases, which needs Actions release permissions. Upstream publishes only an xcframework — a library for Xcode, not a server | Windows and Linux install from upstream `ggml-org` today, digest-pinned |
| The five meeting WebSocket clients | Live keys for AssemblyAI, Deepgram, OpenAI Realtime, Corti, Tinfoil | The capability model and allowlist are done; system audio and meeting detection are not blocked |
| Calendars, real round trip | Google and Microsoft OAuth client IDs, registered as PKCE **public** clients — upstream ships a client secret in the binary, which in an open-source build is not a secret | The flow, storage, and UI |
| Enterprise reasoning, live calls | AWS, Azure, or GCP organisation credentials | Request shaping, tested against published vectors |
| Signed installers | A code-signing identity | Unsigned builds |

## Decisions taken without asking

Recorded so they can be reversed cheaply.

- **The local API is off by default**, opt-in per run. It is an unauthenticated-by-transport
  surface exposing every note and transcript to any local process; that should not be listening
  unless someone asked for it.
- **Model installs are explicit.** Nothing downloads on first launch. A first run that quietly
  fetches 142 MB is a surprise, and the brief's "no startup request" rule is easier to keep
  honest when nothing is automatic.
- **`nircmd` is dropped rather than pinned.** It was only the third-tier Windows paste fallback,
  it is fetched unversioned and unhashed, and `SendInput` works.
- **Dictation history lives in `storage`, not `notes-core`.** See 1a.

## Verification

Every chunk keeps all of it green: `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`,
`tsc --noEmit`, `tools/domain-scanner` and its self-test, and the parity gate.

Chunk 1 adds: each adapter tested against the `app-core` state machine; the overlay visible for
the whole of a recording; the local API not starting unless the setting says so; the discovery
file removed on stop.

**And by hand on Windows, which is the only thing that actually proves it.** Airplane mode on.
Hotkey in Notepad and in VS Code, speak, text lands at the cursor. Clipboard identical before and
after. The dashboard never takes focus. A history row appears; disabling history stops it. Then
`knegiswhisper health` and `knegiswhisper history list` against the running application.

The parity gate will want `developer:local-rest-api` and `developer:cli` marked reachable once
they have a UI — deliberately not before, because a backend integration that cannot be configured
is one of the six conditions that gate exists to catch.
