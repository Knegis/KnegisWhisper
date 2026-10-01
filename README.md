# KnegisWhisper

Local-first voice dictation and meeting transcription, built with Tauri 2, Rust and React.
Windows is the primary development and hands-on validation platform; macOS and Linux support
varies by capability. See [platform support](docs/PLATFORM_SUPPORT.md).

Dictate into other applications, transcribe meetings, and keep searchable notes with editable
Markdown and recording playback. Use local models or configure a bring-your-own-key provider.

**No KnegisWhisper account, subscription, telemetry or hosted backend.** Configured cloud
requests go directly to the provider; provider charges may apply. Local transcription works
offline once the required binaries and models are installed. Downloads and opt-in integrations
have their own documented [network purposes](docs/NETWORK_BOUNDARY.md).

## Current status

**Early development; run from source.** This summary reflects the checked-out `dev` code as of 2026-09-08.
Local tests and Windows observations are separate from hosted CI and release validation.
The release workflow exists, but the checked-in updater public key is empty: secure update
checking and an update popup are not available merely because the app builds.

| Area | Implemented and remaining limits |
| --- | --- |
| Dictation | Global shortcuts, toggle and hold-to-talk, local Whisper/Parakeet and configured cloud routes. On Windows, modifier-only toggles such as `Control+Super` activate after all chord keys are released; an additional key cancels the gesture. |
| Meetings | Microphone and system-audio recording, live transcription, detection/recording offers and a notification panel. Windows WASAPI capture and local/OpenAI Realtime transcription have been exercised. Other cloud meeting providers and some detection/notification scenarios still need live validation. |
| Notes | Independent title and Notes / Transcript / Replay tabs; in windows 1280px or wider the transcript is a column beside the note instead of a tab. Personal Markdown notes precede AI summaries; empty notes can start from the transcript. Playback continues across tabs. Folders, transcript-aware full-text search and txt/md/json/srt/vtt export remain available. See [Notes workspace](docs/NOTES_WORKSPACE.md). |
| Meeting summaries | Optional automatic/manual summaries with an independent model, editable prose, transcript references, chunked processing and retry/cancel. Configure Settings > Cleanup & summaries. See [implementation and validation](docs/MEETING_SUMMARIES.md) for model-quality limits. |
| Recording ownership | Older notes can attach an explicitly selected PCM16 mono/stereo WAV through a native dialog. The source is copied and preserved. Desktop and API note deletion share recording cleanup. Meeting attachment failure keeps the note and WAV and displays recovery guidance. |
| Import | Local audio/video is converted with ffmpeg, transcribed and saved with retained audio for playback. UI and API imports share one active slot, with no queue. Progress survives navigation; job metadata survives restart, while unfinished jobs become failed rather than resuming. URL import remains disabled. OpenWhispr database import is a separate flow. |
| Local inference | Whisper and Parakeet for dictation/import, local meeting routes including streaming models, and llama.cpp for dictation cleanup. Long Parakeet inputs can crash the current offline runtime; see the findings below. Whisper uses serialized on-demand recovery, without automatically replaying a submitted transcription. |
| API, CLI and MCP | Opt-in loopback API, CLI and a stdio MCP server with 23 tools: notes, dictionary, history search, file transcription jobs, meeting recording, status and model discovery. Discovery permissions are verified before serving; detected permission widening revokes the token and stops service. Repair is attempted once, followed by explicit retry. |
| Calendars and diarization | Google/Microsoft public-client PKCE flows and speaker-diarization controls are built. Calendar account round trips and real-meeting speaker accuracy still need validation. Apple Calendar is not implemented. |

The owner has confirmed Windows dictation/hotkey behavior, API start/stop and permission
readback, note autosave (including immediate close), meeting-row/selection playback and
scrubbing, and a 45-minute Whisper import with playback. Short Parakeet imports work.
The real import picker also permits retry after a conversion failure without reselecting the file.

Remaining limits worth knowing:

- **Long Parakeet audio:** isolated 15- and 45-minute inputs exited with `0xC0000409`.
  Three sequential five-minute pieces completed in a follow-up experiment, but production
  chunking is not implemented and the native crash cause is unresolved. See
  [Parakeet benchmark findings](docs/PARAKEET_BENCHMARK.md).
- **Playback precision:** rows use available segment timings, not arbitrary selected words.
  Imports without provider timings use a labelled whole-recording cue.
