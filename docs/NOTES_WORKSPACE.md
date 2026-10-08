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
app closes; the first visit, or one after that note was deleted or moved to Trash, opens the
most recently edited note. The title is a separate autosaved text field above **Notes / Transcript /
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

A row that local Whisper timed word by word (KNEG-408) -- a media import, or a
meeting's whole-recording pass, on a Whisper engine -- is drawn word by word while
it plays, and the word being said is marked. Clicking a word plays from it; a row
or selection played with its Play button still stops at its end when the word is
inside it. Every other row is marked as a whole row, as before: notes saved before
word timings were kept, Parakeet, cloud providers and live captions, and a row whose
text no longer matches its words. With reduced motion asked for, the mark jumps
from word to word instead of fading. How far the mark sits from the sound on a real
Swedish recording has not been judged by ear yet (see Data compatibility for what
was measured).

Under the title, the meta row gives the note's date, the recording's length, its
people and where its transcript was made (KNEG-368): "Transcribed on this
computer", or "Transcribed by" the provider or server that made it. A meeting
whose saved transcript is the whole-recording pass was transcribed on this
computer; when its live captions came from a provider off this computer, the line
adds "· live captions by" that provider. A recovered meeting uses the same disclosure: its `.meeting.json` sidecar
keeps the actual live provider selected at Start. Older sidecars without that
field retain the local transcription attribution without guessing the live provider. A meeting saved from its live captions --
a finished local stream, or the captions kept because the whole-recording pass
failed -- names the live provider. Typed notes alone, notes saved before this was
recorded, and OpenWhispr and shared-folder imports say nothing.

A note's people (KNEG-362) are chips in its meta row, each with a button that removes
them, and **Add person** takes an email and, if wanted, a name. A person is their email:
a new one is trimmed and lower-cased, a second entry for one address is dropped, one that
isn't an address is refused in plain words under the row with the form still open, and a
new edits keep at most 200. An oversized imported list can shrink while it remains
over the cap, preserving unchanged imported entries; additions or renames above the cap are refused. An address an import already put on the note passes as it is, so
one that was never checked can't block changes to the note's people. Imported notes come
with theirs; nothing else fills them in yet (the calendar doing so is a follow-up). Saving
people isn't editing the note, so it doesn't move it up the library. The library's
**People** section, under the folders and the notes in no folder, lists 50 people at a time
with **Show more people** loading another bounded page. It counts everyone
named by email on a note and how many notes they're on, by the name on the newest note
that gives one; someone with a name and no email isn't listed. Pressing a person shows
**Notes with** them in place of the folders, with **Show all notes** to come back. They are
found among every note, not picked from the library's newest hundred, and listed as the
library is, at most a hundred and the most recently edited first; past that, a line says
how many of their notes are shown. Like the search, that is the panel's
state: a note opened from it keeps it, and back and forward don't return to it. A note's
people are part of what its summary was made from, so changing them marks a finished
summary out of date and stops one being made; a summary still waiting to start uses the
people on the note when it starts ([meeting summaries](MEETING_SUMMARIES.md)).

