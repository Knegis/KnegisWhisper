# Engineering log — judgement calls, deviations, and carry-forward

A record of choices made in the moment and why: what was decided, what the alternative was, and
why the alternative was rejected. The reasoning is the point, not the outcome.

This file is **committed**, unlike the scratch `DECISIONS.local.md` it grew out of. The reason is
in the first entry below: several of the findings here are latent defects that were invisible by
inspection, and a record of them that lives only on one machine is not a record.

Newest entries first. The carry-forward section immediately below is the exception — it is
maintained in place rather than appended to, because its purpose is to be current.

---

## Carry-forward — what the next session needs to know

> **All 19 tasks in the plan, plus one added mid-run (Task 20, the SSRF boundary fix), are
> complete.** [BUILD_PLAN.md](BUILD_PLAN.md) covers phases 0–9 as chunks A–J and every chunk is now
> either done or explicitly, permanently deferred with a named reason — there is no "not started"
> left in it. [PHASES_0_6_COMPLETION_PLAN.md](PHASES_0_6_COMPLETION_PLAN.md) keeps the phases 0–6
> detail and the by-hand check that found five real defects; [COMPLETION_PLAN.md](COMPLETION_PLAN.md)
> keeps the phase 7–10 definitions.
>
> **Measured, not asserted, as of 2026-09-01:** `cargo test --workspace` — **1537 passing, 0
> failed** (941 at the start of the original run, 1296 at its close on 2026-08-27); parity —
> **103 entries, 5 declared unreachable** (17 at the start, 8 on 2026-08-27), and **all 13 UI
> surfaces built**, with no "surface not built" entry remaining anywhere. The five: **one** local-ASR
> model (`nemotron-3.5-asr-streaming-0.6b`, the multilingual one — its `sha256` is `null`, so
> `sherpa_models::install` refuses with `MissingPin`), two Tinfoil entries (batch transcription
> and meeting-stream, both declined on principle, not unfinished), Apple Calendar (EventKit —
> described in `kw_calendar_core::apple`, implemented nowhere, on any platform), and URL import
> (withdrawn as a command — see the 2026-08-27 entry on the merged security fixes).
>
> Eight became six when `121dc4c` gave the two **Parakeet** models the Settings selector their
> measured runtime had been waiting for, and six became five on 2026-09-01 when the streaming
> protocol was measured and `nemotron-speech-streaming-en-0.6b` got a client. Worth spelling out
> because an earlier version of this paragraph named Parakeet as one of the unreachable pair, which
> is the opposite of what happened to it.
>
> **The reachability rule, settled and applied repeatedly:** `reachable: true` means a user can
> *configure* the integration through a named, **built** surface — not that the backend code works.
> `reachable: false` is the honest answer for a working runtime with no UI selector (the two sherpa
> Parakeet models), for a capability declared and refused on principle (both Tinfoil entries), and
> for a capability described but implemented nowhere (`calendar:apple`). Four separate times this
> run, a controller brief's arithmetic or premise collapsed "the code works" into "a person can
> reach it", and every time an implementer caught it by flagging rather than complying — see the
> dated entry below, "the reachability rule, stated once and applied four times".
>
> **The most significant finding of the whole run was Task 20's, not a planned task:** the network
> boundary's `https` and `wss` arms ran **no host check at all**, so `https://169.254.169.254/` and
> a redirect to it were permitted under every purpose — the classic SSRF target, unblocked, while the
> documentation called the boundary the control. Fixed centrally in `crates/network-policy`, not
> patched locally, because the gap was general and every existing call site had to be re-audited for
> a newly-refused legitimate self-hosted endpoint (none was found). The review of that fix then found
> a Critical of its own — a trailing dot (`localhost.`) defeated the whole predicate — and the
> more important structural fact underneath both: **the same kind of gap that used to fail closed
> now fails open**, because the fix inverted what a miss in `is_private_host` costs. See "Task 20 —
> the network boundary does not stop SSRF over HTTPS" below.
>
> **`Notes > Upload`'s import paths are now a capability, not an assumption (KNEG-80).**
> `pick_import_file` returned the chosen path to the renderer and `media_import_audio`,
> `media_import_video` and `import_openwhispr` read whatever they were handed; the renderer could
> not *obtain* a path (`capabilities/default.json` is `["core:default"]` and nothing else) but
> nothing *checked* that, so a compromised renderer could name any readable file and, with a cloud
> transcription provider configured, watch it leave the machine. `commands::DialogGrants` now
> records the canonical form of every dialog-returned path for the life of the process and those
> three commands — **the only three in the application that accept a path**, enumerated rather than
> assumed — refuse anything else. Session-lifetime because retry-after-failure exists today,
> count-bounded rather than timed for `meeting::Watcher::offered`'s reason, and keyed on
> `canonicalize` because `display()` is lossy — the cost being that a path whose name does not
> survive `display()` is refused outright. Four unit tests assert the policy; **only a real call
> against the running application proves it is consulted**, which is still open as item 1 under
> `BENCH_CHECK.md`'s Task 12 heading. See "a path the renderer could invent" below.
>
> **Task 19 audited every manifest entry whose `status` asserts a working implementation exists**
> (`verify-parity.mjs`'s `ACTIVE` set) and found three more instances of the same overclaim
> `platform:aec` was already corrected for mid-run: `meeting-stream:tinfoil-realtime` (status
> `"ported"`, no Tinfoil meeting client anywhere), `transcription:tinfoil` (status
> `"rewired-to-direct-byok"`, the client returns `ClientNotImplemented`), and `calendar:apple` (status
> `"platform-specific"`, `AppleCalendarSource` implemented by nothing on any platform). All three now
> carry `temporarily-deferred`, though for the two Tinfoil entries that status is a compromise, not a
> fit — this manifest's vocabulary has no term for "declined on principle", and each entry's
> `deferral.why` says so rather than letting the status imply the refusal is resource-blocked and
> temporary. A fourth, adjacent overclaim was found outside the `ACTIVE` set entirely:
> `developer:agent-skills` (`kind: "documentation"`) claimed `status: "ported"` naming
> `docs/agent-skills/` as where the rewritten docs live, and that directory does not exist — the
> gate's documentation check only requires a target to be *named*, never that it exist. Also
> corrected to `temporarily-deferred`. See "the ACTIVE-status audit" below.
>
> **[BENCH_CHECK.md](BENCH_CHECK.md) remains, in full, the record of what has actually been run on
> hardware, and chunk E is no longer absent from it.** The 2026-08-25/26 runs covered phases 2, 6
> and chunks A–D. Bench runs on **2026-08-27, 2026-08-28, 2026-08-31 and 2026-09-01** then reached
> chunk E: a meeting recorded, transcribed and saved end to end; two real Teams calls; system audio
> resolving a real output device; and the capture-session measurement behind KNEG-94. Calendars,
> media import, the MCP/CLI screens and the updater are **still green tests and code review only**.
> That file's own by-hand lists say which is which item by item, and nothing on them should be read
> as done until someone actually presses the buttons — including the one KNEG-94 left behind, which
> is watching the offer window appear during a call.
>
> **The cross-cutting gap in all four meeting socket clients is closed at the end of the meeting
> and still open during it (KNEG-79, 2026-08-31).** All four (`openai_socket.rs`,
> `assemblyai_socket.rs`, `deepgram_socket.rs`, `corti_socket.rs`) used to log a provider-reported
> error frame with `tracing::warn!` and never turn it into an `Err`, so a server that accepted a
> malformed session-config frame, stayed open and emitted nothing let a meeting "complete
> successfully" holding an empty transcript. They now record into
> `kw_meeting_core::fault::SessionFault` and both `send_audio` and `finish` consult it, so Stop
> reports the provider's own sentence. `stop()` was fixed with it — a pump failure used to return
> early, leaving the runtime idle and the screen stuck on "Recording" with no working button.
> **The mid-meeting half closed the same day (KNEG-95).** A dead provider now changes the
> indicator instead of leaving it saying "Recording" — a fourth `MeetingState`, the recording
> deliberately left running, and the provider's own sentence on `Settings > Meetings`. See the
> 2026-08-31 (KNEG-95) entry.
>
> **What is still true:** three of the four clients have never met a real provider error, because
> no key exists for AssemblyAI, Deepgram or Corti, so the wiring is what the tests check and not
> any provider's behaviour. See the 2026-08-31 (KNEG-79) entry below, and `BENCH_CHECK.md` item 13.
>
> **Eighteen of the thirty-five local text-generation models could not be installed at all**, and
> the reason was arithmetic in a config file rather than anything in the download path: upstream's
> `sizeBytes` was a base-10 "GB" label multiplied by 2^30 instead of 10^9, so eighteen models
> downloaded in full — 101 GB — and were deleted by a 5% size check on arrival. All 39 objects (35
> models, 4 drafters, 202 GB) are now measured from Hugging Face's `x-linked-size` into
> `LOCAL_LLM_PINS` in the registry generator, and a measured size is checked for **exact** equality
> rather than within 5%. Two related facts worth carrying: `qwen3.5-2b-q4_k_m` was passing the old
> check by 0.0042 percentage points, about 5.6 KB, and two Gemma models were already 2 240 bytes
> adrift from their declared size — upstream re-uploads are real, and the `#[ignore]`d
> `the_measured_sizes_still_match_hugging_face` test in `local_llm.rs` is what catches the next one.
> See the 2026-08-31 entry on the local-LLM sizes below.
>
> **All 39 also carry a SHA-256 as of the same day (KNEG-45), and 32 of them are the publisher's
> stated `lfs.oid` rather than bytes hashed here.** That is a weaker standard, deliberately taken,
> and it is carried through the code as a distinct state rather than a footnote:
> `sha256Measured` in the registry, `DigestCheck::Stated` in Rust, `VerifiedBy::StatedDigest` on
> the wire, and a different sentence on screen. Seven objects — one per publishing organisation —
> were downloaded and hashed to test the metadata before any of it was trusted. `node
> tools/model-pin/pin.mjs --verify-stated` re-reads all 39 in 36 requests and runs weekly.
>
> **The Windows DACL gap is closed as of 2026-08-31 (KNEG-34), and the two things it did *not*
> close are worth carrying.** `kw_security::file_acl` applies and reads back an explicit protected
> access list at all four call sites — the discovery file, its directory, and both of
> `kw_sidecar::pid`'s. Measured with `icacls` from outside the process: three trustees (the process's
> token user, `NT AUTHORITY\SYSTEM`, `BUILTIN\Administrators`), no `(I)` on any of them, on both
> paths. **Deliberately not owner-only** — an administrator can take ownership anyway, so what a
> confirmed Windows result claims is that the *unexpected* trustees are gone and inheritance is off,
> which is narrower and true. The snapshot that used to sit behind that verdict is gone — since
> KNEG-110 both screens re-read the access list on every status request. Still open: **CI compiles
> none of the Win32 code**, because the
> `windows-latest` job is off for every trigger but `workflow_dispatch`. The pure classifier is
> covered on Linux by design — `file_acl::classify` takes plain Rust facts and the measured DACL is
> a fixture in its test module. The three Win32 calls are covered by this repository's first two
> `#[cfg(windows)]` tests and by `cargo run -p kw-security --example dacl_probe`, and by nothing
> unattended. KNEG-111 tracks that gap. See the 2026-08-31 KNEG-34 entry below and
> `docs/BENCH_CHECK.md` finding 2.
>
> **The DACL is now measured on the live file**, not only on test directories: after a normal
> application start, `dacl_probe` reports `SE_DACL_PROTECTED` set and exactly three access entries,
> none inherited. `Jobbdator\CodexSandboxUsers` is gone from the real token file.
>
> **Three smaller items recorded rather than chased.** The `kw-storage` test that failed once and
> passed on re-run during Task 8 was **found on 2026-08-31**:
> `import::tests::no_plaintext_token_appears_anywhere_in_the_target_file`, twice in four hundred
> runs and only under eight threads. The assertion that fired was the positive control — meaning
> the two "no plaintext token here" assertions beside it had just passed against a database file
> whose rows were still in the write-ahead log. Fixed by reading every file the database is made
> of, which is also the stronger claim. See the KNEG-88 entry. The remaining two:
> `apps/desktop/src-tauri/src/meeting.rs` at ~1600+ lines, noted repeatedly and deliberately not
> restructured; and `SherpaModel` in the provider registry carrying no SHA-256 for its model
> archives, unlike `WhisperModel` and unlike the sidecar binaries in `config/sidecars.json`.

Maintained in place. If something here becomes false, edit it rather than adding a note.

### CI is paused — nothing is checking any of this

`.github/workflows/ci.yml` still contains every job, but its `push` and `pull_request` triggers
are commented out, so **no commit pushed to this repo is being checked right now**. Only manual
`workflow_dispatch` runs happen. Every claim in this file that reads "CI is green" is, until the
triggers come back, a claim about the last run before the pause.

Re-enabling is uncommenting two blocks at the top of that file. Do that before trusting a green
badge or before the next merge to `main`.

### Committed but not usable end to end

| Thing | State | What is missing |
| --- | --- | --- |
| Local transcription (Phase 2) | **Driven by hand** | The core leg is closed: 2026-08-28, in the real application window, dictation into Notepad produced correct text repeatedly on both `whisper-tiny` and Parakeet. Four smaller checks remain and are KNEG-47's — the same dictation in VS Code, the caret never leaving the target window, the `KEEP ME` clipboard round trip, and `Control+Super` mid-dictation. |
| Meeting transcription (Phase 4) | Four clients written, one confirmed | OpenAI Realtime has met a live server; AssemblyAI, Deepgram and Corti are *believed* wire shapes with no key on any machine this project has touched (KNEG-53, KNEG-55-57). Tinfoil is declined on principle, not missing. A dead provider is reported at Stop *and* mid-meeting, with the recording deliberately left running (KNEG-79, KNEG-95). |
| Enterprise reasoning (Bedrock, Azure, Vertex) | `ShapeNotImplemented` | The three request shapes. They refuse honestly rather than falling back, and none has made a live call (KNEG-54). |
| `storage::import::from_openwhispr` | Reachable | Nothing. `Notes > Upload` exposes it (KNEG-71), and it now refuses a path no file dialog returned (KNEG-80). Nobody has imported a real OpenWhispr database by hand (KNEG-90). |
| Local API and CLI (Phase 6) | Reachable, exercised by hand | 2026-08-25: the compiled CLI read history out of a running application. The discovery file's permissions are re-read on every status request rather than remembered (KNEG-110), and on Windows carry an explicit protected DACL verified on the live file (KNEG-34). |
| Calendars (Phase 5) | Built, never connected | `calendar-core`, loopback PKCE, `Integrations > Calendar`. No calendar code has answered a real request (KNEG-82); Apple Calendar is described and unimplemented (KNEG-81). |
| MCP and the integration screens (Phase 7) | Built, never connected | Thirteen tools over stdio, plus both screens and the agent skills. No MCP client has ever driven it (KNEG-91). |
| Media import (Phase 8) | Reachable | Subtitle export is verified end to end against the real server. Diarization and semantic search are not started (KNEG-73). Nobody has pressed *Import* on a real file (KNEG-90). |
| Release (Phase 9) | Never run | The workflow exists, has never been dispatched, and no signing keypair exists (KNEG-43). |
| Agent features (Phase 10) | Not started | Chat agent, voice agent, translation (KNEG-74). There is no chat screen; the local LLM's one consumer is dictation cleanup. |

### Claims that are unverified rather than verified

These are the places where CI going green means "it compiles", not "it works". Each is honest in
`docs/PLATFORM_SUPPORT.md`; the risk is a future session reading a passing build as proof.

- **Nothing macOS-native has ever run.** No macOS hardware, no macOS target installed. The
  non-activating panel, EventKit, and CoreAudio process taps are the three hardest native pieces
  and all three are macOS-only.
- **Windows capture exclusion has never been tested against a real screen recorder.** The
  read-back proves the flag is set on the window. It does not prove any given recorder honours
  it, and no in-process API reports that. Confirming it needs a second machine or a capture tool.
- **Linux has never been run at all**, and several capabilities will legitimately report
  `unsupported` there (content protection is a no-op; Wayland restricts global shortcuts and
  synthetic input).
- **macOS and Linux CI ran for the first time on the Phases 1–4 push.** If either is red, it is
  a first-run failure, not a regression.

### The general lessons, stated once

- **A pin is only a pin if it was measured.** Every digest in `config/sidecars.json` came from
  downloading the asset and hashing it, not from a release note. The catalogue is compiled in with
  `include_str!` for the same reason — a digest in a file the user can edit is not a pin.
- **Test the contract against the real thing once, and everything else against fixtures.** The
  ignored tests (`real_install`, `local_dictation`) exist because a fixture cannot tell you whether
  the pinned digest matches what the host serves, whether the redirect chain is followable, or
  whether the arguments are ones this build accepts. Those are exactly what breaks when an upstream
  release is re-tagged.

- **A denylist that matches nothing passes trivially.** "The tree is clean" and "the patterns
  work" are different claims and only the second one matters. This is why
  `tools/domain-scanner/scan.test.mjs` asserts the shipped patterns still catch real vendor
  strings — a scan result alone cannot tell you the scanner is alive.
- **A frontend type that declares a field the backend never sends renders a fiction, not an
  error.** TypeScript reads the missing field as `undefined` and any UI built on it displays
  something confident and false. `apps/desktop/src/lib/ipc.ts` carries a header comment saying
  so, because the compiler will not.
- **A purpose is a property of the destination, not the feature.** Getting that backwards made
  local model servers unreachable and produced an error message that blamed the wrong thing.
- **Prefer deriving an allowlist over writing one.** The registry-derived cloud-provider set and
  the client-derived meeting allowlist each fix a class of bug rather than an instance.
- **A development build is two processes, and instructions that name one are wrong.** `cargo run`
  starts the Rust half; the window then loads from the Vite dev server in `tauri.conf.json` and,
  with nothing serving it, shows the webview's own "can't reach this page" — while the log reports
  a flawless startup. Both halves were honest and the pair was undiagnosable. A debug build now
  probes `devUrl` at startup and names the fix in the log, which is the general shape: when a
  component cannot see the failure it causes, make the component that *can* see it say so.

### Open decisions

Unchanged from the Phases 1–4 entry, restated here so they are not buried:

- ~~**whisper.cpp source**~~ — **settled.** `ggml-org/whisper.cpp` at tag `b4938`, pinned in
  `config/sidecars.json`. The worry that upstream might not publish usable binaries turned out to
  apply only to macOS: the `b*` tags carry `whisper-bin-x64.zip` for Windows and
  `whisper-bin-ubuntu-{x64,arm64}.tar.gz` for Linux, all containing a real `whisper-server`, and
  those three are now installed and verified from upstream with no vendor dependency at all. The
  tagged `v*` releases carry source only, which is what made this look impossible at first.
  macOS gets an xcframework — a library for embedding in an Xcode project, not a server — so it
  is recorded as `NotPublished` with a reason and needs a macOS CI job of our own.
