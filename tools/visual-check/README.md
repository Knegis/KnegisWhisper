# Visual checks

This directory is a dependency-free Node 22 measurement toolkit. It is an
instrument, not a frontend test: it records what a particular renderer and
window show so the orchestrator can distinguish measured evidence from a claim
that is merely believed from source.

## Shell rail

Start the Vite server in `apps/desktop`, then run:

```powershell
node tools/visual-check/shell-rail.mjs
```

It needs Node 22, Microsoft Edge, and the Vite dev server at the loopback URL
used by the application. `--edge`, `--url`, `--port`, `--out`, and
`--keep-fixture-banner` are available; `--help` prints the full usage. The
tool measures eleven shell claims (KNEG-363, KNEG-382, KNEG-390):

1. `nav.rail` is 68px wide and exactly one `.rail-item[aria-current=page]`
   exists.
2. The rail's Settings shows `.list-panel`, listing Providers and a Developer
   label.
3. Choosing Providers leaves exactly one panel item with `aria-current`, and
   the rail still marks Settings.
4. The title bar's toggle hides `.list-panel`, and a second press restores it.
5. After a load at 720x520, `.list-panel` starts hidden and
   `document.scrollingElement.scrollWidth <= innerWidth`.
6. Tab reaches every rail entry and the search field, each with a 2px outline.
7. Ctrl+K focuses `input[role=combobox]` (and Escape empties it and leaves).
8. On Notes, `.note-detail` is one grid track with the Notes, Transcript and
   Replay tabs at 1000x720; at 1440x900 it is two, with a 340px transcript
   column and the Transcript tab hidden; back at 1000x720 the tab returns.
9. After a load at 720x520 on Notes with the library opened, nothing scrolls
   sideways (the document or `.notes-main`), every tab sits inside
   `.notes-main`, and fixture note 104's long title ellipsises in the panel.
10. At 1440x900 on Notes, Hide transcript beside the tabs puts the note back to
    one track with three tabs and Show transcript brings the 340px column
    back, the button keeping focus and naming the column in `aria-controls`
    only while it is on screen. While hidden, narrowing to 1000x720 and
    widening again moves nothing, and fixture note 104 opens hidden too;
    showing the column with Transcript open selects Notes.
11. From that note, Dictation in the rail and then Notes reopens it, marked in the
    library; after a reload Notes opens its first note again.

"Hidden" is measured as the computed display and width, not the `hidden`
attribute alone. The 720x520 check reloads at that size, because the panel's
starting state is read from the window width once, at start-up.

The report explicitly says: **measured in headless Edge against dev fixtures,
not in the Tauri window**. The ordinary browser dev page invents IPC results.
By default the tool removes its fixed red fixture banner and resets the body
padding it introduced so the artificial fixture-only offset does not contaminate
rail geometry; the `FIXTURES` title prefix remains. Pass
`--keep-fixture-banner` to measure the page exactly as Vite marked it.

Each run writes PNGs, `report.json`, and `report.md` below the selected output
directory. A completed run with any check measured false exits 1 and names the
false checks on stderr (KNEG-389); the report is written either way, and it is
still the evidence a person judges. A failure to launch Edge, reach Vite, or
evaluate the page also exits nonzero.

## Chip menus

```powershell
node tools/visual-check/chip-menus.mjs --url http://127.0.0.1:5191/
```

