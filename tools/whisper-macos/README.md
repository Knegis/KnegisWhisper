# macOS Whisper server artifacts (KNEG-42)

Run the full **CI** workflow manually on the approved GitHub branch; it calls
the reusable macOS artifact workflow from that same revision. The dedicated
**macOS Whisper server artifacts** workflow also supports manual dispatch.
The workflow builds the pinned whisper.cpp b4938 commit on native Intel and
Apple Silicon runners. It produces two independent tar.gz archives with license
notices and build provenance. The baseline is macOS 13; optional Intel CPU
instructions, Metal, CoreML, OpenMP and dynamically loaded ggml backends are off.
System Accelerate remains available. Binaries have an ad-hoc signature and are
not notarized.

Before upload, the job checks architecture and dynamic-library dependencies,
extracts the archive into a fresh directory, runs `--help`, loads the
repository-pinned tiny model, and transcribes the pinned upstream JFK fixture.
`measurement.json` records archive SHA-256 and archive/extracted sizes only after
that succeeds. Failure logs are retained as a separate diagnostic artifact.

Local Windows validation covers Python syntax and workflow structure only.
Native builds and smoke tests must pass on both runners before release.

## Publication gate

This workflow has read-only repository permissions and uploads Actions artifacts;
it does not create a release or enable macOS in `config/sidecars.json`.
After reviewing both native runs, obtain approval to publish those exact archives
under a project release. Then use their measured digests/sizes and immutable
release URLs in the sidecar catalog. Validate installation from those URLs on
both architectures before declaring KNEG-42 complete. Never substitute an
xcframework or a guessed checksum for a server archive.