**Trash** (KNEG-401). Delete, in a note's menu, moves the note to Trash without asking:
"Moved “{title}” to Trash." shows above the note that opens in its place, the newest one,
with **Undo**. Nothing is deleted: the note keeps its transcript, its recording and the file
behind it, its people and its speaker names, and nothing empties Trash on its own -- it
holds a note until a person restores it or empties it. A note in Trash is in no list,
search, folder count, People count or shared folder, and the title bar's search and the
Meetings list leave it out too. **Trash**, at the foot of the library under People, says
how many notes it holds; pressed, its notes replace the tree, most recently deleted first,
each with when it went and how long its recording runs ("Deleted 3 Oct · 31 min"), and the
main area says what their recordings take on this computer ("Recordings in Trash take 412
MB on this computer."), with **Empty trash**. A note in Trash opens read-only, never in the
editor, so nothing saves into it: "This note is in Trash." with **Restore** and **Delete
forever**, its title and its typed notes as plain text. The same view shows when Back
returns to a note moved to Trash since. Restore brings it back as it was, in its folder --
or in none, when the folder was deleted meanwhile. Delete forever and Empty trash ask first,
naming the note and its recording's length, or how many notes and what their recordings
take, and say "Your original file isn't touched." when a recording was copied from an
imported file; no file name is shown. Empty trash deletes only the notes its question
counted: one moved to Trash while it was asking, through the local API say, stays. A
recording that can't be deleted -- an app holding the file open -- stops the whole
deletion with a line saying so, the notes still in Trash, and pressing again finishes.
After Delete, focus goes to Undo; after Undo or Restore, to the note's title; after Empty
trash, or Delete forever inside Trash, to Trash's page; after Delete forever on a note
reached by Back, to the title of the note that opens in its place. Focus moves only while
nothing else has taken it since the button was pressed: put somewhere else while the action
finishes, or while the title waits for its editor, it stays there. The line above the note
that says these things is a live region, always there, so what comes into it is announced. Leaving Trash opens a note,
the newest when the one open was deleted forever. Discarding a meeting is unchanged: its recording is
deleted at once, not moved to Trash. The local API's `DELETE /v1/notes/{id}`, the CLI's
`notes delete` and the MCP `notes_delete` move a note to Trash too, and from then on treat
it as gone; only the app restores a note or deletes one forever.

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
(KNEG-409/422), off by default. The choice is captured at Start in the meeting's
plan; changing the switch affects the next meeting. A whole-file transcript or
settled local stream qualifies for deletion. A partial transcript keeps the
recording because it may be incomplete; a typed-only note keeps it because
nothing was transcribed. Those are separate outcomes.

A local `note_recording_state` marker is established only after the WAV is
attached to a live note. The marker refers to that exact attachment and records
its planned deletion or kept reason. It is separate from authored content,
recording details and shared-folder exports; older/imported notes have no marker,
and no text matching or backfill guesses their history. Replay explains an
intentional deletion and offers **Attach another recording** if another copy
exists. Kept wording describes this meeting's choice, regardless of the current
setting. A failed attachment leaves no kept claim on the first note; its recording
and plan remain eligible for Interrupted recovery.

Deletion first establishes durable intent and performs a full WAL checkpoint,
then removes the attachment, sets the marker to deleted, removes the file and
commits within one writer transaction. A failed removal rolls back the attachment
change. A missing file with pending intent is read as **deleted-update-failed**:
the file is gone and attachment cleanup still needs an explicit **Retry deletion**.
The marker survives app restart. Retry checks the same key and live-note status
again inside the transaction, refuses notes in Trash, and is idempotent after
success. Replacing an attachment clears the old marker atomically. There is no
automatic deletion retry when opening a note or starting the app.

Filesystem removal cannot be rolled back. An app crash, force-quit, power loss,
or failed database commit after removal can leave an attachment referring to a
missing file. Reading the marker never repairs or deletes anything; it reports
the discrepancy and lets the person retry cleanup. Empty trash can likewise
leave notes in Trash with some recordings already removed when a later removal
fails, and its failure message now says so. Discard always deletes the recording.

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
- Migration `0010_transcribed_by.sql` adds `notes.transcribed_by`, returned as
  nullable `Note.transcribedBy`: where the transcript was made, as JSON
  (`provider`, `name`, `onThisComputer`, and `liveName` for a meeting's
  live-caption provider), set by meeting saves, recovery and media imports.
  Existing notes keep NULL and show nothing; nothing guesses where they were
  transcribed. `notes_create` and the local API's create don't take it, and
  shared-folder packages don't carry it.
- A transcript segment may carry `words`, a list of `{text, start, end}` on the
  segment's clock (KNEG-408). Only local Whisper writes them, from the per-token
  times its server returns with every timed request, joined into words; no forced
  alignment is run. The pinned server was seen sending them on 2026-10-08
  (`crates/transcription-core/tests/word_timings.rs`, ignored, `ggml-tiny`, the
  English fixture): every row came back with words that spell it. Against the
  fixture's measured speech the first sentence's first word started 0.01 s late and
  the second's 0.37 s early, and each row's last word ran on to the row's end. A
  segment without words leaves the key out, so it is
  stored byte for byte as before, and one with a `words` in any other shape, which
  an imported transcript may carry, reads as none. Both search indexes read only
  `text`, so a word is found by its row, never by its timing. `Note.transcript`, which
  `notes_list` and the local API's notes return whole, grows by about 46 bytes a
  word: a synthetic hour of 9,000 words is 169 KB without them and 589 KB with
  (`an_hour_of_words_is_measured` in `transcript.rs`). Media import's own answer to
  the screen (`MediaImportResult.segments`) carries no words.
- Migration `0011_note_trash.sql` adds `notes.trashed_at`, returned as nullable
  `Note.trashedAt`: when the note was moved to Trash, as `CURRENT_TIMESTAMP` writes it, and
  NULL for every note the migration finds. Not `deleted_at`, which is OpenWhispr's sync
  tombstone, refused by the schema checks and filtered out by the importer. A partial index
  holds only the notes in Trash. Every query that lists, searches or counts notes asks for
  `trashed_at IS NULL`. Its title, notes, folder, people and speaker names are refused for
  one in Trash ("This note is in Trash. Restore it to edit it."), as are attaching a
  recording and asking for a summary, and the local API answers `404` for one; `notes_get`
  alone still returns it. Its search index rows are kept, so Restore needs no re-indexing.
  `recordings::storage_keys` is deliberately not filtered: a note in Trash still owns its
  WAV, so crash recovery never offers it as an interrupted meeting. Moving a note to Trash
  cancels a summary queued or being made for it, and its summary can't be edited. Delete
  forever and Empty trash follow `delete_attached`'s order (`recordings::empty_trash` says
  why): a full WAL checkpoint first, so the move to Trash is on disk, and nothing deleted if
  it can't finish; then, in one transaction, the rows of the notes still in Trash deleted and
  their files removed, a file already gone counting as removed and any other failure rolling
  the deletion back with a plain-words error; then the commit. A file is never removed after
  the commit, where a crash would leave a WAV no note owns for recovery to offer back; the
  worst left is notes still in Trash without their recordings -- after an error the person
  saw (a later file that wouldn't go, a failed commit), or after an app crash or force-quit between removal and commit, or a power loss before the
  commit reached the disk, with no error at all -- and pressing again finishes. The OpenWhispr
  importer still needs an empty `notes` table, and notes in Trash count; its refusal says
  how many are there.
- `notes.participants` keeps its shape, a JSON array of `{displayName, email}` read
  leniently, and needed no migration (KNEG-362). `notes_set_participants` writes it
  whole, and keeps what it didn't make: an entry sent back as the screen read it is
  written back as stored, with every key and value an import put on it (its keys in
  alphabetical order); an address already on the note passes unchecked; and entries the
  screen can't show -- not objects, or with no text email or name -- stay, after the
  people. Only new entries are written as `{displayName?, email?}`. What is lost: the
  order of keys within a kept entry, the place of those unshown entries in the list, and
  a value that isn't a JSON array at all (malformed JSON, an object, a string), which
  shows as no people and is replaced by the first edit. An entry the person removes goes
  whole. Emails and names are trimmed of what Rust's `str::trim` strips, in SQL and on
  the screen alike. `people_list` and `notes_list`'s `person` (an email) read it; the
  local API, CLI and MCP return it as before and have no people filter.
- No user-editable transcript API was added. Existing note exports and separate summary
  exports retain their behavior.

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


## Lightweight desktop library (KNEG-420)

Desktop Notes and recent Meetings read `notes_list_summaries`: title, dates,
folder, type, people, duration, Trash state and a stored-transcript flag. Bodies,
summaries and transcript/word JSON are read through `notes_get` only for the
selected note. Existing `notes_list`, HTTP and MCP full-note contracts remain
unchanged. Lightweight lists break equal update-time ties by descending id.

Title/body autosave acknowledges the committed write through `notes_get_summary`
and updates the library entry without rereading folders or scanning Trash files.
A note with email participants also refreshes People because updating it may
change the newest display name chosen for an email. Person-filter entries are
patched from the same metadata, with delayed reads guarded against older results.
Structural changes and participant edits still refresh authoritative backend data.
A metadata-refresh failure is reported separately from a failed save.

Search projects timestamps from SQLite without returning transcript text or word
arrays, validates playable segment field shapes, and computes the selected start
on the same clock as Transcript rows. It still uses the existing FTS ranking and
read transaction. No migration or new index is required.

## External Trash changes (KNEG-424)

The selected note checks its metadata every two seconds while the window is
visible, and on focus. It does not reload the editor, full bodies or library.
If the note is moved to Trash or permanently deleted elsewhere, its mounted
draft stays here and editing stops. **Copy draft** includes typed notes and the
current summary draft. Restore keeps editing paused until **Save retained draft**
is chosen. **Discard unsaved draft** permits navigation without silently replacing
retained text. Read failures are not treated as proof that the note was deleted.
Trash duration falls back to the saved meeting duration when audio was removed.
