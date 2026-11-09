# Known voices implementation and validation

KNEG-319, 2026-11-09. Returning speakers are matched after local diarization.
The release policy in `crates/notes-core/src/known_voices.rs` remains `None`.
Settings cannot enable recognition or enrollment until labeled validation passes.
There is no settings or environment override.

## Retention and enrollment

Only explicitly enrolled profiles persist voice embeddings. Per-meeting vectors
are transient worker output used for matching, then dropped. Migration 0014 deletes
**all** existing `note_speaker_embeddings`, including samples from unremembered
speakers and other meetings. A database trigger refuses new cache inserts, and the
OpenWhispr import no longer copies that table. Explicit profiles and saved note
names survive the upgrade. Incompatible legacy profiles remain visible/deletable
but cannot match.

The note may retain non-biometric speaker-turn metadata: original diarizer times
and cluster IDs, recording storage key, WAV SHA-256, duration, and extraction
pipeline identity. This is not a voice embedding or a substitute audio clip.
The existing recording-retention choice still controls the WAV.

Remember requires a confirmed name, retained original recording, and this original
turn metadata. It runs extraction on demand with all original turns so overlap
exclusion stays identical. It never substitutes transcript timestamps or reruns
clustering to guess the old speaker IDs. Missing recording/turn metadata disables
the checkbox with an explanation; old notes can still be named normally.

The worker checks recording identity before and after extraction. Enrollment
rechecks it again, then transactionally rechecks the attachment, source metadata,
note visibility, opt-in, and generation. Disable/re-enable, forgetting, and manual
name corrections invalidate results already in flight. Explicit enrollment can
create a profile or replace a selected profile. Automatic matches never update
profiles. Forget deletes the profile's embedding; there are no retained samples
in other meetings to restore it. Switching off keeps the visible profile list,
with Forget available while off. Already saved names and retained recordings are
independent of voice profiles. These operations remove live database records;
they do not erase external backups or promise forensic disk erasure.

Matching requires both a minimum cosine score and margin over the next profile.
Several clusters may match one person. Manual names and manual removal take
precedence. The note transaction reads current profiles; a savepoint isolates
recognition-write failures so the transcript still saves. No embeddings appear
in transcript JSON, shared folders, temporary files, or worker diagnostics.

## Worker isolation and recovery

Native inference runs only in a child of the packaged application executable,
started with `--known-voices-worker` before Tauri, windows, settings, or the app
DB initialize. The parent sends a versioned JSON request over stdin and reads
JSON on stdout. Each pipe has a 16 MiB bound; stderr is drained without retaining
it in application logs. Protocol/model identity, speaker IDs, vector dimensions,
finite values, norms, durations, and duplicates are checked before use.

The deadline is `min(600, 30 + 0.1 * recording_duration_seconds)` seconds. Failure,
crash, or timeout retains ordinary speaker labels and the transcript, with a note
explaining the skipped recognition. Timeout/error kills and reaps the child.
Dropping the task kills it; Windows also assigns the worker to a kill-on-close Job
Object before sending its input, so parent termination kills the child too.

Crash recovery never automatically runs native voice extraction. It may save
original turn metadata for a later explicit Remember action. Worker inference
errors still allow the normal recording disposition and note-save path.

Extraction uses mono PCM16/16 kHz audio on diarization turns, excludes overlap,
splits long turns into at most 30-second chunks, and skips chunks under one
second. It normalizes 512-dimensional embeddings and forms a duration-weighted
normalized centroid. The pipeline identity includes the pinned model SHA-256
and extraction version. Native provider speaker labels bypass this pipeline.

## Separate Windows runtime

Ordinary speaker-label model installation checks only whether its existing CLI
is installed. It never repairs Known voices or redownloads sherpa because the
voice runtime is missing, regardless of whether recognition is on or off.