Same options as the shell rail (`--edge`, `--url`, `--port`, `--out`; the
banner is always removed). It measures what KNEG-385 made the task pages'
control-bar chips do: change a setting in place. Run it against the fixtures
only (`npx vite` in `apps/desktop`, any port but the running app's): it clicks
every write, and the fixtures keep them in memory. States the fixture defaults
cannot reach -- a slow write, a refused one, an unconfigured Tidy up, a route
with a caveat -- are made by wrapping the page's `invoke` for that one command
after the shell loads (`installInvokeStub` in `snippets.mjs`), and again after
each reload. It numbers twelve checks (3 is measured in two halves):

1. At 1000x720 the microphone, route, Dictation language, Meetings provider and
   Meetings language menus each open inside the window and above the control
   bar. The Dictation language panel is opened on the fixtures' OpenAI route,
   since its chip shows on cloud routes too (KNEG-370).
2. On the Dictation route menu: ArrowDown reaches a greyed row and Enter there
   does nothing; ArrowUp to a normal row and Enter chooses it; Escape closes
   with focus back on the chip; Tab closes; `aria-expanded` is right and
   `aria-controls` is present only while open.
3. Choosing updates the chip and the privacy line. 3a, Dictation: a cloud
   provider says "Audio is sent to X", a local model "Audio stays on this
   computer". 3b, Meetings: the provider's line changes and names it; the
   language menu greys "Detect language" for a provider that cannot, and the
   keyboard cannot choose it.
4. With `local_server_start` stubbed slow, the route chip says "Starting
   <model>…", cannot be opened, and Start is held.
5. During a dictation (the fixtures' dev-console state) the route, language and
   Tidy up chips are held with "Can't change during a dictation", and Stop is
   enabled; all are live again when it ends.
6. Tidy up and Summary: toggles that flip `aria-pressed` when set up, links when
   not (stubbed for Tidy up, the fixture default for Summary), Tidy up's caveat
   line with its link (stubbed route), and a refused write (stubbed) on the
   error line with the chip unchanged.
7. Meetings: with a saved provider that cannot be used, the hero's "Choose a
   provider" opens the provider menu without leaving the page, and Start is
   held while `meeting_set_provider` (stubbed slow) is in flight.
8. At 720x520 nothing scrolls sideways on Dictation or Meetings, and a menu
   still opens inside the window.
9. An open menu on each page in light and dark (`prefers-color-scheme`
   emulated), with screenshots.
10. Meetings' "Record this call" (the call card the fixtures' detected Zoom call
    puts on the page) and Start recording each send `meeting_start` the
    `systemAudio` the "Other participants" chip shows (KNEG-412): true with
    system audio available and the chip pressed, false with it released, and
    false where it is unsupported or has no device (`meeting_status` stubbed),
    with the chip disabled and unpressed and the note "Other participants can't
    be recorded on this computer." Each press is on a fresh Meetings page, once
    the button is enabled.
11. Dictation's language chip on the fixtures' OpenAI route (KNEG-370): Detect
    automatically, then Swedish, then English, each written once and followed
    by a re-read of the route, with the panel's note reading "OpenAI detects
    the language itself.", "OpenAI is told to expect Swedish." and "With two,
    OpenAI detects the language itself." in turn. Choosing Parakeet Unified
    removes the chip, and choosing Whisper Large v3 Turbo (Q5) brings it back
    with Whisper's note. The fixtures send one language and not two; xAI's,
    Mistral's and Corti's own rules are Rust's, tested in `route.rs`.

Like the shell rail, it reports **measured in headless Edge against dev
fixtures, not in the Tauri window**, and a false check makes it exit 1.
Not covered: Meetings' language-required fix (the fixtures cannot make a
provider require a language on a blank value without a conditional stub) and
anything the Tauri window itself does.

## Transcript search

```powershell
node tools/visual-check/transcript-search.mjs --url http://127.0.0.1:5191/
```

Same options as the shell rail. It measures KNEG-406: a search hit in a meeting
transcript opens the note at the row where the words were said, and only
selects it. Fixture note 101's transcript says "two more days" at 1:12 and
"honest" at 1:28, and its notes say "Thursday". A count of `play()` calls is
installed before the page loads, and every check also requires it to read 0
and every `audio` element to be paused. Five checks:

1. At 1000x720 a title-bar hit for "honest" reads "1:28 · …", and choosing it
   opens the Transcript tab with that row selected and in view.
2. In the note already open, a hit for "more days" moves the selection without
   remounting the note, and choosing it again after the row was deselected
   selects it again.
3. Dictation, then Back: the note is open on that row again.
4. At 1440x900 the row is selected in the side column, and the Notes tab stays.
5. The library filter: "honest" reads "1:28 · …" and opens on its row;
   "Thursday", a hit in the notes, reads no time.