- ~~**ffmpeg and yt-dlp sources**~~ — **settled.** `yt-dlp/yt-dlp` at tag `2026.08.19` (GitHub
  marks the release `immutable` itself) ships a bare executable on Windows, Linux x86_64, Linux
  aarch64 and — one universal2 binary covering both — macOS, so all five targets are pinned with
  `archive: "bare"`. `BtbN/FFmpeg-Builds` at `autobuild-2026-07-31-14-10` covers Windows and Linux
  only; per its own README it never builds macOS at all, under any tag, so that gap is not a
  missing pin to chase but recorded `NotPublished`, the same shape as whisper-server's macOS gap
  above. That release was chosen over a more recent daily autobuild deliberately: BtbN prunes
  daily builds after 14 days and keeps only the last build of each month for two years, so a dated
  tag alone was not enough here — it also had to be one their own retention policy will not delete
  out from under the pin. The `lgpl` (not `gpl`) variant is used because this project stays wary
  of GPL entanglement (see `docs/UPSTREAM_AUDIT.md`'s reasoning about Pluely) and decode-to-WAV
  transcoding never calls the GPL-only encoders (`libx264`, `libx265`) that variant lacks.
- ~~**AEC helper and the five Windows native helpers**~~ — **settled**, see
  [PORTING_MATRIX.md](PORTING_MATRIX.md). Two corrections came out of looking properly. First,
  this tree never fetched any of them; the claim that they "still come from OpenWhispr release
  assets" described *upstream's* arrangement, not ours. Second, they do not block meetings —
  system audio does, and echo cancellation only matters on speakers.

  The five Windows helpers are reimplemented in Rust rather than rebuilt and published. One
  already was: `windows-fast-paste.c` is `platform::window::send_ctrl_v`. Only the AEC helper
  stays a binary we build, because it wraps WebRTC's audio processing module.

  Worth recording for whoever writes that build: **the two third-party inputs cannot be pinned the
  same way.** Abseil's GitHub archive is byte-stable and takes an ordinary sha256. The webrtc-apm
  tarball is generated per request by googlesource — two downloads seconds apart differed in
  length and digest — so it needs a content digest over the extracted tree, which *is* stable
  across downloads. Pinning that URL by archive hash would fail on every build, and a pin that
  always fails gets deleted by the first person it inconveniences. Both measured values are in the
  porting matrix. Upstream pins both by commit and verifies neither.
- ~~**`nircmd`**~~ — **dropped**, as leaned towards: third-tier Windows paste fallback only, and
  `SendInput` works.

---

## 2026-09-01 (KNEG-84) — the documentation was wrong, and only a probe could say so

The sixth meeting client, and the only one whose wire format was **measured**
rather than read. Worth recording is not the client — it is four hundred lines in
the shape of the five beside it — but what measuring bought that reading could not.

### One documented fact was false, and it was the load-bearing one

`sherpa-onnx-online-websocket-client --help` says *"16-bit samples"*. The wire
carries **`f32` little-endian**. As `f32` the values are speech inside ±1; as
`i16` they are noise.

A client written from that sentence would connect, complete a handshake, send
every frame successfully and transcribe nothing — with no error anywhere, because
this server does not report one. It would have looked like a model problem, or a
microphone problem, or a "streaming does not work on this machine" problem. The
cost of *not* probing was not a day of work; it was a defect with no symptom
pointing at its cause.

The probe was cheap for the same reason it was decisive: the reference client
and the real server were both already on disk, so the measurement was two
processes talking to each other with our code listening in between.

### The control came first, and it is why the rest is evidence

Reference client against real server, reproducing the model's own
`test_wavs/trans.txt`. Then our client against the real server, producing the
same transcript. Only then was the fault run meaningful. Without the first step,
"our client got nothing back" and "this model gets nothing back" are the same
observation.

### The finding that shaped the design was a negative one

`--fault` sent an unparseable TEXT frame and a three-byte BINARY frame before
the audio. **The server answered neither, and the transcript came back
byte-identical to the clean run** — the odd-length frame did not even knock the
sample stream out of alignment.

So this provider has no error channel. That is the exact inverse of the four
cloud clients: `fault.rs` exists because those four read a provider-reported
error and carried on, and here there is nothing to read. The consequences are
concrete rather than philosophical:

- `Incoming` has **no `Failed` variant**. Adding one nothing can produce would
  suggest a channel exists.
- `SessionFault` for this client can only ever be `Transport` or `Unreadable`.
- There is a test named `no_error_frame_is_reachable_from_this_client` whose job
  is to be **deleted deliberately** if sherpa ever grows an error frame — rather
  than a `Failed` arm quietly never firing, which is how a dead branch outlives
  the fact that created it.

A negative measurement is easy to leave unrecorded because it produces no
output. This one decided more of the design than any positive one.

### `text` is cumulative, and appending would have been silently plausible

Every update repeats the whole segment so far. A client that appended would
produce *"After After early After early nightfall"* — wrong in a way that reads as
a model artefact rather than a client bug, especially to somebody who has heard
a streaming recogniser revise itself. `Segments` is that rule and nothing else,
and it is tested against the recorded thirteen-update sequence rather than
against a live model.

The socket half therefore **mutates** its event vector rather than only appending
to it, which no other client here does. Each reader holds the index of its own
current segment; indices stay valid because nothing is ever removed.

### Two mutations in my own harness were no-ops, and the harness said so

Recorded because the lesson keeps recurring. I wrote ten mutations against the
rules; eight were caught and two "passed", which looked like two weak tests. Both
were actually **non-mutations**:

- `split_off(taken)` — `split_off(taken.min(len))`. `taken` can never exceed
  `len`, so `.min` changed nothing.
- `chunks_exact(2)` — `chunks(2)`. The `try_from` inside the `filter_map`
  already rejects a one-byte chunk, so the odd trailing byte is dropped either
  way. Two independent guards, and no single-line edit defeats both.

The second is the more interesting: the test was fine and the *code* was
belt-and-braces. Replacing the whole expression with one that pads did fail it.
Both are now noted in `samples`' own doc comment, because "this test survived a
mutation" is worth nothing until you know whether the mutation was real.

### Deliberately not done

**No reconnect.** No session limit, no token rotation, no billing clock — the
server is a child process. `capabilities.reconnect` is `false` rather than a
recovery path that does not exist.

**`language_detection: false`.** The one capability that differs from `local` in
a way a user feels. A sherpa online model is built for one language and there is
no per-request detection the way `whisper-server` has; declaring it true would
make `validate` accept a session with no language and quietly transcribe English
whatever was spoken.

**The server is not started at launch, and not by the settings click either.**
630 MB of model for a user who never records a meeting, or for one who is only
choosing a model, is a cost neither action advertises. `MeetingRuntime::provider`
starts it when a meeting begins and returns the same port if it is already up.

**`sherpa-stream`'s targets are `not-published`.** The binary ships inside the
offline CLI's archive and is already extracted beside it, so there is nothing to
download. Marking the targets published would have described an install path
this code does not use and cost 24 MB a second time. What the entry supplies is
the two things a path cannot: a port range, and the command fragment that lets
the reaper prove a leftover server is ours — `sherpa`'s own fragment
(`sherpa-onnx-offline`) does not match `sherpa-onnx-online-websocket-server`, so
without an entry an orphaned server would hold its port until the machine
restarted.

**`not_offered` was deleted rather than reworded.** It returned a sentence
saying streaming models could not be used because no client existed. That
stopped being true, and a check that can no longer fire is a check that stops
being read.

### Driven the same day, and the distinction held all the way to the end

A real meeting was recorded through it and the words arrived as spoken. So the
feature works — and the thing worth recording is that **every claim between
"the wire is measured" and "a person used it" turned out to be a separate claim,
and one of them was false.**

The wire was measured. The rules were tested and mutated. The integration test
drove the shipped client against the real server. All green, and the first
person to try it still could not start a meeting: the language caveat pointed at
the field *below* it while rendering underneath that field, `corti-realtime` had
carried the same flag for days without anyone reaching a screen where the
sentence rendered, and the wrong one of the two Nemotron models sorts first and
refuses to install. **None of those is reachable from any test in this
repository.**

The same shape as KNEG-94 earlier the same day, and not a coincidence: both were
finished the moment the measurement existed, and finishing the measurement is
not finishing the feature.

**Still not covered by that run:** *Stop and transcribe*, and the saved note.
The live transcript and the saved one come from different passes, so words on
screen say nothing about the note. And the fault path — the client raises the
error, but nobody has watched it reach `MeetingOutcome::Failed` rather than
being flattened into an empty transcript somewhere above.

---

## 2026-09-01 (KNEG-94) — a defect went looking for a capability, not the other way round

Teams calls are now detected, by the microphone the client holds rather than by a process name.
The code is small. What is worth recording is the sequence of judgement calls, because four of them
were about **what a measurement is worth** rather than about what to build.

### The prediction the plan made was backwards, and that is the interesting part

`COMPLETION_PLAN.md` chunk H said the mic listener had no consumer and that a capability with no
consumer is scaffolding this workspace does not build ahead of time. That reasoning was sound and
the conclusion was wrong, in a way worth naming: it assumed capabilities wait for features. What
actually happened is that a **defect nobody could fix any other way went looking for a capability**.
Teams had been unofferable for three weeks; three separate measurements said no process name would
ever fix it. That is a much stronger pull than a feature request, and the chunk had no way to
express it.

Kept in the doc rather than quietly corrected, because "no consumer yet" and "no consumer, and none
will come" read identically in a backlog and only one of them is safe to act on.

### The control was made cross-process on purpose, and the easy version proves less

`mic_users_probe` could have opened the microphone itself and checked it appeared. That was the
first design. It was replaced with a second process launched via `--hold`, because an audio session
belongs to a **process** and the question the issue asks is whether this code can see a session
belonging to *somebody else*. That those are the same mechanism is true, and it is an argument —
and an argument is exactly what a measurement is supposed to replace.

### The first real run passed its control and was still unreadable

This is the one worth remembering. The probe printed `nothing is capturing` and `PASS` on the
control. Both true. Both useless: **nothing in the output said whether Teams had been open at the
time.** "Teams is running and not capturing" and "Teams is not installed" print the same words and
mean opposite things, and only one of them is the run the issue needed.

The same failure shape as KNEG-88's empty file, one level up. There, a negative assertion passed
against nothing and only a positive control noticed. Here the control existed and was passing —
the missing positive was one layer above it, about the *subject* of the measurement rather than the
apparatus. A control proves the instrument works. It does not prove the sample was in the machine.

Fixed by having the probe print what `meeting_detect::detect` itself sees running, reusing the real
detector rather than matching a name by hand: a second opinion computed beside the first is a second
implementation that can be right when the first is wrong.

### The microphone may strengthen a match and never create one

The rule that keeps this honest. A process holding a capture device that is not a known client and
not on the user's own list is **not reported at all**, no matter how loudly it is capturing. The
alternative — report anything holding the microphone — would mean this application's own
dictation offers to record itself, and would make "something is recording you" a claim a process
detector has no business making about a browser tab.

Tested, and the test was mutated: a version that lets the capture list invent a detection fails four
tests.

### Two signals, and refusing to flatten them into one boolean

`CptHost.exe` cannot exist unless a Zoom call is running. A held microphone is also Teams running
its own audio test, and a call the user placed that nobody joined. Both are `in_call`, and they are
not the same claim.

So `in_call` became a derived value and `CallSignal` carries which one fired, all the way to the
offer window, which now says *"Microsoft Teams is using your microphone — that is usually a call,
but a test call looks the same"* rather than asserting a meeting. The offer is the one place a wrong
detection costs the user something: a window over their work about a meeting they are not in. A user
told the wrong thing can only work out which signal was wrong if the window says which one fired.

### Sealing a field rather than trusting a convention

`in_call` and `signal` are two serialised fields carrying one fact, which is how they come to
disagree. Rather than a comment asking future callers to use the constructor, `in_call` is a
**private field** with an accessor and `Detected::new` is the only way to set it — serde and
`ts_rs` both still export it, so the screen is unaffected. The desktop crate's test helper stopped
compiling immediately, which is the proof it works.

The same argument `MeetingRuntime::live` already makes for deriving "stopping has not begun" from
the lock instead of keeping a second flag beside it.

### One process table, two readings

`survey()` takes the process list and the capture list from a single `sysinfo` refresh. Two
refreshes are two instants: a process that starts in between appears in one list and not the other,
and the resulting `Detected` names evidence that had already gone. Cheaper too, and the poll runs
every five seconds for as long as the application is open.

### What was deliberately not built

The **calendar cross-check** KNEG-94 named as a possibly cheaper alternative. It is complementary
rather than competing — it catches scheduled meetings and misses ad-hoc ones, which is the exact
inverse of what landed — and doing both in one pass would mean neither was measured on its own.

The **UIAutomation text monitor**, KNEG-63's other half, is now closed as declined rather than
deferred. No consumer, and unlike the microphone half no defect pointing at it either; a continuous
read of the focused element is a materially larger privacy surface than anything else here and there
is nothing on the other side of the scale. A decision should read as a decision, not as unfinished
work.

### Still not done, and it is not the same claim

**Nobody has watched the offer window appear during a real Teams call.** The signal is measured and
the wiring is tested. Those are two different claims and the second does not imply the first —
recorded in `BENCH_CHECK.md` item 6 rather than left implied by a closed issue.

---

## 2026-08-31 (KNEG-115) — measuring the cost without choosing the fix

KNEG-115 says its own fix *"belongs to the repo owner rather than being chosen in passing"*, because
it changes when the **default** hotkey fires. It also says what would inform that decision and had
not been done: *"whether that is noticed depends on how quickly the overlay appears and whether the
second press stops it — not measured, and worth measuring before deciding severity."*

That half needs nobody's hands. Five tests now drive the shipped `ChordWatcher` with the exact
key-down and key-up order a person produces:

```
Ctrl+Win+Left, once                       1 fire
Ctrl+Win+Left, twice                      2 fires
Ctrl+Win held, arrow tapped four times    1 fire
Ctrl held, Win tapped twice               2 fires
LeftControl+LeftSuper (a sided chord)     1 fire
```

### Two fires is the finding, not one

`dictation::on_hotkey` calls `DictationRuntime::toggle`. So an even number of fires is not a pair
that cancels out: **the first starts recording and the second stops it, transcribes what the
microphone heard in between, and puts the text at the cursor through a synthetic paste.**

A user on the default binding who switches virtual desktop, works for a while, and switches back has
recorded that interval and had a transcript of it typed into whatever had focus. The issue said "a
dictation starts on the way past". It is worse than that, and the difference is the difference
between an annoyance and something that types your speech into a document.

The toggle-and-paste half is read from `on_hotkey` and the inserter, not executed — the fire counts
are measured, the consequence of a fire is traced. Both are labelled that way where they are
written.

### What bounds it, in both directions

Holding Ctrl and Win while cycling four desktops is **one** toggle, not four: rule 2 refuses while a
non-modifier is held, and the arrow going back down brings one with it. But releasing and
re-pressing either modifier re-arms, so Control held with the Windows key tapped twice **is** two
toggles.

And a sided chord is no escape from this one. It is from AltGr, which is why KNEG-37 and this are
separate issues — `LeftControl+LeftSuper` is a prefix of `Ctrl+Win+Left` exactly as the unsided
chord is.

### The fix is still not chosen, and the exposure is now disclosed

All three options in the issue cost something real, and the one it calls honest — fire on release
— adds the hold time to every dictation start, which is the latency `ModifierOnly` exists to
avoid. That is a trade for the person who lives with the default, and choosing it in passing would
be deciding the behaviour of the shipped default on the way past — which is, with some irony,
exactly the failure being warned about.

So `hotkey::prefix_cautions` warns and changes nothing, which is the pattern KNEG-37 established for
AltGr and needs no decision at all. On screen:

> Control+Super fires as soon as both keys are down, so it also goes off on the way into any longer
> shortcut that starts with them. On this platform that includes Ctrl+Win+Left, which switches
> virtual desktop. Dictation is a toggle, so two of those record everything in between and put the
> transcript wherever the cursor is.

Which chord collides with which keystroke is asked of the **real matcher**, exactly as
`altgr_collision` is, for the same reason: a predicate reasoning about modifier names would be free
to drift from the behaviour in precisely the cases nobody wrote a test for.

**Every modifier-only chord gets a caution, not only the five with a named example.**
`COMMON_PREFIXES` cannot be exhaustive — every two-modifier chord is a prefix of something — and a
warning that appeared only for the listed keystrokes would read as a clean bill of health for the
sixth. `Alt+Super` gets the general sentence and no invented example.

**Only the handled action is told what it costs.** Four of the five actions have no handler;
telling one of those that it records and pastes would invent a consequence it does not have.

### Two cautions, kept apart

`Control+Alt` on a Nordic layout now produces two sentences, and a test that asserted *exactly one*
had to be re-scoped rather than relaxed. They are different facts: AltGr depends on the layout,
being a prefix depends only on the chord having two keys, and either holds without the other. On a
US layout the AltGr one disappears and the prefix one stays — a US keyboard cannot press AltGr and
can certainly press Ctrl+Alt+Delete.

That also closes a loop worth noticing. KNEG-37's caution now suggests a sided chord as its remedy,
and KNEG-35 made sided chords typable. Someone who follows that advice gets a chord that is still a
prefix, and `the_altgr_remedy_is_not_a_remedy_for_this` is the test that says so out loud rather
than leaving them to find out.

---

## 2026-08-31 (KNEG-117) — the brute force was the weaker half, and the arithmetic says so

KNEG-88 found one intermittent test in `kw-storage`, and its real content was that a credential
check had been passing against an empty file. The obvious follow-up was to run the same hunt over
the whole workspace.

**Run: 40 rounds, 33 binaries, 1,320 runs, every thread count, control passed, nothing found.**

### What that is worth, stated rather than rounded up

Not much, and the arithmetic is why. The `kw-storage` flake fired **twice in four hundred runs of
that binary** — about 0.5% per run. Forty rounds is forty runs of each binary, so the chance of
catching something at that rate is about **18%**. A clean result at this depth is closer to "not
looked at properly" than to "clean", and matching KNEG-88's confidence across the workspace would
be roughly four hundred rounds — several hours of a machine doing nothing else.

Worth writing down because the temptation is to read 1,320 runs as a big number. It is a big number
of runs and a small number of *rounds*, and rounds are the unit that matters here.

### So the second half was a search rather than a loop

The defect KNEG-88 found has a shape: **an assertion that something is absent, checked against a
resource that may not have been fully written.** That is greppable, and unlike the loop it does not
depend on catching a rare event.

Seventeen candidate sites across the workspace; ten had a positive claim beside the negative and
were fine. Of the remaining seven, five were false positives of the search (a multi-line
`assert_eq!` the pattern did not see, or a test whose whole subject is that nothing happened). Two
were real and both are now hardened:

**`sherpa::the_staged_wav_is_cleaned_up_even_when_the_process_cannot_start`.** It asserted no file
matching `sherpa-input-` remained. That is trivially true of a run that never staged one — so a
`transcribe` failing before the write, or a rename of the prefix, would have left it green while
proving nothing. It was not vacuous today, which is exactly the problem: nothing would have said so
when it became vacuous. It is a claim about raw audio not being left on disk, which is worth more
than most. The control is the scratch directory, which now does not exist beforehand, so
`transcribe` creating it proves the function reached `create_dir_all` — and the WAV write is the
next statement. Mutating `transcribe` to fail before staging turns it red.

**`discovery::no_temporary_file_is_left_behind`.** "Nothing else is in the directory" is also true
of an empty directory. One line asserting the discovery file is there turns it from a tautology into
an observation.

### The lesson, in a form that survives

A negative assertion is only as good as the positive one beside it. Neither of these was broken;
both were one silent change away from being meaningless, and nothing in the tree would have said so.
The search that finds them takes minutes. The loop that finds them takes hours and mostly does not.

`tools/flake-hunt/hunt.mjs` also learnt something in the process: its first workspace run reported
`generated_ipc_types_match_the_rust_types` failing nine times in thirty, which was a Rust type and
its generated TypeScript being edited while the hunt ran. A false finding that names a real test is
worse than an ordinary one, so a run whose binaries changed underneath it is now reported as **void**
rather than as a result.

---

## 2026-08-31 (KNEG-38) — the ambiguity is real, so it is named rather than resolved

WASAPI loopback from an output device with nothing playing delivers **no callback at all**, not a
stream of zeros. So *"nobody else spoke"* and *"the loopback never worked"* produce byte-for-byte
the same nothing, and both arrived on screen as `MeetingOutcome::NothingTranscribed` — along with a
third case that is neither of them.

### What could be separated, and what could not

Nothing this side can separate the first two. Any code claiming to would be inventing evidence, and
the label already said so honestly: *"no speech was recognised, or the provider returned nothing"*.

The third case was hiding in there and **can** be separated: audio was captured, it was audible, and
none of it was transcribed. That is not a quiet meeting. It was being reported as one.

So `Delivered` is a four-way answer per capture — `NotCaptured`, `Nothing`, `Silence`, `Audio` —
folded in the recorder's drain loop as the blocks go past. The outcome carries one for the
microphone and one for the system audio, and the sentence branches on them:

* **either side heard something** — *"and none of it produced a transcript — so this is a
  transcription problem rather than a quiet meeting."* First in the ordering, because it is the only
  branch that should send anybody looking for a cause.
* **everything delivered was digital silence** — nothing was said within reach.
* **nothing was delivered at all** — *"this is what a meeting nobody spoke in looks like, and also
  what a capture that did not work looks like. They cannot be told apart from here."*

The last one is the honest version of the thing this issue asked for. The bench check's wording was
*"zero samples must read as nobody else spoke rather than a failure"*, and reading it that way would
be a guess in the other direction. What the screen now does is say which of the two it is looking at
— having ruled out the third — and explain why they look alike.

### An empty block is not a block of silence, and that is the whole fold

`Delivered::plus` distinguishes an empty slice from a slice of zeros. Collapsing them — the obvious
thing, since both contain no sound — would destroy the distinction before anything downstream could
see it. It is also the first thing the tests check.

The fold is monotone: `Nothing` to `Silence` to `Audio`, never back. A meeting that goes quiet for
its last half hour has still heard something, and the question is whether anything was ever audible
rather than whether the end was.

And it folds the **per-source** blocks, not the mix. The mix is the sum, where a silent system feed
under a live microphone is invisible, and telling those apart is the point.

### `NotCaptured` is not `Nothing`

A meeting recorded without system audio has a reason on `Sources` and no defect behind it. Starting
that tally at `NotCaptured` rather than `Nothing` is what stops the screen reporting a capture that
was never asked for as one that fell silent.

### Still on the by-hand list

With the speakers muted, confirm the sentence a real meeting produces. Everything above is measured
in unit tests against synthetic blocks; no meeting has been recorded since it landed.

---

## 2026-08-31 (KNEG-116) — asking the thread instead of inferring from its silence, and a state name that stops lying

`pump` returns `Ok` when its audio channel closes. At Stop that is exactly right. Reaching it while
a session is still held means the recording thread went without being asked — `record` returns on
`Finish`, on `Cancel` and on its command channel disconnecting, and all three run only after the
session has been taken. So that arm meant a panicked recorder, and the state reported was
`RecordingWithoutTranscript`, whose first word was the one thing that had definitely stopped.

KNEG-95 chose to report it anyway, on the grounds that it was strictly less wrong than the plain
`Recording` it replaced, and filed the rest.

### The fix is to stop inferring

A closed channel and a dead thread look identical from outside, and they are not the same fact. So
the thread says which, itself: a `Drop` guard reading `std::thread::panicking()`, writing
`RecorderEnd::OnPurpose` or `RecorderEnd::Panicked` into a `OnceLock` the runtime already holds.

A guard rather than a line at each return, because `record` has five exits and **a panic is not one
of them** — anything written at the returns would miss the only case worth naming.

**The drop order is load-bearing.** The guard is the first binding in `record` and `audio` is a
parameter; bindings drop in reverse order of declaration, so the guard runs before the sender does.
The cell is therefore written before the pump can observe its channel closing. That is the same
ordering trick `Live::transcript_failure` already depends on, and it is why reading the two together
is meaningful rather than a race.

### A fifth state, now that there is something to distinguish

`MeetingState::Stalled`: nothing is being recorded and nothing asked for that. `is_running()` stays
true, deliberately — whatever was captured before the thread went is on disk, and a state that hid
Stop would throw the recording away in order to report that the recording stopped.

Named for what is true, which is the rule KNEG-95 set for its own state and the rule this one was
filed for breaking.

### The compiler could not catch what adding it broke

`tsc --noEmit` passed with `stalled` added to the union and both screens still asking
`state === "recording" || state === "recording-without-transcript"`. **Adding a member to a string
union does not break a `===` comparison.** So the notification window and `Settings > Meetings`
compiled cleanly and would both have answered *not recording* — offering "Start recording" for a
session that was still held, with a WAV still growing on disk. That is the precise defect KNEG-95
exists to fix, reintroduced by a state added to fix a neighbour of it.

`labels.meetingIsLive` is an exhaustive `switch` with a `never` in the default arm. A sixth state
now fails to compile there, rather than quietly losing a button somewhere else. `meetingStateLabel`
sits beside it for the same reason.

That is worth separating from the rest: the type system was consulted, said yes, and was wrong to.
It was not asked the question it can answer until the switch existed to ask it.

### Verified

`the_recorder_guard_can_tell_a_panic_from_a_return` runs a real thread into a real panic rather than
reasoning about `std::thread::panicking()`, which is easy to write backwards and impossible to see
is backwards. The rest is pure: three distinct sentences, asserted distinct from each other, because
a formatter that collapsed two of them would pass every individual assertion.

### Still not observed

No recorder thread has ever been seen to panic in this project. The case is reached by reading
`record`'s exits. What changed is that it is now reached by the thread saying so, and that the
screen says the true thing when it does.

---

## 2026-08-31 (KNEG-96) — between "died during startup" and "we killed it" there was no observer at all

The local whisper server went away twice in one afternoon on 2026-08-28, unused, and the record
could not say whether it had crashed, been killed, or exited cleanly. `start` reports a child that
dies *during* startup, and `stop` ends one deliberately. Between those two, nothing was watching:
the only trace of a death was a later connection refused with no explanation attached to it.

This does not reproduce the exit. It makes the next one explain itself: a watcher task polls the
child every five seconds and, when it finds it gone without `stop` having been called, logs the
exit status, how long it had been running, and the tail of its diagnostics — then clears the PID
record.

### It deliberately does not restart anything

The transcription and import paths already health-check the recorded port and revive the remembered
model themselves (`2fe5825`). A second thing that restarted servers would race them. What was
missing was never recovery; it was an explanation, and conflating the two would have produced a
supervisor nobody asked for.

### stdout is still thrown away, and that is the reason the tail is safe to log

`start` sends stdout to `Stdio::null()` precisely because it can carry recognised text. stderr
carries the loader and model diagnostics — which is exactly what a post-mortem needs, and is
already shown to the user by `ProcessError::ExitedDuringStartup`. The distinction was already made
in the code; this relies on it rather than re-deciding it.

### Two ways to get this wrong, both closed

The child handle moved behind an `Arc<tokio::sync::Mutex<..>>`, because `stop` needs it too and a
watcher parked in `wait()` would hold it for the life of the process. Polling instead, at five
seconds: this is a post-mortem, not a supervisor, and nothing acts on the answer.

And `stop` sets a flag **before** it kills. Without it, every deliberate model switch would log an
unexplained death — which is precisely the noise that makes a real one easy to scroll past. The flag
is read again on the far side of `try_wait`, because `stop` can set it while the lock is held.

### What a test can see

A log line is not assertable, so the tests watch the PID record instead: same code path, same
trigger, and a record naming a dead process is independently the thing that makes a reused process
id look like a live server. Three mutations — removing the watcher, making it notice and say
nothing, and dropping the flag from `stop` — each caught by the test written for it.

### Still open underneath it

**Why the server exits.** That needs the next occurrence, and until one happens the two candidates
this issue names are still both live. What has changed is that the next occurrence will say which.

---

## 2026-08-31 (KNEG-88) — the flake was the control, and the control was the only honest thing in the test

One line in the carry-forward: *"a `kw-storage` test failed once and passed on re-run"*. The report
did not say **which** test, which is half of why answering it needed a session rather than a minute.

`tools/flake-hunt/hunt.mjs`, written for this and committed: build the test binaries, ask **cargo**
which they are, run a control, then run them repeatedly with the thread count cycling through 1, 2,
4, 8 and 16. Four hundred rounds. It named the test:

```
kw_storage-... :: import::tests::no_plaintext_token_appears_anywhere_in_the_target_file
  failed 2 time(s), at --test-threads=8
```

Two in four hundred, and only ever under eight threads — which is to say, only ever on a machine
busy enough to matter.

### Which assertion, and why that is the whole finding

Eighty more runs to catch the message. It was not either of the two security assertions. It was the
**positive control**:

```
panicked at crates/storage/src/import.rs:1035: the slot reference should be there
```

The test greps the bytes of `kw.db` for a refresh token and an access token and asserts neither is
there, then asserts a slot reference *is*. When the control fired, the main database file contained
**nothing** — the import had committed, `Db::close` had returned, and the rows were still in the
write-ahead log. So both "no plaintext token here" assertions had just passed against an empty file.

That is a false green in the exact place a false green is worst: a test whose whole purpose is to
prove a credential did not reach disk. And the only reason it was visible at all is that somebody
wrote the third assertion. **The flake was the control doing its job.**

### The fix is also the stronger claim

SQLite in WAL mode is three files, and a committed row can be in any of them. Greping only `kw.db`
was never what the test's name says: a refresh token sitting in `kw.db-wal` is exactly as leaked as
one in `kw.db`, and the old test would not have seen it. `every_database_file` reads the main file,
the `-wal` and the `-shm`. The negatives get stronger and the control becomes deterministic in one
change, because the row is now found wherever it actually is.

### Not left resting on a 0.5% event

A regression test that reproduces two runs in four hundred is not a regression test. Holding the
database open forces the same state every time — nothing has closed, so nothing has checkpointed —
and `a_row_that_is_only_in_the_wal_is_still_found` asserts the helper finds a row the main file does
not have. Its first assertion is the *premise*: if SQLite ever checkpoints eagerly enough to put the
row in the main file, the test says so rather than passing. Reverting the helper to the main file
alone fails it every time.

### `Db::close` was overstating itself

Its comment read *"Called on shutdown so the WAL is checkpointed rather than left for the next
launch to recover."* Measured: not always. SQLite checkpoints when the last connection closes and
skips it if it cannot take the lock. Nothing in the application depends on it, because the WAL is
recovered on the next open — but a test did, silently, and the comment now says which.

### What the hunter's own control caught

Its first version reported **100% failures with no output** under every thread count, including what
should have been the control. Each binary path carried a trailing carriage return — the producer
translated newlines, the consumer stripped only the line feed — so nothing was executed at all. A
harness that fails everything is measuring itself; the same mistake in the other direction would
have printed a clean result and been believed. The committed tool runs a control first and **refuses
to report clean unless it passed**.

And the detector has a control of its own: a deliberately unreliable test was planted in
`crates/storage/src/settings.rs`, the hunter run against it, and it named the test and the thread
counts it failed under — then removed. A flake hunter that has never caught a flake is a script
that prints "clean".

### The reporting gap, which was the fixable half of the original report

The tool parses libtest's own `test <name> ... FAILED` lines and reports the name, the count and the
thread counts. A non-zero exit with no named test — a crash, a binary that would not start — is
reported as exactly that rather than counted as clean.

---

## 2026-08-31 (KNEG-106) — the backpressure is the bound, and the first test for it was measuring the scheduler

`extract_tar_xz` filled a `Vec<u8>` with the entire decompressed tar before looking at one entry,
because `lzma_rs::xz_decompress` writes into a `Write` and offers no streaming `Read`. The
`max_bytes` ceiling counts bytes reaching `dest`, and the `Vec` was full before the first one got
there, so a `.tar.xz` was a memory bomb no ceiling could touch.

The fix is the option the issue called honest: run the decompressor on its own thread writing into
a `sync_channel`, and read from the other end. A full channel blocks the producer, so the bytes in
flight cannot exceed the channel's depth however large the archive decompresses to. **The
backpressure is the bound** — there is no separate check that could fall out of step with it.

### Two things it does not bound, both said out loud

`lzma-rs` allocates an LZMA2 dictionary whose size the xz header declares, inside the decompressor
and outside the channel. A few megabytes for any real archive, and not attacker-chosen here anyway,
since `extract` runs only after a mandatory SHA-256 matched. "Streaming" reads as "constant memory"
and it is constant *plus a header-declared dictionary*, so the comment says which.

And a decompression failure arriving after the consumer stops is not reported: dropping the reader
closes the channel and ends the producer. `extract_tar_gz` and `extract_tar_bz2` already have
exactly that property — their decoders are driven by the tar reader and stop when it does — so
this matches the module rather than adding a hole. A consumer that reads to the end always sees the
error, because it travels in-band as the last item rather than through a side channel.

### The first test was measuring the scheduler

It ran a producer and a consumer concurrently, counted how far the producer got ahead, and asserted
the gap stayed under the bound. It passed. Then the mutation harness widened the channel to a
hundred thousand blocks — **sixty-four gigabytes of queue** — and it passed again, because a
consumer that keeps up keeps the observed gap small whatever the queue can hold.

Two of three mutations sailed through. The replacement lets **nothing** read at all and asserts the
producer cannot finish a stream twice the bound, with a one-second timeout in the safe direction:
expiring is the pass, and only the producer *finishing* fails it, so a slow machine cannot cause a
red. An unbounded channel finishes it in milliseconds.

The second miss was subtler. Replacing `buf.chunks(BLOCK)` with `buf.chunks(usize::MAX)` — which
moves the unbounded buffer out of the `Vec` and into `BlockWriter`, where it is just as unbounded
and much harder to see — changed nothing observable, because a producer fed exactly one block at a
time behaves identically either way. It needed its own test writing a single oversized slice, not a
second assertion in the first one. Three mutations, three tests, each caught by the one written for
it.

The lesson is the one this repository keeps relearning in a new costume: a test that passes is not
evidence until something has made it fail.

---

## 2026-08-31 (KNEG-35) — one table, two readers, and a sentence with a hole in it

`keys.rs` has expressed `RightControl+Super` since the hook was written — the headline capability
of running a low-level hook at all, since no cross-platform shortcut API can see which side a
modifier is on. No accelerator a user could type reached it. `hotkey::MODIFIERS` had nine unsided
spellings, so `classify` read `RightControl` as *the key*:

```
     RightControl+Super -> Standard { accelerator: "RightControl+Super" }
RightControl+RightShift -> Invalid { reason: "it names 2 non-modifier keys; a shortcut may have one" }
```

The first went to the system register, the one mechanism that cannot tell the sides apart. The
second was refused with a message blaming the user's spelling for our own missing alias.

### One table, because two lists put both failures one edit away

The fix is not "add the sided spellings to `MODIFIERS`". Two lists of the same set drift, and they
drift in two directions with different symptoms: a spelling `classify` accepts and `Chord::parse`
does not becomes a shortcut the screen calls hookable and `KeyHook::install` then refuses; a
spelling `Chord::parse` accepts and `classify` does not is this issue.

So `keys::SIDED_MODIFIERS` is the one table, `Chord::parse` reads it, and `classify` reads it
through `keys::sided_modifier`. The unsided list stays separate and stays in `hotkey.rs`, because it
answers to something different — the shortcut plugin. That is why `Win` is absent from it and
present in the chord parser: the plugin rejects `Win`, and being stricter than the platform is a
different mistake from being looser.

`every_sided_spelling_the_chord_parser_takes_is_a_modifier_here` is what makes the shared table more
than decoration. A constant only one side actually consults looks identical from the outside.

### The third state nobody had a name for

Adding sided spellings as modifiers creates a case that did not exist before: `RightControl+D`. It
would have fallen through to `Standard` and been handed to the register, which cannot see the side —
strictly worse than the old behaviour, because the old behaviour at least refused.

Neither mechanism can take it. The register is the only thing that handles an ordinary key and
cannot see sides; the hook is the only thing that can see sides and `Chord::parse` refuses anything
that is not a modifier. So it is `Invalid`, with **both** ways out named — drop the key, or drop the
side — and a test asserting the classifier accepts each of them. A refusal that suggests something
the same field then rejects is the defect this issue mostly was.

The same reasoning removed a piece of advice: `explain()` told a user without the hook to *"add a
regular key, for example {accelerator}+Space"*. For a sided chord that produces an `Invalid`. It now
says there is no accelerator to fall back to, because there is not one.

### The caution can finally name a remedy, and it is checked rather than asserted

KNEG-37's AltGr warning deliberately named no remedy: a sided chord avoids AltGr completely, and
the settings field rejected every sided spelling. It accepts them now.

`keys::sided_escape` builds the suggestion from the chord that actually collided and then **passes
it back through `altgr_collision`** — the same matcher that produced the warning. AltGr being left
Control and right Alt specifically is a claim about when a chord fires, and this module's standing
rule is that such claims are answered by running the watcher rather than by reasoning about names.
If the candidate still collides, no remedy is named. Mutating `RightControl` to `LeftControl` in the
builder — the AltGr side — makes the re-check reject it and the test fail, which is what says the
re-check is load-bearing rather than ceremony.

### The hole in the sentence

Both AltGr cautions had shipped like this:

```
This layout types '@'                      with Control and Alt together, and Windows sends
```

Line continuations in the Rust string literals had lost their backslashes in an earlier edit, so the
source indentation became twenty spaces in the middle of a sentence on screen. `cargo fmt` does not
look inside a string literal, and the tests asserted `reason.contains("AltGr")`, so nothing caught
it — it was found by printing the text and reading it, which is the only thing that ever finds
this class.

`no_caution_has_a_hole_where_a_line_continuation_should_be` asserts no caution reason contains a run
of two spaces. Cheap, and it fails the moment a continuation is lost again.

### Not closed by this

A person typing a sided chord into `Dictation > Shortcuts` and watching it fire. The hook path is
measured — `keyhook_probe` injects `RightControl+RightShift` and sees two fires for two presses —
and the classifier is now measured, but the two have never been driven end to end from the screen.
That is KNEG-47's territory.

---

## 2026-08-31 (KNEG-114) — the control the gate asked for, and the hole the gate turned out to have

`meetings.detect` holds the allow and deny lists deciding which running applications count as a
meeting. Every link existed except the last one: the key is read by `Detector`, a command writes it,
a getter reads it, both are registered, both have IPC wrappers, both have dev fixtures — and no
screen called either. The lists were settable only by editing SQLite by hand. The fourth instance
of that pattern, and the first found by `tools/settings-audit/scan.mjs` rather than by a person
reading a screen.

The control is two textareas in the Detection section of `Settings > Meetings`, under the list of
running applications — deliberately, because the name to type is the one each detection there
reports. `parseKeyterms` became `onePerLine` and now serves all three fields, since keyterms and
both lists are the same thing on screen and are normalised the same way in Rust.

### The mutation the fix was supposed to fail, and did not

The habit is to break the fix and watch the gate go red. Replacing the call and leaving the import
in place kept it **green**.

The check was `new RegExp(`\\b${wrapper}\\b`).test(component)` against the whole file, so an
import satisfied it. Every one of the nineteen reachable keys had been passing on a test that a
screen could satisfy by importing a function and never calling it. Nothing was wrong in the tree —
the finding is that the tree being clean had stopped being evidence, which is the exact failure
`scan.test.mjs`'s own header warns about two paragraphs before this code.

### The obvious tightening was wrong, and the tree is what said so

Requiring an open parenthesis after the name separates `setMeetingDetectLists({` from
`setMeetingDetectLists,` in an import list. It also reported `localApi.enabled` as having no way in,
because `IntegrationsApi.tsx` writes:

```tsx
void guard(api.running ? stopLocalApi : startLocalApi);
```

which is a real call by reference with no parenthesis anywhere near the name. **A gate that invents
an unreachable setting is worse than one that misses a reachable one** — the first sends someone
to build a control that already exists, and erodes trust in every other line the gate prints.

So the import statements are stripped and the mention is then taken at face value. It cannot tell a
name in a comment from a name in use; that is written down at the function rather than left to be
rediscovered.

Two things worth separating: I predicted in the first draft of that doc comment that *"no declared
surface passes a wrapper by reference"*, and the very next run refuted it. The prediction was cheap
and running it was cheap; writing the prediction down is what made the refutation visible instead of
silently shipping a false accusation.

### One test was passing for a reason that had just expired

`an unreachable key must say why` deleted the note from `meetings.detect` — the only key
declared unreachable. Making it reachable would have left that test green forever without exercising
anything. It now sets `reachable: false` on a key itself rather than borrowing whichever one the
backlog happens to hold, so it survives the backlog emptying.

### Not closed by this

Whether the command *works* when called. It has still never been driven — not by a screen, not by
hand. The round trip through `kw_storage::settings` is the same one every other meeting setting
uses, and `kw_platform::meeting_detect::detect` has tests for allowing, blocking, duplicate spellings
and blank entries; what has not happened is a person typing a name into the box and watching the
offer appear. That is part of KNEG-51.

---

## 2026-08-31 (KNEG-65) — a rule carried from upstream, measured for the first time, and three quarters wrong

`kw_storage::paths` has relocated the model cache away from the user's own directory since the port,
on a rule written as *"non-ASCII **or space**"* and applied to the binaries' own directory as well
as to the files they read. Nobody had ever run it. `PLATFORM_SUPPORT.md` said so in as many words:
*"the fallback has not been exercised here."*

It cost about twenty minutes to find out, and the answer is much narrower:

```text
                                  a space        a non-ASCII character
the model file                    loads          FAILS
the audio file                    transcribes    FAILS
the binary's own directory        runs           runs
```

**A space breaks nothing, anywhere.** `whisper-server` loads `ggml-tiny.bin` out of `model space/`
and `sherpa-onnx-offline` transcribes a wav from `ljud plain/`. So an ordinary Windows account named
`Ann Marie` was being sent to a machine-wide `%ProgramData%` cache — models shared across every
account on the machine, and a "fell back" notice on her screen — to avoid a failure that does not
happen.

**The binaries' own location breaks nothing either.** This was the load-bearing half of the old
comment: *"`whisper-server` loads its sibling `ggml` libraries relative to its own directory, and
the native code chokes."* It does not choke. It printed
`load_backend: loaded CPU backend from ...\Håkan Test\bin\ggml-cpu-cascadelake.dll` and carried on.
`sherpa-onnx-offline`, `llama-server` and `ffmpeg` all do the same. Four out of four, so `sidecar_dir`
stopped relocating at all — which is not only tidiness: `%ProgramData%` is a directory an
unprivileged account may not be able to write to, so the old fallback could turn a cosmetic problem
into an install failure.

**What is real** is a file the ASR binaries open by path. Both exit 127. `whisper-server` says
nothing at all after `loading model from ...`; `sherpa-onnx-offline` says `Errors in config!` for a
model and `Failed to read '...'` for audio. So the fallback stays for `model_cache_dir` and
`audio_temp_dir`, on evidence rather than on inheritance.

### The decomposition was the whole method

Four confounded variables in one sentence: space against non-ASCII, and the binary's path against
the files it reads. Testing "a path with a space and a non-ASCII character in it", which is how the
claim was written and the obvious thing to try, reproduces the failure and teaches nothing — the
first run did exactly that and looked like a confirmation. Varying one at a time is what turned a
confirmed folk rule into three findings.

### The probe caught itself being wrong

`crates/storage/examples/native_path_probe.rs` is committed so this can be re-run, in the shape
`altgr_probe` already set. Its first version used `whisper-server --model X --help`, on the
assumption that the model is loaded before the usage text is printed. It is not, and the probe
reported `DIES` for every case **including the control** — which is the only reason the mistake was
visible at all. A probe with no control would have printed three plausible failures and been
believed.

The version that ships starts the server and polls the port, because the only honest evidence that a
model loaded is a server that answers.

### What is still not measured, and is now the only part left

**A real Windows account whose own name is non-ASCII.** Everything above used awkward directory
names under an ASCII profile, which exercises the binaries but not the relocation a profile name
triggers. That relocation — models landing in `%ProgramData%`, the screen saying so, a Parakeet
model installed there transcribing — still needs somebody with a `Håkan` account. It stays in
`BENCH_CHECK.md` as the remainder rather than being quietly closed with the rest.

Also unmeasured, and named rather than assumed away: no released build exists (KNEG-43), so nobody
has models in the old location. After a first release, narrowing this rule would move where models
are looked for and would need a migration; today it moves nothing.

---

## 2026-08-31 (KNEG-95) — the lock is the arbiter, and the state leads with the word that is still true

The last of the 2026-08-28 meeting silence. Both OpenAI Realtime sockets closed within milliseconds
of connecting; the application said **Recording**, kept the indicator up, captured both streams for
as long as it was left running, and said nothing. KNEG-79 fixed the end of that meeting — Stop now
reports the provider's own sentence. This is the middle of it.

The shape was the repository owner's to pick, and was picked: **keep recording, and change the
indicator to say the transcript died.** Not stop the recording, and not fall back to the whole-file
pass immediately. The argument is the one artefact that cannot be recreated: the WAV is what saved
the 28 August session, `record` already keeps writing when nothing is consuming the audio — on
purpose, with a comment saying why — and the saved note comes from the whole-file pass over that
file rather than from the live partials. Stopping the recording to report a dead provider would
destroy the valuable half in order to fix the misleading half.

### `MeetingState::RecordingWithoutTranscript`, and why the name matters

The first word is the one that is still true. In the enum, in the JSON on the wire, in a log line
and on the screen, a reader who sees only the first word is not misled: the microphone is open, the
file is growing, and the indicator the brief requires is still up. `TranscriptLost` would have been
shorter and would have invited exactly the wrong inference.

`MeetingState::is_running` came out of `emit_state` at the same time. The tray rule was
`state != MeetingState::Idle` written inline, which is correct and is also the kind of thing a
fourth variant walks past. Now there is one place to answer it and a test that asks all four.

### The lock is the arbiter, and that is the whole design

The hard part was never the state. It was deciding **which ending is worth announcing**, because the
pump ends on every path — including the ordinary one, where Stop closes the recorder and the pump
drains and returns.

The trap: a provider that faults on the *final* frame ends the pump during `finish_recording`, which
runs the whole-file pass and can take minutes on a long meeting. An announcement there would
overwrite `Finishing` with "the transcript died" and leave the user looking at it until the pass
returned. That is not a narrow race; it is a wide and ordinary one.

`live` is now an `Arc<Mutex<Option<Live>>>`, the pump's own task holds a handle to it, and
`announce_transcript_ended` decides and emits **while holding that lock**. `stop` and `cancel` take
the session out under the same lock, so *"there is still a session here"* is exactly *"stopping has
not begun"*. One fact, not a second flag that could drift out of step with it. Emitting after
releasing the lock would reopen the same window, which is why the emit is inside it.

**And a second race underneath it, found by asking what `stop` actually holds.** `stop` takes `Live`
out and then spends the whole-file pass *outside* the lock, and `start` refuses only while a session
is held — so a second meeting can begin while the first is still finishing. A late pump task would
then find *a* session, believe it to be its own, and mark a healthy new meeting as having lost its
transcript. `is_this_session` compares the fault cell with `Arc::ptr_eq` rather than trusting
presence. The cell is the session's identity because it is already per-session and already shared
with exactly the task that needs it; a counter would have been a second thing to keep in step.

### `pump.is_finished()`, narrowed to the one case the cell cannot cover

KNEG-95 named `status()` not consulting `pump.is_finished()` as part of the gap, and it is consulted
now — but the interesting part is what it is *for*. The announcement is written by the pump's own
task **before that task returns**, so the join handle cannot report finished first. "Finished, and
nothing announced" therefore means exactly one thing: the task panicked, or it was aborted. There is
no provider sentence for either, and the screen still must not claim a transcript is being produced.
Without that ordering the check would be redundant; with it, it is precise.

### One sentence, not two

`live_transcript_ended` is the only formatter. The indicator's sentence mid-meeting and the outcome's
sentence at Stop are the same string from the same function, and a test asserts the two call sites
agree rather than trusting that they were written to. A user who reads the indicator and then reads
the outcome is not told two stories about one failure.

Its `Ok` arm is the one worth reading twice. The pump returns `Ok` when the audio channel closes,
which at Stop is what is supposed to happen. Reaching it *while a session is still held* means the
channel closed with no Stop behind it, and `record` has no path that ends by itself — it returns on
`Finish`, on `Cancel`, and on the command channel disconnecting, all three of which run only after
`stop` or `cancel` has taken the session. So that arm means the recording thread panicked, and the
recording has stopped along with the transcription.

**That is the one case where the state is wrong in its first word, and it was reported anyway.** The
alternative was the silence this whole change exists to end, and the sentence — which is what a
person actually reads — said the true thing. Filed as **KNEG-116** (Low) rather than left in a
comment: a fifth state for a case reachable only by a panic, and untestable without one, was more
machinery than the fact justified — and that is a judgement somebody should be able to revisit.

**Revisited the same day, and it was the wrong call.** The fifth state was the smaller half; the
larger one was that the case had to be *inferred* at all. `MeetingState::Stalled` now exists, and
the recorder thread reports how it ended rather than the runtime guessing from a closed channel.
See the KNEG-116 entry.

### Where it shows

The indicator window keeps its red edge, its timer and its Stop button, and changes its second line
to *"live transcript stopped — still recording"* in amber. Amber rather than red for the same
reason `.caveat.warn` splits the mark from the prose: the red edge means **recording**, which is
still true and is the one indicator the brief requires. Painting the line red as well would say the
recording had failed, when the recording is the half that is working.

The provider's own sentence is too long for a 392x92 panel and goes to `Settings > Meetings`, which
has room for it, together with what happens next: recording continues, and Stop will still
transcribe the whole recording. The live-transcript panel there says its lines are the last ones
rather than the most recent ones, because "replaced as it is revised" stops being true the moment
nothing more is coming.

### What this did not do

**No provider was faulted to test it.** The unit tests cover the decisions — which state, whose
session, which sentence — and `announce_transcript_ended` itself is not among them, because it takes
an `AppHandle` and nothing outside a running application can construct one. The by-hand check is in
`BENCH_CHECK.md`: start a meeting on a provider configured wrongly and watch the indicator change
without the recording stopping.

Six mutations, each caught by exactly the test written for it:

```
live_state ignores a vanished pump          -> a_pump_that_vanished_without_a_word...
identity check becomes a presence check     -> a_late_ending_cannot_be_pinned_on_the_next_meeting
any session is my session                   -> a_late_ending_cannot_be_pinned_on_the_next_meeting
the tray rule forgets the new state         -> every_state_but_idle_is_a_running_session
stop formats its own sentence again         -> the_indicator_and_the_outcome_use_one_sentence
the new state serialises under another name -> the_states_serialise_as_the_frontend_reads_them
```

The `TranscriptSink` trait the KNEG-79 entry predicted this work would want was **not** needed, and
that entry has been corrected rather than left standing. The decisions that mattered turned out to
be expressible as free functions over plain values, which is the shape `pump_result`,
`close_recorder` and `failed_message` already had. `pump` still has no test coverage and still needs
none: it propagates its `Err` correctly, and what was wrong was what nobody did with it.

---

## 2026-08-31 (KNEG-37) — asking the matcher instead of describing it, and finding two more

On a Nordic, German or most other non-US layout, `@ $ € { [` are typed with AltGr, and **Windows has
no AltGr virtual key** — the keyboard layout synthesises left Control plus right Alt. So a
modifier-only chord of `Control+Alt` fires on every one of those keystrokes. Measured 2026-08-26 by
injecting exactly that pair and watching a dictation start.

Nothing downstream can separate the two, so this is not fixable at the hook. The fix is that
something says so, which is what was missing.

### The implementation choice that did all the work

The obvious way to decide whether an accelerator collides with AltGr is to ask whether it names
both a Control and an Alt. Two or three lines, and it is what the issue's own description suggests.

Instead, `keys::altgr_collision` **drives the real `ChordWatcher`** with the keys a real AltGr press
sends and reports whether it fired. The argument for it was drift: the matcher is the thing that
actually decides, several hundred times a second, and a separate predicate reasoning about modifier
names would be free to disagree with it — silently, in exactly the cases nobody wrote a test for.

It paid for itself immediately, twice.

**`Control+Shift` collides too.** Nobody predicted this and the name-based predicate would have
called it safe. AltGr's *left Control* arrives while Shift is already held, so the chord is complete
before the Alt is pressed. A user who binds `Control+Shift` — a far more ordinary choice than
`Control+Alt` — starts a dictation whenever they type a shifted AltGr character.

**And then the bigger one, which is not this issue.** Driving the same matcher with an ordinary
three-key shortcut, in the order a person presses it:

```
Control+Super    while pressing Ctrl+Win+Left  -> FIRES
Control+Shift    while pressing Ctrl+Shift+D   -> FIRES
Control+Alt      while pressing Ctrl+Alt+Del   -> FIRES
```

`Control+Super` is the **shipped default**, and `Ctrl+Win+Left` switches virtual desktop. A
two-modifier chord completes on the way to any longer chord that begins with the same two — rule 2
stops the longer chord *matching*, and nothing stops the shorter one *firing* as its prefix. That is
general, it affects the default, and fixing it means changing when a modifier-only chord fires, so
it is KNEG-115 rather than something to quietly fold in here.

### Two measurements, not one, and the second one is the layout

Whether a chord collides is a property of the chord. Whether the user can *make* that keystroke is a
property of their keyboard, and warning a US-layout user about a key they do not have is noise.

`kw_platform::keyboard_layout` asks `VkKeyScanExW` whether any character needs Control and Alt
together. **Not a locale table:** "Swedish" is not the fact, `'@'` needing Control and Alt is — and
the character found is carried into the warning, because *"this layout types '@' with Control and
Alt together"* is a sentence the reader can check against the keyboard in front of them and *"your
layout has AltGr"* is not. On this machine: present, `'@'`.

Three states rather than a boolean, for the reason `content_protection` has them: a platform that
cannot be asked must not answer "no". `Absent` suppresses the warning; `Unknown` still warns and
says it could not tell. Failing open, deliberately — a spare sentence costs a reader a moment, and
silence on a layout that does have AltGr is the defect.

### What is deliberately not warned about

**Standard accelerators.** `Control+Alt+D` goes to the system shortcut register rather than the
hook, and whether *that* also sees AltGr as Control+Alt has never been measured by this project.
Widely believed; unverified here; so not claimed on a screen. It is also the more common binding, so
leaving it out is a real gap rather than a tidy boundary — stated rather than papered over.

**A remedy.** A sided chord avoids AltGr entirely, because AltGr is *left* Control and *right* Alt
specifically: `LeftControl+LeftAlt` and `RightControl+Alt` both come back unaffected, and
`keys::Chord` already parses them. No settings field accepted a sided spelling — that was KNEG-35 —
so suggesting one would have been advice that could not be followed, and the caution stated the
problem and stopped. **KNEG-35 closed on 2026-08-31**, and the caution now names the escape,
computed from the colliding chord and re-checked against `altgr_collision` before it is offered.

### A mutation test that found the wrong reason

Four mutations, to check each part carries weight:

```
only the bare AltGr press modelled  -> 3 tests fail
Absent counts as possible           -> 1 test fails
Alt alone counts as AltGr           -> 1 test fails
the ModifierOnly guard removed      -> nothing fails
```

The fourth was the useful one. `a_standard_accelerator_is_not_claimed_about` passed with the guard
deleted, because `Chord::parse` refuses an ordinary key and had been doing all the excluding. The
guard was not useless — it excludes an `Invalid` accelerator, and `classify("Control+Alt+")` is
invalid (empty segment) while `Chord::parse` skips the empty segment and returns a perfectly good
`Control+Alt`, so without it a shortcut that does nothing at all would collect a warning about what
else it does. The test simply asserted the right outcome for the wrong reason. Fixed by testing the
case the guard is actually for, after which removing it fails.

### Where it shows

A third category on the Shortcuts section, beside `conflicts` and `unusable` and not folded into
either: those two mean the hotkey will not do its job, and a caution means it will, and will also do
it when nobody asked. `hotkey_set` returns the same sentence, because the moment to learn that
`Control+Alt` is AltGr is when you press Set. The mark is red and the prose is not —
`.row-detail.warn` is red *because every string reaching it is a failure*, and this one is the
opposite: the hotkey is registered, live and working, which is precisely why it fires when you type
`@`.

`cargo run -p kw-platform --example altgr_probe` re-runs both measurements on any machine.
---

## 2026-08-31 (KNEG-36) — the cheap fix would have destroyed the thing worth keeping

`knegiswhisper history list` rendered `id, when, provider, text`. A failed dictation has neither a
provider nor text, so it printed two dashes:

```
id  when                 provider  text
19  2026-08-26 09:51:11  -         -
```

The API had the answer the whole time — `/v1/transcriptions/list` returns `status` and
`errorMessage`, and the dashboard shows both. This was a renderer discarding what it was given.

### The obvious fix is wrong, and one code path proves it

Put the error message where the transcript would go. Four columns stay four, nothing widens, and the
row reads `19 … - provider returned 429`. It took reading `Dictation::fail` to see why not:

```rust
let (text, raw, provider, model) = match recovered {
    Some((text, raw, provider, model)) => (text, Some(raw), Some(provider), model),
    None => (String::new(), None, None, None),
};
```

A dictation that **transcribed correctly and then failed to paste** is recorded as `failed` with the
recovered text *and* a provider *and* a reason. That branch exists because the words are the half
worth keeping — its sibling comment says so outright: *"The text exists and only the paste failed,
so the user must be able to…"*. Writing the error into that cell would have thrown away precisely
what the recovery was added to save, and no test would have caught it, because no test had a
recovered failure in it.

So: **`status` becomes a column**, the transcript keeps its own, and the reason goes on a marked
continuation line where it competes with neither.

```
id  when                 status     provider  text
18  2026-08-26 09:50:02  completed  local     the quick brown fox jumps over the lazy dog
19  2026-08-26 09:51:11  failed     -         -
      ↳ provider returned 429
20  2026-08-26 09:52:40  cancelled  -         -
```

`cancelled` came along at no cost, and is the second half of the same defect: it also had no
provider and no text, so "I cancelled it", "it broke" and "the microphone heard nothing" were one
indistinguishable row. Three statuses, three answers.

### A note is not a column, and the reason is arithmetic

An error message is present on a small minority of rows and long when present. A sixth column would
be empty for almost every row and would push the transcript off an 80-column terminal for all of
them to accommodate a few. A continuation line costs a line only where there is something to say.
It is capped at 100 characters rather than the cells' 48, because a provider error is the one string
here whose useful half is usually at the *end* — an HTTP status, a rate-limit reason — and cutting
it at 48 keeps the boilerplate and drops the fact.

One table renderer, not two: `rows` now delegates to `rows_noted` with a closure that never returns
a note, so the four callers with nothing to add are untouched and the width arithmetic exists once.

### `history get` had the same defect, worse, and is fixed differently

It rendered `data.text` alone. For a failed row that is `""` — so the command exited zero and
printed **a blank line**. Worse than two dashes: with `list` there is at least a row to notice.

It is *not* fixed by printing a report, because printing the transcript alone is the command's job:
`knegiswhisper history get 3 > note.txt` must write a file with nothing in it but the words, and
`end_to_end.rs` pins that with `assert_eq!(stdout.trim(), "three")`. So it prints the transcript
when there is one and the status and reason when there is not.

**The limit that leaves, stated rather than discovered:** a recovered failure has both, and `get`
prints only the transcript. The clean answer is the reason on stderr and the words on stdout, which
needs `dispatch`'s `(Value, String)` to become a three-tuple across fifteen arms — too much churn
for one line of output when `list` already shows that row in full. Written into the doc comment
rather than left as an accident.

### Proven by mutation, and by driving the real socket

The renderer's own tests build JSON by hand, which proves the table *can* show a status and says
nothing about whether the server sends one. So `end_to_end.rs` gained a case that records a real
failed row, starts the real server, and runs the compiled binary — the two halves of the claim
checked against each other rather than separately.

Each part of the fix was then removed in turn to confirm a test breaks:

```
the status column removed      -> 2 tests fail
the continuation line removed  -> 5 tests fail
history get back to data.text  -> 2 tests fail
```

Not a redaction concern: the string is the one the dashboard already renders through
`outcomeLabel`, and credentials live in `kw_security::Secret`, whose `Display` is `<redacted>`, so
a key cannot reach an error message in the first place.

---

## 2026-08-31 (KNEG-104) — the fourth instance, found by the gate written for the first three

Three defects in this repository have had one shape, and each was found by a person reading a
screen rather than by anything automatic:

1. Four hotkeys that registered and dispatched nowhere.
2. `history.keepRawText`, hardcoded `false` at the only real call site, under a comment calling it
   opt-in (KNEG-98).
3. `history.enabled`, honoured before every history row and settable only by editing SQLite by hand
   (KNEG-99).

**Why the tests were no help, which is the whole point.** Every one of those settings had a key, a
default, and a passing unit test. A test sets the value directly — so it exercises the branch and
never asks whether anybody can reach it. Three green suites and three settings nobody could change.
That is not a gap in coverage; it is a question the tests were not shaped to ask, and the fix is a
different kind of check rather than more of the same kind.

`config/settings-keys.json` plus `tools/settings-audit/scan.mjs` is that check, in the contracts
job beside the parity gate and the domain scanner.

**It found the fourth instance on its first run.** `meetings.detect` — the allow and deny lists
deciding which running applications count as a meeting — has a `#[tauri::command]` that writes it,
registered in `generate_handler!`, with an IPC wrapper and a dev fixture, and **no screen calls
it**. Everything except the control exists. Filed as KNEG-114 and declared `reachable: false` with
a note, so the gate stayed green while the gap stayed visible — the same treatment the parity gate
gives its unreachable integrations, and for the same reason: a backlog printed on success is read,
and a red gate that is red every day is not. **Closed on 2026-08-31**; the backlog is now empty, and
the attempt to break the new control found that this check had been satisfied by an import all
along — see the KNEG-114 entry above.

### Two design decisions that decide whether it means anything

**The generic writer cannot count as a writer.** `commands::settings_set(key, value)` writes
whatever key the renderer names. Counting it would make the gate pass for every key ever added,
which is a check that reports success by construction. The set of such commands is *computed from
source* — a command whose key argument resolves to a renderer-supplied expression — rather than
hardcoded, so a second generic writer would be caught the day it is added rather than the day
somebody remembers this paragraph. Naming one in `writtenBy` is the `generic-writer` violation.

**An unresolvable key expression is a violation, never a skip.** The scanner resolves four shapes:
a literal, a `const NAME: &str`, a small `fn -> &'static str` match (how the calendar keys are
written), and a declared derived family (the five `hotkey.*`, built by `format!` in another crate
and pinned there by a named test this gate resolves). Anything else fails. A scanner that quietly
ignores the shape it does not understand reports green on exactly the key nobody can change — the
defect it exists to catch, one level up. That is not hypothetical here: rule 5 of the parity gate
had degenerated into a non-empty-string test and passed for sixty-four entries before rule 7 was
added, which is the same failure and is why this one was written to fail loud instead.

### Attribution, and the limit that came with it

A write is attributed to the last function declared at or before it. This is not a call graph and
does not pretend to be — a key written *only* by a helper the command calls would read as unwritten
and fail. That direction is the safe one: it is loud, and the fix is a sentence rather than a
silent pass. Measured before relying on it: of the production writes on the committed tree, every
one sits directly inside a command except calendar's OAuth bookkeeping and
`local_asr::clear_selection`, and the calendar pair are declared `internal` on their own merits.

The same attribution is why test code needs no special handling. A `#[cfg(test)]` function is never
a `#[tauri::command]`, so a test setting a value directly — the exact thing that hid all three
original defects — cannot count as the way a user changes it. Nothing had to be stripped for that
to hold; it falls out of asking the right question.

### Reachability is resolved to a component, not to a string

A key claiming `reachable: true` names a surface in `config/ui-surfaces.json`, and that surface's
component must use the IPC wrapper for one of the key's writers. Naming a screen is not enough —
which is precisely how `meetings.detect` was broken, and was the reason that key failed the check
while every other meeting setting passed. (*Use*, not *call*: what "not enough" means was itself too
weak until KNEG-114 — an import counted. See that entry.) Two surfaces were added to the registry to make this
sayable, `Dictation > Shortcuts` and `Dictation > History`, both real sections of `Dictation.tsx`
that had never been declared because no integration entry happened to name them.

**What the gate does not claim.** That a key changes behaviour, or that anything is wired behind
it. All five `hotkey.*` keys are writable from `Dictation > Shortcuts` and four of the five actions
still have no handler; `commands::hotkeys` reports that separately as an `UnhandledAction`. This
answers one question: can a user change the value at all.

### Proven by neutering, not by passing

Twenty-two self-tests, every case mutating the real committed inputs one at a time; the first two
reconstruct KNEG-99 as it actually stood. Then each of six rules was disabled in turn and the suite
re-run, to confirm a test breaks rather than assuming it would:

```
unwritable-key                  -> a test fails
unreachable-writer              -> a test fails
unresolvable-key-expression     -> two tests fail
command-does-not-write-the-key  -> a test fails
generic-writer                  -> a test fails
undeclared-writer               -> a test fails
```

A gate against "a test passed and proved nothing" that had only ever been run green would be the
same mistake a third time.

### Corrected in passing

- `getSettingsKeys`'s doc comment in `ipc.ts` said it returned *"every settings key Rust will
  accept, so a screen cannot invent one"*. It returns the keys that currently have a stored row, so
  a key sitting at its default is absent — and the command accepts any key `kw_storage::settings`
  does not reject as credential-shaped. The three generic settings wrappers also have no caller,
  which is now stated where somebody would otherwise reach for one.
- `config/ui-surfaces.json`'s note for `Settings > Transcription > Local` still said the screen had
  not been extended to install or select the Parakeet models. `121dc4c` added that selector and
  flipped both manifest entries to `reachable: true`; the note was three commits stale.

---

## 2026-08-31 (KNEG-93) — the residue that stopped being residue when the cost inverted

`is_private_host` carried three gaps recorded as *accepted* rather than missed. Two of them are
closed now, and the reason to reopen them was not that anything new was found. It was that the
decisions had been made against a cost that no longer applies.

**The inversion, which the issue stated and is worth keeping.** Before the SSRF fix, a false
negative in this predicate meant a plaintext request to that address was *refused* — the plaintext
arm only allows what the predicate calls private, so a miss lost permission rather than gaining it.
After the fix, the identical false negative means an HTTPS or WSS request is *allowed*, because the
private check is now what refuses it there. **The same class of gap that used to fail closed now
fails open.** Every one of these was accepted while it failed closed.

### Closed as a family, which is what the note itself asked for

Each embedding had been noticed separately — IPv4-translated during the review, NAT64 and 6to4
raised unprompted by the re-reviewer afterwards — and each was accepted on the same argument: the
prefix is not routed on an ordinary host, so a `connect()` fails rather than arriving.

That argument is about the *host's* configuration. It stops holding the moment a machine has 6to4 or
a NAT64 resolver, and nothing in this process can see whether it does. The recorded decision was
that if the family were ever closed it should be closed by enumerating the RFCs rather than by
adding a prefix each time somebody spots one, and `embedded_ipv4s` is that enumeration: IPv4-mapped,
IPv4-compatible, IPv4-translated, NAT64 well-known, NAT64 local-use, 6to4, Teredo.

Two things about it were not obvious going in:

- **Teredo carries two IPv4 addresses**, not one — the server in the clear at bits 32..64 and the
  client at bits 96..128 XORed with all-ones. Either being private refuses the address, because a
  packet to it involves both hosts. That is why the function returns `[Option<Ipv4Addr>; 2]` rather
  than an `Option`.
- **The NAT64 local-use prefix is not laid out the way the well-known one is.** `64:ff9b::/96` puts
  the address in the low 32 bits; `64:ff9b:1::/48` follows RFC 6052 §2.2, which splits it around a
  reserved octet — sixteen bits at 48..64 and sixteen more at 72..88. Reading four contiguous octets
  parses fine and decodes a different address than the gateway will.

### The tests state the bits, and were checked by breaking the code

Both of those layouts are wrong-in-a-way-that-still-works hazards, and a test written from the same
misreading as the implementation proves nothing. So the addresses are written out in hex, the
decoded value is asserted exactly rather than only its privateness, and four deliberate mutations
were run against the finished tests:

```
NAT64 /48 read as four contiguous octets   -> CAUGHT
Teredo client not XORed                    -> CAUGHT
6to4 read from the wrong offset            -> CAUGHT
SIIT arm removed                           -> CAUGHT
```

Without the exact-value assertions the first two would have passed: in a NAT64 `/48` and a Teredo
address there are enough nearby zero octets that a wrong offset still decodes *something* private.

### The limit that remains, and it is real

RFC 6052 permits a **network-specific** NAT64 prefix of any value at `/32`, `/40`, `/48`, `/56` or
`/64`. Only the well-known and local-use prefixes have fixed values, so a site running NAT64 on its
own prefix is invisible to this. Seeing it would mean reading the host's resolver configuration,
which is the same class of problem as DNS rebinding — and DNS rebinding stays out of scope for
exactly the reason it always has: the check runs against the literal host at validation time and has
no view of what the name resolves to at connect time. That is the problem that withdrew URL import
(KNEG-87), and it is not solved in passing.

### And the small one

`blocked_hosts()` lowercased configured vendor hosts and did not strip trailing dots from them, while
`is_blocked` stripped them from its input. No entry in `config/forbidden-domains.json` has one, so
this was a footgun for whoever next edits that file rather than a live hole. Closed anyway: an
asymmetry that costs one line to remove is not worth keeping, and the test asserts the property over
whatever the file happens to contain rather than over today's entries.

---

## 2026-08-31 (KNEG-112) — asking whether a GitHub asset can be pinned, and finding it cannot

KNEG-103 pinned all 45 Hugging Face URLs to a commit and left two archives behind: the Parakeet
models, which are assets in k2-fsa's rolling `asr-models` GitHub release. The issue filed for them
opened with three directions and said, in as many words, that the first task was a question rather
than a fix and **nobody had looked at any of them.** So this is what looking found.

### (a) Dated immutable tags — dead, measured

k2-fsa publishes plenty of versioned releases: `v1.13.6`, `v1.13.5`, twenty-odd `v1.12.x`, each with
280–340 assets. It looked promising for about a minute.

They carry **library binaries and no models.** `v1.13.6` has 283 assets and **zero** whose name
mentions `parakeet` — `sherpa-onnx-1.13.6.aar`, `sherpa-onnx-jvm-1.13.6.jar`, and so on. Every model
archive lives in `asr-models`, a tag created 2023-11-20 that now holds **498 assets** and is added to
continuously. There is no dated release to move to because the models were never in one.

### (b) The Hugging Face mirror — real, and a different artifact

`csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8` exists, published by sherpa-onnx's own
maintainer, so `resolve/{sha}` is available in principle.

It holds the model **unpacked**: a 652 MB `encoder.int8.onnx`, a decoder, a joiner, `tokens.txt` and
four test wavs. Not the 487 MB `.tar.bz2` this application downloads. Switching would mean replacing
one archive fetch with a multi-file fetch, discarding the extraction path and with it
`maxExtractedSizeBytes` — the decompression-bomb ceiling KNEG-100 and KNEG-102 were about — in
exchange for **no integrity gain**, because the digest is already measured. A rewrite that trades a
working defence for a URL shape is not an improvement.

### The fourth direction the issue did not know about, which also fails

GitHub gives every release asset an id, and `api.github.com/repos/{repo}/releases/assets/{id}`
serves that exact upload — measured: `302` to `release-assets.githubusercontent.com`. It looks
exactly like the `resolve/{sha}` analogue.

**It is not one, and the reason is the whole finding.** Git keeps old commits, which is why pinning
one costs upstream nothing and keeps us working when `main` moves. GitHub does *not* keep old
assets: a name cannot be overwritten, so replacing an asset means deleting it and uploading again,
and the old id goes with the deletion. An id-pinned URL therefore turns a swap into a `404` where
the name-pinned URL turns it into a digest failure. **Both block the install.** The pin would look
like KNEG-103's and buy nothing.

*Reasoned, not measured.* Nothing here can make k2-fsa replace an asset to watch what happens to the
id, so this rests on how GitHub's release API is documented to behave and not on an observation.

### What is actually there, and it is worth having

**GitHub's release API states a `digest` and a `size` per asset.** One request returns all 498. And:

```
parakeet-tdt-0.6b-v3       sha256:5793d0fd…eafb16bf   487,170,055
parakeet-unified-en-0.6b   sha256:99f63605…9a76e150   501,350,460
```

Both digests and both sizes match the committed pins **to the character**. That is the same shape as
`lfs.oid` — the publisher's stated digest, over the same channel as the bytes — and it is
independent corroboration of a measurement this project made by downloading, at no bandwidth cost.

`--verify-stated` reads it weekly now, beside the Hugging Face trees. Two hosts, one verb, and the
success line says which of them was which: for these two the stated digest is **weaker** than how
they were pinned, since their bytes were hashed here, so agreement means the asset has not been
swapped and not that the bytes are still what we hashed. Only the download proves the second thing,
and the download still runs.

### So the URL half is declined, and that is a result

The issue allowed for it: *"the honest answer may be that the digest is all there is, and this
closes as declined with the reasoning written down — which is a better outcome than an issue that
sits open implying somebody forgot."* It is, and it does. What changed is that the reasoning is now
three measurements rather than an assumption, and the gap it leaves is watched weekly instead of
only at install.

---

## 2026-08-31 (KNEG-86 / KNEG-85) — the gate that could not check the one thing it was for

`verify-parity.mjs` refuses a `reachable: true` claim unless the component file
exists and renders its `data-surface` marker. That is the standard it holds UI
surfaces to. A `kind: "documentation"` entry was held to nothing comparable: the
rule checked that it **named** a target and stopped there.

So `developer:agent-skills` carried `status: "ported"` with
`target: "docs/agent-skills/"` for a twenty-task run, against a directory that
had never existed, and no gate in this repository could have said so. It was
found by hand, by the audit that was looking for exactly that pattern one step
further along.

### The exemption was right and the hole was next to it

Documentation carries no code, so the backend and UI requirements genuinely do
not apply. The mistake was the `continue` that followed: nothing took their
place, which made a documentation entry the one kind that could assert work was
done with nothing checking. It gets its own equivalent now — if the status is in
`ACTIVE`, the target has to be there.

**Existence and content.** An empty `docs/agent-skills/` satisfies "the
rewritten documentation lives here" no better than a missing one, and is the
same hole one commit later.

### Injected, and failing closed

`hasTarget` is a parameter, the way `readComponent` already is, because a rule
that can only be run against the committed tree is a rule nobody can prove
bites. The file's own doc comment already made that argument: *"this tree is
clean" and "the rules work" are different claims, and only the second one keeps
working after a refactor.*

Its default is `() => false` — refuse every target. A caller who forgets to
inject it gets a loud failure rather than a silent pass, which is the direction
that matters for a check whose entire job is catching a claim about something
that is not there. There is a test for the default, because a fail-closed
default nobody exercises is an assumption rather than a behaviour.

### Proved by removing the thing

The strongest evidence is not a unit test. With `docs/agent-skills/` moved
aside, the real gate exits **1** with:

```
documentation-target-missing (1)
  developer:agent-skills: status "ported" claims the rewritten documentation is
  at "docs/agent-skills/", which does not exist or is empty
```

and exits 0 with it restored. That is the original defect, reproduced against
the fix.

### And then the documentation, which is the point (KNEG-85)

`docs/agent-skills/` now holds `knegiswhisper-api/SKILL.md` and
`knegiswhisper-cli/SKILL.md`, and the manifest entry says `ported` again — a
claim something now verifies.

Two things about their content are deliberate:

- **No hosted endpoint appears in either**, and neither says what they replaced.
  The manifest's `upstream` array is where the provenance belongs; a skill that
  spends a paragraph on a service that is not there teaches an agent about the
  wrong system.
- **Every shape in them was read out of the code that serves it** — the route
  table in `server.rs`, the request types in `wire.rs`, the `clap` definitions
  in `knegiswhisper/src/main.rs`, the `TOOLS` list in the MCP crate. Not one
  endpoint was described from a memory of what an API like this usually does.

And the limit is stated in the files themselves rather than left for a reader to
discover: **no MCP client has ever connected to `knegiswhisper-mcp`** (KNEG-91).
The protocol handling is unit-tested and has never had a real client on the
other end, so the skill tells the agent to believe the wire over the document if
they disagree. A skill that overstates its own confidence is the same defect
this pair of issues is about, one layer up.

---

## 2026-08-31 (KNEG-109) — a ceiling that cost something, and did not say what

`deepgram_socket` bounds how much transcript one session retains: 16 MB, or 10 000 events. That
bound is right — provider responses are outside the trust boundary, and session-long retention has
to be limited by something other than the provider's good behaviour. What was wrong was every part
of what happened when it was reached.

```rust
if !budget.reserve(text.len()) {
    tracing::warn!(?source, "the provider exceeded the transcript session budget");
    return;
}
```

### One `return` doing three kinds of damage

**It stopped the reader, not the retention.** The meeting kept recording, the provider kept sending,
the socket stayed open, and nothing further was transcribed. What the ceiling exists for is bounding
what is kept, and dropping the text does that on its own; ending the reader is a separate act
nobody asked for.

**It switched off KNEG-79's fix for the rest of the meeting.** This is the part that was not visible
when the issue was filed. A provider error frame arriving after the ceiling has to reach
`SessionFault` — that is the whole of the change landed hours earlier — and a dead reader cannot
record one. So the budget quietly reintroduced the silence, on the client where the wire shape is
believed rather than confirmed.

**It picked a capture at random.** The budget is shared through an `Arc`, so whichever of the two
readers happened to hit the ceiling first was the one that died and the other carried on. A meeting
could end with the microphone transcribed and the system audio not, decided by arrival order, with
nothing recording which.

And `deepgram::transcript` hardcoded `gaps: 0`. `MeetingTranscript::gaps` exists so that *"a
transcript with a hole in it must not look complete"*, and `keep` in `meeting.rs` appends a
paragraph to the saved note when it is non-zero. A budget-exhausted session had a hole and reported
none.

### Counted, not fatal

The issue's own reasoning was that counting is probably right and ending is probably wrong — a
16 MB transcript is a long working meeting, not a failure, and failing it at the ceiling would
discard a good transcript. That is what landed. `FaultKind` gained nothing; the session does not
end. The reader `continue`s, the text is dropped, and the stretch is counted.

### A gap is a run, not a frame

`TranscriptBudget` counts transitions into refusal rather than refusals. Counting per frame would
report a number nobody could act on — a long meeting past the ceiling would claim thousands of
holes where there is one.

Two details make that more than a detail:

- **A refusal is not final.** The byte ceiling is a total, so a short event can still be admitted
  after a long one was turned away. Two holes with transcript between them are two holes, so the
  flag has to close when something gets through. (The *event* ceiling is final once reached, since
  every event costs one.)
- **One flag per capture, not one per session.** The microphone can be dropping while the system
  audio is still being admitted. A single shared flag would end the microphone's stretch on the
  strength of somebody else's frame, and the two captures are not the same hole.

`reserve` returns `Reserved::{Room, NoRoom { opens_a_gap }}` rather than a `bool`, because the
caller has to do something different for the first refusal of a run than for the rest of it: one
`warn!` per stretch. Per frame it would be the loudest line in the log for the remainder of the
meeting.

### The sentence on the note was true and is now only half true

`keep` said: *"N stretch(es) of audio were never transcribed: the transcriber could not keep up with
the meeting."* Accurate while the only source of a gap was `local::LocalWindowProvider`, which
counts a sustained deficit — the model falling four hops behind and skipping to live. A budget gap
is not that. It is this application's own retention ceiling on a very long session, and there is
nothing the transcriber could have done faster.

So `gaps` now has two producers and one number, and the number does not say which. Both the field's
doc comment and the note's sentence name both possibilities rather than asserting one. Carrying the
cause properly would mean `gaps` stops being a `u32`, which is a change to a serialized type for a
distinction nobody has yet needed to act on — recorded as the trade rather than made silently.

### What is not verified, and it is most of it

No key for Deepgram exists on any machine this project has touched, so none of this has run against
a real provider (KNEG-53, KNEG-56). Reaching 16 MB of transcript needs a very long meeting on a
service nobody can currently connect to. The budget arithmetic and the gap counting are unit-tested
against the real `TranscriptBudget`; the reader's behaviour around them is read from the code.

---

## 2026-08-31 (KNEG-110) — a field that carried two facts, and only one of them stayed true

`LocalApiRuntime` stored the `Permissions` that `discovery::write` returned and showed that value
for as long as the application ran. Both settings screens rendered a claim about the past as a claim
about the present: anything with write access to `~/.knegiswhisper` could widen the access list a
second after start, and neither screen would say so until the next launch.

### The shape of the mistake is worth naming, because it is not "forgot to refresh"

One field held two facts:

```rust
/// The permissions the discovery file actually ended up with.
permissions: std::sync::RwLock<Option<Permissions>>,
```

The `Option` meant *is the API running* — the file is deleted on stop, so `None` is the honest
answer then. The `Permissions` inside it meant *what the file grants*. Only the first of those stays
true on its own. And the confusion was load-bearing rather than theoretical: `integrations.rs` asked
`permissions_snapshot().is_some()` **twice**, reading a permission verdict to learn something that
is not about permissions. An idiom like that is what makes a staleness bug easy to walk past.

So the fix is not a refresh call bolted on. The field becomes `discovery_written: AtomicBool` —
"there is a file" and nothing else — and the value is read when it is asked for. `is_running()` is
its own method now, so the two questions are asked separately.

### `discovery::inspect`, extracted rather than written

`write` already ended with exactly the read this needed:

```rust
let file = file_acl::verify(path, PathKind::File);
match parent { Some(p) => file_acl::combine(&file_acl::verify(p, PathKind::Directory), &file), … }
```

That expression is now `discovery::inspect`, and `write` ends by calling it. Extracting rather than
re-writing matters here more than usual: the verdict a caller gets after a write and the verdict it
gets from a later re-read have to *be* the same judgement, not two judgements that agree today. The
directory's verdict gating the file's is the part that would have drifted first — it is the
non-obvious half, and it is the half a second author would most likely leave out.

`the_read_back_describes_the_file_the_caller_is_told_about` already reconstructed the expression
independently and asserted `write` matched it. It keeps doing that, and gained one line asserting
`inspect` matches too, so the test now pins both the value and the fact that there is one copy of it.

### Read, never re-apply

Deliberate, and the reason is written at the call site. Silently repairing an access list somebody
changed on purpose is its own kind of surprise, and it would make the function unusable for what it
exists for: a status screen has to be able to *describe* a bad state without altering it. So the
application will tell you the token file has been widened. It will not quietly narrow it again.

### No cache, and that is a measurement rather than an omission

Two `stat` calls on Unix, two `GetNamedSecurityInfoW` calls on Windows, against a path already in
the filesystem cache. The issue suggested a short cache might be wanted because this is a UI refresh
path — but `useRefresh` has no timer. It fetches when a screen mounts and when something asks it to.
A cache would have added a staleness window, in the function whose entire purpose is removing one,
to fix a cost nobody is paying.

### The regression test, and why it deletes rather than widens

`the_verdict_is_re_read_rather_than_remembered` removes the discovery file behind the runtime's back
and asserts the verdict changes. Deleting is the one way to change what an access list says that
works identically on every platform, and this machine is Windows.

The case the issue actually described — a file that still exists and now grants more — is
`a_mode_widened_after_start_is_reported`, a `#[cfg(unix)]` test that chmods to `0644` and requires
`Wider { mode: 0o644 }`. It does not run here. It runs on CI's `contracts` job, which is
`ubuntu-latest` and is the only place the whole of this file is compiled — the same asymmetry
KNEG-111 is about, pointing the other way for once.

---

## 2026-08-31 (bench) — the install button that could only ever work once

The first person to press *Install* on a whisper model found nothing happened. No `ggml-base.bin`,
no `.partial`, no error worth acting on. The same session installed a 1.3 GB GGUF without trouble,
which is what made the cause findable.

### What it was

`local_model_install` installed the server sidecar **unconditionally** before the model:

```rust
kw_sidecar::install(&local.policy, spec, asset, &sidecars.path, ...).await?;
let installed = models::install(&local.policy, model, &models_dir.path, ...).await?;
```

`kw_sidecar::install` ends by replacing the sidecar directory, and `resume_last_used` has already
started `whisper-server.exe` out of that directory by the time anybody presses anything. On Windows
a directory holding a running executable cannot be removed, so `remove_dir(&final_dir)` failed, the
`?` returned, and the model was never fetched.

**So the button could only work when no whisper model was in use — which is to say, once.** After
the first install there is always a model to resume, so there is always a server running out of the
directory the next install has to replace.

### Measured rather than reasoned

The mechanism was checked on this machine rather than taken from memory, because "Windows locks
running executables" is the kind of thing that is true in outline and wrong in the detail:

- `remove_dir_all` on a directory holding a running `.exe` → **`WinError 5`, Access is denied**,
  reported against the `.exe` itself.
- `rename` of that same directory → **succeeds.** Windows denies delete on the image file, not
  rename of a parent. Worth writing down; it is the escape hatch if a real upgrade path ever needs
  one.

A first attempt at this measurement used `timeout.exe`, which exits immediately with no console
attached, and reported "no lock". It proved nothing and would have sent the diagnosis the wrong way.
The second used `ping -n 40`, confirmed running in the process list before the deletion was tried.

The failed run had also left its own evidence: `sidecars/whisper.incoming` holding a complete,
verified, extracted copy of the sidecar that never got promoted — exactly the state the code leaves
when it fails between extraction and the swap.

### The fix was already written, in the file next door

`local_llm_install` in `local_llm.rs` has always guarded the same call:

```rust
let installation = kw_sidecar::locate(SIDECAR, asset, &search_path, &kw_sidecar::RealFs);
if installation.executable().is_none() { kw_sidecar::install(...).await?; }
```

That is why the GGUF installed on the run that the whisper model failed. The whisper path now does
the same thing, through a named `sidecar_needed` so the judgement is testable — the command takes an
`AppHandle` and is untestable by construction, the same reason `check_importable_file` and
`import_pick_dialog` exist. Three tests: installed skips, missing installs, and **`Incomplete`
installs**, which is the case with the judgement in it — a directory with no executable is an
interrupted extraction, nothing is running out of it, and replacing it is safe.

### Skipping loses nothing, and that is worth arguing rather than asserting

The unconditional call looks like a way to keep the sidecar current. It never was. It can only
succeed when no server is running, which is only the first install, when there is nothing to
refresh. Every later call failed outright rather than updating anything.

But the gap it appeared to cover is real and now has nothing covering it: the installed whisper
sidecar dates from 2026-08-25 and the pinned archive has since gained `bench.exe` and `command.exe`,
so it is measurably behind. Filed as **KNEG-113** with the rename measurement, rather than solved
here by a scheme whose cleanup story is unresolved — a displaced directory still cannot be deleted
while the old executable runs, so it has to survive until a launch where nothing holds it.

### The other finding: two documents told a person to hold a chat

`docs/BENCH_CHECK.md` and `docs/BUILD_PLAN.md` both instructed the reader to install a local LLM and
*hold a chat*. **There is no chat.** The application has thirteen views, none of them a chat;
`config/ui-surfaces.json` has no such surface; the chat agent is phase 10 (KNEG-74).

The local LLM has one consumer, dictation cleanup, and the path is built: `Settings > Local models`
→ *Use this model*, then `Settings > Intelligence` → the **local model** route, then dictate.
`reasoning.rs` already says so on screen — *"no local model is running. Start one under Local
models."* Only the instruction was wrong, and an unrunnable by-hand step is worse than a missing
one: it reads as a defect in the application rather than in the sentence.

---

## 2026-08-31 (KNEG-103) — 45 URLs stop naming a branch

Every model this application downloads was fetched from
`https://huggingface.co/{repo}/resolve/main/{file}`. `main` is a branch pointer. The bytes behind it
change whenever the publisher pushes, and as of earlier the same day every one of those 45 objects
carried a digest measured against the bytes that were there *then*.

So the defect was not that a re-upload would go unnoticed. It was that a re-upload would be noticed
in the worst possible place: at the end of a nineteen-gigabyte download, in front of a user, as an
integrity error they cannot get past, caused by somebody else's routine publishing.

### It was not theoretical, and the measurement says how far from it

`node tools/model-pin/pin.mjs --revisions` reads `/api/models/{repo}` for all thirty-seven
repositories — 36 GGUF plus `ggerganov/whisper.cpp`. Of the thirty-seven, **twenty-one last moved
during 2026, and four moved on 2026-08-24 — seven days before the digests were taken.** The four are
LiquidAI's LFM2.5 repositories.

Nothing had moved *since* the pins: all 39 stated digests and all 39 lengths still agreed, and the
two whisper digests spot-checked during the design pass came back byte-identical to what was
committed. The pins were correct. They were also one push away from not being, on a table where four
rows had changed inside the previous week.

### Where the revision lives, and why not beside the digest

One entry per **repository**, not per object. `HF_REVISIONS` is keyed on `owner/name`, so the six
whisper models share one row and the thirty-nine GGUF objects fill thirty-six. A revision is a
property of the repository at a point in time; a per-object copy would be six identical strings that
could go inconsistent, and there is no state a reader could make sense of where two files in the same
repository are pinned to different commits by accident.

The table sits in the generator beside `WHISPER_PINS`, `SHERPA_PINS` and `LOCAL_LLM_PINS`, for the
reason all three of those are there: `config/upstream-integration-baseline.json` is a frozen record of
what upstream had and must not gain fields we invented, and `config/provider-registry.json` is
generated, so anything hand-edited into it dies at the next regeneration.

### Two families, two mechanisms, one table

The whisper URL is *stored* — upstream's baseline supplies it whole — so the generator rewrites the
revision into it on the way out. The GGUF URL is *composed* in `local_llm.rs` from `hfRepo` +
`fileName`, so it gained a third part. Same pin, two paths, and the difference is a fact about where
each URL comes from rather than a choice.

`parseHfUrl` is exported from `tools/model-pin/pin.mjs` and imported by the generator. That is a real
coupling between a build gate and an audit tool, taken deliberately: the generator rewrites those
three parts and `--verify-stated` reads them, and two regexes for one URL shape would have drifted
apart on the first publisher who put an object in a subdirectory — which several already do, for
multi-part quantisations.

### `revision` is a required field, and `sha256` is not

`LocalLlmModel::sha256` is an `Option`, so a model added before anybody spends the bandwidth degrades
to a size check. `LocalLlmModel::revision` is a plain `String`.

The asymmetry is the point. A missing digest degrades to a weaker check that is still a check. A
missing revision would degrade to `main`, which is not a weaker version of this fix — it is the
defect, reintroduced silently, for one model out of thirty-nine. So the generator refuses to emit a
model without one, and a registry that somehow reached the Rust without it fails to deserialise
rather than quietly floating.

Four guards, each one run against a deliberately broken table before being trusted:

- a repository with no `HF_REVISIONS` entry dies **naming the model that needed it**, not just the
  repository;
- a revision that is not 40 lowercase hex dies, with "re-read it; do not hand-correct it";
- a revision no model downloads from dies as a typo — the same guard the three pin tables carry;
- every shipped whisper URL is re-parsed after the rewrite and must name a commit. `pinnedHfUrl`
  lets a non-Hugging-Face URL through untouched, which is right — there is nothing there to pin —
  and this is what stops that from becoming a quiet way back to a floating reference.

### What this is not

**It is not a stronger integrity claim.** The digest is what proves the bytes; a revision only decides
which bytes were promised. Nothing here defends against a compromised huggingface.co any better than
before — one host still serves both the object and, for thirty-two of the thirty-nine, the `lfs.oid`
they are checked against.

What it removes is a class of spurious failure. What it adds is that the URL and the digest now
describe the same object *by construction* rather than by nobody having pushed yet.

### `--verify-stated` was asking the wrong question and is now asking a better one

It read each repository's tree at `main`. That answered "has the publisher re-uploaded", which was
the right question while the URL pointed at `main` and is the wrong one now — a push cannot reach a
pinned commit, so the answer stopped being about anything the installer would fetch.

Reading the tree at the *committed* revision asks something nothing else in the workspace checks:
**do the digest and the revision committed beside each other describe the same object?** Those two
numbers are measured in different runs by different code and could drift apart with nobody noticing.
It also catches a pinned commit that has stopped resolving — a force-push, or a repository going
private — which would break every install of its models.

A second half reports which repositories have published past their pinned commit, and **does not
fail on it**. Upstream publishing is normal, and that is now the whole point: a weekly job that went
red every time a publisher pushed would be ignored by the second month. It is news about pins worth
revisiting, printed as such.

The tree cache is keyed on repository *and* revision, in the tool and in the Rust test both. One
revision per repository is how the table is built, and a cache that assumed it would read the wrong
tree, silently, the day that stopped holding.

Measured after the change: **39 objects, 36 tree requests, all matching at their pinned commits; no
repository has moved past its pin.** The two `#[ignore]`d Rust tests agree — 39 HEADs at
commit-pinned URLs and 36 tree reads, in nine seconds.

### What is still hanging off a moving reference

The two Parakeet archives. They are GitHub release assets in k2-fsa's rolling `asr-models` tag, where
an asset can be replaced under the same name, and there is no commit-sha equivalent to reach for.
Their digest is the only thing that would notice a swap, and the weekly `--verify` is what runs it.
That is unchanged by this and is recorded in `docs/PLATFORM_SUPPORT.md` rather than left implied by a
fix that did not cover them.

---

## 2026-08-31 (KNEG-34) — the token file finally gets a DACL, and the read-back moves

`discovery::restrict_file` was a no-op under `cfg(not(unix))`, so the local API's bearer token sat
in a file with whatever ACL the profile directory carried. Measured three times on the same machine
across five days — 2026-08-26, 2026-08-31, and again immediately before the fix landed — and
identical every time: two trustees with Modify who were neither the user nor administrators, every
entry inherited, `DC` (delete-child) on both of them so the file could also be deleted and replaced.
A pass on 2026-08-27 had fixed only the *reporting* of that gap. This is the gap.

The reasoning is more interesting than the outcome, which is three `icacls` lines.

**Owner + SYSTEM + Administrators, not owner-only.** The obvious target is a DACL granting only the
user. It is the wrong one. An administrator can take ownership of any object and re-grant themselves
anything, so excluding them costs backup and repair tooling real breakage and buys a strictness that
does not exist; and the project brief forbids attempting to bypass administrator controls in the
first place. What was actually worth removing was the *unexpected* trustees and the inheritance that
kept handing them down — which is exactly what the old code comment falsely claimed the inherited
ACL already gave. So "confirmed" on Windows is a deliberately narrower claim than "confirmed" on
Unix, and the settings label still says an administrator can read the token. Stating a smaller true
thing beats stating a larger one that is not.

**The grant comes from the process token, not from the object's owner.** `GetNamedSecurityInfoW`
returns the owner in the same call, and using it is the obvious shortcut. On a machine whose policy
makes `BUILTIN\Administrators` the owner of objects an elevated process creates, that shortcut
produces a DACL listing Administrators and *not* the user — and a later non-elevated run cannot read
its own file. Silent, and only on machines with that policy, which is the worst combination. So the
grant is `OpenProcessToken`/`TokenUser`. The owner is still read back and still judged: an owner
outside the three holds `WRITE_DAC` and can re-grant itself, so it counts as an extra trustee
whether or not it has an entry of its own. On this machine the token user is `S-1-12-1-…`, an Azure
AD SID rather than the `S-1-5-21-…` a hardcoded guess would have reached for.

**The DACL survives the rename — measured, not reasoned.** `discovery::write` restricts a temporary
and then `fs::rename`s it into place. The argument that this is safe (a move within one directory on
one volume preserves the file object and its descriptor; inheritance is re-applied on a copy or a
cross-volume move) is correct, and it was still only an argument. It is now a `#[cfg(windows)]` test
and a probe run. **The read-back moved anyway**, to after the rename, because the old code verified
`local-api.json.new` — a path that does not exist by the time the verdict reaches the caller. On
Unix that verdict was true of the final file by an argument the code never made. That is the same
class of defect one level down, inside the file written to avoid it.

**The directory-before-file ordering is a real property, and it needed a second half.**
`restrict_directory(parent)` runs before the temporary is created, so the temporary inherits a
protected list and the token's bytes never touch disk under the loose one. Measured: a file created
inside a protected directory does inherit exactly the three trustees — but *not* `SE_DACL_PROTECTED`,
so it reports `StillInheriting` and would follow the directory if the directory changed. Hence the
file is still restricted in its own right. And `fs::write` alone put the token's bytes on disk before
anything narrowed the file, which is a real window on Unix too — a `0700` directory does not stop a
`0644` file inside it. The temporary is now created empty, restricted, and only then written into.

**The directory's verdict gates the file's.** A perfectly restricted file inside a directory another
user may delete children of is not a restricted file: it can be deleted and replaced, and the CLI
and MCP client then read an attacker's port and token. For a file whose contents *are* the access
control, that is worse than being readable, not smaller. `file_acl::combine` is the pure function
that says so, and a perfect file in a loose directory reports `DirectoryNotRestricted`.

**Ten states, not four, and the count-not-names call.** The Windows read-back can fail in ways
`Wider { mode }` cannot express — it is Unix-shaped and cannot carry a trustee — and collapsing them
into `Unknown` would report something that was measured as something that was not. So five mismatch
arms, each with its own sentence in `labels.ts`. `ExtraTrustees` carries a **count**, not names: the
value crosses to TypeScript and onto a screen that ends up in screenshots and bug reports, and a
trustee name is one of the machine's account and group names. The SIDs go to a `tracing::warn!`
inside `verify` instead, and the label says where to look. `DaclFacts.dacl` is an `Option` because a
NULL access list grants everyone everything while an empty one grants nobody anything, and code that
conflates them reports the worst case as the best.

**`is_confirmed()`'s line is checked versus unchecked, not strict versus lax.** The old doc comment
argued the other one, and it had to be rewritten rather than extended: `OwnerAndAdministrators` lets
administrators read the file and passes, because the list was read back entry by entry;
`InheritedFromProfile` may well be perfectly restrictive and does not, because nothing looked.
`kw_local_api::Permissions::is_as_expected()` — the helper whose "as restrictive as the platform
allows" is the sentence that misled once and is recorded as a finding in three documents — was
deleted. It had no non-test caller. There are now two `is_confirmed()` definitions, one per side of
the IPC boundary, and two definitions of one line is the shape of the original defect, so
`the_two_confirmed_decisions_cannot_drift_apart` walks every variant and requires them to agree.

**The second false comment.** `kw_sidecar::pid`'s `restrict_file` claimed *"on Windows the file
inherits the user profile's ACL, which already excludes other users"* — the same assertion as the
one in `discovery.rs`, in a different crate, about the same profile directory, equally false. Two
independent comments asserting an unverified fact about the same thing is the argument for one
shared helper rather than two call sites that each get to believe something. The PID record's
verdict is logged rather than reported to the user, and the comment says why: its worst case is a
forged entry `crate::reap`'s command-line check already refuses to act on, so refusing to start a
sidecar over an odd access list would be the worse outcome. A PID is not a secret — it is protected
for **integrity**, and `pid.rs`'s own "a record another user could rewrite is a request to signal a
PID of their choosing" is the sentence worth keeping.

**Two things found and not fixed here, both filed (KNEG-110, KNEG-111); the first has since been
fixed.** The verdict was a **snapshot taken when the API starts** —
`local_api_status` reported what the write measured, and nothing re-read the DACL while the
application ran. During this run a trustee was added to a protected directory by something outside
the application, between the write's read-back and the next `icacls`; the application would not have
noticed until its next start. (That specific ACE was an artifact of the sandboxed harness the
measurement ran under, and did not reproduce when `discovery::write` created the directory itself,
which is what the application does — but the general fact is real.) And **CI never compiles any of
this**: `.github/workflows/ci.yml`'s `contracts` job runs on `ubuntu-latest`, and the
`windows-latest` matrix job is switched off for every trigger but `workflow_dispatch`. The pure
classifier *is* covered there, deliberately — `file_acl::classify` takes plain Rust facts, and the
measured DACL above is a fixture in its test module, so the regression test for a Windows exposure
runs on a Linux box. The three Win32 calls are covered by two `#[cfg(windows)]` tests — the
repository's first — and by `cargo run -p kw-security --example dacl_probe`, which verifies only
when given a path so the real `~/.knegiswhisper` can be re-measured without the default being
destructive.
## 2026-08-31 (KNEG-79) — four clients that heard "no" and said nothing

The four cloud meeting clients each read a provider-reported error frame, logged it with
`tracing::warn!("the provider reported an error")`, and carried on. None of `connect`, `send_audio`
or `finish` ever returned an `Err` because of it, so a provider that rejected the session-config
frame and kept the socket open produced a meeting that "completed successfully" holding an empty
transcript — and an empty transcript becomes `MeetingOutcome::NothingTranscribed`, which is exactly
what a quiet room produces. The gap had been recorded three times, in `BENCH_CHECK.md` item 13,
`BUILD_PLAN.md` and this file's own carry-forward, and fixed none of them.

It stopped being hypothetical on 2026-08-28. Both OpenAI sockets were closed by the server
milliseconds after connecting; the application reported **Recording**, kept the indicator up,
captured microphone and system audio for as long as it was left running, and said nothing. One
`DEBUG nothing is consuming the meeting audio` per second was the only trace.

### What was built

`crates/meeting-core/src/fault.rs`: `SessionFault`, one cell per session shared by both readers and
consulted by both writers, plus `ends_the_session` — a pure, total function answering *does a fault
of this kind mean the session is over?*

The pure split was not optional. There is no socket harness in this repository: no `wiremock`, no
`mockito`, no fake `MeetingTranscriptionProvider`, no `crates/meeting-core/tests/`. Every test in
the four socket modules returned before a socket ever opened, and each module's own header said so.
Anything reachable only through a live socket is untested by construction, which is the same reason
`openai::SessionClock` was pulled out of the socket: *a rule that only runs against a live
sixty-minute call is a rule nobody checks.*

### Three decisions worth the space

**`OnceLock`, not `Mutex`, so first-wins is true by construction.** A cascade of frames after the
real error would bury it, so the first fault has to win. Written as `if held.is_none() { ... }` that
is a convention a later edit can invert; written as `OnceLock::set` it is the type's own behaviour,
and there is no poisoned-lock branch to write around this workspace's `unwrap_used = "deny"`.

First rather than last, because of the one real observation there is. The 2026-08-28 order was:

```text
WARN the provider reported an error       The Realtime Beta API is no longer supported.
WARN the transcription socket was closed  said=... (code 4000)
```

The first line explains the meeting. The second is a number. Last-wins keeps the number.

**One cell per session, not one per socket.** Every client here refuses to open only one socket —
"a session with only the microphone open would transcribe half the meeting and report success" — so
a dead system-audio socket has to end the session even while the microphone socket is healthy. The
two almost always die of the same cause, and the recorded sentence names which capture it came
from, so one true sentence beats the same sentence twice.

That has one sharp edge, and it is the reason `openai_socket::reconnect` swaps in a *new* cell
before rebuilding. `Connection::close` sends a close frame and only then aborts the reader, so the
socket a reconnect retires can still read the server's reply — and a close frame is a
`FaultKind::Reported`. Shared across generations, this fix would have failed every meeting over
sixty minutes at its own reconnect: a new defect introduced by the fix for an old one.

**The non-JSON case, decided rather than left to fall through.** All four `interpret` functions
answered *any* unparseable frame with `Incoming::Failed`, so an HTML error body and a text keepalive
this parser happens not to know were the same thing. Making `Failed` fatal without splitting them
would have killed a working meeting over a keepalive — a new defect of the fix's own making, and
the exact failure mode this change exists to prevent.

So there is a fifth variant, `Incoming::Unreadable { preview }`, in all four rules modules, and the
rule is about *when* it arrives: **an unreadable frame on a socket that has never yet produced a
frame this client could parse ends the session; one that arrives after it has does not.** That
targets the observed hazard — garbage where a session-accepted frame belonged — and cannot kill a
stream that has demonstrably been speaking this client's language. "Understood" means *parsed*, so
`Ready`, a transcript, an error frame and even an event this client does not act on all count; ping
and pong never reach an `interpret` call. The loop supplies one `bool` and `ends_the_session` holds
the rule.

The frame itself is repeated back rather than replaced with "not JSON", through one shared
`fault::preview` — control characters flattened, runs collapsed, capped at 120 characters, because
provider responses are outside the trust boundary and this text reaches both a log field and a
sentence on screen. `<html>502 Bad Gateway</html>` is worth ten diagnostics that say "not JSON", and
the wire most likely to produce one is Corti's, which was invented with no vendor documentation at
all.

### Edited four times rather than extracted

`type Socket`, `type Sink`, `struct Connection`, `Connection::close`, the whole `read` skeleton,
`fn connection`, `pause`/`resume`/`cancel` and the `static FALLBACK` blocks are near-duplicated
across the four socket modules. Unifying them is a real refactor with its own risk and KNEG-89
already tracks it, so what was extracted here is the *decision* — one `SessionFault`, one
`ends_the_session`, one `close_reason`, one `preview`, one `SocketFault::into_error` — and the four
call sites were edited by hand.

Four hand-applied copies of a security-relevant decision is exactly how one gets missed, so
`all_four_cloud_clients_record_a_fault_and_consult_it_at_both_ends` reads the four modules' own
source with `include_str!` — the device `kw_network_policy` already uses for
`forbidden-domains.json` — and fails if any one of them lacks the field, any of the four record
calls, or the two `self.fault.take()` sites. It is a text match and will not catch a client that
consults the cell and ignores the answer; the two behavioural tests now at the bottom of each socket
module do that, one client at a time.

Those two tests per module are the first in those four files to reach past the guard clauses, and
they exist because `send_audio` checks the cell **first** — before `paused`, before the reconnect
clock, before the connection lookup. A test can then record a fault exactly as the reader task would
and ask what `send_audio` and `finish` do about it, with no socket, no key and no network. Placing
the check first is also correct on its own terms: a provider that reported an error and kept the
socket open produces a *successful* send, so nothing about the send can be asked.

The per-socket `Closed` cell in `openai_socket` was **not** folded in. Two cells, two jobs: that one
supplies better words for a send that has already failed, this one decides whether the session is
over. Folding them would have put `ad9dac1`'s hard-won behaviour — the thing that made five GA-API
faults diagnosable — at risk for a tidiness gain.

### `stop()`, which was worse than it looked

`stop()` returned early from three places: the recorder thread having stopped, the thread not
answering, and the pump failing. `Live` had already been `take`n by then, so each of them left the
runtime idle while skipping the whole tail — no `OUTCOME_EVENT`, no `MeetingState::Idle`, no
`meeting_window::set_visible(false)`, no `watch.forget`.

And the screen was then stuck permanently, not briefly: `guard` in `useRefresh.ts` does not
`reload()` when a mutation throws, so the frontend stayed on `finishing`, where
`SettingsMeetings.tsx` renders neither Stop nor Discard and disables Start. No working button, tray
still lit, indicator window still up, until the application was restarted.

Every failure is an outcome now. Three free functions came out of it — `close_recorder`,
`pump_result`, `failed_message` — testable with no `AppHandle`, `Db` or `NetworkPolicy`, which is
the shape `resolve_key_ref` and its neighbours already use in that file.

**The whole-file local pass still runs after a pump failure**, and that is a fix in its own right
rather than a concession: the saved note comes from that pass and only falls back to the cloud
partials, so a provider that dies at second zero costs the live transcript, not the meeting. The old
`?` skipped the pass entirely, which was a pure loss. `keep` takes `live_failure: Option<&str>` and
uses it twice — an empty result becomes `Failed` naming what the provider said instead of
`NothingTranscribed`, and a note that *was* produced says in its own text that the live half failed.

### No fake provider, and what it would take

`pump` has zero test coverage and still does. Introducing the repository's first fake
`MeetingTranscriptionProvider` was considered and declined: `pump` already propagates its `Err`
correctly through `?`, so the fake would have tested a line that was not broken, while the broken
line was in `stop()` — which needs an `AppHandle`, a `Db` and a `NetworkPolicy`, none of which a
fake provider supplies. The free functions cover what actually changed.

Recorded for whoever does want it: the obstacle is `pump`'s `app: AppHandle`, and the better of the
two routes is giving `pump` a small `TranscriptSink` trait — matching what `calendar-core` did with
`TokenSender` and its test-only `PlaintextLoopbackSender` — rather than enabling `tauri`'s `test`
feature, which nothing in this workspace uses.

**This entry predicted KNEG-95 would need that, and it did not.** Corrected here rather than left
standing: the decisions KNEG-95 turned out to need — which state to report, whose session an ending
belongs to, which sentence to use — are all free functions over plain values, the same shape
`pump_result` and `close_recorder` already had. `pump` is still untested and still does not need to
be. What a `TranscriptSink` would buy is a test of `pump` itself, which nothing has yet wanted.

### What stayed KNEG-95's, and was taken later the same day

**A dead provider was still silent mid-meeting when this landed.** The indicator stayed up,
recording continued, and nothing reached the screen until Stop. No new `MeetingState` variant, no
change to whether recording continues, `MeetingRuntime::status()` not consulting
`pump.is_finished()`, and no mid-meeting error event — all deliberate, because the choice between
stopping the recording, falling back immediately, and carrying on while saying so belonged to the
repository owner.

That choice was made on 2026-08-31: **carry on, and say so.** Every one of the four items above is
now done, and the reasoning is in the 2026-08-31 (KNEG-95) entry above — including the two races
that decision turned out to contain, neither of which was visible from here.

### What could not be verified, and is not claimed anywhere

**No key exists on any machine this project has touched for AssemblyAI, Deepgram or Corti.** Three
of the four clients' behaviour against a real provider error is unverified, and the tests added here
verify this application's wiring rather than any provider's behaviour. The argument for the change
is that it is exactly the mechanism that would have surfaced the five OpenAI faults of 2026-08-28 in
one attempt instead of five. That is an argument, not evidence, and the code comments,
`PLATFORM_SUPPORT.md` and `BENCH_CHECK.md` item 13 all say so in those terms.

One risk is written into `ends_the_session`'s doc comment rather than hidden: `FaultKind::Reported`
ends the session on the **first** error frame, and a provider that emits benign `error` frames
routinely would make that too eager. The mitigation, if it ever matters, is one line — give
`Reported` the same "before anything was understood" test `Unreadable` has.

### Found while here, filed rather than widened

`deepgram::transcript`'s `gaps: 0` carried the comment *"Nothing is dropped by this client: the
socket either carries a frame or the send fails and the session ends."* That was untrue for a second
reason, predating all of this: `deepgram_socket`'s reader `return`s on `TranscriptBudget`
exhaustion, so a session past `MAX_TRANSCRIPT_BYTES` or `MAX_TRANSCRIPT_EVENTS` keeps recording and
transcribes nothing further — silently, and uncounted, because `gaps` is hardcoded to zero. Bounding
untrusted provider output is deliberate; not counting what the bound costs is a gap. The comment now
says so and the behaviour is unchanged.

> **Fixed the same day (KNEG-109), and the filing understated it.** The reader no longer returns, so
> the ceiling bounds retention without also ending transcription — and without silently switching
> *this* change off for the rest of the meeting, which is what a dead reader did to `SessionFault`.
> See the KNEG-109 entry at the top of this file.
## 2026-08-31 (KNEG-80) — a path the renderer could invent, and why the capability beat the fusion

`Notes > Upload` had a hole with a very short description and a very long consequence.
`pick_import_file` opened a real OS dialog and returned the chosen path **to the renderer**, which
held it in React state and passed it as a plain string to `media_import_audio`,
`media_import_video` or `import_openwhispr` on a later click. Those three read whatever path they
were called with. Nothing confirmed it was the one the dialog returned.

The half that held is worth stating first, because it is the reason this sat unnoticed:
`apps/desktop/src-tauri/capabilities/default.json` grants exactly `["core:default"]` — no `fs:*`,
no `dialog:*`, no `shell:*`, no `http:*` — so the renderer genuinely has no independent way to
*obtain* a path. Every path it could hold came from a real dialog. But "cannot obtain" and "is
checked" are different claims, and only the first was true. A compromised renderer — which
`docs/SECURITY.md`'s threat model includes, so this is not an invented adversary — could call
`media_import_audio` with any path it liked, and the path becomes argv for a spawned ffmpeg, which
writes `converted.wav`, which is read into memory and handed to
`kw_transcription_core::Executor::transcribe`. With a cloud provider configured, that is an
exfiltration path for any readable file on the machine, reached with one IPC call.

### Why a capability rather than fusing pick and import

`note_export_to_file` already demonstrates the airtight shape: it fuses picking and writing into
one atomic command, so the frontend never sees or supplies a path at all. Copying that shape here
was the obvious first answer and it is the wrong one, for a reason that is about the screen rather
than about security. Export is a single click — choose where, write, done. Import is
pick-then-review-then-confirm: the user picks a file, the screen shows what it picked and what
provider will transcribe it, and *then* they press Import. Fusing would collapse the review step
that the screen exists to provide. A guarantee bought by deleting the interaction it was protecting
is not a good trade.

So the path still crosses the boundary, and what makes it safe is a capability instead:
`commands::DialogGrants`, a `Mutex<VecDeque<PathBuf>>` managed in `lib.rs`, written only by
`pick_import_file` and consulted by all three path-accepting commands before they touch anything.

Three implementation choices, each of which had a plausible alternative that is worse:

- **Session-lifetime, not single-use.** Single-use is the tighter policy and it breaks a flow that
  works today: `NotesUpload.tsx`'s `catch` leaves the picked path in state after a failure, so the
  same file can be retried without re-picking. A grant consumed by the first attempt would turn
  every failed import into a re-pick. There is a test for the two-attempt case specifically, so the
  next person to reach for single-use has to delete an assertion that says why not.
- **Bounded by count, not by time.** A time bound would refuse a file the user really did pick if
  they left the review step open — and they realistically will, because that screen can sit on a
  picked path across an ffmpeg install of tens of megabytes. This is the same argument
  `meeting::Watcher::offered` makes in its doc comment for keying on the provider rather than on a
  timestamp: a time-tuned suppression gets one of its two cases wrong whichever way it is set, and
  identity gets both right. Reusing that reasoning was better than restating it. The bound is 32,
  which the screen (one path at a time) will never approach; it exists so a renderer calling the
  picker in a loop cannot grow the set without limit.
- **Keyed on a canonicalised `PathBuf`, never the string.** `pick_import_file` returns
  `path.display().to_string()`, which is **lossy**. Comparing strings would mean comparing a path
  that had already lost information. Both the insert and the check go through `std::fs::canonicalize`
  — which on Windows returns the verbatim `\\?\C:\...` form, so both sides must canonicalise or a
  canonical path gets compared against a raw one and never matches.

### What the lossy path costs, stated rather than buried

A path whose name does not survive `display()` cannot be imported. That is a real refusal and it is
written into the code as one. It is **not** a regression: `check_importable_file`'s `is_file()`
already failed on that same lossy string today, silently and by accident. The change is that it is
now deliberate and says so. If someone with a filename that does this ever reports it, the answer
is to stop round-tripping the path through the renderer at all — i.e. the fusion above, for that
one case — not to compare strings.

One smaller decision inside `authorise`: a path that cannot be canonicalised and a path nobody
granted get the *same* refusal. Distinguishing them would make the command answer "does this file
exist" for any path a compromised renderer cared to name, which is a smaller leak than the one
being closed but is free to avoid — and a path that will not canonicalise cannot be in the set
anyway, since every member of it came out of `canonicalize`.

### The surface is closed, and that was checked rather than assumed

`pick_import_file` is the only dialog-returning command that hands a path to the renderer.
`note_export_to_file` returns one too, but only as an after-the-fact report of what it already
wrote, and nothing accepts it back. Every `#[tauri::command]` in
`apps/desktop/src-tauri/src` was enumerated and its parameters read: exactly three take a path —
`commands::import_openwhispr` (through `ImportRequest.path`), `media::media_import_audio` and
`media::media_import_video`. The other struct-shaped parameters (`NoteUpdateRequest`,
`CleanupSettings`, `EnterpriseConfig`, `Lists`) carry no path field; `CleanupSettings.self_hosted_url`
is a URL and goes through the network policy, not the filesystem.

### What this does not prove

Four unit tests assert the policy — an ungranted path is refused, a granted one survives two
imports, the oldest is evicted past the bound, and an ungranted credential-file path is refused
rather than read (including one reached by traversal that canonicalises onto it). `authorise` takes
no `AppHandle` precisely so those tests can exist; there is no Tauri command test harness in this
repo, so anything reachable only through a command is untested by construction. **Only a real call
against the running application proves the policy is consulted on this path**, and that has not
been run. It is item 1 under Task 12 in `BENCH_CHECK.md`, where it was already written before this
change and where it stays.

Dev mode was checked, not assumed: all four commands are stubbed in `devFixtures.ts`, which
intercepts at `invoke` and throws loudly on anything unstubbed, so no fixture reaches Rust and none
needs a fake grant.

Two doc corrections came along with it. `config/ui-surfaces.json`'s `Notes > Upload` note still
described "Audio, video and URL import" months after URL import was withdrawn — the parity gate
checks `surfaceId`, `component`, `built` and the `data-surface` marker, never the note prose, so
nothing was ever going to catch that. And the same entry said nothing about path restriction. Both
fixed.

---

## 2026-08-31 (KNEG-45) — 39 GGUFs get a digest, and a second standard gets a name

The GGUFs were the last download path with no digest. Closing that was declined twice on the same
grounds — 202 GB of downloading is not justified for a pin — and what changed is not the willingness
to spend it. It is that the pin was available for 36 HTTP requests.

### The lead, and what it took to believe it

The whisper-pins run on the same day found that
`https://huggingface.co/api/models/{repo}/tree/main?expand=true` returns `lfs.oid` per object, and
that this **is** the file's SHA-256 — git-lfs is content-addressed on exactly that hash, so the
pointer file committed in the repository is the digest. It agreed with all six measured whisper
digests. `ggml-large-v3.bin`'s row was read from the API while its 2.9 GB download was still in
flight and predicted the answer correctly.

That was one repository. The issue said so, in as many words: *"Nobody has compared an `lfs.oid`
from a bartowski GGUF repo against a real download."* So the first thing done here was not to write
code.

**36 tree calls covered all 39 objects.** Every one returned a well-formed 64-hex `lfs.oid`, and
every one's `lfs.size` matched the byte count already committed from `x-linked-size`. That second
agreement is worth naming precisely: it is one Hugging Face instrument confirming another, not an
independent measurement, because both are the same publisher's metadata.

**Then the smallest object from each of the seven publishing organisations was downloaded in full
and hashed** — bartowski, Qwen, mradermacher, ggml-org, google, unsloth, LiquidAI, 23.6 GiB — by
`tools/model-pin/pin.mjs`, hashing each stream on its way to disk and taking the length from `stat`.
All seven digests and all seven lengths matched the stated ones exactly. With whisper's six that is
**thirteen objects across eight organisations, 30.08 GiB, no disagreement.**

Those seven are not thrown away afterwards. They are committed as `sha256Measured: true`, which
makes the table honest about itself rather than uniform.

### The distinction, and where it is enforced

`LOCAL_LLM_SIZES` became `LOCAL_LLM_PINS` and every row now carries a digest. Seven carry a flag
saying this project hashed it.

**What a stated oid buys.** An exact length turns "roughly this file" into "a file of exactly
6,169,341,984 bytes", which was `KNEG-101` and was a real narrowing. It is still satisfied by *any*
6,169,341,984 bytes. A digest is satisfied by one file. Corrupted transfer, truncation that
happened to keep the length, accidental re-upload: all three now fail where they used to install.

**What it does not buy, and this is the sentence the implementation had to keep.** The oid comes
from the same host as the bytes. A compromised huggingface.co would serve both halves of the
comparison, and a digest committed to *this* repository is what defends against that. So the two
are not the same claim and must not be reported as one.

Three places enforce that, rather than one comment asking nicely:

- **The type.** `DigestCheck` has `Measured(String)` and `Stated(String)`, so nothing downstream can
  reach the digest without having been handed which kind it is. A `String` and a `bool` beside it
  would have let a caller read one and forget the other.
- **The wire.** `local_model::VerifiedBy` gained a fourth variant, `StatedDigest`, spelled
  `"stated-digest"`. The enum's own comment already predicted this: it exists because the frontend
  used to re-declare a `&'static str`'s values by hand, so *"a fourth state added here would have
  reached a UI that did not know about it"*. It duly arrived.
- **The screen.** *checksum verified* and *checksum verified against the publisher's own* are
  different rows in `verificationLabel`, and the row says it **before** the Install button as well
  as after, for the reason the size is said before it.

### On overturning a prohibition

The comment being replaced said, in bold: *"do not add a `sha256` here on the strength of it. A
digest belongs to bytes somebody actually hashed."* Written by the same run that measured the
whisper pins, and it was right about what it was aimed at — the only alternative then on offer was
deriving a digest from one download, which pins whatever arrived while looking like a guarantee. It
still forbids that.

An oid is a different object. It is stated by the publisher *before* any transfer, which is what
makes it testable against one, and thirteen downloads is what testing it looked like. The rewritten
comment carries the old prohibition verbatim and says why this is not an instance of it, because a
rule that quietly disappears when it becomes inconvenient teaches the next reader nothing.

### The check costs a full read of the file

Verification is a second pass: `kw_sidecar::integrity::hash_file` reads the finished download back
off the disk. For `qwen3-32b-q4_k_m` that is 19.8 GB, so it runs under `spawn_blocking` and emits
its own progress state. `LocalLlmProgress::Verifying` exists because a progress line that stops
moving for a minute reads as a hang, not as work.

Two deliberate limits, recorded rather than implied:

- **Length first, then hash.** The length is free — it is already on the filesystem — and a
  truncated transfer is the common failure, so checking it first turns the common case into a
  specific message rather than a digest mismatch that could mean anything.
- **The check happens at install and is not repeated.** `installed: true` afterwards means the path
  exists. The whisper path *does* re-hash on every status call, and gets away with it because its
  largest model is 3 GB; re-reading 20 GB every time a settings screen opens is not a trade worth
  making. A model corrupted after a successful install is therefore not caught. Filed as KNEG-107,
  not hidden.

### An install is as verified as its weakest half

Five models download a speculative-decoding drafter alongside the GGUF, from a different repository.
`VerifiedBy::weaker` picks the lower of the two verdicts for the `Done` event, so a drafter riding
in on a stated oid cannot be reported under the model's measured one. Its ordering is written as
explicit ranks rather than derived from `PartialOrd`, so reordering the variants for readability
cannot silently reorder the strengths.

`Done`'s `verifiedBy` is `Option`, and `None` means the files were already on disk and this install
checked nothing — which is honest where borrowing the previous install's claim would not be.

### `--verify-stated`, and why it is a second verb

`tools/model-pin/pin.mjs --verify` downloads and re-hashes. It cannot cover these 39 objects for the
reason they went unpinned for so long. `--verify-stated` re-reads the trees instead: 36 requests, no
bodies, a few seconds, and it fails if any digest or length has moved. The weekly `pin-verify.yml`
runs it beside the two download-based checks.

Two commands rather than one flag, because a tool that prints "verified" must never be vague about
which of those it did. Its success line says how many of the objects it checked are pinned to bytes
hashed here — the ones it is *not* the right instrument for.

For the thirty-two stated pins this is the same standard they were taken at, so agreement means
"unchanged". For the seven measured ones it is strictly weaker, and the test's doc comment says so:
agreement means Hugging Face has not re-uploaded, not that the bytes are still what we hashed. Only
a download proves the second thing.

### Two counts are asserted rather than assumed

`every_local_model_carries_a_digest_the_installer_will_check` asserts 39 objects, zero with no
digest, and **exactly seven** measured. The last of those is not a lower bound on purpose. If it
moves, either somebody measured more — good, and the number should follow in the same commit — or a
pin was quietly downgraded, which is the thing worth catching. The same shape asserts 39 in
`pin.test.mjs`, so a pin dropped from `LOCAL_LLM_PINS` cannot make `--verify-stated` check 38
objects and pass.

---

## 2026-08-31 (KNEG-100/102) — a ceiling that lied, four unbounded downloads, and 21 measured archives

`f17a5fd` added two real protections and left them almost unused.
`NetworkPolicy::download_to_file_bounded` refuses a body larger than a pinned length before it
reaches disk; `kw_sidecar::extract_with_limit` caps what an archive may write when it is unpacked.
One caller used either. This pass made both of them true everywhere they belong, and found that
the extraction half had never worked for four of its five archive kinds.

### The ceiling applied to one archive kind in five (KNEG-102)

`extract_with_limit`'s doc comment said "refusing to write more than `max_bytes` in total". Its
body threaded `max_bytes` to `TarBz2` alone; `Zip`, `TarGz`, `TarXz` and `Bare` took the argument
and discarded it. **A ceiling that lies is worse than no ceiling** — it is the difference between a
caller knowing it is unprotected and a caller believing it is protected. Nothing was exposed by
it, because the only caller passing a limit installs `.tar.bz2` models. Luck, not design, and the
luck would have run out at the next commit, which threads limits through the sidecar path — `zip`
on Windows, `tar.xz` on Linux.

**The header rule is not enough, and this was read rather than assumed.** The `TarBz2` path
accumulated `entry.size()` and refused before writing. That is exact for a tar: `tar::Entry`'s
reader is an `io::Take` over the same header size (`tar-0.4.46/src/entry.rs`, `EntryIo::Data`), so
the bytes cannot disagree with the header. A zip entry's reader is a decompressor over
`take(compressed_size)` (`zip-2.4.2/src/read.rs`, `make_reader`), and nothing in it holds the
inflated output to the `uncompressed_size` the central directory declares.
`a_zip_whose_header_understates_its_output_is_refused_on_the_real_bytes` patches a one-entry zip's
declared size down to 1 for a 1 MiB payload; the crate hands over all 1 MiB. A header-only ceiling
would have passed it, which is precisely the decompression bomb the ceiling exists for. So the
copy is now bounded on the bytes that arrive as well, with the declared-size check kept in front of
it as the fast refusal that stops an oversized entry landing at all.

**A refusal takes back what it wrote.** `extract_tar_limited` returned `Err` and left every file it
had already written; so did `extract_tar` and `extract_zip` on a corrupt entry or an unsafe name. A
half-unpacked directory can look convincing to a later `locate`. The rollback removes its own files
and then its own directories, deepest first, with `remove_dir` rather than `remove_dir_all` — and
never `dest`, because for `sherpa_models::install` `dest` is the models *root* and clearing it would
take every other installed model.

**`InstallError::ExtractedTooLarge`** replaces `Archive { detail: "extracted data exceeds …" }`.
`tests/tar_bz2_fixture.rs` had asserted on that substring because there was nothing better to match
on.

One test per `ArchiveKind`, every one overflowing on a *later* entry. The only ceiling test that
existed used `max_bytes = 1`, which trips on the first entry and so proves nothing about partial
output. Both sides of the boundary are asserted: an extraction fitting its ceiling exactly succeeds
for all four archive kinds, because a bound that refuses a legitimate install is worse than no
bound.

### The extracted footprint of all 21 published sidecar targets, measured

`Asset` had `sha256` and `sizeBytes` and no analogue of the `maxExtractedSizeBytes` the sherpa
models carry, so there were two options: add the field and measure 21 targets, or bound the
download only and say the extraction ceiling for sidecars is deliberately absent. The measurement
turned out cheap — 1.85 GB in total, and `tools/sidecar-pin/pin.mjs` already downloads and indexes
every archive on `--verify` — so the field exists and is mandatory.

`node tools/sidecar-pin/pin.mjs --extracted --write`, 2026-08-31. Download and extracted are
measurements; ceiling is the judgement.

| target | archive | download | extracted | ceiling |
| --- | --- | --- | --- | --- |
| whisper/windows-x86_64 | zip | 8,361,840 | 21,213,696 | 27,262,976 |
| whisper/linux-x86_64 | tar-gz | 9,503,425 | 24,527,510 | 31,457,280 |
| whisper/linux-aarch64 | tar-gz | 4,572,294 | 10,587,134 | 13,631,488 |
| llama/windows-x86_64 | zip | 17,287,711 | 44,587,872 | 56,623,104 |
| llama/linux-x86_64 | tar-gz | 15,646,315 | 39,784,422 | 50,331,648 |
| llama/linux-aarch64 | tar-gz | 12,665,261 | 31,133,878 | 39,845,888 |
| llama/macos-x86_64 | tar-gz | 11,280,969 | 27,561,662 | 34,603,008 |
| llama/macos-aarch64 | tar-gz | 10,978,734 | 27,450,446 | 34,603,008 |
| ffmpeg/windows-x86_64 | zip | 145,349,121 | 343,922,176 | 429,916,160 |
| ffmpeg/linux-x86_64 | tar-xz | 111,679,252 | 348,784,736 | 436,207,616 |
| ffmpeg/linux-aarch64 | tar-xz | 96,564,568 | 283,579,184 | 355,467,264 |
| yt-dlp/windows-x86_64 | bare | 17,840,399 | 17,840,399 | 17,840,399 |
| yt-dlp/linux-x86_64 | bare | 40,446,224 | 40,446,224 | 40,446,224 |
| yt-dlp/linux-aarch64 | bare | 40,167,448 | 40,167,448 | 40,167,448 |
| yt-dlp/macos-x86_64 | bare | 37,146,048 | 37,146,048 | 37,146,048 |
| yt-dlp/macos-aarch64 | bare | 37,146,048 | 37,146,048 | 37,146,048 |
| sherpa/windows-x86_64 | tar-bz2 | 24,378,225 | 43,812,352 | 55,574,528 |
| sherpa/linux-x86_64 | tar-bz2 | 384,752,102 | 1,249,715,232 | 1,562,378,240 |
| sherpa/linux-aarch64 | tar-bz2 | 337,820,882 | 920,336,696 | 1,151,336,448 |
| sherpa/macos-x86_64 | tar-bz2 | 254,918,578 | 879,667,336 | 1,099,956,224 |
| sherpa/macos-aarch64 | tar-bz2 | 231,438,974 | 777,705,592 | 973,078,528 |

Four things this measurement decided rather than assumed:

- **Extracted is not the archive's uncompressed size.** Only the entries inside the payload
  directory are ever written — the Windows whisper archive carries `main.exe`, `SDL2.dll`,
  `llama.dll` and a dozen test binaries that are not — so `extractedBytes` mirrors
  `install::payload_relative` rather than summing the index.
- **For zip, the entries are inflated rather than read off the central directory.** Same reason the
  Rust now bounds on real bytes: `uncompressed_size` is a declaration. The tool decompresses each
  payload entry with `inflateRawSync` and counts what comes out.
- **The ceiling is one stated rule, not 21 numbers.** Measured + 25%, rounded up to the next MiB
  (`withHeadroom`). The drift a pinned asset sees between upstream builds is a few percent — a
  rebuilt binary, an added microarchitecture DLL — not a multiple, and 25% absorbs that while still
  refusing something that decompresses to several times what was measured. It errs high because
  being wrong high costs protection and being wrong low refuses a legitimate install, which is the
  trade `SHERPA_PINS` already records for its two hand-chosen figures.
- **`Bare`'s ceiling is its download size, with no headroom.** The file is copied verbatim, so
  headroom above a digest-pinned length could only ever be reached by a different file.

**All 21 digests were re-verified in the same run**, as a side effect worth stating: the tool
refuses to measure an asset whose digest has moved, because a ceiling derived from unpinned bytes
pins the wrong number. `--verify` now re-measures the extracted total too and fails when a
committed ceiling has become too tight to install through — the failure mode
`docs/PLATFORM_SUPPORT.md` records for the sherpa `sizeMb` figure, caught by CI rather than by a
user.

### Which downloads got bounded, and which deliberately did not

| caller | download | extraction |
| --- | --- | --- |
| `sidecar::install` — all 21 targets | `sizeBytes`, exact | `maxExtractedSizeBytes` |
| `sherpa_models::install` | already bounded | already limited |
| `models::install` — whisper `.bin` | `expectedSizeBytes` where `Integrity::Pinned` | n/a |
| `local_llm::download_gguf` | `sizeBytes` where `SizeCheck::Exact` | n/a |
| `examples/sherpa_probe.rs` | `expectedSizeBytes` | `maxExtractedSizeBytes` |

**A bound is only ever an exact figure**, and that is the whole reason two of those rows are
conditional. A bound refuses a transfer mid-flight, so a *declared* length cannot be one: the true
length may be on either side of it. Bounding `SizeCheck::Approximate` would have reintroduced
KNEG-101 in a faster form — eighteen GGUFs were declared 6.7–7.9% larger than the file Hugging Face
serves, and the version of `download_gguf` that compared the *finished* download against that
figure deleted all eighteen after 101 GB of successful transfer. A bound would have refused them
sooner and just as wrongly. So `Approximate` and `Integrity::Size` keep their tolerance as a
post-download check and never become bounds, which leaves an unmeasured model degraded rather than
broken. Every shipped model reaches the bounded arm today.

### The partial-file leak, and the first test for `DownloadTooLarge`

`download_to_file_bounded`'s streaming refusal returned after creating and partially filling the
file, with no `remove_file`. So did the mid-stream write error and the chunk error from a dropped
connection — three returns, no cleanup, and a truncated body left on disk that a later
`is_installed` check finds as convincing as a refused one. The body half is now
`stream_to_file`, so every failure leaves through one cleanup path in the caller.

**`PolicyError::DownloadTooLarge` had no test anywhere.** It was written, shipped, used by one
caller and never exercised. There are four now, against a minimal loopback HTTP server in the test
module (extending the `respond_once` helper `get_text` already used, rather than adding a mock
harness). They cover something the one-line doc comment had not recorded: **the `content-length`
pre-check often does not run at all.** It uses `Option::zip`, so a response with no
`content-length` — a chunked one, or HTTP/1.0 with `Connection: close` — skips it entirely, and the
running total in the streaming loop is the check that actually guarantees the bound. That is
correct behaviour, since a header is a claim and a body may be longer than its own header says, but
it was undocumented. The tests exercise both shapes, plus a body exactly on its limit (the normal
case, since every real bound is an exact pinned length) and an unbounded download of any length.

### Verified against the real network

- `cargo test -p kw-sidecar --test real_install -- --ignored` — the pinned whisper sidecar
  downloads, verifies and unpacks under both new bounds.
- `cargo test -p kw-transcription-core --test local_dictation -- --ignored` — a real whisper model
  under the exact-size download bound.
- `cargo test -p kw-transcription-core --test subtitle_export -- --ignored` — a sidecar *and* a
  model, so both new bounds in one run.

None of them refused a legitimate install, which was the risk worth checking: a bound that refuses
one is worse than no bound.

### Filed, not fixed

**KNEG-106** (Low). `extract_tar_xz` decompressed the whole tar into a `Vec<u8>` before looking at
an entry, because `lzma-rs` decompresses into a `Write` and exposes no streaming `Read`.
`max_bytes` bounds what reaches `dest` and could not bound that buffer, so a `.tar.xz` was a memory
bomb whatever ceiling was passed — independent of the output ceiling it now honours. Behind a
mandatory digest, and the only pinned `tar-xz` assets are ffmpeg on Linux at ~100 MB compressed and
~300 MB extracted. **Fixed the same day**: a bounded channel between the decompressor and the tar
reader. See the KNEG-106 entry above — the interesting part is that the first test for the bound
passed against a sixty-four-gigabyte queue.

---

## 2026-08-31 (whisper pins) — six digests measured, and three pin tables become one

`crates/transcription-core/src/models.rs` had carried, since it was written, a `PINNED` table with
one row in it and a paragraph explaining why: pinning the other five whisper models "means
downloading roughly seven gigabytes once, which is a job for a tool rather than something to invent
digests for here". `tools/model-pin/pin.mjs` (`11f0846`) is that tool. It downloaded all six.

### What was measured

Every model fetched in full from its registry `downloadUrl` on 2026-08-31, hashed as the stream
passed through to a file, and its length taken from `stat` afterwards — never from
`content-length`, because a truncated body agrees with its own header. 6,966,596,962 bytes in one
run — 6.49 GiB, 14 min 33 s of transfer on this connection.

| model | file | SHA-256 | bytes | baseline declared |
| --- | --- | --- | --- | --- |
| `tiny` | `ggml-tiny.bin` | `be07e048…c6e1b21` | 77,691,713 | 78,000,000 |
| `base` | `ggml-base.bin` | `60ed5bc3…ba2efe` | 147,951,465 | 148,000,000 |
| `small` | `ggml-small.bin` | `1be3a9b2…ea987b` | 487,601,967 | 488,000,000 |
| `medium` | `ggml-medium.bin` | `6c14d5ad…156208` | 1,533,763,059 | 1,570,000,000 |
| `turbo` | `ggml-large-v3-turbo.bin` | `1fc70f77…e2bc69` | 1,624,555,275 | 1,670,000,000 |
| `large` | `ggml-large-v3.bin` | `64d182b4…94d1e2` | 3,095,033,483 | 3,140,000,000 |

**`tiny` is the control, and it reproduced.** `be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21`
and 77,691,713 bytes, identical to what `models.rs` had committed — a different day, a different
tool, the same answer. That validates the measurement of the other five in the same run as it
re-validates the one pin that already existed.

The declared column is the shape of the whole baseline: every figure is a round million or ten
million, and every one of the six is **high**, by 0.033% to 2.797% — `base` by 48,535 bytes and
`turbo` by 45,444,725. None of them is a measurement of anything, and the errors do not follow a
constant, so they are not the unit-confusion bug `98e836b` found in the GGUF sizes. They are
somebody rounding up.

### An unpredicted finding: Hugging Face states the SHA-256, and it is right

`https://huggingface.co/api/models/ggerganov/whisper.cpp/tree/main?expand=true` returns, per LFS
object, `lfs.oid` — which *is* the file's SHA-256, because that is what git-lfs keys on. It agreed
with all six measured digests and all six measured lengths, on the nose. `large`'s row was read
from that API while its 2.9 GB download was still in flight and predicted
`64d182b4…94d1e2` / 3,095,033,483 correctly.

That matters for **KNEG-45**, now re-scoped to only the 35 local-LLM GGUFs, declined on the
grounds that 202 GB of downloading is not justified for a pin. There is a third option between
"size only" and "bytes we hashed": pin the publisher's stated `lfs.oid`. The cost is **one API call
per repository**, not per object — `tree/main` lists every file in the repo with its oid and size in
one response — so the whole 39-object table would be a handful of requests rather than 202 GB.
Six-for-six agreement over 6.9 GB is real evidence that the metadata is accurate.

**It is a weaker standard and the distinction has to survive.** The oid is a claim by
huggingface.co, delivered over the same channel as the bytes. It defends against a corrupted
transfer and against an accidental re-upload; it does not defend against a compromised
huggingface.co, because one host would serve both halves. A digest measured once and committed to
*this* repository does defend against that, which is the whole reason `config/sidecars.json`'s rule
is "a pin is only a pin if it was measured". So: a middle tier, strictly better than a length,
strictly worse than the strong pin, and it must never be written into a table that reads as the
strong pin.

**Recorded as a lead on KNEG-45 rather than done here — and taken up later the same day.** The lead
held: see the KNEG-45 entry above, where seven objects across seven publishing organisations were
downloaded and hashed to test it before any of it was written down as a pin.

### Three pin tables became one

The pins are `WHISPER_PINS` in `tools/integration-audit/generate-provider-registry.mjs`, beside
`SHERPA_PINS` and `LOCAL_LLM_SIZES` — **not** an extended `PINNED` array in Rust, which is the
architecture call worth recording. The argument is not tidiness. Three model families keeping their
pins in three different places had already produced a concrete absurdity: `model-pin --verify` had
to **parse Rust from Node** to check the whisper half, with a regex over
`const PINNED: [(&str, &str, u64); N]` and the array's declared arity as a checksum on the parse.
That reader worked. It should never have needed to exist.

`models.rs` now reads `sha256` and `expected_size_bytes` off
`kw_provider_registry::WhisperModel`, the generator emits both, and `--verify` reads one generated
JSON document for all eight pins. `parseWhisperPins` and its five tests are deleted. They were
written as a deliberate tripwire for exactly this migration and they fired once, which is what a
tripwire is for — retiring them is the outcome, not a loss of coverage.

### `expectedSizeBytes` is the measured length now, and one claim was false

The registry's `expectedSizeBytes` for whisper is overridden with the measured figure rather than a
tolerance being kept around the rounded one. That was free, and the reason `models.rs` gave for not
doing it was wrong:

> A tolerance rather than a rewrite of the registry, because the rounded figures are also what the
> settings screen shows.

**They are not.** `apps/desktop/src/views/SettingsTranscription.tsx` formats `sizeMb` — `142 MB`,
`466 MB` — and never reads `expectedSizeBytes` at all. The field has exactly one consumer in the
repository, `models::integrity_for`. `sizeMb` stays rounded, because it is a display estimate and
showing a user 77,691,713 helps nobody. The false sentence is corrected where it was written.

### What is kept although nothing reaches it

`Integrity::Size`, `Verification::SizeOnly`, `SIZE_TOLERANCE` and `within_tolerance` all survive
with no shipped model reaching them, and each says so where it is defined so nobody removes it as
dead code. The reason is the same one `98e836b` gave for keeping `SizeCheck::Approximate`: a model
added to the baseline before somebody spends the bandwidth to hash it — 2.9 GB, for a model the
size of `large` — should be **degraded, not broken**.
A tolerance is the right answer to a rounded number and the wrong answer to a measured one, and the
unmeasured model is the only case that still has a rounded number.

`within_tolerance`'s last consumer outside this module is `SizeCheck::Approximate` in
`apps/desktop/src-tauri/src/local_llm.rs`; that is named in the doc comment, because "who still
calls this" is the question a future reader will actually have.

Note the asymmetry with sherpa, which is deliberate: an unpinned sherpa model cannot be installed
at all (`MissingPin`), an unpinned whisper model degrades to a size check. A sherpa model is an
archive that gets extracted; a whisper model is one file from a repository whose five siblings were
hashed.

### What this costs, and it is the same cost twice

All six URLs are `resolve/main` — a branch pointer, not a revision. Until today a re-upload had to
move a file by more than 5% to be noticed; now any change at all fails the install with an
integrity error and the user cannot proceed. Identical to the trade `98e836b` took for the GGUF
sizes, taken again for the same reason: a silent pass means running weights nobody here measured.
See **KNEG-103**, which now covers a digest-exact surface rather than a size-exact one.

> **Superseded the same day.** KNEG-103 landed: all six URLs name commit `5359861c` instead of
> `main`, so this cost is no longer paid — a publisher pushing cannot move the bytes these digests
> describe. The entry above is what was true when the digests were measured, and the reasoning that
> made pinning the revision the obvious next thing. See the KNEG-103 entry at the top of this file.

What notices first is `node tools/model-pin/pin.mjs --verify`, and its weekly cost went from about
1 GB to about **8 GB** — two Parakeet archives plus six whisper models rather than one.
`.github/workflows/pin-verify.yml` carries the new figure and the `--only` escape hatch if the
runtime ever becomes the problem.

### Verified, and not

**Verified:** all six digests reproduced by a full download; `tiny` reproduced its previously
committed pin; a registry invariant test asserts every whisper model resolves to `Integrity::Pinned`
(mirroring `every_installable_sherpa_model_has_integrity_and_size_limits`); `cargo test -p
kw-transcription-core --test local_dictation -- --ignored` drives the real round trip — download,
`Verification::DigestMatched`, server start, correct transcript — through the shipped `install` path
now that it reads the registry instead of a table in its own file.

**Not verified:** nobody has installed any of the five newly pinned models through the application.
`BENCH_CHECK.md` carries that as a by-hand item and **KNEG-105** as an issue, with `base` named
because it is the recommended model and the one a new user presses first. The two
Parakeet digests were also re-measured today by full download (464.6 MiB and 478.1 MiB) and both
matched; that upgrades `SHERPA_PINS`' comment from "the byte counts agree with `content-length`" to
"both digests and both lengths came off the disk", and its comment now says what that does and does
not prove.

---

## 2026-08-31 (local-LLM sizes) — eighteen models nobody could install, and 2^30 ÷ 10^9

`download_gguf` deletes a finished download whose length is more than 5% from the size the registry
declares. Eighteen of the thirty-five local text-generation models were declared 6.7–7.9% larger
than the file Hugging Face actually serves, so **all eighteen downloaded in full, failed the check,
and were deleted** — 101 GB of successful transfer, thrown away on arrival, `Qwen3 32B` and
`GPT-OSS 20B` among them. There was no way to install any of them, and nothing in the failure said
so: the message blamed the download for being the wrong length, which reads like a bad connection
and invites a retry that fails identically.

### Where the numbers came from, measured

Every model and every speculative-decoding drafter — 39 objects, 202 GB — was HEADed at
`https://huggingface.co/{hfRepo}/resolve/main/{fileName}` on 2026-08-31 and its `x-linked-size`
read. Hugging Face serves these through LFS: a `resolve/main` request answers **302**, so
`content-length` describes the redirect and `x-linked-size` carries the object. Four were
cross-checked against `/api/models/{hfRepo}/tree/main`, which reports the same `lfs.size`. The
whole set was then re-measured a second time through the shipped code path — `NetworkPolicy` under
`UserTriggeredModelDownload` — and agreed on all 39.

Pass and fail below are recomputed with `within_tolerance`'s own arithmetic, which divides by
`expected`, not by `actual`. That matters: it is the difference between reporting what is wrong and
reporting what the shipped code rejects.

### The root cause is a base-2 conversion of a base-10 label

Upstream's baseline carries a human `size` label per model — `"5.0GB"`, `"2.5GB"` — and its
`sizeBytes` was produced from that label by multiplying by **2^30 instead of 10^9**.

| model | declared | measured | declared ÷ measured | declared ÷ 2^30 | measured ÷ 10^9 |
| --- | --- | --- | --- | --- | --- |
| `qwen3-8b-q4_k_m` | 5 402 263 552 | 5 027 783 488 | 1.074482 | 5.0313 | 5.0278 |
| `qwen3-8b-q5_k_m` | 6 281 625 600 | 5 851 112 224 | 1.073578 | 5.8502 | 5.8511 |
| `qwen3-4b-q4_k_m` | 2 684 354 560 | 2 497 280 256 | 1.074911 | 2.5000 | 2.4973 |
| `qwen3-1.7b-q8_0` | 1 965 555 712 | 1 834 426 016 | 1.071483 | 1.8306 | 1.8344 |
| `qwen3-32b-q4_k_m` | 21 260 251 955 | 19 762 149 024 | 1.075807 | 19.8002 | 19.7621 |
| `qwen2.5-1.5b-instruct-q5_k_m` | 1 395 864 371 | 1 285 494 304 | 1.085858 | 1.3000 | 1.2855 |
| `qwen2.5-3b-instruct-q5_k_m` | 2 620 055 552 | 2 438 740 384 | 1.074348 | 2.4401 | 2.4387 |
| `qwen2.5-7b-instruct-q4_k_m` | 5 025 128 858 | 4 683 074 240 | 1.073041 | 4.6800 | 4.6831 |
| `qwen2.5-7b-instruct-q5_k_m` | 5 841 530 470 | 5 444 831 936 | 1.072858 | 5.4403 | 5.4448 |
| `mistral-nemo-12b-instruct-q4_k_m` | 8 028 321 792 | 7 477 208 320 | 1.073706 | 7.4770 | 7.4772 |
| `mistral-7b-instruct-v0.3-q4_k_m` | 4 692 635 648 | 4 372 812 000 | 1.073139 | 4.3704 | 4.3728 |
| `mistral-7b-instruct-v0.3-q5_k_m` | 5 519 900 672 | 5 136 175 328 | 1.074710 | 5.1408 | 5.1362 |
| `llama-3.2-3b-instruct-q4_k_m` | 2 168 958 976 | 2 019 377 696 | 1.074073 | 2.0200 | 2.0194 |
| `llama-3.1-8b-instruct-q4_k_m` | 5 282 717 696 | 4 920 739 232 | 1.073562 | 4.9199 | 4.9207 |
| `gpt-oss-20b-mxfp4` | 12 999 763 968 | 12 109 566 624 | 1.073512 | 12.1070 | 12.1096 |
| `gemma-4-e4b-it-q4_k_m` | 5 812 137 984 | 5 405 170 144 | 1.075292 | 5.4130 | 5.4052 |
| `gemma-4-e2b-it-q4_k_m` | 3 711 514 624 | 3 462 680 032 | 1.071862 | 3.4566 | 3.4627 |
| `gemma-3-12b-it-q4_k_m` | 7 847 624 704 | 7 300 575 264 | 1.074932 | 7.3087 | 7.3006 |

**2^30 ÷ 10^9 = 1.073741824.** Over the eighteen, declared ÷ measured has mean **1.074509**. Drop
the single outlier and the seventeen remaining average **1.073841**, none of them more than
**0.210%** from the constant. Read the last two columns as one claim: *declared ÷ 2^30* recovers the
model's size **in base-10 GB**, to three or four significant figures, in every row.

Two rows are the label itself, unrounded:

- `qwen3-4b-q4_k_m` — label `"2.5GB"`, declared `2 684 354 560` = **2.5 × 2^30**, exactly.
- `qwen2.5-1.5b-instruct-q5_k_m` — label `"1.3GB"`, declared `1 395 864 371` = **1.3 × 2^30**,
  exactly. This is the outlier at 7.907%, and it is worse than the rest for a second reason on top
  of the first: the file is 1.2855 GB, so the two-significant-figure label was already 1.1% high
  before anything multiplied it. Label rounding and unit confusion compound.

The other sixteen are finer-grained than their own two-digit label — `qwen3-8b-q4_k_m` declares
5.0313 GiB against a `"5.0GB"` label — so whatever the conversion consumed was a more precise GB
figure than the label the baseline kept. The residual ±0.2% is not explained; the 7.4% is, and the
7.4% is the whole defect.

### Three things the measurement found that the theory did not predict

**1. `llama-3.2-1b-instruct-q4_k_m` is the same bug one unit down, and it passes.** Declared
`847 249 408` = **808 × 2^20**, exactly; the file is `807 694 464` bytes = 807.69 MB, which rounds
to the 808 **MB** that was then read as MiB. Its ratio is 1.048973 against 2^20 ÷ 10^6 =
**1.048576**. It survives only because the mebibyte error is 4.86% and the gibibyte error is 7.37%,
and the tolerance is 5%. One model was saved by its own smallness.

**2. The three `qwen3.5-*` models are not this bug at all, and one of them passes by 5.6 KB.** Their
declared sizes are *smaller* than the real files (95.2–95.5% of them) and are not round in any base
— `5 889 811 552` for a 6 169 341 984-byte file. Each bartowski repo has a single upload
(2026-05-19, checked through `/api/models/…/commits/main`), so these are not stale figures from a
re-upload; where they came from is unknown. What matters is the margin:

| model | difference ÷ expected | margin below the 5% tolerance |
| --- | --- | --- |
| `qwen3.5-2b-q4_k_m` | **4.9958%** | 0.0042 pp — about 5.6 KB |
| `qwen3.5-4b-q4_k_m` | 4.9198% | 0.08 pp |
| `qwen3.5-9b-q4_k_m` | 4.7460% | 0.25 pp |

`qwen3.5-9b-q4_k_m` is the **recommended** model, and `qwen3.5-2b-q4_k_m` was four kilobytes of
upstream metadata away from joining the eighteen. The set that worked was not a set that was right;
it was a set that had not yet drifted far enough to fail.

**3. Two models that looked exact were 2 240 bytes short — the same 2 240 in both.**
`gemma-4-31b-it-q4_k_m` and `gemma-4-26b-a4b-it-q4_k_m`, both from bartowski, both declared exactly
2 240 bytes below what is served. That is the re-upload case, live and already present: two files
whose length changed by a GGUF metadata block since somebody measured them. It is the reason the
`#[ignore]`d re-measurement test below is not a formality.

**Eleven** models matched to the byte, and all four drafters — fifteen of the thirty-nine objects.
They are the ones somebody actually measured: all five `lfm2*`, the four Gemma QAT builds with their
drafters, `gemma-3-4b-it` and `gemma-3-1b-it`. Six of the seventeen that *passed* did not match: the
three `qwen3.5-*`, `llama-3.2-1b`, and the two bartowski Gemma 4 builds 2 240 bytes adrift.

### The fix, and where the numbers now live

`LOCAL_LLM_SIZES` in `tools/integration-audit/generate-provider-registry.mjs`, keyed by model id,
carrying `sizeBytes` and `draftSizeBytes` for all 35 models and all 4 drafters. It sits beside
`SHERPA_PINS` and is built on the same reasoning: `config/upstream-integration-baseline.json` is a
frozen record of what upstream had and must not gain fields we invented, and
`config/provider-registry.json` is generated, so a fix hand-edited into it dies at the next
regeneration — which is exactly what happened to the Parakeet digests in `f17a5fd`. A pin naming an
id the baseline does not have is a `die()`, as is a drafter pin on a model with no drafter.

**The pins are measurements, and they are the weaker kind.** `tools/sidecar-pin/pin.mjs` downloads
the asset and records bytes on disk, which is why it can also pin a SHA-256. These are what the
publisher's own object size says over HTTP. The difference is deliberate and it is a cost decision:
202 GB of downloading to confirm a length that Hugging Face already states is not justified for a
size pin. The doc comment says so in those words, so that nobody later reads the table as the
stronger thing and adds a `sha256` next to it. A digest belongs to bytes somebody hashed.

### Exact equality, and what it costs

The registry now carries `sizeMeasured` per model and per drafter, and `download_gguf` reads it
through a three-state `SizeCheck`: `Exact` for a measured size, `Approximate` (the old 5%) for a
declared one, `Unknown` for no size at all.

**Why exactness became safe:** a tolerance is the right answer to a *rounded* number and the wrong
answer to a *measured* one. Against a figure read from the publisher's own object size there is
nothing left to round — the file either has that many bytes or is not that file. The 5% window was
simultaneously wide enough to hide this defect for the seventeen models it passed and too narrow to
install the eighteen it failed, which is what a tolerance around an unmeasured number always is.

**What it costs:** an upstream re-upload now fails the install loudly instead of passing silently.
Taken deliberately — a silent pass means the user runs weights nobody here ever measured — and it
is not a hypothetical cost, because two models had already drifted by 2 240 bytes. What keeps it
survivable is `the_measured_sizes_still_match_hugging_face` in `local_llm.rs`: `#[ignore]`d, 39 HEAD
requests, no bodies, about six seconds, and it re-measures the committed table against the live
files. It passed on all 39 at the time of writing. The `Approximate` arm is kept, and a unit test
asserts nothing is currently using it — a model added before it is measured is degraded rather than
broken, and the test is where that shows up.

### Not verified

Nobody has installed any of the eighteen through the screen. `BENCH_CHECK.md` carries it as a
by-hand item. What is verified is that the sizes are right and that the check accepts them; that a
20 GB download completes, renames, and starts a server is a separate claim and this run did not make
it.

---

## 2026-08-31 (KNEG-96) — the half of the self-heal that was missing

`d489471` taught `LocalTranscription::start_model` to health-check the port it had recorded and
start a new server when nothing answered. That was the right fix for the symptom in front of it —
pressing *Use for dictation* handed back a dead port — and it left the two paths that actually
carry audio untouched. `RouteTranscriber::local_server` and `MediaImportRuntime::local_server` were
the same three lines: read the recorded port, build a `WhisperServer`, send. Neither asked whether
anything was listening.

So the state that produced four failed dictations on 2026-08-28 was half cured. The button
recovered; **dictating did not, and neither did importing** — the worse of the two, because a
dictation whose server has died can be spoken again and an imported recording generally cannot.
Both now health-check the recorded port and start the remembered model again when it has gone.

### `Revive` is a trait, and the choice is the persisted one

- **A trait rather than an `Arc<LocalTranscription>`.** The two callers want one sentence's worth of
  capability — *start the remembered model, tell me the port* — and handing them the runtime would
  hand them the installer, the reaper and the catalogue as well.
- **Required, not `Option`.** A build that forgets to supply it is a build where a lost server
  silently stays lost, which is the defect. Requiring the field makes forgetting a compile error —
  the lesson from `key_ref: None`, which flipped a manifest entry to reachable for a client that
  could not connect.
- **The *persisted* model, not the in-memory one.** By the time anyone revives, `forget_port` has
  cleared the model alongside the port; they go together on purpose, since a model name with no
  server behind it is the claim that caused this. The persisted choice is also what startup reads,
  so reviving mid-dictation cannot bring back a different model than a relaunch would.
- **Three named states instead of `Option<u16>`.** `Recorded::{Absent, Live, Gone}`, produced by a
  pure `classify(recorded, answering)` so the reading can be tested without a server — the half that
  was wrong. "No server was ever started" and "a server was started and has gone" had been getting
  the same answer: the recorded number, handed to a transcription that then failed against it.

### A second defect, found by the fix rather than by a bench run

The local engine was being resolved on **every** dictation, before anything looked at the transport,
and the executor consults it only in its `Transport::Local` arm. Harmless-looking, and it meant a
sherpa model that was selected but could not be built failed a dictation **bound for a cloud
provider** — `local_engine` returns `Err` for that case and the `?` ran first. Resolving the engine
only for a local route fixes it and removes a loopback health check from every cloud dictation at
the same time. No test caught this because no test paired a broken local selection with a remote
transport.

### Written, not verified

The decision is covered by five new tests (1310 passing). The round trip is not: nothing here has
killed a real `whisper-server` and dictated. Listed as a by-hand item in `BENCH_CHECK.md`, and the
row that used to read *a dead local server now repairs itself* has been narrowed to the link the
2026-08-28 run actually exercised — the button.

**And the underlying question is still open.** Why the child exits unattended is not answered by
any of this; it has now happened three times. KNEG-96 carries the two candidates and how to tell
them apart from the first two log lines after a restart.

## 2026-08-27 (merged security fixes) — six fixes, a stronger merge, and a broken gate

Six fixes landed on `dev` through PRs #7–#14, authored outside this session, after the plan closed:
a Corti environment forced to one DNS label, a Vertex region forced to one DNS label with the
service account's `token_uri` pinned rather than trusted from pasted JSON, Microsoft Graph
pagination URLs constrained and bearer requests pinned, Deepgram transcript memory bounded, and URL
import disabled. Three things about that are worth keeping.

**The Microsoft merge produced a better check than either branch wrote.** PRs #10 and #11 both
edited `microsoft.rs` — one constraining pagination URLs, one pinning bearer requests — and the
merge resolved to a single `validate_graph_url(url, expected_path)` that pins scheme, host, port,
credentials, fragment *and* the exact path. That is strictly stronger than either branch alone. It
also orphaned `use url::Url` and `const GRAPH_HOST`, which belonged to the version that lost. Both
were dropped rather than wired back in: restoring them would have meant restoring the weaker check
beside the stronger one.

**The merged branch did not pass `cargo clippy --workspace --all-targets -- -D warnings`.** Fourteen
errors: the two orphans above, one `field_reassign_with_default` in `deepgram_socket.rs`, and eleven
dead-code errors in `media.rs`. None is a defect in the fixes; all are the residue of removing a
call site without removing what only that call site used. This is the first concrete cost of the
CI triggers being commented out — every one of the fourteen would have been caught before merge.
The pause is deliberate and still stands; the cost is now measured rather than hypothetical. See
"2026-08-25 (CI paused)" below.

**Dormant, not deleted, and said so in the type system.** Withdrawing `media_import_url` as a Tauri
command made the entire URL-import path unreachable: two IPC enum variants, two error variants,
three constants, four functions. The manifest note says the backend is dormant, so deleting it
would have contradicted the record — and re-opening the entry point would then be a rewrite rather
than a decision. Each item carries `#[allow(dead_code)]` pointing at a new `URL IMPORT, DORMANT`
section in `media.rs`'s module doc, which states why the entry point is closed and what re-opening
requires: every address yt-dlp resolves, every redirect it follows, and every request its
extractors generate checked against the policy. Their tests still run. The alternative considered
and rejected was a blanket `#![allow(dead_code)]` on the module, which would have hidden genuinely
abandoned code alongside the deliberately dormant kind.

**What the withdrawal costs, stated plainly:** `media:url-import` goes `reachable: false`, parity
goes 7 unreachable to 8, and this is the first entry on that list that regressed rather than never
having been built. Audio and video import are untouched — they take a local path and fetch nothing.
Task 20's own finding pointed here: the up-front literal-host check was never the weak part, and no
amount of strengthening it reaches DNS rebinding or an extractor's own requests.

## 2026-08-27 (Task 19) — reconciling the record, and the ACTIVE-status audit

The last task in the plan, and the only one whose entire job was making claims match reality
rather than building anything. `docs/BUILD_PLAN.md` opens with its own rule — "when something here
becomes false, edit it rather than appending a note" — and this entry, plus the edits it describes,
is that rule applied to the whole run's paper trail: `BUILD_PLAN.md`, `INTEGRATION_PARITY.md`,
`COMPLETION_PLAN.md`, `PLATFORM_SUPPORT.md`, `PORTING_MATRIX.md`, `BENCH_CHECK.md`, this file, and
several source doc comments and manifest fields.

### The ACTIVE-status audit

`verify-parity.mjs` defines `ACTIVE` as "statuses that assert a working implementation exists" —
`ported`, `ported-with-new-rust-implementation`, `rewired-to-direct-byok`,
`replaced-with-local-equivalent`, `platform-specific`. Mid-run, a reviewer found
`meeting-stream:tinfoil-realtime` carrying `status: "ported"` while no Tinfoil meeting client exists
anywhere in the codebase — the identical pattern `platform:aec` had just been corrected for — and
the controller ruled that Task 19 would audit every `ACTIVE` entry rather than patch that one
instance by hand.

The audit read all 80 entries whose status was in `ACTIVE` at the start of this task (three text
passes over the whole manifest for language like "implements none", "declined", "refused",
"ClientNotImplemented", plus a manual check of every ambiguous case) and found three overclaims,
including the one already flagged:

| Entry | Was | Reality | Now |
| --- | --- | --- | --- |
| `meeting-stream:tinfoil-realtime` | `ported`, backend `crates/meeting-core` (a directory) | No Tinfoil meeting client exists; `capabilities.rs` declares only the refusal reason | `temporarily-deferred` |
| `transcription:tinfoil` | `rewired-to-direct-byok` | `clients::shape()` returns `TranscriptionError::ClientNotImplemented` for it; nothing else implements it | `temporarily-deferred` |
| `calendar:apple` | `platform-specific` | `kw_calendar_core::apple`'s own module doc: "deliberately unimplemented by anything in this crate" on every platform including macOS | `temporarily-deferred` |

Two entries that looked like plausible false positives on a text scan turned out not to be, and are
recorded here so the audit's negative results are visible too: `meeting-detect:facetime` is real,
`#[cfg(target_os = "macos")]`-gated detection code that runs and is tested when compiled for macOS —
platform-specific in the sense the status actually means, unlike `calendar:apple`'s "described,
implemented nowhere on any platform". `reasoning:tinfoil` (as opposed to the two entries above) has
a genuine OpenAI-compatible client in `reasoning-core` and is reachable; only the two entries whose
proposition requires an attested socket or an unattested-POST refusal are declined.

**A fourth, adjacent overclaim outside the `ACTIVE` set entirely.** `developer:agent-skills` (`kind:
"documentation"`) carried `status: "ported"` and `target: "docs/agent-skills/"` — a directory that
does not exist anywhere in this repository, and neither do any rewritten `SKILL.md` files.
`verify-parity.mjs`'s `incomplete-documentation` rule checks only that a documentation entry *names*
a target, never that the path exists on disk, so nothing caught this, and the entry sat outside the
`ACTIVE` audit too: a `kind: "documentation"` entry `continue`s past the `ACTIVE` check entirely,
regardless of its status. Same defect, one step earlier than the pattern this whole audit was
looking for — a claim that specific work was done, when it was not. Not fixed by writing the missing
docs, which this task's mandate does not extend to and which would risk producing inaccurate
documentation under time pressure; corrected to `temporarily-deferred` with a `deferral` block
naming what is actually needed (someone to write the rewrite) and no false claim in the meantime.

**The vocabulary gap.** `calendar:apple`'s move to `temporarily-deferred` fits the term's existing
meaning exactly — the same shape as `platform:aec`: no macOS machine to write or verify an EventKit
binding against, a resource gap with named dependencies. The two Tinfoil entries do not fit it at
all: both are refused on principle, permanently, not blocked on a toolchain or a decision anyone
expects to reverse. This manifest's `statusVocabulary` has no "declined on principle" term. Rather
than invent one silently — the brief was explicit that this is not an option — each entry's
`deferral.why` says plainly that `temporarily-deferred` is a compromise chosen only because it is the
closest status that stops asserting an implementation exists, and that "temporarily" should not be
read literally. Both entries needed a `deferral` block to satisfy the gate's `incomplete-deferral`
rule for the status, which they did not have before (nothing required one under their old, wrong
status).

Measured effect on the gate's own summary, all four corrections together: `ported-with-new-rust-implementation`
52 (unchanged), `rewired-to-direct-byok` 22 → 21, `removed-vendor-service` 18 (unchanged), `ported`
4 → 2, `temporarily-deferred` 4 → 8, `platform-specific` 2 → 1. Total still 102; the 7-unreachable count is
unaffected, because all three entries already carried `reachable: false` — this audit was entirely
about the machine-readable `status` field lying underneath an honest `reachable` claim, which is the
subtler half of the same defect class: a reader who checks only `reachable` was already told the
truth, but a reader (or a future tool) that trusts `ACTIVE` status as "implementation exists" was not.

### Smaller corrections, same audit pass

- **`media:{audio,video,url}-import`'s `backend` said `"crates/notes-core"`**, which holds no import
  code. The real implementation is `MediaImportRuntime` in `apps/desktop/src-tauri/src/media.rs`.
  Same class as `platform:aec`'s corrected `backend` — a location that was simply wrong, independent
  of the status question. Fixed with a note recording the correction rather than a silent edit.
- **The manifest mixed `"note"` and `"notes"` field names** — `meeting-stream:openai-realtime` and
  `meeting-stream:local` used the plural, every other of the 100 entries used the singular. Renamed
  to `note`; nothing in `tools/integration-audit/*.mjs` reads either field name specifically, so this
  was a pure rename with no behaviour to preserve.
- **`docs/NETWORK_BOUNDARY.md`'s "SSRF guards for URL import" section and `docs/SECURITY.md`'s
  matching sentence both said URL import does "DNS resolution followed by rejection of private and
  link-local addresses".** `media.rs` resolves no names at all — it does a literal-host check before
  yt-dlp ever runs, and yt-dlp's own fetch (DNS included) is unobserved. That planned-but-not-built
  description was flagged during Task 20's review and left for this task, since by then
  `NETWORK_BOUNDARY.md` already had a correct "What none of this stops: DNS rebinding" section
  answering the identical question the other way. Both rewritten to describe the real mechanism and
  point at each other rather than disagree.
- **`crates/network-policy/src/lib.rs`'s module doc claimed "No other crate declares `reqwest` as a
  dependency".** False before this run (`calendar-core`, `local-api`, `reasoning-core`, `sidecar`,
  `transcription-core`, `provider-probe` and both `tools/knegiswhisper*` binaries all did, to chain
  onto the `RequestBuilder` `NetworkPolicy::{post,get,loopback}` return) and stayed false throughout
  it. Corrected to the accurate, narrower claim — this crate owns the only `Client` any part of the
  desktop app's own egress path *constructs* — with the two real exceptions named: two test-only
  bare clients confined to their own test binaries, and `tools/knegiswhisper`'s CLI client, which
  talks to the local API over loopback and is therefore not egress under this project's own "loopback
  is not egress" rule.
