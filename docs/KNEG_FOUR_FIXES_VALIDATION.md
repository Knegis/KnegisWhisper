# KNEG-536 / 515 / 533 / 532 validation

Validated 2026-11-10 in an isolated worktree from dev at f4d5bde.
No merge, push, publication, or changes to the user's installed profile.

**Status:** reviewed and merged into dev on 2026-11-10, with the review follow-up
in 26220ba. All four issues were closed in Linear the same day. KNEG-546 moves
the CAM++ embedding download to a byte-equivalent fixed-commit mirror.

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

The English CAM++ embedding download now uses `csukuangfj/speaker-embedding-models`
at commit `0743f301363dec56491a490f6d6cbc9d67f9a3bf` (KNEG-546). Measured
2026-11-24 by downloading from that commit and hashing: 29,596,978 bytes, SHA-256
`357a834f702b80161e5b981182c038e18553c1f2ca752ed6cec2052365d4129b`,
matching the copy the application installed on the development machine on
2026-09-03, from the GitHub asset before it was replaced, and the tree API's LFS
metadata. The single-file layout, size and digest stay unchanged. A publisher can still delete
or rewrite repository history; the download then fails its check and installs
nothing. Different weights cannot be installed.

## Initial implementation checks

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

Before the review follow-up below, a debug-profile Windows NSIS installer built
successfully with the production frontend.
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


## Review follow-up, 2026-11-10

- Notices CI now uses the changes job, including standalone NOTICE/LICENSE inputs.
  Docs-only PRs skip it; manual runs still execute it. Tests cover classification
  and the workflow wiring.
- Per-file installs use one fixed staging path per model under its OS lock. After
  process termination, the next install reclaims that path before downloading or
  reusing a complete installation. Cancellation still cleans it immediately.
  Recovery does not sweep other models' staging paths.
- Retained publication backups are restored when the target is missing/incomplete,
  or removed when the new target is complete. Filesystem failures report the
  recovery action instead of an invalid-manifest error and can be retried.
- The frontend inventory is written to ignored `.notice-build/`, outside `dist/`.
  The rebuilt production bundle contains no dependency-notices.json; the separate
  inventory still contains 110 packages. Fresh cargo-about 0.8.4 generation passes
  `--check` with the same 3,823,482 notice bytes and SHA-256 recorded above.
- PLATFORM_SUPPORT restores "What this proves and what it does not", including
  Whisper commit pins and the different weekly verification guarantees. It records
  immutable upstream export-workflow links establishing the csukuangfj2 publishing
  account for both Nemotrons and Parakeet Unified: three of the five repositories.
  Module-level installer explanations, notice-regeneration guidance in DEVELOPMENT,
  and NOTICE wrapping are restored/corrected.
- All four Linear descriptions now render the full branch name as code. All four
  issues are closed since the merge; see Status at the top. Follow-up commits include explanatory bodies; prior reviewed commits are retained.

Fresh checks on Rust 1.99.0: meeting-core 261, transcription-core 221 and
provider-registry 17 unit tests passed (499 total). Clippy for those crates with
`--all-targets -- -D warnings` and workspace formatting passed. Recovery coverage
includes a child process exiting mid-transfer without destructors, repeated exits
and retry, preservation of other staging data, lock-before-cleanup, publication
boundary states, a failed retry preserving the recovered model, and Windows locks
with an actionable error and successful retry.

Node tool suites: 73 passed (CI selection/entry points, notices, model pins and
retry). License Python tests: 4 passed. Registry freshness, domain scanner,
frontend typecheck/production build and fresh notice generation passed.
Logs for this follow-up are under ignored `.validation/review-*` in the worktree.

The original real-download, browser and NSIS results above were not repeated for
this follow-up; those remain evidence for the initial implementation. No native
window, clean-machine installer, desktop-app Clippy or hosted CI run was added.
No merge or push was performed.
