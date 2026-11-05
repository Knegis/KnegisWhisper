# Release process

The first channel is an unsigned Windows 11 x64 preview. Releases are built from an
existing version tag contained in `main` and always prepared as draft prereleases.
Optional macOS/Linux bundle builds upload experimental workflow artifacts only;
they are not attached to the Windows preview. Repository visibility and access
remain unchanged. Release readiness is tracked in [KNEG-43](https://linear.app/axelalbertkarlsson/issue/KNEG-43).

## Prepare a version

1. Merge reviewed work from `dev` into `main`. Review the exact release commit.
2. Align `workspace.package.version` in Cargo, the desktop package and npm lockfile,
   and the Tauri version. Refresh Cargo.lock when package versions change.
3. Move the user-facing Unreleased changelog into a dated version entry and update
   the release-note template with the actual changes and current limitations.
4. Run the relevant checks in [Development](DEVELOPMENT.md). Commit the release
   preparation, tag that commit as `v0.1.0` for the first preview, and push that tag.
   Do not move a tag after an installer is published.
5. Open **Actions > Release > Run workflow** from the reviewed `main` branch and
   supply the existing tag. The workflow verifies version agreement, the exact
   checkout, main ancestry and absence of a published release for that tag.

## Build and inspect the draft

The workflow runs contracts, frontend and Windows Rust checks, then builds an
unsigned NSIS installer. It creates a draft prerelease containing:

- `KnegisWhisper_<version>_windows_x64_setup.exe`
- `SHA256SUMS.txt`

The release body includes the tagged source SHA, installation instructions and
known limitations. Updater artifacts are explicitly disabled. No signing key is
needed. Existing published releases cannot be replaced by a rerun.

Download the draft installer and compare its SHA-256 hash with the attached file.
Attach the reviewed intro video as `knegiswhisper-intro.mp4`; the media production
instructions describe the source footage and export settings.

## Installed-app acceptance

Use a clean Windows 11 x64 VM or separate test machine with no Rust/Node tools.
Record the machine, commit, installer hash, date and result for each check:

- Install through NSIS with administrator rights; check the missing-WebView2 path.
- Launch from the installed shortcut and confirm the displayed app version.
- Download a local runtime/model through Settings and complete a dictation.
- Record a short microphone/system-audio meeting, stop it and inspect the note.
- Import an audio/video file, edit a note, replay its audio and export the result.
- Restart and verify settings, notes and recordings persist.
- Install a subsequent test version over the preview and verify data preservation.
- Uninstall and check the documented treatment of retained user data.
- Confirm update checking reports that it is not securely configured.

A debug app, browser fixtures or a successful packaging job does not substitute
for these installed-app checks. Keep all test data fictional.

## Publish

Only after the draft assets and installed-app evidence have been reviewed:

1. Present the exact draft release, checksums, video and results for approval.
2. Publish it as a **prerelease**, without marking it as the latest stable release.
3. Download the public installer and verify its checksum again.
4. Replace the README's Get started action with a direct link to that preview's
   release page. Link the video preview to its public MP4 asset.
5. Update the getting-started installation status and verify every public link.

Do not use `/releases/latest` to locate the preview. Future stable promotion,
Windows code signing, updater signing and macOS/Linux installers are separate
release decisions. The current app does not install updates automatically.

## Repeat or withdraw

A failed build can be rerun while the release remains a draft. Fix a defective
published installer through a new version and release; do not silently replace its
bytes or move its tag. If a preview must be withdrawn, mark its release clearly
and point users to the replacement. Preserve its source and validation evidence.

## Repository presentation

The product description and topics can be maintained in GitHub's About panel.
The social-preview image is `docs/assets/social-preview.png`; upload it in
**Settings > General > Social preview** when preparing the public-facing repository.
Keep repository visibility unchanged unless the owner explicitly requests a public launch.
