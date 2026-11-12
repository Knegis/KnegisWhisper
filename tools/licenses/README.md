# Installed third-party notices (KNEG-533)

The installer includes NOTICE and THIRD_PARTY_NOTICES.txt beside the application.
The notice document covers the desktop target's Rust graph, including build dependencies,
the packages present in emitted Vite chunks, bundled fonts, and Silero VAD.
It is an inventory of existing components, not a new dependency approval policy.

Install cargo-about 0.8.4 with `cargo install cargo-about --version 0.8.4 --locked`.
Run `npm.cmd --prefix apps/desktop run build`, `cargo fetch --locked`, then
`python tools/licenses/generate.py`. CI runs the same generator with `--check`.
Use `--target` for a different Rust target; experimental release jobs generate their
own notice file before bundling. The checked-in file is for x86_64-pc-windows-msvc.

The Rust inventory uses the desktop manifest and tauri/custom-protocol feature.
Original packaged license and notice files supplement cargo-about's detected texts.
A missing original attribution fails generation unless a version-specific notice is
recorded under overrides/. Those records include immutable upstream URLs and SHA-256,
taken from each crate's packaged .cargo_vcs_info.json revision. Font license and
provenance texts in `apps/desktop/src/fonts/*.txt` are included verbatim; Ndot 57's
notice records its restrictive embedded terms, not an open-source license grant.
Silero's converted bytes are checked against the publisher's pinned model hash;
assets/sources.json records upstream license and conversion-card provenance.

The frontend build writes `apps/desktop/.notice-build/dependency-notices.json`
from the modules in emitted chunks. This ignored intermediate inventory stays outside
`dist/` and is not bundled into the application. Each build clears stale inventory.
Missing package owners or notice files fail the build. No timestamps or local paths go
into the installed notice document. The development-only --rust-json option reuses a
previous inventory; CI never uses it.

Run `python -m unittest discover -s tools/licenses -p "test_*.py"` for missing-notice,
copyright preservation, deterministic output and source-tampering checks.