Measured in headless Edge against dev fixtures, not in the Tauri window, and a
false check makes it exit 1. The fixtures' search is a substring match standing
in for FTS5: which row Rust names is pinned by `crates/notes-core`'s tests, not
here. The fixtures' recording plays since KNEG-412, but no check here starts
it, so "nothing started" is shown and "a recording already playing kept
playing" is not.

## Note playback

```powershell
node tools/visual-check/note-playback.mjs --url http://127.0.0.1:5191/
```

Same options as the chip menus (the banner is always removed). It measures
KNEG-412: a note's one player keeps playing, from where it was and at its
speed, when the note moves it. The fixtures answer the player's `recording` URL with a two-minute WAV, a soft
beep once a second (`apps/desktop/src/lib/fixtureWav.ts`), for every note that
has a recording (`servePlayableRecordings` in `devFixtures.ts`), and Edge runs
with `--mute-audio`. Each check starts from a fresh load at 1440x900 on Notes,
fixture note 101 open and playing from the transcript column at 1.5x, Play
pressed with a real pointer press (`Input.dispatchMouseEvent`) so the page has
the sticky user activation a click gives it; no autoplay flag is needed. A 50ms
timer in the page samples every `audio` (`startPlaybackSampler` in
`snippets.mjs`). "Playing" is not paused with `readyState >= 3`. Four checks:

1. Notes, Replay, Notes: the player moves from the transcript column to
   Replay's full player and back.
2. On Notes the window goes 1440, 1000, 1440: column, mini player, column.
3. On Replay the window crosses the width both ways: the same `audio` (tagged
   with `data-probe`) stays in the full player, playing, its time never going
   back, and the note really is one track at 1000 and two at 1440.
4. Hide transcript, then Show transcript: column, mini player, column.

After each move in 1, 2 and 4, within 1.5s: exactly one `audio`, in the
expected slot and not hidden, not paused, at 1.5x; the first sample with that
slot playing at or after the position before the move minus 0.25s; under 1s
with nothing playing; never two `audio` elements at once; no player error.

Measured in headless Edge against dev fixtures, not in the Tauri window, whose
recordings Rust streams with range requests; a false check makes it exit 1.
Not measured: the reading position across a move, and a summary source's
interval playback (`playInterval`) across one.

## Word highlight

```powershell
node tools/visual-check/word-highlight.mjs --url http://127.0.0.1:5191/
```

Same options as the chip menus (the banner is always removed). It measures
KNEG-408: a transcript row that local Whisper timed word by word marks the word
being said as its recording plays, and a click on a word goes there. Fixture
note 105, "Word timings", is an import whose row 1 has fourteen words, one every
0.65 s from 0.5 s, row 2 eleven, and row 3 none, as every note saved before
KNEG-408 has none; it plays the fixtures' two-minute WAV, and Edge runs with
`--mute-audio`. The words and times are read from the page's own
`note_transcript_rows`. Each check starts from a fresh load at 1440x900 on Notes
with note 105 opened from the library, its transcript in the side column, and
plays a row with the row's own Play, pressed with a real pointer press. Four
checks:

1. Row 1 playing, three samples 1.2s apart: one span per word, exactly one word
   lit in each, the last to have started at the recording's time (or 0.1s
   before it, the frame between the page's step and the read), later in each
   sample.
2. A real press on a later word, "nästa" at 8.3s: the seek ends within 0.05s of
   its start (`seeked`), that word is lit 300ms later, the recording still
   plays, and the row is not selected by the press.
3. Row 3, without words, playing: its text is drawn whole, no row draws a word
   span, and nothing is lit.
4. Reduced motion: a word span's computed `transition-duration` is not 0s
   without the preference (the control) and 0s with `prefers-reduced-motion:
   reduce` emulated, under which two samples still light the expected word,
   the second later than the first.

Measured in headless Edge against dev fixtures, not in the Tauri window; a false
check makes it exit 1. The fixtures' words are invented, evenly spaced; which
rows Rust gives words, and that a row whose text its words no longer spell gets
none, is covered by `cargo test` (`transcript.rs`, `local.rs`, `speech.rs`,
`media.rs`, `meeting/notes.rs`), and that the pinned server sends word times at
all by the ignored `crates/transcription-core/tests/word_timings.rs`. Not
measured: how far the mark is from the sound on a real recording, and scrolling
or CPU on an hour-long note.

