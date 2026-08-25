# Upstream Audit

Phase 0 record of the two upstream projects KnegisWhisper derives from. This is a factual
audit, not a plan; see `PORTING_MATRIX.md` for what happens to each piece.

## Commits audited

| Upstream | Commit | Role |
| --- | --- | --- |
| OpenWhispr | `c4000db9802d696341f1d239c1d81dc6d1ee233e` | Product functionality. Parity baseline and porting source. |
| — its fork point | `4d1a6eafb6d940a3496276bb9e423fe610406f44` | Pristine upstream `main` @ 2026-08-20. Available as a cross-check. |
| Pluely | `4fb23ac348ad087ef22920272012030398b6b155` (v0.1.9) | Native shell architecture. **Reference only — no code copied.** |

### Why not the brief's `858e824`

The brief pinned `858e82408789f40a787d3a255a96df088d065cad` as the OpenWhispr baseline. That
commit **does not exist in the available clone** — not in any local, `origin`, or `upstream` ref.
The clone's branch `feat/byok-only-dyve` forks from upstream `main` at `4d1a6eaf` (2026-08-20),
so `858e824` (2026-08-25) is roughly three commits ahead of the fork point:

| Commit | Subject | Touches providers? |
| --- | --- | --- |
| `193b02b` | chore(release): prepare 1.9.0 | no |
| `996fe79` | Enforced request body byte limit validation in CLI | no |
| `858e824` | fix(utils): handle nullish and invalid inputs safely in date formatting (2 files, +40/−2) | no |

None affects the provider set or the model registry, so parity measured against `c4000db9` is
equivalent for every integration in the manifest. This is recorded in the baseline as
`provenance.upstreamForkPoint`.

### The BYOK fork

`c4000db9` ("feat: BYOK-only build with Berget AI as a first-class provider") is a single
commit of 66 files, **+937 / −529**, whose parent is the pristine upstream tree. The working
tree is clean — there are no uncommitted modifications — so `git show c4000db9` is the exact,
complete diff of everything stripped or gated. It also adds **Berget AI** as a first-class
transcription and reasoning provider.

> **Stale artifacts.** `src/dist/` holds a renderer build from six minutes *before* the BYOK
> commit, so it still contains pre-BYOK bundles (`upsell-*.js`, `useUsage-*.js`,
> `workspaceStore-*.js`). `dist/` holds a packaged `OpenWhispr 1.8.3.exe`. Both produce false
> positives in any grep of "live" code and must be excluded from audits.

## OpenWhispr — scale

Electron 41, React 19, Vite 8, Node ≥ 24, `better-sqlite3`, whisper.cpp, sherpa-onnx, MIT.

| Area | Files | LOC |
| --- | --- | --- |
| `src/**` (ts/tsx/js/jsx) | 658 | **143,722** |
| ├─ `src/helpers` — the actual backend | 184 | **61,599** |
| ├─ `src/components` — renderer UI | 190 | 45,868 |
| └─ `src/stores` + `services` + `hooks` | 118 | 22,398 |
| `main.js` | 1 | 1,896 |
| `preload.js` | 1 | 1,211 |
| `test/**` | 303 | 51,202 |
| Native C | 10 | 5,797 |
| Native Swift | 7 | 1,674 |
| Native ObjC (vendored mediaremote) | ~11 | ~1,400 |
| Native C++ (WebRTC AEC) | 6 | ~440 |

`src/helpers/ipcHandlers.js` alone is **10,678 lines** and holds **478 of the 489 IPC channels**.
Other large modules: `database.js` 5,597, `audioManager.js` 4,660 (renderer), `clipboard.js`
2,325, `windowManager.js` 1,627, `urlAudioDownloader.js` 1,561, `hotkeyManager.js` 1,356,
`whisperServer.js` 1,168.

**Surface to port: 489 IPC channels + 95 main→renderer events.** About 180 are thin `db-*` CRUD;
roughly 120 are genuinely stateful (streaming ASR sessions, the meeting pipeline, model
downloads).

## OpenWhispr — provider registry

`src/models/modelRegistryData.json`: 8 top-level keys, **23 providers, 96 models** across the
four provider arrays, 114 entries overall. Captured verbatim in
`config/upstream-integration-baseline.json`.

| Group | Providers | IDs |
| --- | --- | --- |
| Transcription | 7 | `openai` `groq` `berget` `xai` `mistral` `corti` `tinfoil` |
| Cloud reasoning | 7 | `openai` `anthropic` `gemini` `groq` `tinfoil` `berget` `corti` |
| Enterprise | 3 | `bedrock` `azure` (0 models — deployment-driven) `vertex` |
| Local GGUF | 6 | `qwen` `gemma` `liquidai` `mistral` `llama` `openai-oss` |

Local ASR: 6 whisper GGML, 4 sherpa (2 Parakeet offline, 2 Nemotron `runtime: "online"`),
2 diarization.

