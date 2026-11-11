# Architecture

KnegisWhisper is a Tauri 2 desktop application: a Rust backend that owns everything stateful or
privileged, and a React 19 frontend that renders. There is no Electron runtime anywhere in the
architecture.

## Layout

```text
apps/desktop/
  src/                     React 19 + Vite + TypeScript
  src-tauri/               thin shell: window creation, command registration, capabilities
crates/
  app-core/                lifecycle, settings, orchestration, calendar
  audio-core/              cpal mic, per-OS system audio, VAD, levels, mixing, WAV
  transcription-core/      route resolution + executors (local, http-batch, streaming)
  reasoning-core/          BYOK, enterprise, and local llama.cpp dispatch
  meeting-core/            MeetingTranscriptionProvider trait + implementations
  notes-core/              notes, folders, transcripts, dictionary, export, import
  storage/                 sqlx, migrations, the OpenWhispr importer
  provider-registry/       data-driven registry; generates Rust and TS types
  provider-probe/          live connection tests; the only thing that may call a provider "connected"
  platform/                tray, autostart, shortcuts, text insertion, clipboard, process detection
  overlay/                 privacy overlay + capture-exclusion diagnostics
  local-api/               axum, loopback only, bearer auth
  security/                OS credential store, redaction, path and URL validation
  network-policy/          the only egress point; purpose-tagged requests
tools/
  knegiswhisper/           CLI
  knegiswhisper-mcp/       stdio MCP server
  integration-audit/       baseline generator + parity gate
config/                    the machine-readable contracts
docs/                      this directory
```

`provider-registry` and `integration-registry` are one crate rather than two: they would share
the same generated types and the same manifest, so splitting them buys nothing and risks a
dependency cycle.

## Division of responsibility

**Rust owns** application lifecycle, window management, tray, global shortcuts, audio capture,
system audio, clipboard and text insertion, database access, credential access, process
detection, the local API, sidecar lifecycle, provider dispatch, network policy, file management,
and import orchestration.

**React owns** rendering, local UI state, forms, settings, transcript display, meeting controls,
notes, model selection, and provider configuration.

They communicate only through typed Tauri commands and events. The frontend has **no**
unrestricted shell, filesystem, HTTP, process-spawning, database, or credential access — and no
`http:` capability at all.

## Single source of truth for providers

`config/provider-registry.json` is derived once from the frozen upstream baseline and is
thereafter authoritative. **23 providers, 96 models, 21 credential slots, declared once.**

The slot count was 9 until the Providers screen was written, because the set was derived from the
provider lists. Six slots belong to no provider — `openrouter`, which upstream pinned no model
list for, and five per-feature custom endpoints — so `config/integration-manifest.json` declared
fifteen while the registry knew nine. A screen offering all fifteen would have had to name those
six itself, which is the duplication this section exists to rule out. They now come from the
baseline's `secretKeys`, and `every_slot_upstream_had_is_declared_here` asserts the two agree.

Both languages read that one file:

- **Rust** — `crates/provider-registry` embeds it with `include_str!` and parses it into typed
  structs, so a shape mismatch is a test failure rather than a runtime surprise.
- **TypeScript** — `tools/integration-audit/generate-ts-types.mjs` emits
  `apps/desktop/src/generated/providers.ts`: literal-union id types, interfaces, and the data.
  An invalid provider id is a compile error.

Three CI checks keep them from drifting: the registry must be current against the baseline, the
generated TypeScript must be current against the registry, and the parity gate must still pass.

This matters because upstream duplicates its provider list across four places —
`secretKeys.js`, the `BYOK_KEY_BRIDGES` array inlined in `preload.js` (sandboxing prevents it
requiring the module), `PROVIDER_REGISTRY`, and `CLOUD_CHAT_PROVIDERS` — and keeps them in sync
with a test. That was not merely untidy: Berget fell through `CLOUD_CHAT_PROVIDERS` and got
silently classified as a *local* model, which is the bug the BYOK commit had to fix.

### One normalisation applied on the way through

Upstream expresses the transcription upload cap as a magic `sizeCapBytes: null` special case for
Berget inside routing code, while OpenAI and Groq are capped at 25 MiB elsewhere. Every provider
now declares an explicit `maxUploadBytes`, with `null` meaning genuinely uncapped. Same
behaviour, no special case, and a test asserts each provider's value.

## Provider connection tests

`provider-probe` is a port of upstream's `scripts/verify-byok.mjs`, and it is what
makes "never show a provider as connected without a real connection test" a checkable
property rather than a promise. Three probes per provider — catalogue, chat, voice —
and four rules worth restating:

