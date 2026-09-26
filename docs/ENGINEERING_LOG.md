# Engineering log — judgement calls, deviations, and carry-forward

A record of choices made in the moment and why: what was decided, what the alternative was, and
why the alternative was rejected. The reasoning is the point, not the outcome.

This file is **committed**, unlike the scratch `DECISIONS.local.md` it grew out of. The reason is
in the first entry below: several of the findings here are latent defects that were invisible by
inspection, and a record of them that lives only on one machine is not a record.

Newest entries first. The carry-forward section immediately below is the exception — it is
maintained in place rather than appended to, because its purpose is to be current.

---

## 2026-09-26 (first, latest) — KNEG-322 to KNEG-325: a day of sidebar and editor asks, and two things the worktree found

A sequence of small owner requests on the notes view, each shipped as it came, and two defects
that were only visible because the work moved into a git worktree.

- **KNEG-322, a note opened a little way down.** Two mechanisms, both measured in headless Edge
  against the fixtures: the pane outlives the note inside it and clamped the previous note's
  offset against the new note's first layout (122px in a 480px window), and an open request from
  the title bar search scrolled the article into view at "start", which parks it flush with the
  pane's edge past the padding (21px). One layout effect now puts the pane at the top when the
  open note or the request changes; the scroll-into-view effect is gone.
- **KNEG-324, tooltips.** Windows' title bubble replaced by one `Tooltips` component in the main
  window: it lifts the `title` out of the attribute while the pointer is on the element, so the
  browser never draws its own, and puts it back afterwards. The arrow is two clipped triangles,
  not a rotated bordered square: at 150% scaling the square's hairlines anti-aliased and its base
  met the box with notches, seen by the owner and then at 8x over the DevTools port. Positions
  are snapped to device pixels. The meeting panel and the overlay keep native tooltips by design.
- **KNEG-325, the sidebar.** Notes drag into folders (the browser's own drag, a private payload
  type the file-drop guard ignores, a "No folder" strip while a filed note is in the air, the same
  move as the right-click menu). Folders come first; their headings are set like note titles and
  lose the count; entries are one line with the creation time as the tooltip; every gap in the
  list is 0.3rem, after a measurement found the folder-to-first-note gap three times that because
  of a default list margin nobody had reset.
- **Marked text in the editor** is Milkdown's control grey (`--crepe-color-hover`), the same for
  `::selection`, a block-selected paragraph and a pressed list row, after two rounds with the
  owner over which of Crepe's colours they meant (the band at 40%, then the grey).
- **KNEG-323, found by the worktree.** The dev app would not start from a fresh worktree: sqlx
  refused the database because migration 6 was recorded with a CRLF checksum. The main checkout's
  working copy of `0006_note_workspace.sql` carries CRLF despite `.gitattributes`, git normalises
  on compare so it reads as clean, and the running app had applied those bytes. Repaired on the
  owner's machine (row updated to the LF checksum, file rewritten); the guard test is still to do.
- **Running the dev app from a worktree** also hit cmd.exe's PATH limit: this machine's PATH is
  6.5k characters of repeated blocks, npm adds a `.bin` entry per ancestor at every layer, and
  `tauri dev` spawns a second npm layer; from four directories deeper that crossed 8191 and the
  nested npm could not find `vite`. Deduplicating PATH for the launch fixed it; the build shares
  the main checkout's target directory.

Nothing above has been dragged with a real pointer; see the bench check.

---

## 2026-09-25 (sixth) — KNEG-321: the notes sidebar groups by folder, and one call had to be added to make folders usable

The owner asked for two icon swaps (new note, new folder), for the folder button to go, and for
expand and collapse buttons beside new folder. The list was flat, so there was nothing for those
two buttons to act on: the folder button had opened a filter (pick one folder, delete it), and
that was the whole of what folders did. Agreed with the owner: the list groups notes under
collapsible folder headings, unfiled notes first, the two buttons open or close every folder, and
the filter goes.

Two things fell out of reading the code:

- **No screen could put a note into a folder.** `folderId` was used by the list filter and
  nowhere else, so every folder was empty by construction. The note's right-click menu now has
  *Move to folder*.
- **No call could take a note out of one.** `notes_update` (and the notes core's `update`) reads
  `folder_id: None` as "leave it alone", which is right for a partial update and leaves "no folder"
  unsayable. `notes::set_folder` and `notes_set_folder` are the one place that can unfile, and
  they leave `updated_at` alone because filing a note is not working on it.

Collapsed state is session state, not a setting: a setting needs a key and a screen under the
settings audit, and which folders were closed an hour ago is not worth that. Selecting a note by
search, by a link from elsewhere, or by moving it opens its folder so the selection is never
hidden. *Delete folder* lives on the heading's right-click menu and says that the notes are kept.
The grouping is a pure function with four `node --test` cases. Not clicked by anyone yet.

---

## 2026-09-25 (fifth) — KNEG-320: `cargo test` killed the running application's Whisper server, twice

The owner saw *Local model server exited … exited with exit code 1 after 90 seconds* on the
Dictation page after the KNEG-318 run. The dev log had two of them, at 20:31:35Z and 21:11:11Z
(the watcher polls every five seconds), each while `cargo test` ran on the main checkout and each
about a minute after `tauri dev` had relaunched the application and it had pre-warmed its server.
Exit code 1 is what `TerminateProcess` leaves. The application's own line — *the sidecar exited on
its own; nothing asked it to stop* — was accurate: the application had not asked.

The test `reaping_with_no_record_does_nothing_and_does_not_fail` in `local_model.rs` called
`reap_stale` on a runtime built by the tests' `local()` helper, and that helper used
`LocalTranscription::new`, whose PID store is the application's own `sidecar-pids` directory. On
a machine where the application is running, that directory names the live server. The reaper
read the record, confirmed the command line through `sysinfo`, and did what it is for. The test's
comment said "the first launch on a new machine", which is the one condition a developer's
machine never satisfies. The sibling native test had already solved this for itself with a
`tempdir` store; the fix moves that into `local()` (one directory per test process, held in a
`OnceLock`) so no test in the module can reach the application's records, and a test pins the
property by comparing the two directories.

Two things worth keeping: **the reaper cannot tell a live server from a stale one, by design**, so
anything that runs it against the production directory from outside the application will kill
the application's server — a second instance, a test, a tool; and a side effect that only shows
on a developer's machine with the application open is invisible to CI by construction, which is
why it survived every green run since the reaper landed.

---

## 2026-09-25 (fourth) — KNEG-318 and KNEG-319: a speaker gets a name, and the voice-recognition spike says "not yet, and not that way"

The owner asked to say who "Speaker 1" is, and then whether the application could recognise the
same voice in later meetings from a setting. The first is built (`19e97f3` Rust, merge
`90a682d`; `bf1566c` frontend, merge `2e6946d`; two Codex workstreams, one
repair round). The second was measured rather than built, and the numbers are in
`SPEAKER_RECOGNITION_SPIKE.md`.

### The rename: the table was waiting, and four readers had an empty map

`speaker_mappings` had sat in the schema since the first migration with a documented
"candidate owner" and no writer. `transcript::speaker_name` already preferred a mapping over the
placeholder. So the work was a `speakers` module with three functions, a raw `speakerId` on every
transcript row so a screen can address the rename, and the same map threaded through the four
places that had been passing an empty one: playback rows, the five export formats in the desktop
app *and* in the local API, and the summary prompt with its source-linked rows. Decided on the
way:

- **A rename is a note edit and lives beside the note**, one row per raw label, never a rewrite
  of the segment JSON. An empty name deletes the row, so "Remove name" and "back to Speaker 1"
  are the same operation.
- **Suggestions are the note's calendar participants first, then names used on other notes**,
  and never a name already on this note. No voice matching sits behind them; a suggestion is a
  spelling, not an identity.
- **`speakerId` is absent when a typed name already wins.** Found in review: round one set it for
  every labelled segment, and an imported segment with a typed name would have offered a rename
  that changed nothing. The doc comment was right and the code was made to match it.
- **The summary prompt now reads `Speaker 1` for an unnamed diarised speaker** where it used to
  read `speaker_0`, because it resolves through the same function the screen uses. A summary
  that already exists is not regenerated by a rename; the next Generate uses the names.
- **The label is the control.** In the Transcript tab the speaker label of a diarised row is a
  button that opens one form above the list; capture-source rows ("You", "Others") keep a plain
  span because they are not people.

### The spike: route 2 is out on one fact, route 1 needs labelled voices

Prepending a reference clip to a recording changed that recording's own cluster count in ten of
twelve runs, so the "let the diarisation CLI do the matching" route would make every meeting's
speaker turns depend on which voices are enrolled. Out. Embeddings from the CAM++ model the
application already ships separate voices cleanly on diarisation turns (not on transcription
segments, which was the first pass's own confound) and carry a voice across recordings made the
same evening at 0.89–0.93 against unrelated voices at 0.81 or below — but the five single-voice
recordings score like strangers against each other, and nobody has labelled who speaks in any of
them. The threshold and the accuracy claim need a labelled set before a settings surface is
built on them; that is the next step on KNEG-319 and it belongs to a person. The in-process route
needs no new download: the archive already pinned for the `sherpa` sidecar carries
`lib/sherpa-onnx-c-api.dll` with the speaker-embedding functions, and the installer only extracts
`bin/` today.

---

## 2026-09-25 (third) — KNEG-317: the provider select remembered nothing, and the offer recorded with something else

The owner reported that Settings > Meetings shows OpenAI Realtime selected on every start, whatever
was picked before. The cause was one line: the select's value was `chosen ?? usable[0]?.id`, React
state seeded from nothing, so the screen "set the meeting provider" (as `ui-surfaces.json` says) for
exactly as long as the component lived. The same reading found the other half: the notification
window's Record button started every offered meeting with a hard-coded `"local"`, so a person who
had picked a cloud provider on the screen and pressed Record got a different one, and nothing on
either surface said so.

### What was decided

- **One key, `meetings.provider`, written on change.** Saved when the select changes rather than at
  Start, because the report is about a choice made and then not used — a person can pick the local
  provider, start no meeting that day, and still expect it there tomorrow. The setter refuses an id
  this build has no client for (`config::resolve_client`, the same sentence `start` gives), so the
  key can never hold a provider that cannot be started.
- **The fallback never writes.** A saved provider that cannot be used right now — the local one with
  no model server running is the everyday case — is named on screen with its availability reason,
  and the first usable provider is selected in its place without touching the setting. The
  alternative, saving whatever the select ends up showing, would forget the choice on the first
  start without the model server, which is the report again one day later. The rule is a pure
  function in `lib/meetingProvider.ts` with six `node --test` cases, because the three inputs
  (chosen now, saved earlier, usable at the moment) have an order of authority that a `??` chain in
  a component would not state.
- **The offer uses the saved provider too**, falling back to `local` only before anybody has chosen.
  Not moved into Rust: `meeting_start` keeps its explicit `provider` argument, and the two callers
  ask the same command for the same value.

### Verified, and not

The commands round-trip over mock IPC (`tauri-test`: absent reads as `null`, an unknown id is
refused and leaves the key untouched, `local` reads back), the selection rule is tested, and every
gate passes. Not measured: the select showing the saved provider after a restart of the built
application, and `MeetingStatus.provider` naming the saved one after Record on an offer. Both are in
`BENCH_CHECK.md` § 2026-09-25 (third).

---

## 2026-09-25 (later) — KNEG-298: the meeting box becomes a notes panel, and the focus rule turned out to be tao's, not ours

The owner asked, with a Granola screenshot, for the meeting window to be a note surface during a
meeting: the Notes section's Milkdown editor, and a toggle between *My notes* and the live
*Transcript*. Four decisions were taken with the owner before any code: notes go into the meeting's
own note; the box opens as the panel when recording starts; two views only; the window may take
focus like a normal window while recording. Built as two sequential Codex workstreams — Rust
foundation `3a0d0c4` (merge `9e5bd07`), frontend `81d7164` (merge `23fac71`).

### Three facts that decided the design

- **No note exists while a meeting records.** `keep` creates it after the whole-file pass with an
  always-empty body, and nothing is persisted live. So typed notes are held by a `NotesDraft`
  managed state, mirrored to `<recording>.notes.md` beside the WAV after every autosave, written
  only by a command the meeting window may call, and consumed by `keep` as `NewNote.content`. A
  note is now created when there are segments *or* notes; cancel keeps non-empty notes as a
  notes-only note and `MeetingOutcome::Cancelled` carries the id. Nothing recovers an orphaned WAV
  or sidecar after a crash — that predates this work for the WAV and is filed as KNEG-316.
- **The raw non-activation treatment never survived a show on its own.** Read in tao 0.35.3
  (`platform_impl/windows/window_state.rs`): `apply_diff` regenerates `GWL_STYLE`/`GWL_EXSTYLE`
  from tao's own flags on every flag change, show and hide included, with `WS_EX_NOACTIVATE`
  present iff `FOCUSABLE` is clear. The `WS_EX_NOACTIVATE` that `set_non_activating` wrote was
  dropped by the next show and came back only because `status()` re-applied the full treatments on
  every call. And the builder's `focused(false)` sets `MARKER_DONT_FOCUS`, which the stored flags
  keep forever (the clear happens on a by-value copy), so every show of this window is
  `SW_SHOWNOACTIVATE` — including the show `set_focusable(true)` triggers on a visible window. So
  the durable form of non-activation is the builder's `focusable(false)`, activation is one
  `set_focusable(true)` granted in `start` after the show, and it is revoked in `stop`/`cancel`
  after the hide, never on a visible window. Finishing stays activatable for that reason. The
  overlay already used `.focusable(false)` with a comment that named the symptom without the
  mechanism; the mechanism is now written down.
- **Milkdown's block handle needs the hosting window's Tauri drag-and-drop handler off** — the
  KNEG-258 lesson again, this time for a Rust-built window (`.disable_drag_drop_handler()`), which
  the capability audit's rule G could not see. Rule G now reads Rust builder chains and a declared
  list of editor-hosting labels; rule H keeps that list attached to reality. The panel root stays
  the drag region and the editor, transcript and tab bar opt out with `data-tauri-drag-region="false"`.

### Found in review

- The activation-mismatch warning treated the `Starting` phase as running, so every meeting start
  would have logged a spurious warning while the Meetings screen polled status. One-line repair.
- The frontend worker stopped with `wrong_premise` because one sentence of the brief's docs
  section said "non-activating outside recording" while the Rust keeps the window activatable
  through the transcribing strip. The worker was right and the wording was the orchestrator's.
- Accepted with a note: `MeetingPanel.tsx` imports `clock` from `MeetingNotification.tsx`, which
  imports the panel — a module cycle that works because the function is only called at render
  time. `clock` belongs in `lib/`; a nit for the next pass.

### Measured, and not

From the owner's running dev instance after the Rust merge (`EnumWindows` by pid,
`GetWindowLongW`, `GetWindowDisplayAffinity`): the idle meeting window reads `WS_THICKFRAME` set,
`WS_MAXIMIZEBOX` clear, `WS_EX_NOACTIVATE` set, affinity `0x11`. That is the builder half of the
rule. **Not measured:** the recording half — `set_focusable(true)` clearing the style after the show
without activating, the panel opening at 440×560, typing, the transcript tab, block-handle drag —
all of which need a meeting to be started and a person at the keyboard. The by-hand list is in
`BENCH_CHECK.md` § 2026-09-25 (later). The `cargo build --workspace` gate reads exit 101 while the
owner's instance runs because cargo cannot replace `knegiswhisper-desktop.exe`; the workspace built
with the binary excluded and the desktop crate built as a library, and the running instance is
itself the post-merge build.

---

## 2026-09-25 — KNEG-315: the meeting box becomes movable, resizable and themed, and two of the five findings were the orchestrator's own

The owner asked for the meeting notification to be movable and resizable, to stay out of screen
shares, and — mid-run — to match the application's colours and look. The first two were built by
one Codex worker in two rounds (`2805b17`), the fourth by a second sequential worker (`2948902`),
both merged `--no-ff` (`1946e4a`, `c90c953`). The third needed nothing: `apply_treatments` already
sets `WDA_EXCLUDEFROMCAPTURE` and KNEG-60 measured it effective on 2026-09-07.

### Choices, and the alternatives they beat

- **Resize is the OS's, not ours.** tao 0.35's `WM_NCHITTEST` handler hit-tests the edges of any
  undecorated window that is `resizable` and not maximized (`event_loop.rs:2178`), so the whole
  feature on the Rust side is `.resizable(true)`, `.min_inner_size(392, 92)` and
  `.maximizable(false)`. The alternative — a pointer-driven resize through a command, in the shape
  of `persistent_overlay::overlay_drag` — would have needed a grip and could only grow the window
  right and down, which for a bottom-right window is the wrong two directions.
- **Move is Tauri's drag region, not the microphone button's hand-rolled drag.** `overlay_drag`
  exists for docking, orientation and an input region the notification does not have. Tauri 2.11's
  `drag.js` accepts `data-tauri-drag-region="deep"` and refuses to start a drag from a `BUTTON`, so
  one attribute on the panel root covers every state without touching the buttons. **Believed,
  not measured:** that the OS move loop works under `WS_EX_NOACTIVATE`. Nothing in the tree had
  tried `start_dragging` on a non-activating window; the by-hand check is in `BENCH_CHECK.md`.
- **A capability of its own, not a wider `notifications.json`.** `core:window:allow-start-dragging`
  is granted by `capabilities/meeting-window.json` to the `meeting` window alone; the update
  notification stays on `core:event:default`. `maximizable(false)` is why `internal_toggle_maximize`
  is deliberately not granted: `drag.js` sends it on a double-click.
- **The backend measures the window; the renderer sends nothing.** `meeting.notificationPlacement`
  is written by two window-gated commands that read `outer_position`/`inner_size` themselves. The
  panel only says *something moved*, 250 ms after the last `tauri://move` or `tauri://resize`.

### Found in review, not by a test

1. **Every programmatic placement re-saved itself.** `restore` runs on every show and `reset`
   repositions a visible window; both fire move/resize events, and 250 ms later the panel asked the
   backend to save the rectangle it had just been given. A first-run window therefore acquired a
   saved placement after one show and stopped following the work area; a reset re-saved the default
   it had just cleared. Fixed by recording the physical rectangle after each programmatic placement
   and skipping a save whose rectangle equals it — exact, where a timer would have raced a user who
   drags within the window. Round-two brief; the round-one brief had said such saves were
   acceptable, so the finding was the orchestrator's as much as the worker's.
2. **A cascade defect that no gate can see.** `.notify-live-expanded { display: none }` was declared
   before `.notify-live { display: flex }` at equal specificity, so at 392×92 the expanded caption
   rendered as a second flex item after the Stop button. CSS has no gate in this repository; reading
   the file top to bottom is the evidence, and the repair brief asked the worker to write down which
   rule wins in each case.
3. **The theme worker reversed a documented decision because the brief told it to.** The brief asked
   for a `--warn` token; the `:root` block carries a comment explaining why `--warn` was removed
   from the theme. The worker replaced the comment rather than flagging the premise. The review
   caught it because the diff deleted a comment; the fix keeps the amber as a panel-local light/dark
   pair with a comment reconciling it, and restores the original comment verbatim. The same round
   also deleted `.notify-empty { background: transparent }`, the rule that keeps a stranded render
   invisible (KNEG-167). Lesson for briefs: a mapping that tells a worker what colour to add is not
   licence to overwrite the comment explaining why it was taken away.

### A Contracts gate that was already red on `dev`

`tools/capability-audit/scan.test.mjs` asserted six window builders in the real tree; the tree had
eight since KNEG-257's vocabulary IPC tests added two mock windows, and the KNEG-258 line-number
assertion had drifted too. It failed at `c17cbec` before this work touched anything. Nobody saw it
because the full CI cannot be dispatched (KNEG-276). The inventory refresh in `2805b17` fixes it.

### Measured, from the built application

`target/debug/knegiswhisper-desktop.exe` at `1946e4a` (Rust identical to `c90c953`), started with
`KW_AUTOMATION_PROFILE` on a disposable profile, windows enumerated by process id and read with
`GetWindowLongW` and `GetWindowDisplayAffinity` thirty seconds after launch:

| window | `WS_THICKFRAME` | `WS_MAXIMIZEBOX` | `WS_EX_NOACTIVATE` | topmost | affinity |
| --- | --- | --- | --- | --- | --- |
| `KnegisWhisper meeting` | **yes** | **no** | yes | yes | `0x11` |
| `KnegisWhisper update` | no | yes | yes | yes | `0x11` |
| `KnegisWhisper overlay` | no | yes | yes | yes | `0x11` |
| `Dictation languages` | no | yes | yes | yes | `0x11` |
| `KnegisWhisper` (main) | yes | yes | no | no | `0x0` |

So the builder flags took, the treatments still apply, and the other three panels are unchanged.
Not measured: any of it under a real mouse — see `BENCH_CHECK.md` § 2026-09-25.

---

## 2026-09-15 — KNEG-258: the block handle could not drag, and the cause was a default nobody wrote

Dragging a paragraph by its handle in the note editor did nothing. It had never worked: `NoteEditor.tsx`
has exactly one commit and `tauri.conf.json` has never carried a drag-and-drop setting, so this was not
a regression, it was the feature arriving broken and nobody trying it until now.

### The chain, read from source rather than guessed

Milkdown's block handle moves a block by HTML5 drag and drop. `@milkdown/plugin-block` sets
`draggable = true` on the handle, and on `dragstart` fills `dataTransfer` and sets `view.dragging`; it
does not perform the move itself — every one of its own `dragover`/`drop`/`dragend` callbacks returns
`false` and leaves the transaction to ProseMirror's `drop` handler. So the move needs a drop.

The `main` window did not set `dragDropEnabled`, and `tauri-utils 2.9.3` defaults it to `true`
(`config.rs:1947`). With it on, `wry 0.55.1` calls `SetAllowExternalDrop(false)` on the controller and
then walks the WebView2 child windows calling `RevokeDragDrop` on each and registering its own
`IDropTarget` — one that reads `CF_HDROP` and nothing else (`webview2/drag_drop.rs`). The drop half of
an in-page drag never reaches Chromium. Tauri's own doc comment on the field says it outright:
*"Disabling it is required to use HTML5 drag and drop on the frontend on Windows."*

The `dragstart` still fires, because that half is internal to the renderer. That is why the drag looks
like it begins, and why the defect reads as "dragging is broken" rather than "nothing happens".

### Measured, because a config change that is not observed is a guess

A scratch probe drove real `SendInput` mouse input at the running window — hover the first paragraph,
read the handle's rect through the WebView2 debugger, press on it, move in six-step increments to the
bottom of the third paragraph, release — and recorded every drag event that reached the page. Two runs
of the same probe against the same build, one key apart:

| | drag events reaching the page | paragraph order | saved markdown |
| --- | --- | --- | --- |
| Before | `dragstart`, `dragend` | unchanged | unchanged |
| After | `dragstart`, `dragenter` ×2, `dragover` ×30, `drop`, `dragend` | Alpha moved to last | Alpha moved to last |

**Synthetic events would have proved nothing.** A CDP-dispatched `dragstart` passes in both builds: the
question is whether the *OS* drag loop reaches WebView2, and only real mouse input asks it. The probe
also refuses to report at all when `SetForegroundWindow` fails — one run produced "nothing moved"
purely because the window never took focus, and a false negative that looks exactly like the defect is
worse than no run. It aborts the same way when the press would not have landed on the handle.

### What the fix costs, and the guard that pays it

With no drag-drop handler registered, wry leaves `AllowExternalDrop` at WebView2's default, and a file
dropped on the window navigates the webview to that file — the application replaced by a picture of a
cat until it is restarted. Tauri had been swallowing those by accident. `src/lib/fileDrops.ts` refuses
them on purpose: `dragover` and `drop` on `window`, bubble phase, and only when `dataTransfer.types`
contains `Files`. An in-page drag carries `text/html` and `text/plain`, so the editor is untouched —
checked in a real browser with real `DragEvent`s, not only in the unit tests, because a made-up event
object cannot tell you that `types` is the right property to read.

Nothing is lost by refusing them. Crepe's `imageBlock` has no drop path and no Crepe feature this build
mounts pulls `@milkdown/plugin-upload`, so a dropped file had nowhere to go. Accepting one is a separate
decision and would start in that module.

### The gate

Rule G in `tools/capability-audit/scan.mjs` fails when the frontend mounts the block handle and a window
in `tauri.conf.json` has not given Tauri's drag and drop up. It reads the *default*, not the absence of
the key, which is the whole point: a reader that only reported an explicit `true` would have passed the
tree that shipped this. Run against the unfixed tree it named `NoteEditor.tsx:53`, the window, the key
and the edit.

This is the same shape as rule F and as KNEG-171 before it: a platform default that is invisible in the
tree, that no test on this machine would fail on, and that needs the gate to read the thing that is
*not* written down.

---

## 2026-09-07 — KNEG-171: the fix was one word, the lesson is the gate

`security.capabilities: ["default"]` had sat in `tauri.conf.json` since the shell landed on
2026-08-25 (`7a5a348`), when `default.json` was the only capability file. KNEG-168 added
`notifications.json` on 2026-09-04 and left the list alone. Tauri treats a non-empty list as an
allow-list (`tauri-utils 2.9.3`, `acl/mod.rs`, `get_capabilities`), so the new file was parsed,
written into `gen/schemas/capabilities.json` — which is why that file looked like proof — and never
compiled into the ACL. The fix adds `"notifications"` to the list. Keeping the list rather than
deleting it was a choice: an explicit list is a second place where a reviewer sees which windows may
do what, and with rule E it can no longer drift from the directory.

**Why a gate and not a comment.** Rule E in `tools/capability-audit/scan.mjs` fails when a
capability file is not enabled by the list, and rule A now counts only enabled capabilities. Run
against the unfixed tree it named the defect exactly; the same gate had passed that tree for three
days because it read the files and not the list. The general form is worth stating: a gate that
checks an artifact exists must also check the switch that decides whether the artifact is used.

**What the panel showed once it could hear.** Recording state, clock, Stop and caption all rendered
on the first real recording — KNEG-163's fill fix, KNEG-164's read-back and KNEG-165's caption
running in their own window for the first time. Two things were wrong that nobody could have seen
before: the second line stays `starting…` for the whole recording because `sources` is read once at
mount and never again (KNEG-172), and the caption shows the head of a partial that this provider
grows to 230 characters, so the live edge sits behind an ellipsis (KNEG-173).

**Small measured facts.** `tauri dev` rebuilds and relaunches on a `tauri.conf.json` change in about
50 s, and the WebView2 debugging port survives the relaunch. CDP `Emulation.setEmulatedMedia` does
change `prefers-color-scheme` inside a WebView2 webview. `tauri dev`'s `Running` line carries ANSI
colour codes between the word and the backtick — strip them before grepping for it.

## 2026-09-07 (later) — KNEG-60, KNEG-161, KNEG-166: instruments instead of eyes, and an inert fix

Three "by hand" issues were closed or advanced by building instruments rather than by looking,
through an orchestrated run (two Codex workers, `e75e860` and `19a92a2`, merged `cd17676` and
`d94ac5b`). The judgement calls:

**A probe for capture exclusion, not a screen recorder.** KNEG-60 asked for "a second machine or a
capture tool". A recorder would have answered for one product; `capture_probe` answers for the three
capture APIs recorders are built on (GDI `BitBlt`, Windows.Graphics.Capture, DXGI Desktop
Duplication), with an unexcluded control window beside the excluded one so a capture path that
cannot see its own known-positive pixels scores `NO ANSWER` rather than `EFFECTIVE`. That control
earned its place on the first run: Desktop Duplication's first acquired frame after `DuplicateOutput`
carries no desktop image when nothing has been presented since, and the probe reported a black
control instead of a false pass. The capture-stack `windows` features are dev-dependencies, so the
shipping crate links nothing new; `PrintWindow` is declared by hand because `windows 0.58` files it
under an XPS feature nothing else wants.

**Headless Edge as the shell's first pixel-level measurement.** KNEG-161's eight claims were
believed from CSS. Edge 152 is the engine WebView2 152 runs, so `tools/visual-check/shell-rail.mjs`
measures computed style and geometry there against the dev fixtures — and says so in its report
header, because fixtures and a real window are different evidence. The fixture banner is removed by
default: it pads `body` by 2.6rem while the fixed rail is not shifted, a misalignment that exists
only in the fixture tab. The tool reports pixels beside the live rem basis rather than asserting
KNEG-160's figures, which turned out to assume a 16px rem in a 14px application.

**WebView2 remote debugging as the way to drive the real application.** Setting
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` before `npm run tauri:dev`
exposes every webview as a CDP target, and `window.__TAURI_INTERNALS__.invoke` works from
`Runtime.evaluate`. Believed from documentation when the brief was written, measured the same
evening. `app-windows.mjs` drives the Meetings screen by its visible controls, not by IPC, because
the question was what a person would see.

**What the instrument found, and why two gates missed it.** During a real recording the meeting
notification window was visible and empty for 60 s. The webview cannot `listen`: `tauri.conf.json`
sets `security.capabilities: ["default"]`, and Tauri enables only the listed capabilities when that
entry is set, so KNEG-168's `notifications.json` was never loaded. Nothing failed loudly — the ACL
denial is a rejected promise the panel discards, `tools/capability-audit/scan.mjs` reads the
capability files rather than the allow-list, and the mock-webview IPC test does not exercise
events. KNEG-171 carries the fix and the gate. The lesson is the one this log keeps relearning: a
fix whose only evidence is that its file exists has not been verified, and the check that would have
caught this is the same check that found it — start a meeting and read the panel.

**Two small measured facts worth not rediscovering.** PowerShell binds `$null` to a .NET `string`
parameter as an empty string, so `FindWindowW($null, title)` finds nothing; enumerate windows or
pass `[NullString]::Value`. And a CDP `Page.captureScreenshot` `clip` is in page coordinates, so a
clip of a fixed element taken after a scroll must add `window.scrollY`.

## 2026-09-07 — KNEG-157: disclose meeting recording attachment failures

The saved meeting outcome now carries `recordingAttached`. Attachment failure
keeps the successful note outcome and original WAV, but the meeting result
explains that playback is unavailable and directs the user to the note's
“Attach recording…” action with the existing recording path. Generated IPC types
and frontend fixtures carry the flag. No migration or automatic retry was added.

A temporary-database trigger forces attachment insertion to fail: the regression
check verifies that the note and recording survive, no attachment is committed,
and attachment succeeds after the fault is removed. Invalid storage keys report
failure too. Frontend tests check the failure/recovery message and its absence
on success; the IPC serialization test verifies the false flag reaches JSON.

## 2026-09-07 — delivery review and persistent import progress

Reviewed recording transactions/cleanup, permission verification and token
revocation, keyboard hook replacement, Whisper startup/watchers and renderer
completion. Added Rust-owned single-import snapshots with operation identity,
stages, elapsed time, provider/model and terminal results. Upload restores status
after navigation and links to the saved note; sidecar-install events stay separate.
The route and local engine are captured before conversion. Failed/busy attempts
remain actionable and the slot is released after failure or interruption.

Parakeet benchmarks reproduced exit 0xC0000409 on long repeated-speech input;
see PARAKEET_BENCHMARK.md. Preserved full native exit status/final diagnostics and
added cancellation cleanup for its child and scratch audio. No native crash cause
is claimed. Fixed the settings audit mutation test to support CRLF and verify
that its intended mutation actually happened.

Automated validation includes the repository gates, Windows workspace and IPC
suites, TypeScript behavior tests, production build and native audio/video
pipeline. Final delivery validation is against an isolated committed checkout.
User-observed evidence and remaining external checks are in BENCH_CHECK.md.
Unrelated icon assets and icon rebuild changes are excluded from this batch.
Publishing, hosted CI execution and merging remain separate steps.

The isolated committed checkout passed 1,776 workspace tests (13 ignored),
375 Tauri mock-webview tests (5 ignored), six TypeScript tests, formatting,
workspace/IPC Clippy, the production frontend build, contract/generated checks
and scanner/CI-selection suites. The existing frontend chunk-size warning remains.
Browser fixtures additionally verified completion restored after navigation and
opening the imported note with editor/transcript controls. That check exposed a
missing development-only `convertFileSrc` mock, now fixed; it is not native audio
playback evidence. The temporary browser preview was closed after verification.

KNEG-170's functional recovery/diagnostics scope is now Done, with the unresolved
Whisper exit cause explicitly linked to open KNEG-96. KNEG-111/KNEG-40 still await
hosted Windows success. The local PR text and complete check/commit inventory are
prepared under ignored `target/delivery/`; no publication was performed.


## 2026-09-05 — hotkeys, API permissions, Whisper recovery and retained imports

Implemented KNEG-115, KNEG-169, KNEG-170/KNEG-96 hardening, KNEG-155 and selective KNEG-111/KNEG-40 CI configuration. The owner approved release-qualified modifier toggles, automatic import retention, repair-then-refuse API behavior and selective PR checks. `Control+Super` remains the default.

- **Hotkeys:** raw press/release events remain available for Hold; Toggle uses a separate qualified activation after full release. Any additional key cancels that candidate until the gesture ends. A separately configured longer modifier chord can qualify independently. The Windows hook probe delivered two qualified activations for two injected clean chords.
- **API permissions:** verify directory and empty exclusive temporary before token bytes; publish and verify before serving. Request admission checks run off the executor; a five-second monitor catches idle exposure. Revocation is permanent for that token, repairs once, stops service and requires explicit retry. Settings, CLI and MCP status retain the failed verdict. An isolated explicit Everyone read grant was detected and removed by the existing ACL implementation; the origin of the real-machine trustee remains unknown.
- **Whisper:** bounded byte-oriented stderr collection survives invalid UTF-8 and keeps unterminated final output. Exit reporting drains for at most one second and reports capture completeness, exit, lifetime and process identity. Conditional PID removal protects newer processes. Serialized startup shares an attempt among overlapping callers, including failure; dictation and imports use one pre-submission recovery path. No second background restarter or transcription retry was added. Runtime diagnostics disclose failure and recovery.
- **Imports:** Rust commits the note and recording together, retaining a validated UUID WAV copy and original basename. Failed copies/transactions remove their generated files. The renderer receives the committed note ID and metadata, with retention disclosure and size. Source files and disabled URL-import policy are preserved.
- **CI:** PRs always run Contracts and select relevant frontend/Windows jobs. Manual dispatch retains the three-platform matrix. No automatic push, release or pin workflow was enabled. File selection includes deletions and renames. Hosted execution has not been observed because these changes have not been published.

Validation: 1,771 workspace tests passed (13 ignored); the Tauri feature suite passed 372 tests (5 ignored). Both Clippy configurations, formatting, repository contracts/scanner suites, TypeScript tests and frontend typecheck/build passed. The existing large-bundle warning remains. Additional focused tests cover the final API lifecycle serialization and startup cleanup.

Native fixture checks passed for audio/video conversion, transcription, persisted recording, playback byte ranges and deletion; concurrent recovery replaced an intentionally terminated owned Whisper process and transcribed successfully. Separate ten-minute idle and repeated-transcription probes saw no unexpected exit; the latter completed 85 requests without errors. These observations do not establish the intermittent crash's cause. See BENCH_CHECK for exact limits and remaining user checks.

## 2026-09-05 — KNEG-162 and KNEG-154: repeatable frontend tests and explicit WAV attachment

`npm test` now compiles TypeScript tests into an ignored output directory and
runs Node's test runner. The navigation suite asserts the intended destination
and group mapping. A wrong-group mutation failed an assertion; an injected type
error stopped before test execution and left no stale compiled test. The CI
Frontend job and README call the same command; workflow triggers stay paused.

Older notes can now attach their original WAV through one Rust-owned dialog
command. Paths never cross this IPC boundary. The original is copied to an
exclusive UUID filename, and the copy is checked sample by sample as 16-bit PCM
mono/stereo WAV before its metadata is inserted. No timestamp-based matching,
transcoding, transcription, replacement, or media-import retention is added.
Failure cleans up the generated copy; cancellation creates nothing. The database
constraints resolve races with another attachment or note deletion.

The note workspace updates only its recording metadata: its editor and autosave
remain mounted. A generation check discards late UI responses after navigation.
Backend tests use disposable databases/files and exercise source preservation,
WAV rejection, failed insertion/copy, duplicate attachment, and protocol ranges.
A separately invoked Windows smoke harness registers the real Notes commands and
playback protocol against disposable data; ordinary test runs never open it.

Validation: 1,753 workspace tests passed (12 intentionally ignored); the explicit
Tauri test suite passed 367 tests (3 ignored, including the interactive harness).
All 134 scanner self-tests, the three TypeScript tests, repository contract checks,
frontend typecheck/build, and workspace Clippy passed. The capability scanner's
inventory now explicitly includes the second mock `main` webview used by the
recording-deletion IPC test. Native picker cancellation, attachment, row-playback
progress, reopening, and source/copy equality were observed; see BENCH_CHECK.md
for the exact scope and remaining by-ear/restart/package checks.

---

## 2026-09-05 — KNEG-153: desktop and API delete the same recording

Both note-deletion surfaces now capture the attachment and delete the note in one
SQLite write transaction, then remove the captured file after commit. The local
API (and MCP through it) no longer skips file cleanup. The shared notes-core
resolver rejects traversal and Windows drive/alternate-stream syntax.

The agreed policy remains best effort after commit: absent files are harmless,
other cleanup failures warn, and the note stays deleted. There is no cleanup queue.
Rollback and concurrency tests use disposable databases and recordings; this is
not evidence of a native dialog or playback check.

---

## Carry-forward — what the next session needs to know

> **All 19 tasks in the plan, plus one added mid-run (Task 20, the SSRF boundary fix), are
> complete.** [BUILD_PLAN.md](BUILD_PLAN.md) covers phases 0–9 as chunks A–J and every chunk is now
> either done or explicitly, permanently deferred with a named reason — there is no "not started"
> left in it. [PHASES_0_6_COMPLETION_PLAN.md](PHASES_0_6_COMPLETION_PLAN.md) keeps the phases 0–6
> detail and the by-hand check that found five real defects; [COMPLETION_PLAN.md](COMPLETION_PLAN.md)
> keeps the phase 7–10 definitions.
>
> **Measured, not asserted, as of 2026-09-04 (after KNEG-168):** `cargo test --workspace` —
> **1740 passing, 0 failed, 12 ignored** (941 at the start of the original run, 1296 at its close on
> 2026-08-27, 1704 and then 1734 on 2026-09-02); parity —
> **103 entries, 4 declared unreachable** (17 at the start, 8 on 2026-08-27), and **all 13 UI
> surfaces built**, with no "surface not built" entry remaining anywhere. The four: **no local-ASR
> model any longer** — all four sherpa models are pinned and installable as of 2026-09-01 — two
> Tinfoil entries (batch transcription
> and meeting-stream, both declined on principle, not unfinished), Apple Calendar (EventKit —
> described in `kw_calendar_core::apple`, implemented nowhere, on any platform), and URL import
> (withdrawn as a command — see the 2026-08-27 entry on the merged security fixes).
>
> Eight became six when `121dc4c` gave the two **Parakeet** models the Settings selector their
> measured runtime had been waiting for; six became five on 2026-09-01 when the streaming protocol
> was measured and `nemotron-speech-streaming-en-0.6b` got a client; and five became four hours
> later when the multilingual Nemotron was pinned. Worth spelling out because an earlier version of
> this paragraph named Parakeet as one of the unreachable pair, which is the opposite of what
> happened to it.
>
> Also worth spelling out: the last of those four moved because a **person pressed the button and
> reported what he saw**, three times. Nothing in the repository was going to.
>
> **The reachability rule, settled and applied repeatedly:** `reachable: true` means a user can
> *configure* the integration through a named, **built** surface — not that the backend code works.
> `reachable: false` is the honest answer for a working runtime with no UI selector (the two sherpa
> Parakeet models), for a capability declared and refused on principle (both Tinfoil entries), and
> for a capability described but implemented nowhere (`calendar:apple`). Four separate times this
> run, a controller brief's arithmetic or premise collapsed "the code works" into "a person can
> reach it", and every time an implementer caught it by flagging rather than complying — see the
> dated entry below, "the reachability rule, stated once and applied four times".
>
> **The most significant finding of the whole run was Task 20's, not a planned task:** the network
> boundary's `https` and `wss` arms ran **no host check at all**, so `https://169.254.169.254/` and
> a redirect to it were permitted under every purpose — the classic SSRF target, unblocked, while the
> documentation called the boundary the control. Fixed centrally in `crates/network-policy`, not
> patched locally, because the gap was general and every existing call site had to be re-audited for
> a newly-refused legitimate self-hosted endpoint (none was found). The review of that fix then found
> a Critical of its own — a trailing dot (`localhost.`) defeated the whole predicate — and the
> more important structural fact underneath both: **the same kind of gap that used to fail closed
> now fails open**, because the fix inverted what a miss in `is_private_host` costs. See "Task 20 —
> the network boundary does not stop SSRF over HTTPS" below.
>
> **`Notes > Upload`'s import paths are now a capability, not an assumption (KNEG-80).**
> `pick_import_file` returned the chosen path to the renderer and `media_import_audio`,
> `media_import_video` and `import_openwhispr` read whatever they were handed; the renderer could
> not *obtain* a path (`capabilities/default.json` is `["core:default"]` and nothing else) but
> nothing *checked* that, so a compromised renderer could name any readable file and, with a cloud
> transcription provider configured, watch it leave the machine. `commands::DialogGrants` now
> records the canonical form of every dialog-returned path for the life of the process and those
> three commands — **the only three in the application that accept a path**, enumerated rather than
> assumed — refuse anything else. Session-lifetime because retry-after-failure exists today,
> count-bounded rather than timed for `meeting::Watcher::offered`'s reason, and keyed on
> `canonicalize` because `display()` is lossy — the cost being that a path whose name does not
> survive `display()` is refused outright. Four unit tests assert the policy; **only a real call
> against the running application proves it is consulted**, which is still open as item 1 under
> `BENCH_CHECK.md`'s Task 12 heading. See "a path the renderer could invent" below.
>
> **Task 19 audited every manifest entry whose `status` asserts a working implementation exists**
> (`verify-parity.mjs`'s `ACTIVE` set) and found three more instances of the same overclaim
> `platform:aec` was already corrected for mid-run: `meeting-stream:tinfoil-realtime` (status
> `"ported"`, no Tinfoil meeting client anywhere), `transcription:tinfoil` (status
> `"rewired-to-direct-byok"`, the client returns `ClientNotImplemented`), and `calendar:apple` (status
> `"platform-specific"`, `AppleCalendarSource` implemented by nothing on any platform). All three now
> carry `temporarily-deferred`, though for the two Tinfoil entries that status is a compromise, not a
> fit — this manifest's vocabulary has no term for "declined on principle", and each entry's
> `deferral.why` says so rather than letting the status imply the refusal is resource-blocked and
> temporary. A fourth, adjacent overclaim was found outside the `ACTIVE` set entirely:
> `developer:agent-skills` (`kind: "documentation"`) claimed `status: "ported"` naming
> `docs/agent-skills/` as where the rewritten docs live, and that directory does not exist — the
> gate's documentation check only requires a target to be *named*, never that it exist. Also
> corrected to `temporarily-deferred`. See "the ACTIVE-status audit" below.
>
> **[BENCH_CHECK.md](BENCH_CHECK.md) remains, in full, the record of what has actually been run on
> hardware, and chunk E is no longer absent from it.** The 2026-08-25/26 runs covered phases 2, 6
> and chunks A–D. Bench runs on **2026-08-27, 2026-08-28, 2026-08-31 and 2026-09-01** then reached
> chunk E: a meeting recorded, transcribed and saved end to end; two real Teams calls; system audio
> resolving a real output device; and the capture-session measurement behind KNEG-94. Calendars,
> media import, the MCP/CLI screens and the updater are **still green tests and code review only**.
> That file's own by-hand lists say which is which item by item, and nothing on them should be read
> as done until someone actually presses the buttons — including the one KNEG-94 left behind, which
> is watching the offer window appear during a call.
>
> **The cross-cutting gap in all four meeting socket clients is closed at the end of the meeting
> and still open during it (KNEG-79, 2026-08-31).** All four (`openai_socket.rs`,
> `assemblyai_socket.rs`, `deepgram_socket.rs`, `corti_socket.rs`) used to log a provider-reported
> error frame with `tracing::warn!` and never turn it into an `Err`, so a server that accepted a
> malformed session-config frame, stayed open and emitted nothing let a meeting "complete
> successfully" holding an empty transcript. They now record into
> `kw_meeting_core::fault::SessionFault` and both `send_audio` and `finish` consult it, so Stop
> reports the provider's own sentence. `stop()` was fixed with it — a pump failure used to return
> early, leaving the runtime idle and the screen stuck on "Recording" with no working button.
> **The mid-meeting half closed the same day (KNEG-95).** A dead provider now changes the
> indicator instead of leaving it saying "Recording" — a fourth `MeetingState`, the recording
> deliberately left running, and the provider's own sentence on `Settings > Meetings`. See the
> 2026-08-31 (KNEG-95) entry.
>
> **What is still true:** three of the four clients have never met a real provider error, because
> no key exists for AssemblyAI, Deepgram or Corti, so the wiring is what the tests check and not
> any provider's behaviour. See the 2026-08-31 (KNEG-79) entry below, and `BENCH_CHECK.md` item 13.
>
> **Eighteen of the thirty-five local text-generation models could not be installed at all**, and
> the reason was arithmetic in a config file rather than anything in the download path: upstream's
> `sizeBytes` was a base-10 "GB" label multiplied by 2^30 instead of 10^9, so eighteen models
> downloaded in full — 101 GB — and were deleted by a 5% size check on arrival. All 39 objects (35
> models, 4 drafters, 202 GB) are now measured from Hugging Face's `x-linked-size` into
> `LOCAL_LLM_PINS` in the registry generator, and a measured size is checked for **exact** equality
> rather than within 5%. Two related facts worth carrying: `qwen3.5-2b-q4_k_m` was passing the old
> check by 0.0042 percentage points, about 5.6 KB, and two Gemma models were already 2 240 bytes
> adrift from their declared size — upstream re-uploads are real, and the `#[ignore]`d
> `the_measured_sizes_still_match_hugging_face` test in `local_llm.rs` is what catches the next one.
> See the 2026-08-31 entry on the local-LLM sizes below.
>
> **All 39 also carry a SHA-256 as of the same day (KNEG-45), and 32 of them are the publisher's
> stated `lfs.oid` rather than bytes hashed here.** That is a weaker standard, deliberately taken,
> and it is carried through the code as a distinct state rather than a footnote:
> `sha256Measured` in the registry, `DigestCheck::Stated` in Rust, `VerifiedBy::StatedDigest` on
> the wire, and a different sentence on screen. Seven objects — one per publishing organisation —
> were downloaded and hashed to test the metadata before any of it was trusted. `node
> tools/model-pin/pin.mjs --verify-stated` re-reads all 39 in 36 requests and runs weekly.
>
> **The Windows DACL gap is closed as of 2026-08-31 (KNEG-34), and the two things it did *not*
> close are worth carrying.** `kw_security::file_acl` applies and reads back an explicit protected
> access list at all four call sites — the discovery file, its directory, and both of
> `kw_sidecar::pid`'s. Measured with `icacls` from outside the process: three trustees (the process's
> token user, `NT AUTHORITY\SYSTEM`, `BUILTIN\Administrators`), no `(I)` on any of them, on both
> paths. **Deliberately not owner-only** — an administrator can take ownership anyway, so what a
> confirmed Windows result claims is that the *unexpected* trustees are gone and inheritance is off,
> which is narrower and true. The snapshot that used to sit behind that verdict is gone — since
> KNEG-110 both screens re-read the access list on every status request. Still open: **CI compiles
> none of the Win32 code**, because the
> `windows-latest` job is off for every trigger but `workflow_dispatch`. The pure classifier is
> covered on Linux by design — `file_acl::classify` takes plain Rust facts and the measured DACL is
> a fixture in its test module. The three Win32 calls are covered by this repository's first two
> `#[cfg(windows)]` tests and by `cargo run -p kw-security --example dacl_probe`, and by nothing
> unattended. KNEG-111 tracks that gap. See the 2026-08-31 KNEG-34 entry below and
> `docs/BENCH_CHECK.md` finding 2.
>
> **The DACL is now measured on the live file**, not only on test directories: after a normal
> application start, `dacl_probe` reports `SE_DACL_PROTECTED` set and exactly three access entries,
> none inherited. `Jobbdator\CodexSandboxUsers` is gone from the real token file.
>
> **Three smaller items recorded rather than chased.** The `kw-storage` test that failed once and
> passed on re-run during Task 8 was **found on 2026-08-31**:
> `import::tests::no_plaintext_token_appears_anywhere_in_the_target_file`, twice in four hundred
> runs and only under eight threads. The assertion that fired was the positive control — meaning
> the two "no plaintext token here" assertions beside it had just passed against a database file
> whose rows were still in the write-ahead log. Fixed by reading every file the database is made
> of, which is also the stronger claim. See the KNEG-88 entry. The remaining two:
> `apps/desktop/src-tauri/src/meeting.rs` at ~1600+ lines, noted repeatedly and deliberately not
> restructured; and `SherpaModel` in the provider registry carrying no SHA-256 for its model
> archives, unlike `WhisperModel` and unlike the sidecar binaries in `config/sidecars.json`.

Maintained in place. If something here becomes false, edit it rather than adding a note.

### Selective CI is configured; hosted execution still needs evidence

`ci.yml` now configures Contracts on every PR, relevant frontend and Windows Rust jobs,
and the full three-platform matrix on manual dispatch. Automatic push runs remain off.
This replaces the all-manual configuration while keeping the budget pause in the historical
entries below. No new hosted run has been observed; do not describe local green checks as
GitHub Actions evidence. Publishing and merging this batch were not authorized.

### Committed but not usable end to end

| Thing | State | What is missing |
| --- | --- | --- |
| Local transcription (Phase 2) | **Driven by hand** | The core leg is closed: 2026-08-28, in the real application window, dictation into Notepad produced correct text repeatedly on both `whisper-tiny` and Parakeet. Four smaller checks remain and are KNEG-47's — the same dictation in VS Code, the caret never leaving the target window, the `KEEP ME` clipboard round trip, and `Control+Super` mid-dictation. |
| Meeting transcription (Phase 4) | Four clients written, one confirmed | OpenAI Realtime has met a live server; AssemblyAI, Deepgram and Corti are *believed* wire shapes with no key on any machine this project has touched (KNEG-53, KNEG-55-57). Tinfoil is declined on principle, not missing. A dead provider is reported at Stop *and* mid-meeting, with the recording deliberately left running (KNEG-79, KNEG-95). |
| Enterprise reasoning (Bedrock, Azure, Vertex) | `ShapeNotImplemented` | The three request shapes. They refuse honestly rather than falling back, and none has made a live call (KNEG-54). |
| `storage::import::from_openwhispr` | Reachable | Nothing. `Notes > Upload` exposes it (KNEG-71), and it now refuses a path no file dialog returned (KNEG-80). Nobody has imported a real OpenWhispr database by hand (KNEG-90). |
| Local API and CLI (Phase 6) | Reachable, exercised by hand | 2026-08-25: the compiled CLI read history out of a running application. The discovery file's permissions are re-read on every status request rather than remembered (KNEG-110), and on Windows carry an explicit protected DACL verified on the live file (KNEG-34). |
| Calendars (Phase 5) | Built, never connected | `calendar-core`, loopback PKCE, `Integrations > Calendar`. No calendar code has answered a real request (KNEG-82); Apple Calendar is described and unimplemented (KNEG-81). |
| MCP and the integration screens (Phase 7) | Built, never connected | Thirteen tools over stdio, plus both screens and the agent skills. No MCP client has ever driven it (KNEG-91). |
| Media import (Phase 8) | Reachable | Subtitle export is verified end to end against the real server. Diarization and semantic search are not started (KNEG-73). Nobody has pressed *Import* on a real file (KNEG-90). |
| Release (Phase 9) | Never run | The workflow exists, has never been dispatched, and no signing keypair exists (KNEG-43). |
| Agent features (Phase 10) | Not started | Chat agent, voice agent, translation (KNEG-74). There is no chat screen; the local LLM's one consumer is dictation cleanup. |

### Claims that are unverified rather than verified

These are the places where CI going green means "it compiles", not "it works". Each is honest in
`docs/PLATFORM_SUPPORT.md`; the risk is a future session reading a passing build as proof.

- **Nothing macOS-native has ever run.** No macOS hardware, no macOS target installed. The
  non-activating panel, EventKit, and CoreAudio process taps are the three hardest native pieces
  and all three are macOS-only.
- **Windows capture exclusion has never been tested against a real screen recorder.** The
  read-back proves the flag is set on the window. It does not prove any given recorder honours
  it, and no in-process API reports that. Confirming it needs a second machine or a capture tool.
- **Linux has never been run at all**, and several capabilities will legitimately report
  `unsupported` there (content protection is a no-op; Wayland restricts global shortcuts and
  synthetic input).
- **macOS and Linux CI ran for the first time on the Phases 1–4 push.** If either is red, it is
  a first-run failure, not a regression.

### The general lessons, stated once

- **A pin is only a pin if it was measured.** Every digest in `config/sidecars.json` came from
  downloading the asset and hashing it, not from a release note. The catalogue is compiled in with
  `include_str!` for the same reason — a digest in a file the user can edit is not a pin.
- **Test the contract against the real thing once, and everything else against fixtures.** The
  ignored tests (`real_install`, `local_dictation`) exist because a fixture cannot tell you whether
  the pinned digest matches what the host serves, whether the redirect chain is followable, or
  whether the arguments are ones this build accepts. Those are exactly what breaks when an upstream
  release is re-tagged.

- **A denylist that matches nothing passes trivially.** "The tree is clean" and "the patterns
  work" are different claims and only the second one matters. This is why
  `tools/domain-scanner/scan.test.mjs` asserts the shipped patterns still catch real vendor
  strings — a scan result alone cannot tell you the scanner is alive.
- **A frontend type that declares a field the backend never sends renders a fiction, not an
  error.** TypeScript reads the missing field as `undefined` and any UI built on it displays
  something confident and false. `apps/desktop/src/lib/ipc.ts` carries a header comment saying
  so, because the compiler will not.
- **A purpose is a property of the destination, not the feature.** Getting that backwards made
  local model servers unreachable and produced an error message that blamed the wrong thing.
- **Prefer deriving an allowlist over writing one.** The registry-derived cloud-provider set and
  the client-derived meeting allowlist each fix a class of bug rather than an instance.
- **A development build is two processes, and instructions that name one are wrong.** `cargo run`
  starts the Rust half; the window then loads from the Vite dev server in `tauri.conf.json` and,
  with nothing serving it, shows the webview's own "can't reach this page" — while the log reports
  a flawless startup. Both halves were honest and the pair was undiagnosable. A debug build now
  probes `devUrl` at startup and names the fix in the log, which is the general shape: when a
  component cannot see the failure it causes, make the component that *can* see it say so.

### Open decisions

Unchanged from the Phases 1–4 entry, restated here so they are not buried:

- ~~**whisper.cpp source**~~ — **settled.** `ggml-org/whisper.cpp` at tag `b4938`, pinned in
  `config/sidecars.json`. The worry that upstream might not publish usable binaries turned out to
  apply only to macOS: the `b*` tags carry `whisper-bin-x64.zip` for Windows and
  `whisper-bin-ubuntu-{x64,arm64}.tar.gz` for Linux, all containing a real `whisper-server`, and
  those three are now installed and verified from upstream with no vendor dependency at all. The
  tagged `v*` releases carry source only, which is what made this look impossible at first.
  macOS gets an xcframework — a library for embedding in an Xcode project, not a server — so it
  is recorded as `NotPublished` with a reason and needs a macOS CI job of our own.
- ~~**ffmpeg and yt-dlp sources**~~ — **settled.** `yt-dlp/yt-dlp` at tag `2026.08.19` (GitHub
  marks the release `immutable` itself) ships a bare executable on Windows, Linux x86_64, Linux
  aarch64 and — one universal2 binary covering both — macOS, so all five targets are pinned with
  `archive: "bare"`. `BtbN/FFmpeg-Builds` at `autobuild-2026-07-31-14-10` covers Windows and Linux
  only; per its own README it never builds macOS at all, under any tag, so that gap is not a
  missing pin to chase but recorded `NotPublished`, the same shape as whisper-server's macOS gap
  above. That release was chosen over a more recent daily autobuild deliberately: BtbN prunes
  daily builds after 14 days and keeps only the last build of each month for two years, so a dated
  tag alone was not enough here — it also had to be one their own retention policy will not delete
  out from under the pin. The `lgpl` (not `gpl`) variant is used because this project stays wary
  of GPL entanglement (see `docs/UPSTREAM_AUDIT.md`'s reasoning about Pluely) and decode-to-WAV
  transcoding never calls the GPL-only encoders (`libx264`, `libx265`) that variant lacks.
- ~~**AEC helper and the five Windows native helpers**~~ — **settled**, see
  [PORTING_MATRIX.md](PORTING_MATRIX.md). Two corrections came out of looking properly. First,
  this tree never fetched any of them; the claim that they "still come from OpenWhispr release
  assets" described *upstream's* arrangement, not ours. Second, they do not block meetings —
  system audio does, and echo cancellation only matters on speakers.

  The five Windows helpers are reimplemented in Rust rather than rebuilt and published. One
  already was: `windows-fast-paste.c` is `platform::window::send_ctrl_v`. Only the AEC helper
  stays a binary we build, because it wraps WebRTC's audio processing module.

  Worth recording for whoever writes that build: **the two third-party inputs cannot be pinned the
  same way.** Abseil's GitHub archive is byte-stable and takes an ordinary sha256. The webrtc-apm
  tarball is generated per request by googlesource — two downloads seconds apart differed in
  length and digest — so it needs a content digest over the extracted tree, which *is* stable
  across downloads. Pinning that URL by archive hash would fail on every build, and a pin that
  always fails gets deleted by the first person it inconveniences. Both measured values are in the
  porting matrix. Upstream pins both by commit and verifies neither.
- ~~**`nircmd`**~~ — **dropped**, as leaned towards: third-tier Windows paste fallback only, and
  `SendInput` works.

---

## 2026-09-04 (KNEG-168) — four issues, one root cause: two windows were in no capability

The corner-window saga ends with the thing none of it was about. `e0c0250` and `74ca215`.

### What it was

`capabilities/default.json` was the only capability file, granting `core:default` to
`["main", "overlay"]`. The application creates four windows: `main` from `tauri.conf.json`, and
`overlay`, `meeting` and `update` from `pub const LABEL` in their own modules. Tauri v2 scopes
capabilities by window label, and **a window matching no capability has an empty permission set.**
`core:default` is what contains `core:event:default`, which is what `listen()` needs.

So the meeting and update notification panels could not subscribe to any event, and never could.
Every `onMeetingState`, `onMeetingOffer`, `onMeetingTranscript`, `onMeetingOutcome` and
`onUpdateCheckResult` in the two windows written to render them was denied.

The capability's own description said how it happened: *"Least-privilege permissions for the
dashboard and overlay windows."* True when written. The notification windows were built later and
nobody added them.

### The measurement, which is the part worth keeping

A meeting was started with `local-streaming`. The log:

```
11:12:24 INFO meeting notification window ready capture_exclusion=Active{...} non_activating=Active{...}
11:14:35 INFO sidecar ready sidecar=sherpa-stream port=8250 pid=18604
11:14:35 INFO starting microphone capture device="Microphone Array (AMD Audio Device)"
11:14:36 INFO starting system audio capture device="Headphones (Bose QC45)"
11:14:36 INFO meeting started provider="local-streaming" system_audio=true
```

Zero `WARN`. Zero `ERROR`. `set_visible(true)` succeeded — it logs at `error` when `show()` fails.
Microphone and system audio both captured for the whole session. **And the window rendered
nothing.**

The panel's only surviving data source was its mount-time `invoke`, which ran at 11:12:24 when the
state genuinely was idle — custom `#[tauri::command]` calls are not ACL-gated, so that one worked.
The state event two minutes later was emitted and dropped. The panel still believed idle.

The overlay was unaffected because it *is* in the capability list. That is why this survived: the
one panel anybody watched was the one that worked.

### Why four issues explained a symptom each

* **KNEG-163** — the empty panel was *white*. Real, and fixed; confirmed by eye when it went
  transparent.
* **KNEG-164** — the offer was unrecoverable after a missed event. Real code, correct, and
  **unreachable**: the offer event never arrives, and the window is only shown at the moment that
  same undelivered event fires.
* **KNEG-165** — the caption. Its design note recorded the assumption that killed the whole
  investigation: *"`AppHandle::emit` is a broadcast, so the notification window is already a
  recipient."* True of the Rust side. False in practice, because the ACL sits between the
  broadcast and the listener. That line is corrected there now.
* **KNEG-167** — the window visible with nothing to render. The reconciliation is right and useful,
  but the panel was empty because its information was two minutes stale, not because a read failed,
  so the `.catch()` half never fired either.

Each of those was diagnosed by reading, each was internally consistent, and each was downstream of
a permission file nobody thought to open. The lesson is not "read the ACL"; it is that four
consecutive explanations that all fit and none of which was verified on screen should itself have
been the signal.

### The fix, and why it is narrow

A second capability, `notifications.json`, granting **only** `core:event:default` to
`["meeting", "update"]`. What the panels use from JavaScript was enumerated rather than assumed:
`invoke` of this application's own commands, which the ACL does not gate, and `listen` plus the
`unlisten` callback it returns. Nothing else is reachable from either panel, including through
`lib/ipc.ts`. `core:default` would additionally grant window, path, webview, menu, tray, image and
resources permissions to windows that call none of them.

Checked against the regenerated ACL rather than believed: `core:event`'s `default_permission` is
`["allow-listen", "allow-unlisten", "allow-emit", "allow-emit-to"]`. So the grant covers both calls
the panels make — and two they do not. Narrowing to the pair was left undone on purpose: `:default`
is the plugin's own unit of grant, and a bespoke pair would break confusingly the first time a
panel legitimately emits.

### The gate, and the rule that keeps it honest

`tools/capability-audit/scan.mjs`, the ninth of its shape here. Rule A is the defect: a label from
`tauri.conf.json` or a Rust `pub const LABEL` that no capability matches. Rule B is its mirror, so
a fix-by-typo fails. Rule C keeps the exemption manifest attached to reality.

**Rule D is the interesting one.** Rule A finds only labels it can *see*, and it sees them by
recognising a convention — so a future window written without a `pub const LABEL` would be
invisible and A would pass while the defect recurred. D therefore requires every
`WebviewWindowBuilder::new(` to resolve to either a same-file `LABEL` const or a string literal
that is already a known label. A literal naming an unknown window still fails. Both shapes present
today are legitimate, including `media.rs`'s mock-webview test passing `"main"`, so no
test-module special case was needed.

It was proved to fire rather than assumed to: its own `violations()` was run over the real parsed
labels and real builder sites with only the capability list swapped for its pre-fix contents, and
produced exactly the two rule A violations the defect had.

What it cannot check is in its module doc — that a window is *covered*, not that the permission set
is *sufficient*. Expanding `core:default` needs `gen/schemas/acl-manifests.json`, which is
git-ignored and built by cargo, while the Contracts job runs on ubuntu-latest with no Rust
toolchain. A capability that covers a window but omits `core:event` would still pass. Only running
the application catches that.

### Still not verified

That an event now arrives. The build proves the capability is well-formed and the permission
exists; `tauri dev` picked the change up and restarted at 11:33. Nobody has yet seen the recording
indicator, the offer panel or the caption. **Every appearance claim in KNEG-163/164/165/167 was made
against a window that could not receive events**, so all of them need re-checking, not just this
one. KNEG-166 carries the list.

---

## 2026-09-04 (KNEG-167) — the box was still there, and the by-hand check earned its keep

The entry below fixed why the corner box was *white*. A person then looked at it — the check
KNEG-166 was filed for — and reported: **transparent with a dark border, and completely empty.**
Two commits followed, `53b19c7` and `84790cf`. This is the entry where the by-hand check paid for
itself twice over, so it is worth being precise about what each observation bought.

### What two sentences from a user settled

*"Transparent with a dark border"* confirmed KNEG-163's background fix, by eye. That was the first
claim in the whole KNEG-163/164/165 set verified by looking rather than by reading, and it needed
no further work.

*"The box is not there directly after launch"*, plus *"only bottom-right"*, killed the leading
hypothesis outright. The suspicion had been that positioning or `always_on_top` was showing a
window built `.visible(false)` — which would have been a much larger defect, affecting all three
panels. Two observations disposed of it: the box is absent at launch, and the dictation overlay at
bottom-centre is never visible, though it is created and positioned identically. Consistent with
the code, checked afterwards: nothing in `crates/platform/src/window.rs` calls `ShowWindow` or
`SetWindowPos`.

`meeting_detect_probe`, run at the same moment, reported Teams **open but not in a call** and
**nothing holding a capture device**. So the window was up while the runtime had nothing to say.

### Every existing pair was already correct, and that was the finding

The instinct is to hunt for a missing `set_visible(false)`. There isn't one. `dismiss` hides;
`recording_started` shows; both `forget` calls follow a hide; both `Idle` emits have a hide on the
next line; `Transition::Withdraw` hides. All checked.

The defect was structural: **visibility was set only by edges, and nothing ever re-checked it.** A
lost, failed or skipped edge was therefore permanent. Three known ways to lose one — `window.hide()`
failing, which is a warning and then forgotten; the hide in `stop` sitting inside
`if ... finished(generation)`, skipped when a stop is superseded; and a detection poll that ended,
after which no `Withdraw` can ever fire again.

That is a class, not an instance, and it is why the fix is a reconciliation rather than a fourth
paired call.

### Why the reconciliation is safe where it sits, and why it only hides

It runs at the end of `poll_for_meetings`, and needs no new lock and no new ordering. The reason is
worth recording because the whole design rests on it: `may_poll` is `matches!(phase, Idle)` and the
`lifecycle` guard is held for the remainder of the function — its own comment says a start cannot
slip in — so **past the early return the phase is provably `Idle`**. Reading the outstanding offer
under `watch` while holding `lifecycle` matches this file's established `lifecycle` → `live` →
`watch` order, and `live` is never needed.

**It hides only.** The symmetric version was considered and rejected: `set_visible(true)` runs at
the *end* of `start`, so forcing the window up whenever a session was active would flash an empty
panel part-way through `Starting` — the same defect from the other side. It would also fight
`dismiss`, whose entire purpose is a visible offer going away while its suppression stays. So the
show path stays edge-triggered, and the brief said so three times because a worker improving it
into symmetry was the obvious failure mode.

One benign interleaving was found and deliberately left: `stop` releases the lifecycle lock when
`finished()` returns and only then hides, so a poll landing in that gap may hide the window a moment
before `stop` would have. The outcome is identical; the cost is a `warn` line that reads as a defect
when it was a race. Guarding it would mean holding the lock across a window operation, which is how
deadlocks get written.

### The frontend half was a defect nobody had asked about

Found while chasing the first one: **none of the three panel windows handled its mount read
failing.** `void getMeetingStatus().then(...)` with no `.catch()` — and the `void` that tells a
linter the promise is deliberately unawaited also discards the rejection. A failed read set no
state, left the component at `null`, and every one of them renders `null` as "nothing to say".
Permanently, silently.

`Overlay.tsx` was the worst case, and the argument is its own module doc: *"the local user is never
in doubt about whether they are being recorded."* A failed read left it rendering as though the
answer were no, on a window Rust only shows *because* a recording is running. It says
`State unknown` now, with the indicator in its non-recording form — asserting a recording it could
not read would be the opposite overclaim, and both are worse than admitting ignorance.

The fallback is gated on having nothing from *either* source, not merely on the read failing: the
subscription is the other half, and state arriving by event afterwards must render normally. In the
meeting window that falls out of the existing branch order for free.

### Process notes

Both slices were independent — no shared type, disjoint files — so no foundation round was needed;
the sequencing was purely a memory ceiling, and the second worker was launched the moment the first
finished rather than after its merge, so its run overlapped the first one's review.

One change in `84790cf` is the orchestrator's, disclosed in that commit: inserting the new branch
into `UpdateNotification.tsx` left a pre-existing comment describing an `if` two blocks below it.
The comment was moved back onto the branch it explains. A comment-truth defect, not a logic one,
and too small to spend the available repair round on — but not the kind of thing this repository
leaves sitting either.

### What is still not verified

Everything about how any of it looks. The gates prove compilation, bundling, audit parity, 1740
passing tests and a clean `clippy -D warnings`. Nobody has made a panel fail its read, nobody has
seen the caption from KNEG-165, and `cargo build --workspace` has now twice been unable to relink
the desktop binary because a running instance holds it — so the final link step is unverified on
this tree. KNEG-166 remains open with the list.

---

## 2026-09-03 (KNEG-163/164/165) — the white box in the corner, and what one sentence was worth

Started from a user sentence — *"what is the white box that shows up in the bottom right corner
that is immovable"* — and ended as four Linear issues and two commits, `b20acaf` and `2f143b57`.
Worth recording because **nothing in it was found by looking at the window**, the fix was designed
and landed the same way, and KNEG-166 is the by-hand check that still has not happened.

### Immovable needed no investigation

`decorations(false)`, `always_on_top(true)`, `skip_taskbar(true)`, `focused(false)`, plus the
imperative non-activating treatment from `overlay::apply_treatments`, and no
`data-tauri-drag-region` anywhere in `apps/desktop/src`. No frame and no drag surface means
nothing to grab. That half was answerable by reading and stayed answered.

### White was three causes, and separating them was most of the work

1. **`body { background: var(--bg) }` painted inside a `transparent(true)` window.** One bundle
   serves four surfaces, so a rule written for the shell lands in the panels. `--bg` is `#fdfdfd`
   in light mode. Unconditional. In dark mode it paints `#181818`, which reads as deliberate —
   which is the likeliest reason it survived this long unremarked.
2. **`.overlay` and `.notify` asked for `height: 100%` with no chain to resolve against.** No
   `html`, `body` or `#root` rule set a height anywhere in `styles.css`, so it computed to `auto`.
   A 92px window held a ~35px panel with background below it, and `.notify-recording`'s inset red
   edge — the thing that makes "recording" read at a glance — was drawn around the content box.
   Its own comment, "the panel is 92px of fixed height", was describing a panel that did not fill
   them. Also unconditional.
3. **The empty state is then entirely background.** `.notify-empty` is transparent with no
   children, so zero height, and its comment already said it only shows "through a stray render".
   With 1 and 2 present, a stray render is a 392×92 rectangle of `#fdfdfd`.

### A correction worth keeping

The first answer given to the user leaned on cause 3, driven by a missed `meeting-offer` event.
That was overweighted, and KNEG-134 is why: it records the offer window appearing on a real Teams
call on 2026-09-01 "with the right words", so delivery demonstrably works at least sometimes and
the race is intermittent at most. Causes 1 and 2 are the load-bearing ones. KNEG-164 was rewritten
to say so rather than leaving the stronger claim standing.

### The race is real anyway, and is now recoverable

`poll` broadcasts `meeting-offer` and *then* shows the window; the read-once on mount recovered
nothing, because `MeetingStatus` carried no offer and `Watcher::outstanding` held only a provider
id; and `consider` returns `Transition::None` while an offer is outstanding, which is correct for
"one offer per meeting, not one per poll" and is exactly what makes one lost delivery permanent for
the length of the call. `UpdateNotification` never had the defect — `get_last_update_check` exists.
That asymmetry between two sibling windows was the argument for the fix rather than any new
mechanism.

Consuming it has a trap worth naming because a later simplification will want to reintroduce it:
`getMeetingStatus()` is awaited and a withdrawal arrives as `null`, so `prev ?? fromStatus` would
resurrect an offer the user had just dismissed. The event has to win outright, via a ref set in the
listener.

### The caption needed no Rust at all

`meeting-transcript` was already broadcast to every window by `app.emit` — a broadcast to all
webviews, not a targeted send — so the window simply had no listener. The round-2 brief said so and
told the worker to report `wrong_premise` rather than invent an event if it disagreed; it did not.

Decisions, with what they were chosen over. **A third line inside the existing panel**, window
unchanged at 392×92 — three 16px lines plus two 0.15rem gaps is ~53px inside a 92px panel that
centres its content. The alternative was the porting matrix's own unbuilt 460×132 "transcription
preview" role, rejected as a larger claim than the request and recorded in KNEG-166 as the fallback
if ~39 characters proves too few. **Newest event wins regardless of source, with no prefix** —
one line at 392px is about 39 monospace characters and a prefix would spend an eighth of it on
attribution `Settings > Meetings` already shows with room. **Replace, never append** — partials
overlap by two seconds. **`text-transform: none`** — the stylesheet's own rule keeps uppercase off
anything the user authored, and this is speech. **Cleared on `recording-without-transcript` and
`stalled`**, not only on outcome: a caption frozen under "live transcript stopped" would claim words
were still arriving.

One edge was considered and left alone: a transcript event arriving *after* the failure state would
put the caption back under the amber line. It is not reachable — `pump` drains its remaining events
before returning, and the failure state is emitted after `pump` finishes — so guarding it would be
code for a case the ordering already prevents.

### `data-window`, not `data-surface`

CSS cannot ask which window it is, and `:has()` on rendered content would flash the wrong background
before React mounts, so the surface is stamped on the root element from `main.tsx` before
`createRoot`. The attribute name is deliberate: `data-surface` is load-bearing for
`verify-parity.mjs` rule 7, which greps component sources for `data-surface="<surfaceId>"` to prove
a declared UI surface exists. The hash mapping now lives once in `lib/surface.ts`, read by both the
stamp and the router, and `App.tsx`'s switch lost its `default` arm — which is what finally delivers
what its comment had claimed since it was written, that a fifth window is a compile-visible
addition.

### Process: `dev` moved under the run, and a lapsed entitlement stopped it dead

Two things worth carrying forward.

`dev` gained three commits (KNEG-160) between preflight and the first merge, and KNEG-160's scope —
`App.tsx`, `styles.css` — overlapped the frontend half's. The merge was clean because the file sets
were disjoint, but the *plan* was stale, so the diagnosis was re-checked against the merged tree
before the second worker launched rather than after. It held: the rail was pinned with
`position: fixed`, which needs no height chain, which is how it added 251 lines of `styles.css`
without touching either defect. The same run also reset the `worker-1` slot out from under KNEG-160's
own in-flight worker; its branch ref survived, but a shared pool wants a liveness check before
allocation.

And both workers first exited 20 with zero-byte patches: `codex exec` returned 404 from
`chatgpt.com/backend-api/codex/responses` on both transports. The identity token refreshes
independently of the entitlement and the model-list route stays open, so it reads exactly like CLI
version skew. It was not — the `id_token` claims gave
`chatgpt_subscription_active_until: 2026-07-11`. After `codex login` the same claim read
`2026-09-11` and both rounds ran normally.

### What this does not establish

Every appearance claim. The gates prove compilation, bundling, audit parity, 1739 passing tests and
a clean `clippy -D warnings`; they prove nothing about how the panel looks. Light mode is the case
that matters, because in dark mode the defect painted `#181818` and would hide its own fix.
KNEG-166 is open for it.

---

## 2026-09-03 (KNEG-160) — a cap that made the window irrelevant, and a class that styled nothing

Two requests arrived together: pin the list of screens to the left edge so the content can be
wider, and stop thirteen screens sitting in one flat list.

### The cap, not the window, was the width

`.shell` was `grid-template-columns: 12rem minmax(0, 1fr)` inside `max-width: 60rem; margin: 0
auto`. Measured: the content pane was **736px in the default 1000×720 window and 736px on a
maximised 4K display**. Every pixel of a larger monitor went into the two margins. That is the
whole defect, and it means "make the content wider" is one deletion — the cap — plus somewhere for
the freed width to stop.

The rail is `position: fixed` now, `.shell` reserves its width with `padding-left: var(--nav-col)`,
and `.content` is what is capped: `--content-max`, 68rem, centred in what is left.

**68rem is derived rather than picked.** The widest layout in the application is `.note-workspace`,
`minmax(0, 1fr) 22rem` — an editor beside a transcript. 44.5rem of editor, `--row-gap`, 22rem of
transcript is 68rem. And the honest accounting, which is in the token's comment: in the default
window the pane goes 736px → ~728px, a wash, because the 60rem cap was already nearly the whole
window. The gain is that it now *grows* — to 1088px, about +48% maximised on 1920px — where before
it was frozen.

Two dead measurements came out with it. `.note-workspace`'s collapse rule was `@media (max-width:
64rem)`, testing the **viewport** while its container was 46rem — so at the 62.5rem default window
it fired every time and **that layout has never shown two columns in the shipped application**.
Re-derived against the real container width to 80rem. And `.shell`'s comment claimed the sidebar
"scrolls with the page rather than being sticky" while the rule twelve lines below it was
`position: sticky`; it was false when it was written.

### Thirteen peers is a list with no shape

Four top-level destinations — Dictation, Meetings, Notes, Upload — plus **Settings** (Providers,
Transcription, Intelligence, Local models, Calendar) and **Developer** (Local API, CLI, MCP,
Diagnostics). All thirteen destinations survive with the label they already shipped; the two group
names are the only new user-visible strings in the change.

The one filing decision worth recording is **Local models**, which was requested beside CLI and
MCP and is in Settings instead. `config/ui-surfaces.json` calls its surface
`settings.intelligence.local` and files it under `Settings > Intelligence`. A rail that disagreed
with the config describing the same screen would be a second, quieter answer to where that screen
lives, and the config is the one a gate reads. Calendar is in Settings for a plainer reason: it is
an OAuth client-ID paste and a sync. Developer is the four surfaces whose audience is another
program.

The old flat list carried an ordering argument — "a key is useless without a shortcut to dictate
with, and both are useless without knowing where the audio goes". That ordered the *setup* path,
and the setup path is now the Settings group in the same relative order. The argument was moved,
not overturned.

`ViewId` is **derived from the tree** by a conditional type, and the view switch took the `never`
guard from `labels.ts:952`. The chain of `{view === "x" ? <X /> : null}` ternaries it replaced was
exhaustive over nothing: an entry with no matching line rendered an empty pane and no error.

### The reversal, and what caused it

A mechanical check — every class the TSX emits has a rule in `styles.css` — found that
**`nav-group` was on every group header and had no rule at all.** A collapsed `Settings` was
pixel-identical to `Dictation`: nothing said one opened a list and the other opened a screen. CSS
has no equivalent of an unused-variable error, so nothing had objected.

Fixing it broke the design. Expansion was first **derived** from the current view: no state, and a
header selected its group's first child. Add a `+`/`−` marker to that and it promises a press that
cannot happen — a derived-open group cannot be collapsed. Add `aria-expanded` and it says the same
thing to a screen reader. And a header that navigates means looking inside `Developer` from
Dictation lands you on Local API, which is the opposite of putting the options where they can be
seen.

So expansion is explicit state in `App.tsx`, a header discloses only, and `groupOf(view)` opens the
group holding a screen however you arrived. That last part is load-bearing rather than tidy:
Transcription and Intelligence both navigate sideways to Providers, and the error boundary resets
to Diagnostics under `Developer` — without it, all three would leave a collapsed rail with nothing
marked. Collapsing the group you are standing in is allowed, which is what a sidebar with sections
does.

**The smaller design was wrong in a way only visible once drawn**, and the check that exposed it
was looking for a dangling class, not for a design error. Worth remembering which one found it.

### What was verified, and by what

`tsc --noEmit` and `vite build` clean; all 14 node gates green with 118 self-test assertions; both
`--check` generators clean. No `config/` file and no screen component was touched — the parity and
settings gates resolve a surface to its *component* and its `data-surface` marker, so a nav
restructure is invisible to them by construction, which was checked by reading them rather than
assumed.

Three mutations, each red on both checks, each restored byte-identically by SHA-256: dropping the
`mcp` case from the switch, adding a fourteenth nav leaf with no branch, and making `groupOf`
always return `null`. The third is the instructive one — `tsc` caught it only incidentally, as an
unused parameter, so the behavioural assertions are what actually prove that function.

No Rust source changed, and the merged tree is byte-identical to the branch tip, so the Rust suite
is unchanged from `d71fcc9` and was not re-measured. Saying so rather than quoting a number nobody
ran this session.

### What did not close with this

- **Nothing here has been looked at.** `tsc` and `vite build` are the entire frontend gate and
  neither renders a pixel, so every claim about appearance — the fixed rail, the `+`/`−` markers,
  the indent, two columns above 80rem, the narrow fallback below 44rem — is believed from reading
  the CSS. KNEG-161 is the by-hand list.
- **`navigation.ts`'s invariants are gated nowhere.** They were asserted by compiling the module to
  a scratch directory and running `node:assert` over it, which proves it today and catches nothing
  tomorrow. That is the KNEG-147 shape one language over: a check that lives somewhere nothing runs
  it. KNEG-162 records the runner decision it needs.

### Codex was unreachable, and the commit gate is what saved the work

This was invoked as a Codex orchestration and no Codex worker ran. `codex exec` returned **HTTP 404
from `chatgpt.com/backend-api/codex/responses`** on both the WebSocket and HTTPS transports, five
retries each — reproduced on a bare probe in an empty directory outside this repository, on the
cheapest model, and on CLI 0.148.0 and 0.153.0 alike. The upgrade to 0.153.0 was tried on a
diagnosis that turned out wrong: the model cache had been refreshed that afternoon stamped
`client_version: 0.153.0` against an installed 0.148.0, which looked exactly like version skew and
was not. The token's own claims give the real cause — `chatgpt_plan_type:
self_serve_business_prolite`, `chatgpt_subscription_active_until: 2026-07-11` — which is
account-side and not fixable from here.

Claude wrote the patch instead, which is a deviation from that skill's division of labour and is
recorded as one. Everything else about the run was unchanged: worktree, base, ownership, gates, and
the commit as the approval gate.

**And the gate earned itself.** Minutes after `fd22135` was committed, another run allocated the
same `worker-1` slot, reset it to base, and wiped the working tree — the slot's HEAD now reads
`codex/20260903-170718/worker-1-offer-status`. Had the work still been uncommitted in that tree it
would be gone. "Claude owns the commit" reads like ceremony until a concurrent run resets the
directory underneath you.

---

## 2026-09-03 (KNEG-159) — every gate green, and the window was blank

The first time anyone opened the merged KNEG-152 tree, `npm run tauri:dev` showed a white window
with nothing in it. Fourteen node gates, `tsc`, `vite build`, the domain scan of the built
assets, clippy, 1734 tests and the mock-webview IPC suite had all passed the same tree an hour
earlier. This entry is about the gap between those two facts, because the gap is structural and
will be there for the next dependency too.

### The cause, measured before anything was changed

The webview and a browser differ in one thing that matters here: Tauri runs every script after
`Object.freeze(Object.prototype)` (`tauri.conf.json` > `app.security.freezePrototype`). Once
frozen, every property a plain object inherits — `toString`, `constructor`, `valueOf` — is
non-writable, and a strict-mode assignment to one of them on an object that has no own property of
that name is a `TypeError`. That was reproduced without the webview: a throwaway page served by the
running Vite dev server with the freeze inline, loaded in headless Edge 152 with console logging,
died at module load with *Cannot assign to read only property 'toString' of object '#<Object>'*.
The same page with the dev CSP applied and no freeze rendered the whole shell, so the CSP — the
thing the whole KNEG-152 design had been careful about — was not the cause. WebView2 and Edge were
the same build, so neither was engine version.

Two modules do the assignment, and they fail differently:

- **`lodash-es/lodash.default.js`** builds the `_` wrapper at load by copying every method onto a
  plain `{}`, `toString` included. Milkdown imports three lodash functions by name; Rollup drops the
  module from the production bundle, but Vite's esbuild pre-bundle for the dev server kept it
  beside `debounce` and `throttle`. It throws before React mounts. **This is the blank window**, and
  it is a dev-only artifact: the packaged application never had it.
- **`@milkdown/transformer`**'s serializer sets `this.toString = …` in its constructor, one line.
  This one is in production too. It does not blank the application — the shell renders — but
  Crepe's `create()` rejects and the editor never mounts. Found by a second probe that mounted
  `NoteEditor` directly under the freeze, which is the check the first probe did not make.

Every pre-bundled chunk was then read for assignments to inherited `Object.prototype` names. Those
two are the whole list. `rope-sequence` and `prosemirror-transform` write `__proto__`, which is an
accessor and still works when frozen; React's dev build writes `constructor` on an object that
inherits a writable one from `Component.prototype`. Nothing else.

### What landed, and why each piece

- **`src/shims/lodash-es.ts`, aliased from the bare specifier in `vite.config.ts`**: the three
  functions from their per-method modules, so the package index — and with it `lodash.default.js`
  — is never evaluated. Applied in build too, so dev and production load the same modules. Narrow
  on purpose: a fourth import fails loudly rather than reopening the package index.
- **A one-line patch to `@milkdown/transformer` 7.22.1**, carried by `patch-package` on
  `postinstall` (`apps/desktop/patches/`). `Object.defineProperty(this, "toString", …)` defines an
  own property, which a frozen prototype permits; the assignment did not. Chosen over turning
  `freezePrototype` off: that is a hardening this project switched on deliberately, Tauri's default
  is off, and giving it up for one upstream line is the wrong trade. `npm ci` fails loudly when a
  future Milkdown no longer matches the patch.
- **`src/lib/devPrototypeFreeze.ts`, imported first in `main.tsx`**: in a plain-browser dev session
  with no Tauri host, freeze `Object.prototype` before the rest of the graph evaluates. This is the
  structural fix. The frontend worker ran the fixtures in a browser and saw a working editor; the
  browser was lying by omission. Now `npm run dev` fails the way the webview fails.
- **`tools/dev-bundle-audit/scan.mjs`**: runs Vite's own optimizer into a scratch cache — the dev
  pre-bundle is a different artifact from the production bundle and nothing gated it — and fails
  on the lodash module's banner or the transformer's unpatched line. Its first draft matched the
  banner's `// node_modules/` prefix and **passed against a pre-bundle that contained the
  offender**: esbuild writes the path relative to the output directory. That is the gate failure
  mode this repository keeps finding — green because it measured nothing — caught here only
  because the rule was run against the known-bad tree before being trusted, and now pinned by
  self-tests over every path shape seen.
- Vue's esm-bundler feature flags are defined in `vite.config.ts`, which silences a dev warning and
  is what Vue asks of bundlers.

### Verified, and the shape of the evidence

Against an isolated Vite instance on the patched `node_modules`, headless Edge with the prototype
frozen rendered the shell, and mounted `NoteEditor` with a heading, a table and list items and
fired `markdownUpdated` — 45 `.milkdown` elements, zero console errors. `tsc`, `vite build`
(the patched line present in the bundle, the unpatched one absent, lodash's full build absent),
the domain scan and the new gate are green. **Not verified:** the Tauri window itself with these
changes, because the running `tauri:dev` session predates them and must be restarted; the by-hand
script is KNEG-158.

---

## 2026-09-02 (KNEG-152) — a Markdown editor on the note, and the transcript kept out of it

The Notes screen rendered a note's body as read-only text. KNEG-152 replaces that with the
Milkdown Crepe editor, autosaving, and — for a note recorded from a meeting — a transcript panel
whose rows play the matching stretch of the WAV. Six decisions are worth the record, three of them
measured rather than argued.

### The transcript is not in the Markdown

The obvious design embeds timestamps in the note text. It was rejected before a line was written:
editing splits, merges and deletes paragraphs, and every one of those operations silently breaks a
link carried inside the text. The stored segments (`notes.transcript`, a `Vec<transcript::Segment>`
with `timestamp`/`end_timestamp`) already carry the timing; what was missing was a durable
statement of *which file* that timing refers to. That is the whole of the new table:
`note_recordings` (migration `0002`), one row per note, holding a bare file name inside
`data_dir()/meetings`, the duration, an original filename where one exists, and nothing else.
Rows, not phrases: word-level timing is not retained consistently across providers, and estimating
a phrase's interval from character positions would feel precise and often be wrong.

### The renderer names a note id, never a path

The brief suggested Tauri's asset protocol behind a narrow filesystem scope. That would have worked
and was still the worse shape: the renderer supplies a path, a Cargo feature has to be enabled, and
a scope glob has to describe the data directory. What landed is a custom `recording` URI scheme
(`apps/desktop/src-tauri/src/recordings.rs`): the `<audio>` element asks for `/<note_id>`, Rust
looks the row up, `resolve` accepts only a plain file name — no separators, no `.`/`..`, no
surrounding whitespace, `file_name() == key` — and joins it onto the recordings directory. Nothing
crosses the IPC boundary in either direction, `capabilities/default.json` stays at `core:default`,
and the CSP gains one directive, `media-src 'self' recording: http://recording.localhost`, spelled
the way `img-src` already spells the asset scheme.

Range requests are not optional: Chromium's media loader seeks by issuing them, and without `206`
a whole recording is read into memory per request and the scrub bar does nothing. `response_for`
mirrors Tauri's own `protocol/asset.rs` — single range, 1 MiB cap per chunk, `416` with
`bytes */len` for anything unsatisfiable, `HEAD` answered with the `GET` headers — and is pure apart
from the file read, so its fourteen tests need no Tauri runtime. Multi-range headers are ignored
and answered with the whole file, which HTTP permits and nothing here needs.

### Rust computes the rows, so the screen and the exports cannot disagree

`transcript::rows` normalises epoch-millisecond timestamps the way export does, resolves speakers
through the same `identified_speaker` export uses, and emits one row per stored segment without
`merge` — a merged paragraph would play the wrong stretch. A live-partials transcript has no
`end_timestamp` on any segment, so a missing end falls to the next row's start and the last one is
`None`, meaning "to the end". A second copy of that arithmetic in TypeScript is exactly the drift
the generated IPC types exist to prevent, so there is none.

### Two Crepe features are out, and the reason was measured before the dependency was added

A probe bundle of the full `Crepe` class was built and read before `@milkdown/crepe` went into
`package.json`: 1.49 MB of JavaScript plus ~1.5 MB of KaTeX fonts, none of
`config/forbidden-domains.json`'s patterns, and exactly one runtime `<style>` injection — CodeMirror's
`style-mod`, which the production CSP's `style-src 'self'` blocks. The editor is therefore
assembled from `@milkdown/crepe/builder` with eight explicit features and per-feature CSS imports;
CodeMirror and LaTeX are left out (KNEG-156 records that as a decision), and code blocks render as
plain `<pre>`. The check is repeated on the built bundle: the only `createElement("style")` in
`dist/assets/*.js` is react-dom's hoistable-style path, present in the base bundle with the same
single occurrence. The JS bundle grows from 296 KB to about 1 MB minified. Vue, which Crepe's
components run on, injects nothing.

### Autosave is serialised, and the editor is uncontrolled

`lib/useAutosave.ts` debounces at 800 ms and allows one save in flight: a newer edit waits and then
runs with the latest patch, so a slow older response can never overwrite newer typing; a failure
keeps the unsent patch behind a Retry button and never retries on its own. The editor reads its
initial Markdown once and the view re-reads the list from Rust after every save, which is the
pattern every other screen already follows (`useRefresh`'s header says why). Deleting a note now
asks first, and the question names the recording that goes with it.

### How it was built, and what that proved

This was the first real run of the `/codex-orchestrator` skill: two Codex workers in the warm
worktree pool, one on the Rust slice (`gpt-5.6-sol`), one on the frontend (`gpt-5.6-terra`), each
against a base commit (`3110b12`) that pre-wired the shared surface — the migration, the two
`TS`-derived boundary types, the regenerated `ipc-types.ts`, the `ipc.ts` wrappers and the npm
dependencies. Disjoint write surfaces meant both merges were conflict-free. The frontend needed one
repair round for eight mechanical findings; the Rust slice needed none. Two things the run
measured about the harness itself: a worker cannot compile `knegiswhisper` until its slot has a
frontend build, and the harness's quota detection fired on a docs line the model had read — fixed
to require an actual codex failure.

### Not verified, and said so

Nobody had opened the application with this change when this was written. It was opened the next
morning and the window was blank — see the 2026-09-03 (KNEG-159) entry above for why every gate
here was green anyway. The CSP claim rests on the bundle checks above and was **not** the cause;
the seek behaviour rests on Tauri's asset protocol doing the same thing for years. Both remain
believed, not observed, and the by-hand check is a sub-issue of KNEG-152. Known gaps filed under
it: the local API's `DELETE /notes/{id}` orphaned the WAV (KNEG-153; fixed 2026-09-05), notes saved
before today have a WAV and no way to attach it (KNEG-154), imports are transcribed and discarded
(KNEG-155), and an attach that fails after the note is saved is only a log line (KNEG-157).

Measured on the merged tree (`111f1e0`), not on the slices:

    14 node gates                                                  all green
    npx tsc --noEmit / npx vite build / domain scan of dist         clean
    cargo fmt --all --check                                        clean
    cargo clippy --workspace --all-targets -- -D warnings          exit 0
    cargo test --workspace                     1734 passed, 0 failed, 12 ignored (1704 before)
    cargo test -p knegiswhisper --lib --features tauri-test        355 passed
    cargo build --workspace --exclude knegiswhisper                exit 0
    cargo build -p knegiswhisper --lib                             exit 0
    cargo build --workspace (desktop binary relink)                blocked: knegiswhisper-desktop.exe
                                                                   was running from a `cargo run` started
                                                                   17:44, before the merge; everything it
                                                                   links was compiled by the two lines above

---

## 2026-09-02 (KNEG-151) — a doc comment asserted the surface was closed, and now something checks

`DialogGrants`'s doc says of the import commands: *"the only three commands that take a path, so the
surface is closed."* That sentence closed KNEG-80 and two things downstream leaned on it. Nothing
checked it.

KNEG-151 is what happened when it started to drift. `media.rs`'s IPC boundary test named the three
commands in a `generate_handler!` it built itself, and named them **again** in its own assertion
loop. So it proved those three functions refuse an ungranted path, and nothing whatever about the
application's import surface: a fourth path-taking command was in neither list, and dropping one
from `lib.rs:388` left every assertion about it green.

### The audit came first, because it decided the design

Before changing anything, every registered command was read for a renderer-supplied path.
**Exactly three, all three already authorising.** `note_export_to_file` looked like a fourth and is
not — it takes `id` and `format` and opens its own save dialog, so the path never crosses the IPC
boundary; `pick_import_file` is the grant *source*. So the list was not wrong. What was wrong is
that nothing kept it right, which is a different defect and wants a different fix.

### One list, and why not the macro the issue asked for

The issue proposed extracting `lib.rs:388`'s `generate_handler!` so the app and the test share
tokens. That was tried on paper and rejected: `generate_handler!` needs literal paths at expansion
time, so sharing the *import subset* means a callback macro that also has to carry the other 93
commands through it. A ninety-six entry list restructured into macro arguments is a large, risky
diff, and it buys only one of the two directions.

What landed instead is `commands::PATH_TAKING_COMMANDS` — the command's IPC name and the dotted
position of its path argument, once — read by three things:

- **The boundary test drives every entry** over real IPC, building the JSON body from the dotted
  shape. `import_openwhispr` wraps its path in an `ImportRequest` and the other two take the string
  directly; writing those two shapes out by hand is exactly what made the old loop a second copy of
  the list. A fourth entry is asserted by being added.
- **`every_path_taking_command_is_registered`** ties the list to `lib.rs` by reading it with
  `include_str!`. Rust has no reflection over `#[tauri::command]` registration, so text is the only
  way in — the same parse `tools/settings-audit/scan.mjs` already does one language over.
- **`tools/command-audit/scan.mjs`** fails when a registered command takes a renderer path and is
  **not** in the list. That is the direction neither test can see, and the reason a node gate exists
  for it at all.

One consequence of that shape, worth recording because the easy answer was wrong: the list has no
consumer outside tests — the application enforces the grant inside each command, it never
enumerates them — so `-D warnings` called it dead code. `#[cfg(test)]` on the const and
`#[cfg(feature = "tauri-test")]` on the body builder say that plainly, and the node gate is
unaffected because it reads the source as text. An `#[allow(dead_code)]` would have silenced the
lint rather than answered it, which is the same call as the build script in the entry below.

### Three mutations, each confirmed real before being believed

- Delete `media::media_import_video` from `lib.rs`'s `generate_handler!` → `every_path_taking_command_is_registered` fails, naming the command and both remedies. This is the mutation that was invisible before.
- Change `import_openwhispr`'s shape from `request.path` to `path` → the boundary test fails with
  *"missing required key request"*, so the dotted shape is load-bearing rather than decoration.
- Drop `grants.authorise` from `media_import_audio` → the boundary test fails, and instructively:
  the command runs *past* the absent guard and dies at ffmpeg instead. The test is detecting a
  missing check, not a coincidence.

Each mutated file was SHA-256'd before and after restoration. That is not ceremony —
`ENGINEERING_LOG.md` § 2026-09-01 (KNEG-131) records a harness whose `shutil.copy2` preserved mtime
so cargo never rebuilt, and a mutation appeared to survive that had never been compiled.

### The gate's first draft passed, and was wrong

It printed **two** path-taking commands where there are three. `import_openwhispr` was missing —
the one whose path reaches `kw_storage::import::from_openwhispr` and its `exists()` check, which is
to say the one the threat model cares most about.

The cause was a comma. `ImportRequest::path`'s doc comment reads *"the source `transcriptions.db`,
as […] returned it"*, and the field splitter treated that comma as a field boundary. The resulting
"field" ended in `pub path`, so the name test saw a blob instead of a name and the struct-following
case silently did not fire.

**It was caught by comparing the printed count against the hand audit, not by the gate going red.**
It never went red. That is the whole argument for printing the counts that would make a gate
vacuous on every successful run, and for doing the audit by hand first: without a number to compare
against, "OK — 2 take a renderer path" reads exactly like success.
`the_struct_following_case_is_found` is the regression, and it asserts the full set of three rather
than just the one that was missed.

### What did not close with this

- **`authorise` is read for, not placed.** The gate checks the call appears in the body, not that it
  precedes the use of the path. The four pure policy tests and the boundary test cover the
  behaviour; a gate that understood order would need a parser rather than a scanner.
- **A path under a name the scanner does not recognise.** `PATH_NAME` is deliberately generous —
  `path|file|dir|dest|target|location|filename` — because a false positive costs a line of argument
  and a false negative is the defect. A command taking `name: String` and treating it as a filename
  is still missed, and that is the honest limit of reading signatures as text.
- **The test's own `generate_handler!` is still its own.** It has to be, for the expansion-time
  reason above. An entry added to the list and not to that list fails on the assertion, carrying the
  real Tauri error, which is loud enough.

Measured: `cargo test --workspace` — **1705 passing, 0 failed, 12 ignored** across 55 blocks
(1704 + the registration test); the feature-on invocation **341 passing**; `node
tools/command-audit/scan.mjs` — 95 registered, 3 take a renderer path, 3 declared; its self-tests
8 passing; all 14 node gates green.

---

## 2026-09-02 (KNEG-147, KNEG-150) — the better fix was refused by cargo, and running the
gate list found a gate that was already red

`2a5af33` added `every_import_command_enforces_the_dialog_grant_at_the_ipc_boundary`, which drives
real IPC requests through Tauri's mock webview and asserts all three import commands refuse an
ungranted path. It is good coverage of a boundary that had been untestable, and the note closing
the issue it was written for said the gap was now "permanently covered".

**It had never run once.** The body was `#[cfg(feature = "tauri-test")]`, and that feature was
declared in one line of one `Cargo.toml` and named by nothing else in the repository — no workflow,
no script, no document. Measured: with the feature on, the lib block is **340 passing**; with it
off, **339**, and `grep -c every_import_command_enforces` over a whole workspace run returns **0**.

Two consequences, and the second is worse. The assertions did not run. And because the module was
not compiled at all, `cargo clippy --workspace --all-targets` never saw it either, so the test
could have stopped compiling without anything failing. That is how feature-gated coverage usually
dies — not with a red run, but by being dropped from the build graph.

### The fix that should have been right, and the exact reason it is not

The obvious move is to delete the feature and make Tauri's mock runtime a **dev-dependency**:

```toml
[dev-dependencies]
tauri = { workspace = true, features = ["test"] }
```

That looks strictly better on every axis. `cargo test --workspace` and `cargo clippy --all-targets`
both compile and run the test with **no flag to forget**, which closes the worse of the two
consequences by construction rather than by discipline. It needs no CI edit at all. And it makes
the `Cargo.toml` comment's intent — *"Never enabled by the application build"* — a property instead
of a convention, because a dev-dependency cannot be switched on in a release build whereas a
feature can. The workspace is `resolver = "2"`, the version that keeps dev-dependency features out
of non-test builds, so that isolation is real and not hoped for.

It was implemented, and cargo refused it:

```
error: invalid instruction `cargo:rustc-link-arg-tests` from build script of `knegiswhisper`
The package knegiswhisper does not have a test target.
```

The reason the feature exists at all is that `build.rs` must know whether it is building the test
executable. Tauri's mock runtime links dialog support that imports `TaskDialogIndirect`, so the
test binary needs a Common Controls v6 manifest — and that manifest must **not** reach the shipped
application. The un-suffixed `cargo:rustc-link-arg` applies to every target in the package, which
is why the committed version had to strip Tauri's own manifest with `new_without_app_manifest()`;
harmless while the feature was never on for an app build, and unacceptable the moment it is always
on. `cargo:rustc-link-arg-tests` is the only key that expresses the needed scope, and as the error
says, it means a `tests/` target. This is a `--lib` unit test.

**So the remaining route was to move the test to `tests/`, and that is worse than the problem.**
Every one of the 27 modules in `lib.rs` is private — `mod`, not `pub mod`. An integration test
would need `commands`, `state` and `media` made public, plus `MAX_DIALOG_GRANTS` and
`DialogGrants::grant` widened from `pub(crate)`, plus a reconstruction of the `runtime()` helper
that only exists under `cfg(test)`. Opening up a deliberately closed crate to relocate one test
buys nothing the flag does not, and costs the encapsulation permanently.

**Decision: the feature stays, and the reason is now written in `Cargo.toml` beside it** — because
the next person to look at this will try the dev-dependency, and the argument against it is a
cargo error message they will not guess.

### Paying the cost the feature has

A feature's cost is that an invocation can forget it, and for a while every invocation did. So the
flag went into both places that run — `ci.yml`'s rust job and the by-hand list in `README.md` — and
then `tools/feature-audit/scan.mjs` was added so that forgetting fails loudly: every feature
declared in a `[features]` table must be named by some CI invocation, or be exempt in
`config/cargo-features.json` with a reason.

This is `tools/settings-audit/scan.mjs` one layer down, and the parallel is exact. There, a setting
the backend honoured that no shipped screen could change (KNEG-104). Here, a test the repository
declared that no invocation could run. In both cases the artefact existed, had a green suite around
it, and was unreachable — and in both cases the suite was no help, because a suite reports on what
it ran.

**It was checked against the defect it was written for, not only against a clean tree.** With
the `--features tauri-test` dropped from the step but the step left in place — the shape a
careless edit would produce — `node tools/feature-audit/scan.mjs` exits 1 naming
`knegiswhisper/tauri-test` and the remedy. `ci.yml` was restored byte-identically afterwards,
verified by SHA-256, and the gate is green again. So this gate would have caught KNEG-147 on the
day it was introduced, which is a different and stronger claim than "the gate passes".

**The gate's own self-test found a hole in the gate**, which is the reason these self-tests exist.
The first draft dropped a line only when its first non-space character was `#`. The test mutated
the real workflow by commenting out the command — producing `run: # cargo test … --features
tauri-test` — and the gate still counted it, because the line still began with `run:`. A gate
against dead checks that accepts a commented-out check is the same defect one level up. Fixed with
a quote-aware `stripComment`, and the direction of its remaining approximation is deliberate: an
unbalanced quote over-strips and fails rule A loudly, where under-stripping would pass silently.

### Running the gate list found a red gate nobody had run (KNEG-150)

This was not read out of the code; it fell out of running the checks before committing.
`cargo clippy --workspace --all-targets -- -D warnings` **exits 101 on `dev`**:

```
error: used `expect()` on a `Result` value
  --> apps\desktop\src-tauri\build.rs:12:9
   = note: requested on the command line with `-D clippy::expect-used`
```

`Cargo.toml:171-173` denies `unwrap_used`, `expect_used` and `panic` across the workspace, and
says why: this application runs unattended and a panic is a crash the user cannot diagnose. A
build script is inside that policy and nothing exempted it. `git log` on that file has two
entries and the newest is `2a5af33` — **the same commit that introduced KNEG-147.** One commit
added a test that never ran and turned a gate red, and neither was noticed.

It hid because the `.expect` is inside `#[cfg(windows)]`: on macOS and Linux the block is
compiled out and clippy is clean, so only `windows-latest` would have gone red, one job of
three. And CI has not run since well before it.

Fixed by making `main` return `Result<(), Box<dyn std::error::Error>>` and using `?`. **No**
`allow` **was added**, deliberately: a build script does not need an exemption from the panic
policy, because cargo prints an `Err` from `main` and fails the build exactly as it would a
panic. Silencing the lint would have been the easier edit and the wrong one — the policy is
right, and the call really was fallible.

**Worth stating plainly: this pass found the red gate only because it ran the gate.** A green
claim in a document, a closed issue, and a passing local run of a *different* command are three
things that all look like evidence and are not. That is KNEG-147 one level up.

### What did not close with this

- **`cargo clippy --all-targets` still does not lint the boundary test**, since clippy runs without
  the feature. The compile check is now covered by the new test step; the lints are not. Cheap to
  add and it doubles a clippy pass, so it was left. Note that test modules here carry blanket
  `#[allow]`s anyway, which is most of what clippy would have said. Measured while fixing
  KNEG-150: `cargo clippy -p knegiswhisper --lib --features tauri-test -- -D warnings` is
  green at this commit, so the gap is "not enforced", not "unknown".
- **The test builds its own handler list.** `media.rs:1478-1482` calls `generate_handler!` with the
  three import commands rather than sharing `lib.rs:388`'s registration, so it proves *those three
  functions* enforce the grant — not that the application's import surface does. A fourth import
  command added without a grant check would not be noticed, and dropping one of the three from the
  app leaves this test green. Filed as KNEG-151.
- **CI still does not run.** `push` and `pull_request` have been commented out by Actions budget
  since before any of this, so what landed is a correct entry in the declared gate list and a
  correct entry in the by-hand list. Which of those two is doing the work today is the by-hand one.

Measured on the merged tree: `cargo test --workspace` — **1704 passing, 0 failed, 12 ignored**
across 55 blocks, unchanged, plus **340 passing** in the feature-on invocation; `node
tools/feature-audit/scan.mjs` — 1 feature declared, 1 enabling invocation, 0 exempt; its self-tests
13 passing.

---

## 2026-09-02 (KNEG-149) — the decisions that landed in seventeen commits

`e10bc85..2a5af33` is seventeen commits. Two edited a document as part of their product change;
none added the decision record below. This entry is deliberately not reconstructed from diff shape
alone. Each choice and rejected alternative comes from the corresponding Linear issue's problem
statement and implementation note, then is checked against the merged source. Where only the
automatable half landed, that boundary is kept: KNEG-44, KNEG-52 and KNEG-108 are still open.

### A status document describes this build, not a platform's possibilities (KNEG-138, `dac5f22`)

The text-insertion table used to list `CGEvent`, XTEST/`xdotool`, and Wayland portal mechanisms as
though the application implemented them. The backend already answered `unsupported` on every
non-Windows platform. The chosen shape is therefore an implementation-state table: Windows says
what this build does; macOS, X11 and Wayland say plainly that no paste backend is written.

The alternative was not to delete the platform research. It remains in `PORTING_MATRIX.md`, where
candidate mechanisms belong. Keeping it in `PLATFORM_SUPPORT.md` with softer wording was rejected:
a reader cannot reliably distinguish a proposal in a support table from a shipped capability.

### A deferred interface needs evidence the gate can read (KNEG-140, `d2618c5`)

The parity gate used to require a prose `interface` field and never resolve it. That let a deferral
name `notes_core::search::SemanticIndex`, a module that has never existed, for months. Deferred
entries now carry one or more `interfaceEvidence` pairs: a repository-relative file and a non-empty
literal that must still occur in it. The reader fails closed when it is absent, the path escapes the
repository, the file disappears, or the literal moves. All five current `temporarily-deferred`
entries carry evidence; four integrations are unreachable, which is a separate parity count.

A `checkedAt` expiry was considered and not used. A date can be advanced without reading anything,
so it would turn review theatre into a green gate. A naked Rust-looking path was also rejected: the
gate would need to guess how prose maps to modules, traits and deliberate absences. The small
path-plus-literal contract catches the two failures already observed without claiming semantic
proof it cannot provide. It proves that the stated interface evidence remains present, not that the
whole deferral rationale is still correct.

### Product availability cannot be a test precondition (KNEG-137, `7153ee1`)

Three freshness tests used the real sidecar catalogue. Because whisper-server is deliberately not
published for macOS, the two positive controls returned before asserting and the remaining test
reached `NotInstalled` through the wrong branch. The fix uses a synthetic catalogue that publishes
a valid fake asset for all five supported targets and still drives the real wrapper, path lookup,
record I/O and freshness decision. Seven tests now cover `NotInstalled`, `Current`, `Stale`, and all
four `Unrecorded` reasons without depending on a third party's release matrix.

Publishing a fake product asset was rejected: it would make the application claim support that does
not exist. Merely printing "skipped" was also insufficient; that makes vacuity audible but leaves
the decision table uncovered. The fake belongs to the test input, where it can make coverage
portable without changing product availability.

### A union nobody consumes should be removed, not renamed (KNEG-139, `97f166e`)

`unregistrable_or_unhandled` combined two unrelated facts: the cross-platform registrar would not
accept an accelerator, and no implementation handled an action. The shipped dictation shortcut is
in the first set and works through the Windows hook; a chat shortcut could be in the second set and
register successfully. No Rust or TypeScript caller consumed the union.

Another rename was rejected because no truthful single name exists and no caller needs the answer.
The field, computation, fixture value, generated IPC member and assertions were deleted. The useful
signals remain separate: `unhandled` carries why an action is dead, conflicts and cautions describe
configuration, and the registration result says what is live.

### Host-dependent network evidence is an ignored test, not a universal claim (KNEG-92, `c30e1cc`)

The SSRF boundary needed proof at a real socket, including the exemption it intentionally grants to
self-hosted endpoints. The chosen probe discovers this machine's active private IPv4 address,
listens there, drives real HTTP and a TLS ClientHello, and checks that non-self-hosted purposes are
refused before the listener sees a connection. It also checks policy decisions for CGNAT and
`0.0.0.0/8`, and fixed the refusal text so a plaintext-scheme error names its target host.

The probe is `#[ignore]` because opening host listeners is not a hermetic workspace-test property.
Fabricating routes or peers for CGNAT and `0.0.0.0/8` was rejected: that would test Windows routing,
not this policy. The recorded result therefore claims real LAN reachability where a listener existed
and policy admission/refusal for the synthetic ranges, never reachability the host did not possess.

### An offer is a call lifecycle, not a client-process flag (KNEG-134, `0c81f65`)

Teams normally remains open between calls. Keying `offered` to the process both left a stale offer
visible after hang-up and suppressed the next call. `Watcher` now returns an explicit `Offer`,
`Withdraw`, or `None` transition. Same-call suppression and the currently visible provider are
separate state: dismissing clears the window but suppresses only that call; hang-up withdraws and
releases suppression; recording start clears any offer behind the recording indicator. One poll
emits one unambiguous transition, so a second provider cannot silently overwrite the first.

Adding another hide call in the window layer was rejected because it would cure only the visible
symptom and leave re-arming broken. Clearing `offered` on dismissal was also rejected because the
same call would be offered again at the next poll. Both symptoms needed one call-scoped model.

### A warm streaming server is leased and bounded (KNEG-132, `e1041f2`; KNEG-144, `4d0ac15`)

Restarting a 630 MB model between adjacent meetings wastes its load time; keeping it until process
exit wastes memory and leaves a stale process indistinguishable from the live one. The selected
middle ground matches the local-LLM precedent: a meeting acquires an opaque generation-bound
`StreamLease`, release starts a five-minute idle timer, and a later acquisition invalidates that
timer. Lease and process state share one lock so an old timer cannot decide "still idle" and stop a
new meeting's server between the check and the take.

Immediate stop was rejected because the existing same-model warm reuse is deliberate. An unbounded
cache was rejected because it has no owner or release point. A bare delayed task without a
generation was rejected because its stale completion could kill a later session. Every start,
configuration, connection, recorder-start, stop and cancel path either transfers or releases the
lease.

The release point is the end of the live pump, before the independent whole-file transcription
pass. Releasing after offline processing was rejected: that work can take minutes and uses neither
the WebSocket nor the streaming model, so it must not postpone the five-minute clock (KNEG-144).

### `None` in the live slot is not the same thing as idle (KNEG-143, `5848ff6`)

Stopping removes `Live` before the potentially long whole-file pass. Treating that empty slot as
idle let a new meeting start or a new offer appear, after which the old completion could emit Idle
and hide the shared window. The runtime now has an explicit `Idle` / `Starting` / `Recording` /
`Finishing` lifecycle. A start reserves the lifecycle across provider setup; stop moves it to
`Finishing` before taking the resources; both new starts and offer polling are refused until that
session completes.

A generation accompanies the phase. It is deliberately a second defence, not permission for two
meetings: even if a future caller relaxes the no-overlap rule, an older completion cannot publish
Idle, hide a newer window, or clear newer state. Keeping only a `finishing: bool` was rejected
because it cannot identify which asynchronous completion owns the transition. Keeping `Live` in
place until post-processing returned was rejected because those resources have already ended and
the long work needs its own honest state.

### Reconnects and gaps are different evidence (KNEG-52, `9aeb4d5`; KNEG-145, `b5ab930`)

OpenAI Realtime has a hard session limit, so a scheduled reconnect can be successful and expected;
an audio gap is lost work. They remain separate fields and separate sentences. `9aeb4d5` carried
completed reconnects through `MeetingOutcome::Saved`, generated IPC, fixtures and the visible
outcome. That was an observability prerequisite, not completion of KNEG-52: the authorised 62–65
minute live run still has not happened.

The first implementation copied the count only when transcript segments existed. That alternative
lost the very evidence an all-silence meeting could still produce, so `b5ab930` added reconnects to
`NothingTranscribed` as well. An empty transcript means no words were emitted; it does not mean the
provider performed no session work.

### A saved note keeps observations, not diagnoses (KNEG-133, `de009d9`)

The recorder already knew whether each capture source delivered anything but discarded the facts
on the successful-note path. Each source now records one of four observations: not captured, opened
but no blocks delivered, blocks containing only digital silence, or at least one non-zero sample.
Both microphone and system-audio facts are appended to the saved note.

A Boolean was rejected because "nothing" and "digital silence" are operationally different. A
sentence blaming clipping, echo cancellation, a muted device or a failed capture was also rejected:
the run that found the defect could not distinguish those causes. The note says only what the
recorder measured, preserving the evidence without inventing an explanation.

### Note body and transcript preview are two different artefacts (KNEG-136, `8f223e6`)

Meeting provenance was stored in `Note.content` while every Notes surface rendered only an exported
transcript. The Notes screen now has accessible Read note / Close note controls and a semantic,
paragraph-preserving body region for the canonical note content. Empty bodies are stated explicitly.

Folding this text into the transcript export preview was rejected. The body contains provenance and
failure disclosures about how the transcript was produced; the preview is a selectable rendering
of transcript segments in TXT, Markdown, JSON, SRT or VTT. Making one masquerade as the other would
either change exported formats or keep the saved artefact undiscoverable.

### Absence of identity is not a speaker named "Unknown" (KNEG-135, `d5f2667`)

Whole-recording transcription has neither capture-source attribution nor diarisation by default.
Repeating `Unknown speaker` on every cue therefore added no information to the common path. Export
now asks whether the stored fields carry a real identity; anonymous segments omit the label in text,
Markdown, SRT and VTT, and use `null` in JSON. Typed names, diarised identities, microphone and
system identities remain, and anonymous segments remain in segment counts but not speaker counts.

Comparing rendered text with the localised `Unknown speaker` label was rejected. A user may type
those words as a real name, and localisation is not identity metadata. The stored speaker, typed-name
and source fields decide whether an identity exists.

### Shared security evidence gets one renderer and a fresh read (KNEG-146, `d58be7b`)

CLI and MCP consume the same discovery file, but only the CLI screen showed its path, read-back
permissions, evidence and administrator caveat. `McpStatus` now carries the same fresh permission
read, and both screens render one `DiscoveryFileRows` component through the same label helper.

Copying the CLI markup into MCP was rejected because the next wording or caveat change could drift
again while both screens continued to compile. Remembering the startup verdict was also rejected:
the status is evidence about the file now, so each status request reads it now.

### Work-area placement is platform evidence, with an honest fallback (KNEG-44, `c6d7450`)

Windows now reads `MONITORINFO.rcWork`, which excludes taskbars and docked app bars. Meeting and
update notifications use a 24-pixel bottom-right margin; dictation uses deterministic bottom-centre
placement. Arithmetic stays in physical pixels until placement so mixed-DPI coordinates and
negative monitor origins are preserved.

Using full monitor bounds plus a guessed taskbar margin was rejected on Windows because the OS
already exposes the usable rectangle. Pretending the same API exists elsewhere was also rejected:
other platforms retain Tauri's monitor-bounds fallback and log the weaker evidence. KNEG-44 remains
open for the exclusive visual check; unit arithmetic and a real `rcWork` read do not prove where a
person sees all three windows.

### Mock IPC proves the import policy; the OS picker remains by hand (KNEG-108, `2a5af33`)

Pure `DialogGrants` tests proved the policy but not that the exported Tauri commands consulted it.
The command functions were made runtime-generic and a Tauri mock-webview test now invokes
`media_import_audio`, `media_import_video`, and `import_openwhispr` by their IPC names. An existing
ungranted canary is refused by all three; one granted invalid audio path reaches the later sidecar
error twice, proving that failure does not consume the session-lifetime grant.

Keeping a devtools-console simulation as the only boundary check was rejected because this part is
deterministic and repeatable in-process. Enabling Tauri's test runtime in production was also
rejected. It sits behind `tauri-test`, and Windows test executables get a Common Controls v6
manifest while the production build retains Tauri's original manifest.

The boundary of the evidence matters. The test hand-builds the three-command handler list and the
feature is not exercised by the normal workspace command; KNEG-147 tracks enforcement and handler
drift. KNEG-108 remains open for the real OS picker, renderer retry and lossy-path observations.

### An unnamed failure earns a frozen hunt, not a story (KNEG-141; no product commit)

One workspace run failed once while its tree was being edited, without preserving the test name;
eight immediate reruns were clean. It was not dismissed because KNEG-88's similarly rare failure
had exposed a real vacuous credential test. It was also not attributed to concurrent rebuilding,
because that explanation was plausible and unmeasured.

The settling run used a locked detached worktree at `2a5af33` and a dedicated target. A full locked
workspace baseline ran first because the hunt harness's internal control exercises only its first
discovered binary. The frozen 400-round run then executed 36 binaries 14,400 times across thread
counts 1, 2, 4, 8 and 16, with zero failures, a passing control and unchanged binary fingerprints.
The result is **not reproduced at depth 400**. It bounds how common the failure can be; it does not
prove there is no flake. Ignored network, model, real-server and hardware tests were outside it.

### Commit-to-decision index

This index is the coverage check for the landing range; the sections above are the record.

| Commit | Decision recorded under |
| --- | --- |
| `dac5f22` | KNEG-138 — support docs report the implementation, not candidate mechanisms |
| `d2618c5` | KNEG-140 — deferrals carry repository-readable interface evidence |
| `c30e1cc` | KNEG-92 — a host-dependent SSRF probe makes narrow, repeatable claims |
| `0c81f65` | KNEG-134 — offers transition with calls rather than client processes |
| `8f223e6` | KNEG-136 — canonical note content is rendered apart from transcript preview |
| `de009d9` | KNEG-133 — saved notes retain source-delivery observations |
| `d5f2667` | KNEG-135 — anonymous cues do not invent a speaker identity |
| `97f166e` | KNEG-139 — the unused, untruthful union report is deleted |
| `e1041f2` | KNEG-132 — local streaming is owned by generation-bound leases |
| `9aeb4d5` | KNEG-52 — successful scheduled reconnects are visible and distinct from gaps |
| `7153ee1` | KNEG-137 — freshness coverage uses a cross-platform synthetic catalogue |
| `c6d7450` | KNEG-44 — Windows placement uses the real work area; visual proof remains open |
| `5848ff6` | KNEG-143 — explicit phases and generations serialize meeting completion |
| `4d0ac15` | KNEG-144 — streaming becomes idle before independent offline processing |
| `b5ab930` | KNEG-145 — quiet outcomes retain completed reconnect evidence |
| `d58be7b` | KNEG-146 — CLI and MCP share live discovery-file evidence |
| `2a5af33` | KNEG-108 — import commands are exercised through mock IPC; picker proof remains open |

---

## 2026-09-01 (KNEG-131) — three tests had been green on macOS for weeks by checking nothing

KNEG-117 found the shape and hardened two instances of it: **an assertion that something is absent,
checked against a resource that may not have been fully written.** This is the same search run over
the tests the last wave added. The interesting half is not the seven sites that were hardened; it is
that the worst instance was not that shape at all.

### The arithmetic first, because the issue's was wrong

The issue says ~190 new tests. Measured three ways — diff hunks, and `git grep -c '#\[test\]'` at
each end of `aa3748a..bcc57f8`:

**182 gross, 54 of them relocations, 128 genuinely new, +128 net** (1531 → 1659). The 54 are the
`meeting.rs` split: 26 to `config.rs`, 19 to `recorder.rs`, 7 to `watcher.rs`, 2 to `notes.rs`, every
one name-for-name. `notes.rs` is the only one of the four that also gained tests — 8 of its 10 were
written. Planning's own correction (185 / ~131 / +129) was closer and still wrong.

The issue also says to search TypeScript with `expect().not`. **There is no TypeScript test suite.**
The four `tools/**/*.test.mjs` files use `node:assert`, and the wave touched one of them.

### Four buckets, 58 sites

The sweep matched eleven negative-assertion spellings against the 128 new test bodies: 54 hits, plus
`diarization.rs:662` which the pattern cannot see, plus two from a separate early-return scan, plus
`local_model.rs:1179`, which carries no negative token at all and is the worst of them.

| | |
| --- | --- |
| **fine** — a positive control in the same test | **37** |
| **false positive of the pattern** | **13** |
| **real** — no control, and no sibling the same mutation kills | **4** |
| **weak** — no control, but a sibling kills the same mutation | **4** |

Reporting all four matters more than reporting the seven fixes. Last time seventeen candidates gave
ten fine, five false positives and two real; a report that lists only what it found cannot be
audited for whether the search was any good.

The thirteen false positives are three sub-shapes, worth naming because a rerun produces them again:
**`assert_ne!` used as a positive distinctness claim** (5), **`Vec::new()` in input construction or as
an accumulator, never in an assertion** (5), and **a literal `0` that is a measured value rather than
an absence** (3 — `nanos: 0`, `speaker == 0`).

### The one that was live rather than latent

`config/sidecars.json` marks whisper `"availability": "not-published"` on **both** macOS targets.
`.github/workflows/ci.yml` runs `macos-latest`. And three tests in `local_model.rs` are about
`sidecar_freshness`, whose first act is to resolve that asset:

- `:1191` and `:1211` opened `let Ok((_, asset)) = catalog.resolve(SIDECAR) else { return; };`.
  On macOS they **executed zero assertions and reported green.**
- `:1179` asserts `NotInstalled` against an empty directory. `sidecar_freshness` reaches
  `NotInstalled` by two roads — no published asset, or the search path finding nothing — and nothing
  told them apart. Its comment claimed *"the search path this function builds is exercised rather
  than described"*, which on macOS was false.

So `sidecar_freshness` has had **no coverage at all on a third of every CI run** — and it is worse
than three tests going quiet independently, because `:1191` and `:1211` are the only *positive*
controls the function has. Where they vacate the sole survivor is the vacuous one. The cluster
empties together and the suite reads 1664 passing either way.

This is KNEG-88's shape with an extra property that makes it worse. There, the resource that might
not have been written was a file. Here it is *the platform's published asset*, and CI has been
reporting green about its absence for weeks on a machine nobody watches.

### The convention that comes out of it: a skipped test says it skipped

The precedent was already in the tree, and the asymmetry is the argument. `real_install.rs`, written
the same week by the same hand, also returns early where whisper is unpublished — and it
`println!`s `"skipped: {err}"` first. Three tests, one road, one of them admitting to it.

`published_asset_or_skip` now does both things for all three: it asserts the one claim that *is* true
with no published asset — `NotInstalled`, the honest answer when nothing is pinned — and it prints
that it took that road. Zero assertions becomes one, and the one is checkable.

**It does not give `sidecar_freshness` real macOS coverage.** That needs a published macOS asset or a
fake catalog, and inventing either at the tail of this task would be building a second thing to keep
in step. The hole is now audible, and filed rather than fixed.

### The blind spots, which are the durable part

Two are known now, and they are a fact about the **method** rather than about these tests. The next
person runs this search from here rather than from the issue.

- **`vec![None]`.** `diarization.rs:662` reads
  `assert_eq!(assign(&turns, &[(18.0, 21.0)]), vec![None])`. A `None` *inside* a `vec![]` matches
  neither `assert_eq!(…, None)` nor `.is_none()`. It is a real site — an empty `turns` answers `None`
  for every span there is — and the grep cannot see it. It was found by hand.
- **The multi-line `assert_eq!`**, already recorded by KNEG-117, and still true.

Both are the same failure: the pattern matches *spellings* of absence, and absence has more spellings
than the list has entries. `Some(_) => …` is the third one waiting to be found. So the honest
statement about this search is the one the four-hundred-round flake hunt had to make about itself two
entries up — **it generates candidates, it does not prove there are none.** The difference is that
this one costs minutes.

### The two candidates that were dropped, and why

A search that reports only its hits cannot be audited, so both rejections are recorded.

**`diarization.rs:563`, `a_silent_recording_is_no_turns_rather_than_a_failure`.** Planning called it
Tier 1. It is not the shape. `recorded_silence()` is a `&str` built from two literals in the same
file, and a source literal cannot fail to be written; more to the point, the emptiness **is** the
subject, so there is no artefact for a control to assert. This is KNEG-117's own "a test whose whole
subject is that nothing happened" class. The discriminator is worth more than the fix would have
been: **the shape needs a resource that may not have been fully written, not merely an absence.**

**`meeting/watcher.rs:170`.** Out of scope — it is one of the 54 relocations, so it is not new code.
And `:220` one screen away already asserts the same `is_none()` for a client that is merely open, and
then the `is_some()` when the call starts. The control exists, in a different test.

### The seven, and the mutation that kills each

Every one was mutated by hand and each mutation checked for having changed behaviour — the record of
two mutations out of ten that mutated nothing is why that second step exists.

| site | control added | mutation that turns it red |
| --- | --- | --- |
| `local_model.rs` ×3 | `published_asset_or_skip` asserts, and prints | macOS `Target::current`, then the no-asset arm → `Current` |
| `local_model.rs:1179` | the answer must change once the executable is placed | `SearchPath.downloaded` → `root.join("nope")` |
| `hotkey.rs:1581` | a `Control+Alt` binding in the same vector | `prefix_cautions` → `Vec::new()` |
| `diarization.rs:715` | the list must carry `--clustering.cluster-threshold=` | `build_args` → `Vec::new()` |
| `diarization.rs:662` | the turn on the far side of the hole | `interpret` → `Ok(Vec::new())` |
| `notes.rs:312` | the segments it was discarding | `labelled` → `(Vec::new(), None)` |

`hotkey.rs:1581` is the one with no sentinel anywhere else. Its two `.is_empty()` negatives are true
of a `Binding::from_stored` that read `Mouse4` as an accelerator neither function looks at — and
nothing in the tree pins `from_stored` on a mouse spelling, because the neighbouring tests all go
through `classify` instead.

### Two mutations proved nothing, and one of them was the harness

The rule that a mutation must be checked for having changed behaviour earned its keep twice.

**`prefix_cautions`' `ModifierOnly` guard is belt-and-braces.** Deleting it looks like it should let
a mouse binding collect a chord caution. It does not: `Chord::parse("Mouse4")` fails on its own
account — `key_of` does not know the spelling, and a chord of one is `TooShort` either way — so the
very next `let Ok(chord) = ... else { continue };` skips it regardless. Two independent guards, and
no single-line edit defeats both. That is the same shape as the `chunks_exact(2)` non-mutation
recorded for `nemotron_socket`'s `samples`, and it is now named in the test's own doc comment so the
next person does not spend the same twenty minutes. The mutation that *is* real for that test is
`prefix_cautions → Vec::new()`, and it is worth more anyway: **the old test passed under it.**

**And the mutation harness lied once.** Restoring a mutated file with `shutil.copy2` preserves its
mtime, so the restore was invisible to cargo and a stale binary went on running. The visible symptom
was a mutation that "survived" — `SearchPath.downloaded` pointed at a directory that does not exist
and every test stayed green. The actual state was a macOS-simulating build from two mutations earlier
still sitting in `target/`, so the tests were taking the skip road and reporting it. Re-run against a
real rebuild, that mutation kills all three.

It is the flake hunt's lesson in miniature — *a run whose binaries changed underneath it is void* —
and it is worth writing down because it is the failure mode a hand mutation is most exposed to: there
is no harness to notice, and a green result reads as a weak test rather than as no test at all. The
restore now touches the file.

### Found and not fixed

**`diarization_models.rs:278`** is weak — `model_path → None` is caught by three siblings in the same
module. **`real_install.rs`'s early return** stays: it is `#[ignore]`d, and it is the precedent the
convention above is built from rather than a defect.

And the wave had already fixed one instance of this class by hand without naming it.
`pin.test.mjs`'s `assert.equal(targets.length, 4)` became a sorted list of ids, on the grounds that
*"a count cannot tell a model that was replaced from one that is still there."* That is this issue's
finding, arrived at independently a week earlier — some evidence the shape is real, and some that
nobody was looking for it systematically.

No production behaviour changed. Nothing here was vacuous because the code was wrong; every one was
vacuous because of what the test did or did not say.
---

## 2026-09-01 (KNEG-129) — a table that overstated a capability, and two sentences that invented one

Three sentences that were true when they were written, and one rename. Nothing here changes
behaviour, and deliberately not: **when a modifier-only chord should fire is KNEG-115 and it is
still open.**

### `is_usable` had drifted, and the codebase already knew

`Binding::is_usable` said *"Whether this action has at least one working hotkey"* and computed
`!self.registrable().is_empty()`. On Windows that is false twice over: a `ModifierOnly` chord and a
`MouseButton` are both unregistrable by construction and both fire through the low-level hook.
`Accelerator::explain` says so on the screen in as many words — *"It still works"*, and *"It works,
and the click is not swallowed"* — and `Accelerator::is_registrable` had the honest wording next
door the whole time: *"Whether `tauri-plugin-global-shortcut` can register this today."* The drift
was confined to one method and one field. It is `has_registrable` now, which pairs with the
`registrable()` immediately above it.

**No user-visible string changed.** `Dictation.tsx` renders `bindings`, `unhandled`, `conflicts` and
`cautions`, and has never read the field this issue is about.

### The field is worse than its name, and the rename says so rather than hiding it

`HotkeyReport::unusable` is the union of *no registrable accelerator* and *no handler*, which are
two unrelated properties. The first stopped meaning "does not work" the moment a hooked platform
existed — on the shipped default, dictation is in that set and it **fires**. The second is the
opposite shape: chat agent's `Control+Shift+Space` is a faultless registrable accelerator and it is
the *action* that has nowhere to go. Neither half implies the other and the union implies neither,
so no short name for it can be true.

It is `unregistrable_or_unhandled` now. The clumsiness is the finding rather than a cost to be
minimised: **a field whose only truthful name is a conjunction is a field that should not exist**,
and this one has no consumer at all — nothing in Rust outside its own tests, nothing in
`apps/desktop/src`. Renaming it stops it lying; deleting it is a separate decision and is filed
rather than taken here. `unhandled` already carries the reason and
`RegistrationReport::is_live` already carries the outcome.

The 2026-08-26 and 2026-08-31 entries below still call it `unusable`. They are dated records of what
was true then and are left alone; rewriting them to use a name that did not exist yet would be its
own small dishonesty.

### `PLATFORM_SUPPORT.md` was wrong eight times more than the issue thought

The issue named two false cells. Against `keyhook::support()`, which returns `Unsupported` for every
target that is not Windows, there were **sixteen** — five macOS, four X11, three Wayland
modifier-only, and the whole push-to-talk row on five platforms — plus three registration mechanisms
(`gsettings + D-Bus`, `KGlobalAccel`, `hyprctl`) that appear nowhere in the repository, and one
Windows cell, `native hook per key`, that contradicted line 426 of its own section eight lines
further down. Line 426 names one-hook-per-key as the upstream design this project deliberately did
not copy.

**But the ranking is the transferable part, and it is not the count.** A cell overstating a
capability is a table being generous. A sentence describing behaviour is a different failure, and
this section had two:

- *"On X11 and GNOME the effective default is adjusted because modifier-only combinations do not
  work there."* No such adjustment has ever existed. `default_accelerator` is a bare `match` with no
  conditional compilation anywhere near it — `grep -n "cfg(target_os"` over `hotkey.rs` and
  `shortcuts.rs` returns nothing — and `nothing_ships_holding_by_default` pins the value. Every
  platform ships `Control+Super`. This one invents a behaviour.
- *"Push-to-talk on Linux needs read access to `/dev/input`; without it the app reports the
  limitation and falls back to toggle mode rather than silently failing."* **This is the worst thing
  in the section and nobody had noticed it.** There is no `/dev/input` reader in this repository —
  zero matches — and nothing falls back to toggle mode; `hotkey.<action>.mode` is a stored per-action
  setting and no code consults the platform when reading it. It invents *two* behaviours, and both
  of them are the careful, self-aware engineering the rest of the document is genuinely full of,
  which is exactly why it reads as true.

A capability cell that is too generous costs a reader an expectation. A sentence like the second one
costs them a belief that the problem was already thought about.

### And one stale reason, folded in

`commands.rs` built an empty speaker map under *"which nothing writes yet because there is no
diarisation."* There is diarisation now, and the **behaviour is right and unchanged** — the map is
empty because nothing renames a speaker, not because nothing diarises. `meeting::notes` writes
`speaker_0` with the placeholder `Speaker 1`, `transcript::speaker_name` renders it from the segment
alone, and what the table would add is a name a person typed, which no screen offers. Only the
reason was false.

---

## 2026-09-01 (KNEG-73) — the deferral was narrowed, not dissolved, and the gate still cannot tell

`search:semantic` made four checkable claims and **two were false on their face**:

| claim | verdict |
| --- | --- |
| `"interface": "notes_core::search::SemanticIndex"` | **Never existed.** `crates/notes-core/src` is `lib.rs`, `notes.rs`, `transcript.rs`; the name appears nowhere in the tree but in this manifest |
| `"degradation": "the UI hides semantic ranking rather than failing"` | **False.** Zero occurrences of `semantic` in `apps/desktop/src`. Nothing is hidden, nothing was built, and `SearchHit` has no score field to rank by |
| ONNX Runtime | **Already in the tree**, as `onnxruntime.dll` inside the pinned sherpa archive — the same discovery that collapsed diarization's identical claim the day before |
| the `ort` crate | **Not required.** `kw_meeting_core::diarization` drives an ONNX model with no `ort` in `Cargo.lock`, by shelling out to a binary that links the runtime itself |
| Qdrant, MiniLM | **Genuinely absent**, and the deferral survives on these two alone |

So it **stays deferred** and nothing was built — no sidecar, no embedding path, no `search` module.
What changed is that the entry now rests on the one argument that holds rather than on three, and
says which parts are measured and which are not.

### The instruction was to delete the deferral block, and that turned out to be impossible

The plan said to correct this the way the diarization entries were corrected: delete the `deferral`
block, move the truth into `note`. That could not be done. `verify-parity.mjs` fails
`incomplete-deferral` when a `temporarily-deferred` entry has no block, and there is no vocabulary
term for "deferred without one". Diarization could delete its block **because its status changed to
built** — that is what made it the model, and it is precisely the part that does not transfer to an
entry that is still deferred. So every field of the block was corrected in place instead.

Worth writing down because the mistake is a natural one: *"correct it the way that one was
corrected"* silently assumes the two entries are in the same state, and these two were in opposite
states.

### The general defect, now with a second instance

`verify-parity.mjs` checks that a deferred entry **has** `why`, `dependencies`, `interface` and
`expectedPhase`. Five existence checks, no type check, no resolution of `interface` against the
source tree, and no way at all to ask whether any of it is still true. That is how this entry named
a nonexistent module for months, and it is the same lesson the KNEG-123 entry drew one day earlier —
*"a deferral block ages exactly like a status claim and nothing gates it."* One instance is an
anecdote; two in two days is the gate's shape.

`search:sqlite-fts` was stale in the same pass, in a smaller way: `"backend": "crates/storage"` named
where the FTS5 table is declared rather than where the query runs, which is
`kw_notes_core::notes::search`.

---

## 2026-09-01 (KNEG-76) — write-only ownership, and a rule that was exact and unwritten

Five orphan tables were really **eight** — `speaker_profiles`, `speaker_mappings` and
`note_speaker_embeddings` are in the same state — and *"touched only by the importer"* was false for
four of them. All eight are asserted to exist by a schema test; `agent_messages` is the subject of
the foreign-key test and `speaker_mappings` of the cascade test; four are named to the user in a
shipped sentence on `Notes > Upload`; and `contacts` is cited **normatively** by two diarization
modules as a thing they deliberately do not reach for. The honest phrase is **write-only
ownership**: the importer writes them, tests and comments depend on them, nothing reads them back.

Two decisions recorded rather than deferred a fourth time. **`snippets` stays** — it is not inert,
the importer copies it and a shipped sentence promises the user it will, so dropping it discards
real data to save a five-column table. And **`speaker_mappings` has a candidate owner it did not
have a week ago**: diarization produces `Speaker 1`, `speaker_name` already prefers a mapping over
the placeholder, and that table is where a rename to a person would live. The owner is named and
nothing was built.

### The best thing in this issue was not in the issue

`contacts` is in `COPY_PLAN` and not in `MUST_BE_EMPTY`, which looked like an oversight worth a
sentence. Checking all sixteen tables against the schema turned it into a rule: **`MUST_BE_EMPTY` is
exactly the `COPY_PLAN` tables declared `INTEGER PRIMARY KEY AUTOINCREMENT`, with no exception in
either direction.** The list guards one risk — the importer preserves row ids, so a non-empty target
could collide — and every table it omits is keyed by something SQLite did not hand out: `contacts`
by `email`, the two speaker child tables by `(note_id, speaker_id)`, the four calendar tables by a
provider-supplied `TEXT` id.

Exact, load-bearing, and **written down nowhere**, which made it a coincidence waiting for whoever
added the next table to either list. It is now on `MUST_BE_EMPTY` as prose and pinned by
`must_be_empty_is_exactly_the_autoincrement_tables`, which reads the live schema rather than a
second hand-written list — a test that restates the nine names proves only that someone typed them
twice. Mutated both ways before being believed: adding `contacts` to the list and removing
`snippets` from it each turn it red.

**No migration**, and that is the shape of the closing rather than a limitation of it. There is one
migration file and `migrations_are_idempotent_across_reopens` asserts `applied == 1`.
---

## 2026-09-01 (KNEG-128) — four installers wrote the record and one screen read it

`kw_sidecar::install` writes `.kw-install.json` into every sidecar directory it promotes, naming the
digest of the archive the files came out of. That landed with KNEG-113 earlier the same day, and it
landed for all of them: four modules install through that function. **One read it back.**

`local_llm.rs`, `local_asr.rs`, `diarization.rs` and `media.rs` all had the evidence sitting beside
their binaries and none of them looked, so each went on claiming *Installed* — a fact about a path —
while the binary fell behind the manifest. That is the exact state KNEG-113 was written to end, left
standing in four places by the change that ended it in the fifth.

Two are fixed here, llama.cpp and sherpa. ffmpeg and yt-dlp stay out: a different status shape,
neither observed stale, and widening would be the speculative reach that issue's own argument for
being Low warns against.

### The exhaustiveness guard went in first, and that ordering was the point

`freshnessLabel` returns `string | undefined`, so falling off the end of its switch was legal. A
fifth `SidecarFreshness` variant would have compiled without a murmur and rendered **no sentence at
all**, and both call sites compare the tag with a bare `===`, which `tsc` cannot check either.

Measured rather than argued, in both directions: a sixth variant added to `ipc-types.ts` by hand
passed `tsc --noEmit` cleanly before the guard and fails with `Type '{ state: "withdrawn"; ... }' is
not assignable to type 'never'` after it. Same defect `meetingIsLive` records being bitten by when
`stalled` arrived for KNEG-116 — *"both screens compiled without a murmur, and answered no"* — and
the same `never` guard closes it.

It went in **before** the call sites, because this issue takes that function from one screen to
three. Fixing it afterwards would have tripled the silent-failure surface first and then asked
somebody to notice.

### llama.cpp: one call for two answers, and a trap that had nothing to do with freshness

`local_llm_status` resolved the asset, located the installation and **threw the `Installation` away**
to keep a bare `bool`. `server_state` replaces the chain and returns both answers from one
`sidecar_freshness`, so `server_installed` is a reading of the freshness rather than a second
question — the disagreement `local_model_status` is arranged to prevent.

`RefreshOutcome` is **shared**, not copied. `ipc_types` asserts that no two Rust types render to the
same TypeScript name, so a second one would have been a failing test rather than a duplicate nobody
noticed; the two refreshes answer the same three questions, so sharing was the right resolution.
`Restart` and `refresh_failure` widened to `pub(crate)` with it.

The refresh reads the running model through `running_model()` and never `LocalEndpoint::port`, which
records a use and holds the idle clock open — the same choice `local_llm_status` makes and for the
same reason.

**And then the thing that was not in the issue.** `local_llm_start` armed the idle watcher;
`LocalLlm::start_model` did not. A refresh that restarts through `start_model` and stops there brings
the server back with **nothing left to ever stop it** — six gigabytes resident until the application
exits, which is worse than any staleness this issue was filed about, and nothing in the tree would
have caught it. Found by reading the start path before copying it, not by a failure.

Fixed two ways, and the split matters. `start_and_watch` is now the only way the ordinary path
reaches a running server, so the pairing is structural. The refresh consults
`must_arm_idle_watcher`, which is pure and tested in both directions — `Restarted` must arm, and
`NothingWasRunning` and `Failed` must not, because a watcher over a server that is not running is a
task spawned to discover that on its first tick. What no test can do is observe a
`tauri::async_runtime::spawn`; that half is held by construction.

### sherpa: the fix is worth more than the feature, and the button is declined

`installed_cli()` hand-built `cli_root()/sherpa/<exe>` and never consulted the catalogue, so
`cli_installed` and any freshness reading were two independent answers to one question. Both now go
through `locate` against the published asset. It is **not** a live bug — all five published targets
ship `.../bin/sherpa-onnx-offline`, so the two spellings agree today — and that is precisely the
condition under which this kind of duplication survives until it does not.

`resolve_id` is reached from `dictation.rs` and `media.rs` through a bare `Selection`, so threading a
`Catalog` to it would mean threading one through `DictationRuntime` and `MediaImportRuntime` as well.
`installed_cli()` parses the shipped catalogue itself instead: an embedded-string `from_str` on a
path that is about to spawn a process and transcribe a WAV, with `LocalAsr::new` having already
proved at startup that it parses.

**No refresh button, and the reason is the streaming server's lifecycle rather than the archive.**
`meeting.rs:656` is the only caller of `SherpaStream::start` and **nothing anywhere calls
`SherpaStream::stop`** — the only stop is the one `start` performs on itself when switching models.
So a running streaming server means *a meeting has happened this session*, not *a meeting is
happening now*. A refresh would either stop it unconditionally, ending the live transcript of a
meeting in progress from a dictation settings screen that never mentions meetings, or need a
live-meeting guard reaching into a second runtime's state. Neither version is testable — the whole
sequence is reachable only through an `AppHandle`, exactly as whisper's button is, and that button
has still never been pressed.

So the screen reports the staleness and says plainly that it cannot fix it, including that one
archive serves dictation, meetings and speaker separation, and that reinstalling means closing the
application. A screen that reports honestly and cannot act beats a button that is wrong at the worst
possible moment.

Worth recording separately, observed and not chased: if nothing ever calls `SherpaStream::stop`, the
streaming server **outlives every meeting**, holding a 630 MB model from the first meeting until the
process exits. That may be deliberate reuse — `running_on` already argues for keeping a loaded server
between meetings — or it may be the defect llama's five-minute idle timer exists to prevent, one
runtime over. It is outside both issues and it is not guessed at here.

### The mutation that survived

Six mutations for this half; five killed, **one did not**. Replacing `located_cli`'s `locate` call
with a hand-built path to a nonexistent executable left the suite green, because the function reached
for `cli_root()` internally and no test could point it anywhere. That is the `media.rs:1334-1340` line
again, in a function written an hour earlier by someone who had just finished writing about it.

Split into `located_cli_in(catalog, root)` and a one-line wrapper, the same mutation now fails with
`left: None, right: Some(...sherpa-onnx-offline.exe)`. The gap was real and the mutation pass is the
only reason it is not still there.

### What is believed rather than measured

Everything on both screens. The freshness reads are exercised against real directories with the
catalogue's own executable name in them, and `server_state`, `server_progress`,
`must_arm_idle_watcher` and `located_cli_in` are all tested. **Nothing has been pressed.** The
llama refresh's stop-install-restart, whether the restarted server actually idle-stops five minutes
later, and how the sherpa sentence reads on a machine whose runtime really is unrecorded are all
by-hand checks, and they go to Linear rather than being claimed here.

---

## 2026-09-01 (KNEG-130) — the response to the finding destroyed the finding

`local_llm_recheck_model` hashes an installed GGUF against its pinned digest. It is the only thing in
this application that actually proves the bytes of a model, and when it found a file that did not
match, `settle_record` **deleted the record beside it**.

That looks like the safe direction and is not. The reasoning behind the deletion was sound as far as
it went — a record that survived a mismatch is a record that is *wrong*, because it says the length
and the timestamp are what they were at install while the bytes say they are not, and leaving it
would have the screen answer *unchanged since it was installed* about a file just proved otherwise.
The mistake was in what deleting leaves behind.

It leaves `GgufIntegrity::Unrecorded`, which `gguf_check` documents at length as **not an
accusation**: it is the state every GGUF installed before records existed is in, including the
1.28 GB Qwen2.5 on the developer's machine. So a file that had been read in full and refused became
indistinguishable from one nobody had ever looked at.

**And it was worse than a display problem.** `start_model`'s warning matched `GgufIntegrity::Changed`
and nothing else. A failed re-check deleted the record, the row fell to `Unrecorded`, the warning
stopped firing — so the log went quiet on exactly the file the application had just proved was not
the one that was installed. Responding to the finding was what destroyed it. The verdict survived
only as long as the command that produced it, and a restart erased it entirely.

### The verdict goes in the one record

Not a second file beside it. Two files describing one model are two things that can disagree, and
the disagreement would be about whether a model can be trusted.

`SCHEMA` moves 1 → 2 and `InstallRecord` gains a `verdict`: `Passed`, or `Failed` carrying the pair
of digests that disagreed. The digests live in the verdict rather than being read back off the
record's own `sha256`, and that is not duplication — `sha256` records what the *install* pinned and
these record what the *re-check* actually compared. They are the same string today. A pin updated in
the registry after a model was installed is exactly when a person reading the file needs both.

**The precedent is `calendar.rs`, not something invented here.** It already persists negative
verdicts — `SyncOutcome::{Failed, TokenUnavailable}` — and folds them into live status through the
pure `combine_token_status`, whose doc makes the argument this needed: one shape for both, so a
screen can never see a different answer from a check it just ran than from the record read back
later. What is new is only the medium. This is the first negative verdict this application writes to
**a file beside an artefact** rather than into SQLite, and the module doc says so plainly, because
the record is ordinary JSON in a user-writable directory — a place to keep a finding, never a place
to enforce one.

### Where the verdict is read is the whole design

Inside the branch where the two integers already agree, never before it. Both halves are load-bearing
and each has its own test:

- **Only there** is what stops a recorded failure rendering as `UnchangedSinceInstall`. A failed
  re-check writes the file's *current* length and mtime, so the cheap comparison agrees — and reading
  only those two integers would have this application make its strongest positive claim about the one
  file it has proved is wrong.
- **Not before it**, which is the obvious way to write it and the wrong one. A verdict describes the
  bytes it was measured against. Once the length or the timestamp moves, those bytes are gone —
  somebody has reinstalled the model — and the state falls back to `Changed` whatever the verdict
  said. An accusation carried forward over a file that has since been repaired is a screen nobody can
  clear.

`integrity_of` stays pure over two integers and a small enum. Nothing here hashes anything; the
branch is chosen by the *recorded* verdict.

`FailedRecheck` is therefore worth exactly the sentence it carries: the file was read in full and
refused, **and nothing has written to it since**.

### Backward compatibility was the part that could have gone wrong quietly

`read_record` compared `record.schema == SCHEMA`. Bumping the constant with that line in place would
have discarded every record already on disk — every installed model dropping from *unchanged since
it was installed* to *installed, never recorded*, for no reason a user could see. That is the same
overclaim this module exists to prevent, running in reverse.

There are two constants now, `SCHEMA` for what is written and `READABLE_SCHEMAS` for what is
understood, because those are different facts. A schema 1 record has no `verdict` key and
`#[serde(default)]` reads it as `Passed`, which is the correct reading: a record written by an
install is a record of a check that passed.

**Proved against a fixture, not a fresh install.** The test writes the JSON by hand, with no
`verdict` key, against a real file whose length and mtime it names — a record built through
`InstallRecord { .. }` would acquire the field from the struct and prove nothing about what is on
somebody's disk today. Its control is a schema 3 fixture in the same test, so the acceptance is a
decision rather than serde taking whatever it is handed.

### `forget_record` is gone, and its argument is not

Deleted rather than left uncalled. Its reasoning moved into `write_failed_record`'s doc, because the
reasoning was never "forget it" — it was *never let the screen say unchanged about this file* — and
writing the verdict satisfies that better than deleting did.

The failed record writes `verified_by: nothing`. Nothing about those bytes was verified; the last
thing that looked at them refused them. Carrying the install's old strength forward would put this
file's strongest word beside its worst verdict.

### The one assertion that carries the argument, and the gate that actually guards it

`start_model`'s warning was the reason this mattered, so the branch had to become reachable.
`load_warning(&GgufIntegrity) -> Option<String>` is pure and tested over all five states: `Changed`
and `FailedRecheck` speak, and `UnchangedSinceInstall`, `Unrecorded` and `NotInstalled` are silent.
Both directions are asserted, because only one of them is the defect and the other is the noise
fixing it would create — a warning that fires for every `Unrecorded` model on disk is a warning
nobody reads.

The cost is honest and small: the structured `recorded_bytes`/`bytes` tracing fields become part of
the message text. A branch nothing can reach is worth less than two log fields.

**What that test does not prove is that `start_model` still calls it**, and this is worth recording
because it is the interesting half. Mutating `load_warning`'s body proves the function; it says
nothing about the call. What catches a deleted call site is not a test at all — `load_warning` is
private, so removing its only caller makes it dead code and `clippy --workspace --all-targets -D
warnings` fails the build. Verified by doing it. The gate is the guard here, not the suite, and those
are different claims.

### What is believed rather than measured

The five states were exercised against real files in a temporary directory, and the failed verdict
was written, read back, cleared by a passing re-check, and invalidated by a subsequent write. **No
real GGUF has been through this.** `local_llm_recheck_model` had never been run before this change
and still has not been: its verdict and its record-settling are tested as pure functions and against
small real files, and the command that wires them to `spawn_blocking` and the registry is the
untestable edge. The first genuine 1.28 GB hash, and the first `failed-recheck` row on a real screen,
are still by-hand checks.

---

## 2026-09-01 (KNEG-117) — the depth that was deferred twice cost two and a half hours

**Run: 400 rounds, 35 binaries, 14,000 runs, thread counts 1/2/4/8/16, control passed, not void,
nothing found.**

That is the run KNEG-117 asked for, and it is the first time the workspace claim rests on anything.

### The estimate was the whole obstacle, and it was wrong

The 2026-08-31 entry deferred this as *"several hours of a machine doing nothing else"*. Nobody had
timed it. `--workspace --rounds 3` takes 66 seconds, so a round is **22 seconds** and four hundred
of them are **two and a half hours**.

Worth recording as a mistake rather than as a measurement: an unmeasured cost estimate deferred a
piece of work twice, and the measurement that dissolved it took one minute. The instinct that says
"this is expensive, do it later" is exactly the one that should be checked with a stopwatch first.

### What the result means, and what it does not

Four hundred rounds is four hundred runs of **each** binary, which is the same exposure KNEG-88's
flake was found at — twice in four hundred runs of `kw-storage`, about 0.5% per run. So this run
matches that confidence for every binary in the workspace, which is precisely what KNEG-117 asked
for and nothing more.

Two ways to state the bound, both honest and neither a verdict:

- **Against a flake as common as the one already found:** 1 — 0.995^400 = **86%**. A flake at that
  rate probably would have shown. It also might not have; fourteen times in a hundred it hides.
- **Rule of three**, for a binary with zero failures in four hundred runs: the 95% upper bound on
  its per-run failure rate is 3/400 = **0.75%**.

Note what the second one says. KNEG-88's flake was 0.5% per run, which sits **below** that bound —
so a flake exactly as common as the one this project has already met could still be sitting in any
of these thirty-five binaries. The run raises the floor; it does not clear the room.

### Two things that make the number mean anything

**It is not void.** The tool refuses to report a run whose test binaries changed underneath it
(`2ad9ea2`), because its first workspace run reported a real test failing nine times out of thirty
and that was a Rust type being edited in another window. The tree sat at `7b7ebfd` untouched for
the whole run, and the completion line says so.

**The control passed**, which is the only reason a clean report is allowed at all — the first
version of this tool reported 100% failures for every thread count including the control, because
a trailing carriage return meant nothing was ever executed. A harness that fails everything is
measuring itself, and one that passes everything for the same class of reason would have been read
as a clean result.

### And one banked earlier the same day

`kw-meeting-core` alone, 400 rounds, 1,200 runs, three binaries, clean and not void — run while
three other branches were still moving, because it was the one crate none of them touched and the
one that had just gained `diarization`, `diarization_models`, `nemotron` and `nemotron_socket`.
Subsumed by the workspace run above, and worth recording as a thing that could be done while the
rest of the tree was unavailable rather than waited on.

### Still not done

The **search** half of KNEG-117 does not need repeating — seventeen candidate sites were read on
2026-08-31 and the two real ones hardened. But it was a search over the tests that existed then.
Roughly 190 tests have been written since, across six issues, and **none of them has been read for
the absent-assertion shape**. The loop cannot find that defect; only the search can.

---

## 2026-09-01 (KNEG-123, KNEG-124) — a deferral block that was wrong three times over

Diarization had sat `temporarily-deferred` since before the build plan began, under a `deferral`
block naming three dependencies. **All three were wrong**, and the cost of not checking was a
feature deferred for months that took a day.

- `ort` (ONNX Runtime): the sherpa CLI links it itself.
- a `sherpa-onnx-diarize` sidecar: the pinned sherpa archive — the one already installed on this
  machine for Parakeet — ships `sherpa-onnx-offline-speaker-diarization.exe` beside the offline
  transcription CLI.
- `silero_vad.onnx`: the pyannote segmentation model does its own voice-activity detection.

The lesson is not "check dependencies". It is that **a deferral block ages exactly like a status
claim and nothing gates it.** `verify-parity.mjs` checks that a deferred entry *has* a reason; it
has no way to check the reason is still true, and this one had been false since whenever the sherpa
archive gained that binary. The entry was corrected in place, not annotated.

### Five measurements, each of which changed the code

Nothing here is in the CLI's `--help`.

| measured | what it changed |
| --- | --- |
| A failure and a recording nobody spoke in produce **byte-identical stdout** | `interpret` takes the exit status. This is the KNEG-38 shape — "capture broken" and "the far end was quiet" looking the same — and there is a test that fails if the status is dropped. |
| Labels are cluster indices and **not dense from zero**: the one-speaker fixture returned `speaker_01` | Read straight through, the only person in the room is called "Speaker 2". `interpret` renumbers by first appearance. |
| Turns overlap, and **one can nest wholly inside another** | Overlapped seconds alone make a nested voice unreachable for every span there is. `assign` needed a second rule. |
| `--clustering.num-clusters=4` asked for four and **produced three** | Never passed. It is a hint to the clusterer, and a meeting does not know its own attendance. |
| 16 kHz is a **refusal, not a resample** | Meeting recordings are already written at 16 kHz, so nothing converts — but an imported file need not be. |

### The control cost one extra download and changed what the feature claims

The first run paired the pinned **English** CAM++ embedding model with the release's **Chinese**
four-speaker reference and returned three voices, one of them swallowing ten seconds belonging to
two other people. That looked like a mediocre result. Re-run with the language-matched Chinese
embedding model, the same audio through the same segmentation model returned four, with different
boundaries.

So the pipeline is fine and **the embedding model is language-sensitive where the segmentation
model is not.** Without the control this would have shipped as "diarization works, a bit roughly".
With it, the limit is named: English is the measured case, and the screen says so rather than
leaving a Swedish meeting's bad speaker turns to be blamed on the feature in general.

### A test that was wrong, and the rule that was wronger

`assign` scored turns by overlapped seconds and took the highest. A test asserted the interrupter
won a span that was mostly theirs, and it failed — 0.45 s each, a tie, and the tie-break gave it
to the earlier turn.

The tempting fix is to change the expectation. The real finding is worse than the test: in the
recorded run `13.025 -- 13.514 speaker_01` lies **wholly inside** `11.455 -- 14.037 speaker_00`, so
the outer turn ties or wins for *every span there is* and the inner voice can never be named. Not a
tie — **a voice unreachable by construction.** The rule gained a second criterion: on equal
overlap, the turn with more of *itself* inside the span wins. 92% of one against 17% of the other
is not a coin toss.

### `assign` is invented, and says so

`docs/PORTING_MATRIX.md` names upstream's `diarizationPolicy.js` (63 lines) as the thing to port.
**That file is not in this repository** — only its line count is. Writing "ported" over a rule
reasoned out from scratch would be exactly the claim these documents exist to prevent, so the
module says **invented** and the rule stands on its tests rather than on upstream's authority.

### The consumer, and a comment that was true when written

`transcribe_whole` accepted `Inference::Timed { text, .. }` and threw the segments away, under a
comment saying this pass "is not asked for timings". True when written, and the reason there was
nothing for a speaker label to attach to. It keeps them now, and `Text` — which a whisper server
started with `--no-timestamps` still returns — remains an ordinary state that the note describes
rather than a failure.

**The note only ever speaks up when the result differs from what the setting asked for.** Labels
that worked add no sentence: they are visible in the note. Labels that were asked for and did not
happen add one, because an unlabelled note reads as a meeting with one speaker in it. Six tests
cover that asymmetry, including the one that distinguishes *the step did not run* from *the step
ran and attributed nothing* — same segments, two different sentences.

### What it costs, said on the screen

Real-time factor 0.081, measured. A one-hour meeting adds roughly five minutes to *Stop and
transcribe*, on top of the transcription pass already there. Off by default, and the screen gives
the figure rather than calling it "a short delay".

### Still not done

**Nobody has recorded two people and checked the turns.** The integration test's control is a
one-speaker fixture, which proves the flags, the exit-status reading and the renumbering end to end
and says nothing about attribution — one voice cannot be confused with another. That is the same
distinction KNEG-84 and KNEG-94 both ended on, and it is not a coincidence: finishing the
measurement is not finishing the feature.
## 2026-09-01 (KNEG-107) — a boolean that described a path, and the third state it could not hold

`local_llm_status` reported `installed: gguf_path(&m.file_name).exists()`. That is a claim about a
**path**, not about a file. The install had hashed the bytes against a pinned SHA-256 and then
nothing looked at them again, so a GGUF corrupted, truncated or replaced months later rendered
identically to one checked five minutes ago — and was loaded without complaint.

The window is specific to these files. They are the largest thing this application writes to disk
(19.8 GB for `qwen3-32b-q4_k_m`), they sit in a user-writable cache directory for months, and a bad
block in one is a far more ordinary event than in a 148 MB whisper model.

### Why the whisper answer was not copied

`kw_transcription_core::models::is_installed` calls `check`, which calls
`kw_sidecar::integrity::verify` — it re-hashes on **every status call**. That is the right answer at
3.1 GB and the wrong one at 19.8: `Settings > Intelligence > Local` would stall for about a minute
per visit. Hashing inside `local_llm_start` instead was considered and rejected on the same
arithmetic, and it is worse there — it makes starting the server feel broken.

So the trade this project already made stands, and it is the *reporting* that changed.

### A record beside the file, and a sentence saying what it is not worth

`gguf_check::write_record` writes `<name>.gguf.checked.json` after the rename, holding the digest
the bytes were hashed against, the exact length, and the modification time. A file whose length and
mtime still agree with that record counts as unchanged since it was installed. Two stat calls and a
small read, against a minute of hashing.

**The sentence that matters is the one about what it does not catch**, and it is in the module doc,
in the type, and on the screen: different bytes written into the same length with the timestamp set
back would still read as unchanged. That is a different threat from the one this closes, and it has
to be said rather than left for a reader to assume the file is authenticated on every load.

The state is therefore called `UnchangedSinceInstall`, not `Verified`. A test asserts that no state
spells itself "verified": two integers agreeing is not a hash, and the weaker word is the point.

### Three states, because two of them used to render alike

`installed: bool` became `integrity: GgufIntegrity`, with four answers:

| state | what it means |
| --- | --- |
| `not-installed` | nothing at the path |
| `unchanged-since-install` | a record, and the length and mtime still agree with it |
| `unrecorded` | the file is there and nothing beside it says what was checked |
| `changed` | a record, and it disagrees |
| `failed-recheck` | a re-hash read the file and refused it, and nothing has written to it since |

The fifth arrived hours later, with KNEG-130 — see that entry. It is listed here rather than left to
be discovered, because the four-state framing above is the thing that made the defect possible.

`unrecorded` is the one the design turns on. **Every GGUF installed before this landed is in it** —
including the 1.28 GB `qwen2.5-1.5b-instruct-q5_k_m.gguf` on the developer's machine right now — and
rendering it as either "verified" or "corrupt" would be the same overclaim in opposite directions.
It gets the neutral treatment `labels.ts` already gives `unknown` everywhere else.

`changed` reports both lengths, so `recordedBytes === bytes` is how the screen tells "something wrote
to this" from "this is the wrong size". A restore from backup moves a timestamp without touching a
byte, and the two deserve different sentences.

### The re-hash is a button, and the button says what it costs

`local_llm_recheck_model` reads the whole file and compares it to the pinned digest — the only thing
that proves the bytes. It returns a `RecheckOutcome` rather than a `Result<(), String>`, because a
file that does not match its digest is a **finding**, not a transport failure, and the two must not
arrive through the same red line.

Two decisions inside it:

- **A mismatch removes the record and keeps the file.** A record that survived a mismatch is a
  record that is wrong — length and mtime unchanged while the bytes are not — which is exactly the
  replacement the cheap check documents itself as unable to see. Left in place it would have the
  screen make the strongest available claim about the worst available file. The weights stay: they
  may be twenty gigabytes somebody chose to keep, and a stale pin here is as likely as rot there.

  **The removal lasted a few hours.** The requirement was right and the remedy was not: deleting the
  record dropped the row to `unrecorded` — the *nothing is known* state, which is not an accusation
  and which every pre-record model reports — and silenced `start_model`'s warning with it, so
  responding to the finding was what destroyed it. A mismatch now **writes** the verdict instead.
  KNEG-130, same day.
- **`nothing-pinned` is its own answer.** Nothing shipped today can reach it, and "there was nothing
  to check" still must not render as "checked, and fine".

### `start_model` warns and does not refuse

One stat, no hashing. A disagreement is logged with both lengths and the load proceeds: a moved
timestamp is a restore as often as it is anything else, and refusing to load a working model on
evidence this deliberately calls weak is a worse failure than the one being guarded against. The
screen is where a person sees it.

### What the mutation pass found

Twenty mutations, and the eighteenth was the useful one. Every three-state mutation was caught —
including the defect itself, an absent record rendering as `UnchangedSinceInstall` — except one:
making every status row report `NotInstalled` survived, because the mapping was inlined in a
`#[tauri::command]` that takes a `State` nothing outside a running application can construct. That
is the `media.rs:1334-1340` line again. Extracting `model_status(provider, dir, model)` moved the
whole mapping to the testable side and left the command holding one call to
`paths::model_cache_dir()`; three further mutations aimed at it were all caught.

A second finding came out of a test rather than the code. Written with `Duration::new(0, 1)`, the
`Mtime` round-trip failed on Windows with "`Mtime { seconds: 0, nanos: 0 }` was produced twice":
`SystemTime` there is a `FILETIME`, which counts in 100 ns ticks, so one nanosecond either side of
the epoch *is* the epoch. A limit of the input rather than of the function — and the same 100 ns
NTFS stores an mtime at, so nothing finer could reach it from a real file.

### What is believed rather than measured

The four states were exercised against real files in a temporary directory: written, recorded,
rewritten at the same length, truncated, and had the record removed, with the answer asserted at
each step. **They have not been exercised against a real model in the real cache directory**, which
needs a person at a machine with a GGUF on it. In particular the *unrecorded* state in the wild —
the installed Qwen2.5 with nothing beside it — is believed to render as `unrecorded` on the strength
of the same code path answering that way for a 4 KB file in a tempdir. Nobody has opened the screen.

`local_llm_recheck_model` itself has never been run: its verdict and its record-settling are tested
as pure functions against real files, and the command that wires them to `spawn_blocking` and the
registry is the untestable edge. The first real 1.28 GB hash is a by-hand check.
## 2026-09-01 (KNEG-64) — two capabilities the table promised, and the default nobody was allowed to change

`docs/PLATFORM_SUPPORT.md`'s global-shortcut table has listed **mouse buttons 4/5** and
**push-to-talk** as things the native hook makes possible since the table was written. Neither was
built. Both are now, and the interesting parts are the three places where the obvious
implementation was wrong.

### The encoding was measured, and the obvious decoder reads zero for every click

`MSLLHOOKSTRUCT::mouseData` carries the X-button number in its **high** word. `SendInput`'s
`MOUSEINPUT::mouseData` carries the same number in its **low** word. Two fields, one name, opposite
halves — and a decoder written from the wrong half finds zero on every press and reports a mouse
with no side buttons, which looks exactly like a mouse with no side buttons.

So `mousehook_probe` measures it rather than reasoning from a header:

```
WM_XBUTTONDOWN mouseData=0x00010000 high word 1     (XBUTTON1, mouse 4)
WM_XBUTTONDOWN mouseData=0x00020000 high word 2     (XBUTTON2, mouse 5)
```

**The control took two attempts, and the first one was worthless.** The first version injected a
zero-pixel mouse move as its "this is not a button" case. Windows filters a zero-pixel move: it
produced no message at all, which is indistinguishable from a hook that is not listening. The
shipped probe moves three pixels right, three back, and turns the wheel one notch — the wheel
because its delta lands in the *same high word* the button number lives in, so it is a case where
the field is non-zero and the answer is still "not a button". If the run reports zero movement
messages it says the control failed and that nothing below that line is evidence.

Phase two then drives the real `KeyHook` and reports `[(0, Press), (0, Release), (1, Press), (1,
Release)]` with zero fires during the control. Two phases because one process can hold one
`WH_MOUSE_LL` hook usefully at a time, and the raw bytes and the shipped decision are different
claims.

### Three of the five buttons are refused, and the refusal is the feature

Left, right and middle cannot be bound. The hook does not swallow the event — the same rule the
keyboard hook has always followed — so binding the left button would start dictation on **every
click in every application** while the click still went through. That is not a limitation to
apologise for on a screen; it is the only correct answer, and `keys::REFUSED_MOUSE_BUTTONS` names
each one so the message can say *which* button rather than "that one".

The two side buttons are not free either, and the difference is why `Accelerator::MouseButton` is a
third variant rather than a `ModifierOnly` with an odd spelling in it. A hooked chord takes nothing
away from anything else. A hooked mouse button leaves `Mouse4` going Back in every browser on the
machine — so the two need different sentences, and an enum variant is how you stop one borrowing
the other's.

### `armed` and `pressed` are one key apart, and sharing them breaks the microphone

Push-to-talk needs `ChordWatcher::key_up` to report a release. The obvious implementation reuses
`armed`, the field that already tracks which chord has fired. It is wrong, and the way it is wrong
is specific:

`armed` is cleared by **any** key going up, including an ordinary one. It has to be — otherwise
holding `Control+Super` and tapping an arrow would never re-arm, which is rule 1. So under a shared
field, `Ctrl+Win` held while an arrow is tapped would report the *arrow's* release as the chord's,
and the real release — the user actually letting go — would find nothing to pair with and report
nothing. Under push-to-talk that is a microphone nobody can close: you let go and the recording
continues.

`pressed` is therefore its own field, cleared only when a key **of the chord** goes up.
`Chord::contains` is the predicate, and
`an_unrelated_key_coming_up_during_a_hold_is_not_the_release` is the test that would have caught the
shared-field version.

`reset` deliberately reports nothing. It is called when the watcher has lost track of the keyboard,
and a caller in that position cannot also claim the key came up; synthesising a release would stop a
dictation on a key the user is still holding, which is worse than the state being recovered from.

### The race that only a short hold reaches — and push-to-talk is made of short holds

The issue's own framing was that press-then-release needs no new runtime method, because
`DictationRuntime::toggle` already branches on state: a press finds `Idle` and starts, a release
finds `Recording` and finishes. That is right about the state machine and wrong about the
concurrency.

`on_hotkey` spawns. `Dictation::start` sets `Recording` **only after the microphone is actually
open**, which on Windows is tens to hundreds of milliseconds of `cpal` work. Two independently
spawned tasks have no order between them, so a release arriving inside that window finds `Idle` and
does one of two wrong things depending on which wrong thing you picked: nothing, leaving the
microphone open with no second half to close it, or `start` again, which is worse.

The fix is not a lock — `tokio`'s mutex is fair among *waiters*, and the two tasks do not reach it
in a defined order. It is that **the hold is one task**: the press starts, waits on a channel
created for that press, and finishes when the key comes up. There is no ordering left to get wrong
because there is only one order. A fresh channel per press rather than a long-lived flag, so a
release belonging to an earlier hold cannot end a later one.

Labelled honestly in the code as **reasoned, not measured**: nothing here has driven two spawned
tasks into that window on purpose. The argument is that the window exists and the design removes it,
which is cheaper than proving how often it is hit.

### What was deliberately not decided

**The shipped default is unchanged.** `hotkey.<action>.mode` ships as `toggle` for all five actions.

Push-to-talk is one of the candidate answers to KNEG-115 — an accidental `Control+Super` on the way
into `Ctrl+Win+Left` brackets a few hundred milliseconds under a hold instead of bracketing
everything the user does before the next accidental press. That makes `Hold` a genuine candidate for
the *default*, and choosing it here would settle an open question as a side effect of adding a
feature, which is with some irony the exact class of failure KNEG-115 describes. So:
`nothing_ships_holding_by_default` is a tripwire test that asserts it, and says in its own comment
that a commit changing it is deciding something that belongs to the repo owner.

### The consequence that is written down rather than discovered

**Push-to-talk makes `InsufficientSpeech` routine rather than rare.** A hold shorter than a syllable
records less than a syllable, the gate refuses to send it — correctly — and the outcome reads
*"Nothing was sent: insufficient speech."* Every mistimed tap ends that way. Somebody who reads that
line as a fault will go looking for a fault that is not there, so it is in `HotkeyMode`'s doc
comment, in `hotkeyModeLabel`'s detail line under the control itself, and in `PLATFORM_SUPPORT.md`.

### What is measured, and what is not

Measured on this machine: the `mouseData` encoding, the mouse hook firing once per press and once
per release for the right index with a live control, and `keyhook_probe` now reporting **two
releases** as well as two presses for two injected chords.

Not measured: the plugin path. Push-to-talk on an ordinary accelerator such as `Control+Shift+D`
routes `ShortcutState::Released` through the same `shortcuts::act`, and nothing in this repository
has run it — reaching it needs a built application with a WebView2 beside it and a person holding a
key. `register_one`'s doc comment says so in those words rather than calling it supported. Also not
measured: any physical side button. Every press in the probe is injected, which is `keyhook_probe`'s
limitation too and is KNEG-47's shape.
## 2026-09-01 (KNEG-113) — the sidecar could be proved to exist and never to be the right one

`kw_sidecar::locate` answers "is there an executable at this path". That is the only question
anything ever asked about an installed sidecar, and it is not the question that matters. The
manifest pins a SHA-256, `install` checks the download against it, and then **the answer is thrown
away**: nothing on disk records which archive the files came from, so from the moment an install
finishes the application can no longer tell its own sidecar from any other.

The consequence was visible on this machine and had been for a week. `sidecars/whisper` dated from
2026-08-25; the pinned archive extracted on 2026-08-31 into `sidecars/whisper.incoming` carried
`bench.exe` and `command.exe` that the installed copy did not. Two directories, one demonstrably
behind the other, and no code path anywhere that would have noticed.

That `.incoming` directory was not incidental either — see the last section.

### Detection first, because replacement without it is a button nobody can aim

Three directions were on the table and the one chosen was the smallest: **make an installed
sidecar's identity knowable.** `install` now writes a `.kw-install.json` into the directory it
promotes, naming the pinned archive's digest, the URL, the upstream repo and release, and when. A
new pure function, `kw_sidecar::record::freshness`, compares that digest against the one this build
pins.

**It is written into the staging directory, not into the install directory.** So it is carried
across by the same `rename` as the files it describes, and there is no window in which a directory
exists without its record or holds a record left over from the extraction before. Written after the
rename instead, a crash between the two steps would leave an install whose record described the one
it replaced — worse than no record, because it reads as a measurement. A test makes the promote fail
at the replace step and asserts the record is already in staging by then.

**What it proves and what it does not**, because the difference is the whole value of writing it
down. It proves this directory came out of the archive with that digest. It does **not** prove the
files are still those bytes: nothing re-hashes an installed executable, and the record is an
ordinary JSON file that anything with write access could rewrite. A staleness check, not a tamper
check.

### Absence is *unknown*, and that is the one judgement in the type

`Freshness` has four states and the fourth is the one that took thought. An install made before this
existed has no record. It is neither current nor stale — nobody measured it — and this machine has
exactly such an install right now, which is what made the temptation concrete:

- Calling it **current** would excuse a binary the pinned archive has demonstrably moved past.
- Calling it **stale** would claim a comparison that never happened, and would have to offer two
  digests when only one exists.

So `Unknown(Unrecorded)` is a first-class answer with a reason attached: no record, an unreadable
record, a record from a schema this build does not understand, or a record naming a different
sidecar. The last two are not defensive padding. A schema number nothing checks is a schema number
that does nothing; and two unrelated digests compare unequal, so without the sidecar-name check a
record copied between directories would report a confident `Stale` built on a comparison of two
things that were never comparable.

The screen says it in those words too. `Settings > Transcription > Local` had one claim about the
server binary — *Installed* — which stayed true while the binary fell behind. It now carries the
freshness sentence under it, and an **Update the server binary** button whenever the answer is not
`current`.

### Refresh is stop, replace, restart — and the alternative was measured before it was rejected

The application owns the process, so it can put the server down for the length of the install and
bring it back. `local_server_refresh` reads the running model *before* stopping (the stop clears
it), installs, and starts it again either way — a refused download must not leave someone whose
dictation was working a moment ago with nothing and no explanation. `refresh_failure` is a pure
function precisely so that second half is testable: "your download failed" and "your download failed
and dictation is now down" are different sentences and the user needs the right one.

The alternative was rename-aside: move the running install out of the way, unpack over the vacated
name, clean up later. It was rejected on a measurement, not a preference.
`crates/sidecar/examples/running_exe_lock_probe.rs`, run on this machine on **2026-09-01**:

```text
control  remove a directory whose exe is not running   Ok
control  rename a directory whose exe is not running   Ok
live     remove a directory whose exe is running       Err os error 5 (Access is denied)
live     rename a directory whose exe is running       Ok
live     remove it after renaming it aside             Err os error 5 (Access is denied)
live     create a new directory at the vacated name    Ok
```

The first four confirm what was recorded on 2026-08-31 by a different session. **The fifth is new
and it is the one that decided this.** The lock is on the mapped image file, not on the path: moving
the directory does not make it deletable. Rename-aside buys a shorter outage and inherits a
directory that cannot be removed until the process ends — which needs a cleanup pass at some later
launch, a `.replaced` name `install` must tolerate finding, and a story for what happens when the
cleanup itself fails. More machinery, for the case where the user has explicitly asked to replace
the thing they are using.

The probe has controls for the reason `kw_storage`'s `native_path_probe` does: that probe's own
first version reported failure for every case including the control. And it refuses to report a case
whose process is not in the process table with its image inside the directory under test — the
discarded 2026-08-31 attempt used `timeout.exe`, which exits at once without a console, and produced
six meaningless passes.

### The permanently stuck install, which existed regardless of any of the above

`install` began with `remove_dir(&staging)?` on a single `{name}.incoming` path. One leftover
directory that would not go away — a file a scanner still had open, a hand-made directory with the
wrong owner — and **every future install of that sidecar failed at the first line, for ever**, with
nothing the user could do from inside the application. It ended with the same call and the same `?`,
so a scanner holding the downloaded `asset` file open turned an install that had already succeeded
into a reported failure.

Both are gone:

- `take_staging` tries up to ten names, `{name}.incoming` then `{name}.incoming.N`, and takes the
  first that was **absent or emptied by this call** — never one it merely managed to write into,
  because whatever a previous attempt left there did not finish verifying and promoting it would put
  unverified bytes where the sidecar goes. Only ten exhausted names are an error, and that error
  carries the operating system's own last words.
- `sweep_staging` runs after a successful promote and is best effort. The user has what they asked
  for; a directory that will not delete is tens of megabytes, not a failure. It also clears whatever
  a previous run stranded, which is how this machine's week-old `whisper.incoming` goes away.

Replacing the *install* directory stays fatal — `rename` cannot land on a directory that is still
there — but it is now `InstallError::InUse` when the refusal looks like a permission problem, which
is the caller's cue and the user's one actionable sentence. A full disk and a running server are not
the same advice, so every non-permission failure stays a plain `Io`.

### What did not land

Nothing re-hashes the installed executable, so a file corrupted or replaced on disk after an install
still reads as whatever the record says. Nothing checks freshness at startup or on a schedule: like
every other download here, a refresh happens because somebody pressed a button.

`local_llm.rs` and `local_asr.rs` install sidecars through the same `kw_sidecar::install` and so get
the record for free, and at the time this was written neither read it. **Both report freshness now**
— KNEG-128, same day, which also found that `diarization.rs` and `media.rs` make four non-readers
rather than two. llama.cpp gained a refresh button with it; sherpa deliberately did not, because the
streaming meeting server runs out of that directory and nothing ever stops it. See that entry.

**The install half is measured; the stop and the restart around it are not.**
`tests/real_install.rs` was run against the real network on 2026-09-01 — not a fixture — and it now
asserts three more things: the record beside a fresh install names the digest the manifest pins,
`freshness` reads `Current` off it, and a reinstall with a junk `whisper.incoming` in the way both
succeeds and takes the leftover with it. Both halves of the staging fix therefore have real bytes
behind them. Everything else is 37 tests and 37 mutations, every mutation killed by the test that
names it.

What has not happened is a **press**. The stop-install-restart sequence is reachable only through an
`AppHandle`, so nothing has exercised it. The by-hand check is: on this machine, whose sidecar
reports `unknown`, with `tiny` resumed, press **Update the server binary** and confirm the directory
gains `bench.exe`, the row turns `current`, `whisper.incoming` is gone, and dictation still works.

---

## 2026-09-01 (KNEG-84) — the documentation was wrong, and only a probe could say so

The sixth meeting client, and the only one whose wire format was **measured**
rather than read. Worth recording is not the client — it is four hundred lines in
the shape of the five beside it — but what measuring bought that reading could not.

### One documented fact was false, and it was the load-bearing one

`sherpa-onnx-online-websocket-client --help` says *"16-bit samples"*. The wire
carries **`f32` little-endian**. As `f32` the values are speech inside ±1; as
`i16` they are noise.

A client written from that sentence would connect, complete a handshake, send
every frame successfully and transcribe nothing — with no error anywhere, because
this server does not report one. It would have looked like a model problem, or a
microphone problem, or a "streaming does not work on this machine" problem. The
cost of *not* probing was not a day of work; it was a defect with no symptom
pointing at its cause.

The probe was cheap for the same reason it was decisive: the reference client
and the real server were both already on disk, so the measurement was two
processes talking to each other with our code listening in between.

### The control came first, and it is why the rest is evidence

Reference client against real server, reproducing the model's own
`test_wavs/trans.txt`. Then our client against the real server, producing the
same transcript. Only then was the fault run meaningful. Without the first step,
"our client got nothing back" and "this model gets nothing back" are the same
observation.

### The finding that shaped the design was a negative one

`--fault` sent an unparseable TEXT frame and a three-byte BINARY frame before
the audio. **The server answered neither, and the transcript came back
byte-identical to the clean run** — the odd-length frame did not even knock the
sample stream out of alignment.

So this provider has no error channel. That is the exact inverse of the four
cloud clients: `fault.rs` exists because those four read a provider-reported
error and carried on, and here there is nothing to read. The consequences are
concrete rather than philosophical:

- `Incoming` has **no `Failed` variant**. Adding one nothing can produce would
  suggest a channel exists.
- `SessionFault` for this client can only ever be `Transport` or `Unreadable`.
- There is a test named `no_error_frame_is_reachable_from_this_client` whose job
  is to be **deleted deliberately** if sherpa ever grows an error frame — rather
  than a `Failed` arm quietly never firing, which is how a dead branch outlives
  the fact that created it.

A negative measurement is easy to leave unrecorded because it produces no
output. This one decided more of the design than any positive one.

### `text` is cumulative, and appending would have been silently plausible

Every update repeats the whole segment so far. A client that appended would
produce *"After After early After early nightfall"* — wrong in a way that reads as
a model artefact rather than a client bug, especially to somebody who has heard
a streaming recogniser revise itself. `Segments` is that rule and nothing else,
and it is tested against the recorded thirteen-update sequence rather than
against a live model.

The socket half therefore **mutates** its event vector rather than only appending
to it, which no other client here does. Each reader holds the index of its own
current segment; indices stay valid because nothing is ever removed.

### Two mutations in my own harness were no-ops, and the harness said so

Recorded because the lesson keeps recurring. I wrote ten mutations against the
rules; eight were caught and two "passed", which looked like two weak tests. Both
were actually **non-mutations**:

- `split_off(taken)` — `split_off(taken.min(len))`. `taken` can never exceed
  `len`, so `.min` changed nothing.
- `chunks_exact(2)` — `chunks(2)`. The `try_from` inside the `filter_map`
  already rejects a one-byte chunk, so the odd trailing byte is dropped either
  way. Two independent guards, and no single-line edit defeats both.

The second is the more interesting: the test was fine and the *code* was
belt-and-braces. Replacing the whole expression with one that pads did fail it.
Both are now noted in `samples`' own doc comment, because "this test survived a
mutation" is worth nothing until you know whether the mutation was real.

### Deliberately not done

**No reconnect.** No session limit, no token rotation, no billing clock — the
server is a child process. `capabilities.reconnect` is `false` rather than a
recovery path that does not exist.

**`language_detection: false`.** The one capability that differs from `local` in
a way a user feels. A sherpa online model is built for one language and there is
no per-request detection the way `whisper-server` has; declaring it true would
make `validate` accept a session with no language and quietly transcribe English
whatever was spoken.

**The server is not started at launch, and not by the settings click either.**
630 MB of model for a user who never records a meeting, or for one who is only
choosing a model, is a cost neither action advertises. `MeetingRuntime::provider`
starts it when a meeting begins and returns the same port if it is already up.

**`sherpa-stream`'s targets are `not-published`.** The binary ships inside the
offline CLI's archive and is already extracted beside it, so there is nothing to
download. Marking the targets published would have described an install path
this code does not use and cost 24 MB a second time. What the entry supplies is
the two things a path cannot: a port range, and the command fragment that lets
the reaper prove a leftover server is ours — `sherpa`'s own fragment
(`sherpa-onnx-offline`) does not match `sherpa-onnx-online-websocket-server`, so
without an entry an orphaned server would hold its port until the machine
restarted.

**`not_offered` was deleted rather than reworded.** It returned a sentence
saying streaming models could not be used because no client existed. That
stopped being true, and a check that can no longer fire is a check that stops
being read.

### Driven the same day, and the distinction held all the way to the end

A real meeting was recorded through it and the words arrived as spoken. So the
feature works — and the thing worth recording is that **every claim between
"the wire is measured" and "a person used it" turned out to be a separate claim,
and one of them was false.**

The wire was measured. The rules were tested and mutated. The integration test
drove the shipped client against the real server. All green, and the first
person to try it still could not start a meeting: the language caveat pointed at
the field *below* it while rendering underneath that field, `corti-realtime` had
carried the same flag for days without anyone reaching a screen where the
sentence rendered, and the wrong one of the two Nemotron models sorts first and
refuses to install. **None of those is reachable from any test in this
repository.**

The same shape as KNEG-94 earlier the same day, and not a coincidence: both were
finished the moment the measurement existed, and finishing the measurement is
not finishing the feature.

**Still not covered by that run:** *Stop and transcribe*, and the saved note.
The live transcript and the saved one come from different passes, so words on
screen say nothing about the note. And the fault path — the client raises the
error, but nobody has watched it reach `MeetingOutcome::Failed` rather than
being flattened into an empty transcript somewhere above.

---

## 2026-09-01 (KNEG-94) — a defect went looking for a capability, not the other way round

Teams calls are now detected, by the microphone the client holds rather than by a process name.
The code is small. What is worth recording is the sequence of judgement calls, because four of them
were about **what a measurement is worth** rather than about what to build.

### The prediction the plan made was backwards, and that is the interesting part

`COMPLETION_PLAN.md` chunk H said the mic listener had no consumer and that a capability with no
consumer is scaffolding this workspace does not build ahead of time. That reasoning was sound and
the conclusion was wrong, in a way worth naming: it assumed capabilities wait for features. What
actually happened is that a **defect nobody could fix any other way went looking for a capability**.
Teams had been unofferable for three weeks; three separate measurements said no process name would
ever fix it. That is a much stronger pull than a feature request, and the chunk had no way to
express it.

Kept in the doc rather than quietly corrected, because "no consumer yet" and "no consumer, and none
will come" read identically in a backlog and only one of them is safe to act on.

### The control was made cross-process on purpose, and the easy version proves less

`mic_users_probe` could have opened the microphone itself and checked it appeared. That was the
first design. It was replaced with a second process launched via `--hold`, because an audio session
belongs to a **process** and the question the issue asks is whether this code can see a session
belonging to *somebody else*. That those are the same mechanism is true, and it is an argument —
and an argument is exactly what a measurement is supposed to replace.

### The first real run passed its control and was still unreadable

This is the one worth remembering. The probe printed `nothing is capturing` and `PASS` on the
control. Both true. Both useless: **nothing in the output said whether Teams had been open at the
time.** "Teams is running and not capturing" and "Teams is not installed" print the same words and
mean opposite things, and only one of them is the run the issue needed.

The same failure shape as KNEG-88's empty file, one level up. There, a negative assertion passed
against nothing and only a positive control noticed. Here the control existed and was passing —
the missing positive was one layer above it, about the *subject* of the measurement rather than the
apparatus. A control proves the instrument works. It does not prove the sample was in the machine.

Fixed by having the probe print what `meeting_detect::detect` itself sees running, reusing the real
detector rather than matching a name by hand: a second opinion computed beside the first is a second
implementation that can be right when the first is wrong.

### The microphone may strengthen a match and never create one

The rule that keeps this honest. A process holding a capture device that is not a known client and
not on the user's own list is **not reported at all**, no matter how loudly it is capturing. The
alternative — report anything holding the microphone — would mean this application's own
dictation offers to record itself, and would make "something is recording you" a claim a process
detector has no business making about a browser tab.

Tested, and the test was mutated: a version that lets the capture list invent a detection fails four
tests.

### Two signals, and refusing to flatten them into one boolean

`CptHost.exe` cannot exist unless a Zoom call is running. A held microphone is also Teams running
its own audio test, and a call the user placed that nobody joined. Both are `in_call`, and they are
not the same claim.

So `in_call` became a derived value and `CallSignal` carries which one fired, all the way to the
offer window, which now says *"Microsoft Teams is using your microphone — that is usually a call,
but a test call looks the same"* rather than asserting a meeting. The offer is the one place a wrong
detection costs the user something: a window over their work about a meeting they are not in. A user
told the wrong thing can only work out which signal was wrong if the window says which one fired.

### Sealing a field rather than trusting a convention

`in_call` and `signal` are two serialised fields carrying one fact, which is how they come to
disagree. Rather than a comment asking future callers to use the constructor, `in_call` is a
**private field** with an accessor and `Detected::new` is the only way to set it — serde and
`ts_rs` both still export it, so the screen is unaffected. The desktop crate's test helper stopped
compiling immediately, which is the proof it works.

The same argument `MeetingRuntime::live` already makes for deriving "stopping has not begun" from
the lock instead of keeping a second flag beside it.

### One process table, two readings

`survey()` takes the process list and the capture list from a single `sysinfo` refresh. Two
refreshes are two instants: a process that starts in between appears in one list and not the other,
and the resulting `Detected` names evidence that had already gone. Cheaper too, and the poll runs
every five seconds for as long as the application is open.

### What was deliberately not built

The **calendar cross-check** KNEG-94 named as a possibly cheaper alternative. It is complementary
rather than competing — it catches scheduled meetings and misses ad-hoc ones, which is the exact
inverse of what landed — and doing both in one pass would mean neither was measured on its own.

The **UIAutomation text monitor**, KNEG-63's other half, is now closed as declined rather than
deferred. No consumer, and unlike the microphone half no defect pointing at it either; a continuous
read of the focused element is a materially larger privacy surface than anything else here and there
is nothing on the other side of the scale. A decision should read as a decision, not as unfinished
work.

### Still not done, and it is not the same claim

**Nobody has watched the offer window appear during a real Teams call.** The signal is measured and
the wiring is tested. Those are two different claims and the second does not imply the first —
recorded in `BENCH_CHECK.md` item 6 rather than left implied by a closed issue.

---

## 2026-08-31 (KNEG-115) — measuring the cost without choosing the fix

KNEG-115 says its own fix *"belongs to the repo owner rather than being chosen in passing"*, because
it changes when the **default** hotkey fires. It also says what would inform that decision and had
not been done: *"whether that is noticed depends on how quickly the overlay appears and whether the
second press stops it — not measured, and worth measuring before deciding severity."*

That half needs nobody's hands. Five tests now drive the shipped `ChordWatcher` with the exact
key-down and key-up order a person produces:

```
Ctrl+Win+Left, once                       1 fire
Ctrl+Win+Left, twice                      2 fires
Ctrl+Win held, arrow tapped four times    1 fire
Ctrl held, Win tapped twice               2 fires
LeftControl+LeftSuper (a sided chord)     1 fire
```

### Two fires is the finding, not one

`dictation::on_hotkey` calls `DictationRuntime::toggle`. So an even number of fires is not a pair
that cancels out: **the first starts recording and the second stops it, transcribes what the
microphone heard in between, and puts the text at the cursor through a synthetic paste.**

A user on the default binding who switches virtual desktop, works for a while, and switches back has
recorded that interval and had a transcript of it typed into whatever had focus. The issue said "a
dictation starts on the way past". It is worse than that, and the difference is the difference
between an annoyance and something that types your speech into a document.

The toggle-and-paste half is read from `on_hotkey` and the inserter, not executed — the fire counts
are measured, the consequence of a fire is traced. Both are labelled that way where they are
written.

### What bounds it, in both directions

Holding Ctrl and Win while cycling four desktops is **one** toggle, not four: rule 2 refuses while a
non-modifier is held, and the arrow going back down brings one with it. But releasing and
re-pressing either modifier re-arms, so Control held with the Windows key tapped twice **is** two
toggles.

And a sided chord is no escape from this one. It is from AltGr, which is why KNEG-37 and this are
separate issues — `LeftControl+LeftSuper` is a prefix of `Ctrl+Win+Left` exactly as the unsided
chord is.

### The fix is still not chosen, and the exposure is now disclosed

All three options in the issue cost something real, and the one it calls honest — fire on release
— adds the hold time to every dictation start, which is the latency `ModifierOnly` exists to
avoid. That is a trade for the person who lives with the default, and choosing it in passing would
be deciding the behaviour of the shipped default on the way past — which is, with some irony,
exactly the failure being warned about.

So `hotkey::prefix_cautions` warns and changes nothing, which is the pattern KNEG-37 established for
AltGr and needs no decision at all. On screen:

> Control+Super fires as soon as both keys are down, so it also goes off on the way into any longer
> shortcut that starts with them. On this platform that includes Ctrl+Win+Left, which switches
> virtual desktop. Dictation is a toggle, so two of those record everything in between and put the
> transcript wherever the cursor is.

Which chord collides with which keystroke is asked of the **real matcher**, exactly as
`altgr_collision` is, for the same reason: a predicate reasoning about modifier names would be free
to drift from the behaviour in precisely the cases nobody wrote a test for.

**Every modifier-only chord gets a caution, not only the five with a named example.**
`COMMON_PREFIXES` cannot be exhaustive — every two-modifier chord is a prefix of something — and a
warning that appeared only for the listed keystrokes would read as a clean bill of health for the
sixth. `Alt+Super` gets the general sentence and no invented example.

**Only the handled action is told what it costs.** Four of the five actions have no handler;
telling one of those that it records and pastes would invent a consequence it does not have.

### Two cautions, kept apart

`Control+Alt` on a Nordic layout now produces two sentences, and a test that asserted *exactly one*
had to be re-scoped rather than relaxed. They are different facts: AltGr depends on the layout,
being a prefix depends only on the chord having two keys, and either holds without the other. On a
US layout the AltGr one disappears and the prefix one stays — a US keyboard cannot press AltGr and
can certainly press Ctrl+Alt+Delete.

That also closes a loop worth noticing. KNEG-37's caution now suggests a sided chord as its remedy,
and KNEG-35 made sided chords typable. Someone who follows that advice gets a chord that is still a
prefix, and `the_altgr_remedy_is_not_a_remedy_for_this` is the test that says so out loud rather
than leaving them to find out.

---

## 2026-08-31 (KNEG-117) — the brute force was the weaker half, and the arithmetic says so

KNEG-88 found one intermittent test in `kw-storage`, and its real content was that a credential
check had been passing against an empty file. The obvious follow-up was to run the same hunt over
the whole workspace.

**Run: 40 rounds, 33 binaries, 1,320 runs, every thread count, control passed, nothing found.**

### What that is worth, stated rather than rounded up

Not much, and the arithmetic is why. The `kw-storage` flake fired **twice in four hundred runs of
that binary** — about 0.5% per run. Forty rounds is forty runs of each binary, so the chance of
catching something at that rate is about **18%**. A clean result at this depth is closer to "not
looked at properly" than to "clean", and matching KNEG-88's confidence across the workspace would
be roughly four hundred rounds.

Worth writing down because the temptation is to read 1,320 runs as a big number. It is a big number
of runs and a small number of *rounds*, and rounds are the unit that matters here.

> **Superseded 2026-09-01.** The four hundred rounds were run. This paragraph said they would cost
> "several hours of a machine doing nothing else"; timed, a round is 22 seconds and the whole run
> is about two and a half. That estimate is the only reason the depth was deferred twice, and it
> was wrong by a factor of four. See the 2026-09-01 (KNEG-117) entry.

### So the second half was a search rather than a loop

The defect KNEG-88 found has a shape: **an assertion that something is absent, checked against a
resource that may not have been fully written.** That is greppable, and unlike the loop it does not
depend on catching a rare event.

Seventeen candidate sites across the workspace; ten had a positive claim beside the negative and
were fine. Of the remaining seven, five were false positives of the search (a multi-line
`assert_eq!` the pattern did not see, or a test whose whole subject is that nothing happened). Two
were real and both are now hardened:

**`sherpa::the_staged_wav_is_cleaned_up_even_when_the_process_cannot_start`.** It asserted no file
matching `sherpa-input-` remained. That is trivially true of a run that never staged one — so a
`transcribe` failing before the write, or a rename of the prefix, would have left it green while
proving nothing. It was not vacuous today, which is exactly the problem: nothing would have said so
when it became vacuous. It is a claim about raw audio not being left on disk, which is worth more
than most. The control is the scratch directory, which now does not exist beforehand, so
`transcribe` creating it proves the function reached `create_dir_all` — and the WAV write is the
next statement. Mutating `transcribe` to fail before staging turns it red.

**`discovery::no_temporary_file_is_left_behind`.** "Nothing else is in the directory" is also true
of an empty directory. One line asserting the discovery file is there turns it from a tautology into
an observation.

### The lesson, in a form that survives

A negative assertion is only as good as the positive one beside it. Neither of these was broken;
both were one silent change away from being meaningless, and nothing in the tree would have said so.
The search that finds them takes minutes. The loop that finds them takes hours and mostly does not.

`tools/flake-hunt/hunt.mjs` also learnt something in the process: its first workspace run reported
`generated_ipc_types_match_the_rust_types` failing nine times in thirty, which was a Rust type and
its generated TypeScript being edited while the hunt ran. A false finding that names a real test is
worse than an ordinary one, so a run whose binaries changed underneath it is now reported as **void**
rather than as a result.

---

## 2026-08-31 (KNEG-38) — the ambiguity is real, so it is named rather than resolved

WASAPI loopback from an output device with nothing playing delivers **no callback at all**, not a
stream of zeros. So *"nobody else spoke"* and *"the loopback never worked"* produce byte-for-byte
the same nothing, and both arrived on screen as `MeetingOutcome::NothingTranscribed` — along with a
third case that is neither of them.

### What could be separated, and what could not

Nothing this side can separate the first two. Any code claiming to would be inventing evidence, and
the label already said so honestly: *"no speech was recognised, or the provider returned nothing"*.

The third case was hiding in there and **can** be separated: audio was captured, it was audible, and
none of it was transcribed. That is not a quiet meeting. It was being reported as one.

So `Delivered` is a four-way answer per capture — `NotCaptured`, `Nothing`, `Silence`, `Audio` —
folded in the recorder's drain loop as the blocks go past. The outcome carries one for the
microphone and one for the system audio, and the sentence branches on them:

* **either side heard something** — *"and none of it produced a transcript — so this is a
  transcription problem rather than a quiet meeting."* First in the ordering, because it is the only
  branch that should send anybody looking for a cause.
* **everything delivered was digital silence** — nothing was said within reach.
* **nothing was delivered at all** — *"this is what a meeting nobody spoke in looks like, and also
  what a capture that did not work looks like. They cannot be told apart from here."*

The last one is the honest version of the thing this issue asked for. The bench check's wording was
*"zero samples must read as nobody else spoke rather than a failure"*, and reading it that way would
be a guess in the other direction. What the screen now does is say which of the two it is looking at
— having ruled out the third — and explain why they look alike.

### An empty block is not a block of silence, and that is the whole fold

`Delivered::plus` distinguishes an empty slice from a slice of zeros. Collapsing them — the obvious
thing, since both contain no sound — would destroy the distinction before anything downstream could
see it. It is also the first thing the tests check.

The fold is monotone: `Nothing` to `Silence` to `Audio`, never back. A meeting that goes quiet for
its last half hour has still heard something, and the question is whether anything was ever audible
rather than whether the end was.

And it folds the **per-source** blocks, not the mix. The mix is the sum, where a silent system feed
under a live microphone is invisible, and telling those apart is the point.

### `NotCaptured` is not `Nothing`

A meeting recorded without system audio has a reason on `Sources` and no defect behind it. Starting
that tally at `NotCaptured` rather than `Nothing` is what stops the screen reporting a capture that
was never asked for as one that fell silent.

### Still on the by-hand list

With the speakers muted, confirm the sentence a real meeting produces. Everything above is measured
in unit tests against synthetic blocks; no meeting has been recorded since it landed.

---

## 2026-08-31 (KNEG-116) — asking the thread instead of inferring from its silence, and a state name that stops lying

`pump` returns `Ok` when its audio channel closes. At Stop that is exactly right. Reaching it while
a session is still held means the recording thread went without being asked — `record` returns on
`Finish`, on `Cancel` and on its command channel disconnecting, and all three run only after the
session has been taken. So that arm meant a panicked recorder, and the state reported was
`RecordingWithoutTranscript`, whose first word was the one thing that had definitely stopped.

KNEG-95 chose to report it anyway, on the grounds that it was strictly less wrong than the plain
`Recording` it replaced, and filed the rest.

### The fix is to stop inferring

A closed channel and a dead thread look identical from outside, and they are not the same fact. So
the thread says which, itself: a `Drop` guard reading `std::thread::panicking()`, writing
`RecorderEnd::OnPurpose` or `RecorderEnd::Panicked` into a `OnceLock` the runtime already holds.

A guard rather than a line at each return, because `record` has five exits and **a panic is not one
of them** — anything written at the returns would miss the only case worth naming.

**The drop order is load-bearing.** The guard is the first binding in `record` and `audio` is a
parameter; bindings drop in reverse order of declaration, so the guard runs before the sender does.
The cell is therefore written before the pump can observe its channel closing. That is the same
ordering trick `Live::transcript_failure` already depends on, and it is why reading the two together
is meaningful rather than a race.

### A fifth state, now that there is something to distinguish

`MeetingState::Stalled`: nothing is being recorded and nothing asked for that. `is_running()` stays
true, deliberately — whatever was captured before the thread went is on disk, and a state that hid
Stop would throw the recording away in order to report that the recording stopped.

Named for what is true, which is the rule KNEG-95 set for its own state and the rule this one was
filed for breaking.

### The compiler could not catch what adding it broke

`tsc --noEmit` passed with `stalled` added to the union and both screens still asking
`state === "recording" || state === "recording-without-transcript"`. **Adding a member to a string
union does not break a `===` comparison.** So the notification window and `Settings > Meetings`
compiled cleanly and would both have answered *not recording* — offering "Start recording" for a
session that was still held, with a WAV still growing on disk. That is the precise defect KNEG-95
exists to fix, reintroduced by a state added to fix a neighbour of it.

`labels.meetingIsLive` is an exhaustive `switch` with a `never` in the default arm. A sixth state
now fails to compile there, rather than quietly losing a button somewhere else. `meetingStateLabel`
sits beside it for the same reason.

That is worth separating from the rest: the type system was consulted, said yes, and was wrong to.
It was not asked the question it can answer until the switch existed to ask it.

### Verified

`the_recorder_guard_can_tell_a_panic_from_a_return` runs a real thread into a real panic rather than
reasoning about `std::thread::panicking()`, which is easy to write backwards and impossible to see
is backwards. The rest is pure: three distinct sentences, asserted distinct from each other, because
a formatter that collapsed two of them would pass every individual assertion.

### Still not observed

No recorder thread has ever been seen to panic in this project. The case is reached by reading
`record`'s exits. What changed is that it is now reached by the thread saying so, and that the
screen says the true thing when it does.

---

## 2026-08-31 (KNEG-96) — between "died during startup" and "we killed it" there was no observer at all

The local whisper server went away twice in one afternoon on 2026-08-28, unused, and the record
could not say whether it had crashed, been killed, or exited cleanly. `start` reports a child that
dies *during* startup, and `stop` ends one deliberately. Between those two, nothing was watching:
the only trace of a death was a later connection refused with no explanation attached to it.

This does not reproduce the exit. It makes the next one explain itself: a watcher task polls the
child every five seconds and, when it finds it gone without `stop` having been called, logs the
exit status, how long it had been running, and the tail of its diagnostics — then clears the PID
record.

### It deliberately does not restart anything

The transcription and import paths already health-check the recorded port and revive the remembered
model themselves (`2fe5825`). A second thing that restarted servers would race them. What was
missing was never recovery; it was an explanation, and conflating the two would have produced a
supervisor nobody asked for.

### stdout is still thrown away, and that is the reason the tail is safe to log

`start` sends stdout to `Stdio::null()` precisely because it can carry recognised text. stderr
carries the loader and model diagnostics — which is exactly what a post-mortem needs, and is
already shown to the user by `ProcessError::ExitedDuringStartup`. The distinction was already made
in the code; this relies on it rather than re-deciding it.

### Two ways to get this wrong, both closed

The child handle moved behind an `Arc<tokio::sync::Mutex<..>>`, because `stop` needs it too and a
watcher parked in `wait()` would hold it for the life of the process. Polling instead, at five
seconds: this is a post-mortem, not a supervisor, and nothing acts on the answer.

And `stop` sets a flag **before** it kills. Without it, every deliberate model switch would log an
unexplained death — which is precisely the noise that makes a real one easy to scroll past. The flag
is read again on the far side of `try_wait`, because `stop` can set it while the lock is held.

### What a test can see

A log line is not assertable, so the tests watch the PID record instead: same code path, same
trigger, and a record naming a dead process is independently the thing that makes a reused process
id look like a live server. Three mutations — removing the watcher, making it notice and say
nothing, and dropping the flag from `stop` — each caught by the test written for it.

### Still open underneath it

**Why the server exits.** That needs the next occurrence, and until one happens the two candidates
this issue names are still both live. What has changed is that the next occurrence will say which.

---

## 2026-08-31 (KNEG-88) — the flake was the control, and the control was the only honest thing in the test

One line in the carry-forward: *"a `kw-storage` test failed once and passed on re-run"*. The report
did not say **which** test, which is half of why answering it needed a session rather than a minute.

`tools/flake-hunt/hunt.mjs`, written for this and committed: build the test binaries, ask **cargo**
which they are, run a control, then run them repeatedly with the thread count cycling through 1, 2,
4, 8 and 16. Four hundred rounds. It named the test:

```
kw_storage-... :: import::tests::no_plaintext_token_appears_anywhere_in_the_target_file
  failed 2 time(s), at --test-threads=8
```

Two in four hundred, and only ever under eight threads — which is to say, only ever on a machine
busy enough to matter.

### Which assertion, and why that is the whole finding

Eighty more runs to catch the message. It was not either of the two security assertions. It was the
**positive control**:

```
panicked at crates/storage/src/import.rs:1035: the slot reference should be there
```

The test greps the bytes of `kw.db` for a refresh token and an access token and asserts neither is
there, then asserts a slot reference *is*. When the control fired, the main database file contained
**nothing** — the import had committed, `Db::close` had returned, and the rows were still in the
write-ahead log. So both "no plaintext token here" assertions had just passed against an empty file.

That is a false green in the exact place a false green is worst: a test whose whole purpose is to
prove a credential did not reach disk. And the only reason it was visible at all is that somebody
wrote the third assertion. **The flake was the control doing its job.**

### The fix is also the stronger claim

SQLite in WAL mode is three files, and a committed row can be in any of them. Greping only `kw.db`
was never what the test's name says: a refresh token sitting in `kw.db-wal` is exactly as leaked as
one in `kw.db`, and the old test would not have seen it. `every_database_file` reads the main file,
the `-wal` and the `-shm`. The negatives get stronger and the control becomes deterministic in one
change, because the row is now found wherever it actually is.

### Not left resting on a 0.5% event

A regression test that reproduces two runs in four hundred is not a regression test. Holding the
database open forces the same state every time — nothing has closed, so nothing has checkpointed —
and `a_row_that_is_only_in_the_wal_is_still_found` asserts the helper finds a row the main file does
not have. Its first assertion is the *premise*: if SQLite ever checkpoints eagerly enough to put the
row in the main file, the test says so rather than passing. Reverting the helper to the main file
alone fails it every time.

### `Db::close` was overstating itself

Its comment read *"Called on shutdown so the WAL is checkpointed rather than left for the next
launch to recover."* Measured: not always. SQLite checkpoints when the last connection closes and
skips it if it cannot take the lock. Nothing in the application depends on it, because the WAL is
recovered on the next open — but a test did, silently, and the comment now says which.

### What the hunter's own control caught

Its first version reported **100% failures with no output** under every thread count, including what
should have been the control. Each binary path carried a trailing carriage return — the producer
translated newlines, the consumer stripped only the line feed — so nothing was executed at all. A
harness that fails everything is measuring itself; the same mistake in the other direction would
have printed a clean result and been believed. The committed tool runs a control first and **refuses
to report clean unless it passed**.

And the detector has a control of its own: a deliberately unreliable test was planted in
`crates/storage/src/settings.rs`, the hunter run against it, and it named the test and the thread
counts it failed under — then removed. A flake hunter that has never caught a flake is a script
that prints "clean".

### The reporting gap, which was the fixable half of the original report

The tool parses libtest's own `test <name> ... FAILED` lines and reports the name, the count and the
thread counts. A non-zero exit with no named test — a crash, a binary that would not start — is
reported as exactly that rather than counted as clean.

---

## 2026-08-31 (KNEG-106) — the backpressure is the bound, and the first test for it was measuring the scheduler

`extract_tar_xz` filled a `Vec<u8>` with the entire decompressed tar before looking at one entry,
because `lzma_rs::xz_decompress` writes into a `Write` and offers no streaming `Read`. The
`max_bytes` ceiling counts bytes reaching `dest`, and the `Vec` was full before the first one got
there, so a `.tar.xz` was a memory bomb no ceiling could touch.

The fix is the option the issue called honest: run the decompressor on its own thread writing into
a `sync_channel`, and read from the other end. A full channel blocks the producer, so the bytes in
flight cannot exceed the channel's depth however large the archive decompresses to. **The
backpressure is the bound** — there is no separate check that could fall out of step with it.

### Two things it does not bound, both said out loud

`lzma-rs` allocates an LZMA2 dictionary whose size the xz header declares, inside the decompressor
and outside the channel. A few megabytes for any real archive, and not attacker-chosen here anyway,
since `extract` runs only after a mandatory SHA-256 matched. "Streaming" reads as "constant memory"
and it is constant *plus a header-declared dictionary*, so the comment says which.

And a decompression failure arriving after the consumer stops is not reported: dropping the reader
closes the channel and ends the producer. `extract_tar_gz` and `extract_tar_bz2` already have
exactly that property — their decoders are driven by the tar reader and stop when it does — so
this matches the module rather than adding a hole. A consumer that reads to the end always sees the
error, because it travels in-band as the last item rather than through a side channel.

### The first test was measuring the scheduler

It ran a producer and a consumer concurrently, counted how far the producer got ahead, and asserted
the gap stayed under the bound. It passed. Then the mutation harness widened the channel to a
hundred thousand blocks — **sixty-four gigabytes of queue** — and it passed again, because a
consumer that keeps up keeps the observed gap small whatever the queue can hold.

Two of three mutations sailed through. The replacement lets **nothing** read at all and asserts the
producer cannot finish a stream twice the bound, with a one-second timeout in the safe direction:
expiring is the pass, and only the producer *finishing* fails it, so a slow machine cannot cause a
red. An unbounded channel finishes it in milliseconds.

The second miss was subtler. Replacing `buf.chunks(BLOCK)` with `buf.chunks(usize::MAX)` — which
moves the unbounded buffer out of the `Vec` and into `BlockWriter`, where it is just as unbounded
and much harder to see — changed nothing observable, because a producer fed exactly one block at a
time behaves identically either way. It needed its own test writing a single oversized slice, not a
second assertion in the first one. Three mutations, three tests, each caught by the one written for
it.

The lesson is the one this repository keeps relearning in a new costume: a test that passes is not
evidence until something has made it fail.

---

## 2026-08-31 (KNEG-35) — one table, two readers, and a sentence with a hole in it

`keys.rs` has expressed `RightControl+Super` since the hook was written — the headline capability
of running a low-level hook at all, since no cross-platform shortcut API can see which side a
modifier is on. No accelerator a user could type reached it. `hotkey::MODIFIERS` had nine unsided
spellings, so `classify` read `RightControl` as *the key*:

```
     RightControl+Super -> Standard { accelerator: "RightControl+Super" }
RightControl+RightShift -> Invalid { reason: "it names 2 non-modifier keys; a shortcut may have one" }
```

The first went to the system register, the one mechanism that cannot tell the sides apart. The
second was refused with a message blaming the user's spelling for our own missing alias.

### One table, because two lists put both failures one edit away

The fix is not "add the sided spellings to `MODIFIERS`". Two lists of the same set drift, and they
drift in two directions with different symptoms: a spelling `classify` accepts and `Chord::parse`
does not becomes a shortcut the screen calls hookable and `KeyHook::install` then refuses; a
spelling `Chord::parse` accepts and `classify` does not is this issue.

So `keys::SIDED_MODIFIERS` is the one table, `Chord::parse` reads it, and `classify` reads it
through `keys::sided_modifier`. The unsided list stays separate and stays in `hotkey.rs`, because it
answers to something different — the shortcut plugin. That is why `Win` is absent from it and
present in the chord parser: the plugin rejects `Win`, and being stricter than the platform is a
different mistake from being looser.

`every_sided_spelling_the_chord_parser_takes_is_a_modifier_here` is what makes the shared table more
than decoration. A constant only one side actually consults looks identical from the outside.

### The third state nobody had a name for

Adding sided spellings as modifiers creates a case that did not exist before: `RightControl+D`. It
would have fallen through to `Standard` and been handed to the register, which cannot see the side —
strictly worse than the old behaviour, because the old behaviour at least refused.

Neither mechanism can take it. The register is the only thing that handles an ordinary key and
cannot see sides; the hook is the only thing that can see sides and `Chord::parse` refuses anything
that is not a modifier. So it is `Invalid`, with **both** ways out named — drop the key, or drop the
side — and a test asserting the classifier accepts each of them. A refusal that suggests something
the same field then rejects is the defect this issue mostly was.

The same reasoning removed a piece of advice: `explain()` told a user without the hook to *"add a
regular key, for example {accelerator}+Space"*. For a sided chord that produces an `Invalid`. It now
says there is no accelerator to fall back to, because there is not one.

### The caution can finally name a remedy, and it is checked rather than asserted

KNEG-37's AltGr warning deliberately named no remedy: a sided chord avoids AltGr completely, and
the settings field rejected every sided spelling. It accepts them now.

`keys::sided_escape` builds the suggestion from the chord that actually collided and then **passes
it back through `altgr_collision`** — the same matcher that produced the warning. AltGr being left
Control and right Alt specifically is a claim about when a chord fires, and this module's standing
rule is that such claims are answered by running the watcher rather than by reasoning about names.
If the candidate still collides, no remedy is named. Mutating `RightControl` to `LeftControl` in the
builder — the AltGr side — makes the re-check reject it and the test fail, which is what says the
re-check is load-bearing rather than ceremony.

### The hole in the sentence

Both AltGr cautions had shipped like this:

```
This layout types '@'                      with Control and Alt together, and Windows sends
```

Line continuations in the Rust string literals had lost their backslashes in an earlier edit, so the
source indentation became twenty spaces in the middle of a sentence on screen. `cargo fmt` does not
look inside a string literal, and the tests asserted `reason.contains("AltGr")`, so nothing caught
it — it was found by printing the text and reading it, which is the only thing that ever finds
this class.

`no_caution_has_a_hole_where_a_line_continuation_should_be` asserts no caution reason contains a run
of two spaces. Cheap, and it fails the moment a continuation is lost again.

### Not closed by this

A person typing a sided chord into `Dictation > Shortcuts` and watching it fire. The hook path is
measured — `keyhook_probe` injects `RightControl+RightShift` and sees two fires for two presses —
and the classifier is now measured, but the two have never been driven end to end from the screen.
That is KNEG-47's territory.

---

## 2026-08-31 (KNEG-114) — the control the gate asked for, and the hole the gate turned out to have

`meetings.detect` holds the allow and deny lists deciding which running applications count as a
meeting. Every link existed except the last one: the key is read by `Detector`, a command writes it,
a getter reads it, both are registered, both have IPC wrappers, both have dev fixtures — and no
screen called either. The lists were settable only by editing SQLite by hand. The fourth instance
of that pattern, and the first found by `tools/settings-audit/scan.mjs` rather than by a person
reading a screen.

The control is two textareas in the Detection section of `Settings > Meetings`, under the list of
running applications — deliberately, because the name to type is the one each detection there
reports. `parseKeyterms` became `onePerLine` and now serves all three fields, since keyterms and
both lists are the same thing on screen and are normalised the same way in Rust.

### The mutation the fix was supposed to fail, and did not

The habit is to break the fix and watch the gate go red. Replacing the call and leaving the import
in place kept it **green**.

The check was `new RegExp(`\\b${wrapper}\\b`).test(component)` against the whole file, so an
import satisfied it. Every one of the nineteen reachable keys had been passing on a test that a
screen could satisfy by importing a function and never calling it. Nothing was wrong in the tree —
the finding is that the tree being clean had stopped being evidence, which is the exact failure
`scan.test.mjs`'s own header warns about two paragraphs before this code.

### The obvious tightening was wrong, and the tree is what said so

Requiring an open parenthesis after the name separates `setMeetingDetectLists({` from
`setMeetingDetectLists,` in an import list. It also reported `localApi.enabled` as having no way in,
because `IntegrationsApi.tsx` writes:

```tsx
void guard(api.running ? stopLocalApi : startLocalApi);
```

which is a real call by reference with no parenthesis anywhere near the name. **A gate that invents
an unreachable setting is worse than one that misses a reachable one** — the first sends someone
to build a control that already exists, and erodes trust in every other line the gate prints.

So the import statements are stripped and the mention is then taken at face value. It cannot tell a
name in a comment from a name in use; that is written down at the function rather than left to be
rediscovered.

Two things worth separating: I predicted in the first draft of that doc comment that *"no declared
surface passes a wrapper by reference"*, and the very next run refuted it. The prediction was cheap
and running it was cheap; writing the prediction down is what made the refutation visible instead of
silently shipping a false accusation.

### One test was passing for a reason that had just expired

`an unreachable key must say why` deleted the note from `meetings.detect` — the only key
declared unreachable. Making it reachable would have left that test green forever without exercising
anything. It now sets `reachable: false` on a key itself rather than borrowing whichever one the
backlog happens to hold, so it survives the backlog emptying.

### Not closed by this

Whether the command *works* when called. It has still never been driven — not by a screen, not by
hand. The round trip through `kw_storage::settings` is the same one every other meeting setting
uses, and `kw_platform::meeting_detect::detect` has tests for allowing, blocking, duplicate spellings
and blank entries; what has not happened is a person typing a name into the box and watching the
offer appear. That is part of KNEG-51.

---

## 2026-08-31 (KNEG-65) — a rule carried from upstream, measured for the first time, and three quarters wrong

`kw_storage::paths` has relocated the model cache away from the user's own directory since the port,
on a rule written as *"non-ASCII **or space**"* and applied to the binaries' own directory as well
as to the files they read. Nobody had ever run it. `PLATFORM_SUPPORT.md` said so in as many words:
*"the fallback has not been exercised here."*

It cost about twenty minutes to find out, and the answer is much narrower:

```text
                                  a space        a non-ASCII character
the model file                    loads          FAILS
the audio file                    transcribes    FAILS
the binary's own directory        runs           runs
```

**A space breaks nothing, anywhere.** `whisper-server` loads `ggml-tiny.bin` out of `model space/`
and `sherpa-onnx-offline` transcribes a wav from `ljud plain/`. So an ordinary Windows account named
`Ann Marie` was being sent to a machine-wide `%ProgramData%` cache — models shared across every
account on the machine, and a "fell back" notice on her screen — to avoid a failure that does not
happen.

**The binaries' own location breaks nothing either.** This was the load-bearing half of the old
comment: *"`whisper-server` loads its sibling `ggml` libraries relative to its own directory, and
the native code chokes."* It does not choke. It printed
`load_backend: loaded CPU backend from ...\Håkan Test\bin\ggml-cpu-cascadelake.dll` and carried on.
`sherpa-onnx-offline`, `llama-server` and `ffmpeg` all do the same. Four out of four, so `sidecar_dir`
stopped relocating at all — which is not only tidiness: `%ProgramData%` is a directory an
unprivileged account may not be able to write to, so the old fallback could turn a cosmetic problem
into an install failure.

**What is real** is a file the ASR binaries open by path. Both exit 127. `whisper-server` says
nothing at all after `loading model from ...`; `sherpa-onnx-offline` says `Errors in config!` for a
model and `Failed to read '...'` for audio. So the fallback stays for `model_cache_dir` and
`audio_temp_dir`, on evidence rather than on inheritance.

### The decomposition was the whole method

Four confounded variables in one sentence: space against non-ASCII, and the binary's path against
the files it reads. Testing "a path with a space and a non-ASCII character in it", which is how the
claim was written and the obvious thing to try, reproduces the failure and teaches nothing — the
first run did exactly that and looked like a confirmation. Varying one at a time is what turned a
confirmed folk rule into three findings.

### The probe caught itself being wrong

`crates/storage/examples/native_path_probe.rs` is committed so this can be re-run, in the shape
`altgr_probe` already set. Its first version used `whisper-server --model X --help`, on the
assumption that the model is loaded before the usage text is printed. It is not, and the probe
reported `DIES` for every case **including the control** — which is the only reason the mistake was
visible at all. A probe with no control would have printed three plausible failures and been
believed.

The version that ships starts the server and polls the port, because the only honest evidence that a
model loaded is a server that answers.

### What is still not measured, and is now the only part left

**A real Windows account whose own name is non-ASCII.** Everything above used awkward directory
names under an ASCII profile, which exercises the binaries but not the relocation a profile name
triggers. That relocation — models landing in `%ProgramData%`, the screen saying so, a Parakeet
model installed there transcribing — still needs somebody with a `Håkan` account. It stays in
`BENCH_CHECK.md` as the remainder rather than being quietly closed with the rest.

Also unmeasured, and named rather than assumed away: no released build exists (KNEG-43), so nobody
has models in the old location. After a first release, narrowing this rule would move where models
are looked for and would need a migration; today it moves nothing.

---

## 2026-08-31 (KNEG-95) — the lock is the arbiter, and the state leads with the word that is still true

The last of the 2026-08-28 meeting silence. Both OpenAI Realtime sockets closed within milliseconds
of connecting; the application said **Recording**, kept the indicator up, captured both streams for
as long as it was left running, and said nothing. KNEG-79 fixed the end of that meeting — Stop now
reports the provider's own sentence. This is the middle of it.

The shape was the repository owner's to pick, and was picked: **keep recording, and change the
indicator to say the transcript died.** Not stop the recording, and not fall back to the whole-file
pass immediately. The argument is the one artefact that cannot be recreated: the WAV is what saved
the 28 August session, `record` already keeps writing when nothing is consuming the audio — on
purpose, with a comment saying why — and the saved note comes from the whole-file pass over that
file rather than from the live partials. Stopping the recording to report a dead provider would
destroy the valuable half in order to fix the misleading half.

### `MeetingState::RecordingWithoutTranscript`, and why the name matters

The first word is the one that is still true. In the enum, in the JSON on the wire, in a log line
and on the screen, a reader who sees only the first word is not misled: the microphone is open, the
file is growing, and the indicator the brief requires is still up. `TranscriptLost` would have been
shorter and would have invited exactly the wrong inference.

`MeetingState::is_running` came out of `emit_state` at the same time. The tray rule was
`state != MeetingState::Idle` written inline, which is correct and is also the kind of thing a
fourth variant walks past. Now there is one place to answer it and a test that asks all four.

### The lock is the arbiter, and that is the whole design

The hard part was never the state. It was deciding **which ending is worth announcing**, because the
pump ends on every path — including the ordinary one, where Stop closes the recorder and the pump
drains and returns.

The trap: a provider that faults on the *final* frame ends the pump during `finish_recording`, which
runs the whole-file pass and can take minutes on a long meeting. An announcement there would
overwrite `Finishing` with "the transcript died" and leave the user looking at it until the pass
returned. That is not a narrow race; it is a wide and ordinary one.

`live` is now an `Arc<Mutex<Option<Live>>>`, the pump's own task holds a handle to it, and
`announce_transcript_ended` decides and emits **while holding that lock**. `stop` and `cancel` take
the session out under the same lock, so *"there is still a session here"* is exactly *"stopping has
not begun"*. One fact, not a second flag that could drift out of step with it. Emitting after
releasing the lock would reopen the same window, which is why the emit is inside it.

**And a second race underneath it, found by asking what `stop` actually holds.** `stop` takes `Live`
out and then spends the whole-file pass *outside* the lock, and `start` refuses only while a session
is held — so a second meeting can begin while the first is still finishing. A late pump task would
then find *a* session, believe it to be its own, and mark a healthy new meeting as having lost its
transcript. `is_this_session` compares the fault cell with `Arc::ptr_eq` rather than trusting
presence. The cell is the session's identity because it is already per-session and already shared
with exactly the task that needs it; a counter would have been a second thing to keep in step.

### `pump.is_finished()`, narrowed to the one case the cell cannot cover

KNEG-95 named `status()` not consulting `pump.is_finished()` as part of the gap, and it is consulted
now — but the interesting part is what it is *for*. The announcement is written by the pump's own
task **before that task returns**, so the join handle cannot report finished first. "Finished, and
nothing announced" therefore means exactly one thing: the task panicked, or it was aborted. There is
no provider sentence for either, and the screen still must not claim a transcript is being produced.
Without that ordering the check would be redundant; with it, it is precise.

### One sentence, not two

`live_transcript_ended` is the only formatter. The indicator's sentence mid-meeting and the outcome's
sentence at Stop are the same string from the same function, and a test asserts the two call sites
agree rather than trusting that they were written to. A user who reads the indicator and then reads
the outcome is not told two stories about one failure.

Its `Ok` arm is the one worth reading twice. The pump returns `Ok` when the audio channel closes,
which at Stop is what is supposed to happen. Reaching it *while a session is still held* means the
channel closed with no Stop behind it, and `record` has no path that ends by itself — it returns on
`Finish`, on `Cancel`, and on the command channel disconnecting, all three of which run only after
`stop` or `cancel` has taken the session. So that arm means the recording thread panicked, and the
recording has stopped along with the transcription.

**That is the one case where the state is wrong in its first word, and it was reported anyway.** The
alternative was the silence this whole change exists to end, and the sentence — which is what a
person actually reads — said the true thing. Filed as **KNEG-116** (Low) rather than left in a
comment: a fifth state for a case reachable only by a panic, and untestable without one, was more
machinery than the fact justified — and that is a judgement somebody should be able to revisit.

**Revisited the same day, and it was the wrong call.** The fifth state was the smaller half; the
larger one was that the case had to be *inferred* at all. `MeetingState::Stalled` now exists, and
the recorder thread reports how it ended rather than the runtime guessing from a closed channel.
See the KNEG-116 entry.

### Where it shows

The indicator window keeps its red edge, its timer and its Stop button, and changes its second line
to *"live transcript stopped — still recording"* in amber. Amber rather than red for the same
reason `.caveat.warn` splits the mark from the prose: the red edge means **recording**, which is
still true and is the one indicator the brief requires. Painting the line red as well would say the
recording had failed, when the recording is the half that is working.

The provider's own sentence is too long for a 392x92 panel and goes to `Settings > Meetings`, which
has room for it, together with what happens next: recording continues, and Stop will still
transcribe the whole recording. The live-transcript panel there says its lines are the last ones
rather than the most recent ones, because "replaced as it is revised" stops being true the moment
nothing more is coming.

### What this did not do

**No provider was faulted to test it.** The unit tests cover the decisions — which state, whose
session, which sentence — and `announce_transcript_ended` itself is not among them, because it takes
an `AppHandle` and nothing outside a running application can construct one. The by-hand check is in
`BENCH_CHECK.md`: start a meeting on a provider configured wrongly and watch the indicator change
without the recording stopping.

Six mutations, each caught by exactly the test written for it:

```
live_state ignores a vanished pump          -> a_pump_that_vanished_without_a_word...
identity check becomes a presence check     -> a_late_ending_cannot_be_pinned_on_the_next_meeting
any session is my session                   -> a_late_ending_cannot_be_pinned_on_the_next_meeting
the tray rule forgets the new state         -> every_state_but_idle_is_a_running_session
stop formats its own sentence again         -> the_indicator_and_the_outcome_use_one_sentence
the new state serialises under another name -> the_states_serialise_as_the_frontend_reads_them
```

The `TranscriptSink` trait the KNEG-79 entry predicted this work would want was **not** needed, and
that entry has been corrected rather than left standing. The decisions that mattered turned out to
be expressible as free functions over plain values, which is the shape `pump_result`,
`close_recorder` and `failed_message` already had. `pump` still has no test coverage and still needs
none: it propagates its `Err` correctly, and what was wrong was what nobody did with it.

---

## 2026-08-31 (KNEG-37) — asking the matcher instead of describing it, and finding two more

On a Nordic, German or most other non-US layout, `@ $ € { [` are typed with AltGr, and **Windows has
no AltGr virtual key** — the keyboard layout synthesises left Control plus right Alt. So a
modifier-only chord of `Control+Alt` fires on every one of those keystrokes. Measured 2026-08-26 by
injecting exactly that pair and watching a dictation start.

Nothing downstream can separate the two, so this is not fixable at the hook. The fix is that
something says so, which is what was missing.

### The implementation choice that did all the work

The obvious way to decide whether an accelerator collides with AltGr is to ask whether it names
both a Control and an Alt. Two or three lines, and it is what the issue's own description suggests.

Instead, `keys::altgr_collision` **drives the real `ChordWatcher`** with the keys a real AltGr press
sends and reports whether it fired. The argument for it was drift: the matcher is the thing that
actually decides, several hundred times a second, and a separate predicate reasoning about modifier
names would be free to disagree with it — silently, in exactly the cases nobody wrote a test for.

It paid for itself immediately, twice.

**`Control+Shift` collides too.** Nobody predicted this and the name-based predicate would have
called it safe. AltGr's *left Control* arrives while Shift is already held, so the chord is complete
before the Alt is pressed. A user who binds `Control+Shift` — a far more ordinary choice than
`Control+Alt` — starts a dictation whenever they type a shifted AltGr character.

**And then the bigger one, which is not this issue.** Driving the same matcher with an ordinary
three-key shortcut, in the order a person presses it:

```
Control+Super    while pressing Ctrl+Win+Left  -> FIRES
Control+Shift    while pressing Ctrl+Shift+D   -> FIRES
Control+Alt      while pressing Ctrl+Alt+Del   -> FIRES
```

`Control+Super` is the **shipped default**, and `Ctrl+Win+Left` switches virtual desktop. A
two-modifier chord completes on the way to any longer chord that begins with the same two — rule 2
stops the longer chord *matching*, and nothing stops the shorter one *firing* as its prefix. That is
general, it affects the default, and fixing it means changing when a modifier-only chord fires, so
it is KNEG-115 rather than something to quietly fold in here.

### Two measurements, not one, and the second one is the layout

Whether a chord collides is a property of the chord. Whether the user can *make* that keystroke is a
property of their keyboard, and warning a US-layout user about a key they do not have is noise.

`kw_platform::keyboard_layout` asks `VkKeyScanExW` whether any character needs Control and Alt
together. **Not a locale table:** "Swedish" is not the fact, `'@'` needing Control and Alt is — and
the character found is carried into the warning, because *"this layout types '@' with Control and
Alt together"* is a sentence the reader can check against the keyboard in front of them and *"your
layout has AltGr"* is not. On this machine: present, `'@'`.

Three states rather than a boolean, for the reason `content_protection` has them: a platform that
cannot be asked must not answer "no". `Absent` suppresses the warning; `Unknown` still warns and
says it could not tell. Failing open, deliberately — a spare sentence costs a reader a moment, and
silence on a layout that does have AltGr is the defect.

### What is deliberately not warned about

**Standard accelerators.** `Control+Alt+D` goes to the system shortcut register rather than the
hook, and whether *that* also sees AltGr as Control+Alt has never been measured by this project.
Widely believed; unverified here; so not claimed on a screen. It is also the more common binding, so
leaving it out is a real gap rather than a tidy boundary — stated rather than papered over.

**A remedy.** A sided chord avoids AltGr entirely, because AltGr is *left* Control and *right* Alt
specifically: `LeftControl+LeftAlt` and `RightControl+Alt` both come back unaffected, and
`keys::Chord` already parses them. No settings field accepted a sided spelling — that was KNEG-35 —
so suggesting one would have been advice that could not be followed, and the caution stated the
problem and stopped. **KNEG-35 closed on 2026-08-31**, and the caution now names the escape,
computed from the colliding chord and re-checked against `altgr_collision` before it is offered.

### A mutation test that found the wrong reason

Four mutations, to check each part carries weight:

```
only the bare AltGr press modelled  -> 3 tests fail
Absent counts as possible           -> 1 test fails
Alt alone counts as AltGr           -> 1 test fails
the ModifierOnly guard removed      -> nothing fails
```

The fourth was the useful one. `a_standard_accelerator_is_not_claimed_about` passed with the guard
deleted, because `Chord::parse` refuses an ordinary key and had been doing all the excluding. The
guard was not useless — it excludes an `Invalid` accelerator, and `classify("Control+Alt+")` is
invalid (empty segment) while `Chord::parse` skips the empty segment and returns a perfectly good
`Control+Alt`, so without it a shortcut that does nothing at all would collect a warning about what
else it does. The test simply asserted the right outcome for the wrong reason. Fixed by testing the
case the guard is actually for, after which removing it fails.

### Where it shows

A third category on the Shortcuts section, beside `conflicts` and `unusable` and not folded into
either: those two mean the hotkey will not do its job, and a caution means it will, and will also do
it when nobody asked. `hotkey_set` returns the same sentence, because the moment to learn that
`Control+Alt` is AltGr is when you press Set. The mark is red and the prose is not —
`.row-detail.warn` is red *because every string reaching it is a failure*, and this one is the
opposite: the hotkey is registered, live and working, which is precisely why it fires when you type
`@`.

`cargo run -p kw-platform --example altgr_probe` re-runs both measurements on any machine.
---

## 2026-08-31 (KNEG-36) — the cheap fix would have destroyed the thing worth keeping

`knegiswhisper history list` rendered `id, when, provider, text`. A failed dictation has neither a
provider nor text, so it printed two dashes:

```
id  when                 provider  text
19  2026-08-26 09:51:11  -         -
```

The API had the answer the whole time — `/v1/transcriptions/list` returns `status` and
`errorMessage`, and the dashboard shows both. This was a renderer discarding what it was given.

### The obvious fix is wrong, and one code path proves it

Put the error message where the transcript would go. Four columns stay four, nothing widens, and the
row reads `19 … - provider returned 429`. It took reading `Dictation::fail` to see why not:

```rust
let (text, raw, provider, model) = match recovered {
    Some((text, raw, provider, model)) => (text, Some(raw), Some(provider), model),
    None => (String::new(), None, None, None),
};
```

A dictation that **transcribed correctly and then failed to paste** is recorded as `failed` with the
recovered text *and* a provider *and* a reason. That branch exists because the words are the half
worth keeping — its sibling comment says so outright: *"The text exists and only the paste failed,
so the user must be able to…"*. Writing the error into that cell would have thrown away precisely
what the recovery was added to save, and no test would have caught it, because no test had a
recovered failure in it.

So: **`status` becomes a column**, the transcript keeps its own, and the reason goes on a marked
continuation line where it competes with neither.

```
id  when                 status     provider  text
18  2026-08-26 09:50:02  completed  local     the quick brown fox jumps over the lazy dog
19  2026-08-26 09:51:11  failed     -         -
      ↳ provider returned 429
20  2026-08-26 09:52:40  cancelled  -         -
```

`cancelled` came along at no cost, and is the second half of the same defect: it also had no
provider and no text, so "I cancelled it", "it broke" and "the microphone heard nothing" were one
indistinguishable row. Three statuses, three answers.

### A note is not a column, and the reason is arithmetic

An error message is present on a small minority of rows and long when present. A sixth column would
be empty for almost every row and would push the transcript off an 80-column terminal for all of
them to accommodate a few. A continuation line costs a line only where there is something to say.
It is capped at 100 characters rather than the cells' 48, because a provider error is the one string
here whose useful half is usually at the *end* — an HTTP status, a rate-limit reason — and cutting
it at 48 keeps the boilerplate and drops the fact.

One table renderer, not two: `rows` now delegates to `rows_noted` with a closure that never returns
a note, so the four callers with nothing to add are untouched and the width arithmetic exists once.

### `history get` had the same defect, worse, and is fixed differently

It rendered `data.text` alone. For a failed row that is `""` — so the command exited zero and
printed **a blank line**. Worse than two dashes: with `list` there is at least a row to notice.

It is *not* fixed by printing a report, because printing the transcript alone is the command's job:
`knegiswhisper history get 3 > note.txt` must write a file with nothing in it but the words, and
`end_to_end.rs` pins that with `assert_eq!(stdout.trim(), "three")`. So it prints the transcript
when there is one and the status and reason when there is not.

**The limit that leaves, stated rather than discovered:** a recovered failure has both, and `get`
prints only the transcript. The clean answer is the reason on stderr and the words on stdout, which
needs `dispatch`'s `(Value, String)` to become a three-tuple across fifteen arms — too much churn
for one line of output when `list` already shows that row in full. Written into the doc comment
rather than left as an accident.

### Proven by mutation, and by driving the real socket

The renderer's own tests build JSON by hand, which proves the table *can* show a status and says
nothing about whether the server sends one. So `end_to_end.rs` gained a case that records a real
failed row, starts the real server, and runs the compiled binary — the two halves of the claim
checked against each other rather than separately.

Each part of the fix was then removed in turn to confirm a test breaks:

```
the status column removed      -> 2 tests fail
the continuation line removed  -> 5 tests fail
history get back to data.text  -> 2 tests fail
```

Not a redaction concern: the string is the one the dashboard already renders through
`outcomeLabel`, and credentials live in `kw_security::Secret`, whose `Display` is `<redacted>`, so
a key cannot reach an error message in the first place.

---

## 2026-08-31 (KNEG-104) — the fourth instance, found by the gate written for the first three

Three defects in this repository have had one shape, and each was found by a person reading a
screen rather than by anything automatic:

1. Four hotkeys that registered and dispatched nowhere.
2. `history.keepRawText`, hardcoded `false` at the only real call site, under a comment calling it
   opt-in (KNEG-98).
3. `history.enabled`, honoured before every history row and settable only by editing SQLite by hand
   (KNEG-99).

**Why the tests were no help, which is the whole point.** Every one of those settings had a key, a
default, and a passing unit test. A test sets the value directly — so it exercises the branch and
never asks whether anybody can reach it. Three green suites and three settings nobody could change.
That is not a gap in coverage; it is a question the tests were not shaped to ask, and the fix is a
different kind of check rather than more of the same kind.

`config/settings-keys.json` plus `tools/settings-audit/scan.mjs` is that check, in the contracts
job beside the parity gate and the domain scanner.

**It found the fourth instance on its first run.** `meetings.detect` — the allow and deny lists
deciding which running applications count as a meeting — has a `#[tauri::command]` that writes it,
registered in `generate_handler!`, with an IPC wrapper and a dev fixture, and **no screen calls
it**. Everything except the control exists. Filed as KNEG-114 and declared `reachable: false` with
a note, so the gate stayed green while the gap stayed visible — the same treatment the parity gate
gives its unreachable integrations, and for the same reason: a backlog printed on success is read,
and a red gate that is red every day is not. **Closed on 2026-08-31**; the backlog is now empty, and
the attempt to break the new control found that this check had been satisfied by an import all
along — see the KNEG-114 entry above.

### Two design decisions that decide whether it means anything

**The generic writer cannot count as a writer.** `commands::settings_set(key, value)` writes
whatever key the renderer names. Counting it would make the gate pass for every key ever added,
which is a check that reports success by construction. The set of such commands is *computed from
source* — a command whose key argument resolves to a renderer-supplied expression — rather than
hardcoded, so a second generic writer would be caught the day it is added rather than the day
somebody remembers this paragraph. Naming one in `writtenBy` is the `generic-writer` violation.

**An unresolvable key expression is a violation, never a skip.** The scanner resolves four shapes:
a literal, a `const NAME: &str`, a small `fn -> &'static str` match (how the calendar keys are
written), and a declared derived family (the five `hotkey.*`, built by `format!` in another crate
and pinned there by a named test this gate resolves). Anything else fails. A scanner that quietly
ignores the shape it does not understand reports green on exactly the key nobody can change — the
defect it exists to catch, one level up. That is not hypothetical here: rule 5 of the parity gate
had degenerated into a non-empty-string test and passed for sixty-four entries before rule 7 was
added, which is the same failure and is why this one was written to fail loud instead.

### Attribution, and the limit that came with it

A write is attributed to the last function declared at or before it. This is not a call graph and
does not pretend to be — a key written *only* by a helper the command calls would read as unwritten
and fail. That direction is the safe one: it is loud, and the fix is a sentence rather than a
silent pass. Measured before relying on it: of the production writes on the committed tree, every
one sits directly inside a command except calendar's OAuth bookkeeping and
`local_asr::clear_selection`, and the calendar pair are declared `internal` on their own merits.

The same attribution is why test code needs no special handling. A `#[cfg(test)]` function is never
a `#[tauri::command]`, so a test setting a value directly — the exact thing that hid all three
original defects — cannot count as the way a user changes it. Nothing had to be stripped for that
to hold; it falls out of asking the right question.

### Reachability is resolved to a component, not to a string

A key claiming `reachable: true` names a surface in `config/ui-surfaces.json`, and that surface's
component must use the IPC wrapper for one of the key's writers. Naming a screen is not enough —
which is precisely how `meetings.detect` was broken, and was the reason that key failed the check
while every other meeting setting passed. (*Use*, not *call*: what "not enough" means was itself too
weak until KNEG-114 — an import counted. See that entry.) Two surfaces were added to the registry to make this
sayable, `Dictation > Shortcuts` and `Dictation > History`, both real sections of `Dictation.tsx`
that had never been declared because no integration entry happened to name them.

**What the gate does not claim.** That a key changes behaviour, or that anything is wired behind
it. All five `hotkey.*` keys are writable from `Dictation > Shortcuts` and four of the five actions
still have no handler; `commands::hotkeys` reports that separately as an `UnhandledAction`. This
answers one question: can a user change the value at all.

### Proven by neutering, not by passing

Twenty-two self-tests, every case mutating the real committed inputs one at a time; the first two
reconstruct KNEG-99 as it actually stood. Then each of six rules was disabled in turn and the suite
re-run, to confirm a test breaks rather than assuming it would:

```
unwritable-key                  -> a test fails
unreachable-writer              -> a test fails
unresolvable-key-expression     -> two tests fail
command-does-not-write-the-key  -> a test fails
generic-writer                  -> a test fails
undeclared-writer               -> a test fails
```

A gate against "a test passed and proved nothing" that had only ever been run green would be the
same mistake a third time.

### Corrected in passing

- `getSettingsKeys`'s doc comment in `ipc.ts` said it returned *"every settings key Rust will
  accept, so a screen cannot invent one"*. It returns the keys that currently have a stored row, so
  a key sitting at its default is absent — and the command accepts any key `kw_storage::settings`
  does not reject as credential-shaped. The three generic settings wrappers also have no caller,
  which is now stated where somebody would otherwise reach for one.
- `config/ui-surfaces.json`'s note for `Settings > Transcription > Local` still said the screen had
  not been extended to install or select the Parakeet models. `121dc4c` added that selector and
  flipped both manifest entries to `reachable: true`; the note was three commits stale.

---

## 2026-08-31 (KNEG-93) — the residue that stopped being residue when the cost inverted

`is_private_host` carried three gaps recorded as *accepted* rather than missed. Two of them are
closed now, and the reason to reopen them was not that anything new was found. It was that the
decisions had been made against a cost that no longer applies.

**The inversion, which the issue stated and is worth keeping.** Before the SSRF fix, a false
negative in this predicate meant a plaintext request to that address was *refused* — the plaintext
arm only allows what the predicate calls private, so a miss lost permission rather than gaining it.
After the fix, the identical false negative means an HTTPS or WSS request is *allowed*, because the
private check is now what refuses it there. **The same class of gap that used to fail closed now
fails open.** Every one of these was accepted while it failed closed.

### Closed as a family, which is what the note itself asked for

Each embedding had been noticed separately — IPv4-translated during the review, NAT64 and 6to4
raised unprompted by the re-reviewer afterwards — and each was accepted on the same argument: the
prefix is not routed on an ordinary host, so a `connect()` fails rather than arriving.

That argument is about the *host's* configuration. It stops holding the moment a machine has 6to4 or
a NAT64 resolver, and nothing in this process can see whether it does. The recorded decision was
that if the family were ever closed it should be closed by enumerating the RFCs rather than by
adding a prefix each time somebody spots one, and `embedded_ipv4s` is that enumeration: IPv4-mapped,
IPv4-compatible, IPv4-translated, NAT64 well-known, NAT64 local-use, 6to4, Teredo.

Two things about it were not obvious going in:

- **Teredo carries two IPv4 addresses**, not one — the server in the clear at bits 32..64 and the
  client at bits 96..128 XORed with all-ones. Either being private refuses the address, because a
  packet to it involves both hosts. That is why the function returns `[Option<Ipv4Addr>; 2]` rather
  than an `Option`.
- **The NAT64 local-use prefix is not laid out the way the well-known one is.** `64:ff9b::/96` puts
  the address in the low 32 bits; `64:ff9b:1::/48` follows RFC 6052 §2.2, which splits it around a
  reserved octet — sixteen bits at 48..64 and sixteen more at 72..88. Reading four contiguous octets
  parses fine and decodes a different address than the gateway will.

### The tests state the bits, and were checked by breaking the code

Both of those layouts are wrong-in-a-way-that-still-works hazards, and a test written from the same
misreading as the implementation proves nothing. So the addresses are written out in hex, the
decoded value is asserted exactly rather than only its privateness, and four deliberate mutations
were run against the finished tests:

```
NAT64 /48 read as four contiguous octets   -> CAUGHT
Teredo client not XORed                    -> CAUGHT
6to4 read from the wrong offset            -> CAUGHT
SIIT arm removed                           -> CAUGHT
```

Without the exact-value assertions the first two would have passed: in a NAT64 `/48` and a Teredo
address there are enough nearby zero octets that a wrong offset still decodes *something* private.

### The limit that remains, and it is real

RFC 6052 permits a **network-specific** NAT64 prefix of any value at `/32`, `/40`, `/48`, `/56` or
`/64`. Only the well-known and local-use prefixes have fixed values, so a site running NAT64 on its
own prefix is invisible to this. Seeing it would mean reading the host's resolver configuration,
which is the same class of problem as DNS rebinding — and DNS rebinding stays out of scope for
exactly the reason it always has: the check runs against the literal host at validation time and has
no view of what the name resolves to at connect time. That is the problem that withdrew URL import
(KNEG-87), and it is not solved in passing.

### And the small one

`blocked_hosts()` lowercased configured vendor hosts and did not strip trailing dots from them, while
`is_blocked` stripped them from its input. No entry in `config/forbidden-domains.json` has one, so
this was a footgun for whoever next edits that file rather than a live hole. Closed anyway: an
asymmetry that costs one line to remove is not worth keeping, and the test asserts the property over
whatever the file happens to contain rather than over today's entries.

---

## 2026-08-31 (KNEG-112) — asking whether a GitHub asset can be pinned, and finding it cannot

KNEG-103 pinned all 45 Hugging Face URLs to a commit and left two archives behind: the Parakeet
models, which are assets in k2-fsa's rolling `asr-models` GitHub release. The issue filed for them
opened with three directions and said, in as many words, that the first task was a question rather
than a fix and **nobody had looked at any of them.** So this is what looking found.

### (a) Dated immutable tags — dead, measured

k2-fsa publishes plenty of versioned releases: `v1.13.6`, `v1.13.5`, twenty-odd `v1.12.x`, each with
280–340 assets. It looked promising for about a minute.

They carry **library binaries and no models.** `v1.13.6` has 283 assets and **zero** whose name
mentions `parakeet` — `sherpa-onnx-1.13.6.aar`, `sherpa-onnx-jvm-1.13.6.jar`, and so on. Every model
archive lives in `asr-models`, a tag created 2023-11-20 that now holds **498 assets** and is added to
continuously. There is no dated release to move to because the models were never in one.

### (b) The Hugging Face mirror — real, and a different artifact

`csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8` exists, published by sherpa-onnx's own
maintainer, so `resolve/{sha}` is available in principle.

It holds the model **unpacked**: a 652 MB `encoder.int8.onnx`, a decoder, a joiner, `tokens.txt` and
four test wavs. Not the 487 MB `.tar.bz2` this application downloads. Switching would mean replacing
one archive fetch with a multi-file fetch, discarding the extraction path and with it
`maxExtractedSizeBytes` — the decompression-bomb ceiling KNEG-100 and KNEG-102 were about — in
exchange for **no integrity gain**, because the digest is already measured. A rewrite that trades a
working defence for a URL shape is not an improvement.

### The fourth direction the issue did not know about, which also fails

GitHub gives every release asset an id, and `api.github.com/repos/{repo}/releases/assets/{id}`
serves that exact upload — measured: `302` to `release-assets.githubusercontent.com`. It looks
exactly like the `resolve/{sha}` analogue.

**It is not one, and the reason is the whole finding.** Git keeps old commits, which is why pinning
one costs upstream nothing and keeps us working when `main` moves. GitHub does *not* keep old
assets: a name cannot be overwritten, so replacing an asset means deleting it and uploading again,
and the old id goes with the deletion. An id-pinned URL therefore turns a swap into a `404` where
the name-pinned URL turns it into a digest failure. **Both block the install.** The pin would look
like KNEG-103's and buy nothing.

*Reasoned, not measured.* Nothing here can make k2-fsa replace an asset to watch what happens to the
id, so this rests on how GitHub's release API is documented to behave and not on an observation.

### What is actually there, and it is worth having

**GitHub's release API states a `digest` and a `size` per asset.** One request returns all 498. And:

```
parakeet-tdt-0.6b-v3       sha256:5793d0fd…eafb16bf   487,170,055
parakeet-unified-en-0.6b   sha256:99f63605…9a76e150   501,350,460
```

Both digests and both sizes match the committed pins **to the character**. That is the same shape as
`lfs.oid` — the publisher's stated digest, over the same channel as the bytes — and it is
independent corroboration of a measurement this project made by downloading, at no bandwidth cost.

`--verify-stated` reads it weekly now, beside the Hugging Face trees. Two hosts, one verb, and the
success line says which of them was which: for these two the stated digest is **weaker** than how
they were pinned, since their bytes were hashed here, so agreement means the asset has not been
swapped and not that the bytes are still what we hashed. Only the download proves the second thing,
and the download still runs.

### So the URL half is declined, and that is a result

The issue allowed for it: *"the honest answer may be that the digest is all there is, and this
closes as declined with the reasoning written down — which is a better outcome than an issue that
sits open implying somebody forgot."* It is, and it does. What changed is that the reasoning is now
three measurements rather than an assumption, and the gap it leaves is watched weekly instead of
only at install.

---

## 2026-08-31 (KNEG-86 / KNEG-85) — the gate that could not check the one thing it was for

`verify-parity.mjs` refuses a `reachable: true` claim unless the component file
exists and renders its `data-surface` marker. That is the standard it holds UI
surfaces to. A `kind: "documentation"` entry was held to nothing comparable: the
rule checked that it **named** a target and stopped there.

So `developer:agent-skills` carried `status: "ported"` with
`target: "docs/agent-skills/"` for a twenty-task run, against a directory that
had never existed, and no gate in this repository could have said so. It was
found by hand, by the audit that was looking for exactly that pattern one step
further along.

### The exemption was right and the hole was next to it

Documentation carries no code, so the backend and UI requirements genuinely do
not apply. The mistake was the `continue` that followed: nothing took their
place, which made a documentation entry the one kind that could assert work was
done with nothing checking. It gets its own equivalent now — if the status is in
`ACTIVE`, the target has to be there.

**Existence and content.** An empty `docs/agent-skills/` satisfies "the
rewritten documentation lives here" no better than a missing one, and is the
same hole one commit later.

### Injected, and failing closed

`hasTarget` is a parameter, the way `readComponent` already is, because a rule
that can only be run against the committed tree is a rule nobody can prove
bites. The file's own doc comment already made that argument: *"this tree is
clean" and "the rules work" are different claims, and only the second one keeps
working after a refactor.*

Its default is `() => false` — refuse every target. A caller who forgets to
inject it gets a loud failure rather than a silent pass, which is the direction
that matters for a check whose entire job is catching a claim about something
that is not there. There is a test for the default, because a fail-closed
default nobody exercises is an assumption rather than a behaviour.

### Proved by removing the thing

The strongest evidence is not a unit test. With `docs/agent-skills/` moved
aside, the real gate exits **1** with:

```
documentation-target-missing (1)
  developer:agent-skills: status "ported" claims the rewritten documentation is
  at "docs/agent-skills/", which does not exist or is empty
```

and exits 0 with it restored. That is the original defect, reproduced against
the fix.

### And then the documentation, which is the point (KNEG-85)

`docs/agent-skills/` now holds `knegiswhisper-api/SKILL.md` and
`knegiswhisper-cli/SKILL.md`, and the manifest entry says `ported` again — a
claim something now verifies.

Two things about their content are deliberate:

- **No hosted endpoint appears in either**, and neither says what they replaced.
  The manifest's `upstream` array is where the provenance belongs; a skill that
  spends a paragraph on a service that is not there teaches an agent about the
  wrong system.
- **Every shape in them was read out of the code that serves it** — the route
  table in `server.rs`, the request types in `wire.rs`, the `clap` definitions
  in `knegiswhisper/src/main.rs`, the `TOOLS` list in the MCP crate. Not one
  endpoint was described from a memory of what an API like this usually does.

And the limit is stated in the files themselves rather than left for a reader to
discover: **no MCP client has ever connected to `knegiswhisper-mcp`** (KNEG-91).
The protocol handling is unit-tested and has never had a real client on the
other end, so the skill tells the agent to believe the wire over the document if
they disagree. A skill that overstates its own confidence is the same defect
this pair of issues is about, one layer up.

---

## 2026-08-31 (KNEG-109) — a ceiling that cost something, and did not say what

`deepgram_socket` bounds how much transcript one session retains: 16 MB, or 10 000 events. That
bound is right — provider responses are outside the trust boundary, and session-long retention has
to be limited by something other than the provider's good behaviour. What was wrong was every part
of what happened when it was reached.

```rust
if !budget.reserve(text.len()) {
    tracing::warn!(?source, "the provider exceeded the transcript session budget");
    return;
}
```

### One `return` doing three kinds of damage

**It stopped the reader, not the retention.** The meeting kept recording, the provider kept sending,
the socket stayed open, and nothing further was transcribed. What the ceiling exists for is bounding
what is kept, and dropping the text does that on its own; ending the reader is a separate act
nobody asked for.

**It switched off KNEG-79's fix for the rest of the meeting.** This is the part that was not visible
when the issue was filed. A provider error frame arriving after the ceiling has to reach
`SessionFault` — that is the whole of the change landed hours earlier — and a dead reader cannot
record one. So the budget quietly reintroduced the silence, on the client where the wire shape is
believed rather than confirmed.

**It picked a capture at random.** The budget is shared through an `Arc`, so whichever of the two
readers happened to hit the ceiling first was the one that died and the other carried on. A meeting
could end with the microphone transcribed and the system audio not, decided by arrival order, with
nothing recording which.

And `deepgram::transcript` hardcoded `gaps: 0`. `MeetingTranscript::gaps` exists so that *"a
transcript with a hole in it must not look complete"*, and `keep` in `meeting.rs` appends a
paragraph to the saved note when it is non-zero. A budget-exhausted session had a hole and reported
none.

### Counted, not fatal

The issue's own reasoning was that counting is probably right and ending is probably wrong — a
16 MB transcript is a long working meeting, not a failure, and failing it at the ceiling would
discard a good transcript. That is what landed. `FaultKind` gained nothing; the session does not
end. The reader `continue`s, the text is dropped, and the stretch is counted.

### A gap is a run, not a frame

`TranscriptBudget` counts transitions into refusal rather than refusals. Counting per frame would
report a number nobody could act on — a long meeting past the ceiling would claim thousands of
holes where there is one.

Two details make that more than a detail:

- **A refusal is not final.** The byte ceiling is a total, so a short event can still be admitted
  after a long one was turned away. Two holes with transcript between them are two holes, so the
  flag has to close when something gets through. (The *event* ceiling is final once reached, since
  every event costs one.)
- **One flag per capture, not one per session.** The microphone can be dropping while the system
  audio is still being admitted. A single shared flag would end the microphone's stretch on the
  strength of somebody else's frame, and the two captures are not the same hole.

`reserve` returns `Reserved::{Room, NoRoom { opens_a_gap }}` rather than a `bool`, because the
caller has to do something different for the first refusal of a run than for the rest of it: one
`warn!` per stretch. Per frame it would be the loudest line in the log for the remainder of the
meeting.

### The sentence on the note was true and is now only half true

`keep` said: *"N stretch(es) of audio were never transcribed: the transcriber could not keep up with
the meeting."* Accurate while the only source of a gap was `local::LocalWindowProvider`, which
counts a sustained deficit — the model falling four hops behind and skipping to live. A budget gap
is not that. It is this application's own retention ceiling on a very long session, and there is
nothing the transcriber could have done faster.

So `gaps` now has two producers and one number, and the number does not say which. Both the field's
doc comment and the note's sentence name both possibilities rather than asserting one. Carrying the
cause properly would mean `gaps` stops being a `u32`, which is a change to a serialized type for a
distinction nobody has yet needed to act on — recorded as the trade rather than made silently.

### What is not verified, and it is most of it

No key for Deepgram exists on any machine this project has touched, so none of this has run against
a real provider (KNEG-53, KNEG-56). Reaching 16 MB of transcript needs a very long meeting on a
service nobody can currently connect to. The budget arithmetic and the gap counting are unit-tested
against the real `TranscriptBudget`; the reader's behaviour around them is read from the code.

---

## 2026-08-31 (KNEG-110) — a field that carried two facts, and only one of them stayed true

`LocalApiRuntime` stored the `Permissions` that `discovery::write` returned and showed that value
for as long as the application ran. Both settings screens rendered a claim about the past as a claim
about the present: anything with write access to `~/.knegiswhisper` could widen the access list a
second after start, and neither screen would say so until the next launch.

### The shape of the mistake is worth naming, because it is not "forgot to refresh"

One field held two facts:

```rust
/// The permissions the discovery file actually ended up with.
permissions: std::sync::RwLock<Option<Permissions>>,
```

The `Option` meant *is the API running* — the file is deleted on stop, so `None` is the honest
answer then. The `Permissions` inside it meant *what the file grants*. Only the first of those stays
true on its own. And the confusion was load-bearing rather than theoretical: `integrations.rs` asked
`permissions_snapshot().is_some()` **twice**, reading a permission verdict to learn something that
is not about permissions. An idiom like that is what makes a staleness bug easy to walk past.

So the fix is not a refresh call bolted on. The field becomes `discovery_written: AtomicBool` —
"there is a file" and nothing else — and the value is read when it is asked for. `is_running()` is
its own method now, so the two questions are asked separately.

### `discovery::inspect`, extracted rather than written

`write` already ended with exactly the read this needed:

```rust
let file = file_acl::verify(path, PathKind::File);
match parent { Some(p) => file_acl::combine(&file_acl::verify(p, PathKind::Directory), &file), … }
```

That expression is now `discovery::inspect`, and `write` ends by calling it. Extracting rather than
re-writing matters here more than usual: the verdict a caller gets after a write and the verdict it
gets from a later re-read have to *be* the same judgement, not two judgements that agree today. The
directory's verdict gating the file's is the part that would have drifted first — it is the
non-obvious half, and it is the half a second author would most likely leave out.

`the_read_back_describes_the_file_the_caller_is_told_about` already reconstructed the expression
independently and asserted `write` matched it. It keeps doing that, and gained one line asserting
`inspect` matches too, so the test now pins both the value and the fact that there is one copy of it.

### Read, never re-apply

Deliberate, and the reason is written at the call site. Silently repairing an access list somebody
changed on purpose is its own kind of surprise, and it would make the function unusable for what it
exists for: a status screen has to be able to *describe* a bad state without altering it. So the
application will tell you the token file has been widened. It will not quietly narrow it again.

### No cache, and that is a measurement rather than an omission

Two `stat` calls on Unix, two `GetNamedSecurityInfoW` calls on Windows, against a path already in
the filesystem cache. The issue suggested a short cache might be wanted because this is a UI refresh
path — but `useRefresh` has no timer. It fetches when a screen mounts and when something asks it to.
A cache would have added a staleness window, in the function whose entire purpose is removing one,
to fix a cost nobody is paying.

### The regression test, and why it deletes rather than widens

`the_verdict_is_re_read_rather_than_remembered` removes the discovery file behind the runtime's back
and asserts the verdict changes. Deleting is the one way to change what an access list says that
works identically on every platform, and this machine is Windows.

The case the issue actually described — a file that still exists and now grants more — is
`a_mode_widened_after_start_is_reported`, a `#[cfg(unix)]` test that chmods to `0644` and requires
`Wider { mode: 0o644 }`. It does not run here. It runs on CI's `contracts` job, which is
`ubuntu-latest` and is the only place the whole of this file is compiled — the same asymmetry
KNEG-111 is about, pointing the other way for once.

---

## 2026-08-31 (bench) — the install button that could only ever work once

The first person to press *Install* on a whisper model found nothing happened. No `ggml-base.bin`,
no `.partial`, no error worth acting on. The same session installed a 1.3 GB GGUF without trouble,
which is what made the cause findable.

### What it was

`local_model_install` installed the server sidecar **unconditionally** before the model:

```rust
kw_sidecar::install(&local.policy, spec, asset, &sidecars.path, ...).await?;
let installed = models::install(&local.policy, model, &models_dir.path, ...).await?;
```

`kw_sidecar::install` ends by replacing the sidecar directory, and `resume_last_used` has already
started `whisper-server.exe` out of that directory by the time anybody presses anything. On Windows
a directory holding a running executable cannot be removed, so `remove_dir(&final_dir)` failed, the
`?` returned, and the model was never fetched.

**So the button could only work when no whisper model was in use — which is to say, once.** After
the first install there is always a model to resume, so there is always a server running out of the
directory the next install has to replace.

### Measured rather than reasoned

The mechanism was checked on this machine rather than taken from memory, because "Windows locks
running executables" is the kind of thing that is true in outline and wrong in the detail:

- `remove_dir_all` on a directory holding a running `.exe` → **`WinError 5`, Access is denied**,
  reported against the `.exe` itself.
- `rename` of that same directory → **succeeds.** Windows denies delete on the image file, not
  rename of a parent. Worth writing down; it is the escape hatch if a real upgrade path ever needs
  one.

A first attempt at this measurement used `timeout.exe`, which exits immediately with no console
attached, and reported "no lock". It proved nothing and would have sent the diagnosis the wrong way.
The second used `ping -n 40`, confirmed running in the process list before the deletion was tried.

The failed run had also left its own evidence: `sidecars/whisper.incoming` holding a complete,
verified, extracted copy of the sidecar that never got promoted — exactly the state the code leaves
when it fails between extraction and the swap.

### The fix was already written, in the file next door

`local_llm_install` in `local_llm.rs` has always guarded the same call:

```rust
let installation = kw_sidecar::locate(SIDECAR, asset, &search_path, &kw_sidecar::RealFs);
if installation.executable().is_none() { kw_sidecar::install(...).await?; }
```

That is why the GGUF installed on the run that the whisper model failed. The whisper path now does
the same thing, through a named `sidecar_needed` so the judgement is testable — the command takes an
`AppHandle` and is untestable by construction, the same reason `check_importable_file` and
`import_pick_dialog` exist. Three tests: installed skips, missing installs, and **`Incomplete`
installs**, which is the case with the judgement in it — a directory with no executable is an
interrupted extraction, nothing is running out of it, and replacing it is safe.

### Skipping loses nothing, and that is worth arguing rather than asserting

The unconditional call looks like a way to keep the sidecar current. It never was. It can only
succeed when no server is running, which is only the first install, when there is nothing to
refresh. Every later call failed outright rather than updating anything.

But the gap it appeared to cover was real and, for a day, had nothing covering it: the installed
whisper sidecar dated from 2026-08-25 and the pinned archive had since gained `bench.exe` and
`command.exe`, so it was measurably behind. Filed as **KNEG-113** with the rename measurement, and
**closed on 2026-09-01** — see that entry. The displaced-directory scheme guessed at here was the
one rejected: a later probe found that renaming a running install aside does not make it deletable
either, so the cleanup story it needed was worse than it looked.

### The other finding: two documents told a person to hold a chat

`docs/BENCH_CHECK.md` and `docs/BUILD_PLAN.md` both instructed the reader to install a local LLM and
*hold a chat*. **There is no chat.** The application has thirteen views, none of them a chat;
`config/ui-surfaces.json` has no such surface; the chat agent is phase 10 (KNEG-74).

The local LLM has one consumer, dictation cleanup, and the path is built: `Settings > Local models`
→ *Use this model*, then `Settings > Intelligence` → the **local model** route, then dictate.
`reasoning.rs` already says so on screen — *"no local model is running. Start one under Local
models."* Only the instruction was wrong, and an unrunnable by-hand step is worse than a missing
one: it reads as a defect in the application rather than in the sentence.

---

## 2026-08-31 (KNEG-103) — 45 URLs stop naming a branch

Every model this application downloads was fetched from
`https://huggingface.co/{repo}/resolve/main/{file}`. `main` is a branch pointer. The bytes behind it
change whenever the publisher pushes, and as of earlier the same day every one of those 45 objects
carried a digest measured against the bytes that were there *then*.

So the defect was not that a re-upload would go unnoticed. It was that a re-upload would be noticed
in the worst possible place: at the end of a nineteen-gigabyte download, in front of a user, as an
integrity error they cannot get past, caused by somebody else's routine publishing.

### It was not theoretical, and the measurement says how far from it

`node tools/model-pin/pin.mjs --revisions` reads `/api/models/{repo}` for all thirty-seven
repositories — 36 GGUF plus `ggerganov/whisper.cpp`. Of the thirty-seven, **twenty-one last moved
during 2026, and four moved on 2026-08-24 — seven days before the digests were taken.** The four are
LiquidAI's LFM2.5 repositories.

Nothing had moved *since* the pins: all 39 stated digests and all 39 lengths still agreed, and the
two whisper digests spot-checked during the design pass came back byte-identical to what was
committed. The pins were correct. They were also one push away from not being, on a table where four
rows had changed inside the previous week.

### Where the revision lives, and why not beside the digest

One entry per **repository**, not per object. `HF_REVISIONS` is keyed on `owner/name`, so the six
whisper models share one row and the thirty-nine GGUF objects fill thirty-six. A revision is a
property of the repository at a point in time; a per-object copy would be six identical strings that
could go inconsistent, and there is no state a reader could make sense of where two files in the same
repository are pinned to different commits by accident.

The table sits in the generator beside `WHISPER_PINS`, `SHERPA_PINS` and `LOCAL_LLM_PINS`, for the
reason all three of those are there: `config/upstream-integration-baseline.json` is a frozen record of
what upstream had and must not gain fields we invented, and `config/provider-registry.json` is
generated, so anything hand-edited into it dies at the next regeneration.

### Two families, two mechanisms, one table

The whisper URL is *stored* — upstream's baseline supplies it whole — so the generator rewrites the
revision into it on the way out. The GGUF URL is *composed* in `local_llm.rs` from `hfRepo` +
`fileName`, so it gained a third part. Same pin, two paths, and the difference is a fact about where
each URL comes from rather than a choice.

`parseHfUrl` is exported from `tools/model-pin/pin.mjs` and imported by the generator. That is a real
coupling between a build gate and an audit tool, taken deliberately: the generator rewrites those
three parts and `--verify-stated` reads them, and two regexes for one URL shape would have drifted
apart on the first publisher who put an object in a subdirectory — which several already do, for
multi-part quantisations.

### `revision` is a required field, and `sha256` is not

`LocalLlmModel::sha256` is an `Option`, so a model added before anybody spends the bandwidth degrades
to a size check. `LocalLlmModel::revision` is a plain `String`.

The asymmetry is the point. A missing digest degrades to a weaker check that is still a check. A
missing revision would degrade to `main`, which is not a weaker version of this fix — it is the
defect, reintroduced silently, for one model out of thirty-nine. So the generator refuses to emit a
model without one, and a registry that somehow reached the Rust without it fails to deserialise
rather than quietly floating.

Four guards, each one run against a deliberately broken table before being trusted:

- a repository with no `HF_REVISIONS` entry dies **naming the model that needed it**, not just the
  repository;
- a revision that is not 40 lowercase hex dies, with "re-read it; do not hand-correct it";
- a revision no model downloads from dies as a typo — the same guard the three pin tables carry;
- every shipped whisper URL is re-parsed after the rewrite and must name a commit. `pinnedHfUrl`
  lets a non-Hugging-Face URL through untouched, which is right — there is nothing there to pin —
  and this is what stops that from becoming a quiet way back to a floating reference.

### What this is not

**It is not a stronger integrity claim.** The digest is what proves the bytes; a revision only decides
which bytes were promised. Nothing here defends against a compromised huggingface.co any better than
before — one host still serves both the object and, for thirty-two of the thirty-nine, the `lfs.oid`
they are checked against.

What it removes is a class of spurious failure. What it adds is that the URL and the digest now
describe the same object *by construction* rather than by nobody having pushed yet.

### `--verify-stated` was asking the wrong question and is now asking a better one

It read each repository's tree at `main`. That answered "has the publisher re-uploaded", which was
the right question while the URL pointed at `main` and is the wrong one now — a push cannot reach a
pinned commit, so the answer stopped being about anything the installer would fetch.

Reading the tree at the *committed* revision asks something nothing else in the workspace checks:
**do the digest and the revision committed beside each other describe the same object?** Those two
numbers are measured in different runs by different code and could drift apart with nobody noticing.
It also catches a pinned commit that has stopped resolving — a force-push, or a repository going
private — which would break every install of its models.

A second half reports which repositories have published past their pinned commit, and **does not
fail on it**. Upstream publishing is normal, and that is now the whole point: a weekly job that went
red every time a publisher pushed would be ignored by the second month. It is news about pins worth
revisiting, printed as such.

The tree cache is keyed on repository *and* revision, in the tool and in the Rust test both. One
revision per repository is how the table is built, and a cache that assumed it would read the wrong
tree, silently, the day that stopped holding.

Measured after the change: **39 objects, 36 tree requests, all matching at their pinned commits; no
repository has moved past its pin.** The two `#[ignore]`d Rust tests agree — 39 HEADs at
commit-pinned URLs and 36 tree reads, in nine seconds.

### What is still hanging off a moving reference

The two Parakeet archives. They are GitHub release assets in k2-fsa's rolling `asr-models` tag, where
an asset can be replaced under the same name, and there is no commit-sha equivalent to reach for.
Their digest is the only thing that would notice a swap, and the weekly `--verify` is what runs it.
That is unchanged by this and is recorded in `docs/PLATFORM_SUPPORT.md` rather than left implied by a
fix that did not cover them.

---

## 2026-08-31 (KNEG-34) — the token file finally gets a DACL, and the read-back moves

`discovery::restrict_file` was a no-op under `cfg(not(unix))`, so the local API's bearer token sat
in a file with whatever ACL the profile directory carried. Measured three times on the same machine
across five days — 2026-08-26, 2026-08-31, and again immediately before the fix landed — and
identical every time: two trustees with Modify who were neither the user nor administrators, every
entry inherited, `DC` (delete-child) on both of them so the file could also be deleted and replaced.
A pass on 2026-08-27 had fixed only the *reporting* of that gap. This is the gap.

The reasoning is more interesting than the outcome, which is three `icacls` lines.

**Owner + SYSTEM + Administrators, not owner-only.** The obvious target is a DACL granting only the
user. It is the wrong one. An administrator can take ownership of any object and re-grant themselves
anything, so excluding them costs backup and repair tooling real breakage and buys a strictness that
does not exist; and the project brief forbids attempting to bypass administrator controls in the
first place. What was actually worth removing was the *unexpected* trustees and the inheritance that
kept handing them down — which is exactly what the old code comment falsely claimed the inherited
ACL already gave. So "confirmed" on Windows is a deliberately narrower claim than "confirmed" on
Unix, and the settings label still says an administrator can read the token. Stating a smaller true
thing beats stating a larger one that is not.

**The grant comes from the process token, not from the object's owner.** `GetNamedSecurityInfoW`
returns the owner in the same call, and using it is the obvious shortcut. On a machine whose policy
makes `BUILTIN\Administrators` the owner of objects an elevated process creates, that shortcut
produces a DACL listing Administrators and *not* the user — and a later non-elevated run cannot read
its own file. Silent, and only on machines with that policy, which is the worst combination. So the
grant is `OpenProcessToken`/`TokenUser`. The owner is still read back and still judged: an owner
outside the three holds `WRITE_DAC` and can re-grant itself, so it counts as an extra trustee
whether or not it has an entry of its own. On this machine the token user is `S-1-12-1-…`, an Azure
AD SID rather than the `S-1-5-21-…` a hardcoded guess would have reached for.

**The DACL survives the rename — measured, not reasoned.** `discovery::write` restricts a temporary
and then `fs::rename`s it into place. The argument that this is safe (a move within one directory on
one volume preserves the file object and its descriptor; inheritance is re-applied on a copy or a
cross-volume move) is correct, and it was still only an argument. It is now a `#[cfg(windows)]` test
and a probe run. **The read-back moved anyway**, to after the rename, because the old code verified
`local-api.json.new` — a path that does not exist by the time the verdict reaches the caller. On
Unix that verdict was true of the final file by an argument the code never made. That is the same
class of defect one level down, inside the file written to avoid it.

**The directory-before-file ordering is a real property, and it needed a second half.**
`restrict_directory(parent)` runs before the temporary is created, so the temporary inherits a
protected list and the token's bytes never touch disk under the loose one. Measured: a file created
inside a protected directory does inherit exactly the three trustees — but *not* `SE_DACL_PROTECTED`,
so it reports `StillInheriting` and would follow the directory if the directory changed. Hence the
file is still restricted in its own right. And `fs::write` alone put the token's bytes on disk before
anything narrowed the file, which is a real window on Unix too — a `0700` directory does not stop a
`0644` file inside it. The temporary is now created empty, restricted, and only then written into.

**The directory's verdict gates the file's.** A perfectly restricted file inside a directory another
user may delete children of is not a restricted file: it can be deleted and replaced, and the CLI
and MCP client then read an attacker's port and token. For a file whose contents *are* the access
control, that is worse than being readable, not smaller. `file_acl::combine` is the pure function
that says so, and a perfect file in a loose directory reports `DirectoryNotRestricted`.

**Ten states, not four, and the count-not-names call.** The Windows read-back can fail in ways
`Wider { mode }` cannot express — it is Unix-shaped and cannot carry a trustee — and collapsing them
into `Unknown` would report something that was measured as something that was not. So five mismatch
arms, each with its own sentence in `labels.ts`. `ExtraTrustees` carries a **count**, not names: the
value crosses to TypeScript and onto a screen that ends up in screenshots and bug reports, and a
trustee name is one of the machine's account and group names. The SIDs go to a `tracing::warn!`
inside `verify` instead, and the label says where to look. `DaclFacts.dacl` is an `Option` because a
NULL access list grants everyone everything while an empty one grants nobody anything, and code that
conflates them reports the worst case as the best.

**`is_confirmed()`'s line is checked versus unchecked, not strict versus lax.** The old doc comment
argued the other one, and it had to be rewritten rather than extended: `OwnerAndAdministrators` lets
administrators read the file and passes, because the list was read back entry by entry;
`InheritedFromProfile` may well be perfectly restrictive and does not, because nothing looked.
`kw_local_api::Permissions::is_as_expected()` — the helper whose "as restrictive as the platform
allows" is the sentence that misled once and is recorded as a finding in three documents — was
deleted. It had no non-test caller. There are now two `is_confirmed()` definitions, one per side of
the IPC boundary, and two definitions of one line is the shape of the original defect, so
`the_two_confirmed_decisions_cannot_drift_apart` walks every variant and requires them to agree.

**The second false comment.** `kw_sidecar::pid`'s `restrict_file` claimed *"on Windows the file
inherits the user profile's ACL, which already excludes other users"* — the same assertion as the
one in `discovery.rs`, in a different crate, about the same profile directory, equally false. Two
independent comments asserting an unverified fact about the same thing is the argument for one
shared helper rather than two call sites that each get to believe something. The PID record's
verdict is logged rather than reported to the user, and the comment says why: its worst case is a
forged entry `crate::reap`'s command-line check already refuses to act on, so refusing to start a
sidecar over an odd access list would be the worse outcome. A PID is not a secret — it is protected
for **integrity**, and `pid.rs`'s own "a record another user could rewrite is a request to signal a
PID of their choosing" is the sentence worth keeping.

**Two things found and not fixed here, both filed (KNEG-110, KNEG-111); the first has since been
fixed.** The verdict was a **snapshot taken when the API starts** —
`local_api_status` reported what the write measured, and nothing re-read the DACL while the
application ran. During this run a trustee was added to a protected directory by something outside
the application, between the write's read-back and the next `icacls`; the application would not have
noticed until its next start. (That specific ACE was an artifact of the sandboxed harness the
measurement ran under, and did not reproduce when `discovery::write` created the directory itself,
which is what the application does — but the general fact is real.) And **CI never compiles any of
this**: `.github/workflows/ci.yml`'s `contracts` job runs on `ubuntu-latest`, and the
`windows-latest` matrix job is switched off for every trigger but `workflow_dispatch`. The pure
classifier *is* covered there, deliberately — `file_acl::classify` takes plain Rust facts, and the
measured DACL above is a fixture in its test module, so the regression test for a Windows exposure
runs on a Linux box. The three Win32 calls are covered by two `#[cfg(windows)]` tests — the
repository's first — and by `cargo run -p kw-security --example dacl_probe`, which verifies only
when given a path so the real `~/.knegiswhisper` can be re-measured without the default being
destructive.
## 2026-08-31 (KNEG-79) — four clients that heard "no" and said nothing

The four cloud meeting clients each read a provider-reported error frame, logged it with
`tracing::warn!("the provider reported an error")`, and carried on. None of `connect`, `send_audio`
or `finish` ever returned an `Err` because of it, so a provider that rejected the session-config
frame and kept the socket open produced a meeting that "completed successfully" holding an empty
transcript — and an empty transcript becomes `MeetingOutcome::NothingTranscribed`, which is exactly
what a quiet room produces. The gap had been recorded three times, in `BENCH_CHECK.md` item 13,
`BUILD_PLAN.md` and this file's own carry-forward, and fixed none of them.

It stopped being hypothetical on 2026-08-28. Both OpenAI sockets were closed by the server
milliseconds after connecting; the application reported **Recording**, kept the indicator up,
captured microphone and system audio for as long as it was left running, and said nothing. One
`DEBUG nothing is consuming the meeting audio` per second was the only trace.

### What was built

`crates/meeting-core/src/fault.rs`: `SessionFault`, one cell per session shared by both readers and
consulted by both writers, plus `ends_the_session` — a pure, total function answering *does a fault
of this kind mean the session is over?*

The pure split was not optional. There is no socket harness in this repository: no `wiremock`, no
`mockito`, no fake `MeetingTranscriptionProvider`, no `crates/meeting-core/tests/`. Every test in
the four socket modules returned before a socket ever opened, and each module's own header said so.
Anything reachable only through a live socket is untested by construction, which is the same reason
`openai::SessionClock` was pulled out of the socket: *a rule that only runs against a live
sixty-minute call is a rule nobody checks.*

### Three decisions worth the space

**`OnceLock`, not `Mutex`, so first-wins is true by construction.** A cascade of frames after the
real error would bury it, so the first fault has to win. Written as `if held.is_none() { ... }` that
is a convention a later edit can invert; written as `OnceLock::set` it is the type's own behaviour,
and there is no poisoned-lock branch to write around this workspace's `unwrap_used = "deny"`.

First rather than last, because of the one real observation there is. The 2026-08-28 order was:

```text
WARN the provider reported an error       The Realtime Beta API is no longer supported.
WARN the transcription socket was closed  said=... (code 4000)
```

The first line explains the meeting. The second is a number. Last-wins keeps the number.

**One cell per session, not one per socket.** Every client here refuses to open only one socket —
"a session with only the microphone open would transcribe half the meeting and report success" — so
a dead system-audio socket has to end the session even while the microphone socket is healthy. The
two almost always die of the same cause, and the recorded sentence names which capture it came
from, so one true sentence beats the same sentence twice.

That has one sharp edge, and it is the reason `openai_socket::reconnect` swaps in a *new* cell
before rebuilding. `Connection::close` sends a close frame and only then aborts the reader, so the
socket a reconnect retires can still read the server's reply — and a close frame is a
`FaultKind::Reported`. Shared across generations, this fix would have failed every meeting over
sixty minutes at its own reconnect: a new defect introduced by the fix for an old one.

**The non-JSON case, decided rather than left to fall through.** All four `interpret` functions
answered *any* unparseable frame with `Incoming::Failed`, so an HTML error body and a text keepalive
this parser happens not to know were the same thing. Making `Failed` fatal without splitting them
would have killed a working meeting over a keepalive — a new defect of the fix's own making, and
the exact failure mode this change exists to prevent.

So there is a fifth variant, `Incoming::Unreadable { preview }`, in all four rules modules, and the
rule is about *when* it arrives: **an unreadable frame on a socket that has never yet produced a
frame this client could parse ends the session; one that arrives after it has does not.** That
targets the observed hazard — garbage where a session-accepted frame belonged — and cannot kill a
stream that has demonstrably been speaking this client's language. "Understood" means *parsed*, so
`Ready`, a transcript, an error frame and even an event this client does not act on all count; ping
and pong never reach an `interpret` call. The loop supplies one `bool` and `ends_the_session` holds
the rule.

The frame itself is repeated back rather than replaced with "not JSON", through one shared
`fault::preview` — control characters flattened, runs collapsed, capped at 120 characters, because
provider responses are outside the trust boundary and this text reaches both a log field and a
sentence on screen. `<html>502 Bad Gateway</html>` is worth ten diagnostics that say "not JSON", and
the wire most likely to produce one is Corti's, which was invented with no vendor documentation at
all.

### Edited four times rather than extracted

`type Socket`, `type Sink`, `struct Connection`, `Connection::close`, the whole `read` skeleton,
`fn connection`, `pause`/`resume`/`cancel` and the `static FALLBACK` blocks are near-duplicated
across the four socket modules. Unifying them is a real refactor with its own risk and KNEG-89
already tracks it, so what was extracted here is the *decision* — one `SessionFault`, one
`ends_the_session`, one `close_reason`, one `preview`, one `SocketFault::into_error` — and the four
call sites were edited by hand.

Four hand-applied copies of a security-relevant decision is exactly how one gets missed, so
`all_four_cloud_clients_record_a_fault_and_consult_it_at_both_ends` reads the four modules' own
source with `include_str!` — the device `kw_network_policy` already uses for
`forbidden-domains.json` — and fails if any one of them lacks the field, any of the four record
calls, or the two `self.fault.take()` sites. It is a text match and will not catch a client that
consults the cell and ignores the answer; the two behavioural tests now at the bottom of each socket
module do that, one client at a time.

Those two tests per module are the first in those four files to reach past the guard clauses, and
they exist because `send_audio` checks the cell **first** — before `paused`, before the reconnect
clock, before the connection lookup. A test can then record a fault exactly as the reader task would
and ask what `send_audio` and `finish` do about it, with no socket, no key and no network. Placing
the check first is also correct on its own terms: a provider that reported an error and kept the
socket open produces a *successful* send, so nothing about the send can be asked.

The per-socket `Closed` cell in `openai_socket` was **not** folded in. Two cells, two jobs: that one
supplies better words for a send that has already failed, this one decides whether the session is
over. Folding them would have put `ad9dac1`'s hard-won behaviour — the thing that made five GA-API
faults diagnosable — at risk for a tidiness gain.

### `stop()`, which was worse than it looked

`stop()` returned early from three places: the recorder thread having stopped, the thread not
answering, and the pump failing. `Live` had already been `take`n by then, so each of them left the
runtime idle while skipping the whole tail — no `OUTCOME_EVENT`, no `MeetingState::Idle`, no
`meeting_window::set_visible(false)`, no `watch.forget`.

And the screen was then stuck permanently, not briefly: `guard` in `useRefresh.ts` does not
`reload()` when a mutation throws, so the frontend stayed on `finishing`, where
`SettingsMeetings.tsx` renders neither Stop nor Discard and disables Start. No working button, tray
still lit, indicator window still up, until the application was restarted.

Every failure is an outcome now. Three free functions came out of it — `close_recorder`,
`pump_result`, `failed_message` — testable with no `AppHandle`, `Db` or `NetworkPolicy`, which is
the shape `resolve_key_ref` and its neighbours already use in that file.

**The whole-file local pass still runs after a pump failure**, and that is a fix in its own right
rather than a concession: the saved note comes from that pass and only falls back to the cloud
partials, so a provider that dies at second zero costs the live transcript, not the meeting. The old
`?` skipped the pass entirely, which was a pure loss. `keep` takes `live_failure: Option<&str>` and
uses it twice — an empty result becomes `Failed` naming what the provider said instead of
`NothingTranscribed`, and a note that *was* produced says in its own text that the live half failed.

### No fake provider, and what it would take

`pump` has zero test coverage and still does. Introducing the repository's first fake
`MeetingTranscriptionProvider` was considered and declined: `pump` already propagates its `Err`
correctly through `?`, so the fake would have tested a line that was not broken, while the broken
line was in `stop()` — which needs an `AppHandle`, a `Db` and a `NetworkPolicy`, none of which a
fake provider supplies. The free functions cover what actually changed.

Recorded for whoever does want it: the obstacle is `pump`'s `app: AppHandle`, and the better of the
two routes is giving `pump` a small `TranscriptSink` trait — matching what `calendar-core` did with
`TokenSender` and its test-only `PlaintextLoopbackSender` — rather than enabling `tauri`'s `test`
feature, which nothing in this workspace uses.

**This entry predicted KNEG-95 would need that, and it did not.** Corrected here rather than left
standing: the decisions KNEG-95 turned out to need — which state to report, whose session an ending
belongs to, which sentence to use — are all free functions over plain values, the same shape
`pump_result` and `close_recorder` already had. `pump` is still untested and still does not need to
be. What a `TranscriptSink` would buy is a test of `pump` itself, which nothing has yet wanted.

### What stayed KNEG-95's, and was taken later the same day

**A dead provider was still silent mid-meeting when this landed.** The indicator stayed up,
recording continued, and nothing reached the screen until Stop. No new `MeetingState` variant, no
change to whether recording continues, `MeetingRuntime::status()` not consulting
`pump.is_finished()`, and no mid-meeting error event — all deliberate, because the choice between
stopping the recording, falling back immediately, and carrying on while saying so belonged to the
repository owner.

That choice was made on 2026-08-31: **carry on, and say so.** Every one of the four items above is
now done, and the reasoning is in the 2026-08-31 (KNEG-95) entry above — including the two races
that decision turned out to contain, neither of which was visible from here.

### What could not be verified, and is not claimed anywhere

**No key exists on any machine this project has touched for AssemblyAI, Deepgram or Corti.** Three
of the four clients' behaviour against a real provider error is unverified, and the tests added here
verify this application's wiring rather than any provider's behaviour. The argument for the change
is that it is exactly the mechanism that would have surfaced the five OpenAI faults of 2026-08-28 in
one attempt instead of five. That is an argument, not evidence, and the code comments,
`PLATFORM_SUPPORT.md` and `BENCH_CHECK.md` item 13 all say so in those terms.

One risk is written into `ends_the_session`'s doc comment rather than hidden: `FaultKind::Reported`
ends the session on the **first** error frame, and a provider that emits benign `error` frames
routinely would make that too eager. The mitigation, if it ever matters, is one line — give
`Reported` the same "before anything was understood" test `Unreadable` has.

### Found while here, filed rather than widened

`deepgram::transcript`'s `gaps: 0` carried the comment *"Nothing is dropped by this client: the
socket either carries a frame or the send fails and the session ends."* That was untrue for a second
reason, predating all of this: `deepgram_socket`'s reader `return`s on `TranscriptBudget`
exhaustion, so a session past `MAX_TRANSCRIPT_BYTES` or `MAX_TRANSCRIPT_EVENTS` keeps recording and
transcribes nothing further — silently, and uncounted, because `gaps` is hardcoded to zero. Bounding
untrusted provider output is deliberate; not counting what the bound costs is a gap. The comment now
says so and the behaviour is unchanged.

> **Fixed the same day (KNEG-109), and the filing understated it.** The reader no longer returns, so
> the ceiling bounds retention without also ending transcription — and without silently switching
> *this* change off for the rest of the meeting, which is what a dead reader did to `SessionFault`.
> See the KNEG-109 entry at the top of this file.
## 2026-08-31 (KNEG-80) — a path the renderer could invent, and why the capability beat the fusion

`Notes > Upload` had a hole with a very short description and a very long consequence.
`pick_import_file` opened a real OS dialog and returned the chosen path **to the renderer**, which
held it in React state and passed it as a plain string to `media_import_audio`,
`media_import_video` or `import_openwhispr` on a later click. Those three read whatever path they
were called with. Nothing confirmed it was the one the dialog returned.

The half that held is worth stating first, because it is the reason this sat unnoticed:
`apps/desktop/src-tauri/capabilities/default.json` grants exactly `["core:default"]` — no `fs:*`,
no `dialog:*`, no `shell:*`, no `http:*` — so the renderer genuinely has no independent way to
*obtain* a path. Every path it could hold came from a real dialog. But "cannot obtain" and "is
checked" are different claims, and only the first was true. A compromised renderer — which
`docs/SECURITY.md`'s threat model includes, so this is not an invented adversary — could call
`media_import_audio` with any path it liked, and the path becomes argv for a spawned ffmpeg, which
writes `converted.wav`, which is read into memory and handed to
`kw_transcription_core::Executor::transcribe`. With a cloud provider configured, that is an
exfiltration path for any readable file on the machine, reached with one IPC call.

### Why a capability rather than fusing pick and import

`note_export_to_file` already demonstrates the airtight shape: it fuses picking and writing into
one atomic command, so the frontend never sees or supplies a path at all. Copying that shape here
was the obvious first answer and it is the wrong one, for a reason that is about the screen rather
than about security. Export is a single click — choose where, write, done. Import is
pick-then-review-then-confirm: the user picks a file, the screen shows what it picked and what
provider will transcribe it, and *then* they press Import. Fusing would collapse the review step
that the screen exists to provide. A guarantee bought by deleting the interaction it was protecting
is not a good trade.

So the path still crosses the boundary, and what makes it safe is a capability instead:
`commands::DialogGrants`, a `Mutex<VecDeque<PathBuf>>` managed in `lib.rs`, written only by
`pick_import_file` and consulted by all three path-accepting commands before they touch anything.

Three implementation choices, each of which had a plausible alternative that is worse:

- **Session-lifetime, not single-use.** Single-use is the tighter policy and it breaks a flow that
  works today: `NotesUpload.tsx`'s `catch` leaves the picked path in state after a failure, so the
  same file can be retried without re-picking. A grant consumed by the first attempt would turn
  every failed import into a re-pick. There is a test for the two-attempt case specifically, so the
  next person to reach for single-use has to delete an assertion that says why not.
- **Bounded by count, not by time.** A time bound would refuse a file the user really did pick if
  they left the review step open — and they realistically will, because that screen can sit on a
  picked path across an ffmpeg install of tens of megabytes. This is the same argument
  `meeting::Watcher::offered` makes in its doc comment for keying on the provider rather than on a
  timestamp: a time-tuned suppression gets one of its two cases wrong whichever way it is set, and
  identity gets both right. Reusing that reasoning was better than restating it. The bound is 32,
  which the screen (one path at a time) will never approach; it exists so a renderer calling the
  picker in a loop cannot grow the set without limit.
- **Keyed on a canonicalised `PathBuf`, never the string.** `pick_import_file` returns
  `path.display().to_string()`, which is **lossy**. Comparing strings would mean comparing a path
  that had already lost information. Both the insert and the check go through `std::fs::canonicalize`
  — which on Windows returns the verbatim `\\?\C:\...` form, so both sides must canonicalise or a
  canonical path gets compared against a raw one and never matches.

### What the lossy path costs, stated rather than buried

A path whose name does not survive `display()` cannot be imported. That is a real refusal and it is
written into the code as one. It is **not** a regression: `check_importable_file`'s `is_file()`
already failed on that same lossy string today, silently and by accident. The change is that it is
now deliberate and says so. If someone with a filename that does this ever reports it, the answer
is to stop round-tripping the path through the renderer at all — i.e. the fusion above, for that
one case — not to compare strings.

One smaller decision inside `authorise`: a path that cannot be canonicalised and a path nobody
granted get the *same* refusal. Distinguishing them would make the command answer "does this file
exist" for any path a compromised renderer cared to name, which is a smaller leak than the one
being closed but is free to avoid — and a path that will not canonicalise cannot be in the set
anyway, since every member of it came out of `canonicalize`.

### The surface is closed, and that was checked rather than assumed

`pick_import_file` is the only dialog-returning command that hands a path to the renderer.
`note_export_to_file` returns one too, but only as an after-the-fact report of what it already
wrote, and nothing accepts it back. Every `#[tauri::command]` in
`apps/desktop/src-tauri/src` was enumerated and its parameters read: exactly three take a path —
`commands::import_openwhispr` (through `ImportRequest.path`), `media::media_import_audio` and
`media::media_import_video`. The other struct-shaped parameters (`NoteUpdateRequest`,
`CleanupSettings`, `EnterpriseConfig`, `Lists`) carry no path field; `CleanupSettings.self_hosted_url`
is a URL and goes through the network policy, not the filesystem.

### What this does not prove

Four unit tests assert the policy — an ungranted path is refused, a granted one survives two
imports, the oldest is evicted past the bound, and an ungranted credential-file path is refused
rather than read (including one reached by traversal that canonicalises onto it). `authorise` takes
no `AppHandle` precisely so those tests can exist; there is no Tauri command test harness in this
repo, so anything reachable only through a command is untested by construction. **Only a real call
against the running application proves the policy is consulted on this path**, and that has not
been run. It is item 1 under Task 12 in `BENCH_CHECK.md`, where it was already written before this
change and where it stays.

Dev mode was checked, not assumed: all four commands are stubbed in `devFixtures.ts`, which
intercepts at `invoke` and throws loudly on anything unstubbed, so no fixture reaches Rust and none
needs a fake grant.

Two doc corrections came along with it. `config/ui-surfaces.json`'s `Notes > Upload` note still
described "Audio, video and URL import" months after URL import was withdrawn — the parity gate
checks `surfaceId`, `component`, `built` and the `data-surface` marker, never the note prose, so
nothing was ever going to catch that. And the same entry said nothing about path restriction. Both
fixed.

---

## 2026-08-31 (KNEG-45) — 39 GGUFs get a digest, and a second standard gets a name

The GGUFs were the last download path with no digest. Closing that was declined twice on the same
grounds — 202 GB of downloading is not justified for a pin — and what changed is not the willingness
to spend it. It is that the pin was available for 36 HTTP requests.

### The lead, and what it took to believe it

The whisper-pins run on the same day found that
`https://huggingface.co/api/models/{repo}/tree/main?expand=true` returns `lfs.oid` per object, and
that this **is** the file's SHA-256 — git-lfs is content-addressed on exactly that hash, so the
pointer file committed in the repository is the digest. It agreed with all six measured whisper
digests. `ggml-large-v3.bin`'s row was read from the API while its 2.9 GB download was still in
flight and predicted the answer correctly.

That was one repository. The issue said so, in as many words: *"Nobody has compared an `lfs.oid`
from a bartowski GGUF repo against a real download."* So the first thing done here was not to write
code.

**36 tree calls covered all 39 objects.** Every one returned a well-formed 64-hex `lfs.oid`, and
every one's `lfs.size` matched the byte count already committed from `x-linked-size`. That second
agreement is worth naming precisely: it is one Hugging Face instrument confirming another, not an
independent measurement, because both are the same publisher's metadata.

**Then the smallest object from each of the seven publishing organisations was downloaded in full
and hashed** — bartowski, Qwen, mradermacher, ggml-org, google, unsloth, LiquidAI, 23.6 GiB — by
`tools/model-pin/pin.mjs`, hashing each stream on its way to disk and taking the length from `stat`.
All seven digests and all seven lengths matched the stated ones exactly. With whisper's six that is
**thirteen objects across eight organisations, 30.08 GiB, no disagreement.**

Those seven are not thrown away afterwards. They are committed as `sha256Measured: true`, which
makes the table honest about itself rather than uniform.

### The distinction, and where it is enforced

`LOCAL_LLM_SIZES` became `LOCAL_LLM_PINS` and every row now carries a digest. Seven carry a flag
saying this project hashed it.

**What a stated oid buys.** An exact length turns "roughly this file" into "a file of exactly
6,169,341,984 bytes", which was `KNEG-101` and was a real narrowing. It is still satisfied by *any*
6,169,341,984 bytes. A digest is satisfied by one file. Corrupted transfer, truncation that
happened to keep the length, accidental re-upload: all three now fail where they used to install.

**What it does not buy, and this is the sentence the implementation had to keep.** The oid comes
from the same host as the bytes. A compromised huggingface.co would serve both halves of the
comparison, and a digest committed to *this* repository is what defends against that. So the two
are not the same claim and must not be reported as one.

Three places enforce that, rather than one comment asking nicely:

- **The type.** `DigestCheck` has `Measured(String)` and `Stated(String)`, so nothing downstream can
  reach the digest without having been handed which kind it is. A `String` and a `bool` beside it
  would have let a caller read one and forget the other.
- **The wire.** `local_model::VerifiedBy` gained a fourth variant, `StatedDigest`, spelled
  `"stated-digest"`. The enum's own comment already predicted this: it exists because the frontend
  used to re-declare a `&'static str`'s values by hand, so *"a fourth state added here would have
  reached a UI that did not know about it"*. It duly arrived.
- **The screen.** *checksum verified* and *checksum verified against the publisher's own* are
  different rows in `verificationLabel`, and the row says it **before** the Install button as well
  as after, for the reason the size is said before it.

### On overturning a prohibition

The comment being replaced said, in bold: *"do not add a `sha256` here on the strength of it. A
digest belongs to bytes somebody actually hashed."* Written by the same run that measured the
whisper pins, and it was right about what it was aimed at — the only alternative then on offer was
deriving a digest from one download, which pins whatever arrived while looking like a guarantee. It
still forbids that.

An oid is a different object. It is stated by the publisher *before* any transfer, which is what
makes it testable against one, and thirteen downloads is what testing it looked like. The rewritten
comment carries the old prohibition verbatim and says why this is not an instance of it, because a
rule that quietly disappears when it becomes inconvenient teaches the next reader nothing.

### The check costs a full read of the file

Verification is a second pass: `kw_sidecar::integrity::hash_file` reads the finished download back
off the disk. For `qwen3-32b-q4_k_m` that is 19.8 GB, so it runs under `spawn_blocking` and emits
its own progress state. `LocalLlmProgress::Verifying` exists because a progress line that stops
moving for a minute reads as a hang, not as work.

Two deliberate limits, recorded rather than implied:

- **Length first, then hash.** The length is free — it is already on the filesystem — and a
  truncated transfer is the common failure, so checking it first turns the common case into a
  specific message rather than a digest mismatch that could mean anything.
- **The hash happens at install and is still not repeated.** The whisper path *does* re-hash on
  every status call, and gets away with it because its largest model is 3 GB; re-reading 20 GB every
  time a settings screen opens is not a trade worth making. What changed on 2026-09-01 is what the
  screen says afterwards: `installed: true` used to mean only that the path existed, so a model
  corrupted after a successful install was not caught and did not look any different. KNEG-107
  replaced that boolean with a four-state `GgufIntegrity` read from a record written beside the
  file, and added a re-hash a user can ask for. See the dated entry.

### An install is as verified as its weakest half

Five models download a speculative-decoding drafter alongside the GGUF, from a different repository.
`VerifiedBy::weaker` picks the lower of the two verdicts for the `Done` event, so a drafter riding
in on a stated oid cannot be reported under the model's measured one. Its ordering is written as
explicit ranks rather than derived from `PartialOrd`, so reordering the variants for readability
cannot silently reorder the strengths.

`Done`'s `verifiedBy` is `Option`, and `None` means the files were already on disk and this install
checked nothing — which is honest where borrowing the previous install's claim would not be.

### `--verify-stated`, and why it is a second verb

`tools/model-pin/pin.mjs --verify` downloads and re-hashes. It cannot cover these 39 objects for the
reason they went unpinned for so long. `--verify-stated` re-reads the trees instead: 36 requests, no
bodies, a few seconds, and it fails if any digest or length has moved. The weekly `pin-verify.yml`
runs it beside the two download-based checks.

Two commands rather than one flag, because a tool that prints "verified" must never be vague about
which of those it did. Its success line says how many of the objects it checked are pinned to bytes
hashed here — the ones it is *not* the right instrument for.

For the thirty-two stated pins this is the same standard they were taken at, so agreement means
"unchanged". For the seven measured ones it is strictly weaker, and the test's doc comment says so:
agreement means Hugging Face has not re-uploaded, not that the bytes are still what we hashed. Only
a download proves the second thing.

### Two counts are asserted rather than assumed

`every_local_model_carries_a_digest_the_installer_will_check` asserts 39 objects, zero with no
digest, and **exactly seven** measured. The last of those is not a lower bound on purpose. If it
moves, either somebody measured more — good, and the number should follow in the same commit — or a
pin was quietly downgraded, which is the thing worth catching. The same shape asserts 39 in
`pin.test.mjs`, so a pin dropped from `LOCAL_LLM_PINS` cannot make `--verify-stated` check 38
objects and pass.

---

## 2026-08-31 (KNEG-100/102) — a ceiling that lied, four unbounded downloads, and 21 measured archives

`f17a5fd` added two real protections and left them almost unused.
`NetworkPolicy::download_to_file_bounded` refuses a body larger than a pinned length before it
reaches disk; `kw_sidecar::extract_with_limit` caps what an archive may write when it is unpacked.
One caller used either. This pass made both of them true everywhere they belong, and found that
the extraction half had never worked for four of its five archive kinds.

### The ceiling applied to one archive kind in five (KNEG-102)

`extract_with_limit`'s doc comment said "refusing to write more than `max_bytes` in total". Its
body threaded `max_bytes` to `TarBz2` alone; `Zip`, `TarGz`, `TarXz` and `Bare` took the argument
and discarded it. **A ceiling that lies is worse than no ceiling** — it is the difference between a
caller knowing it is unprotected and a caller believing it is protected. Nothing was exposed by
it, because the only caller passing a limit installs `.tar.bz2` models. Luck, not design, and the
luck would have run out at the next commit, which threads limits through the sidecar path — `zip`
on Windows, `tar.xz` on Linux.

**The header rule is not enough, and this was read rather than assumed.** The `TarBz2` path
accumulated `entry.size()` and refused before writing. That is exact for a tar: `tar::Entry`'s
reader is an `io::Take` over the same header size (`tar-0.4.46/src/entry.rs`, `EntryIo::Data`), so
the bytes cannot disagree with the header. A zip entry's reader is a decompressor over
`take(compressed_size)` (`zip-2.4.2/src/read.rs`, `make_reader`), and nothing in it holds the
inflated output to the `uncompressed_size` the central directory declares.
`a_zip_whose_header_understates_its_output_is_refused_on_the_real_bytes` patches a one-entry zip's
declared size down to 1 for a 1 MiB payload; the crate hands over all 1 MiB. A header-only ceiling
would have passed it, which is precisely the decompression bomb the ceiling exists for. So the
copy is now bounded on the bytes that arrive as well, with the declared-size check kept in front of
it as the fast refusal that stops an oversized entry landing at all.

**A refusal takes back what it wrote.** `extract_tar_limited` returned `Err` and left every file it
had already written; so did `extract_tar` and `extract_zip` on a corrupt entry or an unsafe name. A
half-unpacked directory can look convincing to a later `locate`. The rollback removes its own files
and then its own directories, deepest first, with `remove_dir` rather than `remove_dir_all` — and
never `dest`, because for `sherpa_models::install` `dest` is the models *root* and clearing it would
take every other installed model.

**`InstallError::ExtractedTooLarge`** replaces `Archive { detail: "extracted data exceeds …" }`.
`tests/tar_bz2_fixture.rs` had asserted on that substring because there was nothing better to match
on.

One test per `ArchiveKind`, every one overflowing on a *later* entry. The only ceiling test that
existed used `max_bytes = 1`, which trips on the first entry and so proves nothing about partial
output. Both sides of the boundary are asserted: an extraction fitting its ceiling exactly succeeds
for all four archive kinds, because a bound that refuses a legitimate install is worse than no
bound.

### The extracted footprint of all 21 published sidecar targets, measured

`Asset` had `sha256` and `sizeBytes` and no analogue of the `maxExtractedSizeBytes` the sherpa
models carry, so there were two options: add the field and measure 21 targets, or bound the
download only and say the extraction ceiling for sidecars is deliberately absent. The measurement
turned out cheap — 1.85 GB in total, and `tools/sidecar-pin/pin.mjs` already downloads and indexes
every archive on `--verify` — so the field exists and is mandatory.

`node tools/sidecar-pin/pin.mjs --extracted --write`, 2026-08-31. Download and extracted are
measurements; ceiling is the judgement.

| target | archive | download | extracted | ceiling |
| --- | --- | --- | --- | --- |
| whisper/windows-x86_64 | zip | 8,361,840 | 21,213,696 | 27,262,976 |
| whisper/linux-x86_64 | tar-gz | 9,503,425 | 24,527,510 | 31,457,280 |
| whisper/linux-aarch64 | tar-gz | 4,572,294 | 10,587,134 | 13,631,488 |
| llama/windows-x86_64 | zip | 17,287,711 | 44,587,872 | 56,623,104 |
| llama/linux-x86_64 | tar-gz | 15,646,315 | 39,784,422 | 50,331,648 |
| llama/linux-aarch64 | tar-gz | 12,665,261 | 31,133,878 | 39,845,888 |
| llama/macos-x86_64 | tar-gz | 11,280,969 | 27,561,662 | 34,603,008 |
| llama/macos-aarch64 | tar-gz | 10,978,734 | 27,450,446 | 34,603,008 |
| ffmpeg/windows-x86_64 | zip | 145,349,121 | 343,922,176 | 429,916,160 |
| ffmpeg/linux-x86_64 | tar-xz | 111,679,252 | 348,784,736 | 436,207,616 |
| ffmpeg/linux-aarch64 | tar-xz | 96,564,568 | 283,579,184 | 355,467,264 |
| yt-dlp/windows-x86_64 | bare | 17,840,399 | 17,840,399 | 17,840,399 |
| yt-dlp/linux-x86_64 | bare | 40,446,224 | 40,446,224 | 40,446,224 |
| yt-dlp/linux-aarch64 | bare | 40,167,448 | 40,167,448 | 40,167,448 |
| yt-dlp/macos-x86_64 | bare | 37,146,048 | 37,146,048 | 37,146,048 |
| yt-dlp/macos-aarch64 | bare | 37,146,048 | 37,146,048 | 37,146,048 |
| sherpa/windows-x86_64 | tar-bz2 | 24,378,225 | 43,812,352 | 55,574,528 |
| sherpa/linux-x86_64 | tar-bz2 | 384,752,102 | 1,249,715,232 | 1,562,378,240 |
| sherpa/linux-aarch64 | tar-bz2 | 337,820,882 | 920,336,696 | 1,151,336,448 |
| sherpa/macos-x86_64 | tar-bz2 | 254,918,578 | 879,667,336 | 1,099,956,224 |
| sherpa/macos-aarch64 | tar-bz2 | 231,438,974 | 777,705,592 | 973,078,528 |

Four things this measurement decided rather than assumed:

- **Extracted is not the archive's uncompressed size.** Only the entries inside the payload
  directory are ever written — the Windows whisper archive carries `main.exe`, `SDL2.dll`,
  `llama.dll` and a dozen test binaries that are not — so `extractedBytes` mirrors
  `install::payload_relative` rather than summing the index.
- **For zip, the entries are inflated rather than read off the central directory.** Same reason the
  Rust now bounds on real bytes: `uncompressed_size` is a declaration. The tool decompresses each
  payload entry with `inflateRawSync` and counts what comes out.
- **The ceiling is one stated rule, not 21 numbers.** Measured + 25%, rounded up to the next MiB
  (`withHeadroom`). The drift a pinned asset sees between upstream builds is a few percent — a
  rebuilt binary, an added microarchitecture DLL — not a multiple, and 25% absorbs that while still
  refusing something that decompresses to several times what was measured. It errs high because
  being wrong high costs protection and being wrong low refuses a legitimate install, which is the
  trade `SHERPA_PINS` already records for its two hand-chosen figures.
- **`Bare`'s ceiling is its download size, with no headroom.** The file is copied verbatim, so
  headroom above a digest-pinned length could only ever be reached by a different file.

**All 21 digests were re-verified in the same run**, as a side effect worth stating: the tool
refuses to measure an asset whose digest has moved, because a ceiling derived from unpinned bytes
pins the wrong number. `--verify` now re-measures the extracted total too and fails when a
committed ceiling has become too tight to install through — the failure mode
`docs/PLATFORM_SUPPORT.md` records for the sherpa `sizeMb` figure, caught by CI rather than by a
user.

### Which downloads got bounded, and which deliberately did not

| caller | download | extraction |
| --- | --- | --- |
| `sidecar::install` — all 21 targets | `sizeBytes`, exact | `maxExtractedSizeBytes` |
| `sherpa_models::install` | already bounded | already limited |
| `models::install` — whisper `.bin` | `expectedSizeBytes` where `Integrity::Pinned` | n/a |
| `local_llm::download_gguf` | `sizeBytes` where `SizeCheck::Exact` | n/a |
| `examples/sherpa_probe.rs` | `expectedSizeBytes` | `maxExtractedSizeBytes` |

**A bound is only ever an exact figure**, and that is the whole reason two of those rows are
conditional. A bound refuses a transfer mid-flight, so a *declared* length cannot be one: the true
length may be on either side of it. Bounding `SizeCheck::Approximate` would have reintroduced
KNEG-101 in a faster form — eighteen GGUFs were declared 6.7–7.9% larger than the file Hugging Face
serves, and the version of `download_gguf` that compared the *finished* download against that
figure deleted all eighteen after 101 GB of successful transfer. A bound would have refused them
sooner and just as wrongly. So `Approximate` and `Integrity::Size` keep their tolerance as a
post-download check and never become bounds, which leaves an unmeasured model degraded rather than
broken. Every shipped model reaches the bounded arm today.

### The partial-file leak, and the first test for `DownloadTooLarge`

`download_to_file_bounded`'s streaming refusal returned after creating and partially filling the
file, with no `remove_file`. So did the mid-stream write error and the chunk error from a dropped
connection — three returns, no cleanup, and a truncated body left on disk that a later
`is_installed` check finds as convincing as a refused one. The body half is now
`stream_to_file`, so every failure leaves through one cleanup path in the caller.

**`PolicyError::DownloadTooLarge` had no test anywhere.** It was written, shipped, used by one
caller and never exercised. There are four now, against a minimal loopback HTTP server in the test
module (extending the `respond_once` helper `get_text` already used, rather than adding a mock
harness). They cover something the one-line doc comment had not recorded: **the `content-length`
pre-check often does not run at all.** It uses `Option::zip`, so a response with no
`content-length` — a chunked one, or HTTP/1.0 with `Connection: close` — skips it entirely, and the
running total in the streaming loop is the check that actually guarantees the bound. That is
correct behaviour, since a header is a claim and a body may be longer than its own header says, but
it was undocumented. The tests exercise both shapes, plus a body exactly on its limit (the normal
case, since every real bound is an exact pinned length) and an unbounded download of any length.

### Verified against the real network

- `cargo test -p kw-sidecar --test real_install -- --ignored` — the pinned whisper sidecar
  downloads, verifies and unpacks under both new bounds.
- `cargo test -p kw-transcription-core --test local_dictation -- --ignored` — a real whisper model
  under the exact-size download bound.
- `cargo test -p kw-transcription-core --test subtitle_export -- --ignored` — a sidecar *and* a
  model, so both new bounds in one run.

None of them refused a legitimate install, which was the risk worth checking: a bound that refuses
one is worse than no bound.

### Filed, not fixed

**KNEG-106** (Low). `extract_tar_xz` decompressed the whole tar into a `Vec<u8>` before looking at
an entry, because `lzma-rs` decompresses into a `Write` and exposes no streaming `Read`.
`max_bytes` bounds what reaches `dest` and could not bound that buffer, so a `.tar.xz` was a memory
bomb whatever ceiling was passed — independent of the output ceiling it now honours. Behind a
mandatory digest, and the only pinned `tar-xz` assets are ffmpeg on Linux at ~100 MB compressed and
~300 MB extracted. **Fixed the same day**: a bounded channel between the decompressor and the tar
reader. See the KNEG-106 entry above — the interesting part is that the first test for the bound
passed against a sixty-four-gigabyte queue.

---

## 2026-08-31 (whisper pins) — six digests measured, and three pin tables become one

`crates/transcription-core/src/models.rs` had carried, since it was written, a `PINNED` table with
one row in it and a paragraph explaining why: pinning the other five whisper models "means
downloading roughly seven gigabytes once, which is a job for a tool rather than something to invent
digests for here". `tools/model-pin/pin.mjs` (`11f0846`) is that tool. It downloaded all six.

### What was measured

Every model fetched in full from its registry `downloadUrl` on 2026-08-31, hashed as the stream
passed through to a file, and its length taken from `stat` afterwards — never from
`content-length`, because a truncated body agrees with its own header. 6,966,596,962 bytes in one
run — 6.49 GiB, 14 min 33 s of transfer on this connection.

| model | file | SHA-256 | bytes | baseline declared |
| --- | --- | --- | --- | --- |
| `tiny` | `ggml-tiny.bin` | `be07e048…c6e1b21` | 77,691,713 | 78,000,000 |
| `base` | `ggml-base.bin` | `60ed5bc3…ba2efe` | 147,951,465 | 148,000,000 |
| `small` | `ggml-small.bin` | `1be3a9b2…ea987b` | 487,601,967 | 488,000,000 |
| `medium` | `ggml-medium.bin` | `6c14d5ad…156208` | 1,533,763,059 | 1,570,000,000 |
| `turbo` | `ggml-large-v3-turbo.bin` | `1fc70f77…e2bc69` | 1,624,555,275 | 1,670,000,000 |
| `large` | `ggml-large-v3.bin` | `64d182b4…94d1e2` | 3,095,033,483 | 3,140,000,000 |

**`tiny` is the control, and it reproduced.** `be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21`
and 77,691,713 bytes, identical to what `models.rs` had committed — a different day, a different
tool, the same answer. That validates the measurement of the other five in the same run as it
re-validates the one pin that already existed.

The declared column is the shape of the whole baseline: every figure is a round million or ten
million, and every one of the six is **high**, by 0.033% to 2.797% — `base` by 48,535 bytes and
`turbo` by 45,444,725. None of them is a measurement of anything, and the errors do not follow a
constant, so they are not the unit-confusion bug `98e836b` found in the GGUF sizes. They are
somebody rounding up.

### An unpredicted finding: Hugging Face states the SHA-256, and it is right

`https://huggingface.co/api/models/ggerganov/whisper.cpp/tree/main?expand=true` returns, per LFS
object, `lfs.oid` — which *is* the file's SHA-256, because that is what git-lfs keys on. It agreed
with all six measured digests and all six measured lengths, on the nose. `large`'s row was read
from that API while its 2.9 GB download was still in flight and predicted
`64d182b4…94d1e2` / 3,095,033,483 correctly.

That matters for **KNEG-45**, now re-scoped to only the 35 local-LLM GGUFs, declined on the
grounds that 202 GB of downloading is not justified for a pin. There is a third option between
"size only" and "bytes we hashed": pin the publisher's stated `lfs.oid`. The cost is **one API call
per repository**, not per object — `tree/main` lists every file in the repo with its oid and size in
one response — so the whole 39-object table would be a handful of requests rather than 202 GB.
Six-for-six agreement over 6.9 GB is real evidence that the metadata is accurate.

**It is a weaker standard and the distinction has to survive.** The oid is a claim by
huggingface.co, delivered over the same channel as the bytes. It defends against a corrupted
transfer and against an accidental re-upload; it does not defend against a compromised
huggingface.co, because one host would serve both halves. A digest measured once and committed to
*this* repository does defend against that, which is the whole reason `config/sidecars.json`'s rule
is "a pin is only a pin if it was measured". So: a middle tier, strictly better than a length,
strictly worse than the strong pin, and it must never be written into a table that reads as the
strong pin.

**Recorded as a lead on KNEG-45 rather than done here — and taken up later the same day.** The lead
held: see the KNEG-45 entry above, where seven objects across seven publishing organisations were
downloaded and hashed to test it before any of it was written down as a pin.

### Three pin tables became one

The pins are `WHISPER_PINS` in `tools/integration-audit/generate-provider-registry.mjs`, beside
`SHERPA_PINS` and `LOCAL_LLM_SIZES` — **not** an extended `PINNED` array in Rust, which is the
architecture call worth recording. The argument is not tidiness. Three model families keeping their
pins in three different places had already produced a concrete absurdity: `model-pin --verify` had
to **parse Rust from Node** to check the whisper half, with a regex over
`const PINNED: [(&str, &str, u64); N]` and the array's declared arity as a checksum on the parse.
That reader worked. It should never have needed to exist.

`models.rs` now reads `sha256` and `expected_size_bytes` off
`kw_provider_registry::WhisperModel`, the generator emits both, and `--verify` reads one generated
JSON document for all eight pins. `parseWhisperPins` and its five tests are deleted. They were
written as a deliberate tripwire for exactly this migration and they fired once, which is what a
tripwire is for — retiring them is the outcome, not a loss of coverage.

### `expectedSizeBytes` is the measured length now, and one claim was false

The registry's `expectedSizeBytes` for whisper is overridden with the measured figure rather than a
tolerance being kept around the rounded one. That was free, and the reason `models.rs` gave for not
doing it was wrong:

> A tolerance rather than a rewrite of the registry, because the rounded figures are also what the
> settings screen shows.

**They are not.** `apps/desktop/src/views/SettingsTranscription.tsx` formats `sizeMb` — `142 MB`,
`466 MB` — and never reads `expectedSizeBytes` at all. The field has exactly one consumer in the
repository, `models::integrity_for`. `sizeMb` stays rounded, because it is a display estimate and
showing a user 77,691,713 helps nobody. The false sentence is corrected where it was written.

### What is kept although nothing reaches it

`Integrity::Size`, `Verification::SizeOnly`, `SIZE_TOLERANCE` and `within_tolerance` all survive
with no shipped model reaching them, and each says so where it is defined so nobody removes it as
dead code. The reason is the same one `98e836b` gave for keeping `SizeCheck::Approximate`: a model
added to the baseline before somebody spends the bandwidth to hash it — 2.9 GB, for a model the
size of `large` — should be **degraded, not broken**.
A tolerance is the right answer to a rounded number and the wrong answer to a measured one, and the
unmeasured model is the only case that still has a rounded number.

`within_tolerance`'s last consumer outside this module is `SizeCheck::Approximate` in
`apps/desktop/src-tauri/src/local_llm.rs`; that is named in the doc comment, because "who still
calls this" is the question a future reader will actually have.

Note the asymmetry with sherpa, which is deliberate: an unpinned sherpa model cannot be installed
at all (`MissingPin`), an unpinned whisper model degrades to a size check. A sherpa model is an
archive that gets extracted; a whisper model is one file from a repository whose five siblings were
hashed.

### What this costs, and it is the same cost twice

All six URLs are `resolve/main` — a branch pointer, not a revision. Until today a re-upload had to
move a file by more than 5% to be noticed; now any change at all fails the install with an
integrity error and the user cannot proceed. Identical to the trade `98e836b` took for the GGUF
sizes, taken again for the same reason: a silent pass means running weights nobody here measured.
See **KNEG-103**, which now covers a digest-exact surface rather than a size-exact one.

> **Superseded the same day.** KNEG-103 landed: all six URLs name commit `5359861c` instead of
> `main`, so this cost is no longer paid — a publisher pushing cannot move the bytes these digests
> describe. The entry above is what was true when the digests were measured, and the reasoning that
> made pinning the revision the obvious next thing. See the KNEG-103 entry at the top of this file.

What notices first is `node tools/model-pin/pin.mjs --verify`, and its weekly cost went from about
1 GB to about **8 GB** — two Parakeet archives plus six whisper models rather than one.
`.github/workflows/pin-verify.yml` carries the new figure and the `--only` escape hatch if the
runtime ever becomes the problem.

### Verified, and not

**Verified:** all six digests reproduced by a full download; `tiny` reproduced its previously
committed pin; a registry invariant test asserts every whisper model resolves to `Integrity::Pinned`
(mirroring `every_installable_sherpa_model_has_integrity_and_size_limits`); `cargo test -p
kw-transcription-core --test local_dictation -- --ignored` drives the real round trip — download,
`Verification::DigestMatched`, server start, correct transcript — through the shipped `install` path
now that it reads the registry instead of a table in its own file.

**Not verified:** nobody has installed any of the five newly pinned models through the application.
`BENCH_CHECK.md` carries that as a by-hand item and **KNEG-105** as an issue, with `base` named
because it is the recommended model and the one a new user presses first. The two
Parakeet digests were also re-measured today by full download (464.6 MiB and 478.1 MiB) and both
matched; that upgrades `SHERPA_PINS`' comment from "the byte counts agree with `content-length`" to
"both digests and both lengths came off the disk", and its comment now says what that does and does
not prove.

---

## 2026-08-31 (local-LLM sizes) — eighteen models nobody could install, and 2^30 ÷ 10^9

`download_gguf` deletes a finished download whose length is more than 5% from the size the registry
declares. Eighteen of the thirty-five local text-generation models were declared 6.7–7.9% larger
than the file Hugging Face actually serves, so **all eighteen downloaded in full, failed the check,
and were deleted** — 101 GB of successful transfer, thrown away on arrival, `Qwen3 32B` and
`GPT-OSS 20B` among them. There was no way to install any of them, and nothing in the failure said
so: the message blamed the download for being the wrong length, which reads like a bad connection
and invites a retry that fails identically.

### Where the numbers came from, measured

Every model and every speculative-decoding drafter — 39 objects, 202 GB — was HEADed at
`https://huggingface.co/{hfRepo}/resolve/main/{fileName}` on 2026-08-31 and its `x-linked-size`
read. Hugging Face serves these through LFS: a `resolve/main` request answers **302**, so
`content-length` describes the redirect and `x-linked-size` carries the object. Four were
cross-checked against `/api/models/{hfRepo}/tree/main`, which reports the same `lfs.size`. The
whole set was then re-measured a second time through the shipped code path — `NetworkPolicy` under
`UserTriggeredModelDownload` — and agreed on all 39.

Pass and fail below are recomputed with `within_tolerance`'s own arithmetic, which divides by
`expected`, not by `actual`. That matters: it is the difference between reporting what is wrong and
reporting what the shipped code rejects.

### The root cause is a base-2 conversion of a base-10 label

Upstream's baseline carries a human `size` label per model — `"5.0GB"`, `"2.5GB"` — and its
`sizeBytes` was produced from that label by multiplying by **2^30 instead of 10^9**.

| model | declared | measured | declared ÷ measured | declared ÷ 2^30 | measured ÷ 10^9 |
| --- | --- | --- | --- | --- | --- |
| `qwen3-8b-q4_k_m` | 5 402 263 552 | 5 027 783 488 | 1.074482 | 5.0313 | 5.0278 |
| `qwen3-8b-q5_k_m` | 6 281 625 600 | 5 851 112 224 | 1.073578 | 5.8502 | 5.8511 |
| `qwen3-4b-q4_k_m` | 2 684 354 560 | 2 497 280 256 | 1.074911 | 2.5000 | 2.4973 |
| `qwen3-1.7b-q8_0` | 1 965 555 712 | 1 834 426 016 | 1.071483 | 1.8306 | 1.8344 |
| `qwen3-32b-q4_k_m` | 21 260 251 955 | 19 762 149 024 | 1.075807 | 19.8002 | 19.7621 |
| `qwen2.5-1.5b-instruct-q5_k_m` | 1 395 864 371 | 1 285 494 304 | 1.085858 | 1.3000 | 1.2855 |
| `qwen2.5-3b-instruct-q5_k_m` | 2 620 055 552 | 2 438 740 384 | 1.074348 | 2.4401 | 2.4387 |
| `qwen2.5-7b-instruct-q4_k_m` | 5 025 128 858 | 4 683 074 240 | 1.073041 | 4.6800 | 4.6831 |
| `qwen2.5-7b-instruct-q5_k_m` | 5 841 530 470 | 5 444 831 936 | 1.072858 | 5.4403 | 5.4448 |
| `mistral-nemo-12b-instruct-q4_k_m` | 8 028 321 792 | 7 477 208 320 | 1.073706 | 7.4770 | 7.4772 |
| `mistral-7b-instruct-v0.3-q4_k_m` | 4 692 635 648 | 4 372 812 000 | 1.073139 | 4.3704 | 4.3728 |
| `mistral-7b-instruct-v0.3-q5_k_m` | 5 519 900 672 | 5 136 175 328 | 1.074710 | 5.1408 | 5.1362 |
| `llama-3.2-3b-instruct-q4_k_m` | 2 168 958 976 | 2 019 377 696 | 1.074073 | 2.0200 | 2.0194 |
| `llama-3.1-8b-instruct-q4_k_m` | 5 282 717 696 | 4 920 739 232 | 1.073562 | 4.9199 | 4.9207 |
| `gpt-oss-20b-mxfp4` | 12 999 763 968 | 12 109 566 624 | 1.073512 | 12.1070 | 12.1096 |
| `gemma-4-e4b-it-q4_k_m` | 5 812 137 984 | 5 405 170 144 | 1.075292 | 5.4130 | 5.4052 |
| `gemma-4-e2b-it-q4_k_m` | 3 711 514 624 | 3 462 680 032 | 1.071862 | 3.4566 | 3.4627 |
| `gemma-3-12b-it-q4_k_m` | 7 847 624 704 | 7 300 575 264 | 1.074932 | 7.3087 | 7.3006 |

**2^30 ÷ 10^9 = 1.073741824.** Over the eighteen, declared ÷ measured has mean **1.074509**. Drop
the single outlier and the seventeen remaining average **1.073841**, none of them more than
**0.210%** from the constant. Read the last two columns as one claim: *declared ÷ 2^30* recovers the
model's size **in base-10 GB**, to three or four significant figures, in every row.

Two rows are the label itself, unrounded:

- `qwen3-4b-q4_k_m` — label `"2.5GB"`, declared `2 684 354 560` = **2.5 × 2^30**, exactly.
- `qwen2.5-1.5b-instruct-q5_k_m` — label `"1.3GB"`, declared `1 395 864 371` = **1.3 × 2^30**,
  exactly. This is the outlier at 7.907%, and it is worse than the rest for a second reason on top
  of the first: the file is 1.2855 GB, so the two-significant-figure label was already 1.1% high
  before anything multiplied it. Label rounding and unit confusion compound.

The other sixteen are finer-grained than their own two-digit label — `qwen3-8b-q4_k_m` declares
5.0313 GiB against a `"5.0GB"` label — so whatever the conversion consumed was a more precise GB
figure than the label the baseline kept. The residual ±0.2% is not explained; the 7.4% is, and the
7.4% is the whole defect.

### Three things the measurement found that the theory did not predict

**1. `llama-3.2-1b-instruct-q4_k_m` is the same bug one unit down, and it passes.** Declared
`847 249 408` = **808 × 2^20**, exactly; the file is `807 694 464` bytes = 807.69 MB, which rounds
to the 808 **MB** that was then read as MiB. Its ratio is 1.048973 against 2^20 ÷ 10^6 =
**1.048576**. It survives only because the mebibyte error is 4.86% and the gibibyte error is 7.37%,
and the tolerance is 5%. One model was saved by its own smallness.

**2. The three `qwen3.5-*` models are not this bug at all, and one of them passes by 5.6 KB.** Their
declared sizes are *smaller* than the real files (95.2–95.5% of them) and are not round in any base
— `5 889 811 552` for a 6 169 341 984-byte file. Each bartowski repo has a single upload
(2026-05-19, checked through `/api/models/…/commits/main`), so these are not stale figures from a
re-upload; where they came from is unknown. What matters is the margin:

| model | difference ÷ expected | margin below the 5% tolerance |
| --- | --- | --- |
| `qwen3.5-2b-q4_k_m` | **4.9958%** | 0.0042 pp — about 5.6 KB |
| `qwen3.5-4b-q4_k_m` | 4.9198% | 0.08 pp |
| `qwen3.5-9b-q4_k_m` | 4.7460% | 0.25 pp |

`qwen3.5-9b-q4_k_m` is the **recommended** model, and `qwen3.5-2b-q4_k_m` was four kilobytes of
upstream metadata away from joining the eighteen. The set that worked was not a set that was right;
it was a set that had not yet drifted far enough to fail.

**3. Two models that looked exact were 2 240 bytes short — the same 2 240 in both.**
`gemma-4-31b-it-q4_k_m` and `gemma-4-26b-a4b-it-q4_k_m`, both from bartowski, both declared exactly
2 240 bytes below what is served. That is the re-upload case, live and already present: two files
whose length changed by a GGUF metadata block since somebody measured them. It is the reason the
`#[ignore]`d re-measurement test below is not a formality.

**Eleven** models matched to the byte, and all four drafters — fifteen of the thirty-nine objects.
They are the ones somebody actually measured: all five `lfm2*`, the four Gemma QAT builds with their
drafters, `gemma-3-4b-it` and `gemma-3-1b-it`. Six of the seventeen that *passed* did not match: the
three `qwen3.5-*`, `llama-3.2-1b`, and the two bartowski Gemma 4 builds 2 240 bytes adrift.

### The fix, and where the numbers now live

`LOCAL_LLM_SIZES` in `tools/integration-audit/generate-provider-registry.mjs`, keyed by model id,
carrying `sizeBytes` and `draftSizeBytes` for all 35 models and all 4 drafters. It sits beside
`SHERPA_PINS` and is built on the same reasoning: `config/upstream-integration-baseline.json` is a
frozen record of what upstream had and must not gain fields we invented, and
`config/provider-registry.json` is generated, so a fix hand-edited into it dies at the next
regeneration — which is exactly what happened to the Parakeet digests in `f17a5fd`. A pin naming an
id the baseline does not have is a `die()`, as is a drafter pin on a model with no drafter.

**The pins are measurements, and they are the weaker kind.** `tools/sidecar-pin/pin.mjs` downloads
the asset and records bytes on disk, which is why it can also pin a SHA-256. These are what the
publisher's own object size says over HTTP. The difference is deliberate and it is a cost decision:
202 GB of downloading to confirm a length that Hugging Face already states is not justified for a
size pin. The doc comment says so in those words, so that nobody later reads the table as the
stronger thing and adds a `sha256` next to it. A digest belongs to bytes somebody hashed.

### Exact equality, and what it costs

The registry now carries `sizeMeasured` per model and per drafter, and `download_gguf` reads it
through a three-state `SizeCheck`: `Exact` for a measured size, `Approximate` (the old 5%) for a
declared one, `Unknown` for no size at all.

**Why exactness became safe:** a tolerance is the right answer to a *rounded* number and the wrong
answer to a *measured* one. Against a figure read from the publisher's own object size there is
nothing left to round — the file either has that many bytes or is not that file. The 5% window was
simultaneously wide enough to hide this defect for the seventeen models it passed and too narrow to
install the eighteen it failed, which is what a tolerance around an unmeasured number always is.

**What it costs:** an upstream re-upload now fails the install loudly instead of passing silently.
Taken deliberately — a silent pass means the user runs weights nobody here ever measured — and it
is not a hypothetical cost, because two models had already drifted by 2 240 bytes. What keeps it
survivable is `the_measured_sizes_still_match_hugging_face` in `local_llm.rs`: `#[ignore]`d, 39 HEAD
requests, no bodies, about six seconds, and it re-measures the committed table against the live
files. It passed on all 39 at the time of writing. The `Approximate` arm is kept, and a unit test
asserts nothing is currently using it — a model added before it is measured is degraded rather than
broken, and the test is where that shows up.

### Not verified

Nobody has installed any of the eighteen through the screen. `BENCH_CHECK.md` carries it as a
by-hand item. What is verified is that the sizes are right and that the check accepts them; that a
20 GB download completes, renames, and starts a server is a separate claim and this run did not make
it.

---

## 2026-08-31 (KNEG-96) — the half of the self-heal that was missing

`d489471` taught `LocalTranscription::start_model` to health-check the port it had recorded and
start a new server when nothing answered. That was the right fix for the symptom in front of it —
pressing *Use for dictation* handed back a dead port — and it left the two paths that actually
carry audio untouched. `RouteTranscriber::local_server` and `MediaImportRuntime::local_server` were
the same three lines: read the recorded port, build a `WhisperServer`, send. Neither asked whether
anything was listening.

So the state that produced four failed dictations on 2026-08-28 was half cured. The button
recovered; **dictating did not, and neither did importing** — the worse of the two, because a
dictation whose server has died can be spoken again and an imported recording generally cannot.
Both now health-check the recorded port and start the remembered model again when it has gone.

### `Revive` is a trait, and the choice is the persisted one

- **A trait rather than an `Arc<LocalTranscription>`.** The two callers want one sentence's worth of
  capability — *start the remembered model, tell me the port* — and handing them the runtime would
  hand them the installer, the reaper and the catalogue as well.
- **Required, not `Option`.** A build that forgets to supply it is a build where a lost server
  silently stays lost, which is the defect. Requiring the field makes forgetting a compile error —
  the lesson from `key_ref: None`, which flipped a manifest entry to reachable for a client that
  could not connect.
- **The *persisted* model, not the in-memory one.** By the time anyone revives, `forget_port` has
  cleared the model alongside the port; they go together on purpose, since a model name with no
  server behind it is the claim that caused this. The persisted choice is also what startup reads,
  so reviving mid-dictation cannot bring back a different model than a relaunch would.
- **Three named states instead of `Option<u16>`.** `Recorded::{Absent, Live, Gone}`, produced by a
  pure `classify(recorded, answering)` so the reading can be tested without a server — the half that
  was wrong. "No server was ever started" and "a server was started and has gone" had been getting
  the same answer: the recorded number, handed to a transcription that then failed against it.

### A second defect, found by the fix rather than by a bench run

The local engine was being resolved on **every** dictation, before anything looked at the transport,
and the executor consults it only in its `Transport::Local` arm. Harmless-looking, and it meant a
sherpa model that was selected but could not be built failed a dictation **bound for a cloud
provider** — `local_engine` returns `Err` for that case and the `?` ran first. Resolving the engine
only for a local route fixes it and removes a loopback health check from every cloud dictation at
the same time. No test caught this because no test paired a broken local selection with a remote
transport.

### Written, not verified

The decision is covered by five new tests (1310 passing). The round trip is not: nothing here has
killed a real `whisper-server` and dictated. Listed as a by-hand item in `BENCH_CHECK.md`, and the
row that used to read *a dead local server now repairs itself* has been narrowed to the link the
2026-08-28 run actually exercised — the button.

**And the underlying question is still open.** Why the child exits unattended is not answered by
any of this; it has now happened three times. KNEG-96 carries the two candidates and how to tell
them apart from the first two log lines after a restart.

## 2026-08-27 (merged security fixes) — six fixes, a stronger merge, and a broken gate

Six fixes landed on `dev` through PRs #7–#14, authored outside this session, after the plan closed:
a Corti environment forced to one DNS label, a Vertex region forced to one DNS label with the
service account's `token_uri` pinned rather than trusted from pasted JSON, Microsoft Graph
pagination URLs constrained and bearer requests pinned, Deepgram transcript memory bounded, and URL
import disabled. Three things about that are worth keeping.

**The Microsoft merge produced a better check than either branch wrote.** PRs #10 and #11 both
edited `microsoft.rs` — one constraining pagination URLs, one pinning bearer requests — and the
merge resolved to a single `validate_graph_url(url, expected_path)` that pins scheme, host, port,
credentials, fragment *and* the exact path. That is strictly stronger than either branch alone. It
also orphaned `use url::Url` and `const GRAPH_HOST`, which belonged to the version that lost. Both
were dropped rather than wired back in: restoring them would have meant restoring the weaker check
beside the stronger one.

**The merged branch did not pass `cargo clippy --workspace --all-targets -- -D warnings`.** Fourteen
errors: the two orphans above, one `field_reassign_with_default` in `deepgram_socket.rs`, and eleven
dead-code errors in `media.rs`. None is a defect in the fixes; all are the residue of removing a
call site without removing what only that call site used. This is the first concrete cost of the
CI triggers being commented out — every one of the fourteen would have been caught before merge.
The pause is deliberate and still stands; the cost is now measured rather than hypothetical. See
"2026-08-25 (CI paused)" below.

**Dormant, not deleted, and said so in the type system.** Withdrawing `media_import_url` as a Tauri
command made the entire URL-import path unreachable: two IPC enum variants, two error variants,
three constants, four functions. The manifest note says the backend is dormant, so deleting it
would have contradicted the record — and re-opening the entry point would then be a rewrite rather
than a decision. Each item carries `#[allow(dead_code)]` pointing at a new `URL IMPORT, DORMANT`
section in `media.rs`'s module doc, which states why the entry point is closed and what re-opening
requires: every address yt-dlp resolves, every redirect it follows, and every request its
extractors generate checked against the policy. Their tests still run. The alternative considered
and rejected was a blanket `#![allow(dead_code)]` on the module, which would have hidden genuinely
abandoned code alongside the deliberately dormant kind.

**What the withdrawal costs, stated plainly:** `media:url-import` goes `reachable: false`, parity
goes 7 unreachable to 8, and this is the first entry on that list that regressed rather than never
having been built. Audio and video import are untouched — they take a local path and fetch nothing.
Task 20's own finding pointed here: the up-front literal-host check was never the weak part, and no
amount of strengthening it reaches DNS rebinding or an extractor's own requests.

## 2026-08-27 (Task 19) — reconciling the record, and the ACTIVE-status audit

The last task in the plan, and the only one whose entire job was making claims match reality
rather than building anything. `docs/BUILD_PLAN.md` opens with its own rule — "when something here
becomes false, edit it rather than appending a note" — and this entry, plus the edits it describes,
is that rule applied to the whole run's paper trail: `BUILD_PLAN.md`, `INTEGRATION_PARITY.md`,
`COMPLETION_PLAN.md`, `PLATFORM_SUPPORT.md`, `PORTING_MATRIX.md`, `BENCH_CHECK.md`, this file, and
several source doc comments and manifest fields.

### The ACTIVE-status audit

`verify-parity.mjs` defines `ACTIVE` as "statuses that assert a working implementation exists" —
`ported`, `ported-with-new-rust-implementation`, `rewired-to-direct-byok`,
`replaced-with-local-equivalent`, `platform-specific`. Mid-run, a reviewer found
`meeting-stream:tinfoil-realtime` carrying `status: "ported"` while no Tinfoil meeting client exists
anywhere in the codebase — the identical pattern `platform:aec` had just been corrected for — and
the controller ruled that Task 19 would audit every `ACTIVE` entry rather than patch that one
instance by hand.

The audit read all 80 entries whose status was in `ACTIVE` at the start of this task (three text
passes over the whole manifest for language like "implements none", "declined", "refused",
"ClientNotImplemented", plus a manual check of every ambiguous case) and found three overclaims,
including the one already flagged:

| Entry | Was | Reality | Now |
| --- | --- | --- | --- |
| `meeting-stream:tinfoil-realtime` | `ported`, backend `crates/meeting-core` (a directory) | No Tinfoil meeting client exists; `capabilities.rs` declares only the refusal reason | `temporarily-deferred` |
| `transcription:tinfoil` | `rewired-to-direct-byok` | `clients::shape()` returns `TranscriptionError::ClientNotImplemented` for it; nothing else implements it | `temporarily-deferred` |
| `calendar:apple` | `platform-specific` | `kw_calendar_core::apple`'s own module doc: "deliberately unimplemented by anything in this crate" on every platform including macOS | `temporarily-deferred` |

Two entries that looked like plausible false positives on a text scan turned out not to be, and are
recorded here so the audit's negative results are visible too: `meeting-detect:facetime` is real,
`#[cfg(target_os = "macos")]`-gated detection code that runs and is tested when compiled for macOS —
platform-specific in the sense the status actually means, unlike `calendar:apple`'s "described,
implemented nowhere on any platform". `reasoning:tinfoil` (as opposed to the two entries above) has
a genuine OpenAI-compatible client in `reasoning-core` and is reachable; only the two entries whose
proposition requires an attested socket or an unattested-POST refusal are declined.

**A fourth, adjacent overclaim outside the `ACTIVE` set entirely.** `developer:agent-skills` (`kind:
"documentation"`) carried `status: "ported"` and `target: "docs/agent-skills/"` — a directory that
does not exist anywhere in this repository, and neither do any rewritten `SKILL.md` files.
`verify-parity.mjs`'s `incomplete-documentation` rule checks only that a documentation entry *names*
a target, never that the path exists on disk, so nothing caught this, and the entry sat outside the
`ACTIVE` audit too: a `kind: "documentation"` entry `continue`s past the `ACTIVE` check entirely,
regardless of its status. Same defect, one step earlier than the pattern this whole audit was
looking for — a claim that specific work was done, when it was not. Not fixed by writing the missing
docs, which this task's mandate does not extend to and which would risk producing inaccurate
documentation under time pressure; corrected to `temporarily-deferred` with a `deferral` block
naming what is actually needed (someone to write the rewrite) and no false claim in the meantime.

**The vocabulary gap.** `calendar:apple`'s move to `temporarily-deferred` fits the term's existing
meaning exactly — the same shape as `platform:aec`: no macOS machine to write or verify an EventKit
binding against, a resource gap with named dependencies. The two Tinfoil entries do not fit it at
all: both are refused on principle, permanently, not blocked on a toolchain or a decision anyone
expects to reverse. This manifest's `statusVocabulary` has no "declined on principle" term. Rather
than invent one silently — the brief was explicit that this is not an option — each entry's
`deferral.why` says plainly that `temporarily-deferred` is a compromise chosen only because it is the
closest status that stops asserting an implementation exists, and that "temporarily" should not be
read literally. Both entries needed a `deferral` block to satisfy the gate's `incomplete-deferral`
rule for the status, which they did not have before (nothing required one under their old, wrong
status).

Measured effect on the gate's own summary, all four corrections together: `ported-with-new-rust-implementation`
52 (unchanged), `rewired-to-direct-byok` 22 → 21, `removed-vendor-service` 18 (unchanged), `ported`
4 → 2, `temporarily-deferred` 4 → 8, `platform-specific` 2 → 1. Total still 102; the 7-unreachable count is
unaffected, because all three entries already carried `reachable: false` — this audit was entirely
about the machine-readable `status` field lying underneath an honest `reachable` claim, which is the
subtler half of the same defect class: a reader who checks only `reachable` was already told the
truth, but a reader (or a future tool) that trusts `ACTIVE` status as "implementation exists" was not.

### Smaller corrections, same audit pass

- **`media:{audio,video,url}-import`'s `backend` said `"crates/notes-core"`**, which holds no import
  code. The real implementation is `MediaImportRuntime` in `apps/desktop/src-tauri/src/media.rs`.
  Same class as `platform:aec`'s corrected `backend` — a location that was simply wrong, independent
  of the status question. Fixed with a note recording the correction rather than a silent edit.
- **The manifest mixed `"note"` and `"notes"` field names** — `meeting-stream:openai-realtime` and
  `meeting-stream:local` used the plural, every other of the 100 entries used the singular. Renamed
  to `note`; nothing in `tools/integration-audit/*.mjs` reads either field name specifically, so this
  was a pure rename with no behaviour to preserve.
- **`docs/NETWORK_BOUNDARY.md`'s "SSRF guards for URL import" section and `docs/SECURITY.md`'s
  matching sentence both said URL import does "DNS resolution followed by rejection of private and
  link-local addresses".** `media.rs` resolves no names at all — it does a literal-host check before
  yt-dlp ever runs, and yt-dlp's own fetch (DNS included) is unobserved. That planned-but-not-built
  description was flagged during Task 20's review and left for this task, since by then
  `NETWORK_BOUNDARY.md` already had a correct "What none of this stops: DNS rebinding" section
  answering the identical question the other way. Both rewritten to describe the real mechanism and
  point at each other rather than disagree.
- **`crates/network-policy/src/lib.rs`'s module doc claimed "No other crate declares `reqwest` as a
  dependency".** False before this run (`calendar-core`, `local-api`, `reasoning-core`, `sidecar`,
  `transcription-core`, `provider-probe` and both `tools/knegiswhisper*` binaries all did, to chain
  onto the `RequestBuilder` `NetworkPolicy::{post,get,loopback}` return) and stayed false throughout
  it. Corrected to the accurate, narrower claim — this crate owns the only `Client` any part of the
  desktop app's own egress path *constructs* — with the two real exceptions named: two test-only
  bare clients confined to their own test binaries, and `tools/knegiswhisper`'s CLI client, which
  talks to the local API over loopback and is therefore not egress under this project's own "loopback
  is not egress" rule.
- **`docs/BENCH_CHECK.md` item 2 for the updater told a person to paste a public key into
  `tauri.conf.json` and press the button again.** `plugins.updater.pubkey` is compiled into the
  binary by Tauri's `generate_context!`, so that instruction cannot work without a rebuild in
  between. A by-hand check that cannot be followed as written is worse than most defects, since it
  is the one document in this project meant to be executed literally by a person at a machine.
  Corrected to require the rebuild explicitly.
- **`docs/PLATFORM_SUPPORT.md`'s Calendars table said Apple was "EventKit — unverified"**, the same
  word used for Google and Microsoft's genuinely-written-but-uncalled code. "Unverified" undersells
  the actual gap — nothing is implemented, on any platform — so the row and a short note were
  corrected to say so plainly, distinguishing it from the two rows that really are only unverified.
- **Stale narrative counts**, none adjusted by arithmetic — each recomputed from the file it
  describes: `docs/BUILD_PLAN.md`'s "45 of 99 integrations" and "5 of 13 UI surfaces do not exist",
  `docs/INTEGRATION_PARITY.md`'s matching numbers and backlog table, and
  `apps/desktop/src/views/SettingsProviders.tsx`'s doc comment ("fifteen slots, ten providers and
  five per-feature custom endpoints" — the registry now has seventeen: twelve providers, after
  `credential:assemblyai` and `credential:deepgram` were added, and the same five custom endpoints).

### What was found and recorded, not fixed

- **The flaky `kw-storage` test** the Task 8 implementer reported (failed once, passed on re-run;
  every other full-suite run this session was clean) — recorded as unresolved rather than hunted, per
  the brief's own instruction. A test that lies in either direction is a live risk to a project whose
  whole verification story rests on the gates meaning something, and it deserves a session of its
  own rather than a guess appended here.
- **`apps/desktop/src-tauri/src/meeting.rs` at roughly 1600 lines**, noted at least three times during
  this run (Tasks 1, 2, and again here) and never restructured, because every task that touched it was
  explicitly told not to. Recorded as a known shape rather than a defect: it grew for a real reason
  (three cloud clients, keyterm/language plumbing, and the meeting-notification window all needed
  something to hold their commands), and restructuring it was out of scope for every task that could
  have.
- **`SherpaModel` in the provider registry carries no SHA-256** for its model archives, unlike
  `WhisperModel`'s size check and unlike every sidecar binary's mandatory `sha256` in
  `config/sidecars.json`. Task 18's layout-verification check (does the extracted archive actually
  contain the four transducer files it should) is a real and reasonable mitigation for a *truncated*
  download, but a corrupted-but-layout-compatible archive from a compromised CDN would still install
  silently. Flagged by that task's reviewer as "worth a future ticket"; there is no future task in
  this plan to give it to, so it is recorded here instead.

---

## 2026-08-26/27 (Task 20) — the network boundary does not stop SSRF over HTTPS

The most significant finding of the whole run, and it did not come from a planned task — it came
from an implementer flagging rather than patching. Task 11 (media import) noticed that
`kw_network_policy::validate`'s `https` arm was `"https" => Ok(parsed)`, with **no host check at
all**, for any purpose. Verified directly: `https://127.0.0.1/`, `https://169.254.169.254/latest/
meta-data/`, `https://[::1]/` and `https://10.0.0.5/` were permitted under every one of the seven
purposes, and because `next_hop` re-validates each redirect through that same `validate`, a redirect
to a link-local or loopback address over HTTPS was *followed*, not refused. `validate_websocket`'s
`wss` arm had the identical gap, and its own doc comment claimed the rules were shared with `https` —
so once one side was fixed, leaving the other would have made that doc comment newly false rather
than merely incomplete.

An earlier brief in this run had asserted "`NetworkPolicy` already re-validates every redirect hop —
that is the SSRF guard" for URL import. Half true: it re-validates, and the validation it re-runs did
not reject a private target sent over TLS. A valid certificate says nothing about whether a host is
on the public internet or on loopback, and the code was treating "https" as proof of the latter.

**Fixed centrally, as its own task, in a worktree.** `network-policy` is a leaf crate nearly
everything in the workspace depends on, so a half-written change to it would have broken every other
task's build if made in the main tree, and the fix's whole value depended on auditing every existing
call site for a newly-refused legitimate endpoint — the brief named finding such a conflict a
*valuable* outcome, not an obstacle, and none was found. The policy already existed in the file:
`allows_plaintext_private()` already restricted private-space access to
`ExplicitlyConfiguredSelfHostedEndpoint`. The gap was in enforcement, not in policy — the `https` and
`wss` arms simply never consulted it.

### The review found a Critical the implementation had not

A trailing dot defeats the whole predicate: `https://localhost./` has host `"localhost."`, which
fails every string rule in `is_private_host` — not equal to `"localhost"`, not ending in `.local`,
not a valid IP literal, and still containing a `.` so the single-label fallback never fires either —
and `getaddrinfo` on this machine genuinely resolves `localhost.` to `127.0.0.1`. The identical miss
existed in the vendor blocklist (`openwhispr.com.` matched neither the exact nor the suffix branch of
`is_blocked`), sitting under a test *named* `case_and_trailing_dot_do_not_evade_the_blocklist` whose
body tested only the case variants — a test whose name claimed coverage its body did not have, the
same defect class this project keeps finding in itself, one level up, on top of a "no vendor
dependency" constraint. Both fixed by `trim_end_matches('.')` upstream of every comparison in both
functions, closing the whole `localhost..` class rather than one instance.

### The completeness bar rose the moment this landed — fail-closed became fail-open

Before the fix, a false negative in `is_private_host` meant a plaintext request to that address was
*refused* — the plaintext arm only allows what the predicate says is private, so a miss loses
permission rather than gaining it. After the fix, the identical false negative means an HTTPS or WSS
request to that address is *allowed*, because the private check is now what refuses it there. The
function did not change what it is for; what changed is that a gap in it now bypasses a control
rather than only breaking a convenience — which is why the trailing-dot miss, harmless in the old
fail-closed world, was a Critical in the new one, and why every remaining gap in `is_private_host`
now matters more than it used to.

### Widened deliberately, in the same fix: CGNAT and the rest of `0.0.0.0/8`

`100.64.0.0/10` (carrier-grade NAT's shared address space, a standard SSRF target) and the rest of
`0.0.0.0/8` were added to the private set. Both cost nothing real to refuse, since the self-hosted
purpose stays exempt for whichever endpoint someone deliberately points at one — the same reasoning
that keeps the private-range exemption itself narrow to one purpose. This widening cuts both ways:
`http://100.64.0.1/` and `http://0.1.2.3/`, previously refused for every purpose including the
self-hosted one, became reachable in the clear for that one purpose exactly as the rest of the
private range already was — recorded plainly in `NETWORK_BOUNDARY.md` rather than only the
restricting half of the change.

### What was found and deliberately not closed

- **`::ffff:0:a.b.c.d`** (the IPv4-translated form, RFC 8215) still classifies as public — the SIIT
  source prefix is unrouted on an ordinary host and a `connect()` to it fails rather than reaching
  loopback, unlike `::ffff:a.b.c.d`, which a dual-stack kernel genuinely delivers.
- **NAT64 (`64:ff9b::/96`) and 6to4 (`2002::/16`)**, raised unprompted by the re-reviewer as *more*
  common in real networks than the form above. Same reasoning, same acceptance, recorded together
  deliberately — if this family of embeddings is ever closed, it should be closed as a family, by
  auditing every RFC-defined way to fold an IPv4 address into 128 bits, rather than one prefix at a
  time as each is separately noticed.
- **`blocked_hosts()` lowercases configured vendor hosts but does not strip trailing dots from
  them**, an asymmetry with `is_blocked`'s now-normalised input. No live consumer — every entry in
  `config/forbidden-domains.json` is written by this project without a trailing dot.
- **DNS rebinding is out of scope, deliberately.** The check runs against the literal host in the URL
  at validation time, before any connection is attempted; it has no view of DNS. Closing it needs a
  check against the address actually dialled, at connect time, which is a materially different and
  harder problem than the ones this fix addresses, and was explicitly not attempted.

All three parked items are recorded in `NETWORK_BOUNDARY.md`'s "Known, accepted residue" section as
decisions, not oversights — this project's convention throughout the run for a gap it chooses not to
close: name it, say why, and say what would have to change for that answer to change.

`Task 11`'s companion finding — `is_private_host`'s `V6` arm never called `to_ipv4_mapped()`, so
`https://[::ffff:127.0.0.1]/` read as an ordinary public address — was folded into this same fix
rather than given its own round, since both agents were editing the identical function and Task 11's
own local guard delegates to it.

---

## 2026-08-26 (chunks E–J) — the reachability rule, stated once and applied four times

`reachable: true` means a user can *configure* the integration through a named, **built** surface —
not that the underlying code runs correctly, and not that a probe proved the runtime works. This was
settled early (`transcription:tinfoil`, declined rather than pending) but re-litigated, unprompted,
four separate times across the second half of this run — each time because a controller brief's own
arithmetic or premise had quietly collapsed "the code works" into "a person can reach it", and each
time an implementer caught it by flagging the discrepancy rather than complying with the wrong
instruction.

**1. `calendar:apple` (Task 8).** The brief's own count assumed flipping it to `reachable: true`
would take parity from 14 to 11. The implementer did as instructed, then flagged that the flip was
"semantically strained since nothing is implemented" rather than silently accepting the number.
Overruled: `calendar:apple` → `reachable: false`, and the real count is 12, not 11. Nothing about
Apple Calendar is configurable — the entry's own note admits `AppleCalendarSource` "implements none
of it, on any platform" — and naming the reason honestly on screen (which `Integrations > Calendar`
does) is not the same claim as reachability. The precedent this drew on was already in the manifest:
`transcription:tinfoil` lists its refusal on screen and stays `reachable: false` too.

**2. The two sherpa Parakeet models (Task 18).** The probe measured a real runtime — the pinned
`sherpa-onnx-offline` CLI transcribing both pinned Parakeet archives to the same words whisper's
`tiny` model produces — and a first pass flipped both to `reachable: true` on the strength of that
result. Caught before the chunk closed: `SettingsTranscription.tsx` told the user directly "the
sherpa-onnx sidecar, which is not built yet" while the manifest claimed the opposite, and the two
statements contradicted each other because both were wrong in different directions. No Tauri command
or Settings selector exists for choosing a Parakeet model, so both entries reverted to
`reachable: false`, on a narrower and more honest ground than before the measurement: a proven
runtime with a specifically named missing piece, not "blocked on sherpa" as a whole.

**3. Task 11's SSRF framing.** Not an arithmetic error but the same species of premise error: a
brief asserted "`NetworkPolicy` already re-validates every redirect hop — that is the SSRF guard"
for URL import. The implementer's own module doc in `media.rs` states plainly that `import_url` hands
the URL straight to `yt-dlp` as a subprocess and never calls `download_to_file` in production, so
that re-validation mechanism — real, and proven for the purposes that do use it — was never on the
real fetch path for URL import at all. This is the finding that led directly to Task 20.

**4. Task 12's internal contradiction (the fourth instance).** The brief's own text disagreed with
itself — "11 → 8" in one line, "10 right now" in another — and the implementer flagged the
inconsistency rather than reconciling it silently in either direction, then measured the real number
independently with `git stash` against the pre-change tree: parity moved from **10 to 7**, not 11 to
8.

The pattern across all four: an implementer measuring or reading the actual state, rather than
trusting a controller's stated arithmetic, is what kept the record honest each time. None of the four
corrections changed what got built — only what the manifest was allowed to claim about it.

---

## 2026-08-26 (bench check) — three findings, and a probe that had verified less than it looked like

Ran everything that can be checked without a person at the keyboard: the two chunk-D probes again, a
new credential probe, the CLI against the live application, injected chords driving real dictations,
and the resumed whisper server handed a synthesised WAV. Full record in
[BENCH_CHECK.md](BENCH_CHECK.md); this is the reasoning.

### The hook probe had verified less than it appeared to

`keyhook_probe` reported *installed, fired exactly twice for two injected presses, uninstalled
cleanly*. Every word true, and it was offered — by me, in a commit message — as evidence that
modifier-only hotkeys worked. They did not: the callback emitted the event and returned, so
`Control+Super` announced itself in the interface and started no dictation.

The lesson is not "the probe was wrong". It is that a probe verifies the link it exercised, and
naming that link is part of reporting the result. `BENCH_CHECK.md` now opens with that rule and each
row says which link it covers, because the alternative is a table of green cells that quietly implies
the ones between them.

Injecting `Control+Alt` this time started a real microphone capture **66 ms** after the chord, and
four presses drove four alternations. That is the link that had been missing.

### Finding: sided modifiers are built and unreachable (closed 2026-08-31, KNEG-35)

`keys.rs` expresses `RightControl+Super` — the headline capability of having a low-level hook at
all, since the cross-platform API cannot see which side a modifier is on. `PLATFORM_SUPPORT.md` said
"Built with the same hook". Both true, and there is no accelerator a user can type that reaches it:

```
     RightControl+Super -> Standard { accelerator: "RightControl+Super" }
RightControl+RightShift -> Invalid { reason: "it names 2 non-modifier keys; a shortcut may have one" }
```

`hotkey::MODIFIERS` has nine entries and no sided spellings, so `classify` reads `RightControl` as
*the key*. The first goes to the system shortcut register, which is precisely the mechanism that
cannot distinguish sides. The second is refused with a message blaming the user's spelling for our
own missing alias — the third time that exact shape has appeared in this project, after the `Ctrl`
alias and the four dead hotkeys.

Not fixed here. It is one entry in `MODIFIERS`-adjacent parsing plus a decision about what the
settings field should accept, and it belongs with whoever next touches the hotkey editor.

### Finding: the local API token file has no permissions of its own on Windows

`discovery::restrict_file` under `cfg(not(unix))` does nothing and returns
`Permissions::InheritedFromProfile`, with a comment saying the file "inherits the profile
directory's ACL, which normally grants this user and the administrators group". `icacls` on this
machine disagrees: every entry inherited, and two of them a local group and an unresolved SID with
Modify — neither the user nor the administrators.

My first reading was that the code was honest and only the documentation overclaimed. That was
wrong, and reading `Permissions` rather than just `restrict_file` is what showed it. Three places
state what the permissions are and not one of them looked:

- The code comment reasons about what an inherited ACL "normally" grants.
- `Permissions::is_as_expected` returns **`true`** for `InheritedFromProfile`, under a doc comment
  defining that as "as restrictive as the platform allows". Windows allows an explicit owner-only
  DACL, so it is not. `Integrations > Local API` warns only when that flag is `false`, so it stays
  silent — under the sentence *"readable by your user account and by administrators, which is what
  your profile directory allows"*, naming two trustees that are neither.
- `docs/SECURITY.md` said the file "is written with owner-only permissions (`0600`)", flatly, for
  every platform. A security document asserting a control that does not exist on the platform the
  application runs on is worse than saying nothing, because it stops anyone looking. Corrected in
  place, per platform, with the measurement.

This is the `contentProtected` pattern with a different name: a control that is not applied,
described by a string that assumes what applying it would have achieved. The fix is the discipline
the Unix branch of this very function already uses — set it, read it back, report what came back —
and until that lands `is_as_expected` should be returning `false` here.

Not fixed here either. An owner-only DACL through `SetNamedSecurityInfo` plus a read-back is real
`windows-rs` work, and it should land with the read-back rather than without it.

### Finding: the CLI hides failed dictations

`history list` renders `id, when, provider, text`. A failed dictation has neither a provider nor
text, so it prints two dashes and says nothing — while `/v1/transcriptions/list` returns
`"status": "failed"` and a precise `errorMessage` that the dashboard does show. The API had it, the
renderer dropped it, and phase 6 is marked verified.

I nearly logged a fourth finding here: my first query used
`coalesce(text, error_message, '')` and `text` is `''` rather than `NULL` for a failed row, so it
looked as though nothing had been stored. The row was fine; the query was wrong. Worth recording
because the check that saved me was reading the column list rather than believing my own output.

### Judgement calls

#### A credential probe, added because the only implementation in use had no test

Every test of `CredentialStore` runs against `MemoryCredentialStore` — correctly, since a test suite
must not write to a developer's keychain. The consequence went unnoticed: `OsCredentialStore`, the
one the application actually uses, was covered by nothing at all.

`crates/security/examples/credential_probe.rs` closes it, in the shape the two chunk-D probes
established. It uses a slot the provider registry does not declare, stores a value that is not a
credential, **compares rather than prints** — a probe that echoes what it stored would echo a real
credential the day somebody pointed it at one — and checks the distinction the application depends
on: a missing credential must report `NotFound`, not `NoBackend`, or "you have not entered a key"
becomes "this machine has no keychain".

It passes. Windows Credential Manager works here.

#### Set `transcription.localModel` directly rather than waiting for a button

The resume path (`9f0f29d`, written the day before) reads that setting, and the setting was absent
because nobody had pressed **Use for dictation** since the feature existed. Writing `"tiny"` into it
is exactly what `local_server_start` writes. What is under test is the code that reads the setting,
and how the setting arrived does not change that.

It worked, first try: `sidecar ready sidecar=whisper port=8178 pid=7788` then `resumed the local
model server`. Left in place, because it is what makes dictation work at the next launch.

#### Handed the whisper server a synthesised WAV when the microphone leg would not cooperate

The speech gate refused synthesised speech played through the speakers
(`reason=InsufficientSpeech duration_ms=4850`). Almost certainly the AMD microphone array cancelling
its own speakers, which is what echo cancellation is for — so it is evidence about the room, not
about the gate.

Rather than conclude anything from that, I posted the same synthesised speech to
`127.0.0.1:8178/inference` directly: `{"text":" The quick brown fox jumps over the lazy dog.\n"}`.
That verifies the resumed server and leaves exactly one leg unverified — microphone into the gate —
which is now the first item on the list for a person. Reporting one unverified leg is worth more
than a green tick over a path I could not actually drive.

### Observed and not fixed

- **`Control+Alt` is AltGr.** On a Nordic or German layout Windows generates left-Control plus
  right-Alt for `@ $ € { [`, and `EitherSide` matches both. Injecting exactly that started a
  dictation. The chord fires on the second modifier going *down*, so the rule that an ordinary key
  disqualifies a modifier-only chord cannot save it — that rule catches a key already held, not one
  pressed a moment later. The shipped default is unaffected and nothing warns anyone who picks
  `Control+Alt`.
- **WASAPI loopback from an idle output device delivers no callback at all**, not silence. The probe
  says so honestly, but at the API "capture is broken" and "the far end was quiet" look identical.
  Chunk E should decide what that means before a meeting recording does. *(Chunk E shipped without
  it; settled on 2026-08-31 as KNEG-38 — the two are named rather than separated, because they
  cannot be separated, and the third case hiding with them can be and now is.)*

### What the run left on the machine

`hotkey.dictation` was changed to `Control+Alt` to exercise the hook and **restored** — it must not
stay that way on this keyboard. `transcription.localModel` was left set. Two failed transcriptions
are real records of these attempts and were left alone. The database was copied first.

---

## 2026-08-26 (honest hotkeys) — four shortcuts that registered and did nothing

`HotkeyAction` declares five actions. `shortcuts.rs` dispatched with
`if action == HotkeyAction::Dictation`, so the other four registered a real OS shortcut, claimed
the chord from every other application on the machine, emitted an event nothing acted on, and fell
off the end of an `if`. A user could bind chat agent to `Control+Shift+Space`, press it, and get
silence — with the settings screen showing a tidy accelerator and no problem beside it.

Same class as `electronPresent` and `recording_indicator_visible`: a confident answer from the half
of the path that worked.

### The second half of the same defect, found while fixing the first

The keyboard hook that landed the day before had it too. Its callback emitted `HOTKEY_EVENT` and
returned. Nothing called `dictation::on_hotkey`, and the frontend's `onHotkey` only writes a "last
fired" line — so `Control+Super`, the *default* dictation hotkey, announced itself in the interface
and dictated nothing.

The probe that verified the hook is honest about what it checked: installed, fired exactly twice
for two injected presses, uninstalled cleanly. All true. It verified that the hook *fires*, and
"the hotkey works" is a different claim. Worth remembering the next time a probe is offered as
evidence — ask which link it exercised, not whether it passed.

### Judgement calls

#### A fifth `Registration` state, rather than reusing `NotRegistrable`

The plan said to report an unhandled action as `NotRegistrable` with a plain reason: no new type,
no new rendering, and the settings screen starts telling the truth through a channel it already
reads. Tempting, and rejected.

`NotRegistrable` means *the accelerator* cannot be taken — modifier-only, or malformed — and the
user fixes it by typing a different one. `Control+Shift+Space` is faultless. Filing it under
"not registrable" would blame the accelerator for the absence of a feature, and this file already
has an entry about a message that blamed a user's spelling for our own missing alias. The reasons
matter more than the fact: they need different words because they need different actions, and only
one of them is the user's to take.

So `Unhandled { accelerator, reason }`, and `changeLabel` says "Saved. The chat agent is not built
yet." rather than "Saved, but not working" — which would send somebody hunting for a typo in a
correctly spelled shortcut.

#### Not registered at all, rather than registered and ignored

The active harm was not the silence; it was the theft. A global shortcut for an action nothing
handles takes `Control+Shift+Space` away from every other application in exchange for nothing.
Four of them. The chord is now left alone, and the reason string says so, because "we deliberately
did not claim these keys" is a thing a user might want to know.

#### The dispatch table is data, and that was forced by a crash

The design is one function both paths read, so the register path cannot offer an accelerator the
fire path will not handle. First attempt returned the handler itself —
`Handler::Rust(fn(&AppHandle))` — on the reasoning that a function pointer makes the coupling
structural: making an action live means putting a function in the table, and both paths follow from
that one edit.

It made the whole test binary unloadable. Every test in the desktop crate died at
`STATUS_ENTRYPOINT_NOT_FOUND` (0xc0000139) *before any of them ran* — not a failure, a process that
could not start. Bisected to the three new tests that called the lookup, and confirmed with a
one-line probe:

```rust
#[test]
fn probe() { let _f: fn(&AppHandle) = crate::dictation::on_hotkey; }
```

That alone reproduces it. Naming the handler as a value keeps it, and the Tauri runtime it reaches,
alive in the test binary — and a `cargo test` executable has no WebView2 beside it. Calling it from
inside a function no test reaches is fine, because the linker drops the whole path.

**This is a general constraint on testing in this workspace, not a quirk of one file:** a unit test
may exercise a decision about Tauri-dependent code, and may not hold a reference to the code
itself. So `Dispatch` is a plain enum naming its handler, `unhandled_reason` is data and testable,
and the pointer appears at the two call sites only. The compiler still enforces the coupling —
adding a variant breaks both `match`es — which is the guarantee that was wanted, checked at each
site instead of carried in a value.

#### The startup warning loop stays quiet about it

`register_all` warns per accelerator when an action has no working hotkey, with the reason, because
"another application owns it" and "this is two modifiers and no key" need different fixes. Four
unhandled actions would have added four warnings to every launch, all restating what the plan says.
They log at `debug` instead. A warning that fires every time on a designed state is how a log stops
being read.

The condition is read from the dispatch table rather than inferred from the results, so it cannot
disagree with what was actually done.

#### `report_for` split out of the `hotkeys` command

The command takes Tauri state, which is why the interesting part of it had no test. Splitting the
report construction into a plain function made the two new command-level tests possible. `unusable`
is now the union — no live accelerator, for any reason — and `unhandled` is the subset with a
reason, because a caller asking "is this action dead?" should get one answer and a caller rendering
it needs the words.

### Also corrected

`Accelerator::ModifierOnly`'s doc comment still said the hook "this application does not yet have
on any platform". That stopped being true when the hook landed. Same defect as the frontend string
that asserted a platform fact — a doc comment can rot into a lie as easily as a label.

Added `Accelerator::text()` while I was there: three call sites were each re-deriving the
accelerator text per variant, one of them through a match arm that was unreachable.

### Left undone, deliberately

The four actions still have no features. This chunk makes the interface honest about that; it does
not build them. Their home is phase 10 in [COMPLETION_PLAN.md](COMPLETION_PLAN.md), and
`nothing_handles_an_action_that_is_not_dictation` is a tripwire: the day one of them gains a
handler, that test fails and asks for the plan and `PLATFORM_SUPPORT.md` to be updated in the same
commit. Leaving the docs describing a state that has ended is how four dead hotkeys survived this
long.

826 tests, clippy, fmt, tsc, vite build and all contract gates green.

---

## 2026-08-26 (chunks B and C) — the backends behind the screens

Chunk A gave every working backend a screen. B and C are the other direction: the backends that
were declared and refused, made real. 26 to 45 reachable.

### Three refusals kept, and why none of them is laziness

The temptation in a chunk like this is to make everything green. Three things are still refused,
and each refusal is the correct answer rather than an unfinished one.

**Tinfoil.** Its whole proposition is an attested enclave — the client verifies an attestation
document against a hardware root of trust before sending anything. An ordinary HTTPS POST to the
same host transcribes perfectly well and provides *none* of that, while appearing in the UI as
"Tinfoil". `route.rs` already refused a Tinfoil host configured as a plain custom endpoint for
exactly this reason; writing the client as an unattested POST would have defeated that refusal
from the inside. So `ClientNotImplemented` stays, and it now carries the reason instead of the
generic "not implemented yet. Choose a provider that uses the standard API" — advice written when
the answer was the same for all four, and wrong now that three work.

**sherpa-onnx.** The plan said "same sidecar pattern". It is not: 276 of the 303 release assets
are `.tar.bz2`, which `install::extract` cannot open; the binaries are a one-shot CLI and a
WebSocket server rather than an HTTP endpoint, so neither `Transport::Loopback` nor `HttpHealth`
applies; and the per-model file layout differs between the Parakeet and Nemotron families. Adding
bz2 support on its own was considered and rejected — an archive format with no consumer is the
scaffolding this workspace avoids.

**Enterprise live calls.** Bedrock's signing is verified against AWS's published vectors, which is
a real guarantee about the *signature* and says nothing about whether Bedrock accepts the request.
`PLATFORM_SUPPORT.md` now has a per-provider table keeping those apart.

### The defect chunk C found

`route.rs` defaulted Corti's environment to `us` and its tenant to `base`. Those look like
placeholders and are not: they compose a live endpoint under a tenancy nobody chose, so an
unconfigured Corti would have sent the user's audio *and their credential* there. That is the
silent-substitution class the brief rules out by name, sitting inside the function written to
prevent it.

Worse, three route tests swept every registry provider and asserted each resolved — and they
passed **because of** those defaults. A test that exercises a default while claiming to test the
logic is a test that protects the default. They now configure the one provider that needs
configuring.

### Judgement calls worth keeping

#### `Dispatch` and `ReasoningDispatch` are asked, never listed

Both screens have to say which providers can actually be used. The obvious implementation is a
list of ids in TypeScript. Both instead call the real resolver — `route::resolve` then
`clients::shape`, and `resolve_chat` — and report what it says. Within one session that decision
paid: three providers moved from "needs a client" to "ready" and nothing in either screen changed.

#### Reading the port is what records a use

The llama server holds several gigabytes and stops itself after five idle minutes. A `touch()`
call at every use site is a call somebody forgets, so `LocalEndpoint::port()` — the only way to
find out where to send a local request — is itself the record. Status screens read
`running_port()`, which deliberately does not, because a settings window left open would otherwise
hold the memory indefinitely.

#### `tools/sidecar-pin` had to exist before `llama` could be added

`config/sidecars.json` has instructed people to use it since the first sidecar, and it did not
exist. Its first act was reproducing all three committed whisper pins exactly — which validates
the tool and the pins in the same run.

#### The generated types earned their keep three times over

`EnterpriseConfig` had `rename_all` without `rename_all_fields`, so it would have crossed the
boundary as `access_key_id` while everything else sends camelCase. `local_model` and `local_llm`
both defined an `InstallProgress`, which in one TypeScript file is a redeclaration where the later
one silently wins — the generator now refuses a name collision by assertion. And
`ModelStatus::verified_by` was a `&'static str` the frontend re-declared by hand as a union.

---

## 2026-08-25 (chunk A2/A3) — the shell, and somewhere to type a key

### What this closed

`Settings > Providers` did not exist, and it was the single thing holding back the most work:
`provider_credential_set` had been in Rust since phase 1 and `setProviderCredential` had been
exported from `ipc.ts` for as long, and **nothing called either**. Reachable integrations went
from 8 to 26 — fifteen credential slots and three transcription providers — with two new commands
and no new subsystem.

`App.tsx` went from 1037 lines holding every screen to a shell: a sidebar, a view switch, and an
error boundary. Six views, one file each, at the paths `config/ui-surfaces.json` already named.

### Judgement calls

#### Two commands, where the plan said none

[BUILD_PLAN.md](BUILD_PLAN.md) predicted chunk A would need no backend. It needed two, and both
came from the same discovery in two places — *a screen cannot be honest about a thing the backend
will not tell it*.

- **`credential_slots`.** Fifteen `provider_credential_present` calls on every read and after
  every save, or one call. One call, and the ordering and the labels come from the registry rather
  than from the screen.
- **`dictation_set_cloud_provider`.** Storing a key does not make a provider reachable if nothing
  can select it. `cloud_transcription_provider` and `cloud_transcription_model` are written
  together, never separately: changing the provider while leaving the previous provider's model
  behind resolves to a model the new provider does not offer, and `resolve_model` refuses —
  correctly, for a reason the user never chose.

#### `Dispatch` is derived from the router, not from a list

Four of the seven transcription providers resolve to `Transport::ProviderClient`, which the
executor refuses. The screen has to say so, and the obvious way was
`["tinfoil", "mistral", "xai", "corti"]` in TypeScript. That is a second copy of the match in
`route.rs:430`, and the two would part company the first time a client was written.

Instead `commands::dispatch_for` builds a `RouteSettings` naming the provider and asks
`route::resolve` what it would do. Three answers: `ready`, `needs-client`, `unresolvable`. A test
asserts the three-and-four split, so a provider that silently starts resolving is a test failure —
which is what a fallback to OpenAI would look like from here.

#### A stored key is reported three separate ways

`configured`, `dispatch`, and `connected` are three different facts and the screen never collapses
them. A key is stored; a transport exists; a real request succeeded. Only the third is "connected",
and only `probe_providers` can establish it.

The custom-endpoint slots get a fourth answer again: the key is stored and it configures nothing,
because the base URL lives with the feature that uses it and that screen is chunk B. The page says
that rather than looking finished — five slots that look like five more providers would be exactly
the false completeness this project keeps finding upstream.

#### The view switch is a `switch`

No router. The URL fragment is already load-bearing — `#overlay` selects the overlay window out of
the same bundle — and there is nothing to link to from outside a settings window. What a router
would have bought is history, and the one piece of history worth having, "reopen where I was", is
actively wrong when where you were is the screen that just failed.

#### An error boundary, because a blank window is the alternative

A thrown render in a Tauri webview is a blank window with no tab to reload and no console in a
bundled build. The boundary shows the message and offers a way to a screen that was working.

### A self-test that stopped testing anything

`verify-parity.test.mjs` had a case that stripped `data-surface="notes.export"` from `App.tsx` and
asserted the gate noticed. Splitting `App.tsx` moved that marker to `views/Notes.tsx`, so the
mutation stopped mutating anything and the test went green while proving nothing — **the exact
failure the file's own header warns about**, one level down. It now reads the file and the marker
out of the surface registry, so it cannot go stale the same way twice.

Worth stating plainly: the gate itself was correct throughout. It was its own self-test that
rotted, and only the fact that self-tests are run at all caught it.

---

## 2026-08-25 (chunk A1) — the IPC types are generated from Rust now

### The spike, and what it answered

[BUILD_PLAN.md](BUILD_PLAN.md) made A1 a gate rather than a formality: **13 files carry
`rename_all_fields = "camelCase"` on internally tagged enums**, and if no generator could express
that shape, generating the IPC types would have meant writing an emitter by hand. The instruction
was to find out rather than guess.

`ts-rs` 12.0.1 expresses it. Derived on `overlay::CaptureExclusion` — the hardest shape in the
tree, internally tagged with kebab-case variants, camelCase struct-variant fields and a nested
`Evidence` — it emitted

```ts
export type CaptureExclusion =
  | { "state": "active", verifiedBy: Evidence }
  | { "state": "unsupported", reason: string }
  ...
```

which is exactly the hand-written union it replaces. `specta` and the custom-emitter fallback were
not needed. A test now asserts that specific property directly, because it is the one that would
silently regress: `ipc_types::tests::a_tagged_enums_struct_fields_are_camel_case`.

### Judgement calls

#### One writer, not `#[ts(export)]`

`ts-rs` can export each type on its own during `cargo test`, and the obvious thing would have been
to point every derive at one file. But the types live in eight crates whose test binaries run
concurrently, and they would all have been writing that file. Instead
`apps/desktop/src-tauri/src/ipc_types.rs` names every type in one list and renders the whole file
in one pass, from one test. No race, a deterministic order, and the list itself is greppable — a
type that crosses the boundary and is not in it stands out.

#### The check is a test, not a CI step

There are already three `--check` steps in the contracts job, and a fourth was the obvious shape.
A `cargo test` assertion is better: it runs on every machine and in every job that runs the test
suite, so it cannot be forgotten the way a workflow step can. `KW_WRITE_IPC_TYPES=1` turns the same
test into the generator.

#### `ts-rs` is a normal dependency, not an optional one

It could have been feature-gated so release builds never see it. That would make a type crossing
the boundary *without* a `TS` derive a silent omission rather than a compile error, which is the
defect class this whole change exists to close. Nothing calls into `ts-rs` at runtime and release
builds strip the impls, so the cost is compile time only.

#### 64-bit integers are declared `number`, not `bigint`

`ts-rs` defaults `i64`/`u64` to `bigint`. Every 64-bit integer crossing this boundary is a SQLite
rowid, a byte count or a millisecond duration; `serde_json` sends them as JSON numbers and Tauri's
`invoke` parses them with `JSON.parse`, so a `bigint` declaration would have described a value the
frontend never receives. `Config::with_large_int("number")`, and a test.

### Two real defects the generator exposed

Both were invisible while the declarations were written by hand, and both are the same shape as the
`electronPresent` defect: a type and the thing it describes, disagreeing.

| Where | What was wrong | Fix |
| --- | --- | --- |
| `storage::import::{TableReport, RelocatedCredential, ImportReport}` | Sent `absent_columns`, `skip_reason`, `fts_rows` — **snake_case**, while every other struct in the application sends camelCase. One API with two conventions is how the next person writing a frontend type gets it wrong. | `#[serde(rename_all = "camelCase")]`. No consumer existed yet; the importer has no UI. |
| `local_model::ModelStatus::verified_by` | A `&'static str` with three possible values, which the frontend re-declared by hand as `"digest" \| "size" \| "nothing"`. Nothing held the two in step, so a fourth value added in Rust would have reached a UI that did not know about it. | A real `VerifiedBy` enum, generated into the union. |

`provider_probe::ProbeOutcome` was a third, milder case: the hand-written type said
`{ outcome: "skipped" } & Partial<SkipReason>`, which admits `{ outcome: "skipped" }` with no
reason at all — something serde never sends. The generated declaration drops the `Partial`.

### What did not change

The hand-written `invoke()` wrappers in `apps/desktop/src/lib/ipc.ts`, because the defect was never
in the wrappers. That file is now command names, argument names, and two predicates that mirror a
Rust method; every payload type it exports comes from `../generated/ipc-types`.

`EXPORT_FORMATS` is the one list still written by hand — `ts-rs` declares the union but cannot emit
a runtime array. The Rust test `every_export_format_has_the_wire_spelling_the_frontend_sends`
already pins those five strings.

---

## 2026-08-25 (CI paused) — GitHub Actions triggers commented out

Requested as a temporary pause. No reason was recorded, and nothing was found wrong with the
workflow itself — the jobs are untouched and still valid — so this is a pause, not a verdict on
CI's value. Treat it as short-lived.

### Judgement calls

- **Commented out the triggers rather than the file.** The ask was to "comment out the code", and
  the literal reading — comment out all 142 lines — produces a YAML file that parses to nothing,
  which GitHub surfaces as *Invalid workflow file* in the Actions tab. That trades silent
  green for a permanent red, which is worse than either. Commenting out only `push` and
  `pull_request` leaves a valid workflow that simply never fires on its own.
- **Kept `workflow_dispatch`.** It costs nothing while the workflow is dormant and means the
  full suite can still be run by hand from the Actions tab during the pause, which is the
  difference between "paused" and "gone".
- **Did not use `gh workflow disable`.** It is the cleaner mechanism — it takes effect across
  every branch at once and leaves no diff — but it also leaves no trace in the tree. A pause that
  is invisible in the repository is a pause a future session will not know about. The comment in
  the file, and this entry, are the point.

### Carry-forward consequence

The pause partly invalidates a claim in the carry-forward above. "macOS and Linux CI ran for the
first time on the Phases 1–4 push. If either is red, it is a first-run failure, not a regression."
was written expecting the next push to resolve it. With triggers off, it stays unresolved.

`main` also keeps its own copy of the workflow, so pushes to `main` still trigger CI until this
change is merged there. Only `dev` is actually paused.

---

## 2026-08-25 (later) — Phases 1-4

Branch `dev`, commits `f750196` .. `e820ff9`. 386 tests, all gates green.

### Bugs found by writing tests, not by reading code

Worth listing separately because these were latent and none was visible by inspection.

#### The network boundary made local model servers unreachable

`RequestPurpose::UserConfiguredAiProvider` and `UserConfiguredTranscriptionProvider` both refuse
plaintext HTTP. Both self-hosted routes were passing one of them, so a local whisper server or
llama.cpp server was refused by the policy layer before anything was sent -- and the error said
"scheme not permitted", which reads as a bug in the URL rather than in the purpose.

Underneath it, two layers disagreed about what "private" means: route resolution accepted the
whole RFC 1918 range, the policy layer accepted only loopback. So an endpoint the settings screen
would accept was refused at send time.

Fixed at the root rather than patched:

- `is_private_host` moved into `kw-network-policy` and `transcription-core::url` delegates to it.
  The security layer owns the security predicate; the two cannot disagree any more.
- The plaintext exemption widened from loopback to the private range. A model server on another
  machine on the user's LAN is an ordinary setup and refusing it means the only way to use one is
  to put a certificate on it. Audio and any credential do cross the local network in the clear --
  that trade belongs to the user, for an endpoint they typed themselves. A test asserts no other
  purpose gets the exemption.
- The purpose is now derived from the *destination*, not the feature, in both executors.

#### Tightening a forbidden-domain pattern opened a hole

`\bamplitude\b` matched `crates/audio-core`'s own DSP code -- peak amplitude, signal amplitude
-- which is the same false positive the Phase 0 audit found upstream, where every apparent hit
was `peakAmplitude`. The right fix is to match the vendor rather than the word.

The first attempt double-escaped the dot and silently stopped matching `api.amplitude.com`. It
still passed the scan, because the scan only proves nothing *in this tree* matches. A pattern
that matches nothing passes trivially.

So there is now a scanner self-test asserting the shipped patterns still catch real vendor
strings and still ignore ordinary technical English. `\bsentry\b` was tightened the same way
after the new test caught it matching "a sentry value".

The general lesson: for a denylist, "the tree is clean" and "the patterns work" are different
claims and only the second one matters.

#### A frontend type claimed a field Rust never sent

`Diagnostics` in `ipc.ts` declared `electronPresent`, so `App.tsx` rendered "not present" from
`undefined`. It looked like verification and verified nothing. Removed rather than implemented:
the absence of Electron is a build-time property provable from the dependency tree, not something
to probe at runtime.

#### The importer was silently losing rows

Per-row inserts through a 4-connection pool hit `SQLITE_BUSY_SNAPSHOT` (code 517) in WAL mode,
which `busy_timeout` does not help with because the snapshot has to be restarted rather than
waited on. It showed up as a flaky test and a table copying zero rows. Fixed by running the whole
import in one transaction on one connection, which also makes the import atomic -- it was not
before, despite the module documentation claiming it was reversible.

The report also counted skipped rows without saying why. A count with no reason is not something
a user can act on, so `TableReport` now carries `skip_reason`.

### Judgement calls

#### Corrected the plan on Windows capture exclusion

The Phase 0 audit recorded that `SetWindowDisplayAffinity` has "no documented getter" and
concluded Windows would have to report `unknown`. That is wrong -- `GetWindowDisplayAffinity`
exists and has since Windows 7. Windows now reports `active` with real read-back evidence.

Kept honest about what the read-back proves: the flag is set on the window, not that any given
screen recorder honours it. No API reports the latter from inside the process, and the UI says
exactly that rather than showing a bare green check.

#### Reported upstream's default dictation hotkey as broken rather than registering it

`Control+Super` is modifier-only and no cross-platform shortcut API can register it; upstream
uses per-key native listener processes. Registering it and failing silently would leave the user
pressing a dead key, so `classify` returns `ModifierOnly` with an explanation and a suggested
fix, and the dashboard shows it.

Added a test asserting the shortcut plugin *also* refuses it. If the plugin ever gains support,
that test fails and says to revisit the classifier -- rather than leaving a stale limitation
documented forever.

#### Renamed upstream's `proxied` transport to `ProviderClient`

Nothing is proxied through a vendor; it means the provider needs its own client rather than the
generic OpenAI-compatible POST. Given the brief forbids hidden remote fallbacks, a transport that
*reads* as a proxy was worth renaming.

#### Did not port upstream's strip-and-retry for rejected chat parameters

Upstream retries with shaped parameters removed when a provider rejects them by name, logging
loudly because "a fallback that fires on every request is a dialect bug to fix, not a rescue".
That is a sound safety net for a data table that might be wrong. With the registry as a single
source of truth and a test walking every model, the first move is to keep the table right. If a
provider turns out to need it, it belongs there with a test naming the provider.

#### Stopped meeting-core at the contract

The capability model, trait, and derived allowlist are complete; the five WebSocket clients are
not written. Each needs real credentials against a live service to verify, which cannot be done
from this machine, and a client that compiles without having ever connected is worth very little.
`is_allowed` returns true only for a provider with a client, so the gap is in the type rather
than only in a document.

### Deviations from the approved plan

- **Two unplanned crates.** `provider-probe` (the capability spans transcription and reasoning,
  so it fits in neither, and putting it in `provider-registry` would give a pure data crate a
  network dependency) and nothing else -- `platform` and `storage` were planned.
- **`verify-byok.mjs` became a feature, not a CI test.** It is a live network probe needing real
  credentials, so it cannot run in CI. Ported as `provider-probe`, which is what makes "never
  show a provider as connected without a real connection test" checkable.
- **Autostart uses `tauri-plugin-autostart`** rather than hand-rolled registry code. Less unsafe,
  maintained upstream.
- **The MDM policy layer is not ported.** Upstream's `resolveTranscriptionRoute` takes a
  fail-closed policy floor for managed installations. Not in the brief, and no substitute is
  pretended at.
- **`import_openwhispr` has no UI**, so it is deliberately absent from the integration manifest.
  Adding it there before it is reachable would be exactly the unconfigurable-backend the parity
  gate exists to catch.

### Open decisions

- **whisper.cpp source.** Went with upstream `ggml-org/whisper.cpp` at a pinned tag rather than
  the vendor org, keeping upstream's reason for pinning (a bump must not change transcription
  output between releases). One line of config to reverse if a fork is preferred. **Still needs
  the sidecar download and lifecycle work before Phase 2 is usable end to end.**
- **AEC helper + five Windows native helpers** still come from `OpenWhispr/openwhispr` release
  assets. Source is in-repo and MIT, so this is a CI build-and-publish job. Needed before
  meetings work.
- **`nircmd`** is still unpinned and unhashed. Leaning towards dropping it: it is only the
  third-tier Windows paste fallback, and `SendInput` now works.

---

## 2026-08-25 — Phase 0 + Phase 1 foundation

Branch `dev`, commits `f1a2f55` … `7c4f32f`.

### Judgement calls

#### Did not allowlist `crates/network-policy` from the domain scanner

The first real scan flagged `crates/network-policy/src/lib.rs`, because the runtime blocklist has
to name the hosts it blocks and the tests have to assert those hosts are blocked.

The obvious fix was a path allowlist entry. **Rejected:** that would blind the scanner across the
entire file that owns all outbound network access — the worst possible place to lose coverage.
A future leak added anywhere in that file would then be invisible.

Instead:

- Added line-level suppression to the scanner: `domain-scanner:allow <reason>` and
  `domain-scanner:allow-next-line <reason>`. A reason is mandatory; the bare marker does nothing.
  The exemption stays visible to anyone reading the code.
- Moved the vendor-host negative tests to `crates/network-policy/tests/blocklist.rs`, which uses
  only the public API and is covered by the `crates/*/tests/**` allowlist entry.

`lib.rs` now needs exactly two inline suppressions (the parse-failure fallback constant, and one
assertion about a specific host being in the blocklist) and is otherwise fully scanned.

Side benefit: moving the tests made them better. They now assert every blocked host is blocked
for **all seven** egress purposes — a blocklist applying to only some purposes would be bypassed
by mislabelling a request — and that lookalike domains such as `notopenwhispr.com` and
`openwhispr.com.example.org` are *not* caught by a naive suffix match.

#### Diverged from upstream on fail-closed provider dispatch

The brief requires failing closed on ambiguous dispatch. Upstream does the opposite in three
places:

| Upstream | Behaviour |
| --- | --- |
| `transcriptionRoute.ts:290-307` | terminal fallback returns `provider:"openai"`, `keyRef:"openai"` for any unrecognised id |
| `constants.ts:59` | `buildApiUrl` second-order default to `https://api.openai.com/v1` |
| `chatRouting.js` | anything outside `CLOUD_CHAT_PROVIDERS` is silently classified `local` |

Followed the brief, not upstream. Unknown provider ids now error.
`resolve_transcription_model` also rejects an empty string as a custom model id, which upstream's
`allowCustomModelId: true` path would have accepted.

This is a deliberate behavioural difference from the port source, so it is called out in
`docs/PORTING_MATRIX.md` and each of the three has a test.

Worth remembering *why* upstream's version was dangerous: a typo in a provider id sent the user's
audio to a provider they never chose, using a credential they had configured for something else.

### Deviations from the approved plan

- **Parity gate landed in commit 2, not commit 7.** A manifest without a checker is just a
  claim, and shipping the claim five commits before the check felt wrong.
- **Created 4 crates, not 13.** The brief asks for vertical slices over broad unfinished
  scaffolding, so the remaining crates are listed as comments in the workspace manifest and get
  created by the commit that gives them content.
- **Added an unplanned `.gitattributes` commit** (`8065e71`). `core.autocrlf=true` on this
  machine meant a fresh clone would check the generated baseline out as CRLF, while the generator
  emits LF — so `--check` would have failed on a clean tree for no real reason. Verified by
  deleting the file and re-checking it out.

### Corrections made along the way

- Told the user the baseline-vs-`main` delta was nil, based on a GitHub compare page that had
  actually failed to render. Real position: `858e824` is **not in the local clone at all**; the
  fork point is `4d1a6eaf` (2026-08-20) and the pinned baseline is ~3 commits ahead of it. None
  of the three touch providers or the registry, so parity against `c4000db9` is equivalent.
- First Electron check was wrong — it looked for *any* `electron` process on the machine and
  found three, which turned out to be the upstream OpenWhispr checkout running. Re-verified
  properly via the dependency tree, `node_modules`, and the binary format.
- `docs/ARCHITECTURE.md` initially claimed a build script emits both Rust structs and TS types.
  That is not what was built (Rust uses `include_str!` + serde; only TS is generated). Corrected
  the doc rather than leaving it overstating.

### Open decisions

- **whisper.cpp source.** Upstream pins `OpenWhispr/whisper.cpp` tag `0.0.8` — a vendor org, now
  blocked by the scanner. Options: upstream `ggml-org/whisper.cpp` at a pinned tag, or a fork
  under `axelalbertkarlsson/`. Upstream's *reason* for pinning is sound (a bump must not change
  transcription output between releases), so keep the pinning and change only the org.
  Needed before Phase 2.
- **AEC helper + five Windows native helpers** currently come from
  `OpenWhispr/openwhispr` release assets. Source is in-repo and MIT, so this is a CI build-and-
  publish job rather than a rewrite. Needed before Phase 4.
- **`nircmd`** is fetched from `nirsoft.net` with no version pin and no hash. It is only the
  third-tier Windows paste fallback behind `windows-fast-paste.exe`; leaning toward dropping it
  rather than pinning it.
