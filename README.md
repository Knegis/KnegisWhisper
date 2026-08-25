# KnegisWhisper

Local-first voice dictation and meeting transcription for macOS, Windows, and Linux. Built with
Tauri 2 and Rust — no Electron.

Global-hotkey dictation into any application, meeting transcription from microphone and system
audio, local Whisper and Parakeet models, bring-your-own-key cloud providers, local notes with
search, and a lightweight privacy overlay.

**No account. No subscription. No telemetry. No hosted backend.** Cloud providers are used only
when you configure them with your own credentials, and requests go straight to the provider.

> **Status: early development. Not installable yet, and not usable end to end.**
>
> | Phase | State |
> | --- | --- |
> | 0 — Upstream audit and parity contracts | Complete |
> | 1 — Tauri foundation | Complete. Boots to a dashboard, lives in the tray, registers hotkeys, creates a capture-excluded overlay. |
> | 2 — Dictation | The local path works: the pinned whisper-server installs, the model downloads and its digest is checked, the server starts on loopback, and speech comes back as text. Proven by `crates/transcription-core/tests/local_dictation.rs` against the real binary. **Not yet reachable from the interface** — no Tauri command installs a model and no hotkey runs the session, so it works in tests before it works for a user. |
> | 3 — Reasoning | Routing, request shaping, and chat dispatch complete. Cleanup works against any OpenAI-compatible provider. Bedrock, Azure, and Vertex refuse honestly. |
> | 4 — Meetings and notes | Notes, folders, search, and transcript export complete. The meeting *contract* and capability model are complete; the five WebSocket clients are not written. |
> | 5 — Calendars | Not started. |
> | 6 — Local API and CLI | The API and the `knegiswhisper` CLI are complete and tested end to end, including the compiled binary's exit codes. **Not yet started by the application** — no Tauri command brings the server up, the same gap as dictation. |
> | 7–9 | Not started. |
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
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crate layout, key interfaces, storage design |
| [ENGINEERING_LOG.md](docs/ENGINEERING_LOG.md) | Judgement calls, deviations, and what is not usable yet |
| [INTEGRATION_PARITY.md](docs/INTEGRATION_PARITY.md) | How integration parity is enforced |
| [NETWORK_BOUNDARY.md](docs/NETWORK_BOUNDARY.md) | Every reason this app makes a network request |
| [PLATFORM_SUPPORT.md](docs/PLATFORM_SUPPORT.md) | What works where, and what is unverified |
| [PORTING_MATRIX.md](docs/PORTING_MATRIX.md) | How each upstream piece is handled |
| [SECURITY.md](docs/SECURITY.md) | Threat model, credential handling, reporting |
| [UPSTREAM_AUDIT.md](docs/UPSTREAM_AUDIT.md) | Phase 0 audit of both upstream projects |

## Development

Requires Rust 1.90+, Node 22+, and a platform C toolchain.

```sh
# Repository contracts — the same checks CI gates on
node tools/integration-audit/verify-parity.mjs   # integration parity
node tools/domain-scanner/scan.mjs               # forbidden vendor domains
node --test tools/domain-scanner/scan.test.mjs   # scanner self-tests

# Rust
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

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