Three corrections to the brief's inventory:

- **Four providers the brief omits are live:** Berget AI, xAI, Mistral, Liquid AI.
- **`promptTemplate` does not exist.** There is no `promptTemplate`, `chatTemplate`,
  `stopTokens`, or `bosToken` field anywhere in the registry; templating is delegated to
  llama.cpp's GGUF metadata. The brief asks to preserve prompt templates — there are none.
- **Speculative decoding covers exactly 4 models**, all Gemma QAT, via
  `draftHfRepo` / `draftFileName` / `draftSizeBytes`.

`openwhisprCloudModels` has **zero readers** across `src/`, `test/`, and `scripts/` — a hosted
tiering vestige, excluded from the baseline as `removed-vendor-service`.

## OpenWhispr — vendor coupling

Better than the brief assumed, with two exceptions.

- **`openwhispr.com` appears in zero shipping source files.** Only in CI workflows, agent-skill
  docs, `docs/network-allowlist.md`, README/CHANGELOG, `nix/package.nix`, a `support@` address,
  and test-fixture emails.
- **No telemetry at all.** Zero real matches for posthog / sentry / amplitude / mixpanel /
  datadog, and no analytics dependency. A `telemetryEnabled` setting exists, defaults to
  `false`, and **nothing reads it** — a dead switch with no collector.
- **No license or activation machinery.**
- **AssemblyAI already mints tokens directly** from
  `streaming.assemblyai.com/v3/token?expires_in_seconds=60`; Deepgram BYOK sends the raw key;
  Corti always does its own OAuth2. The brief's acceptance criterion about removing a token
  broker is already satisfied in BYOK mode — the work is deletion, not reimplementation.
- **Calendar OAuth already goes direct** to Google and Microsoft via PKCE on an ephemeral
  loopback listener. The openwhispr.com "desktop callback" is only a cosmetic post-auth browser
  redirect, and is empty in this build.

**Exception 1 — a live re-injection path.** `.github/workflows/build-and-notarize.yml` and
`release.yml` still default `VITE_AUTH_URL` to `https://auth.openwhispr.com`. The renderer
neutralises it, but `getAuthUrl()` (`ipcHandlers.js:4977`) has **no guard**, so a CI-built binary
carries the vendor host in the main process. Not currently reachable, but it is why the domain
scanner must cover workflows and built assets.

**Exception 2 — build-time dependencies on the vendor's GitHub org**, which no host blocklist
would catch:

| Artifact | Source | Pin |
| --- | --- | --- |
| whisper.cpp | **`OpenWhispr/whisper.cpp`** | tag `0.0.8` |
| AEC helper + 5 Windows helpers | **`OpenWhispr/openwhispr` releases** | `meeting-aec-helper-v*`, `windows-*-v*` |
| llama.cpp | `ggml-org/llama.cpp` | `b9763` |
| sherpa-onnx | `k2-fsa/sherpa-onnx` | `1.13.4` |
| yt-dlp | upstream | `2026.07.04` |
| nircmd | `nirsoft.net` | **no version, no hash** |

The whisper.cpp pin exists deliberately so an upstream bump cannot change transcription output
between releases. That reasoning is sound and should be kept — only the org must change.

**The paid surface was gated, not removed.** A single flag,
`BYOK_ONLY = (env.VITE_BYOK_ONLY as string) !== "false"`, plus 15 call sites. Note the polarity:
BYOK-only is the *default*. The entire account / billing / usage / workspace subsystem still
compiles and is merely starved of a base URL, plus short-circuited by `settingsStore.ts:195`
coercing any persisted `"openwhispr"` mode back to `"providers"`. It can be deleted outright.

## OpenWhispr — database

`better-sqlite3`, `<userData>/transcriptions.db`, WAL. **18 tables plus one FTS5 virtual table.**

**There is no versioned migration runner.** The schema is applied idempotently on every boot:
`CREATE TABLE IF NOT EXISTS` per table, then **≈70 blocks** of
`try { ALTER TABLE … ADD COLUMN … } catch { if (!/duplicate column/) throw }` spanning
`database.js:123`–`1014`, plus `IF NOT EXISTS` indexes and triggers, one real table rebuild
(`folders_new` → copy → rename), and one `pragma index_list` introspection.

`pragma user_version` is used exactly once, and **not as a schema version**: it gates a one-time
data seed. Current value is `1`. The on-disk schema version is therefore unknowable from the
database itself — see `../docs/ARCHITECTURE.md` and the migration plan for how we handle that.

**Refresh tokens are stored in plaintext SQLite columns** —
`google_calendar_tokens.refresh_token` / `access_token` and
`microsoft_calendar_tokens.refresh_token`. They are not in the keychain and not encrypted.
This is a defect we fix rather than port.

## OpenWhispr — secrets

