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
