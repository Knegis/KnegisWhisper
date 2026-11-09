# Get started

## Install or run from source

The first Windows preview is being prepared. Until a tested installer is published,
follow [the source-build instructions](DEVELOPMENT.md#development).

Published previews will appear on the [Releases page](https://github.com/Knegis/KnegisWhisper/releases).
Choose the Windows x64 installer and its `SHA256SUMS.txt`. In PowerShell:

```powershell
Get-FileHash .\KnegisWhisper_0.1.1_windows_x64_setup.exe -Algorithm SHA256
```

Compare the hash with the checksum file from the same release. The initial preview
is unsigned, requires administrator rights and targets Windows 11 x64. Read the
release's installation notes and [known limitations](KNOWN_LIMITATIONS.md).

## Your first local dictation

1. Open **Settings > Transcription** and select a local Whisper route.
2. Install its required runtime and model through the app. Downloads require a
   connection and disk space; the installer does not contain every model.
3. Open **Settings > Shortcuts** and set a dictation shortcut.
4. Return to **Dictation**, choose your microphone and language, and leave
   **Tidy up** off for a fully local first test.
5. Focus a text field in another app, activate your shortcut and speak a sentence.
   Stop recording with the configured shortcut and check the inserted text.

Once installed, local transcription can run offline. For long imported recordings,
start with Whisper rather than Parakeet.

## Use a cloud provider

Add your own credentials in **Settings > Providers**, then choose the provider in
**Settings > Transcription**. Requests go directly to the selected provider and
provider charges may apply. No KnegisWhisper account is required.

Optional cleanup and summaries have independent model choices in
**Settings > Cleanup & summaries**. A local transcription route combined with cloud
cleanup still sends text to that cloud provider.

## Record a meeting

Choose the meeting provider and language in **Settings > Meetings**. Open
**Meetings**, confirm the microphone/system-audio choices and start recording.
Stop the recording, then open its note to review the transcript and replay audio.
Use a short recording of your own voice for the first check.

## Import and keep notes

Open **Notes**, reveal the list panel if needed, and choose **Import audio or video**.
Select a local recording. Converted audio is retained with its note for replay;
your source file stays intact. You can edit notes, search transcripts and export
text, Markdown, JSON or subtitle formats. See [Notes workspace](NOTES_WORKSPACE.md).

## If something goes wrong

- **No audio:** check the selected input, Windows microphone permissions and whether
  another recording is active.
- **Model unavailable:** finish its runtime/model download before starting.
- **Cloud request fails:** check the provider credentials, route and account quota.
- **Long import fails:** try Whisper and consult the Parakeet limitation above.
- **Need help:** search [existing issues](https://github.com/Knegis/KnegisWhisper/issues),
  then submit a bug report with your version, route and shortest reproduction.
  Redact credentials and private transcript content from diagnostics.
