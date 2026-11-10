# Development

[Contributing](../CONTRIBUTING.md) · [Documentation](README.md)

## Development

Requires Rust 1.90+, Node 22+, and the platform's native build/webview dependencies.
On Windows, use the MSVC C++ build tools and WebView2. The
[CI workflow](../.github/workflows/ci.yml) lists Linux system packages.

On Windows PowerShell, use `npm.cmd` instead of `npm` if script execution policy
blocks the npm PowerShell shim.

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

### Updating dependency notices

After changing a Rust/npm dependency, bundled font, or Silero asset, regenerate the
installer notices. This requires **cargo-about 0.8.4** and a fresh production build:

```sh
cargo install cargo-about --version 0.8.4 --locked
npm --prefix apps/desktop ci
npm --prefix apps/desktop run build
cargo fetch --locked
python tools/licenses/generate.py
python tools/licenses/generate.py --check
```

Commit `apps/desktop/src-tauri/resources/THIRD_PARTY_NOTICES.txt` with the change.
The frontend inventory in `apps/desktop/.notice-build/` is an ignored build input;
it is not shipped. CI checks freshness for code, dependency, license and build
changes; docs-only PRs skip the Windows notices job. See
[notice generation](../tools/licenses/README.md) for target selection and attribution.

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
normal user profile; [isolated debug profiles](CLI_MCP_WORKFLOWS.md) are available
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
node --test tools/model-pin/retry.test.mjs
node --test tools/sidecar-pin/pin.test.mjs
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
from Rust types; see [ipc_types.rs](../apps/desktop/src-tauri/src/ipc_types.rs) for regeneration.
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

### Optional real microphone checks

Ordinary tests use a fake capture factory for the microphone-thread reply proof.
Run hardware checks individually; a bare `-- --ignored` also selects unrelated
network and native-dialog tests. These four checks open a real microphone:

```sh
cargo test -p knegiswhisper --lib --features tauri-test a_real_microphone_thread_answers_or_reports_open_failure -- --ignored --nocapture
cargo test -p knegiswhisper --lib --features tauri-test a_started_recording_hands_its_level_over_and_it_ends_with_the_recording -- --ignored --nocapture
cargo test -p knegiswhisper --lib --features tauri-test the_meters_are_the_captures_own_and_end_with_the_recorder -- --ignored --nocapture
cargo test -p knegiswhisper --lib --features tauri-test the_real_default_microphone_is_checked_and_closed -- --ignored --nocapture
```

The five-second test has a ten-second command deadline. Meeting and dictation
starts are refused in Rust while its worker owns the microphone. Timeout requests
cancellation, but keeps ownership until the capture thread actually exits. A
permanently blocked driver may require restarting the app. Browser fixtures and
device-free tests do not establish microphone quality or behavior across platforms.

The complete KNEG-422/424 disposition and evidence are in
[the validation ledger](KNEG_422_424_VALIDATION.md).

## CI and delivery

`dev` is the integration branch. Pull requests always run Contracts; changed-file selection
adds frontend and Windows Rust checks as appropriate. Frontend inputs also select Windows
because the desktop embeds them. Documentation-only PRs run Contracts. Manual dispatch
retains Windows, macOS and Linux coverage. Caches and superseded-run cancellation are enabled.

There is no automatic push trigger, so a direct push to `dev` does not run CI. The earlier
Actions budget pause remains part of the engineering history; selective PR checks have since
been restored. Workflow configuration and passing local Windows checks do not establish a
successful hosted Windows run.

[Release](../.github/workflows/release.yml) is manually dispatched. The separate
[sidecar-pin verifier](../.github/workflows/pin-verify.yml) has a weekly schedule and manual dispatch;
it is not triggered by pushes. Publishing, signing configuration and installed-release
validation are not implied by a successful development build. See
[SECURITY.md](SECURITY.md) for updater signing.

## Release tooling

Release maintainers also need Python 3.11+ for version and asset validation.

```sh
python -m unittest discover -s tools/release -p "test_*.py"
```

See [the release process](RELEASING.md) for tagged builds and installed-app acceptance,
and [the product presentation validation](PRODUCT_REPOSITORY_VALIDATION.md) for the
initial local build and media evidence.
