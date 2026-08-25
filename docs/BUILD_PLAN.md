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

What the same period established is that **the remaining work is mostly reachability, not
capability**. The parity gate now resolves each manifest entry through `config/ui-surfaces.json`
and prints what cannot be configured:

- **36 of 99 integrations are reachable**, up from 8 when this plan was written. 28 are
  implemented-or-declared and unreachable; the other 35 are removed vendor services and internal
  platform capabilities with no surface by design.
- **6 of 13 UI surfaces do not exist.** The 7 that do are one component each; `App.tsx` is the
  shell.
- The keystone is closed: `Settings > Providers` stores a key for every one of the fifteen slots
  the registry declares. Before chunk A, `provider_credential_set` existed in Rust and
  `setProviderCredential` was exported from `ipc.ts` and **nothing called either**, so every cloud
  provider was unreachable regardless of backend state.

So this plan front-loads the screens, then closes genuine backend gaps behind them, then does the
platform work that no amount of UI can substitute for.

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
| 1 — Tauri foundation | Complete | 2 of 6 window roles exist; Linux/macOS hotkey fallbacks unwritten |
| — Chunk A (shell, types, Providers) | **Done, unchecked by hand** | Storing a key and passing a connection test has not been done by a person on this machine |
| 2 — Dictation | **Verified on Windows** | 4 of 7 transcription providers need their own client (`Transport::ProviderClient` → `ClientNotImplemented`); sherpa local ASR absent |
| 3 — Reasoning | Complete, enterprise unverified | All three enterprise shapes written and tested against vectors and fixtures; no live call. No llama-server sidecar for `ChatRoute::Local`, which already points at `127.0.0.1:8221` |
| 4 — Meetings and notes | Notes reachable; meetings contract only | No system audio anywhere; 5 WebSocket clients; 5 process detectors; AEC helper |
| 5 — Calendars | Not started | No crate, no OAuth flow |
| 6 — Local API and CLI | **Verified on Windows** | No `Integrations > CLI` screen |
| 7 — MCP | Not started | `tools/knegiswhisper-mcp` does not exist; upstream's was hosted, so this is new work |
| 8 — Diarization + semantic search | Not started | 2 diarization models (`temporarily-deferred`), Qdrant sidecar, embeddings |
| 9 — Packaging | Not started | `createUpdaterArtifacts: false`, no signing identity, **CI triggers are commented out** |

## The unlock arithmetic

Why the order below is what it is. Each chunk's integrations-made-reachable:

