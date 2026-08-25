# Completing phases 0–6

A tracked plan, maintained in place. When something here becomes false, edit it rather than
appending a note. The reasoning behind individual choices lives in
[ENGINEERING_LOG.md](ENGINEERING_LOG.md); this file is about what is left to do.

## Where each phase stands

Status words mean something specific. **Complete** means it works and a test proves it.
**Reachable** means a user can get to it but no person has yet exercised it by hand. **Contract
only** means the types exist and the implementation does not.

| Phase | Status | Evidence |
| --- | --- | --- |
| 0 — Audit and contracts | Complete | `docs/UPSTREAM_AUDIT.md`, `config/integration-manifest.json`, the parity and domain gates |
| 1 — Tauri foundation | Complete | Tray, autostart, shortcuts, overlay, capture exclusion, credential store, FTS all real; `apps/desktop/src-tauri/src/` and `docs/PLATFORM_SUPPORT.md` |
| 2 — Dictation | Reachable, unverified by hand | The hotkey drives a real session: microphone, route, transcription, cleanup, clipboard insertion, history. `crates/transcription-core/tests/local_dictation.rs` proves the local path against the real binary. **Not yet exercised by a person** — see Verification |
| 3 — Reasoning | Complete except enterprise | `crates/reasoning-core/src/chat.rs:191` returns `ShapeNotImplemented` for Bedrock, Azure, Vertex |
| 4 — Meetings and notes | Notes reachable; meetings contract only | Notes, folders, search and five-format export are wired to twelve commands and a Notes section. `crates/meeting-core/src/lib.rs` documents the five missing clients. No system audio exists — `grep -r wasapi crates/` is empty |
| 5 — Calendars | Not started | No `crates/calendar-core`, no OAuth flow |
| 6 — Local API and CLI | Reachable | Off by default, started from the settings screen, discovery file written and removed with it. `crates/local-api/tests/api.rs` (20) and `tools/knegiswhisper/tests/end_to_end.rs` (16, driving the compiled binary) |

### What made them unreachable, and what fixed it

`apps/desktop/src-tauri/Cargo.toml` depended on 7 of the 14 workspace crates — not on
`app-core`, `audio-core`, `transcription-core`, `reasoning-core`, `notes-core`, `sidecar`, or
`local-api` — and `shortcuts.rs` emitted a hotkey event nothing consumed. It was a wiring problem,
not a missing-feature problem, which is why it was the first chunk. It now depends on all of them.

## Defects found and fixed during chunk 1

Kept here rather than only in commit messages, because four of the five are the *same class* of
mistake and that pattern is the useful part.

1. **Two modules owned one table.** `storage::transcriptions` and a `notes-core::history` both
   read and wrote `transcriptions`, with two row types, one used by the Tauri commands and the
   other by the local API. Merged into `storage`. Note the plan said the opposite — it assumed
   notes-core's version was the fuller one, and reading both showed it was missing failure
   recording, which is the thing that makes a failed dictation visible at all.
2. **A live frontend fiction.** `ipc.ts` declared `Transcription` in camelCase while the Rust
   struct carried no `rename_all`, so it sent `error_message` where the UI read `errorMessage`, and
   `App.tsx` rendered `(empty)` instead of the reason a dictation failed.
3. **Every tagged enum sent snake_case fields.** `rename_all` on an enum renames the *variants*,
   not the fields inside them, so tagged unions sent `verified_by` while structs sent camelCase.
   One API with two conventions is how the next person writing a frontend type gets it wrong. All
   13 now carry `rename_all_fields = "camelCase"`.
4. **A guessed type in brand-new code.** The `GateReason` union written for the settings screen
   invented its variants (`"speech"`, `"too-short"`) instead of reading them; the real ones are
   `unavailable`, `silence`, `insufficient-speech`, `speech-detected`. Caught before it landed —
   the first time this class was.
5. **Two workspace binaries shared a name.** The desktop app and the CLI both produced
   `knegiswhisper`, so they overwrote each other in `target/debug` and whichever linked last won.
   The CLI's end-to-end tests resolve `CARGO_BIN_EXE_knegiswhisper`, so once the app had been
   linked more recently they launched the GUI application fifteen times with command-line arguments
   it ignored — leaving a stray window on screen and a locked executable that broke the next build.
   The tests had been passing throughout. `tools/integration-audit/verify-binaries.mjs` now fails
   the build on any collision, verified by reintroducing this one.

