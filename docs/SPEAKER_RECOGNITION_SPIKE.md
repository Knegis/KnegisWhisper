# Speaker recognition across meetings — spike, 2026-09-25

Measured on the owner's machine for KNEG-319 ("Known voices"). The question: can a voice named
in one note be recognised in the next meeting, and by which route? Two routes were on the table
and neither had a number behind it. This file holds the numbers. Nothing here shipped; the scripts
were throwaway Python over the same models the application already installs.

## Setup

- **Recordings:** the ten meeting notes with speaker labels and a recording on disk (notes 17, 18,
  20, 21, 22, 27, 28, 29, 30, 31; 35–129 s each; 16 kHz mono; recorded 2026-09-03 to 09-08).
  **Nobody has labelled who speaks in them.** That limit runs through every conclusion below.
- **Models:** the pinned pair the application diarises with — pyannote segmentation 3.0 and the
  CAM++ English speaker-verification embedding (`3dspeaker_speech_campplus_sv_en_voxceleb_16k`,
  512-dimensional output) — through the `sherpa-onnx` 1.13.8 Python bindings at the
  application's own tuning (`cluster-threshold 0.5`, `min-duration-on 0.3`, `min-duration-off
  0.5`), and the shipped `sherpa-onnx-offline-speaker-diarization.exe` 1.13.4 for route 2.
- **Controls:** seven of sherpa's bundled ASR test WAVs (other people, other microphones, five
  languages), 20 s each, as voices that are certainly not in any recording.
- Cosine similarity between L2-normalised embeddings throughout.

## Route 2: prepend a reference clip, run the shipped diarisation once — rejected

Eight seconds of a voice from recording A were placed in front of recording B with half a
second of silence, and the CLI was run on the joined file. Twelve pairs.

| fact | measured |
| --- | --- |
| B's cluster count changed by the prepended clip | 10 of 12 runs (3→5, 3→6, 4→3, 3→4, …) |
| the clip's cluster | its own, first-heard cluster in 12 of 12 |
| clip's cluster coincided with the voice route 1 predicted for it in B | 2 of 12 |
| wall time per run | 4–17 s |

The first row is a fact that needs no labels: a reference clip changes the clustering of the
meeting it was meant to help label. That alone rules the route out — it would make the speaker
turns of every meeting depend on which voices are enrolled.

## Route 1: an embedding per speaker, compared across recordings

### First pass, on transcription segments — inconclusive, and the confound was ours

Embedding the saved transcription segments (the units the application assigns a speaker to by
overlapped duration) gave different speakers *inside one recording* cosines of 0.19–0.92 and
cross-recording pairs up to 0.99. A transcription segment can straddle a turn change, so the
embedding is a blend; the pass says nothing reliable either way.

### Second pass, on diarisation turns — the model separates voices, and the truth is missing

The same pipeline through the Python bindings, cutting audio on the turns it returned, one mean
embedding per cluster.

**The Python run reproduces the application's clustering** on seven of ten recordings and finds
one more cluster on three (22: 1→2; 27: 4→5; 28: 3→4), so it is the same pipeline within the
usual run-to-run variance of the clusterer rather than a different one.

**Inside one recording**, each turn against its own cluster's centroid versus the nearest other
cluster's centroid:

| note | turns | mean own | mean nearest other | turns closer to another cluster |
| --- | --- | --- | --- | --- |
| 17 | 4 | 0.973 | 0.214 | 0 |
| 18 | 3 | 1.000 | 0.659 | 0 |
| 27 | 22 | 0.739 | 0.700 | 9 |
| 28 | 28 | 0.793 | 0.715 | 9 |
| 31 | 4 | 0.928 | 0.497 | 0 |

Three recordings are cleanly separated. Notes 27 and 28 (77 s and 111 s, recorded 33 minutes
apart on 2026-09-07, five and four clusters) are not: nine turns in each sit closer to another
cluster than to their own, which reads as one voice split several ways rather than four or five
people. That is a finding for KNEG-125 (are the turns right on a real recording) as much as for
this issue.

**Across recordings**, the strongest pairs are all among the 2026-09-07 recordings: 27/28 at
0.933 and 0.931, 27/29 at 0.924, 28/29 at 0.896. The bottom of the table is −0.15. So the model
does carry a voice from one recording to the next when it is the same voice on the same evening.

**The five single-cluster recordings** (20, 21, 22, 29, 30 — one voice each, presumed the owner
at the same desk) against each other: −0.06 to 0.47, ten pairs. Either they are not the same
person (they are short test recordings, and could be anything played into the room), or the model
does not carry a voice across days. Only a labelled recording can say which, and that is the
one thing this spike could not produce.

**Controls** (voices certainly absent from every recording) against every recording cluster:
maximum 0.25–0.81 per control, mean 0.04–0.42; the seven controls against each other average
0.33 with one pair at 0.87. So a different voice can score 0.8 on a single clip, while the
same-evening matches above sit at 0.89–0.93. There is a margin on this data; whether it survives
different days, rooms and microphones is the unmeasured half.

**Cost**, on this machine: diarisation 0.9–10.3 s per recording (real-time factor 0.05–0.08,
matching the 0.081 recorded on 2026-09-01), embedding every turn of a recording 0.1–0.9 s more.
Embeddings are not the expensive part.

## What this decides

1. **Route 2 is out**, on the perturbation fact alone.
2. **Route 1 is the route**, on diarisation turns, never on transcription segments.
3. **No threshold and no accuracy claim can be made yet.** The margin seen (same evening ≥ 0.89;
   strangers ≤ 0.81 on 20-second clips) comes from unlabelled recordings and one evening. The
   settings surface KNEG-319 describes — an opt-in, a list of known voices, a strictness control —
   needs a labelled set first: two or three people, each in two recordings on different days
   through the normal meeting path, with the owner saying who is who. That is a by-hand task
   and it is the next step, not the code.
4. **The in-process embedding needs no new download.** The Windows archive already pinned for the
   `sherpa` sidecar (`sherpa-onnx-v1.13.4-win-x64-shared-MT-Release.tar.bz2`, sha256 verified
   against `config/sidecars.json`) carries `lib/sherpa-onnx-c-api.dll` and
   `include/sherpa-onnx/c-api/c-api.h` with the `SherpaOnnxSpeakerEmbeddingExtractor` functions;
   the installer extracts only `bin/` today. Loading that library beside the `onnxruntime.dll`
   the sidecar already ships, through `libloading` (already in `Cargo.lock` as a transitive
   dependency, not yet a direct one), is the route that keeps the project's "no `ort` crate"
   position. The Python bindings used here ship the same DLL, which is how the numbers above were
   produced.
5. **Over-splitting is a design input, not a blocker.** When one voice is split into several
   clusters (notes 27, 28), a profile will match several clusters of one meeting; the screen
   should show one name on all of them rather than refuse.

## Method, for re-running

The two throwaway scripts are kept under `tools/speaker-spike/` with a README that says how to
run them: a virtual environment with `sherpa-onnx==1.13.8` and `numpy`, the two diarisation
models installed by Settings > Meetings > Speaker labels, and Python 3.14. They read the
database read-only and the recordings under `%APPDATA%\KnegisWhisper\meetings`, and write
every derived file beside themselves. Nothing imports them.
