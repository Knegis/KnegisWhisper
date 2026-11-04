# Porting Matrix

How each part of the two upstreams is handled. Groups follow the brief's A/B/C/D classification.
Facts and line counts come from `UPSTREAM_AUDIT.md`.

## Group A — reuse directly or with minimal changes

Already React 19 / TypeScript, and platform-independent.

| What | Upstream | Note |
| --- | --- | --- |
| Renderer components | `src/components` (190 files, 45.9 kLOC) | Minus the account / billing / workspace cluster (Group D). |
| Types | `src/types` (3.0 kLOC) | Minus cloud-sync ack manifests. |
| Model registry accessors | `src/models/ModelRegistry.ts` | Drop the import-time `readCachedTinfoilModels()` mutation at `:131-137`; it becomes an explicit async load. |
| Transcription route resolution | `src/helpers/transcriptionRoute.ts` (308) | The cleanest boundary in the codebase — see below. |
| Meeting provider tables | `meetingStreamingProviders.js`, `realtimeTokenProviders.js` | Drop the managed-mode branches. |
| Chat / reasoning / dictation routing | `chatRouting.js`, `reasoningRouting.js`, `dictationStreamingRouting.js` | With the fail-closed changes below. |
| Transcript formatting | `transcriptFormatter.js`, `transcriptText.js` | txt / md / json / srt / vtt. |
| Diarization clustering policy | `diarizationPolicy.js` (63) | Deliberately Electron-free upstream, for testability. **Not ported — the file is not in this repository, only this line count is.** `kw_meeting_core::diarization::assign` is a rule written from scratch and labelled invented, which is a different claim from ported. |
| Hotkey list parsing | `hotkeyList.js` | Comma-separated accelerators with a comma-key escape rule. |
| Smart spacing | `smartSpacing.js` | |
| Note search query builder | `noteSearch.js` | |
| Whisper VAD config | `whisperVadConfig.js` (41) + `constants/whisperVad.json` | Ported as `kw_audio_core::vad`, and unused until KNEG-404 (2026-10-06). Now applied by finding the speech first with whisper.cpp's `whisper-vad-speech-segments` and transcribing it in chunks (`kw_transcription_core::speech`), **not** by upstream's route of passing the flags to the server: the server's own VAD was measured putting row boundaries up to 30 s from their speech. The live `local` meeting view uses the same finder to skip windows with no speech in them (KNEG-410). |
| Tests for all of the above | part of `test/**` (303 files, 51.2 kLOC) | |

### Why `transcriptionRoute.ts` is the model boundary

Pure function, no store imports, erasable TS only, dual-loaded by renderer *and* main process,
and **routes never carry secrets** — `auth.keyRef` names a key *slot* that the executor
resolves. Port it as-is; the Rust side becomes the executor that turns `keyRef` into a
credential-store lookup. Two behaviours to preserve exactly:

- The `custom` branch **fails closed if the URL still equals `API_ENDPOINTS.TRANSCRIPTION_BASE`**
  (the untouched store default). Without that check it would ship the user's custom key and audio
  to OpenAI.
- xAI drops `language` unless it is in the 25-member `XAI_STT_LANGUAGES` set, so ITN via
  `format=true` stays valid.

### Deliberate divergence: fail closed