- **`docs/BENCH_CHECK.md` item 2 for the updater told a person to paste a public key into
  `tauri.conf.json` and press the button again.** `plugins.updater.pubkey` is compiled into the
  binary by Tauri's `generate_context!`, so that instruction cannot work without a rebuild in
  between. A by-hand check that cannot be followed as written is worse than most defects, since it
  is the one document in this project meant to be executed literally by a person at a machine.
  Corrected to require the rebuild explicitly.
- **`docs/PLATFORM_SUPPORT.md`'s Calendars table said Apple was "EventKit — unverified"**, the same
  word used for Google and Microsoft's genuinely-written-but-uncalled code. "Unverified" undersells
  the actual gap — nothing is implemented, on any platform — so the row and a short note were
  corrected to say so plainly, distinguishing it from the two rows that really are only unverified.
- **Stale narrative counts**, none adjusted by arithmetic — each recomputed from the file it
  describes: `docs/BUILD_PLAN.md`'s "45 of 99 integrations" and "5 of 13 UI surfaces do not exist",
  `docs/INTEGRATION_PARITY.md`'s matching numbers and backlog table, and
  `apps/desktop/src/views/SettingsProviders.tsx`'s doc comment ("fifteen slots, ten providers and
  five per-feature custom endpoints" — the registry now has seventeen: twelve providers, after
  `credential:assemblyai` and `credential:deepgram` were added, and the same five custom endpoints).

