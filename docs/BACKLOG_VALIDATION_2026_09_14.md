# Backlog implementation and validation ? 2026-09-14

Starting revision: `9fb260b` on `dev`. Remote `dev` was verified at
`baa4c65907ce5c381058eff13806a0c486b5276e`; the existing unpushed
`9fb260b` commit also contains the earlier KNEG-226/253 fixes. Changes are local; no new push,
release publication, or hosted CI run has been performed in this implementation.

| Issue | Local change | Evidence and remaining gate |
| --- | --- | --- |
| KNEG-256 | Refresh live capability/window inventories; keep historical fixtures; regress missing popup coverage and disabled titlebar. | 15 capability tests pass. All repository Contracts commands pass. Hosted Contracts pending. |
| KNEG-255 | Use the mock webview's actual origin for IPC tests; assert an unrelated remote origin is still denied. | Windows Tauri IPC suite: 389 passed, 6 intentionally ignored. Linux hosted IPC execution pending. Production capabilities were not loosened. |
| KNEG-254 | Drain at most 16 KiB of callback headers under the existing two-second courtesy-response deadline before closing. Preserve parsed OAuth results if draining or writing fails. | 93 calendar unit tests and 4 integration tests pass on Windows, including fragmented/incomplete/oversized/stalled headers. macOS CI repeats the complete TCP response test 100 times. Native macOS result pending. |
| KNEG-42 | Pinned Intel/Apple Silicon server build, archive/provenance/licenses, architecture/dependency checks, fresh-extraction speech smoke test. | Workflow validator and Python syntax pass. Full manual CI calls the reusable workflow at the same revision. Native artifacts, measured pins, approval to publish, and installation validation remain pending. Catalog macOS entries remain unavailable. |
| KNEG-227 | Opt-in Linux portal setting, off by default and applied after restart; lifetime/reconfiguration/shutdown handling; grant/refusal feedback; press/release routing. | Platform library passes Linux cross-target Clippy. Fake-portal regression passes on a private WSL D-Bus session. Full Tauri Linux build and a real compositor are still unverified. |
| KNEG-96 | Bounded isolated-profile observation and recovery diagnostics; no speculative runtime fix. | Detailed observation below. The unexplained-exit cause remains open. |

## Shared local checks

- Windows workspace: **1,833 passed, 0 failed, 14 intentionally ignored**.
- Windows workspace Clippy with `--all-targets -- -D warnings` and
  `cargo build --profile ci --workspace` passed.
- Frontend TypeScript/production build passed; **13 frontend tests passed**.
- Generated IPC types, all 18 repository contract commands, baseline integrity,
  and the actual development-bundle scan passed.
- `cargo fmt --all --check`, `git diff --check`, and actionlint 1.7.12 passed.
- Linux portal library and extracted private-bus test cross-compile; the actual
  test executes under WSL using the existing native GCC/linker. This isolated
  harness avoids the desktop/audio SDK dependencies and is not a full Linux
  application build. Hosted CI runs the test in the real workspace under
  `dbus-run-session`.

The fake portal deliberately emits request responses before returning methods.
It checks partial grants, changed granted triggers, press/release delivery,
user cancellation/refusal without fallback, session closure, portal service
loss, and unavailable-service fallback. Real GNOME/KDE consent UI, focus,
activation and shortcut persistence still require native desktop evidence.

## KNEG-96 observation

The run uses only disposable profile `target/kneg96-1789418529743/profile`,
with copied installed Whisper binaries and base/KB-tiny models. It does not
read or write the owner's database. Application logs and timestamped observations
remain under that ignored target directory. The native preview executable
predates these changes; the Whisper runtime is unchanged by this work.
Its SHA-256 is `3ae371769e4d8ee9f41523e74085d1f78e79ad7eb3295b108067ca3bf59ffc14`.

- KB tiny starts and resumes after an application restart.
- Main observation: 20:42:33?21:07:57 UTC, about 25 minutes. No spontaneous
  exit was observed. Four deliberate base/KB-tiny switches all succeeded.
- At 21:03:20 UTC the repository speech fixture returned nonempty transcription
  from KB tiny in 424 ms through the server HTTP endpoint. This checks the
  server, not editor insertion or microphone capture.
- The controlled-kill harness initially searched for `.json` PID records;
  actual records use `.pid`. It safely refused to kill an unidentified process
  and stopped the first run. A second short attempt also stopped at that guard.
  The first diagnosis of a canonical-path mismatch was premature. Neither
  harness failure is evidence of a Whisper exit.
- Corrected completion phase: 21:11:26?21:16:35 UTC. Together with the main
  segment, this is just over 30 minutes of observation, split by harness failures;
  it is not one uninterrupted 30-minute run.
- At 21:12:33 UTC, controlled termination of the verified owned child PID 18696
  succeeded. The watcher recorded exit code -1, a 75-second lifetime and complete
  bounded diagnostics. The UI status cleared the dead port and explained recovery.
- Explicit restart succeeded at 21:12:42 UTC; subsequent status checks stayed
  healthy through completion. The harness stopped its server and preview afterward.
- The opt-in native concurrent-consumer recovery regression **passed** in the
  disposable profile: stale child PID 19148 was replaced by PID 19736, both
  consumers shared the same replacement, and fixture transcription succeeded.
  This exercises the consumer recovery path separately from the explicit-start
  check. The test stopped its replacement process afterward.

No clean observation can establish the cause of the historical spontaneous
exit. Deliberate termination validates detection/recovery only. KNEG-96 stays open.

## Delivery gates

Approve pushing the reviewed `dev` commits and dispatching the full **CI**
workflow for that exact revision. Inspect Contracts, frontend, Windows, macOS,
Linux, and both macOS Whisper artifact results before closing their CI issues.
Artifact publication is a separate approval; no release credentials or guessed
catalog digests are introduced. KNEG-227 stays unverified on real desktops even
if headless CI passes.

Linear status updates succeeded. Automatic approval review rejected the detailed
implementation comment as repository-information egress. This report preserves
that evidence locally until approval to post the summary to the six Linear issues.
