# KnegisWhisper

Local-first voice dictation and meeting transcription for macOS, Windows, and Linux. Built with
Tauri 2 and Rust — no Electron.

Global-hotkey dictation into any application, meeting transcription from microphone and system
audio, local Whisper and Parakeet models, bring-your-own-key cloud providers, local notes with
search, and a lightweight privacy overlay.

**No account. No subscription. No telemetry. No hosted backend.** Cloud providers are used only
when you configure them with your own credentials, and requests go straight to the provider.

> **Status: early development.** Phase 0 (upstream audit and parity contracts) is complete.
> Phase 1 (Tauri foundation) is in progress. Nothing is installable yet.

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
| [INTEGRATION_PARITY.md](docs/INTEGRATION_PARITY.md) | How integration parity is enforced |
| [NETWORK_BOUNDARY.md](docs/NETWORK_BOUNDARY.md) | Every reason this app makes a network request |
| [PLATFORM_SUPPORT.md](docs/PLATFORM_SUPPORT.md) | What works where, and what is unverified |
| [PORTING_MATRIX.md](docs/PORTING_MATRIX.md) | How each upstream piece is handled |
| [SECURITY.md](docs/SECURITY.md) | Threat model, credential handling, reporting |
| [UPSTREAM_AUDIT.md](docs/UPSTREAM_AUDIT.md) | Phase 0 audit of both upstream projects |

## Development

Requires Rust 1.90+, Node 22+, and a platform C toolchain.

```sh
# Verify the integration parity contract
node tools/integration-audit/verify-parity.mjs

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