Explicit Known voices installation/repair uses the unchanged pinned sherpa
1.13.4 archive (24,378,225 bytes, SHA-256
`c312b30e55258d291067537ce4a6f90e155f1c16b6a10381a5729924e98b7879`).
It reuses a verified `known-voices-archive.tar.bz2` cache, otherwise downloads once
under the normal user-triggered network policy. It extracts exactly three DLLs
from `lib/`: `sherpa-onnx-c-api.dll`, `onnxruntime.dll`, and
`onnxruntime_providers_shared.dll`, bounded to 22,013,440 extracted bytes.
The ordinary `bin/` payload retains its original 55,574,528-byte ceiling.

The runtime lives in `sidecars/known-voices-1.13.4`, outside the shared sherpa
installation. A staging directory is verified before publication. Failed
replacement preserves/restores the previous runtime. A Windows exclusive file
lease serializes extraction and repair across processes, releases on crash, and
stays held until blocking publication finishes even if its caller is cancelled.
A locked streaming-server/CLI directory is never renamed or removed by repair.

The initial native spike crashed after resolving an unrelated ONNX Runtime 1.17.1
while sherpa required API 27. The worker still explicitly preloads its bundled
runtime with scoped DLL search paths and checks API 27 and sherpa version.
The model checksum is checked before inference. Isolation now contains failures
that cannot be expressed as a Rust error, including native access violations.

## Podcast evaluation protocol

The owner suggested playing podcasts through Spotify because labeled cross-day
meeting recordings are not yet available. Use normal meeting capture with system
audio on, microphone off, and speaker labels on. Known voices stays disabled.
Keep each recording, episode URL, playback timestamp range, language, capture day,
and manually confirmed speaker labels in a local evaluation folder. Do not commit
podcast audio or embeddings to Git.

Choose at least three recurring people, each in three distinct episodes. Reserve
whole episodes for enrollment, calibration, or final evaluation; do not divide
one episode among splits. Include at least three unenrolled speakers in calibration
and at least three in the final set. Include Swedish and English material. Listen
to each diarization cluster: exclude mixed/uncertain clusters and document those
exclusions. Label all remaining clusters, including unknown participants.

Podcast results test recorded voices and system-audio capture. They do not replace
a small live test with the same people on different days and a microphone or room
change. Record source-session dates where known; episode publication dates are not
proof of separate recording days. A podcast-only result does not open the release gate.

## Reproducible tools

Run the pinned diarization executable with the application model paths and tuning
(`--clustering.cluster-threshold=0.5`, `--min-duration-on=0.3`, `--min-duration-off=0.5`;
use `diarization::build_args` as the authoritative argument builder), saving stdout.
Then run the production extractor without opening or modifying the application DB:

```text
cargo run -p knegiswhisper --example voice_samples -- APP_EXE LIBRARY MODEL WAV DIARIZER_STDOUT
```

Capture its stdout as UTF-8 JSON; Windows PowerShell 5 redirection defaults to
UTF-16, so use `Out-File -Encoding utf8` or Python subprocess
capture when preparing files for the evaluator. Each output contains model identity,
embedding format, extraction duration, and a sample per usable cluster.

Create a UTF-8 manifest array. One row describes a manually labeled cluster:

```json
{"samples_file":"episode-a.json","samples_sha256":"REPLACE_WITH_FILE_SHA256","speaker_id":"speaker_0","person":"host-a","session":"episode-a","day":"2026-11-10","split":"enroll","language":"sv","condition":"spotify-system-audio"}
```

Populate `samples_sha256` with each file's lowercase SHA-256 before calibration,
including held-out files. The evaluator verifies these hashes before reading vectors.
Use one explicit enrollment sample per person. Include missing/too-short clusters
in the manifest so exclusion counts remain visible. Then:

```text
python tools/speaker-spike/evaluate.py calibrate manifest.json frozen-policy.json
python tools/speaker-spike/evaluate.py evaluate manifest.json frozen-policy.json --report held-out-report.json
python -m unittest discover -s tools/speaker-spike -p test_evaluate.py
```

