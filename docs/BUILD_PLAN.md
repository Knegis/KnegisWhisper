# The remaining build — phases 0–9

A tracked plan, maintained in place. When something here becomes false, edit it rather than
appending a note.

- Reasoning behind individual choices: [ENGINEERING_LOG.md](ENGINEERING_LOG.md).
- The phases 0–6 detail, including the by-hand Windows check and the chunk-1 defect list:
  [PHASES_0_6_COMPLETION_PLAN.md](PHASES_0_6_COMPLETION_PLAN.md). This document supersedes its
  forward-looking half and leaves its history intact.

## Context

Phases 0–4 and 6 are wired and, since the bench check on 2026-08-25, **phase 2 and phase 6 are
verified on real hardware** rather than only tested: seven local transcriptions in three minutes
with the network off, text landing at the caret in Notepad and VS Code, the clipboard given back,
focus never moving, and the compiled CLI reading history out of the running application. That is
the first thing in this project to earn the word *verified*.

What the same period established is that **the remaining work was mostly reachability, not
capability** — and by the close of this plan (chunks A–J, plus Task 20's unplanned network-boundary
fix) that work is done. The parity gate resolves each manifest entry through
`config/ui-surfaces.json` and prints what cannot be configured; quoting its actual output rather
than restating a number, run at the end of this plan:

```
integration parity OK — 102 entries against baseline c4000db9802d
  ported-with-new-rust-implementation    52
  rewired-to-direct-byok                 21
  removed-vendor-service                 18
  temporarily-deferred                   8
  ported                                 2
  platform-specific                      1

6 integrations are declared unreachable:
  Settings > Transcription > Local      2  (surface built)
  Settings > Transcription              1  (surface built)
  Settings > Meetings                   1  (surface built)
  Integrations > Calendar               1  (surface built)
  Notes > Upload                        1  (surface built)
```

- **61 of 102 integrations are reachable**, up from 8 when this plan was written and 45 when this
  section was last written mid-run. **6** are implemented-or-declared and unreachable, down from 19
  — and every one of them names a *built* surface, which was not true when this section was first
  drafted; the other 35 are removed vendor services and internal platform capabilities with no
  surface by design. The count was 7 at the close of this plan, became 8 when URL import was
  withdrawn as an SSRF surface the application could not police (chunk G), and then 6 when the two
  Parakeet models got the selector B4 had left unbuilt.
- **0 of 13 UI surfaces do not exist.** All 13 are built, one component each; `App.tsx` is the
  shell. `Notes > Upload`, `Integrations > CLI` and `Integrations > MCP` — the last three that did
  not exist — were built in chunks G and H below.
- The keystone closed early and stayed closed: `Settings > Providers` stores a key for every one of
  the **seventeen** slots the registry now declares (twelve providers, five per-feature custom
  endpoints — up from ten and fifteen total, once `credential:assemblyai` and `credential:deepgram`
  were added for meeting streaming in chunk E). Before chunk A, `provider_credential_set` existed in
  Rust and `setProviderCredential` was exported from `ipc.ts` and **nothing called either**, so
  every cloud provider was unreachable regardless of backend state.
- **The six that remain unreachable, each for a specific, named reason rather than "not done
  yet":** two local-ASR models (Nemotron — streaming, over `sherpa-onnx-online-websocket-server`'s
  own protocol, for which nobody has written a client; listed on the screen with that reason rather
  than hidden), two Tinfoil entries (batch transcription and meeting-stream,
  both declined on principle — an unattested client would present as Tinfoil while providing none of
  the confidentiality the name means), Apple Calendar (EventKit — described in
  `kw_calendar_core::apple`, implemented nowhere, on any platform, for lack of a macOS machine), and
  URL import (withdrawn as a command: the up-front literal-host check cannot see where a hostname
  resolves at connect time, nor any redirect or extractor-generated request `yt-dlp` makes on its
  own — the backend is dormant, not deleted).

So this plan front-loaded the screens, then closed genuine backend gaps behind them, then did the
platform work that no amount of UI could substitute for — meetings, calendars, media import, MCP,
packaging — leaving only the AEC helper and two diarization/search entries in the deliberately-deferred
state chunk I and chunk E4 describe below.

## Decisions taken (2026-08-25)

1. **Hand-rolled view switch**, not react-router. A sidebar plus a `view` state, one component per
   surface at the paths `config/ui-surfaces.json` already names. No new dependency; the hash is
   already claimed by `#overlay`.
2. **Generate the IPC types from Rust.** Three of the five chunk-1 defects were one thing — a
   TypeScript type describing a Rust struct, written by hand. Nine new surfaces would multiply it.
3. **Write blocked work and mark it unverified**, following the macOS precedent, with the gap
   visible in a type and in `PLATFORM_SUPPORT.md` rather than only in prose.
4. **Start with the shell and the credential screen.** Pure UI over backend that already exists,
   and it is the keystone.

## Where each phase stands