## Note meta

```powershell
node tools/visual-check/note-meta.mjs --url http://127.0.0.1:5191/
```

Same options as the chip menus (the banner is always removed). It measures
KNEG-368: the line a note's meta row gives where its transcript was made. The
fixtures store it as Rust does (`Note.transcribedBy`): note 101 is a meeting
transcribed on this computer, 104 an import transcribed by OpenAI, and 102 has
no record, as every note saved before KNEG-368 has none. Their `meeting_stop`
saves the record Rust's `transcribed_by` makes: the whole-recording pass, on
this computer, naming the live provider when its audio leaves the computer.
Each check starts from a fresh load at 1000x720; notes are opened from the
library by title. Four checks:

1. Note 101: exactly one meta item starting "Transcribed", reading
   "Transcribed on this computer", drawn with the `computer` icon, inside the
   row and the window, beside the date, length and people.
2. Note 104: "Transcribed by OpenAI", the same way.
3. Note 102: no such item, and the row still has its date, length and
   people.
4. A meeting started with the fixtures' default provider (Deepgram) and
   stopped: its note reads "Transcribed on this computer · live captions by
   Deepgram", inside the row and the window at 1000x720 and again at 720x520,
   with nothing scrolling sideways.

Measured in headless Edge against dev fixtures, not in the Tauri window; a
false check makes it exit 1. Which route Rust names, when a meeting's saved
text is its live text or the whole-recording pass, and that imports, recovery
and typed-notes-only notes record what they should, is covered by
`cargo test` (`transcribed_by.rs`, `meeting/notes.rs`, `meeting.rs`,
`media.rs`), not here. Not measured: a real meeting or import saving it.

## People

```powershell
node tools/visual-check/people.mjs --url http://127.0.0.1:5191/
```

Same options as the chip menus (the banner is always removed). It measures
KNEG-362: people added to a note on its meta row, and the library's People
list. The fixtures answer `people_list`, `notes_set_participants` and
`notes_list`'s `person` as Rust's `notes_core::people` does, refusals worded
the same; `invoke` is wrapped to log what reached them. Text is typed with
`Input.insertText` into the focused field and buttons pressed with real
pointer presses, at 1000x720 unless stated. Checks 1-4 are one sequence from
one fresh load; 5 and 6 each start from their own. Six checks:

1. Note 103 ("Scratch", nobody on it): Add person, an email and a name, Add.
   `notes_set_participants` gets that one person; the note shows one chip
   with "Remove {name}", the form closes and focus returns to Add person; the
   People list adds them with 1, in name order among the fixtures' three.
2. Pressing them in People asks `notes_list` for their email alone, and shows
   "Notes with {name}" holding only "Scratch" and Show all notes, with no
   folders; the person reads pressed.
3. Show all notes: the Folders, People and Not in a folder headings back, the
   folders and their notes exactly as before, nobody pressed, the same note
   open.
4. Remove {name}: `notes_set_participants` gets an empty list, the stored
   participants are null, the chip and the People row go, focus is on Add
   person.
5. "maja@": the row says "'maja@' isn't an email address." as an alert, the
   form stays open with "maja@" in it, and nothing is stored.
6. Note 101 (two people) with the form open at 720x520: two chips, every meta
   item and the people item inside the window, nothing scrolling sideways.

Measured in headless Edge against dev fixtures, not in the Tauri window; a
false check makes it exit 1. Which addresses Rust accepts, how it counts and
names people, that it finds a person's notes in any case, that malformed
participants are no people, and that the summary is told names and no
addresses are covered by `cargo test` (`people.rs`, `commands.rs`,
`summaries.rs`), not here.

## Meeting discard

```powershell
node tools/visual-check/meeting-discard.mjs --url http://127.0.0.1:5191/
```

