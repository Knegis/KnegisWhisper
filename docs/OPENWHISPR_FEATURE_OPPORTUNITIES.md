# Feature opportunities inspired by OpenWhispr

Reviewed: 2026-09-20

Comparison of OpenWhispr's current repository, selected source files and recent
changes with the KnegisWhisper checkout. This is a code-review-based recommendation,
not runtime validation of either application. Upstream links point to `main` and may
change. These proposals are not implementation commitments.

## Recommended priorities

| Priority | Improvement | Current KnegisWhisper foundation | Proposed addition |
| --- | --- | --- | --- |
| 1 | Meeting summaries and action items | Editable notes, timed transcripts, playback and reasoning-provider infrastructure | A Generate summary action covering decisions, commitments and unresolved questions, with links to supporting transcript rows |
| 2 | Reliable long recordings and batch imports | One active import and interruption recovery; long Parakeet inputs remain a documented problem | Bounded audio chunks, saved progress, retry of failed chunks and a sequential file queue; explicit missing-audio ranges |
| 3 | Project-aware vocabulary | Provider-specific recognition hints with explicit limits | Project vocabulary sets, priority terms, participant names and a user-triggered Remember this correction action |
| 4 | Search by meaning | SQLite full-text search | Optional local semantic search alongside exact search, with results that jump to relevant recording timestamps |
| 5 | More useful speaker labels | Local diarization assigns speakers within a recording | Rename, merge and split controls first; optional recognition of confirmed speakers across meetings later |
| 6 | Explicit translation mode | Dictation cleanup preserves the original language and meaning | A separate shortcut for speaking Swedish and inserting English, with configurable target language and preservation of names and technical terms |

## 1. Meeting summaries and action items

This offers the biggest new workflow benefit. Keep generated summaries separate
from the original transcript and user-authored notes. Link generated points to
supporting transcript rows so users can inspect the source and play the audio.

OpenWhispr's recent changes provide useful implementation lessons:

- Summary requests need different timeouts from short dictation cleanup.
- Empty model responses must not become saved summaries.
- Action items should reflect actual commitments rather than invented tasks.

Reuse KnegisWhisper's reasoning routes, but give summarization its own request
policy and output handling.

Sources:

- [OpenWhispr changelog](https://github.com/OpenWhispr/openwhispr/blob/main/CHANGELOG.md)
- [KnegisWhisper cleanup implementation](../crates/reasoning-core/src/chat.rs)

## 2. Reliable long recordings and batch imports

Prioritize import reliability before expanding the model catalogue. OpenWhispr's
file-transcription interface represents failed chunks and distinguishes successful
transcription from failed speaker labeling. A useful transcript should survive an
optional processing failure.

KnegisWhisper's reviewed pipeline submits the converted recording as one
transcription input. Add backend-specific chunk limits, timestamp offsets, careful
handling of words at chunk boundaries, saved progress and retry of failed chunks.
A sequential queue would then let users import multiple files without running
several expensive inference jobs simultaneously.

The current interruption-recovery implementation is a foundation; it is not
equivalent to resuming transcription from a completed chunk. Upstream's cloud
chunk support does not establish a fix for KnegisWhisper's Parakeet crash. Validate
long recordings against the actual local runtime before claiming that resolved.

Sources:

- [OpenWhispr file transcription](https://github.com/OpenWhispr/openwhispr/blob/main/src/services/fileTranscription.ts)
- [KnegisWhisper import pipeline](../apps/desktop/src-tauri/src/media.rs)
- [KnegisWhisper import recovery](../apps/desktop/src-tauri/src/media_job.rs)
- [Parakeet benchmark findings](PARAKEET_BENCHMARK.md)

## 3. Project-aware vocabulary

The reviewed hint builder sorts terms alphabetically before applying budgets.
Local Whisper has a 200-byte application prompt budget, so alphabetically early
words can displace more relevant project names.

Prioritize explicitly selected project terms and meeting participants before
general vocabulary. Preserve the existing visibility into included and omitted
terms and provider-specific support.

OpenWhispr distinguishes manual and learned dictionary entries. KnegisWhisper
could retain similar provenance for corrections users explicitly save through a
Remember this correction action. This proposal does not require continuous
monitoring of text in other applications.

Sources:

- [KnegisWhisper hint selection](../crates/transcription-core/src/vocabulary.rs)
- [OpenWhispr dictionary types](https://github.com/OpenWhispr/openwhispr/blob/main/src/services/DictionaryService.ts)

## 4. Search by meaning

OpenWhispr advertises semantic search. Add optional local semantic search alongside
KnegisWhisper's existing full-text search. For example, a query for "the discussion
about postponing launch" could find a transcript that says "move the release to
October."

Return relevant excerpts and recording timestamps so the result leads directly
to supporting audio. Preserve exact search for names, identifiers and literal
phrases.

Sources:

- [OpenWhispr features](https://github.com/OpenWhispr/openwhispr#features)
- [KnegisWhisper note search](../crates/notes-core/src/notes.rs)

## 5. More useful speaker labels

Improve speaker editing first: rename speakers and correct incorrectly merged or
split identities within a meeting. This would make existing diarization more
useful without depending on persistent voice recognition.

OpenWhispr advertises voice recognition across meetings. Consider optional
recognition of user-confirmed speakers later, after evaluating accuracy on real
Swedish/English meetings. Within-recording diarization and persistent speaker
identity are distinct capabilities.

Sources:

- [OpenWhispr features](https://github.com/OpenWhispr/openwhispr#features)
- [KnegisWhisper diarization](../crates/meeting-core/src/diarization.rs)

## 6. Explicit translation mode

OpenWhispr advertises a dedicated translation shortcut. Add an explicit mode such
as "speak Swedish, insert English," with a configurable target language and
preservation of names and technical terms.

Keep ordinary dictation's existing language-preservation behavior. Translation
should be a deliberate action rather than an implicit side effect of cleanup.

Sources:

- [OpenWhispr features](https://github.com/OpenWhispr/openwhispr#features)
- [KnegisWhisper cleanup prompt](../crates/reasoning-core/src/chat.rs)

## Additional opportunity: duplicate speech in speakerphone recordings

OpenWhispr has transcript deduplication conditioned on microphone-bleed evidence.
This is worth investigating if recording meetings through speakers is important.

KnegisWhisper's documentation explicitly records acoustic echo cancellation (AEC)
as declined for now. Treat this as a product decision to revisit, not unfinished
implementation already approved for delivery. Transcript deduplication and audio
echo cancellation are different techniques and need separate evaluation.

Sources:

- [OpenWhispr deduplication implementation](https://github.com/OpenWhispr/openwhispr/blob/main/src/helpers/diarization.js)
- [KnegisWhisper AEC decision](COMPLETION_PLAN.md#k--aec-helper)

## Suggested implementation order

1. Harden long imports to address a known reliability need.
2. Add meeting summaries for a substantial new everyday workflow benefit.
3. Improve vocabulary selection for project names and meeting participants.
4. Reassess semantic search, speaker editing and translation against actual usage.

The feature priorities above describe product value; this implementation order
puts the known import reliability problem first.