- **Platform privacy:** capture-exclusion readback confirms an OS flag, not that every screen
  recorder honors it. Some native window, capture and shortcut paths remain unsupported or
  unverified outside Windows.
- **Deferred or declined:** semantic search, chat/voice agents, translation, Apple Calendar,
  the AEC helper and attested Tinfoil integration are not available. Code blocks remain plain;
  CodeMirror highlighting is excluded under the strict CSP. There is no standalone chat screen.
- **External verification:** the long OpenAI Realtime reconnect run and release/signing
  validation remain separate checks. Consult the actual Actions run for hosted CI status;
  the checked-in workflows and local test reports do not establish that status.

[BENCH_CHECK.md](docs/BENCH_CHECK.md) and [ENGINEERING_LOG.md](docs/ENGINEERING_LOG.md)
record dated observations, fixes and limitations. Older entries describe the repository at
that time; they are not a claim that previously completed work is still missing.

## Find things in the app

- **The rail** down the left edge has four entries: **Dictation, Meetings, Notes**, then
  **Settings**. Each opens a list panel beside the page; the title bar's list toggle hides and
  shows it, and it starts hidden in windows 900px wide or narrower.
- **Dictation** and **Meetings** are centred task screens. Their control bar holds the start
  button and says what is in effect and where the audio goes, and its chips change those values
  in place: the microphone, the route and the languages on Dictation, the provider and language
  on Meetings, and the Tidy up and Summary switches once they have been set up in Settings (until
  then they link there). The route, the languages and Tidy up are held while a dictation runs.
  Dictation's list panel holds its history, grouped by day; Meetings' holds Recent and Interrupted
  recordings.
- **Notes:** folders and notes are in the list panel; the note opens beside it. In windows
  1280px or wider, the transcript also shows as a column to the right, which the note's top bar
  can hide. Import audio or video is
  reached from Notes' list panel or its empty state, not from the rail.
- **Settings:** the list panel names Shortcuts, Meetings, Vocabulary, Transcription, Cleanup &
  summaries, Local text models, Providers and Calendar, then, under a **Developer** heading,
  Local API, CLI, MCP and Diagnostics.

Select the transcription provider/model in **Settings > Transcription**. Import audio or video
uses that route; **Settings > Meetings** has its own provider selection. Converted import audio
stays until its note is deleted, while the selected source file stays intact.

For a first local dictation, install the server/model from **Settings > Transcription**,
select the local route, then configure the shortcut in **Settings > Shortcuts**. For cloud
transcription, add credentials in **Settings > Providers**, then select that provider in
**Transcription**. **Settings > Cleanup & summaries** configures optional dictation cleanup;
**Local text models** manages local LLMs. A local transcription route with cloud cleanup still
sends text to that provider.

## Local API, CLI and MCP

Keep the desktop app running and enable **Settings > Developer > Local API**. Both clients use its
authenticated loopback API; the desktop owns transcription, storage and recording.
The default discovery file is `~/.knegiswhisper/local-api.json` (under your Windows user
profile on Windows). It contains the port and bearer token and should not be shared.

Build the clients from the repository root:

```sh
cargo build -p kw-cli -p kw-mcp
cargo run -p kw-cli -- health
cargo run -p kw-cli -- status
cargo run -p kw-cli -- models list
cargo run -p kw-cli -- transcribe file "meeting.wav"
cargo run -p kw-cli -- jobs status JOB_ID
cargo run -p kw-cli -- notes search "budget review"
cargo run -p kw-cli -- notes export NOTE_ID --output "notes.md"
```

Add `--json` for machine-readable CLI output. File transcription returns a job ID to poll
or cancel; it uses the app's configured transcription route. Clients upload file bytes,
so the API does not accept arbitrary server filesystem paths. Recording commands control
the visible meeting recorder, including sessions started in the UI.

Configure your MCP client's stdio command with the absolute path to
`target/debug/knegiswhisper-mcp.exe` on Windows, or `target/debug/knegiswhisper-mcp` on
macOS/Linux. No arguments are needed. Protocol messages use stdout and diagnostics use
stderr. **Settings > Developer > CLI** and **MCP** provide discovery and setup information.

See [CLI and MCP workflows](docs/CLI_MCP_WORKFLOWS.md) for export formats, upload limits,
cancellation, restart behavior and dated Windows validation, and [agent skills](docs/agent-skills/)
for the HTTP routes and client/tool reference.