Upstream defaults ambiguous dispatch to OpenAI in three places. The brief forbids this ("do not
route an unknown model to OpenAI by default; fail closed when provider dispatch is ambiguous"),
so we diverge. Each change gets a test.

| Upstream | Behaviour | Ours |
| --- | --- | --- |
| `transcriptionRoute.ts:290-307` | Terminal fallback returns `provider:"openai"`, `keyRef:"openai"` for any unrecognised provider id. | Explicit error. |
| `constants.ts:59` | `buildApiUrl` second-order default: `normalizeBaseUrl(base) \|\| "https://api.openai.com/v1"`. | Explicit error. |
| `chatRouting.js` | Anything outside `CLOUD_CHAT_PROVIDERS` is classified `local` — a silent misroute, and exactly the bug the BYOK commit had to fix for Berget. | Explicit error. |

## Group B — approved sidecars

Five long-lived local servers, all loopback with a port drawn from a fixed range:

| Sidecar | Ports | Pin | Note |
| --- | --- | --- | --- |
| `whisper-server` | 8178–8199 | `ggml-org/whisper.cpp` tag `b4938` | **Done.** Digest-pinned in `config/sidecars.json`, measured rather than copied from a release note. Windows and Linux come from upstream; macOS publishes only an xcframework and is recorded as not-published. Vulkan cold start still needs a long timeout — shader compile. |
| `llama-server` | 8221–8240 | `b9763` | 5-minute idle auto-stop. |
| sherpa-onnx Parakeet WS | 6006–6029 | `1.13.4` | Separate offline and online binaries. |
| Qdrant | 6333–6350 | latest | Phase 8. |
| local API | 8200–8219 | in-repo | Ported to `axum`. |

Plus 17 one-shot or streaming children: `sherpa-onnx-diarize` (upstream's name; this project
uses `sherpa-onnx-offline-speaker-diarization` out of the sherpa archive it already installs, and
never built a separate sidecar), `ffmpeg`, `yt-dlp`,
`meeting-aec-helper`, `macos-audio-tap`, `windows-system-audio-helper`,
`linux-system-audio-helper`, `macos-globe-listener`, `windows-key-listener` (one process **per
watched key**), `linux-key-listener`, three `*-text-monitor`, three `*-fast-paste`, `nircmd`,
`macos-calendar-listener`, `macos-media-remote`, `macos-mic-listener`, `windows-mic-listener`,
`ps-list`.

Two upstream mechanisms to port rather than reinvent:

- **`binaryResolver.js`** — a 6-candidate path probe requiring `X_OK`, returning `null` for
  graceful fallback rather than throwing.
- **`sidecarReaper.js`** — PID files at `<userData>/sidecar-pids/<name>.pid`, and crucially it
  **verifies the PID's command line matches an expected binary fragment** before SIGTERM → 5 s
  grace → SIGKILL. Without that check, a stale PID file gets an unrelated process killed after a
  crash.

## Group C — port to Rust

| Electron | Rust | Upstream |
| --- | --- | --- |
| `ipcMain.handle` × 489 | `#[tauri::command]`, split across crates | `ipcHandlers.js` (10.7 kLOC) |
| 95 renderer events | typed Tauri events | `broadcastToWindows` |
| `better-sqlite3` | `sqlx` — FTS5, WAL, real migrations | `database.js` (5.6 kLOC) |
| `@napi-rs/keyring` + `safeStorage` + AES-GCM file blobs | `keyring-rs`, secrets **directly** in the OS store | `secretCrypto.js`, `environment.js` |
| `BrowserWindow` × 6 roles | `WebviewWindow` + native panel | `windowConfig.js`, `windowManager.js` |
| `globalShortcut` + 4 fallbacks | `tauri-plugin-global-shortcut` + `zbus` + native hooks | `hotkeyManager.js` (1.4 kLOC) |
| `@homebridge/dbus-native` | `zbus` | `gnomeShortcut.js`, `kdeShortcut.js`, `hyprlandShortcut.js` |
| `clipboard` + synthetic paste | `arboard` + per-OS injection | `clipboard.js` (2.3 kLOC) |
| `ps-list` | `sysinfo` | `meetingProcessDetector.js` |
| `onnxruntime-node` | `ort` | `onnxWorker.js` |
| `ws` | `tokio-tungstenite` | 5 streaming clients |
| `electron-updater` | **our own** releases, checked via `kw_network_policy::NetworkPolicy` rather than `tauri-plugin-updater` — see below | `updater.js` |
| `http.createServer` | `axum` on loopback | `cliBridge.js` (429) |
| `utilityProcess` + `MessageChannelMain` | `tokio` task or sidecar | `onnxWorkerClient.js` |
| `unzipper` / `unbzip2-stream` / `tar` | `zip`, `bzip2`, `tar` | download scripts |

### The updater does not use `tauri-plugin-updater`

An earlier pass at this table named that plugin as the replacement for
`electron-updater`, and `docs/COMPLETION_PLAN.md`'s phase 9 said the same. The
plugin ships its own HTTP client, and using it as intended would mean a
second piece of code in the workspace opening its own connections outside
`kw_network_policy::NetworkPolicy` — the crate whose module doc and the
brief's network-boundary section both say is *the* egress point, not one of
several. `apps/desktop/src-tauri/src/updater.rs` gets the same outcome
(checking this project's own GitHub releases, never at startup, never
without a person asking) by calling `NetworkPolicy::get_text` under
`RequestPurpose::OwnGithubReleasesUpdater` directly. `bundle.createUpdaterArtifacts`
in `tauri.conf.json` is still turned on, because producing signed artifacts is
the bundler's job and has nothing to do with which client later checks for
them; `plugins.updater.pubkey` is still read from the location that plugin
would use, so a keypair generated the standard way (`docs/SECURITY.md`) needs
no translation if this project adopts the plugin later.

### Window roles

**4 of 6 built.** Dictation panel and Control panel existed before this run; Meeting notification
and Update notification were built during it (chunks E3 and J respectively) and are marked below.
Agent overlay and Transcription preview remain — the former is phase 10 scope
([COMPLETION_PLAN.md](COMPLETION_PLAN.md)), the latter has no task in this plan.

All `frame: false`.

| Role | Size | Key flags |
| --- | --- | --- |
| Dictation panel | 96×96 | `transparent`, `alwaysOnTop`, `skipTaskbar`, **`focusable: false`**, per-DE `type` |
| Control panel | 1200×800 | opaque, normal, resizable |
| Agent overlay | 420×300 | `transparent`, **`focusable: true`** |
| Transcription preview | 460×132 | `focusable: false` |
| Meeting notification | 392×102 compact for offers and the transcribing strip; 440×560 recording panel, minimum 392×280 | **content-protected** — "visible to user but out of screen shares". **Built.** `apps/desktop/src-tauri/src/meeting_window.rs` reuses `overlay::apply_treatments` for capture-exclusion read-back. Non-activating while idle and offering; activatable from meeting start through recording and the transcribing strip, revoked only after the window is hidden on return to idle. The recording panel hosts the note editor, so its Rust builder disables Tauri's drag-and-drop handler. Movable and resizable; saved monitor-relative placement applies to the panel only and can be reset on Settings > Meetings. |
| Update notification | 392×104 | **content-protected** — same rule as the meeting notification. **Built.** `apps/desktop/src-tauri/src/update_window.rs`, the same builder and `overlay::apply_treatments` unchanged, so it reports the same read-back evidence. Shown only once `updater.rs`'s explicit, user-triggered check finds a newer release; never on its own |

Per-platform z-order policy from `setupAlwaysOnTop()`, worth copying exactly: macOS
`("floating", 1)` plus `setVisibleOnAllWorkspaces({visibleOnFullScreen, skipTransformProcessType})`;
Windows `"pop-up-menu"`; GNOME/Wayland `"floating"`; other Linux `"screen-saver"`.

### Text insertion — the most forked subsystem

Always clipboard + synthetic paste keystroke, never direct injection.

| Platform | Tiers |
| --- | --- |
| macOS | `macos-fast-paste` (CGEvent ⌘V) → `osascript`. Gated on Accessibility; without it, copy only. |
| Windows | `windows-fast-paste.exe` (SendInput, with `AttachThreadInput`/`SetForegroundWindow` to defeat the foreground lock) → `nircmd` → PowerShell `SendKeys`. |
| Linux | `linux-fast-paste` (XTEST **and** an xdg-desktop-portal RemoteDesktop D-Bus mode with a persisted restore token) → `ydotool` → `wtype` → `xdotool key` → `xdotool type`. Plus PRIMARY selection and a 30-entry terminal-class list needing Ctrl+Shift+V. |

Foreground restore differs three ways: macOS by PID with confirmation polling, Windows by HWND,
Linux by re-detecting the window.

**Copy the clipboard-restore logic exactly.** It re-reads the clipboard and only restores if the
content still equals what it wrote, so a user copy during the paste window is not clobbered.

### Global shortcuts — five backends

`Control+Super` is the *default* hotkey, and modifier-only combinations, right-side modifiers,
Globe/`Fn`, mouse buttons 4/5, and push-to-talk are all invisible to the cross-platform API.
Hence: the Tauri plugin, GNOME gsettings + D-Bus, KDE KGlobalAccel, Hyprland `hyprctl`, and
per-key native hook processes.

## Group D — remove or replace

**Vendor services.** All 18 are enumerated in `config/integration-manifest.json` under
`vendor:*`. Summary: OpenWhispr hosted auth / API / MCP / transcription / reasoning / sync /
team spaces / usage / billing / referrals / realtime-token broker / cloud model tiers; Pluely
backend / licensing / updater; PostHog; machine-uid; and the dead `telemetryEnabled` switch.

**Dead-but-compiled paid surface** — delete outright rather than gate:
`src/lib/{usageStore,subscriptionFlag,billingPortalError,upsell,teamSpacesCapability,spacePermissions,workspaceSelection,authAccountScope,auth}.ts`;
`src/hooks/{useUsage,useBillingPortal,useAuth,useTeamSpacesCapability}.ts`;
`src/services/{WorkspacesService,membershipActions,accountSpaceValidation,syncPassPolicy,SyncService}.ts`;
`src/components/settings/Workspace*.tsx`;
`src/components/{UpgradePrompt,InviteTeammateDialog,AcceptInvitationModal,AuthenticationStep,EmailVerificationStep}.tsx`;
`src/services/ai/inferenceProviders/openwhispr.ts`; and the `ipcHandlers.js` blocks at ≈4955-5010
(cookie/auth plumbing), ≈6089-6120 (`postServerToken`), ≈8160-8300 (usage and Stripe).

**Orphaned i18n keys** still present in all 10 locale files with no code reading them:
`integrations.plan.pro`, `integrations.mcp.*`, `integrations.api.*`, `integrations.cli.cloud.*`,
`integrations.cli.viewPlans`, `integrations.sections.{api,mcp}`, `manageStripe`.

**Dead imports** the BYOK commit left in `IntegrationsView.tsx`: `Code2`, `Dialog`,
`DialogContent`, `DialogDescription`, `DialogHeader`, `DialogTitle`.

**Build-time vendor coupling** (see `UPSTREAM_AUDIT.md`): re-source whisper.cpp from
`ggml-org/whisper.cpp` — **done**, digest-pinned in `config/sidecars.json`; pin and checksum
`nircmd` or drop it — **dropped**, it was only the third-tier Windows paste fallback and
`SendInput` works. The five Windows helpers are settled below, and the AEC helper is the one
remaining binary this project intends to build for itself.

### The five Windows helpers are reimplemented, not inherited

Upstream fetches each of these from its own GitHub releases with `fetchLatestRelease` and a tag
*prefix* — unpinned, unhashed, whatever the newest matching tag happens to be — and then executes
it. **This tree has never fetched any of them**: `config/sidecars.json` holds one entry, whisper,
from `ggml-org`. So what follows closes an intent, not a live dependency. Earlier drafts of this
document and the engineering log said these "still come from OpenWhispr release assets", which
described upstream's arrangement rather than ours.

The decision is to write Rust rather than publish six binaries. That is what `ARCHITECTURE.md`
already claims — "Rust owns … audio capture, system audio, clipboard and text insertion, process
detection" — and one of the six turned out to be done already.

| Upstream helper | LOC | Replacement | Status |
| --- | --- | --- | --- |
| `windows-fast-paste.c` | 342 | `platform::window::send_ctrl_v`, native `SendInput` | **done** |
| `windows-system-audio-helper.c` | 634 | Rust `wasapi` loopback behind the system-audio trait | chunk 3 |
| `windows-key-listener.c` | 372 | Rust `SetWindowsHookExW` / `WH_KEYBOARD_LL`. This is the gap `platform/src/hotkey.rs:149` names, and what modifier-only, right-side-modifier, Globe/`Fn`, mouse-button and push-to-talk hotkeys all need | with chunk 3 |
| `windows-text-monitor.c` | 251 | Rust UIAutomation focused-element read | **declined** (KNEG-63) — no consumer and no defect asking for it; a continuous read of the focused element needs its own disclosure |
| `windows-mic-listener.c` | 1425 | Rust `IAudioSessionManager2` in `platform::microphone` | **done** 2026-09-01 (KNEG-94) |

Upstream runs **one key-listener process per watched key**. An in-process hook replaces all of
them, so this is fewer moving parts rather than a like-for-like port.

### The AEC helper is the exception, and it is a build pipeline

`meeting-aec-helper` wraps WebRTC's audio processing module, which has no Rust equivalent that
is not itself a binding to the same C++. It stays a `Transport::Stdio` sidecar — a child driven
over stdin and stdout, which is the shape `crates/sidecar` gained a type for.

Its cost is not the six files of helper source. `native/meeting-aec-helper/CMakeLists.txt`
requires a generated source manifest, and upstream's `scripts/lib/meeting-aec-build.js` (446
lines) produces it by downloading the whole webrtc-apm module plus Abseil and running a Python
generator over GN labels, with per-architecture AVX2 handling, across five targets.

