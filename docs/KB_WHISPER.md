# KB Whisper local dictation

Settings > Transcription > Local offers five KB Whisper models: **Tiny**,
**Base**, **Small**, **Medium** and **Large (Swedish, Q5)**. Click Install, then
Use for dictation. They run on the existing whisper.cpp server, including
dictation started from the floating microphone. Model selection persists and the
existing server recovery applies. After installation, local transcription
requires no cloud credentials.

Medium and Large are labelled "slower; best for imports and meetings", as is every
Whisper model of 500 MB or more (`whisperModelHint`). One Whisper model runs at a
time, so the model chosen for long recordings is also the one dictation uses, and
the screen says so under the list. A separate model for long recordings was
considered and not built (KNEG-407).

The **dictation languages** picker is available in Settings > Transcription,
under where dictation goes, from the language chip on the Dictation page on every
route but a Parakeet model's, and from the `>` button beside the floating
microphone. Pick one or two languages, or choose Detect automatically to clear
the selection. The choice persists and stays in sync between all three. On local
Whisper a single language forces that decoder language. Two languages first run
local language detection, then decode using whichever selected language has the
highest reported probability. This adds a detection pass and chooses one
language per dictation; it does not guarantee accurate language switching within
one sentence. If neither selected language has a reported score, the app asks
for automatic detection or a single language.

Supported choices are Swedish, English, Danish, Norwegian, Finnish, German,
French, and Spanish. Parakeet models keep their model-defined languages, and the
picker is off while one is in use. Changing this setting does not restart the
model.

The same choice applies to cloud dictation and to imports, which share the
dictation route (KNEG-370): a cloud provider is sent the language when exactly one
is chosen, and detects it itself when none or two are. Two provider rules apply on
top, in `route::resolve`: xAI is not told a language outside the 25 it accepts
(Norwegian and Finnish among the eight here), and Corti, which needs one, is told
English unless exactly one is chosen. The line under the picker says which, read
from the language the resolved route carries (`RouteReport.cloud_language`). That
each provider accepts each of the eight codes is believed, not measured: no cloud
dictation with a chosen language has been run against a live provider.

These are KBLab's official default Stage 2 GGML Q5 models, specialized for Swedish.
They supplement the existing Whisper models. The downloads are approximately
30 MB (Tiny), 55 MB (Base), 175 MB (Small), 540 MB (Medium) and 1.1 GB (Large).
The quantized files reduce download and storage size. This integration does not
claim a measured accuracy or latency advantage on the user's recordings: that
larger models are more accurate and slower is believed from how Whisper sizes
behave, not timed here.

- [KB Whisper Tiny model card](https://huggingface.co/KBLab/kb-whisper-tiny)
- [KB Whisper Base model card](https://huggingface.co/KBLab/kb-whisper-base)
- [KB Whisper Small model card](https://huggingface.co/KBLab/kb-whisper-small)
- [KB Whisper Medium model card](https://huggingface.co/KBLab/kb-whisper-medium)
- [KB Whisper Large model card](https://huggingface.co/KBLab/kb-whisper-large)
- Tiny's and Base's model cards declare Apache-2.0 licensing. The other three
  cards were not re-read for this change.

The generator's `OWN_WHISPER_MODELS` adds these models without changing the frozen
upstream baseline. URLs use immutable publisher revisions and every download is
checked against a measured SHA-256 and byte count. Distinct `ggml-kb-whisper-*`
cache filenames prevent collisions with the existing models or each other.

## Validation on Windows, 2026-09-10

Both official downloads were measured with `tools/model-pin/pin.mjs --url ...
--keep ...` and matched the publisher's LFS metadata:

| Model | Revision | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| Tiny | `76d796af43a50fa34321efa562c9b9887a187463` | 29,875,738 | `98d46b7d23e5528d006e8a42e29eb0cb39b44bed94e1329f10f57d1fd15c658b` |
| Base | `1499d2d2f0c7ed545bd6f2eec85287cf8d8c8b38` | 55,295,450 | `aead29b356bca8840e72a8dc2286e2d69e6702639751a1e60cb3c8eacefec546` |

The preview app's actual install commands returned digest verification for both
models. Both started using the app-managed server and returned nonempty text from
`/inference` for `crates/transcription-core/tests/fixtures/speech-16k-mono.wav`.
The dictation route reported local execution with no problem and no audio leaving
the device. This is runtime compatibility evidence, not a Swedish accuracy test:
the fixture contains English speech. Live Swedish dictation and target-editor
insertion remain hands-on acceptance checks.

Provider-registry tests (17), transcription-core tests (146), frontend tests (13),
frontend/native builds, provider-registry Clippy, generated registry/type checks,
and integration parity passed. The new registry test checks that both KB models
are present, pinned, labelled Swedish, and have unique cache filenames.

## Small, Medium and Large, 2026-10-07

Added for KNEG-407. The revisions are what the Hugging Face API gave for `main` on
2026-10-06. Each file was downloaded at that commit with `tools/model-pin/pin.mjs
--url ...` on 2026-10-07, and each digest and byte count agrees with the LFS
metadata the API stated:

| Model | Revision | Bytes | SHA-256 |
| --- | --- | ---: | --- |
| Small | `3564d61a42fc210ceaa55a22a96dd64478959c78` | 175,209,680 | `6768836a51abc902e420c613153e6d418c90ea2774e913274d02ab23170225b7` |
| Medium | `0abe10b9d7f75d0902656e5c06c5c4d549604dc5` | 539,212,484 | `7f8762e0ade9e0073674c0d5acae942a0b1ea98add9baa008ee89c94eaba43d0` |
| Large | `d5d5984b4d8f7c4847a8ea203f1976285fb28300` | 1,081,140,203 | `6d2863812d7410322bb7d8647a5c7260761300fa946714c9ed66d22bb30bcb19` |

That is the whole of what was measured. None of the three has been installed
through the app, started on its server, or given a recording: installing each,
timing a Swedish import with each, and cancelling Large's 1.1 GB download midway
are by-hand checks for the owner. The registry test now checks all five KB models
are present, pinned, labelled Swedish, and have unique cache filenames, and the
weekly *Sidecar pin verification* job re-downloads and re-hashes all three.