Items 2, 3 and 4 are one defect wearing three hats: **a type describing the other side of a
boundary, written rather than derived.** The standing mitigation is that every such type names the
Rust item it mirrors and a test asserts the JSON keys. Generating them from the Rust would be
better and is not yet done.

**A guarantee that had nothing behind it**, also fixed:
`OverlayStatus::recording_indicator_visible` was a hardcoded `true` because nothing recorded. It is
now a real read of the window, driven only by the dictation state sink.

## The work

### Chunk 1 — consolidate, then wire *(done)*

The chunk that turns phases 2 and 6 from unreachable into usable.

- **1a. One owner per table.** Merge into `storage`, not out of it: the schema and `settings.rs`
  already live there, and `notes-core` should stay about notes. `storage::transcriptions` gains
  cursor paging, the redacted `Debug`, `AudioForgotten`'s three outcomes, and
  `rename_all = "camelCase"` (fixing defect 2). The dictionary moves to a new
  `storage::dictionary`. `notes-core::history` is deleted and its callers repointed.
- **1b. Adapters for the six trait seams.** Real implementations of `Recorder`, `Transcriber`,
  `Cleanup`, `Inserter`, `History`, and `StateSink` in `apps/desktop/src-tauri/src/dictation.rs`,
  reusing `audio_core::capture::Capture` (real cpal),
  `transcription_core::{route, Executor, WhisperServer}`, `reasoning_core::chat::cleanup`, and
  `platform::insertion::{TextInserter, SystemClipboard}`.

  No new platform code was needed after all: the plan claimed a missing `Keyboard` implementation,
  but the trait is `PasteKeystroke`, `WindowsPaste` implements it, and `platform::paste_keystroke()`
  already resolves per platform — returning `None` where synthetic paste does not exist, which is
  the honest answer and the reason the settings screen can say so up front.

  The microphone did need its own OS thread: `cpal::Stream` is not `Send`, so it cannot live in
  Tauri's managed state. The `Recorder` seam hides that entirely — the state machine never learns
  the microphone is thread-bound, and the thread never learns what a dictation is.
- **1c. Sidecar and model lifecycle.** `AppState` gains the catalogue, the PID store, and the
  running server. Stale sidecars are reaped on boot before anything touches a port. Commands to
  check, install (with progress), start, and stop.
- **1d. Local API lifecycle.** Off unless a setting says otherwise. Start allocates from
  `PORT_RANGE`, serves, writes the discovery file and surfaces the `Permissions` value it returns
  verbatim; stop removes it.
- **1e. The hotkey does something.** `HotkeyAction::Dictation` drives the session. The overlay is
  shown for the whole recording, and `recording_indicator_visible` becomes a real read of that
  (fixing defect 3).
- **1f. Frontend.** Three sections — dictation, local transcription, local API — and only types
  Rust actually sends.

**Still outstanding from chunk 1:** nobody has pressed the hotkey and watched text appear. That is
the check that matters and it is the first item under Verification.

### Chunk 1b — notes reachable *(done)*

Phase 4's notes half was in exactly the state chunk 1 had just fixed for dictation, and the plan
called it complete. `notes-core` had notes, folders, FTS search and five-format export;
`local-api` served all of it; **the Tauri layer had not one notes command**, and `kw-notes-core`
was a declared dependency of the desktop crate that nothing used. `transcript::export` had no
caller anywhere outside its own tests.

Twelve commands, a Notes section, and the export path. Two things worth keeping:

- **Every judgement is the local API's, referenced rather than re-derived.** No cursor on
  `notes_list` (notes order by `updated_at`, so an id cursor skips or repeats rows), prose
  escaped into FTS5 through `notes::escape_query`, the same limit clamping, the same refusal of
  an empty patch. The two surfaces read one database and must not disagree.
- **The save dialog is opened from Rust.** `note_export_to_file` renders first, then asks the OS
  for a path, so no path ever crosses the IPC boundary and the renderer needs no filesystem or
  dialog capability. `SECURITY.md` names path traversal through export as a threat; this is the
  shape that answers it rather than validating a path the frontend supplied.

`deny_unknown_fields` on both request types, and a serde test per boundary type asserting the
exact JSON keys — the standing mitigation for the defect class above.

### Chunk 1c — the gate learns what a UI surface is *(done)*

Rule 5 — "a backend integration exists but cannot be configured in the UI" — only ever checked
that the `ui` field was a **non-empty string**. Sixty-four entries named screens like
`Settings > Meetings` and `Notes > Upload` that nobody had written, and the gate reported green
throughout. A gate that cannot tell a screen from a string is the same defect it exists to catch,
one level up.