## Development

Requires Rust 1.90+, Node 22+, and the platform's native build/webview dependencies.
On Windows, use the MSVC C++ build tools and WebView2. The
[CI workflow](.github/workflows/ci.yml) lists Linux system packages.

From the repository root:

```sh
npm --prefix apps/desktop ci
npm --prefix apps/desktop run tauri:dev
```

`tauri:dev` starts Vite and the Rust desktop application. Opening Vite in an ordinary browser
instead shows explicitly labelled **DEV FIXTURES** with invented data and no Rust backend;
use the native window for microphone, dialog, playback and API checks.

To build the desktop executable without creating installer bundles:

```sh
npm --prefix apps/desktop run tauri:build -- --no-bundle
```

The executable is `target/release/knegiswhisper-desktop.exe` on Windows. The desktop binary
is named `knegiswhisper-desktop`; `knegiswhisper` is the CLI. Building the application does
not install transcription models or sidecars; install those through the app.

### Repository layout and runtime data

| Path | Responsibility |
| --- | --- |
| `apps/desktop/src/` | React views, navigation, editor, recording playback and typed IPC client |
| `apps/desktop/src-tauri/` | Native app lifecycle, IPC commands, windows and runtime orchestration |
| `crates/` | Rust libraries for audio, transcription, meetings, notes, storage, providers, security and the local API |
| `tools/knegiswhisper/`, `tools/knegiswhisper-mcp/` | CLI and MCP clients of the running desktop app |
| `tools/*-audit/`, `tools/domain-scanner/`, `config/` | Repository contracts, generated registries and consistency checks |
| `vendor/cpal/` | Patched CPAL dependency selected by the workspace Cargo manifest |
| `docs/` | Architecture, workflows, platform limits and dated validation evidence |

SQLite data lives in the per-user `KnegisWhisper` data directory, normally
`%APPDATA%/KnegisWhisper/knegiswhisper.db` on Windows. Models, sidecars and recordings
are managed by the app; native model/audio paths may use an ASCII-safe fallback.
Provider credentials use the OS credential store. Ordinary development runs use the
normal user profile; [isolated debug profiles](docs/CLI_MCP_WORKFLOWS.md) are available
for smoke checks that need a separate database and credential store.

### Manual verification commands

Run these from the repository root. Build the frontend before Rust checks so Tauri can embed
the current assets. `npm ci` applies the checked-in dependency patches through `postinstall`.

```sh
# Frontend: strict typechecking, TypeScript behavior tests and production assets
npm --prefix apps/desktop ci
npm --prefix apps/desktop test
npm --prefix apps/desktop run build
node tools/domain-scanner/scan.mjs
node tools/dev-bundle-audit/scan.mjs

# Repository contracts and generated artifacts
node tools/integration-audit/verify-parity.mjs
node tools/settings-audit/scan.mjs
node tools/integration-audit/verify-binaries.mjs
node tools/command-audit/scan.mjs
node tools/integration-audit/generate-provider-registry.mjs --check
node tools/integration-audit/generate-ts-types.mjs --check
node tools/feature-audit/scan.mjs
node tools/capability-audit/scan.mjs

# Contract/tool self-tests
node --test tools/ci/changes.test.mjs
node --test tools/domain-scanner/scan.test.mjs
node --test tools/integration-audit/verify-parity.test.mjs
node --test tools/settings-audit/scan.test.mjs
node --test tools/model-pin/pin.test.mjs
node --test tools/feature-audit/scan.test.mjs
node --test tools/capability-audit/scan.test.mjs
node --test tools/command-audit/scan.test.mjs
node --test tools/dev-bundle-audit/scan.test.mjs

# Rust workspace, then the additional feature-gated Tauri mock-webview suite
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
cargo clippy -p knegiswhisper --lib --tests --features tauri-test -- -D warnings
cargo test -p knegiswhisper --lib --features tauri-test
```

The workflow also checks baseline integrity: the manifest's `baseline.commit` must equal
the upstream baseline's `provenance.commit`. Baseline regeneration requires an OpenWhispr
checkout; it is not a routine build step:

```sh
node tools/integration-audit/generate-baseline.mjs --upstream <path>
```