Same options as the chip menus (the banner is always removed). It measures
KNEG-418: what Meetings does when the meeting window doesn't answer Stop's or
Discard's request to save its notes. Both send `meeting-notes-flush` to the
`meeting` window (`flushMeetingPanelNotes` in `lib/ipc.ts`) and wait up to 5s
for its answer. The fixtures now answer for that window: their
`plugin:event|emit_to` handler replies `meeting-notes-flushed` with the same
request id, unless `kw.meetingWindowSilent(true)` was called (the
`meetingWindowSilent` snippet). Before this, every Stop and Discard in the
fixtures ran out the 5s and stopped there. `window.confirm` is replaced with a
scripted answer that is logged in order with the page's calls
(`scriptConfirm`, `installInvokeStub`). Each check starts from a fresh load at
1000x720 with a meeting started from Start. Six checks:

1. Meeting window silent: after Stop and save has waited at least 5s, the
   caveat is in view with Stop recording anyway and Discard anyway beside
   Discard and Stop and save. One `emit_to` was sent, and nothing was stopped.
2. Discard anyway, answered no: the question ends "your latest edit may be
   missing". The status is read and the question asked; nothing is cancelled
   or sent to the meeting window, and the meeting is still recording.
3. Answered yes: the same three steps, then exactly one `meeting_cancel`, with
   no new `emit_to`. The page goes idle with "Discarded. The recording was
   deleted." A meeting started next shows only Discard and Stop and save.
4. As 3, with `meeting_status` stubbed to 600s in and switched to 1s in while
   the question is open: another meeting, so nothing is cancelled.
5. Meeting window answering: Stop and save sends one `emit_to`, then
   `meeting_stop`, and the page goes idle saying it saved, well inside the 5s.
6. Meeting window answering: Discard asks its own question, reads the status
   again, sends one `emit_to`, then cancels.

