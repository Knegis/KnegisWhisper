# KNEG-536 / 515 / 533 / 532 validation

Validated 2026-11-10 in an isolated worktree from dev at f4d5bde.
No merge, push, publication, or changes to the user's installed profile.

Implementation commits: ec420d6 (checksums), 24eec8a (layout),
2102334 (notices), 83e7252 (model downloads).

## Changes

- KNEG-536: release checksums use explicit UTF-8/LF; the unit test checks raw bytes.
  The Linux draft-release job verifies the downloaded Windows artifact with sha256sum.
- KNEG-515: the collapsed-library layout reserves space in the inline transcript header.
  Pointer activation, Enter/Space, focus transfer and non-overlap are checked at 720x520.
- KNEG-533: deterministic target notices include the desktop Rust dependency graph
  (build tooling included), 110 bundled npm packages, three font licenses and Silero VAD.
  Original package notices supplement detected license texts. Missing original notices
  use version-specific, hash-checked upstream copies at packaged VCS revisions.
  The NSIS installer bundles NOTICE and THIRD_PARTY_NOTICES.txt; CI compares regeneration.
- KNEG-532: 17 required files across four speech models and pyannote segmentation now
  use full publisher Hugging Face commits. All required model bytes were compared with
  their existing digest-verified GitHub archive. Names, installed paths and weights stay
  compatible. New downloads are bounded, staged and verified before publication, with
  OS locking and preservation of an existing complete installation.

The English CAM++ embedding download is still on its existing verified GitHub URL.
No byte-equivalent publisher mirror was established; no model was substituted.
This is the remaining KNEG-532 item.

## Automated checks

- Release Python tests: 7 passed. Actual Git Bash GNU sha256sum accepted the generated
  checksum and rejected a deliberately modified disposable installer fixture.
- License Python tests: 4 passed; frontend notice plugin tests: 2 passed.
  Fresh cargo-about 0.8.4 generation matched the checked-in notice bytes.
- Frontend: 568 tests, typecheck and production build passed.
- Headless Edge: 7 speaker-form/layout scenarios passed. Both themes at 720x520 have
  separate toggle/label hit areas, no horizontal overflow, pointer activation, and
  keyboard focus preserved. Screenshots inspected. This is fixture/browser evidence,
  not a native-window acceptance run.
- Repository contract suite passed, including 176 tool tests. Model-pin tests: 47 passed.
- Rust: meeting-core 261, transcription-core 216, provider-registry 17 tests passed.
  Existing external/hardware tests remain ignored. The new real segmentation download
  test was run separately and passed.
- Changed Rust crates passed Clippy with warnings denied; touched Rust files formatted.
- Installer failure tests cover truncated, oversized and corrupt bytes, cancellation,
  staging cleanup, competing installers, preserved existing models and Windows locks.

## Native and download evidence

All four existing speech archives were downloaded, checked against their committed
archive SHA-256, and their encoder/decoder/joiner/token files matched the publisher
mirror. Segmentation's model.onnx also matched. Pins are in config/model-file-pins.json.

The production Rust installer downloaded Parakeet TDT v3 into a disposable workspace
directory: 670,576,784 bytes verified. The existing sherpa-onnx runtime transcribed the
repository's 6.33-second speech fixture successfully:
"The quick brown fox jumps over the lazy dog. This is a test of local transcription."
This proves download/layout/runtime compatibility, not recognition quality.
Stated-metadata verification passed for TDT (including measured Git-tracked tokens)
and segmentation. No live streaming protocol or recognition-quality gate changed.

A debug-profile Windows NSIS installer built successfully with the production frontend.
The frontend was built separately because the local Tauri beforeBuildCommand could
not resolve tsc through the worktree dependency junction. A validation-only config
disabled that duplicate step; shipped workflow/build commands were not weakened.

The generated installer.nsi includes File entries for both notices and the Silero
resource; the staged notice equals the source by SHA-256:
a17b4ebf1f951aaecdf124e106a2fa2b96845dc15c7fce85770ae90c012343a9.
Installer SHA-256:
8d675af36458dd6d7233e87e435978995252e7e7191b173a0e26385fc7a91f48.

This is bundling/resource evidence. The installer was not installed into the user's
profile, and this is not an optimized release, clean-machine, macOS or Linux acceptance run.
Experimental platform jobs generate target-specific notices and fail if new attribution
needs attention; those jobs were not run locally.