TypeScript tests use the existing compiler and Node's built-in runner, emitting only into
ignored `.test-build`. No DOM test framework is installed. Rust IPC declarations are generated
from Rust types; see [ipc_types.rs](apps/desktop/src-tauri/src/ipc_types.rs) for regeneration.
Ignored native/manual tests are not counted as passes by the ordinary suites.
Hosted Rust checks use `--profile ci`, which keeps debug assertions while disabling
dependency optimization to reduce cold build time. The commands above use the local
development profile.

### Optional Windows fixture checks

These use temporary databases and recordings. The pipeline test requires installed ffmpeg
and the Whisper base model; it starts an isolated server and checks conversion, transcription,
recording persistence, byte ranges and deletion. It does not prove audible playback.

```sh
cargo test -p knegiswhisper --lib --features tauri-test native_audio_video_import_pipeline -- --ignored --nocapture
```

The attachment test opens a real native dialog. Keep `npm --prefix apps/desktop run dev`
running in a separate terminal, then run the following from the repository root and close
the test window when finished:

```sh
cargo test -p knegiswhisper --lib --features tauri-test native_recording_attachment_smoke -- --ignored --nocapture
```

## CI and delivery

`dev` is the integration branch. Pull requests always run Contracts; changed-file selection
adds frontend and Windows Rust checks as appropriate. Frontend inputs also select Windows
because the desktop embeds them. Documentation-only PRs run Contracts. Manual dispatch
retains Windows, macOS and Linux coverage. Caches and superseded-run cancellation are enabled.

There is no automatic push trigger, so a direct push to `dev` does not run CI. The earlier
Actions budget pause remains part of the engineering history; selective PR checks have since
been restored. Workflow configuration and passing local Windows checks do not establish a
successful hosted Windows run.

[Release](.github/workflows/release.yml) is manually dispatched. The separate
[sidecar-pin verifier](.github/workflows/pin-verify.yml) has a weekly schedule and manual dispatch;
it is not triggered by pushes. Publishing, signing configuration and installed-release
validation are not implied by a successful development build. See
[SECURITY.md](docs/SECURITY.md) for updater signing.

## Documentation

| Document | What it covers |
| --- | --- |
| [BENCH_CHECK.md](docs/BENCH_CHECK.md) | Dated hardware observations and remaining manual checks |
| [PARAKEET_BENCHMARK.md](docs/PARAKEET_BENCHMARK.md) | Long-input failures, measurements and the chunking experiment |
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crate layout, interfaces and storage design |
| [ENGINEERING_LOG.md](docs/ENGINEERING_LOG.md) | Decisions, deviations and delivery evidence |
| [BUILD_PLAN.md](docs/BUILD_PLAN.md) / [COMPLETION_PLAN.md](docs/COMPLETION_PLAN.md) | Implementation history and deferred work |
| [PHASES_0_6_COMPLETION_PLAN.md](docs/PHASES_0_6_COMPLETION_PLAN.md) | Earlier phase audit and completion plan |
| [PLATFORM_SUPPORT.md](docs/PLATFORM_SUPPORT.md) | Platform capabilities and validation limits |
| [INTEGRATION_PARITY.md](docs/INTEGRATION_PARITY.md) / [PORTING_MATRIX.md](docs/PORTING_MATRIX.md) | Integration contracts and upstream mapping |
| [NETWORK_BOUNDARY.md](docs/NETWORK_BOUNDARY.md) / [SECURITY.md](docs/SECURITY.md) | Egress policy, credential handling and threat model |
| [Agent skills](docs/agent-skills/) | Driving a running instance through CLI and MCP |
| [CLI_MCP_WORKFLOWS.md](docs/CLI_MCP_WORKFLOWS.md) | File jobs, recording commands, exports, limits and isolated debug validation |
| [UI surfaces](config/ui-surfaces.json) / [Settings keys](config/settings-keys.json) | Reachable controls and backend settings |
| [UPSTREAM_AUDIT.md](docs/UPSTREAM_AUDIT.md) | Original upstream audit |

## Provenance

KnegisWhisper ports product functionality from [OpenWhispr](https://github.com/OpenWhispr/openwhispr)
(MIT) and takes its native shell architecture as a design reference from
[Pluely](https://github.com/iamsrikanthnani/pluely) (GPL-3.0, **no code copied** — see
[NOTICE](NOTICE)). It is not affiliated with either project and depends on neither one's
services.

## License

MIT. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
