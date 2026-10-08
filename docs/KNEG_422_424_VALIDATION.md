# KNEG-422 and KNEG-424 implementation and validation

Implemented on `codex/kneg-422-424` from local `dev` commit `307fc2d`, in an
isolated worktree. This ledger covers the scope selected by the owner: all
correctness, UI, documentation, refactor and automated-test follow-ups; explicit
recording deletion retry; meeting/dictation starts wait until a microphone test
actually releases the device. No merge, push, hosted CI or Linear status change.

## KNEG-422 disposition

| Item | Implementation and evidence |
| --- | --- |
| Replay after intentional deletion | Migration `0012_note_recording_state.sql`, local structured disposition, intentional-deletion copy and Attach another recording. Browser meeting-recordings and follow-ups checks. No prose matching/backfill. |
| Failed deletion never retried | Durable pending/failed intent plus `note_recording_retry_deletion`; explicit Retry deletion, player paused first, double-click guard. File-database reopen/idempotence/replacement tests and native IPC smoke. |
| File removed before commit failure | Derived `deleted-update-failed` status without read-time mutation, no broken player, attachment cleanup action. A deferred foreign-key trigger makes commit fail after actual file removal in core and native tests. |
| Kept claim before attachment | Composition no longer writes ownership prose. State is established only against a matching live attachment. Missing-attachment test and stopped-save tests; failed attachment retains Interrupted recovery. |
| Wording describes current setting | Disposition describes the choice captured for this meeting at Start. Meeting-recordings check changes the setting mid-session. |
| Typed-only save has wrong reason | Separate `kept-untranscribed` outcome/state and copy. Eligibility/save/recovery tests and native kept-state refusal. |
| Crash window missing from docs | NOTES_WORKSPACE and ENGINEERING_LOG now name app crash, force-quit, power loss and post-removal commit failure. Filesystem removal remains irreversible even if a database transaction rolls back. |

The marker belongs to a note and specific storage key; note deletion cascades it,
and attachment insertion clears previous intent in the same transaction. Retry
rechecks identity and Trash under `BEGIN IMMEDIATE`, performs the WAL checkpoint,
and never deletes a replacement's file. Status/error updates after a failed
removal are best effort; the established intent remains if that update fails.
Older and imported notes return no local disposition. Shared-folder packages do
not contain the marker or deletion error. These are local-retention facts.

## KNEG-424 disposition

