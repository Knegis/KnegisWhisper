# KnegisWhisper

Local-first voice dictation and meeting transcription for macOS, Windows, and Linux. Built with
Tauri 2 and Rust — no Electron.

Global-hotkey dictation into any application, meeting transcription from microphone and system
audio, local Whisper and Parakeet models, bring-your-own-key cloud providers, local notes with
search, and a lightweight privacy overlay.

**No account. No subscription. No telemetry. No hosted backend.** Cloud providers are used only
when you configure them with your own credentials, and requests go straight to the provider.

> **Status: early development. Not installable yet** — the release workflow has never been
> dispatched and no signing keypair exists.
>
> The table below is what is *built*. What has actually been run on hardware is a different claim
> and lives in one place: [BENCH_CHECK.md](docs/BENCH_CHECK.md), which lists the checks nobody has
> done as plainly as the ones that passed. Where the two disagree, believe that file.
>
> | Phase | State |
> | --- | --- |
> | 0 — Upstream audit and parity contracts | Complete |
> | 1 — Tauri foundation | Complete. Boots to a dashboard, lives in the tray, registers hotkeys, creates a capture-excluded overlay. |
> | 2 — Dictation | Complete, and driven by hand: dictation into a real editor produced correct text repeatedly on both `whisper-tiny` and Parakeet (2026-08-28), after seven local transcriptions with the network off on 2026-08-25. A handful of smaller checks around it are still open — see [BENCH_CHECK.md](docs/BENCH_CHECK.md). |
> | 3 — Reasoning | Complete. Routing, request shaping and chat dispatch; cleanup against any OpenAI-compatible provider. Bedrock, Azure and Vertex refuse honestly rather than falling back, and none of the three has made a live call. |
> | 4 — Meetings and notes | Notes, folders, search and export to txt/md/json/srt/vtt are reachable from the dashboard. Since 2026-09-02 a note opens in the Milkdown Crepe editor with autosave, and a meeting recorded from then on is linked to its WAV so the transcript's rows play back in place — built and gated, not yet driven by hand (KNEG-152). System audio is built and verified on hardware (Windows WASAPI loopback). Four cloud clients are written — OpenAI Realtime, AssemblyAI, Deepgram, Corti — and only OpenAI has met a live server; the fifth, Tinfoil, is declined on principle rather than unfinished. The local route produced a live transcript on 2026-08-28. |
> | 5 — Calendars | Built. `calendar-core`, loopback PKCE, tokens held by reference, and `Integrations > Calendar`. No calendar code has answered a real request. |
> | 6 — Local API and CLI | Complete. Off by default, started from the settings screen, which re-reads the discovery file's real permissions rather than remembering them. The `knegiswhisper` CLI is tested against it as a compiled binary, exit codes included. |
> | 7 — MCP and the integration screens | Built. A `knegiswhisper-mcp` stdio server with thirteen tools, `Integrations > MCP`, `Integrations > CLI`, and [agent skills](docs/agent-skills/) for both. No MCP client has ever connected. |
> | 8 — Media import | Built. ffmpeg and yt-dlp sidecars behind `Notes > Upload`; subtitle export is verified end to end against the real transcription server. Speaker diarization is built and reachable through `Settings > Meetings`, measured against the real CLI — though whether its speaker turns are right in a real meeting is unproven. Semantic search is not started. |
> | 9 — Release | The workflow exists and has never been dispatched. No keypair. |
> | 10 — Agent features | Not started. There is no chat screen; the local LLM's only consumer today is dictation cleanup. |
>
> Every capability claim in the settings screen is read back from the operating system rather
> than assumed, so what the application says it can do is what it can do. Where something is
> unimplemented it says so, rather than failing quietly. See
> [PLATFORM_SUPPORT.md](docs/PLATFORM_SUPPORT.md).

## What it will do

- Global-hotkey dictation with toggle and push-to-talk, inserted at the previous cursor position
- Manual and automatic meeting transcription with live transcripts and speaker labels
- Local transcription that works fully offline: whisper.cpp, Parakeet, Nemotron
- Direct BYOK cloud transcription and reasoning — 23 providers, 96 models
- Local LLM inference via llama.cpp
- Notes, folders, transcripts, and audio history with local full-text search
- Google, Microsoft 365, and Apple calendar integration
- A local REST API, CLI, and MCP server
- A non-activating privacy overlay with honest capture-exclusion reporting

