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