`config/ui-surfaces.json` now names the component behind each of the thirteen surfaces, and rule 7
requires a `built` surface to exist and to render `data-surface="<surfaceId>"`. Entries carry
`reachable`, and absent is a violation rather than a default.

`built` and `reachable` had to be two fields, and `Settings > Transcription` is why: the section
exists, lists every provider and runs a real connection test, but **nothing anywhere in the
application can store an API key** — `setProviderCredential` is exported from `ipc.ts` and never
called. The surface is built and not one provider is reachable through it.

Eight integrations are reachable today: six whisper models, the local REST API, and transcript
export. The gate prints the other 56 grouped by surface on every successful run, so the frontend
backlog is sized in integrations rather than adjectives. `verify-parity.test.mjs` asserts each new
rule actually fires, for the reason the domain scanner has self-tests.

> Since superseded. `Settings > Providers` was written in chunk A and 26 integrations are
> reachable now; see [BUILD_PLAN.md](BUILD_PLAN.md). The paragraph above is left as the record of
> what the gate found when it was first able to tell a screen from a string.

### Chunk 1d — sidecar transport *(done)*

`SidecarSpec::port_range` was required and validated non-zero, so a one-shot or streaming child
could only be described by inventing a port it would never bind — and seventeen of the
twenty-two sidecars in the porting matrix are exactly that. `Transport` is now a tagged enum,
`Loopback { port_range }` or `Stdio`, chosen because an `Option<PortRange>` is absent both when a
child genuinely does not listen and when someone forgot to write the range down.

Writing it reproduced defect 3 from the list above within the hour: `rename_all` on an enum
renames the *variants*, not the fields inside them, so the shipped catalogue was refused for
saying `portRange` instead of `port_range`. `rename_all_fields` fixes it. Four tests caught it.

### Chunk 2 — close out phase 3

Bedrock, Azure, and Vertex request shapes. SigV4 signing and body shaping are unit-testable
against AWS's published test vectors, so this is **not blocked** — only a live call is.

### Chunk 3 — system audio and the Windows natives (phase 4)

WASAPI loopback on Windows behind a trait; macOS (`cidre` process taps, 14.4+) and Linux
(PulseAudio monitor) written and marked unverified. Nothing exists today. **This, not the AEC
helper, is what blocks meetings.**

It now also carries the three small Windows capabilities that were going to be inherited as
binaries — key listener, text monitor, mic listener. See `PORTING_MATRIX.md`; the decision is
recorded, the code is not written. The key listener is the one with reach beyond meetings: it is
what modifier-only, right-side-modifier, Globe/`Fn`, mouse-button and push-to-talk hotkeys need,
and `platform/src/hotkey.rs:149` currently tells the user their shortcut does nothing.

### Chunk 4 — calendars (phase 5)

Loopback PKCE against Google and Microsoft, refresh tokens as `token_ref` into the OS credential
store — never the plaintext columns upstream used — and the UI. Buildable and testable against a
stub authorisation server; only the real round trip needs client IDs.

### Chunk 5 — meeting clients (phase 4)

Blocked. See below.

### Chunk 6 — the AEC helper

The one binary this project builds for itself, and a build pipeline rather than a port. It is a
**quality** chunk: without echo cancellation a meeting recorded on speakers transcribes the
far-end voice twice, and on headphones there is nothing to cancel.

Blocked on a toolchain and a release run, not on a decision. The design is settled in
`PORTING_MATRIX.md`, including the two measured third-party pins — abseil by archive sha256,
webrtc-apm by a content digest over its extracted tree, because googlesource generates that
tarball per request and it is not byte-stable. What remains is vendoring the six source files,
porting the 446-line generator, a five-target workflow with `contents: write`, and digests
measured from the published assets. Ride the macOS `whisper-server` job on the same workflow: it
is the same shape and the last `not-published` target in `config/sidecars.json`.

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
| The AEC helper | cmake, MSVC/clang and a workflow run with `contents: write`. The design and both third-party pins are settled in `PORTING_MATRIX.md`; what is missing is a machine that can compile it and a release to measure the digest from | Meetings themselves. Echo cancellation only matters on speakers — see chunk 6 |

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
- **Transcript export writes through an OS save dialog opened by Rust**, not to a path the
  frontend chose. See 1b.
- **The five Windows helpers are reimplemented in Rust rather than rebuilt and published.** One
  already was. Only the AEC helper stays a binary. See `PORTING_MATRIX.md`.

## Verification

Every chunk keeps all of it green: `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`,
`tsc --noEmit`, `tools/domain-scanner` and its self-test, and the parity gate.

