# Windows validation, 7–8 September 2026

This records the autonomous scope agreed for KNEG-172, KNEG-173, KNEG-50,
parts of KNEG-166/KNEG-51 and KNEG-90, and the hosted CI follow-up for
KNEG-40/KNEG-111. Native observations used the real Tauri/WebView2 application,
installed local models and Windows audio. Synthetic speech supplied disposable
content; no customer call or OpenWhispr database was used.

## Meeting panel and streaming recovery

KNEG-172: the persistent notification now refreshes its runtime snapshot on
state transitions. New events supersede older reads, subscriptions are installed
before the initial read, and late subscriptions are cleaned up on unmount.
Recording with microphone and loopback showed `you and everyone else`; a
microphone-only recording showed `you only`. A new recording reset the timer
to zero and it continued counting.

KNEG-173: the caption clips its leading edge when it exceeds the available
width. Its trailing edge remains visible without accumulating partials or
estimating a character count. Native speech produced a caption wider than
1,100 CSS pixels in a roughly 292-pixel container. The inspected image showed
the newest words, with Stop still inside the window. Short text retains its
normal alignment. Light and dark media schemes were inspected; the panel's
deliberate fixed colours remain legible in both.

For KNEG-166/KNEG-51, terminating only the disposable application's streaming
server during a recording produced `recording-without-transcript`. The amber
warning replaced the caption, the timer kept counting, and Stop remained
visible inside the 392×92 window. Stop saved the recording. A subsequent run
showed the Transcribing panel before returning to idle; the saved outcome had
an attached WAV, zero gaps and zero reconnects. Win32 inspection of the
disposable application's own PID confirmed the meeting window was hidden
afterwards, with display affinity 0x11.

Dictation and meeting recording also displayed their simultaneous recording
states in the overlay and meeting panel. Canceling dictation left the meeting
recording. This does not establish the tray tooltip or real-call detection.
Recorder panic was not induced. Real-call offers, screen-recorder exclusion
and the other dedicated live-call checks remain outside this pass.

The failure experiment exposed KNEG-176: starting the next meeting reused a
cached port belonging to a dead streaming process. Start now checks process
liveness and a bounded WebSocket readiness probe before reuse, otherwise
replacing the old server through the existing serialized lifecycle. The native
application successfully started and transcribed the next meeting after the
controlled process loss.

KNEG-174 was a false alarm from the first failure screenshot. Repeated native
inspection put Stop at x=333.82..379.41 inside the 392-pixel viewport. The issue
was canceled; no speculative panel-size change was made.

## Local model cleanup

The Local models UI showed the installed Qwen2.5 1.5B Q5 model as checksum
verified. Starting it through the actual model control exposed the local
server on port 8221. The real dictation `ModelCleanup` adapter, using its local
route and the running server, transformed:

> um please bring apples oranges and bananas to the meeting tomorrow

into:

> please bring apples, oranges, and bananas to the meeting tomorrow

Two observed generations took 0.96 and 0.94 seconds. The opt-in native test
rejects the raw-text fallback, checks retained content, and checks that using
the adapter touches its endpoint idle clock. It requires an already-running
installed model; it is ignored in ordinary CI.

This tests generation through the cleanup adapter. It does not claim a complete
microphone-to-clipboard dictation: the attempted speaker-to-microphone test
was rejected as insufficient speech. The native application idle watcher was
observed separately, rather than inferred from the adapter's test endpoint.

With Local models left open throughout, a fresh UI start at 22:04:13 UTC
stopped automatically by 22:09:14 UTC. The five-second samples showed port
8221 until 22:09:09, then no port/model and `Installed, not running` on the
actual screen. The server process (PID 30456) had exited. This establishes
the five-minute shutdown and that status rendering does not keep it alive;
the sampling precision is five seconds. Evidence: `llm-idle-final.jsonl`.

## Media import

KNEG-90's disposable video used `Åsa video check/sample video.mp4`. The real
Windows picker and Upload button produced a note with three timed transcript
segments, a retained 10.68-second recording and working native playback/seek.
The source SHA-256 was unchanged before and after import:
`75A739FA04B2508591D0B64890375655CEBC1193DC3415F5D6918A3C4632F4C2`.

