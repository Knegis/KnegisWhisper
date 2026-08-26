# Completion plan

What is left across every phase, in the order it should be done. Maintained in place: when
something here becomes false, edit it rather than appending a note. The reasoning behind individual
choices lives in [ENGINEERING_LOG.md](ENGINEERING_LOG.md); this file is about what is left.

> **Read [BUILD_PLAN.md](BUILD_PLAN.md) alongside this.** The two were written independently and
> letter their chunks differently — this file's `H` is that file's `D`, and so on — so a bare chunk
> letter is ambiguous between them. Where they disagree about what is done, `BUILD_PLAN.md` is the
> one being kept current against the commits.
>
> **Chunks B, C, D, F and G have been removed from this file because they are finished**, along
> with everything else here that has landed. The letters are not renumbered: a gap where a chunk
> used to be is more useful than a renumbering that silently invalidates every reference to it.

## Where this actually stands

**45 of 99 integrations are reachable**, up from 8 when this file was written.
`node tools/integration-audit/verify-parity.mjs` prints the remaining 19 on every successful run,
grouped by surface.

The framing this file opened with — *"the dominant problem is not missing subsystems; it is built,
tested backend with no way in"* — was right, and that problem is now largely closed. The credential
screen, the intelligence screen, the enterprise shapes and the local LLM lifecycle all exist. What
is left is genuinely new capability, plus one thing that belonged to no plan and still does not
have one:

1. **CI has not run on push since `afe59b4`.** The triggers are commented out "temporarily" and no
   reason was recorded. The macOS and Linux jobs are the only runtime evidence this project has for
   those platforms. Chunk A.

The other item that used to sit here — *four of five hotkeys register real OS shortcuts and do
nothing* — was chunk C and is fixed. Those four actions are no longer registered at all, and the
settings screen names the action and says why. The keys they used to claim are back with the
applications that use them.

## Order, and why

Ordered by integrations unlocked per unit of work, not by phase number.

| # | Chunk | Unlocks | New subsystem? |
| --- | --- | --- | --- |
| A | CI back on | — | no |
| E | CLI screen | 1 | no |
| H | The two remaining Windows natives | — | yes |
| I | Meetings screen and clients | 6 | yes |
| J | Calendars | 3 | yes |
| K | AEC helper | — | yes, blocked |

---

## A — CI back on

`.github/workflows/ci.yml`: uncomment the `push` and `pull_request` blocks, and drop the
"TEMPORARILY DISABLED" header. Every gate passes locally, so this is a two-line change that stops
the tree drifting unverified, and it is first because everything after it should be gated by it.

Remove the engineering-log entry describing the pause, so the record stops describing a state that
has ended.

Two additions the tree has grown since this was written, both belonging here:

- The `ipc_types` check is a `cargo test`, not a CI step, so it is already covered by the Rust job —
  no new step needed, which was the point of making it a test.
- **`tools/sidecar-pin/pin.mjs --verify` deserves a scheduled job of its own.** It re-downloads every
  pinned target, so it does not belong in the per-push contracts job — but nothing else notices a
  release retagged in place, and a pin that is never re-measured is a pin nobody is checking. Weekly
  is enough.

## E — CLI screen

The smallest item here. `tools/knegiswhisper` is complete and verified end to end against the running
API as a compiled binary, and nothing in the application says it exists or where the discovery file
is. A short `Integrations > CLI` section beside the Local API one: discovery file path, current
port, and the noun-verb surface. One integration, and it finishes phase 6.

## H — The two remaining Windows natives

System audio and the key listener are done and verified on hardware — see
[BUILD_PLAN.md](BUILD_PLAN.md) chunk D and the tables in
[PLATFORM_SUPPORT.md](PLATFORM_SUPPORT.md). What this chunk still holds is the other two capabilities
settled in [PORTING_MATRIX.md](PORTING_MATRIX.md) as Rust rather than inherited binaries:

- **Text monitor** — a UIAutomation focused-element read.
- **Mic listener** — `IAudioSessionManager2`, for "another application is using the microphone".

Both serve meetings, so they sit **behind** chunk I rather than ahead of it: neither has a consumer
until the meetings screen exists, and a capability with no consumer is the scaffolding this
workspace avoids.

Also still open, and not Windows: the Linux and macOS shortcut fallbacks. `zbus` against the GNOME,
KDE and Hyprland portals, with `PLATFORM_SUPPORT.md` continuing to say per compositor what Wayland
refuses. Unverifiable from this machine.

## I — Meetings

`Settings > Meetings` plus the five WebSocket clients. The capability model and the client-derived
allowlist are done, and `is_allowed` returns true only for a provider with a client, so the gap is
visible in the type.

System audio — the thing that actually blocked this — is now built and verified, so what remains is
the clients, the detectors, and the screen. Blocked on live keys for four of the five providers;
`meeting-stream:local` needs none and is the one that can be verified here.

## J — Calendars

Loopback PKCE against Google and Microsoft, refresh tokens as `token_ref` into the OS credential
store — never the plaintext columns upstream used, which `assert_no_credential_columns` already
guards. Buildable and testable against a stub authorisation server; only the real round trip needs
client IDs registered as PKCE **public** clients.

## K — AEC helper

Blocked on a toolchain and a release run. Design and both measured third-party pins are settled in
[PORTING_MATRIX.md](PORTING_MATRIX.md). A quality chunk, not a gate: without echo cancellation a
meeting recorded on speakers transcribes the far-end voice twice, and on headphones there is nothing
to cancel.

---