### What was found and recorded, not fixed

- **The flaky `kw-storage` test** the Task 8 implementer reported (failed once, passed on re-run;
  every other full-suite run this session was clean) — recorded as unresolved rather than hunted, per
  the brief's own instruction. A test that lies in either direction is a live risk to a project whose
  whole verification story rests on the gates meaning something, and it deserves a session of its
  own rather than a guess appended here.
- **`apps/desktop/src-tauri/src/meeting.rs` at roughly 1600 lines**, noted at least three times during
  this run (Tasks 1, 2, and again here) and never restructured, because every task that touched it was
  explicitly told not to. Recorded as a known shape rather than a defect: it grew for a real reason
  (three cloud clients, keyterm/language plumbing, and the meeting-notification window all needed
  something to hold their commands), and restructuring it was out of scope for every task that could
  have.
- **`SherpaModel` in the provider registry carries no SHA-256** for its model archives, unlike
  `WhisperModel`'s size check and unlike every sidecar binary's mandatory `sha256` in
  `config/sidecars.json`. Task 18's layout-verification check (does the extracted archive actually
  contain the four transducer files it should) is a real and reasonable mitigation for a *truncated*
  download, but a corrupted-but-layout-compatible archive from a compromised CDN would still install
  silently. Flagged by that task's reviewer as "worth a future ticket"; there is no future task in
  this plan to give it to, so it is recorded here instead.