Measured in headless Edge against dev fixtures, not in the Tauri window; a
false check makes it exit 1. The fixtures' `meeting_cancel` sends the cancelled
outcome Rust's `cancel` sends, with no note kept, since this tab has no meeting
window to type in. It doesn't send the `idle` state event: no other fixture
meeting command sends a state, and an `idle` heard once would outrank every
later status read. Not measured: a real meeting window hanging, and which notes
Rust keeps on cancel (`meeting.rs`'s `cancel`, `notes.take()`).

## Meeting plan

```powershell
node tools/visual-check/meeting-plan.mjs --url http://127.0.0.1:5191/
```

Same options as the chip menus (the banner is always removed). It measures
KNEG-365: naming a meeting and choosing its folder before recording. The name
is typed with CDP's `Input.insertText` into the focused field, and the folder
chosen by pressing the Folder chip and its row. The page's `invoke` is wrapped
(`installInvokeStub`) to log what reached `meeting_start`. The fixtures keep
the plan on `meeting_status` while the meeting runs, as Rust keeps it with the
session. Their `meeting_stop` saves a new meeting note as Rust's `keep` does
and announces the outcome as Rust's `stop` does, which is what makes the page
read Recent again. Each check starts from a fresh load at 1000x720. Five
checks:

1. With "Board review" typed and Internal (fixture folder 3) chosen, the
   Folder chip reads "Save to Internal". One `meeting_start` carries that
   title and `folderId` 3, from Start recording and, on a fresh page, from
   Record this call. The field's placeholder and accessible name are read too.
2. While it records, the live view's title and the Now row read the name.
3. Stop and save: Recent's first row is the name, the field is empty again,
   and the chip still names the folder. In Notes, the Internal folder lists
   the note.
4. Folder 3 deleted through the fixtures while the meeting records, then Stop:
   the caveat says "The folder you chose was deleted, so the note isn't in a
   folder.", the chip is back at "Save to Notes", and Notes lists the note
   under "Not in a folder".
5. No name and no folder: `meeting_start` gets `null` for both, the live title
   is "Meeting", and the note is named "Meeting — " and when it started, in no
   folder.

Measured in headless Edge against dev fixtures, not in the Tauri window; a
false check makes it exit 1. Rust's own side -- the plan beside the
recording, recovery's name and folder, and a folder deleted between Rust's
lookup and its insert -- is covered by `cargo test`, not here. Not measured:
the notification window's Record, which starts an unnamed meeting.

## Meeting recordings

```
node tools/visual-check/meeting-recordings.mjs --url http://127.0.0.1:5191/
```

Same options as the chip menus (the banner is always removed). It measures
KNEG-409: deleting each meeting recording once its transcript is saved. The
checkbox and the buttons are pressed with real CDP pointer presses, and the
page's `invoke` is wrapped (`installInvokeStub`) to log what reached the
setting's writer. The setting is turned on, where a check needs it, through
the fixtures' `meeting_set_delete_recordings` before the page reads it. The
fixtures read it into the plan at `meeting_start`, as Rust does, and their
`meeting_stop` saves a note with two transcript rows: with a recording attached
when the plan keeps it, and with none, its details saying why, when the plan
deletes it. Each check starts from a fresh load, at 1440x900 unless it says
otherwise. Five checks:

1. Settings > Meetings has a Recordings section with "Delete each recording
   once its transcript is saved", unticked, its caveat and its Learn more.
   One press calls `meeting_set_delete_recordings` once, with `enabled: true`,
   and ticks it.
2. With it off, the Meetings page has no note about it and its list's foot
   says recordings are saved on this computer. With it on: "Recordings are
   deleted once each meeting is transcribed." beside Start and "Recordings are
   deleted after transcribing." at the foot. At 720x520, with the note shown,
   nothing scrolls sideways, measured as chip-menus check 8 measures it.
3. A meeting started with it on has "Recording deleted after saving" in its
   live sub-line, and `meeting_start` is sent no delete choice. Turned off
   while the meeting records, and the page remounted, the sub-line still
   says it. A meeting started with it off does not.
4. Stop and save: "Saved as a note (31 min). The recording was deleted; the
   transcript is in the note." Its Learn more is "Note N." and no path.
5. The saved note, opened from Recent, on Replay: no `audio`, no
   `.recording-player`, no `.transcript-play`, its two transcript rows, and
   the recording details "The recording was deleted after transcribing
   (Settings › Meetings)." The same steps with it off find a player and two
   row Play buttons, so the probe sees them where they are.

Measured in headless Edge against dev fixtures, not in the Tauri window; a
false check makes it exit 1. Rust's side -- the attachment and the file
removed in one transaction, what is kept when the transcript may be
incomplete or the file can't be removed, and recovery following the plan --
is covered by `cargo test`, not here. Not measured: a real meeting with it on,
whose transcript is complete and whose file is gone.

## Level meters

```
node tools/visual-check/level-meters.mjs --url http://127.0.0.1:5191/
```

Same options as the chip menus (the banner is always removed). It measures
KNEG-371: the live sound-level meters. Levels reach the page as `audio-level`
events, as Rust's publishers send them (`src-tauri/src/audio_level.rs`). The
fixtures' dev console sends one reading (`kw.level(0.75)`) or ten a second
(`kw.levels()`, stopped with a last 0), and the fixtures' `meeting_start` sends
ten a second for each capture that opened, stopped at Stop or Discard.
Dictation's recording is announced with `kw.dictationState`. The main window is
1000x720; the overlay is measured at its own window's sizes. A reading is read
150ms after the meter shows it, once the 80ms fill swap has finished. Six
checks:

1. While a dictation records, the Dictation hero has exactly one meter, 16
   segments, hidden from assistive technology with no `role`, showing 0.
   After one reading of 0.75, it shows 0.75 with 12 segments lit, drawn in a
   colour other than the unlit ones. At idle it is gone. Loaded at 720x520
   and recording, the meter is inside the window and nothing scrolls sideways.
2. With no reading after that 0.75 for 650ms, the same meter shows 0 with
   none lit, still mounted: staleness at 400ms.
3. A meeting started with Other participants pressed has "You" and "Others"
   meters in its live header, both above 0 and lit differently. After Stop and
   save, none. Started with the chip released, one, "You". Loaded at 720x520
   with both, both meters are inside the window and nothing scrolls sideways,
   measured as chip-menus check 8 measures the idle pages.
4. The Dictation meter for 2s at ten readings a second, each way. Without
   emulation the segments' transition is 0.08s and at least 12 changes show.
   With `prefers-reduced-motion: reduce` emulated it is 0s, with at most 10
   changes and none closer than 240ms to the one before. A `MutationObserver`
   in the page timestamps the changes.
5. `#overlay` at 280x96 (`overlay.rs`), label mode: the 8-segment meter right
   of "Recording", inside the window, 6 of 8 lit at 0.75, gone at idle.
6. `#overlay` at 96x96 (`persistent_overlay.rs`' `SIZE`) as the floating
   microphone, horizontal and vertical: a 6-segment strip inside the button
   and the window, 5 of 6 lit at 0.75, gone at idle.

Measured in headless Edge against dev fixtures, not in the Tauri window; a
false check makes it exit 1. Rust's side is covered by `cargo test`: the
meter's arithmetic and max-hold (`kw_audio_core::level`), `Buffer::push`
feeding it, and the publisher's last 0 and its pacing. The dictation and
meeting readers ending with their captures are tested too, but those tests
open the real microphone, so they are `#[ignore]`d and run by hand with
`cargo test -p knegiswhisper --lib -- --ignored`. Not measured: whether a
real microphone or real system audio moves a meter, how the −60..0 dBFS range
feels, and what the meters cost in CPU. No audio reaches a browser tab, so
those are by hand.

## Meeting microphone

```
node tools/visual-check/meeting-microphone.mjs --url http://127.0.0.1:5191/
```

Same options as the chip menus (the banner is always removed). It measures
KNEG-364: choosing the microphone on the Meetings page, which one a live
meeting names, and "Test microphone". Chips, rows and buttons are pressed with
real pointer presses; the page's `invoke` is wrapped (`installInvokeStub`) to
log `dictation_set_device` and, for one failure, to refuse the check. The
fixtures' `microphone_check` sends levels on the `microphone-check` session
for 5 s and answers as Rust would, refusing while `kw.dictationState` says a
dictation records; their `meeting_start` names the microphone it opened. Each
check starts from a fresh load on Meetings at 1000x720. Eight checks:

1. The chip, named "Microphone: System default", opens a listbox above the
   bar with one heading, "Used by meetings and dictation", over the four
   rows, and the footer "Test microphone"; its title says choosing here
   changes dictation's microphone too.
2. Choosing the headset calls `dictation_set_device` once with it, the chip
   names it, and Dictation's Microphone chip does too.
3. A meeting started with the headset chosen names it on its live chip, a
   static span. With "Line In" chosen on Dictation while it runs, and Meetings
   entered again, the live chip still names the headset.
4. Test microphone: a `role=status` region reading "Listening for 5 seconds —
   say something. Nothing is recorded." with a 12-segment meter, hidden from
   assistive technology and above 0; Start and Record this call disabled; the
   menu without its footer. Four to seven seconds later the region reads
   "Heard you on Microphone Array (Realtek(R) Audio).", the meter is gone and
   both buttons are enabled again.
5. With "USB Mic" stored (not a fixture device), the chip reads "USB Mic (not
   connected)" with its note in view, and the check ends "USB Mic isn't
   connected, so this tested Microphone Array (Realtek(R) Audio). Heard you
   on …".
6. While a dictation records, the check's refusal is one alert line in Rust's
   own words ("Not now: …") with no Details; with the check refused by the
   stub, "The microphone couldn't be tested." with the error under Details.
   Neither holds Start.
7. At 720x520, listening: the meter is inside the window and nothing scrolls
   sideways, as chip-menus check 8 measures.
8. With `heard: false` laid over the fixtures' answer (the stub's merge), the
   region reads "Heard nothing from … Check it isn't muted, or choose another
   microphone." as a caveat.