## Phases 7–10, defined

They existed only as "Not started" in the README. This is the scope later work aims at.

**Phase 7 — developer surface.** The `tools/knegiswhisper-mcp` stdio server — entirely new work,
because upstream's MCP is a hosted service with no local implementation anywhere in the checkout —
and `docs/agent-skills/` rewritten to reference the local API rather than `api.openwhispr.com`. Two
manifest entries.

**Phase 8 — the deferred model work.**

- Parakeet and Nemotron via the sherpa-onnx sidecar (4 entries). **Blocked, and the reason is now
  specific**: every desktop asset is `.tar.bz2`, which `install::extract` cannot open; the binaries
  are a one-shot CLI and a WebSocket server rather than an HTTP endpoint, so neither
  `Transport::Loopback` nor `HttpHealth` applies; and the file layout differs per model family. See
  [BUILD_PLAN.md](BUILD_PLAN.md) chunk B4.
- Audio, video and URL import via ffmpeg and yt-dlp, with the SSRF guards already specified in
  [SECURITY.md](SECURITY.md) (3 entries).
- Diarization and semantic search, which are the three `temporarily-deferred` entries and already
  carry dependencies and named interfaces.

**Phase 9 — shipping.** `tauri-plugin-updater` against this project's own releases, a bundle job in
CI, and signing. `tauri.conf.json` already declares nsis/dmg/appimage/deb targets with
`createUpdaterArtifacts: false`, and no CI job builds any of them.

**Phase 10 — agents.** Chat agent, voice agent and translation: the window role, the dispatch, the
four `*-custom` credential slots, and the orphan tables. The home for the three actions whose
hotkeys now say, honestly, that nothing handles them.

The five `*-custom` credential slots can now be *stored* — `Settings > Providers` lists them, with
the caveat that a key alone configures nothing until the feature that owns the base URL exists. This
phase is what makes them mean something.

## Loose ends worth closing as they are passed

Not chunks of their own; each belongs to whichever chunk touches it first. All three were still true
at the last check.

- **Three `RequestPurpose` variants have no caller.** `UserTriggeredMediaImport`,
  `UserConfiguredCalendarProvider` and `OwnGithubReleasesUpdater` appear only in the allowlist array
  in `crates/network-policy/src/lib.rs`. They are promises with nothing behind them; phase 8,
  chunk J and phase 9 respectively make them true.
- **Five orphan tables.** `snippets`, `actions`, `agent_conversations`, `agent_messages` and
  `contacts` are created by `0001_initial.sql` and touched only by the importer. No crate owns them
  and no manifest entry mentions them. Phase 10 claims four; `snippets` needs a decision.
- **`storage::import::from_openwhispr` still has no manifest entry.** The log says that was
  deliberate until it was reachable. It has a Tauri command and an `ipc.ts` wrapper and still no
  screen, so the entry and its surface should be added together — which is phase 8's media-import
  work, since that is where `Notes > Upload` gets built.

---

## Blocked, and what unblocks it

| Item | Needs | What is *not* blocked by it |
| --- | --- | --- |
| Anything macOS-native | A Mac, or someone with one | It compiles in CI and ships marked unverified |
| macOS `whisper-server` | A macOS CI job publishing under this project's own releases, which needs Actions release permissions | Windows and Linux install from upstream `ggml-org` today, digest-pinned. `llama-server` publishes real macOS binaries and is already pinned for both |
| The five meeting WebSocket clients | Live keys for AssemblyAI, Deepgram, OpenAI Realtime, Corti, Tinfoil | The capability model, the allowlist, system audio, meeting detection, and `meeting-stream:local`, which needs no key |
| Calendars, real round trip | Google and Microsoft OAuth client IDs registered as PKCE **public** clients — upstream ships a client secret in the binary, which in an open-source build is not a secret | The flow, storage, and UI |
| Tinfoil transcription | Enclave attestation verification against a hardware root of trust | Nothing. It is **declined**, not pending: an unattested request would present as Tinfoil while providing none of the confidentiality the name means |
| sherpa-onnx local ASR | bz2 extraction, a `Transport::Stdio` executor path, and the binary on a machine to learn its per-model flags | Everything else local — whisper transcription and llama generation both work |
| Signed installers | A code-signing identity | Unsigned builds |
| The AEC helper | cmake, MSVC/clang, and a workflow run with `contents: write` | Meetings themselves |

## Verification

Every chunk keeps all of it green: `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, `tsc --noEmit`,
`vite build`, the domain scanner and its self-tests, `verify-binaries`, and the parity gate with its
self-tests.

**The parity gate is the acceptance test for E and I.** Each ends by flipping entries to
`reachable: true` and a surface to `built: true`, and the gate refuses the claim unless the component
exists and renders its `data-surface` marker. The unreachable count it prints on success is the
progress measure: **19 today.**

By hand on Windows, which is the only thing that actually proves any of it:

- **E** — `knegiswhisper health` against the port the screen displays.
- **I** — join a call in Zoom and see detection fire; run `meeting-stream:local` end to end with
  system audio and the microphone both recording.

**See [BENCH_CHECK.md](BENCH_CHECK.md) for what has actually been run.** The 2026-08-26 pass
covered everything reachable without a person and found three more defects. Seven items still need
somebody at the machine, and the ones that matter most are chunk A's and B's central claims: storing
a provider key and watching a connection test pass, switching cleanup on and reading the tidied
text, installing a local model and holding a chat. All green in tests; none of it done by a person.
The two by-hand passes so far have found eight real defects between them that every test had
missed.