- **A 2xx is not a pass.** `200 OK` with an empty completion, a non-JSON body, or a
  missing `text` field is a failure. This is precisely how an integration comes to be
  displayed as working while doing nothing.
- **A tone cannot verify recognition.** A 440 Hz sine transcribes to `""` everywhere, so
  a tone-based pass proves endpoint, credential, and model id and nothing more. The
  report says which audio it used; the weaker claim is stated, not implied.
- **A run that probed nothing is not a pass.** `is_connected` is false when every probe
  was skipped for want of a credential.
- **The catalogue probe compares the live model list against the ids we pinned**, so a
  provider retiring a model is caught here rather than in a user's dictation.

Two upstream behaviours are deliberately dropped: its hardcoded provider list (ours comes
from the registry, so an unknown id is an error rather than a probe of OpenAI), and its
`gate: false` flag that downgraded some failures to warnings. `ProbeOutcome` has no
warning state.

## Network policy

`network-policy` owns the only `reqwest::Client` in the workspace. Constructing a request
requires a `RequestPurpose`, so a call site that cannot name its purpose does not compile. See
`NETWORK_BOUNDARY.md`.

## Key interfaces

### Dictation pipeline

The first vertical slice. Each arrow is a crate boundary, so each stage is testable alone:

```text
platform::shortcut → audio-core::MicStream → audio-core::Vad
  → transcription-core::resolve_route → Executor{Local|HttpBatch|Streaming}
  → reasoning-core::cleanup (optional) → platform::TextInserter
  → notes-core::history
```

`resolve_route` is a direct port of upstream's `transcriptionRoute.ts`: pure, secret-free, and
returning a route whose `auth.key_ref` is a *slot name*. Only `security::Credentials` can turn a
slot into a secret, and only `network-policy` can send it. Two consequences worth stating: no test
needs a real credential, and a secret cannot reach a log line by accident.

### Meeting providers

```rust
#[async_trait]
pub trait MeetingTranscriptionProvider {
    async fn connect(&mut self, config: MeetingProviderConfig) -> Result<()>;
    async fn send_audio(&mut self, chunk: AudioChunk) -> Result<()>;
    async fn pause(&mut self) -> Result<()>;
    async fn resume(&mut self) -> Result<()>;
    async fn finish(&mut self) -> Result<MeetingTranscript>;
    async fn cancel(&mut self) -> Result<()>;
    fn capabilities(&self) -> MeetingProviderCapabilities;
}
```

`MeetingProviderCapabilities` must describe what each provider *actually* does, not a uniform
shape. Keyterms, for instance:

```rust
enum KeytermSupport {
    Unsupported,
    Supported { max: Option<usize>, wire: WireShape },
}
```

because AssemblyAI caps at 100 terms, Deepgram uses repeated query params, Corti uses a JSON
config block, and OpenAI Realtime and Tinfoil **silently ignore keyterms entirely**. It also
carries `audio_leaves_device`, `session_limit` (OpenAI: 60 minutes), `token_rotation` (Deepgram
only), `sample_rates`, `max_audio_bytes`, `speaker_labels`, `partials`, `finals`,
`language_detection`, `reconnect`, and `batch_fallback`.

The provider allowlist stays *derived* from the client registry, as upstream does deliberately: a
provider that is permitted but has no client must be a compile error, never a silent fall-through
to OpenAI.

### Overlay diagnostics

Capture exclusion returns evidence, never a boolean:

```rust
enum CaptureExclusion {
    Active { verified_by: Evidence },
    Unsupported { reason: String },
    Failed { error: String },
    Unknown { reason: String },
}
```

No code path may report `Active` by assumption. That is the specific defect found in Pluely,
and `PLATFORM_SUPPORT.md` is where the result surfaces.

## Meeting providers, and what is not built

`meeting-core` holds the contract — the trait, the capability model, and the allowlist derived
from it — and none of the five WebSocket clients. That is a deliberate stopping point: each client
needs real credentials against a live service to verify, and one that compiles without ever
having connected is worth very little. `is_allowed` returns true only for a provider with a
client, so the gap is visible in the type rather than described in a document.

## Storage

SQLite via `sqlx`, owned entirely by the `storage` crate.

Upstream has **no versioned migration runner** — the schema is rebuilt idempotently on every boot
from roughly 70 `ALTER TABLE`/catch-duplicate-column blocks, and `pragma user_version` is used
only as a one-time data-seed sentinel. The on-disk schema version is therefore unknowable from
the database itself.

