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
each reload. It numbers eleven checks (3 is measured in two halves):

1. At 1000x720 the microphone, route, Dictation language, Meetings provider and
   Meetings language menus each open inside the window and above the control
   bar.
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