Two facts were measured before designing the pin, and they decide its shape:

| Input | Pinned by | Why |
| --- | --- | --- |
| `abseil-cpp` @ `9ac7062b`, GitHub archive | `sha256:fd5062972ac6c5575bc8fb6fb1cd6e7c563730be96be4c17b8dcca74a9c5892d`, 2 268 399 bytes | Byte-stable: two downloads produced identical archives. |
| `webrtc-apm` @ `08f235eb`, googlesource `+archive` | **content digest** `b2db82b1a06a28d965eeddc3928a43c5067cad820866907be29faf4e26f5c37b` over 735 files | **Not** byte-stable. Two downloads seconds apart differed in both length and hash — googlesource generates the tarball per request. The extracted tree is identical, so the pin is a sorted `path + file digest` roll-up rather than an archive hash. |

An archive digest on that second URL would fail on every build, which is worse than no pin: it
would be removed by whoever hit it first. Upstream pins both by commit only and verifies neither.
Both pins above are the durable part of this work and are kept as measured — re-downloading
`webrtc-apm` to re-derive its digest would cost a day, for no different an answer.

**Declined for now, not unfinished.** What remains beyond the two pins is vendoring six source
files, porting the 446-line generator, and a five-target CI workflow granted `contents: write` to
publish the built binaries as this project's own release assets. All three need a C++ toolchain —
cmake, ninja, clang — and none is installed on this development machine, which is also why every
decompressor in this workspace (`zip`, `bzip2`, `tar`) is pure Rust rather than a C binding. See
`config/integration-manifest.json`'s `platform:aec` entry for the full accounting.