A separate 600-second synthetic WAV reached the transcribing stage before the
isolated application process was terminated. After restart its conversion
scratch was cleaned up. The upload screen showed neither the interrupted job
nor a retry notice. This user-visible recovery gap is filed as KNEG-177.
The real OpenWhispr database check remains outstanding on KNEG-90.

## Reproduction and evidence

The checked-out repository's ignored `target/issue-validation` directory holds
the native reports and screenshots: `meeting2`, `stream-revival`,
`failure-geometry.json/png`, `finishing.json/png`, `native-hidden.json`,
`video-import.json`, `interrupted-before.json`, `native-cleanup2.log` and
`native-cleanup3.log`. These are local evidence, not files shipped by this
commit. The disposable profile is under this task's Codex visualization
directory; its models and sidecars refer to installed assets, while its notes,
imports and WebView data are isolated.

Reusable native panel tooling is documented in `tools/visual-check/README.md`.
Start the development UI from `apps/desktop` with `npm run tauri:dev` and the
documented WebView2 debugger argument. Use a separate WebView data directory
when running an isolated second native instance.

Local checks passed: frontend typecheck/build and eight tests; 1,793 workspace
tests (14 opt-in tests ignored); and the desktop library's Tauri-enabled suite
(382 passed, six opt-in tests ignored). Rust 1.98 formatting and workspace
Clippy with warnings denied also passed. Rust 1.98 is used
for matching the hosted runner. The hosted follow-up is PR #38; its actual
Windows job, not these local results, supplies KNEG-40/KNEG-111's evidence.

## Hosted audio failure and follow-up