Calibration searches a fixed grid of threshold, margin, and minimum duration using
only enrollment/calibration vectors. It freezes manifest and input hashes and never
reads held-out vectors. Evaluation verifies frozen inputs, rejects split overlap,
reports incorrect names, unknown acceptance, recall, and exclusions, and refuses
to overwrite reports. Preserve the first held-out result; if it fails, develop on
new recordings and reserve a new final set rather than tuning on the old test.

## Release acceptance

- Zero incorrect names and zero accepted unenrolled speakers; at least 80% of
  eligible returning speakers recognized. Inspect exclusions and per-person,
  language, and recording-condition results before accepting the aggregate.
- Finish the live cross-day/microphone check and clean-install/upgrade UI flow:
  enable, record A, explicitly enroll, restart, record B, correct, forget, disable.
- Measure additional processing on short and hour-long recordings against the
  same recordings with recognition off. Target less than 5% added real-time factor.
- Record the frozen policy, source hashes, sample counts, limitations, and packaged
  build identity. Only then set `RELEASE_POLICY`, update its gate assertion, and
  repeat the integration tests. There is no settings or environment override.

These quality, live hardware, and packaged-app gates remain pending; no recognition
accuracy or production-readiness claim is made by the automated smoke tests.

## Automated validation of the remediation

- 176 notes-core tests and two workflow tests passed, including no per-meeting
  caches, forget/disable races, repeated manual corrections, attachment changes,
  persistence across restart, and note-save rollback.
- 77 storage tests passed, including upgrading enrolled and unremembered caches,
  preserving profiles/names, refusing cache insertion, and excluding imported caches.
- 170 sidecar unit tests plus archive fixtures passed. The real pinned archive
  test confirmed exactly three voice DLLs and the unchanged bin-only budget.
- 560 desktop tests passed with `tauri-test` (17 hardware/external tests ignored).
  The separately run pinned-archive repair test passed with a locked sherpa file,
  a locked voice DLL, a corrupted archive, and repeated successful repair.
- The worker protocol integration executable passed a valid response, native-style
  abort, malformed/oversized stdout and stderr, wrong protocol/model/speaker/vector/
  duration, duplicate samples, timeout with reap, and task-cancellation termination.
  Fault modes exist only in the test executable, never the shipped worker.
- A Windows GUI executable (PE subsystem 2), with embedded production assets,
  passed actual worker extraction on a five-second synthetic tone: one finite
  512-value vector in 0.48 s including startup on the final build. It rejected malformed and >16 MiB
  input and created no app-data files. This checks isolation/plumbing, not quality.
  Built with `cargo rustc -p knegiswhisper --bin knegiswhisper-desktop --features
  tauri/custom-protocol --offline -- -C debug-assertions=no`; this used development
  optimization and was not an installer or release-performance test. Binary SHA-256:
  `31c2b23aa7be7911fd61f7d82219c47bb1e6d59f30ece7740ff74270ec519ec3`.
- All-target Clippy with warnings denied, generated IPC types, 568 frontend tests,
  typecheck/Vite build, and 20 capability-audit tests passed.
- Isolated Edge fixture checks passed: release gate, missing recording, missing
  original turns, explicit enrollment, disable retaining profiles, Forget while off,
  and no horizontal overflow. The six existing speaker-rename visual checks also
  passed. This is UI evidence with invented data.

Reproduce the worker fault checks with
`cargo test -p knegiswhisper --test voice_worker_protocol --offline`.
Set `KW_SHERPA_ARCHIVE` to the pinned archive for the ignored
`voice_runtime::archive_tests` and `known_voices_archive` tests.
Run `node tools/visual-check/known-voices.mjs` against the worktree's fixture Vite
server on port 5197 for the UI checks.

Local evidence is in `.validation/`. Recognition-quality, live microphone,
hour-long timing, and real clean-install/upgrade acceptance remain pending.
The changes are not merged or pushed.
