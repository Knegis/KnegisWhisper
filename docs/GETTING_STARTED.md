# Get started

## Install or run from source

The newest Windows release, [v0.2.0](https://github.com/Knegis/KnegisWhisper/releases/tag/v0.2.0), is an unsigned preview,
and the release GitHub marks Latest. It has not yet been through the installation checks on a clean machine. To build the app yourself instead, follow
[the source-build instructions](DEVELOPMENT.md#development).

From that release page, download `KnegisWhisper_0.2.0_windows_x64_setup.exe` and its
`SHA256SUMS.txt`. In PowerShell:

```powershell
Get-FileHash .\KnegisWhisper_0.2.0_windows_x64_setup.exe -Algorithm SHA256
```

Compare the hash with the checksum file from the same release. The installer
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

Open **Notes**, reveal the list panel if needed, and choose **Import**, then **Audio or video**.
Select a local recording. Converted audio is retained with its note for replay;
your source file stays intact. You can edit notes, search transcripts and export
text, Markdown, JSON or subtitle formats. See [Notes workspace](NOTES_WORKSPACE.md).

## Calendars

Settings > Calendar offers Google Calendar and Outlook (Outlook.com, Hotmail and Microsoft 365).
Builds carrying app-owned client values support one-click Connect; Advanced accepts a custom
client, including for forks without those values. A development build carrying the app-owned
values has connected a personal Google account and a personal Outlook.com account; no release
build has yet. See [provider setup and R12](CALENDAR_PROVIDER_SETUP.md).
Sync now requests a bounded snapshot. Meetings opening requests one at most once per 5 minutes
per provider; never launch, focus or timer. Meetings Upcoming/event picker and recording links
are built and fixture-tested, still unverified against real accounts.

## Uninstall on Windows

Quit KnegisWhisper from its tray menu, then use **Windows Settings > Apps >
Installed apps > KnegisWhisper > Uninstall**. Closing the window alone can leave
it running in the tray. Uninstall requires administrator rights.

### The uninstall helper (v0.2.0 and later)

From v0.2.0 the installer registers a separate uninstall helper. It opens two choices,
both unchecked:

- **Delete my KnegisWhisper data** removes the initiating Windows account's
  notes, transcripts, summaries, history, saved voices, recordings, recovery
  files, settings, downloaded models/runtimes and temporary audio. It also
  removes that account's two app-specific WebView folders, saved provider and
  calendar credentials, local API discovery file and app startup registry values.
- **Delete shared downloads and temporary audio for all users** separately
  removes `%ProgramData%\KnegisWhisper\cache`. Other Windows accounts may use it.

Leaving both unchecked keeps these data. A final confirmation appears before
Windows asks for administrator access. Personal cleanup stays in the original
account when you supply another administrator's credentials. Start uninstall
normally; an already-elevated interactive launch refuses to guess whose data
to remove. Exports, original imports and other users' personal data stay intact.

Locked files, access failures and redirected folders stop cleanup with a
Retry/Cancel message. Cancel can leave partial deletion; it cannot restore files.
The helper does not kill running recordings or model processes. A restart may
be needed to remove running installer/helper files; it never restarts Windows.
Upgrades, the helper's `--quiet` mode and direct NSIS `/S` or `/P` uninstall
retain all personal and shared data. These are local deletions, not secure erasure
or revocation of access at a remote provider.

This flow has build and fixture-test evidence. The full disposable-machine
acceptance matrix, including alternate-administrator elevation, is still pending
under [KNEG-545](https://linear.app/axelalbertkarlsson/issue/KNEG-545).
See [the implementation record](UNINSTALL_IMPLEMENTATION.md).

### Published v0.1.1 and older uninstallers

**These uninstallers keep your notes, recordings and saved credentials,
even if you select ?Delete app data?.** That checkbox removes different folders
from the ones used for the main application data. This is tracked in
[KNEG-545](https://linear.app/axelalbertkarlsson/issue/KNEG-545).

The following is the expected behaviour from the pre-helper Windows installer script
and application source, not a completed uninstall test. The v0.1.1 source uses the
same storage paths and uninstall configuration; its published installer still
needs the [installed-app acceptance checks](RELEASING.md#windows-uninstall-acceptance).
See the [audit record](UNINSTALL_AUDIT.md) for versions and evidence.

| Data or files | Checkbox unchecked | Checkbox checked |
| --- | --- | --- |
| Installed executable, bundled resources and app shortcuts | Removed by ordinary uninstall | Removed by ordinary uninstall |
| Notes, transcripts, summaries, settings, dictation history and saved voice data in `%APPDATA%\KnegisWhisper\knegiswhisper.db` | Kept | Kept |
| Retained recordings and recovery files in `%APPDATA%\KnegisWhisper\meetings` | Kept | Kept |
| The application's log in `%APPDATA%\KnegisWhisper\logs` | Kept | Kept |
| Downloaded models, runtimes and temporary audio under `%APPDATA%\KnegisWhisper` | Kept | Kept |
| Fallback models and temporary audio under `%ProgramData%\KnegisWhisper\cache` | Kept | Kept |
| Provider keys and calendar tokens in Windows Credential Manager | Kept | Kept |
| Tauri/WebView data under `%APPDATA%\com.knegis.knegiswhisper` and `%LOCALAPPDATA%\com.knegis.knegiswhisper`, if present | Kept | Deletion attempted for the account running the elevated uninstaller |
| Exported/shared files, original imported recordings and logs saved outside those folders | Kept | Kept |
| `%USERPROFILE%\.knegiswhisper\local-api.json`, if left after an abnormal exit | Kept | Kept |

Deleting WebView data can reset interface preferences even though notes and
application settings remain. An upgrade using the installer's update mode skips
the delete-data step. Silent and passive uninstall offer no checkbox and retain
user data. Reinstalling under the same Windows account is expected to find the
retained data again. Removal can fail for locked files; a checkbox is not proof
that a folder was removed.

### Remove retained data yourself

1. Back up anything you want to keep. Export wanted notes and copy their recordings.
   For a full local data backup, quit the app and copy the entire
   `%APPDATA%\KnegisWhisper` folder. Credentials are stored separately.
2. If the app is still installed, clear saved provider keys in **Settings >
   Providers** and disconnect calendar accounts in **Settings > Calendar**.
   Quit from the tray. Stop any remaining KnegisWhisper transcription/model
   processes before deleting files; check their executable paths so you do not
   stop another application's runtime.
3. Sign into the Windows account that used the app. In File Explorer, enter
   `%APPDATA%\KnegisWhisper` and verify the full resolved path and folder name.
   Delete that specific folder only when you intend to erase its contents,
   including notes, recordings, settings, downloads and any diagnostic files.
   Check the Recycle Bin if you also want to remove the deleted copy.
4. If present, inspect and remove only the two app-specific Tauri folders listed
   above. After the app has stopped, the specific leftover
   `%USERPROFILE%\.knegiswhisper\local-api.json` can also be removed. It may contain
   a local API token; do not paste its contents into a support report.
5. Open **Control Panel > Credential Manager > Windows Credentials** and inspect
   **Generic Credentials**. This build stores entries named
   `<slot>.com.knegis.knegiswhisper`, with the username equal to `<slot>`; for
   example, `openai.com.knegis.knegiswhisper`. Remove only entries matching that
   exact app suffix and username convention. Do not clear all saved credentials.
   Local deletion does not revoke keys or account access at the remote provider.
6. Treat `%ProgramData%\KnegisWhisper\cache` separately. It can hold shared models
   and temporary audio when a Windows path contains non-ASCII characters. Review
   it with the PC's owner and other app users before removing identified files;
   do not delete the shared cache just because one account uninstalled the app.

If elevation asks for another administrator's credentials, that administrator's
AppData folders may be targeted by the checkbox instead of yours. Check cleanup
under each intended account; do not assume a per-machine uninstall erased every
user's data. Uninstall does not remove copies you exported or sent elsewhere.
These steps remove local copies; they are not a secure-erasure guarantee.

## If something goes wrong

- **No audio:** check the selected input, Windows microphone permissions and whether
  another recording is active.
- **Model unavailable:** finish its runtime/model download before starting.
- **Cloud request fails:** check the provider credentials, route and account quota.
- **Long import fails:** try Whisper and consult the Parakeet limitation above.
- **Need help:** search [existing issues](https://github.com/Knegis/KnegisWhisper/issues),
  then submit a bug report with your version, route and shortest reproduction.
  Redact credentials and private transcript content from diagnostics.