---

## 2026-08-26/27 (Task 20) — the network boundary does not stop SSRF over HTTPS

The most significant finding of the whole run, and it did not come from a planned task — it came
from an implementer flagging rather than patching. Task 11 (media import) noticed that
`kw_network_policy::validate`'s `https` arm was `"https" => Ok(parsed)`, with **no host check at
all**, for any purpose. Verified directly: `https://127.0.0.1/`, `https://169.254.169.254/latest/
meta-data/`, `https://[::1]/` and `https://10.0.0.5/` were permitted under every one of the seven
purposes, and because `next_hop` re-validates each redirect through that same `validate`, a redirect
to a link-local or loopback address over HTTPS was *followed*, not refused. `validate_websocket`'s
`wss` arm had the identical gap, and its own doc comment claimed the rules were shared with `https` —
so once one side was fixed, leaving the other would have made that doc comment newly false rather
than merely incomplete.

An earlier brief in this run had asserted "`NetworkPolicy` already re-validates every redirect hop —
that is the SSRF guard" for URL import. Half true: it re-validates, and the validation it re-runs did
not reject a private target sent over TLS. A valid certificate says nothing about whether a host is
on the public internet or on loopback, and the code was treating "https" as proof of the latter.

**Fixed centrally, as its own task, in a worktree.** `network-policy` is a leaf crate nearly
everything in the workspace depends on, so a half-written change to it would have broken every other
task's build if made in the main tree, and the fix's whole value depended on auditing every existing
call site for a newly-refused legitimate endpoint — the brief named finding such a conflict a
*valuable* outcome, not an obstacle, and none was found. The policy already existed in the file:
`allows_plaintext_private()` already restricted private-space access to
`ExplicitlyConfiguredSelfHostedEndpoint`. The gap was in enforcement, not in policy — the `https` and
`wss` arms simply never consulted it.

