# Completion plan

What is left across every phase, in the order it should be done. Maintained in place: when
something here becomes false, edit it rather than appending a note. The reasoning behind individual
choices lives in [ENGINEERING_LOG.md](ENGINEERING_LOG.md); this file is about what is left.

> **Read [BUILD_PLAN.md](BUILD_PLAN.md) alongside this.** The two were written independently and
> letter their chunks differently — this file's `H` is that file's `D`, and so on — so a bare chunk
> letter is ambiguous between them. Where they disagree about what is done, `BUILD_PLAN.md` is the
> one being kept current against the commits.
>
> **Chunks B, C, D, E, F, G, I and J have been removed from this file because they are finished**,
> along with everything else here that has landed. The letters are not renumbered: a gap where a
> chunk used to be is more useful than a renumbering that silently invalidates every reference to
> it. (Task 19 removed E, I and J on this pass — CLI screen, meetings, and calendars respectively —
> following the same convention this file already established for the others.)

## Where this actually stands

**59 of 102 integrations are reachable**, up from 8 when this file was written and 45 when this
section was last written. `node tools/integration-audit/verify-parity.mjs` prints the remaining
**8** on every successful run, grouped by surface — down from 19, and every one of those 8 now
names a *built* surface, which was not true when this section was first drafted. It read 7 until
URL import was withdrawn on 2026-08-27, the only entry on that list that regressed rather than
never having been built.

The framing this file opened with — *"the dominant problem is not missing subsystems; it is built,
tested backend with no way in"* — was right, and that problem is now closed everywhere except the
two items still open below. The credential screen, the intelligence screen, the enterprise shapes,
the local LLM lifecycle, the meetings screen and its clients, the calendar screen, the media
importer, and the MCP and CLI screens all exist. What is left is genuinely new capability that
either has no consumer yet or has an explicit, permanent reason it stays undone:

1. **CI has not run on push since `afe59b4`, and still has not — deliberately.** Three workflow
   files (`ci.yml`, `release.yml`, `pin-verify.yml`) were written, reviewed character by character
   for the property that matters most given the Actions budget (none can fire on a push), and
   dispatched manually. The triggers remain commented out on purpose this run, not by oversight —
   see `docs/BUILD_PLAN.md` chunk J. The macOS and Linux jobs are still the only runtime evidence
   this project has for those platforms. Chunk A.
2. **One of the two remaining Windows natives is written; the other is declined.** The
   `IAudioSessionManager2` mic listener landed on 2026-09-01 (KNEG-94) because Teams forced it —
   see chunk H. The UIAutomation text monitor is closed as declined rather than deferred (KNEG-63):
   chunk I shipped without it, nothing filed since has asked for it, and a continuous read of the
   focused element is a materially larger privacy surface than anything else here, being weighed
   with no feature on the other side of the scale.

The other item that used to sit here — *four of five hotkeys register real OS shortcuts and do
nothing* — was chunk C and is fixed. Those four actions are no longer registered at all, and the
settings screen names the action and says why. The keys they used to claim are back with the
applications that use them.

## Order, and why

Ordered by integrations unlocked per unit of work, not by phase number.

| # | Chunk | Unlocks | New subsystem? |
| --- | --- | --- | --- |
| A | CI back on | — | no |
| H | The two remaining Windows natives | — | yes |
| K | AEC helper | — | yes, deferred on purpose |

Everything else this table used to carry — the CLI screen, meetings, and calendars — is done; see
`docs/BUILD_PLAN.md` chunks E, F and H for what actually landed.

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

## H — The two remaining Windows natives *(one built, one declined)*

System audio and the key listener are done and verified on hardware — see
[BUILD_PLAN.md](BUILD_PLAN.md) chunk D and the tables in
[PLATFORM_SUPPORT.md](PLATFORM_SUPPORT.md). The other two capabilities settled in
[PORTING_MATRIX.md](PORTING_MATRIX.md) as Rust rather than inherited binaries have now ended in
opposite places, and the difference between them is worth stating because it is the whole argument
this chunk was making.

**Mic listener — built 2026-09-01, KNEG-94.** `kw_platform::microphone` enumerates capture
sessions through `IAudioSessionManager2`, and `meeting_detect::detect` takes the result beside the
process list.

The prediction this chunk made was that the capability would wait for a consumer. What happened was
the reverse: a defect nobody could fix any other way went looking for a capability. New Teams runs
the call inside `ms-teams.exe` and spawns nothing — measured 2026-08-26, 2026-08-28 and
2026-08-31 — so **no process name could ever have separated a call from an open client**, and
Teams was unofferable for three weeks. `mic_users_probe` measured the difference on 2026-09-01 with
a cross-process control on every run: Teams idle holds no capture session, Teams in a call holds
one. Windows only; nothing equivalent is read on Linux or macOS.