| Area / finding | Disposition |
| --- | --- |
| Meetings: unnamed offer/API starts | Accepted existing limit; these entry points still start unnamed in no folder. |
| Duplicate flush-timeout wording | Live error suppressed when the existing actionable flush caveat is shown. Discard browser checks pass. |
| Discard check 4 flake | Assert the status/question/status fence relative to the question, allow unrelated polling outside it, and still require zero cancellation on a changed meeting. |
| Large meeting module | Plan/title helpers moved to `meeting/plan.rs`; composition and stopped/discarded/recovered persistence moved to `meeting/save.rs`. Settings keys/commands remain top-level and settings audit passes. |
| Folder retry with recording | Triggered folder-deletion race with a real recording attachment; asserts exactly one saved note with the attachment. |
| Stale microphone result | Request generation invalidated on device change, recording state and unmount. Delayed result after an external meeting is tested in the browser. |
| Missing Rust start refusal | Shared capture interlock covers meeting start and recorder thread, and dictation thread. Atomic fake-gate/factory tests; all UI, hotkey and API starts use these paths. |
| Microphone deadline | Five-second capture, ten-second command deadline, cooperative cancellation. Fake worker success/open failure/cancellation/deadline tests show that a blocked worker retains the interlock until exit. A driver permanently blocked inside open may require app restart. |
| Over-broad ignored-test command | README names four individual real-microphone filters. Ordinary thread-reply proof uses an injected failing capture factory. |
| Device-free thread and lifecycle proofs | Unignored fake-factory thread response and refused-before-open tests; `is_under_way` uses the tested phase predicate. |
| Minimum-window microphone menu | Open menu measured and screenshot inspected at 720×520, including footer and every option inside the viewport. |
| Stale transcription setting reads | `useRefresh` read/mutation generations; serialized language write lane with latest committed readback and event handling. Rapid three-choice browser check. |
| Chip wrapping unseen | Measure chip positions and row counts, alongside scroll widths. Default strips fit the intended maximum two rows at 720×520; both menus and microphone menu fit. |
| Unsupported Mistral request language | Explicit unsupported request-level language returns an error. Stored dictation-language fallback remains detection with UI disclosure. Route regression test; no provider/network request made. |
| Hidden/elsewhere model installs | Rust owns active job snapshots/events with monotonic revisions. Renderer subscribes then reads; status is independent of the filter and survives page navigation. Actual English-only model hidden by Swedish filter, elsewhere snapshot and out-of-order completion event tested. |
| All engines unpublished copy | Empty model list says no local speech models are available on this platform; language-filter emptiness still names the filter. |
| Deepgram provenance fixture name | Saved fixture provenance uses Deepgram Realtime. Fixture picker labels retain their established presentation. |
| Over-claiming test name | Notes JSON test renamed to optional_live_caption_name_is_omitted_from_json_when_absent, matching its actual assertion. |
| Local streaming recovery failure | Already covered on starting dev by failed_recovery_keeps_streaming_text_and_marks_it_incomplete; retained and passed. |
| Duplicate text/word conversion | Dependency-free `kw-transcript-text` comparison shared by notes/transcription cores; desktop stored-word conversion shared by media and meetings. Unicode/punctuation comparison test. |
| Keyboard word seek | Roving tab stop with ArrowLeft/Right/Home/End, Enter/Space seek, event propagation isolated from row selection. Browser keyboard check. |
| Highlight pause cleanup | Browser check verifies no highlighted word immediately and after pause; effect cancels its animation frame. |
| Silent word loss | Aggregate discarded token/word counts and fixed reason codes in diagnostics. No token text, transcript, addresses or personal note content logged. Real engine timing coverage remains a manual quality check. |
| Unreadable participant entries | Accepted documented compatibility: preserved verbatim even if no readable people remain. |
| Imported participant cap | Above-cap imports can remove unchanged entries; additions/renames above 200 refused. Boundary/preservation test; normal new lists still capped. |
| Queued summary transcript change | Accepted existing invalidation; no production transcript text editor is introduced. |
| Unbounded People list | SQL-backed 50+1 lookahead pages and Show more people. Full-list API preserved. Core page bounds/offset test and 65-person browser fixture. |
| Participant refresh falsely fails save | Commit and readback errors separated; optimistic committed list retained and Retry refresh offered. Browser commits a participant while metadata readback fails. |
| Trash focus after blank-space click | Pointer/keyboard interaction generation cancels pending focus before/during async completion; title focus polling also stops on interaction. Trash browser check covers blank-space presses during delayed Undo. |
| Partial recording loss after failed Empty trash | Plain failure message explicitly says some recordings may already have been removed. Existing rollback/multi-file deletion tests retained. |
| Open note externally trashed/deleted | Two-second visible/focus metadata checks; mounted note/summary drafts retained, read-only editor/title/people/rename controls, Copy/Restore/explicit Save retained draft/Discard. Browser retains a failed dirty title through Trash and explicit restore/save. |
| Trash duration without recording | SQL coalesces attachment duration with note audio duration, without inventing attachment/bytes. Core test. |
| Copied visual helpers | Shared `interactions.mjs`, used by touched meeting/chip/people/word/loading scripts and the new follow-ups probe. |
| Existing cpal/Vite warnings | Deferred existing toolchain/bundle concerns. Current clippy -D warnings is clean; production build still reports the existing Vite chunk-size warning. |

## Verification

- Full Rust workspace with `--features tauri-test`: 2,199 passed, 27 ignored,
  across 63 unit/integration/doc-test suites. After that run, the additional fake
  microphone-open deadline test passed with the whole microphone-check group
  (10 passed, 1 real-device test ignored). The JSON test rename changes no behavior.
- Clippy workspace/all targets with tauri-test and `-D warnings`: passed.
- Rust formatting and diff whitespace checks: passed.
- Desktop TypeScript suite: 401 passed; typecheck and production build passed.
- Generated IPC types/provider registry, integration parity, settings reachability,
  command grants, capabilities, feature usage, domain and dev-bundle audits passed.
- Headless Edge against invented fixtures: follow-ups 14 checks; word highlight 5;
  meeting recordings 5; chip menus 12; meeting discard 6; notes loading 11; Trash 10.
  Reports and screenshots are in ignored `tools/visual-check/out/`; minimum-window
  microphone menu, deleted Replay and retained external-Trash draft were inspected.
- Real Windows hidden WebView2, disposable reopened file database and synthetic
  files: recording_deletion_retry_crosses_native_ipc_after_reopening_the_database
  passed (actual commit failure/file removal, persisted cleanup, Trash refusal,
  explicit retry, kept reason, People page and installation snapshot IPC).

Repeat browser probes with Vite on 5199, e.g.:

```sh
node tools/visual-check/follow-ups.mjs http://127.0.0.1:5199/
node tools/visual-check/chip-menus.mjs --url http://127.0.0.1:5199/
node tools/visual-check/word-highlight.mjs --url http://127.0.0.1:5199/
cargo test -p knegiswhisper --lib --features tauri-test recording_deletion_retry_crosses_native_ipc_after_reopening_the_database -- --ignored --nocapture
```

## Remaining evidence limits

Real microphone/driver behavior, recording from UI/hotkeys/API while the test runs,
ASR word-timing quality (KNEG-378 item 41), audible playback, real downloaded-model
progress, macOS/Linux, hosted CI and installer/release validation were not exercised.
The native check establishes WebView IPC/storage behavior, not speech quality.
KNEG-421 local-Whisper lock contention remains a separate issue.
