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
tool measures all eight KNEG-161 rail claims: pinned geometry on scroll,
disclosure glyph states and layout, current-child marking, content width using
the live rem basis, Notes grid tracks, sideways navigation, and keyboard focus.

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

This WebView2 remote-debugging route is **believed from Microsoft documentation,
not yet measured on this machine**. If the loopback port does not answer, the
tool repeats the environment-variable launch command rather than treating an
empty target list as absence of application windows. Its output directory also
contains append-only-in-practice `report.json` and `report.md` records for the
chosen command sequence.