**Text monitor — declined, KNEG-63, at no priority beside KNEG-69 and KNEG-77.** No consumer,
and unlike the microphone half, no defect pointing at it either. A UIAutomation focused-element
monitor reads what the user is looking at, continuously; that needs its own disclosure and there is
nothing on the other side of the scale to weigh it against. Closed as a decision rather than left
in the backlog reading as unfinished work.

Also still open, and not Windows: the Linux and macOS shortcut fallbacks. `zbus` against the GNOME,
KDE and Hyprland portals, with `PLATFORM_SUPPORT.md` continuing to say per compositor what Wayland
refuses. Unverifiable from this machine.

## K — AEC helper

**Declined for now, not unfinished** — corrected mid-run from "blocked on a toolchain and a release
run" to a stated, permanent-until-reversed decision. Design and both measured third-party pins are
settled in [PORTING_MATRIX.md](PORTING_MATRIX.md); `config/integration-manifest.json`'s
`platform:aec` entry carries `temporarily-deferred` with a full `deferral` block, not
`ported-with-new-rust-implementation` as it once wrongly did — nothing implementing it exists: no
helper, no generator port, no workflow, and no C++ toolchain on this development machine to build
one with. A quality chunk, not a gate: without echo cancellation a meeting recorded on speakers
transcribes the far-end voice twice, and on headphones there is nothing to cancel. What actually
gates whether a meeting can be recorded at all — system audio capture — is done.

---

## Phases 7–10, defined

They existed only as "Not started" in the README. This is the scope later work aims at.

**Phase 7 — developer surface. Done, except the docs.** The `tools/knegiswhisper-mcp` stdio server
is built — entirely new work, because upstream's MCP is a hosted service with no local
implementation anywhere in the checkout — with `Integrations > MCP` and 28 crate tests plus a
protocol-test suite. `docs/agent-skills/` rewriting the two upstream `SKILL.md` files to reference
the local API rather than `api.openwhispr.com` **was not done** — `developer:agent-skills` claimed
`status: "ported"` with that target for the whole run and Task 19 found the directory does not
exist, corrected to `temporarily-deferred` rather than written under time pressure at the close of
an unrelated task.

**Phase 8 — the deferred model work. Media import done; the rest deferred.**

- Parakeet and Nemotron via the sherpa-onnx sidecar (4 entries). **Unblocked technically, blocked
  narrowly.** bz2 extraction landed with its consumer, and a probe installed the real pinned CLI and
  both pinned Parakeet archives and drove real transcriptions matching whisper's `tiny` model word
  for word — the runtime is proven. What remains for those two: a `Settings > Transcription > Local`
  selector, deliberately not built as part of the measurement task. The two Nemotron models are a
  different, still-unstarted gap: they need a streaming client for
  `sherpa-onnx-online-websocket-server`'s own protocol, which nobody has written. See
  [BUILD_PLAN.md](BUILD_PLAN.md) chunk B4.
- Audio and video import via ffmpeg — **done.** `Notes > Upload` is built and three of the four
  media entries (`media:{audio,video}-import`, `media:openwhispr-import`) are reachable.
- **URL import is disabled**, and is one of the eight entries the parity gate reports as
  unreachable. The up-front literal-host check cannot see where a hostname resolves at connect
  time, nor any redirect or extractor-generated request `yt-dlp` makes on its own, so
  `media_import_url` was unregistered as a command rather than shipped with a guard that does not
  cover the fetch. The backend is dormant, not deleted. See `docs/SECURITY.md`'s Network section
  and `docs/NETWORK_BOUNDARY.md`'s "SSRF guards for URL import".
- Semantic search, still `temporarily-deferred`, carrying a dependency claim nobody has
  re-checked. **Diarization is no longer beside it**: it was built on 2026-09-01 (KNEG-123,
  KNEG-124) once the three dependencies its `deferral` block named turned out not to be needed —
  the pinned sherpa release already ships the CLI. Both models are pinned and reachable through
  `Settings > Meetings`. What remains unproven is whether the speaker turns are *right*, which
  needs a two-person recording.