We do not reproduce that. Instead:

1. **Flatten.** `0001_initial.sql` describes the final post-ALTER shape of the tables we keep.
   There is no reason to replay 70 incremental migrations for a schema no KnegisWhisper user has
   ever had.
2. **Version properly from day one.** `sqlx::migrate!` with a `_migrations` table. Every later
   change is a numbered file. No `IF NOT EXISTS` schema-drift patching, ever.
3. **Import rather than migrate.** An OpenWhispr database is a *foreign* database, so it gets a
   one-way importer: attach read-only, introspect with `pragma table_info` (the source column set
   is genuinely unknowable up front), copy what we keep, drop every cloud/sync column, and rebuild
   the FTS index. It never writes to the source file, which makes it inherently reversible.
4. **Relocate secrets during import.** Upstream stores calendar refresh tokens in plaintext
   SQLite columns. The importer moves them into the OS credential store and leaves only a key
   reference behind. Google Calendar and Outlook resolve an Advanced custom client before the
   app-owned compile-time client, otherwise Missing. Grants are bound to their issuing client;
   unbound/imported grants require reconnecting. Google expanded `events.list` and Outlook Graph
   v1.0 `calendarView` fetch snapshots over [now - 24 hours, now + 30 days). Each calendar commits
   atomically with overlap pruning; failed pages keep its cache. A snapshot-version gate hides
   old caches until their first new sync purges them. Local Upcoming and candidate readers use
   that gate without network I/O. Meetings-open sync is throttled to once per 5 minutes per
   provider, alongside Sync now; never launch, focus or timer. Meetings Upcoming/event picker
   and the recording-event sidecar link are built and fixture-tested. Calendars need no database migration;
   new persisted serde fields default on read. Real accounts remain unverified; see
   [the provider runbook](CALENDAR_PROVIDER_SETUP.md).

Dropped on import because they exist only to serve OpenWhispr Cloud: the `spaces`,
`pending_vector_purges`, and `optimistic_folder_delete_rows` tables, and every `cloud_id`,
`sync_status`, `deleted_at`, `client_*_id`, `left_team`, `updated_by_user_id`, `cloud_updated_at`,
`owner_user_id`, `is_shared`, and `share_token` column, plus the triggers that populate them.
That removes roughly a third of the schema surface.

Two upstream designs to keep: the **FTS5 external-content** table mirroring `notes` through three
triggers, and the **ASCII-safe cache path fallback** — relocating the model cache and the audio
scratch directory when the user data path contains a non-ASCII character, because the native
whisper and parakeet binaries cannot open a file under such a path.

The fallback is kept; the *rule* was narrowed on 2026-08-31 after being measured for the first
time. Upstream's version also rejected spaces and also applied to the binaries' own directory, and
neither of those is real. See `kw_storage::paths` and `PLATFORM_SUPPORT.md`.

## Sidecars

Six sidecars are pinned in `config/sidecars.json` and downloaded when the user installs them;
none is kept in this repository.

- Three are long-lived loopback servers: `whisper` (whisper.cpp's server), `llama` (llama.cpp's
  server) and `sherpa-stream` (sherpa-onnx's streaming WebSocket server, for local streaming
  meetings).
- Two run once per job over stdio: `ffmpeg`, which converts imported media to WAV, and
  `sherpa`, sherpa-onnx's command-line tools for file transcription and speaker labels.
- `yt-dlp` is pinned for URL import, which is withdrawn, so nothing installs or starts it.

The local API is a fourth loopback server, but it runs inside the application rather than as a
child. Known voices extracts voice samples in a second copy of the application, started with
`--known-voices-worker`, so a native crash there cannot close the app. Each child is started by
Rust; the frontend cannot spawn anything.

Two upstream mechanisms are ported rather than reinvented: a **multi-candidate binary path probe**
that requires the executable bit and returns `None` for graceful fallback, and a **PID-file reaper
that verifies the process command line** before signalling. Without that verification, a stale PID
file gets an unrelated process killed after a crash.

Versions are pinned and digests verified. The pinning is deliberate: an upstream whisper.cpp bump
must not change transcription output between releases.

## Security posture

Detailed in `SECURITY.md`. In summary: strict CSP, least-privilege capabilities, typed command
payloads, secrets in the OS credential store, loopback-only listeners with constant-time token
comparison, and no logging of credentials, tokens, clipboard contents, raw audio, or full
transcripts.