Measured in headless Edge against dev fixtures, not in the Tauri window; a
false check makes it exit 1. Rust's side is covered by `cargo test`
(`microphone_check.rs`): the refusals, one check at a time, what counts as
heard from a series of levels, and which microphone stood in for the one
chosen. Its whole check on the real default microphone is an `#[ignore]`d
test, run by hand. Not measured: real devices, whether opening a Bluetooth
headset switches it to its call profile, and the system's microphone
indicator during the five seconds.

## Local models

```
node tools/visual-check/local-models.mjs --url http://127.0.0.1:5191/
```

Same options as the chip menus (the banner is always removed). It measures
KNEG-330: Settings > Transcription lists the Whisper and Parakeet models as
one list. Row buttons are pressed with real pointer presses; the page's
`invoke` is wrapped (`installInvokeStub`) to log what the buttons call and,
once, to hold `local_model_install` for 4 s. The fixtures keep the dictation
choice (`sherpa_use_model`, `local_server_start`) and the meetings choice
(`sherpa_use_stream_model`) in memory as Rust keeps them in settings, and
list five Whisper models and four sherpa models with the registry's
languages. Each check starts from a fresh load on Settings > Transcription at
1000x720. Eight checks:

1. One list in the local section, a row for each of the nine models, headed
   only "Local transcription" and "Engines". Every Details is closed and no
   model id is in the text shown; each id is under a row's Details.
