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

The repository description and topics were updated. Its visibility remains private.
There were no GitHub releases when inspected. GitHub private vulnerability reporting
returned 404 for this private repository; the security policy uses a private
maintainer channel instead of advertising an unavailable reporting form.

No public release or version tag is created by this documentation work. Review and
merge the changes before tagging `main`, then run the release workflow against that
exact tag. A hosted release build and clean Windows installation/upgrade/uninstall
checks remain required. This environment has no Windows Sandbox; the local build
is not a substitute for those checks. Release readiness remains tracked in
[KNEG-43](https://linear.app/axelalbertkarlsson/issue/KNEG-43).

The social-preview artwork is ready for upload in GitHub repository settings.
Repository visibility, certificate acquisition and automatic updates are unchanged.