**Echo cancellation is a quality feature, not a gate.** Without it, a meeting recorded on
speakers picks up the far-end voice through the microphone and transcribes it twice; on headphones
there is nothing to cancel. It gates nothing: what actually blocks a meeting from being recorded at
all is system audio capture, and that is Rust work that is already done (`platform:system-audio`).

**Identity:** appId `com.gizmolabs.openwhispr`, protocol `openwhispr://`, signing identity
`Gizmo Labs Inc. (T832773L2J)`, Azure Trusted Signing endpoint, and Pluely's
`com.srikanthnani.pluely` — all replaced by `com.knegis.knegiswhisper`.

## Not a port: new implementations

| Deliverable | Why |
| --- | --- |
| CLI (`tools/knegiswhisper`) | Upstream CLI is a separate npm package, absent from the repo. Only the documented noun-verb surface and exit codes 0–4 survive as a spec. |
| MCP server (`tools/knegiswhisper-mcp`) | Upstream MCP is a hosted service. No local or stdio implementation exists anywhere in the checkout. |
| `MeetingProviderCapabilities` | No formal capability object exists upstream; capability is implied by scattered flags and wire-shape differences. |
| Windows non-activating overlay | Pluely's attempt is commented out; there is no reference implementation to copy. |
| Capture-exclusion diagnostics | Pluely assumes success and reports nothing. |