**Phase 9 — shipping.** Done except by-hand verification. `.github/workflows/release.yml`
(`workflow_dispatch` only) builds and publishes unsigned nsis/dmg/appimage/deb bundles on
all three OSes. `createUpdaterArtifacts` is now `true`, and the update check itself
(`apps/desktop/src-tauri/src/updater.rs`) is opt-in and user-triggered, against this
project's own GitHub releases through `kw_network_policy::NetworkPolicy` -- **not**
`tauri-plugin-updater` as originally planned here, so that crate's "only egress point in
the workspace" stays a real property rather than a nominal one with a second HTTP client
sitting beside it. `docs/SECURITY.md` documents the keypair procedure; no keypair exists,
so a check reports `NoPublicKeyConfigured` rather than checking unsafely or silently. The
workflow has never been dispatched and no release has ever been published, so every path
past `git tag` -- including whether the bundler actually needs the signing key present
when `createUpdaterArtifacts` is on -- stays unverified; see `docs/PLATFORM_SUPPORT.md`.

**Phase 10 — agents.** Chat agent, voice agent and translation: the window role, the dispatch, the
four `*-custom` credential slots, and the orphan tables. The home for the three actions whose
hotkeys now say, honestly, that nothing handles them.

The five `*-custom` credential slots can now be *stored* — `Settings > Providers` lists them, with
the caveat that a key alone configures nothing until the feature that owns the base URL exists. This
phase is what makes them mean something.

## Loose ends worth closing as they are passed

Not chunks of their own; each belonged to whichever chunk touched it first. Two of the three closed
as work. The third — the orphan tables — closed on 2026-09-01 as a **recorded decision**: the tables
stay, one of them gained a candidate owner, and the invariant nobody had written down is now pinned
by a test.

- **All three `RequestPurpose` variants named here now have callers — closed.**
  `UserTriggeredMediaImport` is called from `apps/desktop/src-tauri/src/media.rs`;
  `UserConfiguredCalendarProvider` from `crates/calendar-core`'s exchange and sync code;
  `OwnGithubReleasesUpdater` from `apps/desktop/src-tauri/src/updater.rs`. None of the seven
  `RequestPurpose` variants is still a promise with nothing behind it.
- **Eight write-only tables — still open, and one now has a candidate owner.** `snippets`,
  `actions`, `agent_conversations`, `agent_messages`, `contacts`, `speaker_profiles`,
  `speaker_mappings` and `note_speaker_embeddings` are created by `0001_initial.sql` and written by
  `storage::import::from_openwhispr`. Nothing reads any of them back and no manifest entry mentions
  them. Phase 10 claims four. Nothing in this plan's 19 tasks, or the one added mid-run, touched
  phase 10.

  **"Touched only by the importer" was wrong, and the honest phrase is *write-only ownership*.**
  Four of the eight are depended on elsewhere. All eight are asserted to exist by
  `every_expected_table_exists_and_no_cloud_table_does` (`crates/storage/src/lib.rs:268`);
  `agent_messages` is the subject of `foreign_keys_are_enforced` (`:464`) and `speaker_mappings` of
  `deleting_a_note_cascades_to_its_speaker_rows` (`:476`); four are named to the user in a shipped
  sentence at `apps/desktop/src/views/NotesUpload.tsx:342-343` — snippets, custom actions, agent
  conversations and speaker profiles; and `contacts` is cited **normatively** by two diarization
  modules (`apps/desktop/src-tauri/src/diarization.rs:36`,
  `crates/meeting-core/src/diarization.rs:6`) as a thing they deliberately do not reach for. The
  importer writes them, tests and comments depend on them, nothing reads them back.

  **`snippets` stays — decided 2026-09-01, not deferred again.** It is not inert:
  `crates/storage/src/import.rs:106` copies it and the screen above promises the user it will.
  Dropping it would discard a column of the user's OpenWhispr data that a shipped sentence says is
  copied, to save a five-column table that costs nothing to keep.

  **`speaker_mappings` has a candidate owner it did not have a week ago.** Diarization writes
  `speaker_0` with the placeholder `Speaker 1` beside it, and
  `kw_notes_core::transcript::speaker_name` already prefers a mapping over the placeholder
  (`crates/notes-core/src/transcript.rs:127-129`), so `speaker_mappings` is exactly where a name a
  person typed would live. No screen offers that rename and **nothing was built here** — the owner
  is recorded so the next person past does not have to derive it again.

  **`contacts` is in `COPY_PLAN` and not in `MUST_BE_EMPTY`, and that is deliberate.** The rule was
  exact and unwritten; it is now written on `MUST_BE_EMPTY` (`import.rs:224-241`) and pinned by
  `must_be_empty_is_exactly_the_autoincrement_tables`. `MUST_BE_EMPTY` guards one risk — the
  importer preserves row ids, so a non-empty target could collide — and it holds precisely the
  `COPY_PLAN` tables declared `INTEGER PRIMARY KEY AUTOINCREMENT`, with no exception in either
  direction. `contacts` is keyed by `email`, `speaker_mappings` and `note_speaker_embeddings` by
  `(note_id, speaker_id)`, the four calendar tables by a provider-supplied `TEXT` id: none has a
  surrogate id to collide. The test fails if a future table joins one list and not the other.

  **No migration, and that is the shape of this closing.** There is one migration file and
  `migrations_are_idempotent_across_reopens` (`lib.rs:250-265`) asserts `applied == 1`, so adding
  `0002_*.sql` fails it. This closes as a recorded decision rather than as work.