### The review found a Critical the implementation had not

A trailing dot defeats the whole predicate: `https://localhost./` has host `"localhost."`, which
fails every string rule in `is_private_host` — not equal to `"localhost"`, not ending in `.local`,
not a valid IP literal, and still containing a `.` so the single-label fallback never fires either —
and `getaddrinfo` on this machine genuinely resolves `localhost.` to `127.0.0.1`. The identical miss
existed in the vendor blocklist (`openwhispr.com.` matched neither the exact nor the suffix branch of
`is_blocked`), sitting under a test *named* `case_and_trailing_dot_do_not_evade_the_blocklist` whose
body tested only the case variants — a test whose name claimed coverage its body did not have, the
same defect class this project keeps finding in itself, one level up, on top of a "no vendor
dependency" constraint. Both fixed by `trim_end_matches('.')` upstream of every comparison in both
functions, closing the whole `localhost..` class rather than one instance.

### The completeness bar rose the moment this landed — fail-closed became fail-open

Before the fix, a false negative in `is_private_host` meant a plaintext request to that address was
*refused* — the plaintext arm only allows what the predicate says is private, so a miss loses
permission rather than gaining it. After the fix, the identical false negative means an HTTPS or WSS
request to that address is *allowed*, because the private check is now what refuses it there. The
function did not change what it is for; what changed is that a gap in it now bypasses a control
rather than only breaking a convenience — which is why the trailing-dot miss, harmless in the old
fail-closed world, was a Critical in the new one, and why every remaining gap in `is_private_host`
now matters more than it used to.

