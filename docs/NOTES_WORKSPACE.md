# Notes workspace

Implemented on 2026-09-22 for [KNEG-297](https://linear.app/axelalbertkarlsson/issue/KNEG-297).
Recording-time personal notes followed in
[KNEG-298](https://linear.app/axelalbertkarlsson/issue/KNEG-298) and
[KNEG-300](https://linear.app/axelalbertkarlsson/issue/KNEG-300). Release
validation remains open.

## Using Notes

### Sharing a folder

Use a folder's **…** actions button or context menu and choose **Share folder…**.
Select the notes and whether to include personal notes, summaries, transcripts and
recordings. Everything starts checked. Save the `.knegis` file and send it through
your usual file-transfer channel. This is a snapshot; later edits are not shared
automatically. Packages have no password protection.

The recipient chooses **Import shared folder…**, checks the preview and folder name,
then imports a separate, editable local copy. Existing notes stay intact. A duplicate
folder name gets a numbered suffix; importing again creates another copy. Included
recordings play against their transcripts, without retranscription or summary calls.
When transcripts are excluded, summary source links are removed.

Cancel stops unfinished work; **Keep working** allows navigation while processing.
Return to Notes to see progress. See [folder sharing](FOLDER_SHARING_DESIGN.md) for
format limits, recovery behavior and validation commands.

### Editing notes

The library — folders, notes and search — is the list panel on the left, beside the
open note. Coming back to Notes from another page reopens the note that was open, until the
app closes; the first visit, or one after that note was deleted, opens the most recently
edited note. The title is a separate autosaved text field above **Notes / Transcript /
Replay**.
Notes opens by default, with personal Markdown notes first and an independently
editable AI summary below. An empty personal note offers **Start from transcript**;
this copies literal speech and available speaker labels without changing the source.

Transcript shows the original rows and playback actions. In windows 1280px or wider
a note's transcript is a column to the right of the note instead, and the Transcript tab
is hidden; crossing that width keeps playback and the reading position. A note with no
transcript has no column, and keeps the one-column layout at any width. **Hide
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
If the panel cannot respond, Meetings offers **Stop recording anyway** and
**Discard anyway** so a broken window cannot leave capture running or force a save
of a meeting meant to be thrown away (KNEG-418). Neither asks the window again.
Both keep the notes the app already has and warn that the newest edit may be
missing; Discard anyway asks first, as Discard does.
Typed notes become the saved note body even if transcription fails. Discarding a
recording keeps nonempty typed notes as a notes-only meeting note. An interrupted
WAV and its notes sidecar can be recovered from Meetings (KNEG-316).

A meeting can be named, and given a folder in the Folder chip, before Start on
the Meetings page (KNEG-365). Rust keeps both with the session and in a
`.meeting.json` beside the WAV, so the saved note, a discarded meeting's
notes-only note and a recovered note all take them, however the meeting ends. A
meeting with no name is named for when it started. A folder deleted before the
note is saved is left out; after Stop or Recover the outcome says the note isn't
in a folder, while a discarded meeting's notes-only note, and the notes saved
when a recording can't be closed, are left out of it without a word. The
notification window's Record and the local API start an unnamed meeting in no
folder.

Settings › Meetings can delete each recording once its transcript is saved
(KNEG-409), off by default. A meeting reads the switch when it starts and keeps
that choice in the session and the `.meeting.json`, so changing it mid-meeting
affects the next meeting. The recording is deleted only when its note was saved
with a whole transcript: a note of typed notes alone, or of the running
transcript saved because the whole-recording pass failed, keeps its recording,
and the outcome says why. The WAV is attached exactly as with the switch off,
then the attachment and the file are removed in one transaction
(`recordings::delete_attached`), which also adds "The recording was deleted
after transcribing (Settings › Meetings)." to the note's recording details; a
file that can't be removed stays attached, and the outcome says so. The note
keeps its transcript and typed notes, and Replay has no player. While the switch
is on the Meetings page says so beside Start, at the foot of its list and in the
live view of a meeting started that way. Discard always deletes the recording.

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
- Migration `0007_search_tokenizer.sql` rebuilds and refills both search indexes,
  notes and dictation history, with `unicode61 remove_diacritics 0` (KNEG-405). The
  default tokenizer folded å and ä to a and ö to o, so "for" found "för", "har" found
  "här" and "sa" found "så". Letter case still folds. Every accent now counts, so
  "cafe" no longer finds "café".
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
