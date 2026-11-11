# Settings return and task loading validation

Validated on Windows on 2026-11-11, in `codex/linear-triage-20261111`, based on
`dev` at `ed697ab`. This records local development checks, not a published release.

## Behavior

- KNEG-548: dismissing Settings returns to the latest non-settings destination
  on the current history branch, retaining note ID, transcript row and summary
  focus. Tile selection, programmatic closure and title-bar click-through do
  not trigger an additional return. The existing unsaved-settings guard controls
  departure; heading focus follows the loaded destination until the user acts.
- KNEG-547: Dictation and Meetings render their task layout while awaiting fresh
  backend data. Neutral detail bars appear after 150 ms; ready content appears
  immediately with a 120 ms fade. Reduced motion disables the fade. There is no
  cached readiness, fabricated recording control or change to IPC/schema.

## Automated checks

All passed:

- Frontend suite: 570 tests, including return-history regressions.
- TypeScript typecheck and production frontend build.
- Domain scan, dev-bundle audit, integration parity, settings audit and generated
  IPC types freshness check.
- `git diff --check`.
- Native debug build from this worktree, using the existing Cargo cache and
  `TAURI_CONFIG` with `build.devUrl` set to `http://127.0.0.1:5197`.

Browser regression commands, against the worktree's Vite server:

```powershell
node tools/visual-check/settings-return.mjs --url http://127.0.0.1:5197/ --out .validation/settings-return-final --port 9393
node tools/visual-check/task-loading.mjs --url http://127.0.0.1:5197/ --out .validation/task-loading --port 9396
```

The first passed 13 scenario groups: Close, Escape and backdrop from both tasks;
selected-note restoration through multiple settings screens; a direct settings
link; Stay, Discard, Save and failed saving; and title-bar click-through with Back.
The second passed 12 groups: both tasks in both themes at 720x520 and 1280x800;
errors, outstanding reads after unmount and fast revisits; a background refresh
that retains loaded content; and reduced motion. These use invented fixtures,
controlled read delays and synthetic errors. Screenshots confirm the neutral
layout and no horizontal overflow; screenshots/results are under ignored
`.validation/`.

The headless renderer initially crashed under sandbox execution. The passing
browser runs used normal desktop permissions with separate disposable profiles.
The probes were corrected to use the exact fixture note title and the exposed
upper edge of the title-bar button, rather than clicking through the sheet body.

## Real Tauri window

Built and launched this worktree's native debug application with
`KW_AUTOMATION_PROFILE` and `WEBVIEW2_USER_DATA_FOLDER` pointing to fresh directories
under `.validation/`, and WebView2 debugging on port 9395. The native log confirms
that the database was in `native-profile-final`. The main window loaded the
worktree frontend on port 5197; no development fixture banner was present.

Using real IPC, the checks passed Close/Escape/backdrop from Dictation and
Meetings, returning to an invented note created in that disposable database,
and unsaved Stay/Discard with the original stored value confirmed on reopening.
A frame probe, without modifying or delaying native IPC, saw 8 loading frames
on Meetings and 28 on Dictation before ready content. These are observations
from one empty-profile run, not performance benchmarks. Native screenshots were
inspected. Raw results are in `.validation/native-results.json`.

Native Save/failure behavior was not separately fault-injected; the browser
regression covers those paths. No recording, model inference, paid provider
request, installer acceptance or release validation is claimed. No Rust code
changed, so a full Rust test matrix was not repeated.
