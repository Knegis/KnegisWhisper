# KNEG-177: desktop import recovery

When the app stops during a desktop audio/video import, Upload now restores an
`Import interrupted` notice with instructions to choose the file again and select
`Import and transcribe`. The notice survives further restarts until another
desktop import is attempted. It does not display an invented elapsed duration.

The existing durable `import_jobs` record remains the source of truth. Migration
0004 adds an origin field; the desktop records its origin before preparing the
transcription route. API uploads retain their existing behavior. Startup first
marks unfinished jobs failed, then restores the latest desktop job only if that
job was interrupted. Successful, failed and cancelled newer desktop jobs prevent
older interruptions from reappearing. Note creation and successful job completion
already share a transaction, so a crash after commit does not produce a retry
notice for a saved note.

No source path, audio, transcript or picker grant is added to recovery metadata.
Recovery does not resume an import. The existing startup scratch cleanup remains
in place, and the import commands still require a fresh picker grant after restart.
Jobs written before migration 0004 cannot be reliably attributed to the desktop;
they are not retroactively displayed as desktop interruptions.

## Automated verification

- Desktop library: 381 passed, four opt-in native tests ignored.
- Storage: 60 passed, including migration reopen checks.
- Frontend: eight passed; TypeScript and production Vite build passed.
- Clippy for desktop/storage, all targets, with warnings denied: passed.
- Generated IPC declarations regenerated and checked by the desktop tests.
- Tauri test feature: five IPC tests passed, including all import commands'
  picker-grant enforcement at the IPC boundary.
- Import command grant audit, integration parity and generated registry type
  checks passed; `git diff --check` passed.

New recovery tests reopen a file-backed database twice, verify that a newer API
job cannot hide the desktop interruption, reject cancellation of a recovered
terminal snapshot, allow another import, preserve a live snapshot, and ensure
newer successful/failed/cancelled desktop jobs suppress the old notice.

## Native Windows verification, 8 September 2026

Used a disposable `KW_AUTOMATION_PROFILE`, a separate WebView2 folder, copied
installed Whisper/ffmpeg binaries and the tiny model. No owner database or cloud
provider was used. The production frontend build also passed; this native check
used the worktree's Vite frontend in the real Tauri application.

1. Selected a generated 600-second WAV through the Windows file picker and
   pressed Upload's `Import and transcribe` button.
2. Measured operation `b75f4d16-b845-4c25-be89-ffc9d484c149` transcribing through
   local/tiny at 9.72 seconds, then forcibly terminated only the verified
   disposable application process.
3. Restarted the same profile. The actual `media_import_snapshot` returned the
   same ID with stage `interrupted`, no result, and file-reselection guidance.
   Visually inspected Upload: the notice was fully visible, the picker was
   enabled and `Import and transcribe` was disabled until selection.
4. The import scratch directory was empty. Source SHA-256 was unchanged:
   `4981A821F0D381E156D880AA289CC2244D009E0DAC607F57D585B0B7BD7FC920`.
5. Invoking the import command with the previous path before reselection was
   rejected: `this file was not chosen through the import dialog, or is no
   longer where it was. Choose it again.`
6. Selected the same file again through the native picker and pressed Import.
   A new operation, `fee99bb3-1405-42b5-826c-bf78446b5a80`, reached transcribing.
   It completed in 16.70 seconds, saving note 1 with its 600-second recording.
7. Restarted again after completion: `media_import_snapshot` returned null,
   confirming that the old interruption did not reappear.

Local evidence is under `target/kneg-177-native` in the worktree, including
`before.json`, `recovered.json`, `recovered.png`, `retry-success.json` and
`final-evidence.json`.