Chunk 1 adds: each adapter tested against the `app-core` state machine; the overlay visible for
the whole of a recording; the local API not starting unless the setting says so; the discovery
file removed on stop.

### The by-hand check on Windows

The only thing that actually proves dictation works. Nothing below needs a developer — it is all
buttons and one command — but it does need a person at the machine.

**Before you start.** Only one copy of the application may run at a time. A second instance finds
every shortcut already claimed by the first and registers none of them; the startup log now names
each refusal, so if the Shortcuts section looks dead, check there first.

```powershell
# Stops anything left over, then launches both halves of the application.
taskkill /F /IM knegiswhisper-desktop.exe 2>$null
cd apps\desktop
$env:KNEGISWHISPER_LOG = "info"
npm run tauri:dev
```

**`npm run tauri:dev`, not `cargo run`.** In a development build the window loads from the Vite
dev server named in `tauri.conf.json` (`http://127.0.0.1:5183`) rather than from files on disk, so
`cargo run` produces a perfectly healthy Rust process behind a window that says only "can't reach
this page" — the webview's own error, naming neither cause nor fix. `tauri dev` starts Vite first
and waits for it. A debug build now also probes that URL during startup and says so in the log
when nothing answers, because two honest halves and no diagnosis is the worst way to fail.

The window is the dashboard. Leave the terminal visible — the log is the fastest diagnosis.

1. **Check what the machine can do.** The Privacy overlay section should say capture exclusion is
   active with `GetWindowDisplayAffinity` evidence, and that the overlay will not take focus. Both
   are read back from the real window rather than assumed. Text insertion, under Dictation, should
   say *Supported*.

2. **Give dictation a working shortcut.** It has none: the inherited default `Control+Super` is two
   modifiers and no key, and no cross-platform API can register that. In the Shortcuts section type
   `Control+Shift+D` next to `dictation` and press **Set**. It applies immediately — the log says
   `hotkeys re-registered live=5 total=5`.

3. **Install a local model.** Under Local transcription, press **Install** on `tiny`. About 86 MB:
   the pinned `whisper-server` and the model. `tiny` rather than the recommended `base` because it
   is the one with a measured checksum — the row says `will be checksum verified`, where `base` says
   `size checked only`, and for this check the stronger guarantee is worth more than the accuracy.

4. **Point dictation at it.** Press **Use for dictation** on the installed model. The Route row
   should change to `local model — audio stays on this machine`.

5. **Turn on airplane mode.** Everything from here must work with no network at all. If it does
   not, that is the finding.

6. **Try it from the button first.** Press **Start dictation**, say a sentence, press **Stop and
   transcribe**. This isolates the pipeline from the shortcut: if this works and the hotkey does
   not, the problem is registration, not transcription. Watch for the overlay appearing while
   recording and going away afterwards.

7. **Now the hotkey, into Notepad.** Open Notepad, click into it, press `Control+Shift+D`, say
   *"the quick brown fox jumps over the lazy dog"*, press it again. The text should appear at the
   cursor. Repeat in VS Code — a different application with different focus handling.

8. **Check the clipboard survived.** Copy something distinctive first (`KEEP ME`), dictate, then
   paste. You should get `KEEP ME` back: dictation borrows the clipboard and puts it back only if
   what is there is still what it pasted.

9. **Check the dashboard never stole focus.** At no point during 7 should the caret have left
   Notepad. If text appeared in the dashboard instead, non-activation failed despite reporting
   otherwise.

10. **Check history, then that it can be switched off.** The History section should show what you
    dictated. A failed attempt should show its reason rather than `(empty)`.

11. **Then the CLI.** Start the local API from its section — it is off by default — and:

```powershell
.\target\debug\knegiswhisper.exe health
.\target\debug\knegiswhisper.exe history list
.\target\debug\knegiswhisper.exe notes list
```

`health` should name the version; `history list` should show the dictation from step 7. Note the
binary is `knegiswhisper.exe` and the application is `knegiswhisper-desktop.exe` — they used to
collide, which is its own entry above.

**What I would not be surprised to see fail:** the paste keystroke landing before the target
application has read the clipboard (the delay is upstream's measured 500 ms, but it is a race
either way), and the overlay's position being wrong or unhelpful, since nobody has looked at it.
Both are worth reporting precisely rather than working around.

The parity gate will want `developer:local-rest-api` and `developer:cli` marked reachable once
they have a UI — deliberately not before, because a backend integration that cannot be configured
is one of the six conditions that gate exists to catch.