| Phase | Status | The gap |
| --- | --- | --- |
| 0 — Audit and contracts | Complete | — |
| 1 — Tauri foundation | Complete | 4 of 6 window roles exist (Dictation panel, Control panel, Meeting notification, Update notification); Agent overlay is phase 10 scope and Transcription preview has no task in this plan. Linux/macOS hotkey fallbacks unwritten. Modifier-only shortcuts work on Windows through a keyboard hook. Four of the five hotkey actions have no feature behind them and are reported as such rather than registered |
| — Chunk A (shell, types, Providers) | **Done, unchecked by hand** | Storing a key and passing a connection test has not been done by a person on this machine — still true at the close of this plan; see `BENCH_CHECK.md`'s "still unchecked" list, item 3 |
| 2 — Dictation | **Verified on Windows** | 6 of 7 transcription providers reachable; Tinfoil declined pending enclave attestation. Both Parakeet local-ASR models are now reachable: `Settings > Transcription > Local` installs the pinned sherpa-onnx CLI and a model and selects it, and dictation resolves to exactly one local engine. `parakeet-unified-en-0.6b` is the one proven end to end; `parakeet-tdt-0.6b-v3` is offered on the same measured layout and has never been run. The 2 Nemotron models stay blocked on a streaming protocol nobody has written a client for — see B4 |
| 3 — Reasoning | Complete, enterprise unverified | All three enterprise shapes written and tested against vectors and fixtures; no live call, no organisational credentials for any of the three. (The `llama-server` sidecar this phase's own chunk B3 built is done and pinned — the earlier version of this row said it was still missing, which stopped being true the same session it was written) |
| 4 — Meetings and notes | **Complete**, three refusals kept | System audio, the meeting notification window, all five detectors (`zoom`/`teams`/`webex`/`facetime`/`generic`), and 4 of 5 WebSocket clients (`openai-realtime`/`assemblyai-realtime`/`deepgram-realtime`/`corti-realtime`, plus `meeting-stream:local`) are done. `meeting-stream:tinfoil-realtime` stays declined, for the identical reason as the batch client. The AEC helper stays `temporarily-deferred` — no C++ toolchain on this machine, and it gates quality (double-transcribed far-end audio on speakers), not whether a meeting records at all |
| 5 — Calendars | **Complete** | Google and Microsoft: loopback PKCE, calendar list and incremental sync, all reachable — no client ID on this machine, so no live call has been made. Apple Calendar: `temporarily-deferred`, described in `kw_calendar_core::apple` and implemented nowhere, for lack of a macOS machine |
| 6 — Local API and CLI | **Verified on Windows** | `Integrations > CLI` is now built. (Chunk H built its screen and phase 7's `Integrations > MCP` screen together, in one task, since both render the same discovery-permissions state) |
| 7 — MCP | **Complete** | `tools/knegiswhisper-mcp`, a stdio server (~1573 lines incl. tests), reusing the local API's tool surface and token comparison. 28 crate tests plus protocol tests. Unverified by a person driving a real MCP client — see `BENCH_CHECK.md` |
| 8 — Media import, diarization, semantic search | Local media import complete; URL import withdrawn; diarization and search deferred | `media:{audio,video}-import` and `media:openwhispr-import` are reachable through `Notes > Upload`. `media:url-import` was reachable at the close of this plan and is not now: the command was unregistered because the fetch belongs to `yt-dlp` and cannot be policed — see chunk G. The two diarization models and `search:semantic` stay `temporarily-deferred` — Qdrant sidecar, ONNX Runtime, embeddings, none attempted this run |
| 9 — Packaging | Written, unrun | `createUpdaterArtifacts: true` now (chunk J), release/CI workflows exist and pass locally, but no signing identity exists, no release has ever been published, and **CI triggers remain commented out by design** — chunk J's own value is deferred until someone uncomments them, so every gate in this document runs only when a person runs it |

## The unlock arithmetic

Why the order below is what it is. Each chunk's integrations-made-reachable:

| Chunk | New backend | Integrations unlocked |
| --- | --- | --- |
| A — shell + Providers *(done)* | 2 commands | **18**: 15 credential slots + 3 transcription providers (the registry grew two more provider slots later, in chunk E, for AssemblyAI and Deepgram — seventeen total by the close of this plan) |
| B — Intelligence *(done but B4)* | 3 enterprise shapes, llama sidecar | 7 reasoning + 3 enterprise + 6 local LLM = **16**, all reachable |
| C — transcription clients *(done)* | 3 clients; the fourth declined | **6** of 7 transcription; Tinfoil needs enclave attestation |
| D — system audio + natives *(done)* | large | 0 directly; unblocked E entirely, which is exactly what happened |
| E — meetings *(done)* | 3 new clients (AssemblyAI, Deepgram, Corti) + 5 detectors | **5 of 6** streams reachable (`openai-realtime` predates this plan; `local` needs no key; Tinfoil stays declined) + 5 detectors, all built |
| F — calendars *(done)* | new crate | **2 of 3** (Google, Microsoft; Apple `temporarily-deferred` for lack of a macOS machine) |
| G — media import *(done)* | ffmpeg, yt-dlp sidecars | **4 at the time, 3 now**: audio/video/url import on the two new sidecars, plus `media:openwhispr-import` — already written and tested, newly given the screen that made it reachable. URL import was withdrawn after this plan closed; see the chunk G section |
| H — MCP + CLI screens *(done)* | MCP server | **2** |
| I — diarization + search *(not attempted this run)* | sherpa + Qdrant | **0**. Neither `ort`, Qdrant, nor the diarization/semantic-search work was started; both entries stay `temporarily-deferred`, unchanged since before this plan. (The 2 Parakeet local-ASR models Task 18 measured and proved are chunk B4's local-ASR work, not this chunk — they stay `reachable: false` for want of a Settings selector, deliberately not built as part of that measurement task) |

---

## Chunk A — the shell, generated types, and the credential screen *(done)*

The keystone. Two new commands rather than the none this plan predicted — `credential_slots` and
`dictation_set_cloud_provider`, both of which only read and write things that already existed. What
forced them was the same discovery twice: a screen cannot render fifteen slots from a registry that
knows nine, and a stored key does not make a provider reachable if nothing can select it.

### A1. Type generation spike, first and gated *(done — `ts-rs` 12.0.1)*

`ts-rs` expresses `rename_all_fields` on an internally tagged enum; `specta` and the custom
emitter were not needed. One writer renders the whole file rather than `#[ts(export)]` per type,
because the types live in eight crates whose test binaries run concurrently. The check is a
`cargo test`, not a fourth CI step. Reasoning and the two defects it exposed:
[ENGINEERING_LOG.md](ENGINEERING_LOG.md).


Before anything depends on it, prove the generator can express what this codebase actually uses:
**13 files carry `rename_all_fields = "camelCase"`** on internally-tagged enums
(`crates/overlay/src/lib.rs` ×3, `crates/sidecar/src/manifest.rs` ×3,
`apps/desktop/src-tauri/src/local_model.rs` ×2, and 8 more).

- Add `ts-rs` to one crate and derive `TS` on `overlay::CaptureExclusion` — the hardest shape in
  the tree: internally tagged, kebab-case variants, camelCase struct-variant fields, with a nested
  `Evidence`.
- Compare the emitted TypeScript against the hand-written union in `ipc.ts:58-98`.
- **If `rename_all_fields` does not survive**, fall back in this order: `specta` (better serde
  coverage), then extending `tools/integration-audit/generate-ts-types.mjs` with a serde-attribute
  reader. Do not proceed to A2 having guessed.

Then roll out to the ~40 command return types and the 13 tagged enums, emit
`apps/desktop/src/generated/ipc-types.ts`, and add a `--check` mode to CI beside the three checks
that already exist. Hand-written `invoke()` wrappers stay — the defect was never in the wrappers.

*Reuse:* the `--check` pattern in `generate-ts-types.mjs`, and the CI step shape in
`.github/workflows/ci.yml` (contracts job).

### A2. The shell *(done)*

`App.tsx` becomes shell and navigation only. One component per surface, at exactly the paths in
`config/ui-surfaces.json` — the parity gate reads that file and requires each `built` surface to
exist **and** render `data-surface="<surfaceId>"`.

```
src/App.tsx                  shell, sidebar, view state, error boundary
src/views/Dictation.tsx      moved out of App.tsx
src/views/SettingsTranscription.tsx        (settings.transcription, + .local)
src/views/SettingsProviders.tsx            (settings.providers)          NEW
src/views/Notes.tsx                        (notes.export)
src/views/IntegrationsApi.tsx              (integrations.api)
src/views/Diagnostics.tsx     overlay evidence, platform support
src/lib/useRefresh.ts         the shared refresh/guard pattern from App.tsx:312
```

Keep the existing `refresh()` + `guard()` shape — it already re-reads after every mutation, which
is why the hotkey row updates correctly. Lift it into a hook rather than reimplementing per view.
`styles.css` (261 lines) gains a two-column layout; no CSS framework.

### A3. Settings > Providers *(done)*

The screen that unblocks everything else. Renders from
`apps/desktop/src/generated/providers.ts` (already generated, 1413 lines) — never a hand-written
provider list.

Per slot: whether a secret is stored (`provider_credential_present`), a password input that
**writes and is cleared, never read back**, a Clear action, and the real connection test from
`probe_providers` — which already refuses to call a provider connected on a 2xx alone.

**Reconcile first, in the same chunk:** the registry declares **9** credential slots
(`openai anthropic gemini groq berget xai mistral corti tinfoil`) and the manifest lists **15**
`credential:*` entries — the extras being `openrouter` and five custom-endpoint slots
(`note-formatting-custom`, `translation-custom`, `dictation-agent-custom`,
`dictation-agent-vision-custom`, `chat-agent-custom`). A screen that invents the missing six
contradicts "declared once" in [ARCHITECTURE.md](ARCHITECTURE.md). Add them to
`config/provider-registry.json` with the custom slots modelled explicitly, regenerate, and let
the manifest and the registry agree.

*Files:* `config/provider-registry.json`, `config/ui-surfaces.json` (flip `built`),
`config/integration-manifest.json` (flip `reachable` for the 15 + the 3 HttpBatch transcription
providers), `apps/desktop/src/views/SettingsProviders.tsx`.

---

## Chunk B — Intelligence: reasoning, enterprise, local LLM

### B1. Settings > Intelligence (7 reasoning providers) *(done)*

Backend was done: `reasoning-core::{route, chat, params}` dispatch, with the cloud set **derived
from the registry** so an unknown id fails closed. `DictationRuntime::build` was handed
`Arc::new(RwLock::new(None))` for cleanup with a comment saying a half-configured cleanup is worse
than none, and nothing could make it a `Some`.

`apps/desktop/src-tauri/src/reasoning.rs` now holds `CleanupSettings`, two commands, and the
startup path that restores the configuration. The runtime keeps the shared slot so a change takes
effect on the next dictation rather than after a restart. Three things it refuses: to guess a model
(providers disagree about `max_tokens` versus `max_completion_tokens`), to hide the ten providers
that cannot be used, and to report cleanup as on when a missing key means it will silently leave
the transcript alone.

### B2. Settings > Intelligence > Enterprise (3 shapes) *(done)*

`crates/reasoning-core/src/enterprise.rs` and `sigv4.rs`. The `ShapeNotImplemented` arm is gone.

- **Bedrock:** SigV4 written out rather than taken from `aws-sdk-bedrockruntime`, which would
  bring its own HTTP stack around `network-policy` and a credential *chain* that reads the
  environment, `~/.aws`, and IMDS. Verified against AWS's five published vectors, including
  through the public `sign` entry point. Plus the Converse body: `system` is not a message,
  content is a list of blocks, and the token cap is `inferenceConfig.maxTokens`.
- **Azure OpenAI:** deployment in the path and selecting the model, `api-version` as a **query
  parameter**, and `api-key` rather than a bearer token. Sending `api-version` as a header gets a
  401, which reads as a credential problem.
- **Vertex:** service-account JWT signed RS256, exchanged for an access token, region in the host
  (`global` has its own), and the publisher prefix on the model id.

Configuration is one JSON document per connector in **one credential-store slot** — three slots
added to the registry, since upstream kept these in loose settings fields. Not split across
settings and the keychain: a struct half in each is a struct reassembled wrongly.

Live calls stay unverified and `PLATFORM_SUPPORT.md` now has a table saying so per provider.

### B3. Local LLM — the llama-server sidecar (6 providers, ~20 models) *(done)*

`ChatRoute::Local` pointed at `http://127.0.0.1:8221/v1` and nothing served it. Now something does.

- **`tools/sidecar-pin/pin.mjs` had to be written first.** `config/sidecars.json` has instructed
  people to use it since the first sidecar, and it did not exist — so the one rule protecting the
  digests was unfollowable. It downloads while hashing, records the *observed* size, and reads the
  archive index to find the executable, so `executableInArchive` is measured too. Its first act was
  to reproduce all three committed whisper pins exactly, which validates the tool and the pins at
  once. `--verify` re-measures every published target: the only thing that notices a release
  retagged in place.
- `llama` is pinned at `ggml-org/llama.cpp` `b9763`, ports **8221–8240**, all five targets
  measured — including **both macOS ones**, which whisper.cpp does not publish. CPU builds: an
  accelerated build is 15–25× the download, needs a matching driver, and fails at launch in a way
  that reads as a broken install.
- GGUF download through `network-policy` with `.partial`-then-rename, held to an **exact** length
  and to a **SHA-256**, both from the registry, and the partial deleted on either refusal. All 39
  published objects — 35 models and 4 drafters — carry a digest as of 2026-08-31. Seven were
  downloaded and hashed here, one per publishing organisation; the other thirty-two are Hugging
  Face's stated `lfs.oid`, which is a real digest of the object but arrives over the same channel
  as the bytes. **The screen says which.** *checksum verified* and *checksum verified against the
  publisher's own* are different rows, because they are different claims (KNEG-45). The URL names
  the commit the digest was measured against — `resolve/{sha}`, never `resolve/main` — so a
  publisher pushing cannot move the bytes out from under it (KNEG-103).
- **5-minute idle auto-stop.** `LocalEndpoint` is the trick: reading the port is what records a
  use, so there is no `touch()` for anyone to forget, and a settings screen reads through
  `running_port()` which does not.
- `Chat` no longer hardcodes `8221`. It took a real port, because `port::allocate` searches
  8221–8240 and a second instance would otherwise have had its requests sent to the first one's
  server.

Two things the work turned up. `DictationRuntime::build` reached eight arguments, so the three
local destinations became a `LocalTargets` struct — the shape was inviting a ninth, and the ninth
is sherpa. And `local_model` and `local_llm` both had an `InstallProgress`, which in one
TypeScript file is a redeclaration where the later one silently wins; `ipc_types` now refuses a
name collision by assertion.

### B4. Local ASR — sherpa-onnx (4 models) *(Parakeet done; Nemotron deferred, on purpose)*

The plan said "same sidecar pattern". It is not. Looking at
`k2-fsa/sherpa-onnx` `v1.13.4` — 303 assets — turned up three prerequisites the whisper and llama
sidecars did not need:

1. **Everything is `.tar.bz2`.** 276 of the 303 assets, including every desktop binary bundle
   *and* all four model archives the registry already points at. The four `zip` assets are macOS
   xcframeworks, which are libraries rather than executables — the same situation whisper.cpp is
   in on macOS. `sidecar::manifest::Archive` knows `zip` and `tar-gz`, `install::extract` handles
   those two, and `tools/sidecar-pin` reads those two. All three need a third.
2. **There is no HTTP server to talk to.** whisper.cpp and llama.cpp both ship a binary that
   serves an HTTP API on a port, which is why `Transport::Loopback` and `HttpHealth` fit them.
   sherpa-onnx ships `sherpa-onnx-offline` (a one-shot CLI that reads a WAV and prints) and
   `sherpa-onnx-online-websocket-server` (its own WebSocket protocol). Neither is an HTTP endpoint.
   The offline CLI would be `Transport::Stdio`, which `sidecar::manifest` already supports; the
   streaming ones need a client for a bespoke protocol, which is chunk E's kind of work.
3. **The argument and directory contract is per-model.** An extracted model is a directory of
   encoder/decoder/joiner/tokens files whose layout differs between the Parakeet and Nemotron
   families, and the CLI flags follow. That cannot be written correctly from a release page; it
   needs the binary and one model on a machine, which is a ~330 MB download to find out.

So the four models stay unreachable, and the reason is now specific rather than "no sidecar". What
would make it tractable, in order: a pure-Rust bz2 decompressor in `install::extract` (`bzip2-rs`
is decompress-only, which is all that is needed, and matches why `flate2` is on `rust_backend`);
then the two Parakeet models over `Transport::Stdio`; then the two Nemotron ones, which are
streaming and belong with the meeting clients.

Adding bz2 support on its own was considered and rejected: an archive format with no consumer is
the scaffolding this workspace deliberately avoids, and the commit that adds it should be the
commit that uses it.

**Update (Task 18): the tractability list's first two steps are done.** bz2 support landed with
its consumer (Task 17, `ArchiveKind::TarBz2`, the sherpa-onnx CLI pinned for all five platforms).
This task did the third: `cargo run -p kw-transcription-core --example sherpa_probe` installed the
real CLI and both pinned Parakeet archives and drove a real transcription with each, rather than
guessing at the contract from the release page as point 3 above said could not be done safely.
Corrections to what the plan assumed, found by actually running it:

- **No `Transport::Stdio` variant was needed.** `crates/transcription-core::local::LocalTranscriber`
  already existed as the seam `WhisperServer` implements; `SherpaOffline` is a second
  implementation of the same trait, and `route::Transport::Local` already says engine selection
  stays with the local manager. `kw_sidecar::manifest::Transport::Stdio` (already declared for
  sherpa in `config/sidecars.json`) describes how the *sidecar binary* is installed and run, which
  is a different question from how a transcription request is routed — the plan's phrasing
  conflated the two.
- **The measured layout is flat, not nested**, and identical between the two Parakeet models:
  `encoder.int8.onnx`, `decoder.int8.onnx`, `joiner.int8.onnx`, `tokens.txt` directly in the model
  directory, plus a `test_wavs/` sample folder. See docs/PLATFORM_SUPPORT.md's "Local ASR:
  sherpa-onnx" section for the full measurement, including a real bug the measurement caught (the
  registry's `sizeMb` is the extracted footprint, not the download size, so a byte-count check
  against the download had to be replaced with a check that the real files are actually there).
- **Both Parakeet models transcribe the same fixture whisper's `tiny` model does, correctly** —
  same words, differing only in capitalisation. `kw_transcription_core::sherpa::SherpaOffline` and
  `sherpa_models::install` are real, tested code, proven end to end by a `--ignored` integration
  test (`tests/sherpa_dictation.rs`).
- **`local-asr:parakeet-tdt-0.6b-v3` and `local-asr:parakeet-unified-en-0.6b` stay
  `reachable: false`, on a much narrower ground than before.** A first pass through this task flipped
  both to `true` on the strength of the transcript above, on the reading that "the probe proved it
  works". That reading conflated *the runtime works* with *a user can reach it*, which is exactly the
  distinction this project already drew for `calendar:apple`: reachable means configurable through
  the named, *built* surface, and `Settings > Transcription > Local` has no selector for a Parakeet
  model — `SettingsTranscription.tsx` said so directly, in words that contradicted the manifest's own
  claim. Corrected before this chunk closed. The Tauri command layer and Settings UI wiring this
  list's remaining work assumed would follow immediately — `apps/desktop/src-tauri/src/dictation.rs`'s
  `LocalTargets` comment already names this as the next shape change — is what actually turns these
  two `true`.
- **The two Nemotron models stay exactly where point 2 above left them**: streaming, over
  `sherpa-onnx-online-websocket-server`'s own protocol, "chunk E's kind of work" — not attempted by
  this task, and not offline-shaped, so the offline CLI measured here says nothing about whether it
  would serve them.

---

## Chunk C — the four transcription clients

*(three of four done; the fourth is refused on purpose)*

`crates/transcription-core/src/clients.rs` shapes the requests; `executor.rs` sends them.

| Provider | State | The difference that matters |
| --- | --- | --- |
| **Mistral** | done | Its own model id, required — no default worth inventing, since a guess is a 422 that reads as a broken integration. |
| **xAI** | done | **Takes no model parameter.** The route resolves `grok-stt` for its own bookkeeping and it is deliberately not forwarded. `format=true` travels only alongside a language, because the route has already dropped anything outside xAI's 25. |
| **Corti** | done | OAuth2 client-credentials exchanged first, environment in the hostname, tenant as a header, and a response shaped as a list of utterances rather than a `text` field. |
| **Tinfoil** | **refused, deliberately** | See below. |

**Tinfoil is not unfinished; it is declined.** Its whole proposition is an attested enclave: the
client verifies an attestation document against a hardware root of trust before sending anything.
A plain HTTPS POST to the same host would transcribe correctly and provide none of that, while
appearing in the UI as "Tinfoil" — a confidentiality claim the request did not earn. `route.rs`
already refuses a Tinfoil host configured as a plain custom endpoint for exactly this reason, and
implementing the client as an unattested POST would defeat that refusal from the inside. The error
now carries that reason rather than "not implemented yet".

**A real defect surfaced on the way.** `route.rs` defaulted Corti's environment to `us` and its
tenant to `base`. Those are not placeholders — they compose a live endpoint under a tenancy nobody
chose, so an unconfigured Corti would have sent the user's audio and their credential there. It
refuses now, naming which of the two is missing, and `Settings > Transcription` grew the two
fields. The tests that swept every registry provider had been passing *on those defaults*, which
is how they survived.

Live calls stay unverified: each needs a key this project does not have.

---

## Chunk D — system audio and the Windows natives

The largest backend chunk, and **the thing that blocks meetings** — not the AEC helper.

### D1. System audio behind a trait *(done, and verified on hardware)*

`crates/audio-core/src/{system_audio,mix}.rs`, and **no new dependency**. The plan called for the
`wasapi` crate replacing upstream's 634-line `windows-system-audio-helper.c`; `cpal` — already a
dependency — does WASAPI loopback, and says so in its own documentation. The Windows path is: take
the default *output* device and open an *input* stream on it.

One thing only a real run could find. `cpal` sets `AUDCLNT_STREAMFLAGS_LOOPBACK` inside
`build_input_stream` when the device is a render endpoint, but `default_input_config()` *refuses* a
render endpoint outright — "the requested stream type is not supported by the device". The mix
format has to come from `default_output_config()`. Reading the documentation had not found that;
running it found it in one attempt.

**Verified on this machine:** 97 920 samples, 2 channels at 48 kHz, peak 1.0, captured from
`Speakers (Realtek(R) Audio)` while a sound played, then mixed with a microphone track.
`cargo run -p kw-audio-core --example system_audio_probe` reproduces it.

- **Windows:** WASAPI loopback through `cpal`. Verified.
- **Linux:** a PulseAudio or PipeWire `.monitor` source, which `cpal` lists as an ordinary input.
  Written, and reported as `unverified: true` so a screen cannot present it as tested.
- **macOS:** `Unsupported`, with the reason — `AudioHardwareCreateProcessTap` (14.4+) or a
  user-installed virtual device, neither of which `cpal` exposes. Not written, because unbuildable
  `cidre` code that no toolchain here can even compile is worse than an honest refusal.

`mix.rs` is the pure half and is fully tested: align at the start, pad the shorter track rather
than truncate the longer, and sum-then-limit rather than divide by the number of sources — because
dividing halves every passage where only one source is active, which is most of a meeting.

### D2. The three Windows capabilities, in Rust not as binaries *(the key listener is done)*

- **Key listener** *(done, and verified on hardware)*. `crates/platform/src/keys.rs` is the pure
  state machine and `keyhook.rs` is the `WH_KEYBOARD_LL` plumbing. One hook on one thread inside
  the process, replacing upstream's one child process per watched key.

  Four rules, each a test: fire once per press rather than on every repeat; a superset does not
  match, so `Control+Super` stays quiet during `Control+Shift+Super`; left and right are distinct
  keys, which is what makes `RightControl+Super` possible at all; and an ordinary key held
  disqualifies every modifier-only chord, which is what stops the hook firing while somebody types.

  It refuses a chord containing an ordinary key. That is the boundary with the platform's own
  shortcut register, which arbitrates such chords against other applications — claiming them here
  as well would mean two mechanisms firing for one keystroke.

  It does not swallow keystrokes and it does not log them. `Control+Super` still reaches the shell.
  What leaves the module is the index of a chord that matched, never a key.

  **Verified on this machine:** installed, fired exactly twice for two injected presses, uninstalled
  cleanly. `cargo run -p kw-platform --example keyhook_probe` reproduces it.

  Two consequences worth noting. `Accelerator::explain` no longer asserts that the hook is
  unimplemented — it asks. And `Registration` gained a `Hooked` state distinct from `Registered`,
  because a hooked chord does not take the keystroke away from other applications and reporting
  both as "registered" is the small lie that surfaces when the Start menu appears anyway.

- **Text monitor** — UIAutomation focused-element read. Still not written. Chunk E (meetings) is
  now done and did not end up needing it — the meeting screen, the five clients and the notification
  window none of them read the focused element. Left here rather than deleted, since nothing in
  this plan built it and no task claimed otherwise.
- **Mic listener** — `IAudioSessionManager2`, for "another application is using the microphone".
  Still not written, same reason: chunk E shipped without it. `meeting-detect.rs:120`'s own comment
  still names it as the dependency for microphone-activity detection, which chunk E also did not
  attempt (the five detectors that did ship are all process-based, not audio-based).

### D3. Linux and macOS shortcut fallbacks

`zbus` against GNOME, KDE and Hyprland portals; Wayland restricts global shortcuts and synthetic
input, and `PLATFORM_SUPPORT.md` must keep saying so per compositor.

---

## Chunk E — meetings *(done, except the AEC helper — deferred on purpose)*

### E1. Detection (5 detectors) *(done)*

`meeting-detect:{zoom,teams,webex,facetime,generic}` via `sysinfo`'s process table — the same
crate `sidecar::reap` already uses to verify a command line before signalling. Port
`meetingProcessDetector.js`. **`meeting_detect_probe` found `ms-teams.exe` on this machine** without
a bench check being run — the only detector row in `PLATFORM_SUPPORT.md` that says "seen to match"
rather than "written, unverified".

### E2. Three of the five WebSocket clients *(done; the fourth declined; the fifth predates this plan)*

`tokio-tungstenite`, one per provider, against the existing `MeetingTranscriptionProvider` trait
and `MeetingProviderCapabilities` (`crates/meeting-core/src/capabilities.rs`, 529 lines and
already written). The capability model is the specification: AssemblyAI caps keyterms at 100,
Deepgram uses repeated query params, Corti a JSON config block, and **OpenAI Realtime and Tinfoil
silently ignore keyterms** — so the client must not pretend otherwise. Honour `session_limit`
(OpenAI: 60 minutes) and `token_rotation` (Deepgram only, and only as a declared capability — with
BYOK there is nothing to rotate, and a test proves no rotation clock exists so the flag cannot
quietly grow one).

`is_allowed` stays **derived** from the client registry: a permitted provider with no client must
be a compile error, never a fall-through to OpenAI. Also `meeting-stream:local`, which needs no
keys and is the one that can be verified here.

**AssemblyAI, Deepgram and Corti are written and tested against fixtures; none has been called
live** — no key exists on any machine this project has touched for any of the three, and Corti's
*socket* wire specifically (endpoint path, message envelope, most of the session-config block) was
invented outright with no vendor documentation to check it against, a strictly weaker epistemic
state than AssemblyAI's and Deepgram's "read from public docs, unconfirmed". Each client got **two
WebSockets, one per capture** — overriding this plan's own "one socket, not two" premise, which was
wrong: `TranscriptEvent::source` is non-optional and none of these three providers' speaker labels
say *which stream* an event came from, only who spoke *within* one. A single socket would have made
`source` a guess for every event, the exact defect OpenAI Realtime's existing two sockets already
avoid. **Tinfoil stays declined**, for the reason chunk C already gives its batch client: an
unattested socket would stream audio correctly while presenting as Tinfoil in the UI, a
confidentiality claim it did not earn.

### E3. Settings > Meetings, and two more window roles *(done)*

The screen, plus the **meeting notification** window — content-protected, "visible to the user but
out of screen shares" — which is `overlay::apply_treatments` applied to a second window. The
overlay module already returns evidence rather than a boolean; reused unchanged.

### E4. The AEC helper *(temporarily-deferred; not attempted)*

The one binary this project intended to build for itself. `Transport::Stdio` already exists in
`sidecar::manifest::Transport`. Both third-party pins are settled and measured: abseil by archive
sha256, webrtc-apm by a **content digest over its extracted tree** because googlesource generates
that tarball per request and it is not byte-stable. What remains — vendoring six source files,
porting the 446-line generator, and a five-target workflow with `contents: write` — needs a C++
toolchain (cmake, ninja, clang), and none is installed on this development machine, which is also
why every decompressor this workspace added (`bzip2-rs` for sherpa, `lzma-rs` for the archive
formats before it) is pure Rust rather than a C binding. **Declined for now, not unfinished** —
`config/integration-manifest.json`'s `platform:aec` entry was corrected mid-run from
`ported-with-new-rust-implementation` to `temporarily-deferred`, because nothing implementing it
exists: no helper, no generator port, no workflow. It gates nothing meetings actually need: system
audio capture is what determines whether a meeting can be recorded at all, and that is done. Without
AEC, a meeting recorded on speakers transcribes the far-end voice twice; on headphones there is
nothing to cancel.

### A cross-cutting gap in all four cloud clients — *fixed 2026-08-31 (KNEG-79)*

`openai_socket.rs`, `assemblyai_socket.rs`, `deepgram_socket.rs` and `corti_socket.rs` all used to
treat a provider-reported error frame as `tracing::warn!` and never turn it into an `Err` from
`connect`/`send_audio`/`finish`. A server that accepted a malformed session-config frame, stayed
open, and never emitted a transcript let a meeting "complete successfully" holding an empty
transcript, evidenced only by a background log line nobody in the room can see. It predated every
task in this chunk and was shared equally by all four.

All four now record into `kw_meeting_core::fault::SessionFault` — one cell per session, shared by
both sockets, first fault wins — and both `send_audio` and `finish` consult it, so the meeting ends
as `MeetingOutcome::Failed` carrying the provider's own words. Whether a recorded fault ends the
session is one pure function, `fault::ends_the_session`, because nothing in this workspace can open
a socket in a test. `stop()` was fixed with it: a pump failure used to return early and leave the
screen stuck on "Recording" with no working button on it.

**Two halves remain open and are not this.** The failure is still silent *while* the meeting runs —
KNEG-95, which carries a design decision about whether a dead provider stops the recording, falls
back, or says so in a fourth `MeetingState`. And three of the four clients have still never met a
real provider error: no key exists for AssemblyAI, Deepgram or Corti. See `docs/BENCH_CHECK.md`
item 13 and the 2026-08-31 (KNEG-79) entry in `docs/ENGINEERING_LOG.md`.

---

## Chunk F — calendars (phase 5) *(done, Apple deferred on purpose)*

New `crates/calendar-core`. Google and Microsoft via **loopback PKCE public clients** — upstream
ships a client secret in the binary, which in an open-source build is not a secret. **Apple EventKit
is not "behind a trait, unverified" — it is described and implemented nowhere, deliberately.**
`kw_calendar_core::apple`'s own module doc: `AppleCalendarSource` is "deliberately unimplemented by
anything in this crate," on every platform this builds for today including macOS, because there is
no macOS machine in this development environment to write or verify an EventKit binding against.
The manifest's `calendar:apple` status was corrected mid-run (Task 19) from `platform-specific` to
`temporarily-deferred` for exactly this reason — the same overclaim `platform:aec` was already fixed
for, applied one file section later.

Refresh tokens became a `token_ref` naming a credential-store slot, exactly as planned. `storage`'s
`assert_no_credential_columns` fails if a credential-shaped column reappears, and the OpenWhispr
importer relocates upstream's plaintext `google_calendar_tokens.refresh_token` into the OS store.
Tested end to end against a stub authorisation server on loopback; the real round trip still needs
client IDs this project does not have, so Google and Microsoft are `reachable: true` with **no live
call ever made** — the wire shapes (`calendarList`/`events.list`, `/me/calendars`/`events/delta`) are
covered by fixtures modelled on each vendor's public documentation.

**A `TokenSender` trait seam was needed and settled as sound.** The loopback stub server the tests
need could not be reached through `kw_network_policy`, which correctly refuses plaintext for
`UserConfiguredCalendarProvider` — so a `PlaintextLoopbackSender` test double exists, confined to
its own test binary (`crates/calendar-core/tests/oauth_flow.rs`) and never linked into the shipped
binary. `NetworkPolicySender` is the only implementation the application ships, constructed at
exactly two sites, with no default and no fallback — stricter than this codebase's own
`CredentialStore` precedent, where `MemoryCredentialStore` does ship.

Three defects found and fixed during the by-hand and code-review passes: a partially-failed sync
that discarded a calendar-prune error with `let _ =` and still reported `SyncOutcome::Completed`; a
settings-read *failure* (not merely a first connect) being treated as "no previous account" and
skipping the auto-disconnect, which could leave a live, working refresh token in the OS credential
store the app could no longer see or revoke; and `NeedsRefresh` rendering unconditionally as
"Connected" even after a sync had recorded `TokenUnavailable` telling the user to reconnect. All
three fixed in the same task, with new tests proving the fix reached both provider paths and that
the new `RefreshFailed` status is actually produced, not merely defined.

---

## Chunk G — media import and the importer UI *(done)*

- `ffmpeg` and `yt-dlp` (`2026.08.19`) as `Transport::Stdio` sidecars → `Notes > Upload`
  (`media:audio-import`, `media:video-import`, `media:url-import`). The backend for all three is
  `apps/desktop/src-tauri/src/media.rs` (`MediaImportRuntime`) — the manifest said
  `crates/notes-core` until Task 19 corrected it; that crate holds no import code. **URL import was
  withdrawn after this plan closed**; see the last bullet in this section. `yt-dlp` stays pinned and
  installable, because the dormant backend still locates it.
- **`storage::import::from_openwhispr` got its screen.** It attaches read-only, introspects with
  `pragma table_info`, drops every cloud column, rebuilds FTS, and relocates secrets. Was
  deliberately absent from the manifest until it had a screen; both landed together, as planned.
- **URL import got its own purpose, `UserTriggeredMediaImport`, and did not become a general-purpose
  fetch** — but it also does not reach the network directly the way this line implied. `media.rs`
  makes exactly one direct use of `kw_network_policy`: a literal-host check (scheme, the vendor
  blocklist, a private/link-local host) *before* the URL is ever handed to anything that would fetch
  it. Fetching is yt-dlp's own job as a separate process, with its own DNS resolution and redirect
  handling, none of it observed or routed through the policy layer — bounded instead by flags yt-dlp
  enforces on itself (a 500 MB filesize cap, a bounded socket timeout). See
  `docs/NETWORK_BOUNDARY.md`'s "SSRF guards for URL import" section, corrected by Task 19 to say so.
- **And then URL import was withdrawn, after this plan closed** (`cf965ce`, merged through PR #9
  with five other security fixes). The gap above is not one a stricter up-front check can close: DNS
  rebinding beats a literal-host check by construction, because the check runs before any name is
  resolved, and yt-dlp's extractors generate requests this application never sees. So
  `media_import_url` is no longer a registered command, `Notes > Upload` has no URL field, and
  `media:url-import` is `reachable: false`. The backend is kept dormant rather than deleted — each
  item carries `#[allow(dead_code)]` pointing at `media.rs`'s `URL IMPORT, DORMANT` section, and its
  tests still run — so that re-opening it is a decision about the fetch boundary rather than a
  rewrite. **Audio and video import are unaffected**: they take a local path and fetch nothing.

**This is where Task 20 started.** Reviewing this chunk's guard, an implementer noticed
`kw_network_policy::validate`'s `https` arm ran no host check at all, for any purpose — a gap far
larger than media import, closed centrally rather than patched locally. See "the network boundary
does not stop SSRF over HTTPS" in `docs/ENGINEERING_LOG.md`.

**Two parity self-tests broke when `Notes > Upload` was finally built, and that was the system
working.** They had been written earlier in the run specifically to *find* an unbuilt surface and
fail loudly if none remained, and this was the last one. Fixed to use a synthetic surface — the
gate is pure and injectable precisely so a test can construct its own inputs rather than depending
on a real surface staying unbuilt, which was always the fragile choice.

---

## Chunk H — MCP and the last two screens *(done)*

`tools/knegiswhisper-mcp`, a **stdio** MCP server (upstream's was a hosted Streamable-HTTP
service; there is no local implementation to port) — ~1573 lines including 340 lines of protocol
tests, 28 crate tests. Reused the local API's tool surface (an exact 1:1 match against its route
table, in both directions) and its `LocalApiToken` constant-time comparison. Built `Integrations >
MCP` and `Integrations > CLI` together — the CLI screen shows the discovery-file path, its **real**
permissions, and the binary's location, reusing what `IntegrationsApi` already renders.

**One correction fixed during review, and one scope extension the controller ordered.**
`initialize` originally echoed any client-supplied `protocolVersion` unchanged — proven by its own
test asserting a fabricated `"2099-01-01"` came back — which defeats MCP's negotiation contract; a
server should answer with the version it actually supports so the client can decide. Fixed to gate
the echo through `SUPPORTED_PROTOCOL_VERSIONS`. Separately, building the CLI screen's honest
`DiscoveryPermissions` enum exposed that `IntegrationsApi.tsx` and the `LocalApiStatus` payload
still carried an old collapsed boolean and still soft-passed `InheritedFromProfile` as
acceptable — the identical caveat needed to live in one place, not two screens that could drift
apart the moment one was edited and the other was not. Both screens now render through one
`discoveryPermissionsLabel`, and the startup warning shares the same `is_confirmed()` decision.
`developer:agent-skills`, the third phase-7 entry, is documentation only and carries no backend or
UI surface of its own — but **the rewrite itself was never done.** It claimed `status: "ported"`
naming `docs/agent-skills/` as where the rewritten `SKILL.md` files live, and neither the directory
nor any rewritten file exists anywhere in this repository. Found by Task 19 (the gate's
documentation check only requires a target to be *named*, not to exist) and corrected to
`temporarily-deferred` rather than written under time pressure at the end of an unrelated task.

---

## Chunk I — diarization and semantic search (phase 8) *(not attempted this run)*

- Diarization: `ort` + the 2 `temporarily-deferred` sherpa models, plus a port of
  `diarizationPolicy.js` (63 lines, deliberately Electron-free upstream and therefore testable).
- Semantic search: Qdrant sidecar, ports **6333–6350**, and an embedding path. `search:semantic`
  is `temporarily-deferred`; `search:sqlite-fts` already works through the FTS5 external-content
  table and its three triggers.

None of this chunk's work was started across the 19 tasks in this plan or the one added mid-run.
`diarization-model:3dspeaker-campplus-en`, `diarization-model:sherpa-onnx-pyannote-segmentation-3-0`
and `search:semantic` remain exactly as `temporarily-deferred` as they were before this plan began,
each still carrying its own `deferral` block with dependencies and a named interface. Recorded here
rather than left silent, so a future session does not have to rediscover that this chunk is where
the plan actually stopped: everything else in chunks A–J is either done or explicitly, permanently
declined, and this is the one piece that is simply not-yet-begun.

---

## Chunk J — packaging, updater, CI (phase 9) *(written; CI still paused by design)*

- **CI stayed paused, deliberately.** `.github/workflows/ci.yml` still has its `push` and
  `pull_request` triggers commented out. Three workflow files were written and pass every gate
  they claim to run, dispatched manually and reviewed character by character for the one property
  that matters most given the Actions budget — none can fire on a push — but nobody uncommented the
  triggers. **This chunk's whole value is therefore still deferred**: every gate in this document
  has run only when a person or an agent ran it by hand this session, and every claim of "green"
  anywhere in this repository's history is a claim about the last run before the pause, not a
  standing fact CI is checking right now.
- **A scheduled `sidecar-pin --verify` job exists** (`pin-verify.yml`), for the reason planned: it
  re-downloads every pinned target, so it does not belong in the per-push contracts job, and nothing
  else would notice a release retagged in place.
- **The updater is built, and it checks less than "an update" implies, deliberately.** It only
  notifies — no download, no install, no signature verification — against this project's own
  GitHub releases, through `kw_network_policy::NetworkPolicy::get_text` under
  `RequestPurpose::OwnGithubReleasesUpdater`, **not** `tauri-plugin-updater` as originally planned
  here: that plugin ships its own HTTP client, and using it as intended would have put a second
  piece of code in the workspace opening its own connections outside the one crate whose whole
  claim is being the only egress point. `bundle.createUpdaterArtifacts` is `true` now. With no
  `plugins.updater.pubkey` configured, `check_now` returns `NoPublicKeyConfigured` **before any
  network request** — the decisive question ("does it claim authenticity it cannot back") answers
  no, twice over. Opt-in is structural: no caller, no timer, and deliberately no boolean setting
  that could be flipped without noticing. The **update notification** window role is built, reusing
  `overlay::apply_treatments` and `meeting_window.rs`'s shape unchanged.
- NSIS/dmg/AppImage/deb bundles build in `release.yml`, unsigned — no signing identity exists, and
  none has been generated. `docs/SECURITY.md` documents the keypair procedure for whenever one is.
- **No release has ever been published.** The workflow has never been dispatched for a real tag, so
  `GET /repos/{owner}/{repo}/releases/latest` has only ever had one possible answer (404) on every
  platform, and whether GitHub's actual response shape matches what `updater.rs` assumes, whether a
  `.sig` file actually appears next to a bundle, and whether `tauri build` tolerates
  `createUpdaterArtifacts: true` with no signing key present are all unverified in the ordinary
  sense — believed, not observed. See `docs/PLATFORM_SUPPORT.md`'s "Updates" section.
- Windows and Linux installs still unverified by a person; macOS unverified until a Mac exists.

---

## Cross-cutting, threaded through every chunk

1. **The gates stay green, always:** `cargo test --workspace`,
   `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`,
   `tsc --noEmit`, `vite build`, the domain scanner **and its 15 self-tests**, the parity gate
   **and its 10 self-tests**, `verify-binaries`, `generate-ts-types --check`,
   `generate-provider-registry --check`, `generate-baseline --check`, and the IPC-type check from
   A1. Quoted from the last run, not restated: `cargo test --workspace` reports **1296 passing, 0
   failed** (941 when this plan was written, 1290 at its close); the domain scanner reports **214
   files scanned against 22 patterns**; the parity gate reports **102 entries**, as the Context
   section above shows in full.
2. **Flip `reachable` and `built` in the same commit as the screen.** An entry claiming a surface
   that does not render `data-surface` fails rule 7 — that gate exists because 64 entries once
   named screens nobody had written.
3. **Never report success without evidence.** The `CaptureExclusion`/`Permissions`/`Verification`
   pattern extends to every new capability: a state, and what verified it.
   *Corollary, learned the hard way twice:* it applies to a capability that exists but has nothing
   behind it, not only to one that failed. Four hotkeys registered successfully and dispatched
   nowhere, and the keyboard hook fired for `Control+Super` and dictated nothing — both reported
   success from the half of the path that worked. See the honest-hotkeys entry in
   [ENGINEERING_LOG.md](ENGINEERING_LOG.md).
4. **The agent features have no phase in this plan.** Chat agent, voice agent and translation are a
   whole feature area — three hotkey actions, five `*-custom` credential slots, the
   `agent_conversations`, `agent_messages`, `actions` and `contacts` tables, and a 420×300 "Agent
   overlay" window role in [PORTING_MATRIX.md](PORTING_MATRIX.md) — and phases 0–9 do not contain
   them. [COMPLETION_PLAN.md](COMPLETION_PLAN.md) defines phase 10 as their home; this plan does
   not cover it.
5. **Each chunk ends with a by-hand check on Windows**, in the shape that found five real defects
   on 2026-08-25 and three more on 2026-08-26. [BENCH_CHECK.md](BENCH_CHECK.md) is the record and
   the template, and it opens with the rule that run established: a probe verifies the link it
   exercised and nothing beyond it, so say which link.

## Blocked, and what is not blocked by it

| Item | Needs | Not blocked |
| --- | --- | --- |
| Anything macOS-native | A Mac | Compiles in CI, ships marked unverified |
| ~~sherpa-onnx local ASR selector~~ | **Built on 2026-08-27.** `apps/desktop/src-tauri/src/local_asr.rs` plus a second list on `Settings > Transcription > Local`: three commands, a remembered selection, and one local engine at a time. `parakeet-unified-en-0.6b` was already proven end to end; `parakeet-tdt-0.6b-v3` is offered unrun on the same measured layout | — |
| sherpa-onnx streaming (Nemotron) | A client for `sherpa-onnx-online-websocket-server`'s own protocol — nobody has written or tested one | The offline CLI path the two Parakeet models use, which is unrelated |
| macOS `whisper-server` | A macOS CI job under this project's releases; upstream ships only an xcframework | Windows and Linux install today, digest-pinned |
| 4 meeting clients, live | Keys for AssemblyAI, Deepgram, OpenAI Realtime, Corti | Wire shapes vs fixtures; `meeting-stream:local`; detection; system audio — all built |
| 3 transcription clients, live | Keys for Mistral, xAI, Corti | The clients and their fixtures, both written |
| Tinfoil (transcription **and** meeting-stream) | Enclave attestation verification — certificate-chain and measurement checks against a hardware root | Nothing. Both are declined rather than pending — an unattested request would claim Tinfoil's confidentiality without providing it. Recorded `temporarily-deferred` for want of a better term; see `docs/ENGINEERING_LOG.md`'s "the ACTIVE-status audit" |
| Enterprise reasoning, live | AWS / Azure / GCP org credentials | SigV4 and body shaping vs published vectors |
| Calendars, real round trip | Google + Microsoft OAuth client IDs as PKCE public clients | Flow, storage, UI, stub server — all built |
| Apple Calendar | A macOS machine to write and verify an EventKit binding against, which does not exist in this development environment, and the binding itself, which does not exist anywhere in this repository | Google and Microsoft calendar sync, which are unrelated |
| AEC helper | cmake + MSVC/clang and a `contents: write` run | Meetings themselves — echo cancellation only matters on speakers, and system audio capture (what actually gates recording) is done |
| Diarization + semantic search (chunk I) | `ort`, the Qdrant sidecar, embeddings — none of it attempted this run | FTS5 keyword search, which is fully functional and is what the UI falls back to |
| The rewritten agent-skills docs | Someone to write `docs/agent-skills/`, referencing `knegiswhisper`/`knegiswhisper-mcp` instead of the vendor endpoints — the manifest claimed this was done; it was not, and Task 19 corrected the claim rather than writing the docs under time pressure | Nothing else — documentation only |
| Signed installers | A code-signing identity | Unsigned builds |
| CI actually running | Someone uncommenting `push`/`pull_request` in `.github/workflows/ci.yml` — deliberately not done this run | Every gate still runs by hand; it has, repeatedly, this session |
| A published release | Someone dispatching `release.yml` for a real tag — never done | The workflow itself, which builds locally-equivalent bundles on `workflow_dispatch` |

## Verification

**Per chunk:** the full gate list above, plus new tests at the seam each chunk adds — a client
against recorded fixtures, a sidecar against its readiness poll, a screen against the surface
registry.

**End to end, by hand on Windows, per chunk — and, per `docs/BENCH_CHECK.md`, none of chunks E
onward have actually been run this way.** What follows is still the checklist; the honest state of
each item is in that file, not here.

- **A:** store a key for OpenAI, run the connection test, see it pass; confirm the input never
  reads a stored secret back; confirm the parity gate's unreachable count drops. **Not yet done by
  a person** — `BENCH_CHECK.md`'s "still unchecked" list, item 3.
- **B:** dictate with cleanup on and see the tidied text; start llama-server and hold a chat;
  confirm the 5-minute idle stop. **Not yet done by a person** — same list, items 4–5.
- **C:** one live provider per client, if a key exists; otherwise the honest refusal.
- **D:** record system audio while a video plays, with the microphone muted, and confirm both
  streams; set a modifier-only hotkey and have it work. **Done and verified on hardware** —
  `BENCH_CHECK.md`'s 2026-08-26 entry.
- **E:** join a call in Zoom and see detection fire; run `meeting-stream:local` end to end; record
  through each cloud provider with a real key. **Not run.** `meeting_detect_probe` found
  `ms-teams.exe` on this machine without a full bench check, which is the only live evidence chunk
  E has. See `BENCH_CHECK.md`'s "Added by chunk E, and unchecked" list, 13 items.
- **F:** the stub PKCE round trip passes in tests; a real one needs client IDs this project does
  not have. **Not run by a person; no client ID exists.**
- **G:** import an OpenWhispr database and confirm the source file is unmodified; import a local
  audio/video file and a URL. **Not run by a person.**
- **H:** drive the MCP server from a client over stdio; use the CLI against a running application.
  **Not run by a person with a real MCP client** — the CLI itself was exercised on 2026-08-26.
- **J:** press "Check for updates" with no key configured and confirm the honest refusal message;
  generate a keypair, rebuild, and confirm a real request; dispatch `release.yml` once for real.
  **None of it run** — `BENCH_CHECK.md`'s "Added by Task 16" list, 5 items, all deliberately
  deferred rather than run against a shared development machine mid-session.

## Risks

- **A1 is a real gate, not a formality.** If neither `ts-rs` nor `specta` expresses
  `rename_all_fields` on internally-tagged enums, the fallback is a custom emitter — a day, not an
  hour. Everything after A1 depends on the answer, so it goes first and its result is recorded.
- **Chunk D turned out shorter than expected, for one reason.** `cpal` already did WASAPI
  loopback, so the largest single item — replacing a 634-line C helper — became a function that
  picks the output device. The risk the note was pointing at was real; it just did not land here.
  What remains of D is the text monitor and the mic listener, both of which serve meetings and are
  therefore behind chunk E rather than ahead of it.
- **The registry/manifest credential mismatch (9 vs 15) — settled in A3**, as planned, and grew
  again since: the registry now declares seventeen slots (twelve providers, five custom endpoints),
  after `credential:assemblyai` and `credential:deepgram` were added in chunk E. "Declared once"
  held through both changes — one registry, regenerated, never a hand-written second list.
- **`id=8` in the bench-check history** — a connection refused to `127.0.0.1:8178` while the
  application believed a server was live — is still not fully explained. It did not recur in any
  bench check run this session, including after B3 added `llama-server` as a second long-lived
  loopback sidecar, which is evidence against it but not an explanation of it. Nobody has gone
  looking for the root cause; it remains a known, unresolved gap rather than a closed one.