| Chunk | New backend | Integrations unlocked |
| --- | --- | --- |
| A — shell + Providers *(done)* | 2 commands | **18**: 15 credential slots + 3 transcription providers |
| B — Intelligence | 3 enterprise shapes, llama sidecar | 7 reasoning *(done)* + 3 enterprise *(done)* + 6 local LLM = **16** |
| C — transcription clients | 4 clients | **7** transcription (3 immediately from A, 4 from C) |
| D — system audio + natives | large | 0 directly; unblocks E entirely |
| E — meetings | 5 clients + 5 detectors | **6** streams + 5 detectors |
| F — calendars | new crate | **3** |
| G — media import | ffmpeg, yt-dlp sidecars | **3** |
| H — MCP + CLI screens | MCP server | **2** |
| I — diarization + search | sherpa + Qdrant | **4** (+4 local ASR from B's sidecar) |

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

### B3. Local LLM — the llama-server sidecar (6 providers, ~20 models)

`ChatRoute::Local` already points at `http://127.0.0.1:8221/v1`. Nothing serves it.

- `config/sidecars.json` gains `llama` — `ggml-org/llama.cpp` tag `b9763`, ports **8221–8240**,
  `transport: loopback`, digests **measured, not copied from a release note** (the standing rule).
- GGUF download reusing `transcription-core::models` (`.partial` then rename, `Integrity`/
  `Verification` reporting), extended for `hfRepo`/`fileName` and the 4 Gemma QAT models with
  speculative-decoding draft pairs (`draftHfRepo`).
- **5-minute idle auto-stop**, per the porting matrix. Reuse `sidecar::process::start` and the
  `HttpHealth` readiness poll; extend `LocalTranscription`'s lifecycle shape rather than inventing
  a second one — rename it to a shared `SidecarRuntime` if it generalises cleanly.
- Model sizes reach 5.9 GB. The install UI must show bytes and be cancellable, unlike the current
  86 MB path.

### B4. Local ASR — sherpa-onnx (4 models)

Same sidecar pattern: `k2-fsa/sherpa-onnx` `1.13.4`, ports **6006–6029**, **separate offline and
online binaries** (2 Parakeet offline, 2 Nemotron `runtime: "online"`). Feeds
`Settings > Transcription > Local`, which is already built and today offers whisper only.

---

## Chunk C — the four transcription clients

`crates/transcription-core/src/executor.rs:176` refuses `Transport::ProviderClient` honestly.
Four clients, each genuinely different (`route.rs:100`):

| Provider | Why it needs its own client |
| --- | --- |
| **Tinfoil** | Attested enclave; resolves its own model. Tinfoil hosts are already refused as plain custom endpoints — keep that. |
| **Mistral** | Provider SDK shape. |
| **xAI** | Takes no model parameter; already drops languages outside its 25-language set so `format=true` ITN stays valid. |
| **Corti** | OAuth2 client-credentials, tenant- and environment-scoped; `CortiConfig` has no safe global default. |

Test each against recorded wire fixtures. Live calls need keys and stay unverified.

---

## Chunk D — system audio and the Windows natives

The largest backend chunk, and **the thing that blocks meetings** — not the AEC helper.

### D1. System audio behind a trait

New `crates/audio-core/src/system_audio.rs`. `grep -ri wasapi crates/` is empty today.

- **Windows:** `wasapi` loopback capture. Replaces upstream's 634-line
  `windows-system-audio-helper.c`.
- **macOS:** `cidre` process taps, 14.4+ — written, unverified.
- **Linux:** PulseAudio monitor source via `libpulse-binding` — written, unverified.

Mixing microphone and system audio needs a shared clock and the existing ring-buffer discipline;
reuse `audio-core::{capture, wav, vad, gate}` and keep the two rules already documented: VAD off
by default for dictation, and the speech gate **fails open**.

### D2. The three Windows capabilities, in Rust not as binaries

Decision already recorded in [PORTING_MATRIX.md](PORTING_MATRIX.md); the code is unwritten.

- **Key listener** (`SetWindowsHookExW`/`WH_KEYBOARD_LL`) — the one with reach beyond meetings.
  `crates/platform/src/hotkey.rs:162` currently tells the user their modifier-only shortcut does
  nothing, which is honest and fixable. This is what modifier-only, right-side-modifier,
  Globe/`Fn`, mouse-button and push-to-talk hotkeys need. Upstream ran one process per watched
  key; an in-process hook replaces all of them.
- **Text monitor** — UIAutomation focused-element read.
- **Mic listener** — `IAudioSessionManager2`, for "another application is using the microphone".

### D3. Linux and macOS shortcut fallbacks

`zbus` against GNOME, KDE and Hyprland portals; Wayland restricts global shortcuts and synthetic
input, and `PLATFORM_SUPPORT.md` must keep saying so per compositor.

---

## Chunk E — meetings

### E1. Detection (5 detectors)

`meeting-detect:{zoom,teams,webex,facetime,generic}` via `sysinfo`'s process table — the same
crate `sidecar::reap` already uses to verify a command line before signalling. Port
`meetingProcessDetector.js`.

### E2. The five WebSocket clients

`tokio-tungstenite`, one per provider, against the existing `MeetingTranscriptionProvider` trait
and `MeetingProviderCapabilities` (`crates/meeting-core/src/capabilities.rs`, 529 lines and
already written). The capability model is the specification: AssemblyAI caps keyterms at 100,
Deepgram uses repeated query params, Corti a JSON config block, and **OpenAI Realtime and Tinfoil
silently ignore keyterms** — so the client must not pretend otherwise. Honour `session_limit`
(OpenAI: 60 minutes) and `token_rotation` (Deepgram only).

`is_allowed` stays **derived** from the client registry: a permitted provider with no client must
be a compile error, never a fall-through to OpenAI. Also `meeting-stream:local`, which needs no
keys and is the one that can be verified here.

### E3. Settings > Meetings, and two more window roles

The screen, plus the **meeting notification** window — content-protected, "visible to the user but
out of screen shares" — which is `overlay::apply_treatments` applied to a second window. The
overlay module already returns evidence rather than a boolean; reuse it unchanged.

### E4. The AEC helper

The one binary this project builds. `Transport::Stdio` already exists in
`sidecar::manifest::Transport`. Both third-party pins are settled and measured: abseil by archive
sha256, webrtc-apm by a **content digest over its extracted tree** because googlesource generates
that tarball per request and it is not byte-stable. What remains is vendoring six source files,
porting the 446-line generator, and a five-target workflow with `contents: write`. Ride the macOS
`whisper-server` job on the same workflow — same shape, and it is the last `not-published` target
in `config/sidecars.json`.

---

## Chunk F — calendars (phase 5)

New `crates/calendar-core`. Google and Microsoft via **loopback PKCE public clients** — upstream
ships a client secret in the binary, which in an open-source build is not a secret. Apple EventKit
behind a trait, unverified.

Refresh tokens become a `token_ref` naming a credential-store slot. `storage`'s
`assert_no_credential_columns` already fails if a credential-shaped column reappears, and the
OpenWhispr importer already relocates upstream's plaintext `google_calendar_tokens.refresh_token`
into the OS store. Testable end to end against a stub authorisation server on loopback; only the
real round trip needs client IDs.

---

## Chunk G — media import and the importer UI

- `ffmpeg` and `yt-dlp` (`2026.07.04`) as `Transport::Stdio` sidecars → `Notes > Upload`
  (`media:audio-import`, `media:video-import`, `media:url-import`).
- **`storage::import::from_openwhispr` has no UI.** It attaches read-only, introspects with
  `pragma table_info`, drops every cloud column, rebuilds FTS, and relocates secrets — and no user
  can run it. Deliberately absent from the manifest until it has a screen; add both together.
- URL import is the one place a user-supplied URL reaches the network. It needs its own
  `RequestPurpose` and must not become a general-purpose fetch.

---

## Chunk H — MCP and the last two screens

`tools/knegiswhisper-mcp`, a **stdio** MCP server (upstream's was a hosted Streamable-HTTP
service; there is no local implementation to port). Reuse the local API's tool surface and its
`LocalApiToken` constant-time comparison. Plus `Integrations > MCP` and `Integrations > CLI` —
the CLI screen showing the discovery-file path, its **real** permissions, and the binary's
location, reusing what `IntegrationsApi` already renders.

---

## Chunk I — diarization and semantic search (phase 8)

- Diarization: `ort` + the 2 `temporarily-deferred` sherpa models, plus a port of
  `diarizationPolicy.js` (63 lines, deliberately Electron-free upstream and therefore testable).
- Semantic search: Qdrant sidecar, ports **6333–6350**, and an embedding path. `search:semantic`
  is `temporarily-deferred`; `search:sqlite-fts` already works through the FTS5 external-content
  table and its three triggers.

---

## Chunk J — packaging, updater, CI (phase 9)

- **Unpause CI.** `.github/workflows/ci.yml` has its `push` and `pull_request` triggers commented
  out. Every gate described here is only as good as the fact that something runs it.
- Updater against this project's own releases (`createUpdaterArtifacts: false` today), its own
  signing key, and the **update notification** window role.
- NSIS/dmg/AppImage/deb bundles; unsigned until an identity exists.
- Windows and Linux installs verified; macOS unverified until a Mac exists.

---

## Cross-cutting, threaded through every chunk

1. **The gates stay green, always:** `cargo test --workspace`,
   `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`,
   `tsc --noEmit`, `vite build`, the domain scanner **and its 15 self-tests**, the parity gate
   **and its 10 self-tests**, `verify-binaries`, `generate-ts-types --check`,
   `generate-baseline --check`, and the new IPC-type check from A1.
2. **Flip `reachable` and `built` in the same commit as the screen.** An entry claiming a surface
   that does not render `data-surface` fails rule 7 — that gate exists because 64 entries once
   named screens nobody had written.
3. **Never report success without evidence.** The `CaptureExclusion`/`Permissions`/`Verification`
   pattern extends to every new capability: a state, and what verified it.
4. **Each chunk ends with a by-hand check on Windows**, in the shape that found five real defects
   on 2026-08-25. The bench check in `PHASES_0_6_COMPLETION_PLAN.md` is the template.

## Blocked, and what is not blocked by it

| Item | Needs | Not blocked |
| --- | --- | --- |
| Anything macOS-native | A Mac | Compiles in CI, ships marked unverified |
| macOS `whisper-server` | A macOS CI job under this project's releases; upstream ships only an xcframework | Windows and Linux install today, digest-pinned |
| 5 meeting clients, live | Keys for AssemblyAI, Deepgram, OpenAI Realtime, Corti, Tinfoil | Wire shapes vs fixtures; `meeting-stream:local`; detection; system audio |
| 4 transcription clients, live | Keys for Tinfoil, Mistral, xAI, Corti | The clients and their fixtures |
| Enterprise reasoning, live | AWS / Azure / GCP org credentials | SigV4 and body shaping vs published vectors |
| Calendars, real round trip | Google + Microsoft OAuth client IDs as PKCE public clients | Flow, storage, UI, stub server |
| AEC helper | cmake + MSVC/clang and a `contents: write` run | Meetings themselves — echo cancellation only matters on speakers |
| Signed installers | A code-signing identity | Unsigned builds |

## Verification

**Per chunk:** the full gate list above, plus new tests at the seam each chunk adds — a client
against recorded fixtures, a sidecar against its readiness poll, a screen against the surface
registry.

**End to end, by hand on Windows, per chunk:**

- **A:** store a key for OpenAI, run the connection test, see it pass; confirm the input never
  reads a stored secret back; confirm the parity gate's unreachable count drops by 15.
- **B:** dictate with cleanup on and see the tidied text; start llama-server and hold a chat;
  confirm the 5-minute idle stop.
- **C:** one live provider per client, if a key exists; otherwise the honest refusal.
- **D:** record system audio while a video plays, with the microphone muted, and confirm both
  streams; set a modifier-only hotkey and have it work.
- **E:** join a call in Zoom and see detection fire; run `meeting-stream:local` end to end.
- **F:** the stub PKCE round trip, then a real one if client IDs exist; confirm no token lands in
  SQLite.
- **G:** import an OpenWhispr database and confirm the source file is unmodified.
- **H:** drive the MCP server from a client over stdio.
- **J:** install the built bundle on a clean Windows machine and dictate.

## Risks

- **A1 is a real gate, not a formality.** If neither `ts-rs` nor `specta` expresses
  `rename_all_fields` on internally-tagged enums, the fallback is a custom emitter — a day, not an
  hour. Everything after A1 depends on the answer, so it goes first and its result is recorded.
- **Chunk D is the long one** and has no partial payoff: system audio half-written unblocks
  nothing. Expect it to take longer than B and C combined.
- **The registry/manifest credential mismatch (9 vs 15)** must be settled in A3 rather than
  papered over in the screen, or the "declared once" property quietly stops being true.
- **`id=8` in the bench-check history** — a connection refused to `127.0.0.1:8178` while the
  application believed a server was live — is not fully explained. The resume path makes it less
  likely; it has not been proven gone. Watch for it during B3, which adds a second long-lived
  loopback sidecar and would show the same symptom.
