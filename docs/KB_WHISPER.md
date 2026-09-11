# KB Whisper local dictation

Settings > Transcription > Local offers **KB Whisper Tiny (Swedish, Q5)** and
**KB Whisper Base (Swedish, Q5)**. Click Install, then Use for dictation. They run
on the existing whisper.cpp server, including dictation started from the floating
microphone. Model selection persists and the existing server recovery applies.
After installation, local transcription requires no cloud credentials.

The **Whisper dictation languages** picker is available in Settings and from the
`>` button beside the floating microphone. Pick one or two languages, or choose
Detect automatically to clear the selection. The choice persists and stays in
sync between both surfaces. A single language forces that decoder language.
Two languages first run local language detection, then decode using whichever
selected language has the highest reported probability. This adds a detection
pass and chooses one language per dictation; it does not guarantee accurate
language switching within one sentence. If neither selected language has a
reported score, the app asks for automatic detection or a single language.

Supported choices are Swedish, English, Danish, Norwegian, Finnish, German,
French, and Spanish. The setting applies to local Whisper dictation; other local
engines retain their model-defined languages. Changing this setting does not
restart the model or change cloud-provider language preferences.

These are KBLab's official default Stage 2 GGML Q5 models, specialized for Swedish.
They supplement the existing Whisper models. Tiny downloads approximately 30 MB;
Base downloads approximately 55 MB. The quantized files reduce download and
storage size. This integration does not claim a measured accuracy or latency
advantage on the user's recordings.

- [KB Whisper Tiny model card](https://huggingface.co/KBLab/kb-whisper-tiny)
- [KB Whisper Base model card](https://huggingface.co/KBLab/kb-whisper-base)
- Both model cards declare Apache-2.0 licensing.

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