## Documentation

| Document | What it covers |
| --- | --- |
| [BENCH_CHECK.md](docs/BENCH_CHECK.md) | **What has been run on real hardware, and what has not** |
| [BUILD_PLAN.md](docs/BUILD_PLAN.md) | Phases 0-9 as chunks A-J, and what each one landed |
| [COMPLETION_PLAN.md](docs/COMPLETION_PLAN.md) | Phase 7-10 scope, and the blocked table |
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crate layout, key interfaces, storage design |
| [ENGINEERING_LOG.md](docs/ENGINEERING_LOG.md) | Judgement calls, deviations, and what is not usable yet |
| [PHASES_0_6_COMPLETION_PLAN.md](docs/PHASES_0_6_COMPLETION_PLAN.md) | What is left in phases 0-6, and what is blocked on what |
| [`docs/agent-skills/`](docs/agent-skills/) | Skills for driving a running instance from a coding agent |
| [INTEGRATION_PARITY.md](docs/INTEGRATION_PARITY.md) | How integration parity is enforced |
| [NETWORK_BOUNDARY.md](docs/NETWORK_BOUNDARY.md) | Every reason this app makes a network request |
| [PLATFORM_SUPPORT.md](docs/PLATFORM_SUPPORT.md) | What works where, and what is unverified |
| [PORTING_MATRIX.md](docs/PORTING_MATRIX.md) | How each upstream piece is handled |
| [`config/ui-surfaces.json`](config/ui-surfaces.json) | Every UI surface an integration or a setting may claim, and whether it exists |
| [`config/settings-keys.json`](config/settings-keys.json) | Every settings key the backend reads, and how a user changes it |
| [SECURITY.md](docs/SECURITY.md) | Threat model, credential handling, reporting |
| [UPSTREAM_AUDIT.md](docs/UPSTREAM_AUDIT.md) | Phase 0 audit of both upstream projects |

## Development

Requires Rust 1.90+, Node 22+, and a platform C toolchain.

```sh
# Repository contracts — every check CI's Contracts job gates on, in its order.
# This list is what actually runs: ci.yml has had its push and pull_request
# triggers commented out by Actions budget, so nothing checks a pushed commit.
node tools/integration-audit/verify-parity.mjs               # integration parity
node tools/settings-audit/scan.mjs                           # settings keys a user can reach
node tools/integration-audit/verify-binaries.mjs             # binary name collisions
node tools/command-audit/scan.mjs                            # import commands check their grant
node tools/domain-scanner/scan.mjs                           # forbidden vendor domains
node tools/integration-audit/generate-provider-registry.mjs --check
node tools/integration-audit/generate-ts-types.mjs --check
node tools/feature-audit/scan.mjs                            # cargo features nothing enables
node --test tools/domain-scanner/scan.test.mjs               # scanner self-tests
node --test tools/integration-audit/verify-parity.test.mjs   # parity gate self-tests
node --test tools/settings-audit/scan.test.mjs               # settings gate self-tests
node --test tools/model-pin/pin.test.mjs                     # model pin self-tests
node --test tools/feature-audit/scan.test.mjs                # feature audit self-tests
node --test tools/command-audit/scan.test.mjs                # command audit self-tests
# The one step not repeated here is Baseline integrity, an inline node -e in
# ci.yml: it asserts the manifest and the baseline file name the same commit.

# Rust
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# The mock-webview IPC boundary test, which is behind a feature and so runs in
# neither invocation above. Last on purpose: it resolves tauri with a different
# feature set, so it rebuilds tauri and everything downstream.
cargo test -p knegiswhisper --lib --features tauri-test

# Frontend
cd apps/desktop && npm ci && npx tsc --noEmit && npx vite build

# Run the app
cd apps/desktop && npx tauri dev

# Regenerate the upstream baseline (needs an OpenWhispr checkout)
node tools/integration-audit/generate-baseline.mjs --upstream <path>
```

## Provenance

KnegisWhisper ports product functionality from [OpenWhispr](https://github.com/OpenWhispr/openwhispr)
(MIT) and takes its native shell architecture as a design reference from
[Pluely](https://github.com/iamsrikanthnani/pluely) (GPL-3.0, **no code copied** — see
[NOTICE](NOTICE)). It is not affiliated with either project and depends on neither one's
services.

## License

MIT. See [LICENSE](LICENSE) and [NOTICE](NOTICE).
