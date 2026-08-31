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
> **Measured, not asserted, as of 2026-08-27 after the security fixes merged:** `cargo test
> --workspace` — **1296 passing, 0 failed** (941 at the start of the run, 1290 at its close);
> parity — **102 entries, 8 declared unreachable** (17 at the start), and **all 13 UI surfaces
> built**, with no "surface not built" entry remaining anywhere. The eight: four local-ASR models
> (sherpa/Parakeet — runtime measured and proven by a probe re-run cold by the controller, missing
> only a Settings selector), two Tinfoil entries (batch transcription and meeting-stream, both
> declined on principle, not unfinished), Apple Calendar (EventKit — described in
> `kw_calendar_core::apple`, implemented nowhere, on any platform), and URL import (withdrawn as a
> command after the run closed — see the 2026-08-27 entry on the merged security fixes).
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
> hardware — and nothing from chunks E onward is in it.** The 2026-08-25/26 runs covered phases 2,
> 6 and chunks A–D. Everything built since (meetings, calendars, media import, MCP/CLI screens,
> the updater, sherpa) is green tests and code review only; that file's own by-hand lists for E–K
> and for Task 20 say so item by item, and none of them should be read as done until someone
> actually presses the buttons.
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
> **What is still true:** the failure is invisible *while* the meeting runs (KNEG-95, which carries
> a design decision this did not take), and three of the four clients have never met a real
> provider error because no key exists for AssemblyAI, Deepgram or Corti. See the 2026-08-31
> (KNEG-79) entry below, and `BENCH_CHECK.md` item 13.
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
> which is narrower and true. Still open: **the verdict is a snapshot taken at start**, and nothing
> re-reads the DACL while the application runs, so a trustee added afterwards will not show on
> `Integrations > API` until the next start; and **CI compiles none of the Win32 code**, because the
> `windows-latest` job is off for every trigger but `workflow_dispatch`. The pure classifier is
> covered on Linux by design — `file_acl::classify` takes plain Rust facts and the measured DACL is
> a fixture in its test module. The three Win32 calls are covered by this repository's first two
> `#[cfg(windows)]` tests and by `cargo run -p kw-security --example dacl_probe`, and by nothing
> unattended. Both open items are filed — KNEG-110 for the snapshot, KNEG-111 for the CI gap. See
> the 2026-08-31 KNEG-34 entry below and `docs/BENCH_CHECK.md` finding 2.
>
> **Three smaller items recorded rather than chased**, each detailed in its own entry below: a
> `kw-storage` test that failed once and passed on re-run during Task 8 (every other full-suite run
> this session was clean, so intermittent rather than new); `apps/desktop/src-tauri/src/meeting.rs`
> at ~1600+ lines, noted repeatedly and deliberately not restructured; and `SherpaModel` in the
> provider registry carrying no SHA-256 for its model archives, unlike `WhisperModel` and unlike the
> sidecar binaries in `config/sidecars.json`.

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
| Local transcription (Phase 2) | Reachable, unconfirmed by hand | The interface exists now: install and server commands, a route the user can point at the local model, and a hotkey that runs the session. What is missing is a person pressing that hotkey and watching text appear — see the by-hand check in `docs/PHASES_0_6_COMPLETION_PLAN.md`. Green tests are not that. |
| Meeting transcription (Phase 4) | Contract only | The five WebSocket clients. `is_allowed` returns true only for a provider with a client, so nothing claims otherwise. Each needs real credentials against a live service to verify. |
| Enterprise reasoning (Bedrock, Azure, Vertex) | `ShapeNotImplemented` | The three request shapes. They refuse honestly rather than falling back. |
| `storage::import::from_openwhispr` | Works, no UI | Deliberately absent from `config/integration-manifest.json` — adding it before it is reachable would be exactly the unconfigurable-backend the parity gate exists to catch. Add both together. |
| Local API and CLI (Phase 6) | Reachable, unconfirmed by hand | The application starts and stops the server now, off by default, with the discovery file's real permissions shown in settings. What is missing is the compiled CLI having been run against a running application by a person rather than by a test harness. |
| Phases 5, 7–9 | Not started | Calendars, MCP, diarization + Qdrant, packaging. System audio and the meeting clients remain from Phase 4. |

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

**Two things found and not fixed, both filed (KNEG-110, KNEG-111).** The verdict is a **snapshot taken when the API starts** —
`local_api_status` reports what the write measured, and nothing re-reads the DACL while the
application runs. During this run a trustee was added to a protected directory by something outside
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

Recorded for whoever does want it, because KNEG-95 will: the obstacle is `pump`'s `app: AppHandle`,
and the better of the two routes is giving `pump` a small `TranscriptSink` trait — matching what
`calendar-core` did with `TokenSender` and its test-only `PlaintextLoopbackSender` — rather than
enabling `tauri`'s `test` feature, which nothing in this workspace uses.

### What stays KNEG-95's, and was deliberately not taken

**A dead provider is still silent mid-meeting.** The indicator stays up, recording continues, and
nothing reaches the screen until Stop. No new `MeetingState` variant, no change to whether recording
continues, `MeetingRuntime::status()` still does not consult `pump.is_finished()`, and no new
mid-meeting error event. That is a product decision about whether a dead provider stops the
recording, falls back immediately, or carries on saying so, and it belongs to the repository owner.

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

**KNEG-106** (Low). `extract_tar_xz` decompresses the whole tar into a `Vec<u8>` before looking at
an entry, because `lzma-rs` decompresses into a `Write` and exposes no streaming `Read`.
`max_bytes` bounds what reaches `dest` and cannot bound that buffer, so a `.tar.xz` remains a
memory bomb whatever ceiling is passed — independent of the output ceiling it now honours. Behind a
mandatory digest, and the only pinned `tar-xz` assets are ffmpeg on Linux at ~100 MB compressed and
~300 MB extracted. Needs a streaming LZMA2 reader; the function's doc comment now says so.

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

### Finding: sided modifiers are built and unreachable

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
  Chunk E should decide what that means before a meeting recording does.

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