Run 34164649234 passed Windows Format and Clippy, then crashed in the
kw-audio-core test process with STATUS_ACCESS_VIOLATION (0xc0000005). Compilation
took about 51 minutes before execution reached the failing device-probe tests.
This is KNEG-178. CPAL 0.15.3 caches an IMMDeviceEnumerator created in the first
calling thread's COM apartment. That pointer can outlive the apartment, matching
[the upstream lifetime report](https://github.com/RustAudio/cpal/issues/1302).

The dependency is pinned to a local copy of the same 0.15.3 release with a small
WASAPI patch: create and release an owned enumerator on each calling thread.
The patch and provenance are documented in `vendor/cpal/KNEG_PATCH.md`. It
applies to production too. A separate integration-test process exercises eight
sequential probing threads. This test passes locally both before and after the
patch; the hosted environment must establish resolution of its observed crash.

Hosted checks now use the `ci` profile, with dependency optimization disabled
and limited debug information. Debug assertions remain enabled, while ordinary
development and release profiles retain their existing settings. Dependency
cache saving on failure prevents another test failure from throwing away the
entire cold build. Vendored files also select the Windows CI job.

Run 34168329984 subsequently passed all 86 hosted audio-core tests and the
separate thread-lifetime regression, confirming the crash is resolved. Its next
failure was KNEG-179: the ACL inheritance test assumed the file owner was the
process token user. Elevated Windows may assign Administrators as owner. The
test now checks the actual token user; the production access policy is unchanged.
Workspace CI uses `--no-fail-fast` to report failures across all packages in one
run while still failing the check if any test fails.

## Later on 8 September: the tray tooltip, the detection lists, the capture
## result, and the Apple Calendar row

A second autonomous native session, against `83c72ea`, covering the parts of
KNEG-51 that no test can reach and the one part of KNEG-81 that needs no Mac.
Isolated the same way as the earlier runs: `KW_AUTOMATION_PROFILE` pointed at a
disposable profile under `target/issue-validation/kneg51-profile`, with `models`
and `sidecars` as directory junctions to the installed assets so the run used
real models without copying 4.5 GB. **The owner's database was not opened.**
Its SHA-256 was recorded before and after and is identical
(`c4bd126e657ee051f216c9fe05b1cbd0adfe2d7a7d90c8050db490e6fef75055`); the
junctions were removed afterwards with `cmd /c rmdir`, which removes a reparse
point without following it, and the installed `models` (4.0 GB) and `sidecars`
(461 MB) were re-measured intact.

### The tray tooltip, during a meeting and a dictation at once (KNEG-51)

The state machine behind this was already unit-tested, including the
both-at-once case. What had never been observed is whether `tray.set_tooltip`
reaches the tray. It does. Read through UI Automation off the notification
area's `SystemTray.NormalButton`, across three states in one session:

| State | Tooltip read from the tray |
| --- | --- |
| Launched, no activity yet | `KnegisWhisper` — the builder's initial value; `set_tooltip` has not been called |
| `local-streaming` meeting recording | `KnegisWhisper — Recording a meeting` |
| Dictation started on top of it | `KnegisWhisper — Recording a meeting and a dictation` |
| Dictation cancelled, meeting still live | `KnegisWhisper — Recording a meeting` |

The last row is the one worth having: it shows on screen what
`the_two_recordings_do_not_overwrite_each_other` asserts in the unit test —
ending the dictation does not clear the meeting. Windows' own privacy indicator
corroborated the capture independently, reading `Microphone in use by:
KnegisWhisper` for the same period.

### The detection allow and deny lists (KNEG-51, and KNEG-114's remainder)

`meetings.detect` had never been driven by anything — not by a screen, not by
hand. It is now, through the real `Settings > Meetings` textareas and again
through `meeting_set_detect_lists` directly. `notepad.exe` was used as the
subject deliberately: certainly running, certainly not a meeting client, so it
tests the list without asserting anything about a real client's signature,
which is KNEG-58's separate question. Record was never pressed.

The list works. Within one poll the detection list gained
`{provider: "generic", name: "A program you listed", evidence: "notepad.exe",
inCall: false, signal: "none"}`, and both textareas persisted and read back.
Adding the same name to the deny list removed it again, and both lists were
restored empty.

**No offer appeared, and that is correct — but it contradicts what two issues
say.** `watcher::consider` filters to `Detected::in_call()`, and a
user-listed program is given `CallSignal::Microphone` only if it is actually
holding a capture session, otherwise `CallSignal::None`. Notepad holds no
microphone, so `offer` stayed `null` with `promptOnDetect: true`. KNEG-58's own
text states this correctly — *"a user-listed program is reported as running,
never as in a call"* — but KNEG-114's closing note, and the KNEG-51 checklist
item derived from it, promise an offer *"within five seconds"* for any process
the user lists. That wording is wrong for a listed program that is not
capturing audio. The escape hatch itself is real for the case it was written
for, since a Zoom in a call does hold the microphone; it is the "any process
you have running" phrasing that overstates it.

### The capture-delivery result, read rather than assumed (KNEG-51)

A 107-second `local-streaming` meeting with system audio requested, speakers
muted and nothing playing. `meeting_stop` returned:

```
outcome: nothing-transcribed   mic: audio   system: nothing   reconnects: 0
recording: …/kneg51-profile/meetings/20260908-205622.wav
```

So the two captures are reported separately and named, which is what the check
asked for: the microphone delivered audible audio, the system output delivered
nothing at all — the branch an idle output device produces, distinct from
digital silence and not folded into it. On screen:

> Nothing was transcribed. The microphone was recorded with audible sound in the
> recording, and none of it produced a transcript — so this is a transcription
> problem rather than a quiet meeting. The recording is at …

The right one of the three branches, and it sends the reader after a cause
rather than calling it a quiet meeting.

**One thing checked and found not to be a defect.** That sentence says nothing
about system audio having delivered nothing, even though it was requested. That
is `deliveryLabel`'s documented priority, not an omission: audible audio with no
transcript is reported first *wherever it happened*, because it is the only
branch that should send anybody looking for a cause. Recorded here because the
opposite conclusion was the obvious one to reach.

### The Apple Calendar row (KNEG-81)

`Settings > Calendar` renders the Apple section visible (746×145 at y=580, not
hidden, no empty calendar list) reading **"Not supported in this build"** with
the full EventKit reason beneath it, naming macOS explicitly as unimplemented
rather than merely unavailable. That is the whole of what KNEG-81 can settle
without a Mac; the binding itself still needs one.

### Evidence

Under the git-ignored `target/issue-validation/`:
`kneg51-81-part1.json`, `kneg51-detection.json`,
`kneg51-detect-readback.json`, `kneg51-stop-outcome.json`,
`kneg51-screen-after-stop.json`, `owner-db-before.txt`, `tauri-dev.log`, and
the disposable profile itself. Local, not shipped by this commit.
