# Notes workspace

Implemented on 2026-09-22 for [KNEG-297](https://linear.app/axelalbertkarlsson/issue/KNEG-297).
Recording-time personal notes followed in
[KNEG-298](https://linear.app/axelalbertkarlsson/issue/KNEG-298) and
[KNEG-300](https://linear.app/axelalbertkarlsson/issue/KNEG-300). Release
validation remains open.

## Using Notes

The library — folders, notes and search — is the list panel on the left, beside the
open note. The title is a separate autosaved text field above **Notes / Transcript /
Replay**.
Notes opens by default, with personal Markdown notes first and an independently
editable AI summary below. An empty personal note offers **Start from transcript**;
this copies literal speech and available speaker labels without changing the source.

Transcript shows the original rows and playback actions. In windows 1280px or wider
the transcript is a column to the right of the note instead, and the Transcript tab
is hidden; crossing that width keeps playback and the reading position. **Hide
transcript**, beside Export and More above the editor, puts the note back in the one-column
layout with the Transcript tab, and **Show transcript** brings the column back; both keep
playback and the reading position the same way, and the choice holds for every note until
the app closes. Replay
provides the full player and recording details without repeating the transcript, at
any width: in a wide window the player sits at the foot of the transcript column and
moves into Replay while that tab is open, keeping its place and whether it was playing.
Audio continues across tabs through one player; compact controls remain available
after playback starts. Leaving the note
stops playback. Missing recordings offer the existing native WAV attachment flow.
Summary references reveal the supporting row — in Transcript, or in the column when
the window is that wide — and play its interval when audio exists. Tabs support arrow
keys, Home and End; a hidden Transcript tab is skipped. At widths of 900px or less,
the list panel initially collapses (and collapses when entering that width); the
title bar's list toggle reopens it. Tabs wrap when necessary.

Title, personal notes and summaries flush before tab changes, note changes and
native window closure. A failed save retains the draft and prevents those transitions;
retry remains available. Blank titles become “Untitled note”.

During a recording, **My notes** in the meeting panel is a separate editor from
the live **Transcript**. The editor autosaves to memory and a `.notes.md` sidecar
beside the WAV. The panel's Stop action flushes its draft before ending the meeting.
Stop and Discard in Meetings ask the meeting window to flush and acknowledge the
draft first; a reported save failure keeps the recording open and shows an error.
If the panel cannot respond, Meetings offers **Stop recording anyway** so a broken
window cannot leave capture running. It warns that the newest edit may be missing.
Typed notes become the saved note body even if transcription fails. Discarding a
recording keeps nonempty typed notes as a notes-only meeting note. An interrupted
WAV and its notes sidecar can be recovered from Meetings (KNEG-316).

## Data compatibility

- New meetings save the typed personal notes, or an empty body if none were typed.
  Media imports, including API imports, start with an empty personal-note body.
  Transcript, recording and automatic summary scheduling stay separate.
- Recording-source facts and transcription warnings are saved separately as
  `notes.recording_details`, returned as nullable `Note.recordingDetails`, and shown
  read-only in Transcript and Replay. Update requests cannot edit this field.
- Historical note bodies and OpenWhispr imports are preserved. A leading H1 matching
  the stored title is hidden as the old editor title representation, and removed
  from storage only on an explicit title/body edit. Opening a note does not write it.
  Other headings are retained. A historical body may mix transcript text, warnings
  and authored notes; no automatic migration attempts to guess which text belongs
  to whom. The stored transcript is still shown independently in Transcript.
- Migration `0006_note_workspace.sql` rebuilds FTS with a derived `transcript_text`
  column in the index itself, rather than duplicating that projection in `notes`.
  SQL triggers maintain it for all writers, including foreign imports and transcript
  updates. Structured segment text is indexed; legacy plain text is retained for
  search. Snippets prefer matching personal notes, then transcript text.
- No new renderer command or user-editable transcript API was added. Existing note
  exports and separate summary exports retain their behavior.

## Verification

Automated coverage includes storage migration backfill, body preservation, transcript
search with empty personal notes, index update/deletion, note wire shape, empty import
bodies, literal transcript copying and generated types. Frontend build/tests, affected
Rust tests, strict Clippy and repository settings/command/capability audits passed.
Counts: desktop Rust 388 passed / 4 ignored, notes 71, storage 66, local API 50,
and frontend 27 passed. Native checks below are separate evidence.

Native Windows WebView2 validation used a disposable profile and synthetic notes/audio:

- Matching legacy title handling without writes on opening; explicit title/body save.
- Literal transcript copy and three-tab navigation; blank titles and keyboard focus.
- One audio instance continuing across tabs with compact controls.
- Forced SQLite save failures blocking navigation; retry retaining drafts.
- Forced failure on native close keeping the window open, followed by immediate
  close persisting title and body edits (checked in SQLite).
- Missing-recording/transcript states and a 720px viewport with every tab visible.
- Existing synthetic-provider summary smoke: generation, edited-result retention,
  cancellation and source navigation into Transcript.

Reproduce only against a disposable native profile:

```powershell
$env:KW_AUTOMATION_PROFILE = Join-Path (Get-Location) 'target/notes-smoke/profile'
$env:WEBVIEW2_USER_DATA_FOLDER = Join-Path (Get-Location) 'target/notes-smoke/webview-native'
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9337'
# Start Vite on 5183, then the debug desktop binary with these variables.
# Optional summary regression, before closing the window:
node tools/visual-check/summary-smoke.mjs --isolated-profile
node tools/visual-check/notes-smoke.mjs --isolated-profile --close-window
```

The Notes smoke creates synthetic notes/audio and a temporary failure trigger only
in `target/notes-smoke/profile`. Reports and screenshots are under `target/notes-smoke`.
The summary smoke changes provider settings and uses a loopback synthetic provider.
Never point either smoke at the normal app profile.

On 2026-09-28, a second disposable native WebView2 profile checked the
main-to-meeting save request and acknowledgement, rejection on a synthetic save
failure, and a synthetic saved note with separate authored content, transcript and
player-only Replay view. The report and screenshot are under
`target/kneg300-native2/`. The app and dev server were stopped afterward. This
did not record a real meeting or exercise the editor's Stop/Discard sequence with
live audio; those KNEG-300 acceptance checks remain for a real meeting session.
The current frontend passed typecheck, build, 56 tests and the capability audit;
12 meeting composition/lifecycle tests and three notes-sidecar tests passed in
Rust. `git diff --check` passed.

Native file-dialog attachment/cancellation was not exercised in this pass. macOS,
Linux, hosted CI and release validation remain unverified. Synthetic audio/provider
checks do not establish real-meeting transcription or summary quality; KNEG-278's
model-quality limitations still apply. Remaining checks are tracked on KNEG-297.
