# Recovery provenance and lightweight Notes validation

Implemented in `codex/kneg-423-420`, based on `dev` at `17762dc`.

KNEG-423 captures the actual selected live provider in the existing meeting-plan
sidecar. Recovery and normal saves share disclosure composition. Older, absent,
unreadable and unknown-provider plans do not invent a live provider; typed-only
notes have no transcript provenance. There is no database migration or backfill.

KNEG-420 adds desktop-only metadata list/read commands while retaining existing
full-note API/CLI/MCP responses. Autosave updates committed metadata without a
library/folder/Trash scan and never treats an acknowledgment failure as an unsaved
write. Search's SQLite projection returns clocks rather than text/words, retaining
segment positions and timestamp normalization.

## Evidence

Final local gates: 531 desktop library tests passed (12 ignored), 159 notes-core
unit tests passed, 50 local API tests passed, and 399 frontend tests passed.
The named native ignored test passed separately. Typecheck/build, Clippy with
warnings denied, formatting, IPC type drift, command/capability/domain/integration
and development-bundle audits passed.

- Rust tests cover sidecar compatibility, authoritative provider selection and
  cloud/local/unknown/typed-only recovery, and normal-save disclosure.
- Projection tests compare serialized metadata before and after adding a 1 MB
  body, a 1 MB summary and a large word payload: metadata is identical. Full-note
  reads still return the original transcript. Filters, person-email matching,
  limits and Trash exclusion are covered.
- Search tests compare projected times to actual Transcript rows for relative
  and epoch clocks, missing timestamps and malformed segment fields. The measured
  three-row timing projection is under 150 bytes despite 30,000 word-text bytes.
- `tools/visual-check/notes-loading.mjs`: 11 headless Edge fixture checks, including
  targeted autosave calls, focus/editor identity, delayed acknowledgments, failure
  and retry, reverse-order selection reads, People-filter autosave and 720x520.
- Existing People probe: 6 checks; playback probe: 4 checks, all passed against
  browser fixtures. These are separate from storage and native evidence.
- Native Windows ignored test
  `meeting::native_recovery_test::recovered_provider_and_lightweight_notes_cross_native_ipc`
  passed with an in-memory database and disposable recordings/WebView profile.
  It invokes recovery with synthetic transcription output through real WebView
  IPC, then verifies cloud provenance, lightweight listing and committed-update
  metadata. It does not run an ASR model or measure real-meeting quality.

## Commands

```powershell
cargo test -p kw-notes-core -p kw-local-api -p knegiswhisper --lib --features tauri-test
cargo clippy -p kw-notes-core -p kw-local-api -p knegiswhisper --all-targets --features tauri-test -- -D warnings
cargo fmt --all --check
npm.cmd --prefix apps/desktop run build
npm.cmd --prefix apps/desktop test
node tools/command-audit/scan.mjs
node tools/capability-audit/scan.mjs
node tools/domain-scanner/scan.mjs
node tools/integration-audit/verify-parity.mjs
node tools/dev-bundle-audit/scan.mjs
```

For native verification start the worktree Vite server on 5198, then run the named
ignored test with `-- --ignored --nocapture`. The browser probes also accept that
server URL. Reports/screenshots are ignored artifacts under `tools/visual-check/out`.

No real user database, live provider, hosted CI, release package or other operating
system was exercised. No real-library latency claim is made. The production build
retains the existing Vite chunk-size warning.