- **`storage::import::from_openwhispr` has its manifest entry and its screen — closed.**
  `media:openwhispr-import` is `reachable: true` through `Notes > Upload`, landed together with the
  audio and video importers exactly as planned. The URL importer landed with them and was
  withdrawn afterwards; see the media-import entry above.

---

## Blocked, and what unblocks it

| Item | Needs | What is *not* blocked by it |
| --- | --- | --- |
| Anything macOS-native | A Mac, or someone with one | It compiles in CI and ships marked unverified |
| macOS `whisper-server` | A macOS CI job publishing under this project's own releases, which needs Actions release permissions | Windows and Linux install from upstream `ggml-org` today, digest-pinned. `llama-server` publishes real macOS binaries and is already pinned for both |
| A sherpa-onnx local-ASR selector | A Tauri command layer and `Settings > Transcription > Local` UI wiring — not built, deliberately, as a task of its own | The runtime: measured, proven, transcribes correctly |
| Nemotron streaming local ASR | A client for `sherpa-onnx-online-websocket-server`'s own protocol — unwritten | The offline Parakeet path, which is unrelated |
| 4 meeting WebSocket clients, live | Keys for AssemblyAI, Deepgram, OpenAI Realtime, Corti | The capability model, the allowlist, system audio, meeting detection, and `meeting-stream:local`, all built |
| Calendars, real round trip | Google and Microsoft OAuth client IDs registered as PKCE **public** clients — upstream ships a client secret in the binary, which in an open-source build is not a secret | The flow, storage, and UI, all built |
| Apple Calendar | A macOS machine to write and verify an EventKit binding against, and the binding itself — neither exists | Google and Microsoft calendar sync, unrelated |
| Tinfoil (transcription and meeting-stream) | Enclave attestation verification against a hardware root of trust | Nothing. Both are **declined**, not pending: an unattested request would present as Tinfoil while providing none of the confidentiality the name means |
| The rewritten agent-skills docs | Someone to write `docs/agent-skills/` — claimed done, was not; corrected rather than written under time pressure | Nothing else — documentation only |
| Signed installers | A code-signing identity | Unsigned builds |
| The AEC helper | cmake, MSVC/clang, and a workflow run with `contents: write` | Meetings themselves — system audio, the thing that actually gates recording, is done |
| CI actually running | Someone uncommenting the triggers in `.github/workflows/ci.yml` — deliberately not done this run | Every gate, which still runs by hand and has, repeatedly |
| A published release | Someone dispatching `release.yml` for a real tag — never done | The workflow itself, which builds on `workflow_dispatch` today |

## Verification

Every chunk keeps all of it green: `cargo test --workspace` (**1296 passing, 0 failed**, quoted
from the actual run rather than restated),
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, `tsc --noEmit`,
`vite build`, the domain scanner and its self-tests, `verify-binaries`, and the parity gate with its
self-tests.

**The parity gate was the acceptance test for every chunk this file used to list as open.** Each
ended by flipping entries to `reachable: true` and a surface to `built: true`, and the gate refuses
the claim unless the component exists and renders its `data-surface` marker. The unreachable count
it prints on success is the progress measure: **7 today**, down from 19 when this section was last
written, and **all 13 UI surfaces are built** — zero remain unbuilt, where 5 did not exist when this
plan began.

By hand on Windows, which is the only thing that actually proves any of it — **and which has not
happened for anything this file used to track as open**: `knegiswhisper health` against the CLI
screen's port, joining a real call and watching detection fire, running `meeting-stream:local` end
to end, and a real calendar round trip all remain undone by a person, exactly as they were when this
section was last written. See `docs/BUILD_PLAN.md`'s Verification section for the full by-chunk
breakdown of what has and has not been run.

**See [BENCH_CHECK.md](BENCH_CHECK.md) for the complete, current record of what has actually been
run.** The 2026-08-25/26 passes covered phases 2, 6 and chunks A–D on real hardware and found eight
real defects between them that every test had missed. **Nothing built since — meetings, calendars,
media import, the MCP and CLI screens, the updater — has been run by a person at all.** That file's
by-hand lists for chunk E onward and for the updater are the honest record of that, item by item,
and none of them should be read as done until someone actually presses the buttons.
