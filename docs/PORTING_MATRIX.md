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
| Diarization clustering policy | `diarizationPolicy.js` (63) | Deliberately Electron-free upstream, for testability. |
| Hotkey list parsing | `hotkeyList.js` | Comma-separated accelerators with a comma-key escape rule. |
| Smart spacing | `smartSpacing.js` | |
| Note search query builder | `noteSearch.js` | |
| Whisper VAD config | `whisperVadConfig.js` (41) + `constants/whisperVad.json` | |
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
| `whisper-server` | 8178–8199 | tag `0.0.8` | **Re-source from `ggml-org/whisper.cpp`** (Group D). Vulkan cold start needs a 120 s timeout — shader compile. |
| `llama-server` | 8221–8240 | `b9763` | 5-minute idle auto-stop. |
| sherpa-onnx Parakeet WS | 6006–6029 | `1.13.4` | Separate offline and online binaries. |
| Qdrant | 6333–6350 | latest | Phase 8. |
| local API | 8200–8219 | in-repo | Ported to `axum`. |

Plus 17 one-shot or streaming children: `sherpa-onnx-diarize`, `ffmpeg`, `yt-dlp`,
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
| `electron-updater` | `tauri-plugin-updater` → **our own** releases | `updater.js` |
| `http.createServer` | `axum` on loopback | `cliBridge.js` (429) |
| `utilityProcess` + `MessageChannelMain` | `tokio` task or sidecar | `onnxWorkerClient.js` |
| `unzipper` / `unbzip2-stream` / `tar` | `zip`, `bzip2`, `tar` | download scripts |

### Window roles

All `frame: false`.

| Role | Size | Key flags |
| --- | --- | --- |
| Dictation panel | 96×96 | `transparent`, `alwaysOnTop`, `skipTaskbar`, **`focusable: false`**, per-DE `type` |
| Control panel | 1200×800 | opaque, normal, resizable |
| Agent overlay | 420×300 | `transparent`, **`focusable: true`** |
| Transcription preview | 460×132 | `focusable: false` |
| Meeting notification | 392×92 | **content-protected** — "visible to user but out of screen shares" |
| Update notification | 392×92 | as above |

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
`ggml-org/whisper.cpp`; rebuild the AEC helper and five Windows helpers under this project's own
releases (the C/Swift source is in-repo and MIT, so this is a CI job); pin and checksum `nircmd`
or drop it, since it is only the third-tier Windows paste fallback.

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