2. The rows in this order: Whisper Small, Whisper Large v3 Turbo, Parakeet
   Unified EN, Parakeet TDT, KB Whisper Large, Whisper Medium, Whisper Base
   (English, size unknown), then Nemotron Speech Streaming EN and Nemotron
   3.5: dictation before meetings, each smallest first.
3. Use for meetings on Nemotron 3.5 sends exactly one
   `sherpa_use_stream_model` and no `sherpa_use_model`, `dictation_use_local`
   or `local_server_start`. Nemotron 3.5 reads "In use for meetings", Nemotron
   EN "Use for meetings", and "Dictation goes to" is unchanged.
4. Use for dictation on Parakeet Unified sends `sherpa_use_model` then
   `dictation_use_local`; it reads "In use for dictation", Whisper Small (the
   model the server has loaded) says "Loaded, but dictation goes to Parakeet
   Unified EN 0.6B", and "Dictation goes to" reads "This computer". Then on
   Whisper Large v3 Turbo: `local_server_start` then `dictation_use_local`,
   Turbo in use, Parakeet Unified not, Whisper Small's warning gone. Nemotron
   EN stays "In use for meetings" throughout.
5. No install call on load. Install on Whisper Medium, with the install held,
   sends one `local_model_install` and no `sherpa_install_model`; "Starting…"
   is on Whisper Medium's row and no other, and a progress event for it
   (`kw.modelProgress`) reads "Downloading Whisper Medium (Q5) — …" there.
6. "Show models for" offers "Any language" (chosen) and the eight dictation
   languages; Swedish keeps Whisper Small, Turbo, Parakeet TDT, KB Whisper
   Large and Whisper Medium.
7. Under Engines: the Parakeet engine's caveat "This engine can't be updated
   from here yet. …" with "Why, and which folder", both engine rows, and
   "About the two engines".
8. Loaded at 720x520, nothing scrolls sideways (the document, `.split-main`
   and the section), with every Details in the section closed and then open.

Measured in headless Edge against dev fixtures, not in the Tauri window; a
false check makes it exit 1. Which model a row reads as in use is
`lib/localModels.ts`'s rule, pinned by `tests/localModels.test.ts`, and each
Whisper model's languages are derived from its id, held to the registry
there. Not measured: a real install, and Rust's refusals of a model that
can't be chosen.

## Running application windows

Launch the running development application with its WebView2 debugger exposed:

```powershell
cd apps/desktop
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9222'
npm run tauri:dev
```

Then run one command, for example:

```powershell
node tools/visual-check/app-windows.mjs run-all
```

The subcommands are `status`, `start`, `watch`, `theme`, `stop`, and `run-all`.
The normal route drives the Meetings UI; `--via invoke` is an explicit fallback.
`start` waits up to 180 seconds by default because the local streaming sidecar
can take time to load. `watch` samples the notification DOM every 500ms and its
Win32 visibility every two seconds, recording transitions and the first five
caption changes. `theme` captures light and dark, while `stop` records finishing,
window hiding, and the still-existing hidden meeting target.

This WebView2 remote-debugging route was **measured on 2026-09-07**: with the
variable set before `npm run tauri:dev`, all four webviews (`main`, `#overlay`,
`#meeting`, `#update`) appear on the port and `__TAURI_INTERNALS__.invoke` works
from `Runtime.evaluate`. If the loopback port does not answer, the tool repeats
the environment-variable launch command rather than treating an empty target
list as absence of application windows. Its output directory also
contains append-only-in-practice `report.json` and `report.md` records for the
chosen command sequence.
