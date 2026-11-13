# {{TAG}} — Windows preview

Local-first dictation, meeting transcription and searchable notes.

## Included

- Dictation with configurable shortcuts and local or bring-your-own-key routes.
- Meeting recording, transcripts, editable notes and recording playback.
- Local audio/video import and transcript/notes export.

{{CALENDAR_CLIENTS}}

## In this version

- Google Calendar and Outlook in Settings > Calendar. The lines above say whether
  this build connects in one click; any build accepts a custom client under Advanced.
- Meetings lists upcoming events. A recording can be linked to the calendar event
  it was made in, chosen at Start and changeable while recording.
- The app checks GitHub once a day for a newer release and offers to open its page.
  Turn it off with Check for updates automatically in Settings > Diagnostics.
- A new uninstall helper offers to delete your own data, and separately shared
  downloads. Both choices start unchecked, and upgrades keep everything.
- Custom server keys are now bound to their server address. Re-enter existing
  custom keys in Settings > Providers after upgrading.
- Security fixes for scratch audio, CLI output, imports and Corti settings, plus
  interface fixes across Settings and the loading screens. See the
  [changelog](https://github.com/Knegis/KnegisWhisper/blob/{{TAG}}/CHANGELOG.md).

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
- v0.2.0 and later check GitHub once a day and link to a newer release, with a switch
  in Settings > Diagnostics, but never download or install anything. v0.1.1 cannot,
  so install this version by hand.
- No real calendar account has connected to a build yet, so treat calendars as
  unverified.
- The new uninstall helper hasn't been through its acceptance run on a disposable
  machine. Read what is kept before uninstalling.
- Provider, calendar and speaker-identification validation varies by capability.

See [Getting started](https://github.com/Knegis/KnegisWhisper/blob/{{TAG}}/docs/GETTING_STARTED.md)
and [Known limitations](https://github.com/Knegis/KnegisWhisper/blob/{{TAG}}/docs/KNOWN_LIMITATIONS.md).

Built from commit `{{SHA}}`. SHA-256 checksums identify the attached installer;
they are not a code signature.