### Widened deliberately, in the same fix: CGNAT and the rest of `0.0.0.0/8`

`100.64.0.0/10` (carrier-grade NAT's shared address space, a standard SSRF target) and the rest of
`0.0.0.0/8` were added to the private set. Both cost nothing real to refuse, since the self-hosted
purpose stays exempt for whichever endpoint someone deliberately points at one — the same reasoning
that keeps the private-range exemption itself narrow to one purpose. This widening cuts both ways:
`http://100.64.0.1/` and `http://0.1.2.3/`, previously refused for every purpose including the
self-hosted one, became reachable in the clear for that one purpose exactly as the rest of the
private range already was — recorded plainly in `NETWORK_BOUNDARY.md` rather than only the
restricting half of the change.

### What was found and deliberately not closed

- **`::ffff:0:a.b.c.d`** (the IPv4-translated form, RFC 8215) still classifies as public — the SIIT
  source prefix is unrouted on an ordinary host and a `connect()` to it fails rather than reaching
  loopback, unlike `::ffff:a.b.c.d`, which a dual-stack kernel genuinely delivers.
- **NAT64 (`64:ff9b::/96`) and 6to4 (`2002::/16`)**, raised unprompted by the re-reviewer as *more*
  common in real networks than the form above. Same reasoning, same acceptance, recorded together
  deliberately — if this family of embeddings is ever closed, it should be closed as a family, by
  auditing every RFC-defined way to fold an IPv4 address into 128 bits, rather than one prefix at a
  time as each is separately noticed.
- **`blocked_hosts()` lowercases configured vendor hosts but does not strip trailing dots from
  them**, an asymmetry with `is_blocked`'s now-normalised input. No live consumer — every entry in
  `config/forbidden-domains.json` is written by this project without a trailing dot.
- **DNS rebinding is out of scope, deliberately.** The check runs against the literal host in the URL
  at validation time, before any connection is attempted; it has no view of DNS. Closing it needs a
  check against the address actually dialled, at connect time, which is a materially different and
  harder problem than the ones this fix addresses, and was explicitly not attempted.

All three parked items are recorded in `NETWORK_BOUNDARY.md`'s "Known, accepted residue" section as
decisions, not oversights — this project's convention throughout the run for a gap it chooses not to
close: name it, say why, and say what would have to change for that answer to change.

`Task 11`'s companion finding — `is_private_host`'s `V6` arm never called `to_ipv4_mapped()`, so
`https://[::ffff:127.0.0.1]/` read as an ordinary public address — was folded into this same fix
rather than given its own round, since both agents were editing the identical function and Task 11's
own local guard delegates to it.

---

## 2026-08-26 (chunks E–J) — the reachability rule, stated once and applied four times

`reachable: true` means a user can *configure* the integration through a named, **built** surface —
not that the underlying code runs correctly, and not that a probe proved the runtime works. This was
settled early (`transcription:tinfoil`, declined rather than pending) but re-litigated, unprompted,
four separate times across the second half of this run — each time because a controller brief's own
arithmetic or premise had quietly collapsed "the code works" into "a person can reach it", and each
time an implementer caught it by flagging the discrepancy rather than complying with the wrong
instruction.

**1. `calendar:apple` (Task 8).** The brief's own count assumed flipping it to `reachable: true`
would take parity from 14 to 11. The implementer did as instructed, then flagged that the flip was
"semantically strained since nothing is implemented" rather than silently accepting the number.
Overruled: `calendar:apple` → `reachable: false`, and the real count is 12, not 11. Nothing about
Apple Calendar is configurable — the entry's own note admits `AppleCalendarSource` "implements none
of it, on any platform" — and naming the reason honestly on screen (which `Integrations > Calendar`
does) is not the same claim as reachability. The precedent this drew on was already in the manifest:
`transcription:tinfoil` lists its refusal on screen and stays `reachable: false` too.

**2. The two sherpa Parakeet models (Task 18).** The probe measured a real runtime — the pinned
`sherpa-onnx-offline` CLI transcribing both pinned Parakeet archives to the same words whisper's
`tiny` model produces — and a first pass flipped both to `reachable: true` on the strength of that
result. Caught before the chunk closed: `SettingsTranscription.tsx` told the user directly "the
sherpa-onnx sidecar, which is not built yet" while the manifest claimed the opposite, and the two
statements contradicted each other because both were wrong in different directions. No Tauri command
or Settings selector exists for choosing a Parakeet model, so both entries reverted to
`reachable: false`, on a narrower and more honest ground than before the measurement: a proven
runtime with a specifically named missing piece, not "blocked on sherpa" as a whole.

**3. Task 11's SSRF framing.** Not an arithmetic error but the same species of premise error: a
brief asserted "`NetworkPolicy` already re-validates every redirect hop — that is the SSRF guard"
for URL import. The implementer's own module doc in `media.rs` states plainly that `import_url` hands
the URL straight to `yt-dlp` as a subprocess and never calls `download_to_file` in production, so
that re-validation mechanism — real, and proven for the purposes that do use it — was never on the
real fetch path for URL import at all. This is the finding that led directly to Task 20.

**4. Task 12's internal contradiction (the fourth instance).** The brief's own text disagreed with
itself — "11 → 8" in one line, "10 right now" in another — and the implementer flagged the
inconsistency rather than reconciling it silently in either direction, then measured the real number
independently with `git stash` against the pre-change tree: parity moved from **10 to 7**, not 11 to
8.

The pattern across all four: an implementer measuring or reading the actual state, rather than
trusting a controller's stated arithmetic, is what kept the record honest each time. None of the four
corrections changed what got built — only what the manifest was allowed to claim about it.

---

## 2026-08-26 (bench check) — three findings, and a probe that had verified less than it looked like

Ran everything that can be checked without a person at the keyboard: the two chunk-D probes again, a
new credential probe, the CLI against the live application, injected chords driving real dictations,
and the resumed whisper server handed a synthesised WAV. Full record in
[BENCH_CHECK.md](BENCH_CHECK.md); this is the reasoning.

### The hook probe had verified less than it appeared to

`keyhook_probe` reported *installed, fired exactly twice for two injected presses, uninstalled
cleanly*. Every word true, and it was offered — by me, in a commit message — as evidence that
modifier-only hotkeys worked. They did not: the callback emitted the event and returned, so
`Control+Super` announced itself in the interface and started no dictation.

The lesson is not "the probe was wrong". It is that a probe verifies the link it exercised, and
naming that link is part of reporting the result. `BENCH_CHECK.md` now opens with that rule and each
row says which link it covers, because the alternative is a table of green cells that quietly implies
the ones between them.

Injecting `Control+Alt` this time started a real microphone capture **66 ms** after the chord, and
four presses drove four alternations. That is the link that had been missing.

### Finding: sided modifiers are built and unreachable (closed 2026-08-31, KNEG-35)

`keys.rs` expresses `RightControl+Super` — the headline capability of having a low-level hook at
all, since the cross-platform API cannot see which side a modifier is on. `PLATFORM_SUPPORT.md` said
"Built with the same hook". Both true, and there is no accelerator a user can type that reaches it:

```
     RightControl+Super -> Standard { accelerator: "RightControl+Super" }
RightControl+RightShift -> Invalid { reason: "it names 2 non-modifier keys; a shortcut may have one" }
```

`hotkey::MODIFIERS` has nine entries and no sided spellings, so `classify` reads `RightControl` as
*the key*. The first goes to the system shortcut register, which is precisely the mechanism that
cannot distinguish sides. The second is refused with a message blaming the user's spelling for our
own missing alias — the third time that exact shape has appeared in this project, after the `Ctrl`
alias and the four dead hotkeys.

Not fixed here. It is one entry in `MODIFIERS`-adjacent parsing plus a decision about what the
settings field should accept, and it belongs with whoever next touches the hotkey editor.

### Finding: the local API token file has no permissions of its own on Windows

`discovery::restrict_file` under `cfg(not(unix))` does nothing and returns
`Permissions::InheritedFromProfile`, with a comment saying the file "inherits the profile
directory's ACL, which normally grants this user and the administrators group". `icacls` on this
machine disagrees: every entry inherited, and two of them a local group and an unresolved SID with
Modify — neither the user nor the administrators.

My first reading was that the code was honest and only the documentation overclaimed. That was
wrong, and reading `Permissions` rather than just `restrict_file` is what showed it. Three places
state what the permissions are and not one of them looked:

- The code comment reasons about what an inherited ACL "normally" grants.
- `Permissions::is_as_expected` returns **`true`** for `InheritedFromProfile`, under a doc comment
  defining that as "as restrictive as the platform allows". Windows allows an explicit owner-only
  DACL, so it is not. `Integrations > Local API` warns only when that flag is `false`, so it stays
  silent — under the sentence *"readable by your user account and by administrators, which is what
  your profile directory allows"*, naming two trustees that are neither.
- `docs/SECURITY.md` said the file "is written with owner-only permissions (`0600`)", flatly, for
  every platform. A security document asserting a control that does not exist on the platform the
  application runs on is worse than saying nothing, because it stops anyone looking. Corrected in
  place, per platform, with the measurement.

This is the `contentProtected` pattern with a different name: a control that is not applied,
described by a string that assumes what applying it would have achieved. The fix is the discipline
the Unix branch of this very function already uses — set it, read it back, report what came back —
and until that lands `is_as_expected` should be returning `false` here.

Not fixed here either. An owner-only DACL through `SetNamedSecurityInfo` plus a read-back is real
`windows-rs` work, and it should land with the read-back rather than without it.

### Finding: the CLI hides failed dictations

`history list` renders `id, when, provider, text`. A failed dictation has neither a provider nor
text, so it prints two dashes and says nothing — while `/v1/transcriptions/list` returns
`"status": "failed"` and a precise `errorMessage` that the dashboard does show. The API had it, the
renderer dropped it, and phase 6 is marked verified.

I nearly logged a fourth finding here: my first query used
`coalesce(text, error_message, '')` and `text` is `''` rather than `NULL` for a failed row, so it
looked as though nothing had been stored. The row was fine; the query was wrong. Worth recording
because the check that saved me was reading the column list rather than believing my own output.

### Judgement calls

#### A credential probe, added because the only implementation in use had no test

Every test of `CredentialStore` runs against `MemoryCredentialStore` — correctly, since a test suite
must not write to a developer's keychain. The consequence went unnoticed: `OsCredentialStore`, the
one the application actually uses, was covered by nothing at all.

`crates/security/examples/credential_probe.rs` closes it, in the shape the two chunk-D probes
established. It uses a slot the provider registry does not declare, stores a value that is not a
credential, **compares rather than prints** — a probe that echoes what it stored would echo a real
credential the day somebody pointed it at one — and checks the distinction the application depends
on: a missing credential must report `NotFound`, not `NoBackend`, or "you have not entered a key"
becomes "this machine has no keychain".

It passes. Windows Credential Manager works here.

#### Set `transcription.localModel` directly rather than waiting for a button

The resume path (`9f0f29d`, written the day before) reads that setting, and the setting was absent
because nobody had pressed **Use for dictation** since the feature existed. Writing `"tiny"` into it
is exactly what `local_server_start` writes. What is under test is the code that reads the setting,
and how the setting arrived does not change that.

It worked, first try: `sidecar ready sidecar=whisper port=8178 pid=7788` then `resumed the local
model server`. Left in place, because it is what makes dictation work at the next launch.

#### Handed the whisper server a synthesised WAV when the microphone leg would not cooperate

The speech gate refused synthesised speech played through the speakers
(`reason=InsufficientSpeech duration_ms=4850`). Almost certainly the AMD microphone array cancelling
its own speakers, which is what echo cancellation is for — so it is evidence about the room, not
about the gate.

Rather than conclude anything from that, I posted the same synthesised speech to
`127.0.0.1:8178/inference` directly: `{"text":" The quick brown fox jumps over the lazy dog.\n"}`.
That verifies the resumed server and leaves exactly one leg unverified — microphone into the gate —
which is now the first item on the list for a person. Reporting one unverified leg is worth more
than a green tick over a path I could not actually drive.

### Observed and not fixed

- **`Control+Alt` is AltGr.** On a Nordic or German layout Windows generates left-Control plus
  right-Alt for `@ $ € { [`, and `EitherSide` matches both. Injecting exactly that started a
  dictation. The chord fires on the second modifier going *down*, so the rule that an ordinary key
  disqualifies a modifier-only chord cannot save it — that rule catches a key already held, not one
  pressed a moment later. The shipped default is unaffected and nothing warns anyone who picks
  `Control+Alt`.
- **WASAPI loopback from an idle output device delivers no callback at all**, not silence. The probe
  says so honestly, but at the API "capture is broken" and "the far end was quiet" look identical.
  Chunk E should decide what that means before a meeting recording does. *(Chunk E shipped without
  it; settled on 2026-08-31 as KNEG-38 — the two are named rather than separated, because they
  cannot be separated, and the third case hiding with them can be and now is.)*

### What the run left on the machine

`hotkey.dictation` was changed to `Control+Alt` to exercise the hook and **restored** — it must not
stay that way on this keyboard. `transcription.localModel` was left set. Two failed transcriptions
are real records of these attempts and were left alone. The database was copied first.

---

## 2026-08-26 (honest hotkeys) — four shortcuts that registered and did nothing

