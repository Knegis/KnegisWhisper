# Engineering status and validation reference

## Engineering status snapshot

This engineering snapshot was moved from the previous README. Its September 8 status date predates subsequent UI work; use the current release notes and platform guide for release-specific support.
Local tests and Windows observations are separate from hosted CI and release validation.
The v0.2.0 update check needs no key. It runs once a day automatically by default or through
"Check for updates", and the notice links to a newer release without downloading or installing.
The check needs a public repository; while private, it reports "No public release could be found".
v0.1.1 users must download v0.2.0 by hand. Release selection, 404 mapping, the once-a-day rule
and URL building are unit-tested with invented data. The app's list request and the notice for
a real release have never run. Updater signing remains future work; see `PLATFORM_SUPPORT.md`.

| Area | Implemented and remaining limits |
| --- | --- |
| Dictation | Global shortcuts, toggle and hold-to-talk, local Whisper/Parakeet and configured cloud routes. On Windows, modifier-only toggles such as `Control+Super` activate after all chord keys are released; an additional key cancels the gesture. |
| Meetings | Microphone and system-audio recording, live transcription, detection/recording offers and a notification panel. Windows WASAPI capture and local/OpenAI Realtime transcription have been exercised. Other cloud meeting providers and some detection/notification scenarios still need live validation. |
| Notes | Independent title and Notes / Transcript / Replay tabs; in windows 1280px or wider the transcript is a column beside the note instead of a tab. Personal Markdown notes precede AI summaries; empty notes can start from the transcript. Playback continues across tabs. Folders, transcript-aware full-text search and txt/md/json/srt/vtt export remain available. See [Notes workspace](NOTES_WORKSPACE.md). |
| Meeting summaries | Optional automatic/manual summaries with an independent model, editable prose, transcript references, chunked processing and retry/cancel. Configure Settings > Cleanup & summaries. See [implementation and validation](MEETING_SUMMARIES.md) for model-quality limits. |
| Recording ownership | Older notes can attach an explicitly selected PCM16 mono/stereo WAV through a native dialog. The source is copied and preserved. Deleting a note, in the app or through the API, moves it to Trash with its recording; the copy is removed only by Delete forever or Empty trash, never on its own. Meeting attachment failure keeps the note and WAV and displays recovery guidance. |
| Import | Local audio/video is converted with ffmpeg, transcribed and saved with retained audio for playback. UI and API imports share one active slot, with no queue. Progress survives navigation; job metadata survives restart, while unfinished jobs become failed rather than resuming. URL import remains disabled. OpenWhispr database import is a separate flow. |
| Local inference | Whisper and Parakeet for dictation/import, local meeting routes including streaming models, and llama.cpp for dictation cleanup. Long Parakeet inputs can crash the current offline runtime; see the findings below. Whisper uses serialized on-demand recovery, without automatically replaying a submitted transcription. |
| API, CLI and MCP | Opt-in loopback API, CLI and a stdio MCP server with 23 tools: notes, dictionary, history search, file transcription jobs, meeting recording, status and model discovery. Discovery permissions are verified before serving; detected permission widening revokes the token and stops service. Repair is attempted once, followed by explicit retry. |
| Calendars and diarization | Google Calendar and Outlook (personal and Microsoft 365) have app-owned-or-custom one-click PKCE clients, bounded snapshot sync, local readers and Settings > Calendar built with unit tests and dev-fixture screenshots. No client is registered, no build has carried the values and no real account has connected; see [the owner runbook](CALENDAR_PROVIDER_SETUP.md). Meetings Upcoming/event picker and recording links are built and fixture-tested, still unverified against real accounts. Speaker-diarization controls are built; real-meeting accuracy needs validation. Apple Calendar is not implemented. |

The owner has confirmed Windows dictation/hotkey behavior, API start/stop and permission
readback, note autosave (including immediate close), meeting-row/selection playback and
scrubbing, and a 45-minute Whisper import with playback. Short Parakeet imports work.
The real import picker also permits retry after a conversion failure without reselecting the file.

Remaining limits worth knowing:

- **Long Parakeet audio:** isolated 15- and 45-minute inputs exited with `0xC0000409`.
  Three sequential five-minute pieces completed in a follow-up experiment, but production
  chunking is not implemented and the native crash cause is unresolved. See
  [Parakeet benchmark findings](PARAKEET_BENCHMARK.md).
- **Playback precision:** rows use available segment timings, not arbitrary selected words.
  Imports without provider timings use a labelled whole-recording cue.
- **Platform privacy:** capture-exclusion readback confirms an OS flag, not that every screen
  recorder honors it. Some native window, capture and shortcut paths remain unsupported or
  unverified outside Windows.
- **Deferred or declined:** semantic search, chat/voice agents, translation, Apple Calendar,
  the AEC helper and attested Tinfoil integration are not available. Code blocks remain plain;
  CodeMirror highlighting is excluded under the strict CSP. There is no standalone chat screen.
- **External verification:** the long OpenAI Realtime reconnect run and release/signing
  validation remain separate checks. Consult the actual Actions run for hosted CI status;
  the checked-in workflows and local test reports do not establish that status.

[BENCH_CHECK.md](BENCH_CHECK.md) and [ENGINEERING_LOG.md](ENGINEERING_LOG.md)
record dated observations, fixes and limitations. Older entries describe the repository at
that time; they are not a claim that previously completed work is still missing.
