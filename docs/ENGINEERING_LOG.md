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
> **A cross-cutting gap, recorded and deliberately not fixed:** all four meeting socket clients
> (`openai_socket.rs`, `assemblyai_socket.rs`, `deepgram_socket.rs`, `corti_socket.rs`) log a
> provider-reported error frame with `tracing::warn!` and never turn it into an `Err`. A server that
> accepts a malformed session-config frame, stays open, and emits nothing would let a meeting
> "complete successfully" holding an empty transcript, evidenced only by a log line nobody in the
> room can see. `BENCH_CHECK.md` item 13 under chunk E already carries this; it predates every task
> in this run and is shared by all four clients equally.
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