27 encrypted-at-rest keys: 15 manifest-generated BYOK slots (`src/config/secretKeys.js`) plus 12
bespoke. Stored as AES-256-GCM blobs at `userData/secure-keys/<ENV_VAR>.enc` with layout
`iv(12)|tag(16)|ciphertext`; the master key lives in the OS keyring under service `"OpenWhispr"`
/ account `"secrets-master-key"`. Migration is sentinel-gated with a round-trip verify before
plaintext is stripped — careful work worth learning from, even though we store secrets in the
credential store directly.

## OpenWhispr — what is *not* in the repository

Two of the brief's deliverables are treated as ports but do not exist here:

- **The CLI** is a separate npm package, `@openwhispr/cli`. Only its command surface is
  documented, in `agent-skills/openwhispr-cli/SKILL.md`.
- **The MCP server** is a hosted Streamable-HTTP service at `mcp.openwhispr.com/mcp`. There is
  no local or stdio MCP implementation anywhere in the checkout.

The **local API is** in-repo and is a good model: `cliBridge.js` (429 lines), HTTP on
`127.0.0.1`, port from 8200–8219, discovery file `~/.openwhispr/cli-bridge.json` at mode `0600`,
`randomBytes(32)` bearer compared with `timingSafeEqual`, non-loopback `remoteAddress` rejected,
1 MB body cap, 15 routes under `/v1/`.

## Pluely — audit

Tauri 2, React 19, Vite 7, **GPL-3.0**, bundle `com.srikanthnani.pluely`, v0.1.9.

The license is why nothing is copied: KnegisWhisper is MIT, and lifting GPL-3.0 Rust would make
the whole application a copyleft derivative. Pluely is used as a behavioural specification.

**Worth learning from:**

| Area | Approach |
| --- | --- |
| macOS non-activating overlay | `tauri-nspanel`: `NSFloatWindowLevel` (4), `NSWindowStyleMaskNonActivatingPanel` (`1<<7`), collection behaviour `FullScreenAuxiliary \| CanJoinAllSpaces` |
| Dock-icon hiding | `ActivationPolicy::Regular` ↔ `Accessory` |
| Windows system audio | `wasapi` 0.19 — default `Direction::Render` device initialized for `Direction::Capture`, mono f32, rate from `get_mixformat()` |
| macOS system audio | `cidre` CoreAudio process taps — `TapDesc::with_mono_global_tap_excluding_processes` → `create_process_tap` → `AggregateDevice` |
| Linux system audio | `libpulse-simple-binding`, `Simple::new(Direction::Record, "<sink>.monitor")`, fallback `@DEFAULT_MONITOR@` |
| Audio stream shape | `SpeakerStream: futures::Stream<Item = f32>` |

**Where Pluely under-delivers against its own README** — these are gaps we must fill, not
patterns to copy:

1. **Windows non-activating does not exist.** `src-tauri/src/window.rs` has the attempt
   commented out (`// let _ = window.set_focusable(false);`). No `WS_EX_NOACTIVATE`, no
   `WM_MOUSEACTIVATE`.
2. **Click-through does not exist anywhere.** No `set_ignore_cursor_events` in any file. The
   "invisible cursor" feature is CSS `cursor: none !important`, which hides only Pluely's own
   cursor rendering.
3. **Capture exclusion is unverified.** The whole mechanism is Tauri's `contentProtected` flag.
   Nothing queries the resulting display affinity or `sharingType`; failure is invisible; it is
   a no-op on Linux. The screen-capture selection overlays in `capture.rs` are not protected at
   all.
4. **VAD is a hand-rolled RMS+peak gate**, not Silero.
5. **Security posture is inverted from ours:** `"csp": null` and a capability granting
   `http:default` over `http://**` and `https://**` — unrestricted webview network access.
6. `app.get_webview_panel("main").unwrap()` in the macOS show path is a crash, not a fallback.

**README claims not to inherit:** "absolute stealth", "Undetectable Everywhere", "completely
hidden from your audience", "Zero Detection", and a recommendation to use it "during job
interviews … without detection". It attributes screenshot resistance to *translucency*, which
provides none. The only admitted platform limitation concerns cursor hiding on Linux — there is
no Linux caveat on the capture-exclusion claims, even though the mechanism does nothing there.

`PLATFORM_SUPPORT.md` is the deliberate counterweight: capability is reported with evidence,
per platform, and never claimed.

**To remove entirely:** `activate.rs` (licensing), `api.rs` (all AI routed through Pluely's own
backend via a build-time `APP_ENDPOINT`), `tauri-plugin-posthog`, `tauri-plugin-machine-uid`,
and the `https://pluely.com/api/update` updater.

**Risks inherited if we adopt its crate choices:** `cidre` process taps require **macOS 14.4+**
while Pluely declares `minimumSystemVersion: "10.13"` (a hard runtime failure, not degradation);
`tauri-nspanel` is an **unpinned git branch** dependency; the Linux path is PulseAudio-only and
hardcodes 44100 Hz instead of querying the device.
