# Known limitations

KnegisWhisper is in early development. Read the notes attached to the exact release
you install; source builds can contain changes that are not released yet.

## Windows installer

The installer targets Windows 11 x64. It is unsigned and may display an
unknown-publisher or Windows reputation warning. Verify the published SHA-256
checksum before running it. The current installer requires administrator rights.
Transcription binaries and models are downloaded separately through the app.
v0.1.1 and v0.2.0 were both published before their installation and upgrade checks on a
clean machine were run, so those checks are still to come.

There is no automatic update installation and no updater signing. v0.2.0 and later
run an automatic check once a day and link to a newer release page. The switch is
"Check for updates automatically" on Settings > Diagnostics, on by default.
The check downloads and installs nothing. It needs a public repository, which this
is (checked 16 November 2026); no installed v0.2.0 has run a check against it yet.
v0.1.1 still reports "Update checking is not securely configured". Those users
must download v0.2.0 by hand.

The published v0.1.1 uninstaller's **Delete app data** checkbox misses the main
notes/recordings folder and saved credentials. v0.2.0 implements separate
personal and shared cleanup options, but their disposable-machine acceptance
matrix is pending ([KNEG-545](https://linear.app/axelalbertkarlsson/issue/KNEG-545)). Read
[what is retained and how to remove it](GETTING_STARTED.md#uninstall-on-windows)
before uninstalling. Published-installer verification remains pending under KNEG-534;
fixture tests and a new installer build do not establish that result.

## Audio and models

- Long Parakeet imports have crashed the native runtime. Use Whisper for long
  recordings; see [the measured findings](PARAKEET_BENCHMARK.md).
- Replay seeks use available segment timings. Recordings without those timings
  can have only a whole-recording playback cue.
- A meeting's note is saved once its whole recording is transcribed. If the application ends
  before that, there is no note: the recording and typed notes stay in the `meetings` folder,
  the window says so on its next launch, and Meetings offers **Recover**. Speaker labels are
  added after the note is saved, so a failure there costs only the labels. Neither has been
  tried by ending the real application mid-meeting; see
  [interrupted meeting recovery](KNEG_316_MEETING_RECOVERY.md).
- Google Calendar and Outlook offer app-owned one-click Connect or an Advanced custom client
  and bounded snapshot sync. On 2026-11-13 a development build carrying the app-owned values
  connected a personal Google account and a personal Outlook.com account and synced them; no
  release build has been tried. While the Google sign-in is in testing, only accounts added as
  test users can connect Google, and Google ends their sign-in after 7 days. A Microsoft 365
  work account may need its administrator's approval: the owner's work tenant required it.
  Meetings Upcoming/event picker and recording links are built and fixture-tested, still
  unverified against real accounts.
  See [the owner runbook](CALENDAR_PROVIDER_SETUP.md).
- Cloud providers, speaker identification and meeting
  detection have different validation coverage. See [platform support](PLATFORM_SUPPORT.md)
  and [the engineering status reference](ENGINEERING_STATUS.md).
- Local audio/video import is supported. URL import is disabled.

## Platforms and privacy

Windows is the primary hands-on validation platform. macOS and Linux source builds
are available for development, with capability-specific limits; they are not part
of the first downloadable release.

Local transcription does not imply local cleanup or summaries: selecting a cloud
provider for either sends the relevant text to that provider. Model downloads and
opt-in integrations also use the network. See [network purposes](NETWORK_BOUNDARY.md).

Semantic search, translation, standalone chat/voice agents and Apple Calendar are
not available. Do not infer support from another project's feature list.

Release readiness is tracked in [KNEG-43](https://linear.app/axelalbertkarlsson/issue/KNEG-43).
