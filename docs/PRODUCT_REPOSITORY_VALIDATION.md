# Product repository and preview preparation

## Verified locally on Windows, 5 November 2026

- Frontend production build succeeded; 568 frontend tests passed.
- Repository contracts and their 176 self-tests passed, including baseline provenance.
- Rust workspace tests: 2,217 passed, 24 ignored; ignored checks are not passes.
- Feature-gated Tauri boundary suite: 557 passed, 16 ignored.
- Workspace formatting and Clippy passed with warnings denied.
- Release-tool tests: 7 passed, covering exact tags, main ancestry, version drift,
  missing/ambiguous/empty installers, stale outputs, checksums and source provenance.
- Release and CI workflows passed official actionlint v1.7.12 with shellcheck and
  pyflakes disabled. Download checksum was verified before running actionlint.
- The actual NSIS installer built successfully with updater artifacts disabled.
  Authenticode reports `NotSigned`, as intended for the preview.
- GitHub's Markdown API rendered the README. Isolated Edge checks found no broken
  local images in light/dark views; desktop and narrow layouts were inspected.
- Local targets in the new and reorganized documentation were checked.
- Native footage capture verified editing and advancing replay of generated sample
  speech. The export is 40 seconds, 1920x1080, 30 fps, silent. Its authored sample
  transcript is not model-quality evidence.

## Review artifacts

The installer and video are ignored build artifacts, not files committed to Git.

| Artifact | Size | SHA-256 |
| --- | --- | --- |
| Windows NSIS installer | 5550291 bytes | `8f12ba67335c04210267fcf46a4b720b4d3b1b157d9cec960d993bb790c1d850` |
| Intro MP4 | 831412 bytes | `4f6d099406f444de8fff62472ab2b7ca2bdf436f0f2ec4b4e0c8f1e2a5e040e3` |

Installer: `target/release/bundle/nsis/KnegisWhisper_0.1.0_x64-setup.exe`.
Video: `target/product-media/knegiswhisper-intro.mp4`.
The README GIF, screenshots, poster and social-preview artwork are versioned.
Capture sources are the native development app and its active development frontend,
not an installed or tagged release candidate. See [video notes](INTRO_VIDEO.md).

## GitHub state and remaining release gates

The repository description and topics were updated. The repository was private when
v0.2.0 was published and is public now (checked 16 November 2026), with the rulesets in
[repository rules](RELEASING.md#repository-rules). GitHub private vulnerability reporting
returned 404 while the repository was private. It was enabled on 16 November 2026: the API
reports it on, and the signed-out Security tab shows **Report a vulnerability**, whose form
asks the visitor to sign in. No test report has been filed. `SECURITY.md` and the issue
chooser's security link point to that form.

## v0.2.0, published 13 November 2026

- Tag `v0.2.0` is `ac6212b`, the merge of PR #69 into `main`.
- The first Release run, 43432303057, failed at "Verify bundled notices": the notice
  generator works offline, and the release workflow had never fetched `aes`, a crate only
  the Linux credential store uses (KNEG-533). `0f04001` added the `cargo fetch --locked`
  step CI already had, and reached `main` through PR #70, merged without waiting for CI
  because it changes only the workflow file. The tag did not move: the workflow comes from
  `main` and builds the tag.
- Run [43437664044](https://github.com/Knegis/KnegisWhisper/actions/runs/43437664044) passed
  every step and created the draft. It reported `calendar: google=included
  microsoft=included`, and the notes say both one-click calendars are included.
- Published as a prerelease, not marked latest, at 15:56 UTC. Later the same day, at the
  owner's request, it became a full release marked Latest. Its title still says "Windows
  preview", and its notes still call it an early preview rather than a stable release.
  No intro video is attached, because the only one is v0.1.1's, with its outdated closing
  caption (KNEG-537).
- The repository was still private at publication.

| Asset | Size | SHA-256 |
| --- | --- | --- |
| `KnegisWhisper_0.2.0_windows_x64_setup.exe` | 5772277 bytes | `754ebd284d7b6f098a9ccb63b029fd07d22acc16374dc0b396865b6c4d17ca42` |

The installer was downloaded from the draft and again after publication, and both copies
matched `SHA256SUMS.txt`. Its version resource reads 0.2.0, and it is not signed.
`SHA256SUMS.txt` now has LF line endings (KNEG-536), and `sha256sum -c` passes in Git Bash.
No installed-app acceptance has been run on this installer.

## v0.1.1, published 9 November 2026

- Tag `v0.1.1` is `c87b31c`, the merge of PR #66 into `main`.
- The Release workflow built it in run
  [42690352308](https://github.com/Knegis/KnegisWhisper/actions/runs/42690352308), after
  passing the repository contracts, frontend tests and Windows Rust checks. An earlier
  run, 42665895730, built the installer, but its draft job was never started during
  GitHub's Actions outage that evening, and GitHub refuses to retry that run.
- Published as a prerelease, not marked latest, at 21:43 UTC. Shortly afterwards,
  at the owner's request, it became a full release marked latest. Its title lost
  "Windows preview", and its notes lost the line calling it an early preview rather
  than a stable release. The unsigned warning and known limitations were kept.
- The v0.1.0 draft was deleted without being published; the `v0.1.0` tag remains.

| Asset | Size | SHA-256 |
| --- | --- | --- |
| `KnegisWhisper_0.1.1_windows_x64_setup.exe` | 5509696 bytes | `8096a57567eec76e29fcf9f4ae706c3d2e72412fb2f6fc6a6f0337fb877baad6` |
| `knegiswhisper-intro.mp4` | 831412 bytes | `4f6d099406f444de8fff62472ab2b7ca2bdf436f0f2ec4b4e0c8f1e2a5e040e3` |

The video is the one reviewed on 5 November, listed above. Its closing caption still
reads "Windows preview in preparation".

The installer was downloaded again after publication and matched `SHA256SUMS.txt`.
That file has Windows line endings, so `sha256sum -c` on Linux, macOS or Git Bash
reports the installer as unreadable. The `Get-FileHash` comparison in
[Getting started](GETTING_STARTED.md) is not affected.

**The installed-app acceptance in [Releasing](RELEASING.md#installed-app-acceptance)
has not been run.** No clean Windows 11 machine was available, and the owner chose to
publish before it; it will be run later on a separate computer. Until then, nothing
in this record shows that the published installer installs, launches or keeps data.
Two of its checks cannot pass yet as written. The upgrade check needs a later
installer. The uninstall check has nothing to compare against, because no document
says which user data an uninstall keeps. Release readiness remains tracked in
[KNEG-43](https://linear.app/axelalbertkarlsson/issue/KNEG-43).

The social-preview artwork is ready for upload in GitHub repository settings.
Repository visibility, certificate acquisition and automatic updates are unchanged.