## Nothing copied from Pluely

KnegisWhisper is MIT; Pluely is GPL-3.0. Treat any verbatim Pluely code in a pull request as a
defect. `UPSTREAM_AUDIT.md` records its techniques as a specification to implement against, and
the crates it selects (`cpal`, `wasapi`, `cidre`, `libpulse-binding`, `hound`, `ringbuf`,
`tauri-nspanel`) are independently available.

## Deliberate behavioural divergences

Each of these is a place where following upstream faithfully would have carried a defect across.
Every one has a test.

| # | Upstream | Here | Why |
| --- | --- | --- | --- |
| 1 | `transcriptionRoute.ts:290-307` terminal fallback returns `provider:"openai"`, `keyRef:"openai"` for any unrecognised id | `RouteError::UnknownProvider` | A typo in a provider id sent the user's audio to a provider they never chose, using a credential configured for something else. Note the distinction kept: *absent* configuration still defaults to OpenAI, because the user chose nothing; an *unrecognised* id fails closed. |
| 2 | `constants.ts:59` `buildApiUrl` falls back to `https://api.openai.com/v1` | `UrlError` | Same class of bug, second instance: an empty or unparseable configured base URL silently became OpenAI. |
| 3 | `chatRouting.js` classifies anything outside `CLOUD_CHAT_PROVIDERS` as `local` | `ChatRouteError::UnknownProvider`, with the cloud set **derived from the registry** | A "local" route goes to llama.cpp on loopback, so a cloud provider missing from the hardcoded set produced a connection failure that blamed the local model. This is the bug Berget hit. Deriving the set fixes the class, not the instance. |
| 4 | `resolveByokModel` accepts an empty custom model id via `allowCustomModelId: true` | Falls back to a concrete default | An empty model id produces a request with no model. |
| 5 | `proxied` transport | `Transport::ProviderClient` | Nothing is proxied through a vendor; it means the provider needs its own client. Given the brief forbids hidden remote fallbacks, a transport that *reads* as a proxy was worth renaming. |
| 6 | 25 MiB constant with Berget special-cased to `null` inside routing code | `maxUploadBytes` per provider in the registry | Same behaviour, no special case, and one place to change it. |
| 7 | `google_calendar_tokens.refresh_token` as a plaintext SQLite column | `token_ref` naming an OS credential-store slot | A security defect that would have been ported faithfully by accident. `assert_no_credential_columns` fails if any credential-shaped column reappears. |
| 8 | ~70 `ALTER TABLE` blocks replayed on every boot; `pragma user_version` used only as a data-seed sentinel | One flattened `0001_initial.sql` and `sqlx::migrate!` | The on-disk schema version was unknowable from the database. |
| 9 | No meeting capability object | `MeetingProviderCapabilities` | Calling code could not ask whether a provider honours keyterms. Two providers accept them and ignore them, and one caps at 100 and truncates silently. |
| 10 | Errors returned inside the route union | `Result` | Upstream has no choice in JavaScript; in Rust a `Result` makes ignoring the error impossible. |

### Preserved exactly, because each prevents a credential going somewhere it should not

- The `custom` branch fails closed when its URL still equals the default OpenAI base — Custom
  selected but never configured. Passing it through ships the custom credential and the audio to
  OpenAI.
- xAI drops a language outside its 25-language set, so inverse text normalisation via
  `format=true` stays valid.
- Tinfoil hosts are refused as plain custom endpoints, because Tinfoil needs its attested client.
- The FTS5 external-content table and its three triggers.
- The ASCII-safe cache path fallback: the native whisper and sherpa binaries fail on paths
  containing spaces or non-ASCII characters, which a Windows account named `Ann Marie` is enough
  to trigger.
- The clipboard-restore rule: restore the user's clipboard only if it still holds what we pasted.
- Silero VAD off by default for dictation. On a pause-heavy dictation, VAD can strip the speech
  and leave whisper decoding near-silence seeded with the dictionary prompt — returning the
  dictionary words as the transcript.
- The speech gate fails **open** when it cannot measure. Discarding a dictation the user spoke is
  worse than transcribing silence.
