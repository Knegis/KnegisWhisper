# Parakeet long-input benchmark — 2026-09-07

The selected installed `parakeet-tdt-0.6b-v3` INT8 model completed 1- and
5-minute repeated-speech fixtures and failed on 15- and 45-minute fixtures.
This reproduces a native long-input failure; it does not identify its root cause.
Whisper independently completed the owner's 45-minute recording and playback.

| Audio duration | Wall time | Peak working set | Sampled CPU time | Result |
| --- | --- | --- | --- | --- |
| 1 minute | 10.02 s | 1.43 GB | 15.66 s | exit 0 |
| 5 minutes | 59.56 s | 2.92 GB | 107.55 s | exit 0 |
| 15 minutes | 16.02 s | 10.66 GB | 20.95 s | exit 0xC0000409 |
| 45 minutes | 297.94 s | 19.11 GB | 235.12 s | exit 0xC0000409 |
| 15 minutes, confirmation | 18.02 s | 10.66 GB | 23.16 s | exit 0xC0000409 |

Successful runs took 0.167 and 0.199 seconds per second of audio. Ratios for failed
runs are not throughput measurements. Process creation took 7–12 ms; the CLI
exposes no separate model-ready boundary, so model loading and inference times
cannot be separated from this observation. All runs plus confirmation used less
than seven minutes of execution time within the agreed 30-minute budget.

Hardware: AMD Ryzen AI 7 PRO 350, 16 logical CPUs, 29,684,842,496 bytes physical
RAM, Windows 11 build 26200. The CLI's default two threads were retained. Builds
were active on the same machine; this was not a controlled performance lab.
Fixtures repeat `crates/transcription-core/tests/fixtures/speech-16k-mono.wav`
to 1, 5, 15 and 45 minutes as 16 kHz PCM audio. Repetition is not representative
of natural long-form speech. The original user recording was not read or changed.

Binary SHA-256: `58cb45b3b08dea9141f801cae3d7484d6d27fd8c65b1b39045fe076d08e0f6d1`.
Model encoder SHA-256: `acfc2b4456377e15d04f0243af540b7fe7c992f8d898d751cf134c3a55fd2247`.
Full model hashes, per-run results and benchmark scripts are under ignored
`target/delivery/`. Every generated WAV was removed and every child exited.
Stdout was discarded; stderr was drained and discarded, retaining only byte
counts and predefined error markers in the confirmation run. That run identified
an ONNX Runtime marker but no allocation-specific marker. High memory use is
measured; memory exhaustion, model defects and hardware limitations are not
established explanations for the native exit.

Delivery hardening preserves the complete native exit code and final stderr in
the returned error, and cleans up the process/scratch WAV on cancellation.
App-owned import state retains failures after navigation. No model replacement,
GPU integration, automatic retries or chunking was introduced. A separate
follow-up should investigate this native failure using the reproducer before
choosing a model/runtime change or chunking strategy. Whisper is the measured
working option for the owner's long recording.

## Follow-up investigation — 2026-09-07

A fresh isolated 15-minute reproduction using the same installed model and
binary exited with `0xC0000409` after 17.21 seconds. Stderr drained completely,
but the diagnostic filter found no exception/allocation error line. A targeted
Application event-log query found no matching recent crash report. Neither
observation identifies the native failure cause. The generated WAV was removed.

The same repeated-speech signal was then divided into three contiguous five-minute
pieces, preserving the source repetition offset at each boundary. Each piece ran
in a separate sequential process, with the same model, binary and default two
threads. This was an experiment, not a production code change.

| Piece | Wall time | Peak working set | Exit |
| --- | --- | --- | --- |
| 0–5 minutes | 56.06 s | 2.920 GB | 0 |
| 5–10 minutes | 56.06 s | 2.920 GB | 0 |
| 10–15 minutes | 54.06 s | 2.919 GB | 0 |

All three completed in 166.18 seconds of process time. The ten-minute experiment
cap was not reached. All children exited and generated WAVs were removed. Stdout
was discarded, so successful exit does not establish transcript completeness or
quality at the boundaries. Measurements and scripts are in ignored
`target/delivery/parakeet-diagnosis.json` and `parakeet-chunk-probe.json`.

### Upstream evidence and interpretation

[NVIDIA's model card](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3)
distinguishes full attention (its 24-minute example uses an A100 with 80 GB) from
local attention for longer recordings, and demonstrates changing the attention
configuration in NeMo. The [sherpa v1.13.4 export script](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.4/scripts/nemo/parakeet-tdt-0.6b-v3/export_onnx.py)
exports the default model without that attention change. The app currently passes
one complete WAV to its offline CLI; it does not apply a long-form inference path.

Input-length-dependent encoder memory is therefore a supported hypothesis, not
a confirmed allocation failure or a diagnosis of insufficient hardware. The
same binary and machine successfully handled all three shorter pieces. These
sources describe the upstream configuration; inspecting an export script alone
does not establish every property of the installed ONNX graph.

### Proposed next implementation

Prototype bounded, sequential chunks only for Parakeet imports, using five minutes
as an initially measured upper bound rather than a universal safe limit. Preserve
the original retained WAV, apply chunk offsets to transcript timings, and validate
word/segment continuity before selecting an overlap/deduplication policy. Do not
silently accept missing chunks, replay a submitted failed chunk, switch providers,
or claim a percentage of internal model inference. The existing import slot,
failure reporting and cleanup must cover every child and temporary chunk.

Before shipping: measure a complete 45-minute fixture, assert timing offsets and
boundary content on known speech, test failure/cancellation cleanup and playback
against the retained full recording. A new runtime/model export, GPU integration
or native crash dump investigation remains an alternative follow-up, not a change
made by this experiment.
