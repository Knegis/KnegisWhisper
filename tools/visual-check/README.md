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
tool measures seven shell claims (KNEG-363):

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
directory. False checks do not make a completed measurement run fail: they are
evidence for a person to judge. A failure to launch Edge, reach Vite, or
evaluate the page exits nonzero.

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
