# KNEG-455, KNEG-400 and KNEG-395 progress validation

Implemented on `codex/kneg-455-400-395`, based on `dev` at `09635c8`.

## Changes

- KNEG-455 keeps one transcript mounted across layout changes, preserving Find,
  collapsed sections and speaker-rename drafts/focus. A narrow mini player remains
  below it, wide Replay has the header, single selections have Clear, Ctrl+F/Cmd+F
  opens Find outside editable fields/dialogs, and failed media cannot play or seek.
- KNEG-400 retains each Settings form's save convention and registers persistent
  drafts with a shared navigation gate. Save/Discard/Stay covers history, search,
  setup links, Return to note and local credential-row replacement. Writes serialize,
  submitted revisions are acknowledged, failed drafts survive and credential values
  remain in their form state. Forced termination and tray Quit are outside scope.
- KNEG-395 reports actual Stop/recovery stages and total monotonic processing time,
  with revision-ordered snapshots/events and cleanup on completion/failure. Both
  meeting views use the same labels. Recovery does not open a new compact window.
  Start remains disabled while finishing. No percentage, ETA, early note access or
  background speaker-label update was added. No database migration is needed.
  Since 2026-11-17 the note is saved before its speakers are analysed and the labels are
  added by a job on the note (KNEG-487), so there is no Analysing speakers stage; see
  [interrupted meeting recovery](KNEG_316_MEETING_RECOVERY.md#speaker-labels-after-the-save).

## Evidence (Windows, 2026-10-12)

- Frontend: 497 tests passed; typecheck and production build passed. Vite retains
  its existing advisory about a chunk larger than 500 kB.
- Desktop Rust: 540 tests passed, 15 ignored in the ordinary suite. Clippy with
  warnings denied and Rust formatting passed; generated IPC types match Rust.
- `three-issues.mjs`: 28 passing scenario groups over invented browser fixtures,
  split across light/dark. Includes transcript resizing/focus, failed playback,
  save races, failure/retry, credential privacy, blur-save waits, calendar drafts,
  independent immediate-setting failures/retries, refresh failure after a committed
  write, main progress and the compact 392x92 layout.
- Tool self-tests: 197 passed.
- Existing transcript-column probe: all 17 checks passed in both themes, including
  narrow search-hit reveal, sticky player/header, range playback and long transcripts.
  PageDown is allowed to settle before the next distinct Follow press.
- Existing Follow/F2 probe: 4 passing theme/width combinations.
- Existing summary setup probe: 4 passing theme/width combinations at 720/1440px,
  including explicit saving, same-note return/focus and manual generation.
- Hidden native WebView: `native_meeting_progress_and_settings_ipc` passed when
  explicitly run, using an in-memory database and a synthetic WAV/typed-notes
  sidecar in a disposable profile. Real recovery emitted Preparing recording,
  Transcribing recording and Saving note; skipped live/speaker stages were absent.
  Elapsed time stayed monotonic and progress cleared after success and failure.
  Settings writes round-tripped; invalid settings preserved the committed value.

Browser reports/screenshots and native logs are under `target/three-issues` in
this worktree. Restricted browser launches failed; disposable headless/native
checks succeeded when run with process permissions outside that restriction.

## Limits

No user database, recordings or credentials were used. These checks do not prove
live-provider authentication, acoustic quality, real speaker-analysis quality,
macOS/Linux behavior, Narrator/touchscreen behavior, hosted CI or release readiness.
The other 14 ignored native tests were not explicitly run for this batch.

No merge, push or Linear status update was performed during implementation.
