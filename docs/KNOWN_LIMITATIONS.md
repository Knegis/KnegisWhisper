# Known limitations

KnegisWhisper is in early development. Read the notes attached to the exact release
you install; source builds can contain changes that are not released yet.

## Windows installer

The installer targets Windows 11 x64. It is unsigned and may display an
unknown-publisher or Windows reputation warning. Verify the published SHA-256
checksum before running it. The current installer requires administrator rights.
Transcription binaries and models are downloaded separately through the app.
v0.1.1 was published before its installation and upgrade checks on a clean machine
were run, so those checks are still to come.

There is no automatic update installation. Update checking is not securely
configured while the embedded updater public key is empty. Download a later
version from the repository's Releases page.

The published v0.1.1 uninstaller's **Delete app data** checkbox misses the main
notes/recordings folder and saved credentials. Source builds now implement separate
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
- Google Calendar and Outlook offer app-owned one-click Connect or an Advanced custom client
  and bounded snapshot sync. Owner registration and real-account validation are pending; no
  build has carried the values. Meetings Upcoming/event picker and recording links are built
  and fixture-tested, still unverified against real accounts.
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
