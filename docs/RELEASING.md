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
- Run the [Windows uninstall acceptance matrix](#windows-uninstall-acceptance)
  against the [documented retention behaviour](GETTING_STARTED.md#uninstall-on-windows).
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

## Windows uninstall acceptance

KNEG-535 records the pre-helper behaviour. KNEG-534 owns the published v0.1.1
installed-app run. The new helper and its release acceptance are tracked in
[KNEG-545](https://linear.app/axelalbertkarlsson/issue/KNEG-545). Do not close that
issue or publish the new deletion behaviour until the matrix below passes.

Use a disposable Windows 11 VM or separate test machine, with a snapshot/reset
between cases. Do not uninstall the owner's working app to obtain evidence.
Record the installer filename, SHA-256, source commit/tag, Windows version,
installation directory, invoking/elevated account and exit code. Record whether
results are observed, source-derived or not run; capture the actual checkbox.

Before each case, create fictional notes, settings, a retained recording and a
recovery file. Seed small identifiable files in the per-user downloads/audio
folders, both bundle-ID folders, the shared fallback cache and an external export
folder. Include sentinels in another test user's app folders and an unrelated
sibling directory. Record filenames/hashes without transcript contents. Use only
fake, uniquely named credentials; record target names and presence, never values.
Include a non-ASCII Windows test account to exercise the real fallback paths.

| Case | Required result for the new helper |
| --- | --- |
| Windows Installed apps, both options unchecked | Choices appear before UAC. App/resources and ordinary shortcuts removed; all personal/shared fixtures and credentials retained. |
| Personal checked, shared unchecked; split-token administrator | Original account's app root, both bundle-ID folders, discovery file, exact app credentials and startup registry values removed. Shared cache and all external/other-user sentinels retained. |
| Personal checked; standard user supplies a different administrator at UAC | Same original-user cleanup. Administrator's personal files, credentials and startup values must remain unchanged. |
| Personal unchecked, shared checked | Only shared cache and application removed. Personal data/credentials of every account retained. |
| Both checked | Both selected scopes removed; unrelated siblings, exports and other users' personal data untouched. |
| Cancel each choice/confirmation and deny UAC | No data, credentials or application removed. |
| Launch interactive helper already elevated | Refuses to select a personal profile; no deletion. Normal unelevated launch works. |
| Reinstall after checked and unchecked cases | Kept data reopens; removed data does not reappear. Also reinstall before reboot and verify the NEW helper survives that reboot. |
| Upgrade from v0.1.1 and helper-based build, including `/UPDATE` | No helper choices or personal/shared deletion. Notes/settings/recordings/credentials survive. Check reinstall UI and updater-style arguments separately. |
| Helper `--quiet`, direct NSIS `/S`, `/P`, ordinary direct NSIS | All personal/shared data retained. Direct UI explains retention and has no delete-data checkbox. |
| Active app/recording, model server, locked file, read-only file, denied ACL | Stop/retry or explicit failure; no false success or forced termination. Unlocking then retrying completes cleanup. Cancel may leave partial deletion and says so. |
| Junction at app root, child or ancestor; redirected folder and non-ASCII account | Reparse points rejected without touching target sentinel. Ordinary non-ASCII paths work. Shared fallback follows only the separate choice. |
| Broker exits/crashes, pipe spoof, connection timeout, app relaunch during uninstall | Explicit failure, never a success message for unfinished cleanup. Authenticate exact peer process and retain unrelated sentinels. |
| Installer/helper file removal requires reboot | Explicit restart-required result (3010); no automatic restart. Record leftovers before/after reboot and uninstall registration/shortcuts. |

Compare filesystem hashes and credential presence before/after each case, then
open the reinstalled app for the reinstall cases. Verify external exports and
other users' sentinels are unchanged. Any divergence from the expected table is
a finding with exact version/account context, not permission to adjust expected
results silently. Clean up only the test fixtures or reset the VM afterward.

For the published v0.1.1 asset, record its own hash and results. The
[source audit](UNINSTALL_AUDIT.md) compares its tag with current code, but neither
a newer debug installer nor the credential naming probe is a substitute for
running that published asset. Use the older retention table in GETTING_STARTED.md
for v0.1.1, not the new helper matrix. Keep its unrun cases pending in KNEG-534;
keep the new helper cases pending in KNEG-545.
