# {{TAG}} — Windows preview

Local-first dictation, meeting transcription and searchable notes.

## Included

- Dictation with configurable shortcuts and local or bring-your-own-key routes.
- Meeting recording, transcripts, editable notes and recording playback.
- Local audio/video import and transcript/notes export.

## In this version

- Parakeet TDT 0.6B installs from its current upstream download, which was rebuilt
  on 2026-11-07. The unpublished `v0.1.0` build expected the old file and refused it.
- Scrollbars are a slim rounded handle with no arrows, and stay inside the rounded
  note and Settings sheets.
- The note editor's add and drag icons are centred in their highlight and stay
  visible when pointed at.

## Install

Windows 11 x64. Download the `.exe` and `SHA256SUMS.txt`, compare the installer
hash with `Get-FileHash <installer.exe> -Algorithm SHA256`, then run the installer.
The installer requires administrator rights and may download WebView2 if missing.

**Unsigned preview:** no Windows code-signing certificate is applied. Windows may
display publisher/reputation warnings. This is an early preview, not a stable release.

Transcription runtimes and models are downloaded separately inside Settings.
Local transcription can then work offline. Cloud transcription, cleanup and summaries
use your chosen provider directly; provider charges may apply.

## Known limitations

- Use Whisper for long recordings; long Parakeet imports have crashed the runtime.
- macOS/Linux installers are not included in this preview.
- Automatic updates are unavailable. Download later previews from Releases.
- Provider, calendar and speaker-identification validation varies by capability.

See [Getting started](https://github.com/Knegis/KnegisWhisper/blob/{{TAG}}/docs/GETTING_STARTED.md)
and [Known limitations](https://github.com/Knegis/KnegisWhisper/blob/{{TAG}}/docs/KNOWN_LIMITATIONS.md).

Built from commit `{{SHA}}`. SHA-256 checksums identify the attached installer;
they are not a code signature.
