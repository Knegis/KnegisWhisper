# Known voices implementation and validation

KNEG-319, 2026-11-09. This implementation recognizes explicitly remembered
speakers after local meeting diarization. It does not perform live identification.
The release policy in `crates/notes-core/src/known_voices.rs` is deliberately
`None`: normal settings and enrollment cannot enable this unvalidated feature.
Changing a generic setting cannot bypass the gate. Do not substitute the old
unlabeled spike's similarity values for a calibrated policy.

## Behavior and data

Known voices is a persisted opt-in. After successful local diarization, a blocking
worker reads mono PCM16/16 kHz audio on the returned turns, excludes overlapping
turns, splits long turns into at most 30-second chunks, and skips chunks shorter
than one second. It normalizes each 512-dimensional vector, forms a
duration-weighted normalized centroid, and records usable speech duration.
Native provider speaker labels bypass this pipeline.

The note-save transaction rechecks opt-in and matches against current profiles.
Both a minimum cosine score and a margin over the runner-up are required.
Several clusters can match one person. Errors retain the transcript and ordinary
labels. A savepoint prevents partial recognition writes from breaking note save.
Recovery uses the same path. No embeddings are added to transcript JSON or shared
folder payloads.

Enrollment requires the explicit Remember checkbox and a confirmed name. A user
may create a profile or explicitly replace an existing profile's sample.
Automatic matches never modify profiles. Manual names and manual removal survive
recognition retries. Renaming a profile affects future meetings; saved note names
remain editable snapshots. Forget deletes the profile and its enrollment-source
cache. Switching off retains profiles but stops extraction and matching.

Migration 0013 adds model/format/duration metadata, mapping origin, enrollment
references, and manual-override tombstones. Legacy embeddings remain stored but
cannot match until explicitly re-enrolled. The model identity includes its SHA-256
and the extraction version. Unknown dimensions, non-finite vectors, and zero
norms are rejected.

Desktop IPC: `known_voices_status`, `known_voices_set_enabled`,
`known_voice_enroll`, `known_voice_rename`, `known_voice_delete`, and
`known_voices_repair`. Types are generated through the existing Rust IPC generator.

## Windows runtime finding

The pinned sherpa 1.13.4 archive is unchanged. Installation now includes
`voice-runtime/{sherpa-onnx-c-api.dll,onnxruntime.dll,onnxruntime_providers_shared.dll}`.
The measured selected archive payload is 65,825,792 bytes, reflected in the
manifest extraction ceiling. Older bin-only installations can use Repair.

The first native smoke resolved ONNX Runtime 1.17.1 and crashed because the C API
requested API 27. Both archive copies of ONNX Runtime have identical hashes;
the older library came from outside the selected bundle. Explicitly preloading
the bundled runtime and checking API 27 before constructing sherpa objects fixes
this. DLL loading is scoped to the selected directory and system search paths.
The extractor checks sherpa version and model checksum. The C API ABI comes from
the [pinned upstream header](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.4/sherpa-onnx/c-api/c-api.h).

A five-second synthetic tone produced one finite 512-dimensional embedding in
0.83 seconds, including initialization. This verifies extraction/ownership only;
it establishes neither recognition accuracy nor the performance target on meetings.

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
cargo run -p knegiswhisper --example voice_samples -- LIBRARY MODEL WAV DIARIZER_STDOUT
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

## Automated validation in the implementation worktree

- Notes core: 174 unit tests and two workflow tests passed, including savepoint rollback and new
  matching, opt-in, explicit enrollment, manual override, deletion, and restart tests.
- Sidecar: 170 unit tests passed; archive fixture suites passed. The explicit
  pinned Windows archive test passed with all three voice-runtime libraries.
- Storage: migration/reopen and existing data tests passed after advancing the
  expected migration count to 13.
- Desktop: 557 tests passed, 16 existing tests ignored. Generated IPC types checked.
- Frontend: 568 tests passed; typecheck and production Vite build passed.
- Capability audit: 20 tests passed. Existing speaker-rename visual regression:
  six checks passed in headless Edge with invented fixtures. Known voices settings
  gate checked disabled/unchecked with an explanation and no horizontal overflow.
- Evaluator: two tests passed, including reserved-file isolation during calibration.
  A synthetic end-to-end calibration/freeze/evaluate run also passed; it is not
  recognition-quality evidence.
- Native extraction: pinned model and archive, synthetic tone only; not a voice
  quality test. No real application settings or database were modified.

Local evidence is in the worktree's `.validation/` directory. Packaged installation
and upgrade with real users, hour-long timing, live microphone validation, and
podcast identity labeling remain outstanding. The changes are not merged or pushed.