`HotkeyAction` declares five actions. `shortcuts.rs` dispatched with
`if action == HotkeyAction::Dictation`, so the other four registered a real OS shortcut, claimed
the chord from every other application on the machine, emitted an event nothing acted on, and fell
off the end of an `if`. A user could bind chat agent to `Control+Shift+Space`, press it, and get
silence — with the settings screen showing a tidy accelerator and no problem beside it.

Same class as `electronPresent` and `recording_indicator_visible`: a confident answer from the half
of the path that worked.

### The second half of the same defect, found while fixing the first

The keyboard hook that landed the day before had it too. Its callback emitted `HOTKEY_EVENT` and
returned. Nothing called `dictation::on_hotkey`, and the frontend's `onHotkey` only writes a "last
fired" line — so `Control+Super`, the *default* dictation hotkey, announced itself in the interface
and dictated nothing.

The probe that verified the hook is honest about what it checked: installed, fired exactly twice
for two injected presses, uninstalled cleanly. All true. It verified that the hook *fires*, and
"the hotkey works" is a different claim. Worth remembering the next time a probe is offered as
evidence — ask which link it exercised, not whether it passed.

### Judgement calls

#### A fifth `Registration` state, rather than reusing `NotRegistrable`

The plan said to report an unhandled action as `NotRegistrable` with a plain reason: no new type,
no new rendering, and the settings screen starts telling the truth through a channel it already
reads. Tempting, and rejected.

`NotRegistrable` means *the accelerator* cannot be taken — modifier-only, or malformed — and the
user fixes it by typing a different one. `Control+Shift+Space` is faultless. Filing it under
"not registrable" would blame the accelerator for the absence of a feature, and this file already
has an entry about a message that blamed a user's spelling for our own missing alias. The reasons
matter more than the fact: they need different words because they need different actions, and only
one of them is the user's to take.

So `Unhandled { accelerator, reason }`, and `changeLabel` says "Saved. The chat agent is not built
yet." rather than "Saved, but not working" — which would send somebody hunting for a typo in a
correctly spelled shortcut.

#### Not registered at all, rather than registered and ignored

The active harm was not the silence; it was the theft. A global shortcut for an action nothing
handles takes `Control+Shift+Space` away from every other application in exchange for nothing.
Four of them. The chord is now left alone, and the reason string says so, because "we deliberately
did not claim these keys" is a thing a user might want to know.

#### The dispatch table is data, and that was forced by a crash

The design is one function both paths read, so the register path cannot offer an accelerator the
fire path will not handle. First attempt returned the handler itself —
`Handler::Rust(fn(&AppHandle))` — on the reasoning that a function pointer makes the coupling
structural: making an action live means putting a function in the table, and both paths follow from
that one edit.

It made the whole test binary unloadable. Every test in the desktop crate died at
`STATUS_ENTRYPOINT_NOT_FOUND` (0xc0000139) *before any of them ran* — not a failure, a process that
could not start. Bisected to the three new tests that called the lookup, and confirmed with a
one-line probe:

```rust
#[test]
fn probe() { let _f: fn(&AppHandle) = crate::dictation::on_hotkey; }
```

That alone reproduces it. Naming the handler as a value keeps it, and the Tauri runtime it reaches,
alive in the test binary — and a `cargo test` executable has no WebView2 beside it. Calling it from
inside a function no test reaches is fine, because the linker drops the whole path.

**This is a general constraint on testing in this workspace, not a quirk of one file:** a unit test
may exercise a decision about Tauri-dependent code, and may not hold a reference to the code
itself. So `Dispatch` is a plain enum naming its handler, `unhandled_reason` is data and testable,
and the pointer appears at the two call sites only. The compiler still enforces the coupling —
adding a variant breaks both `match`es — which is the guarantee that was wanted, checked at each
site instead of carried in a value.

#### The startup warning loop stays quiet about it

`register_all` warns per accelerator when an action has no working hotkey, with the reason, because
"another application owns it" and "this is two modifiers and no key" need different fixes. Four
unhandled actions would have added four warnings to every launch, all restating what the plan says.
They log at `debug` instead. A warning that fires every time on a designed state is how a log stops
being read.

The condition is read from the dispatch table rather than inferred from the results, so it cannot
disagree with what was actually done.

#### `report_for` split out of the `hotkeys` command

The command takes Tauri state, which is why the interesting part of it had no test. Splitting the
report construction into a plain function made the two new command-level tests possible. `unusable`
is now the union — no live accelerator, for any reason — and `unhandled` is the subset with a
reason, because a caller asking "is this action dead?" should get one answer and a caller rendering
it needs the words.

### Also corrected

`Accelerator::ModifierOnly`'s doc comment still said the hook "this application does not yet have
on any platform". That stopped being true when the hook landed. Same defect as the frontend string
that asserted a platform fact — a doc comment can rot into a lie as easily as a label.

Added `Accelerator::text()` while I was there: three call sites were each re-deriving the
accelerator text per variant, one of them through a match arm that was unreachable.

### Left undone, deliberately

The four actions still have no features. This chunk makes the interface honest about that; it does
not build them. Their home is phase 10 in [COMPLETION_PLAN.md](COMPLETION_PLAN.md), and
`nothing_handles_an_action_that_is_not_dictation` is a tripwire: the day one of them gains a
handler, that test fails and asks for the plan and `PLATFORM_SUPPORT.md` to be updated in the same
commit. Leaving the docs describing a state that has ended is how four dead hotkeys survived this
long.

826 tests, clippy, fmt, tsc, vite build and all contract gates green.

---

## 2026-08-26 (chunks B and C) — the backends behind the screens

Chunk A gave every working backend a screen. B and C are the other direction: the backends that
were declared and refused, made real. 26 to 45 reachable.

### Three refusals kept, and why none of them is laziness

The temptation in a chunk like this is to make everything green. Three things are still refused,
and each refusal is the correct answer rather than an unfinished one.

**Tinfoil.** Its whole proposition is an attested enclave — the client verifies an attestation
document against a hardware root of trust before sending anything. An ordinary HTTPS POST to the
same host transcribes perfectly well and provides *none* of that, while appearing in the UI as
"Tinfoil". `route.rs` already refused a Tinfoil host configured as a plain custom endpoint for
exactly this reason; writing the client as an unattested POST would have defeated that refusal
from the inside. So `ClientNotImplemented` stays, and it now carries the reason instead of the
generic "not implemented yet. Choose a provider that uses the standard API" — advice written when
the answer was the same for all four, and wrong now that three work.

**sherpa-onnx.** The plan said "same sidecar pattern". It is not: 276 of the 303 release assets
are `.tar.bz2`, which `install::extract` cannot open; the binaries are a one-shot CLI and a
WebSocket server rather than an HTTP endpoint, so neither `Transport::Loopback` nor `HttpHealth`
applies; and the per-model file layout differs between the Parakeet and Nemotron families. Adding
bz2 support on its own was considered and rejected — an archive format with no consumer is the
scaffolding this workspace avoids.

**Enterprise live calls.** Bedrock's signing is verified against AWS's published vectors, which is
a real guarantee about the *signature* and says nothing about whether Bedrock accepts the request.
`PLATFORM_SUPPORT.md` now has a per-provider table keeping those apart.

### The defect chunk C found

`route.rs` defaulted Corti's environment to `us` and its tenant to `base`. Those look like
placeholders and are not: they compose a live endpoint under a tenancy nobody chose, so an
unconfigured Corti would have sent the user's audio *and their credential* there. That is the
silent-substitution class the brief rules out by name, sitting inside the function written to
prevent it.

Worse, three route tests swept every registry provider and asserted each resolved — and they
passed **because of** those defaults. A test that exercises a default while claiming to test the
logic is a test that protects the default. They now configure the one provider that needs
configuring.

### Judgement calls worth keeping

#### `Dispatch` and `ReasoningDispatch` are asked, never listed

Both screens have to say which providers can actually be used. The obvious implementation is a
list of ids in TypeScript. Both instead call the real resolver — `route::resolve` then
`clients::shape`, and `resolve_chat` — and report what it says. Within one session that decision
paid: three providers moved from "needs a client" to "ready" and nothing in either screen changed.

#### Reading the port is what records a use

The llama server holds several gigabytes and stops itself after five idle minutes. A `touch()`
call at every use site is a call somebody forgets, so `LocalEndpoint::port()` — the only way to
find out where to send a local request — is itself the record. Status screens read
`running_port()`, which deliberately does not, because a settings window left open would otherwise
hold the memory indefinitely.

#### `tools/sidecar-pin` had to exist before `llama` could be added

`config/sidecars.json` has instructed people to use it since the first sidecar, and it did not
exist. Its first act was reproducing all three committed whisper pins exactly — which validates
the tool and the pins in the same run.

#### The generated types earned their keep three times over

`EnterpriseConfig` had `rename_all` without `rename_all_fields`, so it would have crossed the
boundary as `access_key_id` while everything else sends camelCase. `local_model` and `local_llm`
both defined an `InstallProgress`, which in one TypeScript file is a redeclaration where the later
one silently wins — the generator now refuses a name collision by assertion. And
`ModelStatus::verified_by` was a `&'static str` the frontend re-declared by hand as a union.

---

## 2026-08-25 (chunk A2/A3) — the shell, and somewhere to type a key

### What this closed

`Settings > Providers` did not exist, and it was the single thing holding back the most work:
`provider_credential_set` had been in Rust since phase 1 and `setProviderCredential` had been
exported from `ipc.ts` for as long, and **nothing called either**. Reachable integrations went
from 8 to 26 — fifteen credential slots and three transcription providers — with two new commands
and no new subsystem.

`App.tsx` went from 1037 lines holding every screen to a shell: a sidebar, a view switch, and an
error boundary. Six views, one file each, at the paths `config/ui-surfaces.json` already named.

### Judgement calls

#### Two commands, where the plan said none

[BUILD_PLAN.md](BUILD_PLAN.md) predicted chunk A would need no backend. It needed two, and both
came from the same discovery in two places — *a screen cannot be honest about a thing the backend
will not tell it*.

- **`credential_slots`.** Fifteen `provider_credential_present` calls on every read and after
  every save, or one call. One call, and the ordering and the labels come from the registry rather
  than from the screen.
- **`dictation_set_cloud_provider`.** Storing a key does not make a provider reachable if nothing
  can select it. `cloud_transcription_provider` and `cloud_transcription_model` are written
  together, never separately: changing the provider while leaving the previous provider's model
  behind resolves to a model the new provider does not offer, and `resolve_model` refuses —
  correctly, for a reason the user never chose.

#### `Dispatch` is derived from the router, not from a list

Four of the seven transcription providers resolve to `Transport::ProviderClient`, which the
executor refuses. The screen has to say so, and the obvious way was
`["tinfoil", "mistral", "xai", "corti"]` in TypeScript. That is a second copy of the match in
`route.rs:430`, and the two would part company the first time a client was written.

Instead `commands::dispatch_for` builds a `RouteSettings` naming the provider and asks
`route::resolve` what it would do. Three answers: `ready`, `needs-client`, `unresolvable`. A test
asserts the three-and-four split, so a provider that silently starts resolving is a test failure —
which is what a fallback to OpenAI would look like from here.

#### A stored key is reported three separate ways

`configured`, `dispatch`, and `connected` are three different facts and the screen never collapses
them. A key is stored; a transport exists; a real request succeeded. Only the third is "connected",
and only `probe_providers` can establish it.

The custom-endpoint slots get a fourth answer again: the key is stored and it configures nothing,
because the base URL lives with the feature that uses it and that screen is chunk B. The page says
that rather than looking finished — five slots that look like five more providers would be exactly
the false completeness this project keeps finding upstream.

#### The view switch is a `switch`

No router. The URL fragment is already load-bearing — `#overlay` selects the overlay window out of
the same bundle — and there is nothing to link to from outside a settings window. What a router
would have bought is history, and the one piece of history worth having, "reopen where I was", is
actively wrong when where you were is the screen that just failed.

#### An error boundary, because a blank window is the alternative

A thrown render in a Tauri webview is a blank window with no tab to reload and no console in a
bundled build. The boundary shows the message and offers a way to a screen that was working.

### A self-test that stopped testing anything

`verify-parity.test.mjs` had a case that stripped `data-surface="notes.export"` from `App.tsx` and
asserted the gate noticed. Splitting `App.tsx` moved that marker to `views/Notes.tsx`, so the
mutation stopped mutating anything and the test went green while proving nothing — **the exact
failure the file's own header warns about**, one level down. It now reads the file and the marker
out of the surface registry, so it cannot go stale the same way twice.

Worth stating plainly: the gate itself was correct throughout. It was its own self-test that
rotted, and only the fact that self-tests are run at all caught it.

---

## 2026-08-25 (chunk A1) — the IPC types are generated from Rust now

### The spike, and what it answered

[BUILD_PLAN.md](BUILD_PLAN.md) made A1 a gate rather than a formality: **13 files carry
`rename_all_fields = "camelCase"` on internally tagged enums**, and if no generator could express
that shape, generating the IPC types would have meant writing an emitter by hand. The instruction
was to find out rather than guess.

`ts-rs` 12.0.1 expresses it. Derived on `overlay::CaptureExclusion` — the hardest shape in the
tree, internally tagged with kebab-case variants, camelCase struct-variant fields and a nested
`Evidence` — it emitted

```ts
export type CaptureExclusion =
  | { "state": "active", verifiedBy: Evidence }
  | { "state": "unsupported", reason: string }
  ...
```

which is exactly the hand-written union it replaces. `specta` and the custom-emitter fallback were
not needed. A test now asserts that specific property directly, because it is the one that would
silently regress: `ipc_types::tests::a_tagged_enums_struct_fields_are_camel_case`.

### Judgement calls

#### One writer, not `#[ts(export)]`

`ts-rs` can export each type on its own during `cargo test`, and the obvious thing would have been
to point every derive at one file. But the types live in eight crates whose test binaries run
concurrently, and they would all have been writing that file. Instead
`apps/desktop/src-tauri/src/ipc_types.rs` names every type in one list and renders the whole file
in one pass, from one test. No race, a deterministic order, and the list itself is greppable — a
type that crosses the boundary and is not in it stands out.

#### The check is a test, not a CI step

There are already three `--check` steps in the contracts job, and a fourth was the obvious shape.
A `cargo test` assertion is better: it runs on every machine and in every job that runs the test
suite, so it cannot be forgotten the way a workflow step can. `KW_WRITE_IPC_TYPES=1` turns the same
test into the generator.

#### `ts-rs` is a normal dependency, not an optional one

It could have been feature-gated so release builds never see it. That would make a type crossing
the boundary *without* a `TS` derive a silent omission rather than a compile error, which is the
defect class this whole change exists to close. Nothing calls into `ts-rs` at runtime and release
builds strip the impls, so the cost is compile time only.

#### 64-bit integers are declared `number`, not `bigint`

`ts-rs` defaults `i64`/`u64` to `bigint`. Every 64-bit integer crossing this boundary is a SQLite
rowid, a byte count or a millisecond duration; `serde_json` sends them as JSON numbers and Tauri's
`invoke` parses them with `JSON.parse`, so a `bigint` declaration would have described a value the
frontend never receives. `Config::with_large_int("number")`, and a test.

### Two real defects the generator exposed

Both were invisible while the declarations were written by hand, and both are the same shape as the
`electronPresent` defect: a type and the thing it describes, disagreeing.

| Where | What was wrong | Fix |
| --- | --- | --- |
| `storage::import::{TableReport, RelocatedCredential, ImportReport}` | Sent `absent_columns`, `skip_reason`, `fts_rows` — **snake_case**, while every other struct in the application sends camelCase. One API with two conventions is how the next person writing a frontend type gets it wrong. | `#[serde(rename_all = "camelCase")]`. No consumer existed yet; the importer has no UI. |
| `local_model::ModelStatus::verified_by` | A `&'static str` with three possible values, which the frontend re-declared by hand as `"digest" \| "size" \| "nothing"`. Nothing held the two in step, so a fourth value added in Rust would have reached a UI that did not know about it. | A real `VerifiedBy` enum, generated into the union. |

`provider_probe::ProbeOutcome` was a third, milder case: the hand-written type said
`{ outcome: "skipped" } & Partial<SkipReason>`, which admits `{ outcome: "skipped" }` with no
reason at all — something serde never sends. The generated declaration drops the `Partial`.

### What did not change

The hand-written `invoke()` wrappers in `apps/desktop/src/lib/ipc.ts`, because the defect was never
in the wrappers. That file is now command names, argument names, and two predicates that mirror a
Rust method; every payload type it exports comes from `../generated/ipc-types`.

`EXPORT_FORMATS` is the one list still written by hand — `ts-rs` declares the union but cannot emit
a runtime array. The Rust test `every_export_format_has_the_wire_spelling_the_frontend_sends`
already pins those five strings.

---

## 2026-08-25 (CI paused) — GitHub Actions triggers commented out

Requested as a temporary pause. No reason was recorded, and nothing was found wrong with the
workflow itself — the jobs are untouched and still valid — so this is a pause, not a verdict on
CI's value. Treat it as short-lived.

### Judgement calls

- **Commented out the triggers rather than the file.** The ask was to "comment out the code", and
  the literal reading — comment out all 142 lines — produces a YAML file that parses to nothing,
  which GitHub surfaces as *Invalid workflow file* in the Actions tab. That trades silent
  green for a permanent red, which is worse than either. Commenting out only `push` and
  `pull_request` leaves a valid workflow that simply never fires on its own.
- **Kept `workflow_dispatch`.** It costs nothing while the workflow is dormant and means the
  full suite can still be run by hand from the Actions tab during the pause, which is the
  difference between "paused" and "gone".
- **Did not use `gh workflow disable`.** It is the cleaner mechanism — it takes effect across
  every branch at once and leaves no diff — but it also leaves no trace in the tree. A pause that
  is invisible in the repository is a pause a future session will not know about. The comment in
  the file, and this entry, are the point.

### Carry-forward consequence

The pause partly invalidates a claim in the carry-forward above. "macOS and Linux CI ran for the
first time on the Phases 1–4 push. If either is red, it is a first-run failure, not a regression."
was written expecting the next push to resolve it. With triggers off, it stays unresolved.

`main` also keeps its own copy of the workflow, so pushes to `main` still trigger CI until this
change is merged there. Only `dev` is actually paused.

---

## 2026-08-25 (later) — Phases 1-4

Branch `dev`, commits `f750196` .. `e820ff9`. 386 tests, all gates green.

### Bugs found by writing tests, not by reading code

Worth listing separately because these were latent and none was visible by inspection.

#### The network boundary made local model servers unreachable

`RequestPurpose::UserConfiguredAiProvider` and `UserConfiguredTranscriptionProvider` both refuse
plaintext HTTP. Both self-hosted routes were passing one of them, so a local whisper server or
llama.cpp server was refused by the policy layer before anything was sent -- and the error said
"scheme not permitted", which reads as a bug in the URL rather than in the purpose.

Underneath it, two layers disagreed about what "private" means: route resolution accepted the
whole RFC 1918 range, the policy layer accepted only loopback. So an endpoint the settings screen
would accept was refused at send time.

Fixed at the root rather than patched:

- `is_private_host` moved into `kw-network-policy` and `transcription-core::url` delegates to it.
  The security layer owns the security predicate; the two cannot disagree any more.
- The plaintext exemption widened from loopback to the private range. A model server on another
  machine on the user's LAN is an ordinary setup and refusing it means the only way to use one is
  to put a certificate on it. Audio and any credential do cross the local network in the clear --
  that trade belongs to the user, for an endpoint they typed themselves. A test asserts no other
  purpose gets the exemption.
- The purpose is now derived from the *destination*, not the feature, in both executors.

#### Tightening a forbidden-domain pattern opened a hole

`\bamplitude\b` matched `crates/audio-core`'s own DSP code -- peak amplitude, signal amplitude
-- which is the same false positive the Phase 0 audit found upstream, where every apparent hit
was `peakAmplitude`. The right fix is to match the vendor rather than the word.

The first attempt double-escaped the dot and silently stopped matching `api.amplitude.com`. It
still passed the scan, because the scan only proves nothing *in this tree* matches. A pattern
that matches nothing passes trivially.

So there is now a scanner self-test asserting the shipped patterns still catch real vendor
strings and still ignore ordinary technical English. `\bsentry\b` was tightened the same way
after the new test caught it matching "a sentry value".

The general lesson: for a denylist, "the tree is clean" and "the patterns work" are different
claims and only the second one matters.

#### A frontend type claimed a field Rust never sent

`Diagnostics` in `ipc.ts` declared `electronPresent`, so `App.tsx` rendered "not present" from
`undefined`. It looked like verification and verified nothing. Removed rather than implemented:
the absence of Electron is a build-time property provable from the dependency tree, not something
to probe at runtime.

#### The importer was silently losing rows

Per-row inserts through a 4-connection pool hit `SQLITE_BUSY_SNAPSHOT` (code 517) in WAL mode,
which `busy_timeout` does not help with because the snapshot has to be restarted rather than
waited on. It showed up as a flaky test and a table copying zero rows. Fixed by running the whole
import in one transaction on one connection, which also makes the import atomic -- it was not
before, despite the module documentation claiming it was reversible.

The report also counted skipped rows without saying why. A count with no reason is not something
a user can act on, so `TableReport` now carries `skip_reason`.

### Judgement calls

#### Corrected the plan on Windows capture exclusion

The Phase 0 audit recorded that `SetWindowDisplayAffinity` has "no documented getter" and
concluded Windows would have to report `unknown`. That is wrong -- `GetWindowDisplayAffinity`
exists and has since Windows 7. Windows now reports `active` with real read-back evidence.

Kept honest about what the read-back proves: the flag is set on the window, not that any given
screen recorder honours it. No API reports the latter from inside the process, and the UI says
exactly that rather than showing a bare green check.

#### Reported upstream's default dictation hotkey as broken rather than registering it

`Control+Super` is modifier-only and no cross-platform shortcut API can register it; upstream
uses per-key native listener processes. Registering it and failing silently would leave the user
pressing a dead key, so `classify` returns `ModifierOnly` with an explanation and a suggested
fix, and the dashboard shows it.

Added a test asserting the shortcut plugin *also* refuses it. If the plugin ever gains support,
that test fails and says to revisit the classifier -- rather than leaving a stale limitation
documented forever.

#### Renamed upstream's `proxied` transport to `ProviderClient`

Nothing is proxied through a vendor; it means the provider needs its own client rather than the
generic OpenAI-compatible POST. Given the brief forbids hidden remote fallbacks, a transport that
*reads* as a proxy was worth renaming.

#### Did not port upstream's strip-and-retry for rejected chat parameters

Upstream retries with shaped parameters removed when a provider rejects them by name, logging
loudly because "a fallback that fires on every request is a dialect bug to fix, not a rescue".
That is a sound safety net for a data table that might be wrong. With the registry as a single
source of truth and a test walking every model, the first move is to keep the table right. If a
provider turns out to need it, it belongs there with a test naming the provider.

#### Stopped meeting-core at the contract

The capability model, trait, and derived allowlist are complete; the five WebSocket clients are
not written. Each needs real credentials against a live service to verify, which cannot be done
from this machine, and a client that compiles without having ever connected is worth very little.
`is_allowed` returns true only for a provider with a client, so the gap is in the type rather
than only in a document.

### Deviations from the approved plan

- **Two unplanned crates.** `provider-probe` (the capability spans transcription and reasoning,
  so it fits in neither, and putting it in `provider-registry` would give a pure data crate a
  network dependency) and nothing else -- `platform` and `storage` were planned.
- **`verify-byok.mjs` became a feature, not a CI test.** It is a live network probe needing real
  credentials, so it cannot run in CI. Ported as `provider-probe`, which is what makes "never
  show a provider as connected without a real connection test" checkable.
- **Autostart uses `tauri-plugin-autostart`** rather than hand-rolled registry code. Less unsafe,
  maintained upstream.
- **The MDM policy layer is not ported.** Upstream's `resolveTranscriptionRoute` takes a
  fail-closed policy floor for managed installations. Not in the brief, and no substitute is
  pretended at.
- **`import_openwhispr` has no UI**, so it is deliberately absent from the integration manifest.
  Adding it there before it is reachable would be exactly the unconfigurable-backend the parity
  gate exists to catch.

### Open decisions

- **whisper.cpp source.** Went with upstream `ggml-org/whisper.cpp` at a pinned tag rather than
  the vendor org, keeping upstream's reason for pinning (a bump must not change transcription
  output between releases). One line of config to reverse if a fork is preferred. **Still needs
  the sidecar download and lifecycle work before Phase 2 is usable end to end.**
- **AEC helper + five Windows native helpers** still come from `OpenWhispr/openwhispr` release
  assets. Source is in-repo and MIT, so this is a CI build-and-publish job. Needed before
  meetings work.
- **`nircmd`** is still unpinned and unhashed. Leaning towards dropping it: it is only the
  third-tier Windows paste fallback, and `SendInput` now works.

---

## 2026-08-25 — Phase 0 + Phase 1 foundation

Branch `dev`, commits `f1a2f55` … `7c4f32f`.

### Judgement calls

#### Did not allowlist `crates/network-policy` from the domain scanner

The first real scan flagged `crates/network-policy/src/lib.rs`, because the runtime blocklist has
to name the hosts it blocks and the tests have to assert those hosts are blocked.

The obvious fix was a path allowlist entry. **Rejected:** that would blind the scanner across the
entire file that owns all outbound network access — the worst possible place to lose coverage.
A future leak added anywhere in that file would then be invisible.

Instead:

- Added line-level suppression to the scanner: `domain-scanner:allow <reason>` and
  `domain-scanner:allow-next-line <reason>`. A reason is mandatory; the bare marker does nothing.
  The exemption stays visible to anyone reading the code.
- Moved the vendor-host negative tests to `crates/network-policy/tests/blocklist.rs`, which uses
  only the public API and is covered by the `crates/*/tests/**` allowlist entry.

`lib.rs` now needs exactly two inline suppressions (the parse-failure fallback constant, and one
assertion about a specific host being in the blocklist) and is otherwise fully scanned.

Side benefit: moving the tests made them better. They now assert every blocked host is blocked
for **all seven** egress purposes — a blocklist applying to only some purposes would be bypassed
by mislabelling a request — and that lookalike domains such as `notopenwhispr.com` and
`openwhispr.com.example.org` are *not* caught by a naive suffix match.

#### Diverged from upstream on fail-closed provider dispatch

The brief requires failing closed on ambiguous dispatch. Upstream does the opposite in three
places:

| Upstream | Behaviour |
| --- | --- |
| `transcriptionRoute.ts:290-307` | terminal fallback returns `provider:"openai"`, `keyRef:"openai"` for any unrecognised id |
| `constants.ts:59` | `buildApiUrl` second-order default to `https://api.openai.com/v1` |
| `chatRouting.js` | anything outside `CLOUD_CHAT_PROVIDERS` is silently classified `local` |

Followed the brief, not upstream. Unknown provider ids now error.
`resolve_transcription_model` also rejects an empty string as a custom model id, which upstream's
`allowCustomModelId: true` path would have accepted.

This is a deliberate behavioural difference from the port source, so it is called out in
`docs/PORTING_MATRIX.md` and each of the three has a test.

Worth remembering *why* upstream's version was dangerous: a typo in a provider id sent the user's
audio to a provider they never chose, using a credential they had configured for something else.

### Deviations from the approved plan

- **Parity gate landed in commit 2, not commit 7.** A manifest without a checker is just a
  claim, and shipping the claim five commits before the check felt wrong.
- **Created 4 crates, not 13.** The brief asks for vertical slices over broad unfinished
  scaffolding, so the remaining crates are listed as comments in the workspace manifest and get
  created by the commit that gives them content.
- **Added an unplanned `.gitattributes` commit** (`8065e71`). `core.autocrlf=true` on this
  machine meant a fresh clone would check the generated baseline out as CRLF, while the generator
  emits LF — so `--check` would have failed on a clean tree for no real reason. Verified by
  deleting the file and re-checking it out.

### Corrections made along the way

- Told the user the baseline-vs-`main` delta was nil, based on a GitHub compare page that had
  actually failed to render. Real position: `858e824` is **not in the local clone at all**; the
  fork point is `4d1a6eaf` (2026-08-20) and the pinned baseline is ~3 commits ahead of it. None
  of the three touch providers or the registry, so parity against `c4000db9` is equivalent.
- First Electron check was wrong — it looked for *any* `electron` process on the machine and
  found three, which turned out to be the upstream OpenWhispr checkout running. Re-verified
  properly via the dependency tree, `node_modules`, and the binary format.
- `docs/ARCHITECTURE.md` initially claimed a build script emits both Rust structs and TS types.
  That is not what was built (Rust uses `include_str!` + serde; only TS is generated). Corrected
  the doc rather than leaving it overstating.

### Open decisions

- **whisper.cpp source.** Upstream pins `OpenWhispr/whisper.cpp` tag `0.0.8` — a vendor org, now
  blocked by the scanner. Options: upstream `ggml-org/whisper.cpp` at a pinned tag, or a fork
  under `axelalbertkarlsson/`. Upstream's *reason* for pinning is sound (a bump must not change
  transcription output between releases), so keep the pinning and change only the org.
  Needed before Phase 2.
- **AEC helper + five Windows native helpers** currently come from
  `OpenWhispr/openwhispr` release assets. Source is in-repo and MIT, so this is a CI build-and-
  publish job rather than a rewrite. Needed before Phase 4.
- **`nircmd`** is fetched from `nirsoft.net` with no version pin and no hash. It is only the
  third-tier Windows paste fallback behind `windows-fast-paste.exe`; leaning toward dropping it
  rather than pinning it.
