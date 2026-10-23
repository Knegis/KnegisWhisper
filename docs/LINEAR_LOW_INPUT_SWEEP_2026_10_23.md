# Low-input Linear sweep — 2026-10-23

This executes the plan `docs/plans/2026-10-22-low-input-backlog-sweep.md`. That file is untracked in
the owner's checkout. The plan's planning review was blocked on four findings (X-R1 to X-R4); each
is handled where it applies and named below. The baseline is the 72 open `knegiswhisper` issues read
from Linear on 2026-10-22: 18 In Progress or In Review, and 54 Backlog. Re-read on 2026-10-23, they
were unchanged; the newest update to any of them is 2026-10-12.

**Frozen refs.**

- `dev` = `origin/dev` = `06a77af`.
- `origin/main` = `db3c1c1`, the PR #59 merge. `dev` is its strict ancestor: `27 0`.
- The sweep branch `sweep/2026-10-23-low-input` starts at `origin/main`, and its upstream is unset.
  `main`'s tree (`0b32238`) is byte-identical to the merge ref that PR CI run 38360225387 tested.
  So the Rust gates were not re-run locally on unchanged code (C-R4).

**What the owner's "implement this plan" reached, and what it did not.**

- Applied: T0, T1, T2's preparation and measurement (Q2a), the doc fixes, the native session
  (Q3a), the CI dispatch (Q4) and this commit (Q5).
- Refused: every Linear write. The Claude Code auto-mode classifier refused them as external
  writes. The exact operations are prepared as a packet, and none has been applied, so the Outcome
  column reads *pending Q1* wherever a write is involved.
- Not taken: the optional extras Q3b and Q7. The rulings Q6a to Q6d are unanswered, so each of
  those issues carries only its question.

## New findings

| | Finding | Evidence | Proposed issue |
| -- | -- | -- | -- |
| X-A | `dev`, and the release build installed on 2026-09-19, start `sherpa-onnx-online-websocket-server` without a host argument. v1.13.4 then listens on every interface. The server has no authentication, and an enabled Windows Firewall inbound **Allow** rule for the executable (Private and Public profiles) leaves it reachable. The current network is Private. The loopback guard (`5c8f6aa`, PR #31, 2026-09-05; `dc4f19f` is an unmerged twin) reached `main` only. | **Measured:** started alone with `dev`'s `build_args` on port 8269, it listened on `0.0.0.0:8269` after 10.1 s (`Get-NetTCPConnection`, 15:32 CEST), and was stopped 0.2 s later. `--help` lists `--port` and no host option (v1.13.4, Git SHA1 `14280725`). **Read from the binary:** `C:\Program Files\KnegisWhisper\knegiswhisper-desktop.exe` contains `num-work-threads` but neither `--host=127.0.0.1` nor the guard's refusal text. **Believed:** a remote client gets its own recognition stream, not the meeting's audio. | `Bug`, **Urgent**. Closes when `dev` holds the guard, the owner's next `tauri:dev` runs on it, and the installed build is replaced or removed. |
| X-B | With the guard in place, the pinned server is refused at every start, so local-streaming meetings stop working on `dev`. Meetings' readiness (`lib/readiness.ts:181-195`) does not know about the guard and still reports the provider Ready. | Read from source against the measured help text, not run. | `Improvement`, Medium. A loopback-capable server; the options are left unchosen. |
| X-C | `cpal`'s deprecation warning under clippy 1.99 and Vite's chunk-size advisory were deferred by KNEG-424. | `KNEG_422_424_VALIDATION.md`, row "Existing cpal/Vite warnings". Neither fails a gate. | `Improvement`, Low. |
| X-D | KNEG-395 built the finalisation stages and timing. It did not build its second half, opening the note before speaker analysis finishes. | `KNEG_THREE_ISSUES_VALIDATION.md`: "No percentage, ETA, early note access or background speaker-label update was added". | `Improvement` + `Idea`, Low. |
| X-E | Switching a playing note from Replay to Notes reloads its recording, so the native `notes-smoke` fails at `notes-smoke.mjs:72`, three runs of three. | **Measured over CDP in a disposable native profile:** the one `<audio>` element moves into the mini player, and `readyState` falls from 4 to 0 and `currentTime` from 0.243 to 0. Within 600 ms it plays again at 0.786, which means it resumed near its old position. A scratch copy of the smoke that waits for the resume passes all eight groups. | `Bug`, Medium. |
| X-F | `crates/local-api/tests/api.rs` `a_non_loopback_listener_is_refused` binds `0.0.0.0:0` on purpose. So every freshly built test binary raises a Windows Firewall prompt. About a hundred inbound Allow rules for `api-*.exe` test binaries have built up, most for deleted paths. | `Get-NetFirewallApplicationFilter`, 2026-10-23. | `Improvement`, Low. |

## The native session (T6, Q3a)

The session used a debug build of `main`'s tree from this worktree, with
`KW_AUTOMATION_PROFILE=target/notes-smoke/profile`, an isolated `WEBVIEW2_USER_DATA_FOLDER`, its own
Vite on 5183, and CDP on 9337. The data was synthetic: no microphone was opened, no TTS, no provider.
The owner's `tauri:dev` was not running, and all three ports were free.

- **Ownership (X-R2).** In every launch, the CDP listener was `msedgewebview2.exe`, a child of the
  launched `knegiswhisper-desktop.exe`, running on the isolated `--user-data-dir`. The app's own PID
  never listens on 9337, which is why this check uses process ancestry rather than PID equality.
- **Setup and teardown (X-R3).** The runner created `target/summary-smoke` and
  `target/notes-smoke`. Each run ended by killing only the app's process tree and its own Vite, and
  every run released 5183, 9337 and 18881.
- `summary-smoke`: **passed** (9 provider requests). A first attempt failed on an evaluation error
  whose text the runner lost, and that failure was not reproduced.
- `notes-smoke`: **failed** at line 72, three runs of three (X-E). The scratch variant passed all
  eight groups: legacy title, transcript copy, playback across tabs, forced save failure, empty
  states, title and focus, the 720px fit, and close-window persistence.
- **KNEG-332's navigation:** 10 of 10 checks at 1000×720 and 720×520. The rail is Dictation,
  Meetings, Notes and Settings. Settings lists Vocabulary, and Developer is a group of its own with
  Local API, CLI, MCP and Diagnostics. Vocabulary opens with the rail on Settings. Notes › Import
  audio or video opens with the rail on Notes. Nothing scrolls sideways.
- **A limit on "no microphone".** While it ran, the disposable instance registered its default
  dictation shortcut (Control+Super) system-wide. It was not pressed.

All of this is *measured by instrument in a disposable native profile, not seen by the owner*.

## CI (T4, Q4)

`ci.yml` was dispatched once on `main` at `db3c1c1`: run 39964300965, 2026-10-23 13:48 UTC. Its
result settles KNEG-220, 222, 255, 227, 180, 42 and 276 (T5). Until it is read, those rows say
*waiting on run 39964300965*.

## Disposition of the 72

L0 means no input. L1 means one yes. Q1 gates every Linear write.

| Issue | At freeze | Disposition · task | Operation and evidence | Did not close → where it lives | Outcome |
| -- | -- | -- | -- | -- | -- |
| KNEG-358 | In Review | take · T3 · L0+Q1 | Done. `dfc2cfe`, `1205002`; pin verification green on `main` in runs 38244291659 (2026-10-12) and 39231588631 (2026-10-19), with the two size-only notes. The docs fixes are in place: `SECURITY.md:340-351`, `PLATFORM_SUPPORT.md:1093-1095`. | The optional asset-id pin was not built, and no issue is filed for it. | pending Q1 |
| KNEG-413 | In Review | take · T3 · L0+Q1 | Done. `5ec144d`; the same two green runs; `tools/model-pin/retry.test.mjs`. | The retry never fired in hosted CI. Unit-tested, believed on the wire. | pending Q1 |
| KNEG-393 | In Review | take · T3 · L0+Q1 | Done. `148d6ca`; PR CI run 38360225387 (and earlier PR runs) green with Clippy on rustc 1.99.0, Windows. | Linux and macOS on 1.99 come with run 39964300965. `#[allow(deprecated)]` stays until `rust-version` reaches `try_update`. | pending Q1 |
| KNEG-420 | Backlog | take · T3 · L0+Q1 | Done. `307fc2d`; `KNEG_423_420_VALIDATION.md`. | Real-library feel → KNEG-378 item 41. | pending Q1 |
| KNEG-421 | Backlog | take · T3 · L0+Q1 | Done. `c682916`, `505c562`; five-fixes ledger (100 ms contention timeout, one note per purge transaction). | Accepted limit: a single slow filesystem removal can still hold its transaction. | pending Q1 |
| KNEG-422 | Backlog | take · T3 · L0+Q1 | Done. `902e511`, `802b458`; 422/424 ledger, seven-item table, migration 0012. | → KNEG-378 item 35. | pending Q1 |
| KNEG-423 | Backlog | take · T3 · L0+Q1 | Done. `5ad24ce`; `recovered_provider_and_lightweight_notes_cross_native_ipc`. | → KNEG-378 item 40. | pending Q1 |
| KNEG-424 | Backlog | take · T3 · L0+Q1 | Done after X-C is filed. `902e511`, `802b458`; 422/424 ledger, row by row. | Accepted limits stated as accepted: unnamed offer/API starts, preserved unreadable participant entries, queued-summary invalidation, a driver blocked inside open. The cpal/Vite warnings → X-C. | pending Q1 |
| KNEG-453 | Backlog | take · T3 · L0+Q1 | Done. `8045f07`; five-fixes ledger (F2 rename, one stop per turn removed). | Narrator → KNEG-378 item 47. | pending Q1 |
| KNEG-454 | Backlog | take · T3 · L0+Q1 | Done. `cbb4f45`; five-fixes ledger. | The macOS overlay scrollbar and a real-window flick → KNEG-378 item 44. | pending Q1 |
| KNEG-455 | Backlog | take · T3 · Q1+Q6b | Re-scope in place, Low, open, retitled to items 6–9. `96a0a01` landed items 1–5 (three-issues ledger). | Item 7 waits on the owner's ruling (Q6b). | pending Q1 |
| KNEG-456 | Backlog | take · T3 · L0+Q1 | Done. `a5557b7`; note-meta 4/4 in both themes (five-fixes ledger). | It does not validate Deepgram authentication. | pending Q1 |
| KNEG-395 | Backlog | take · T3 · L0+Q1 | Done after X-D is filed. `6235a0c`; three-issues ledger; native `native_meeting_progress_and_settings_ipc`. | Early note access → X-D. No percentage or ETA, by design. | pending Q1 |
| KNEG-399 | Backlog | take · T3 · L0+Q1 | Done. `87888b9`; five-fixes ledger, native summary workflow test. | Live provider and summary quality. | pending Q1 |
| KNEG-400 | Backlog | take · T3 · L0+Q1 | Done. `715b8ec`; three-issues ledger. | Forced termination and tray Quit are out of scope, as the ledger says. | pending Q1 |
| KNEG-316 | In Progress | take · T3 · L0+Q1 | Done. `ef14e38`; the two native IPC recovery tests. `KNEG_316_MEETING_RECOVERY.md` § Validation corrected on this branch. | Pressing Recover after a real crash → KNEG-378 item 34. | pending Q1 |
| KNEG-334 | In Review | take · T3 · L0+Q1 | Done, with a correction appended to its stale line "nothing has been rendered yet … branch ui-approachability". `9273ccc`, `8dc78f9`, merge `598f796` (2026-09-29). | The Ready mark's look in the real window → KNEG-378 item 32. | pending Q1 |
| KNEG-332 | Backlog | take · T3, T6 · Q1, Q3a | Done, as delivered by KNEG-337 (`c11f80d`). Native navigation checked 10/10 at normal and minimum size (above). | Title-bar search and deep links were not re-driven natively. | pending Q1 |
| KNEG-411 | Backlog | take · T3 · L0+Q1 | Add `Feature`; Done. `25d620f`, `fd025b3`; `FOLDER_SHARING_DESIGN.md` § Local validation on 2026-10-07 records the Windows sharing smoke round trip. The status line is corrected on this branch. | Hosted CI, packaged builds, macOS/Linux. | pending Q1 |
| KNEG-51 | Backlog | take · T3 · L0+Q1 | Done, as an umbrella. Its own text: "Nothing autonomous is left here". | 58, 121, 166, 60, 52, 44, 55–57 each own one remaining check. | pending Q1 |
| KNEG-300 | In Progress | take · T3 · Q1 | Move to In Review. `baaf900`; `NOTES_WORKSPACE.md` § Verification (the synthetic save evidence of 2026-09-28). | All six started-meeting acceptance checks stay listed. | pending Q1 |
| KNEG-90 | Backlog | take · T3 · Q1 | Retitle to its one remaining check, and keep it open. | The real OpenWhispr `transcriptions.db` import. | pending Q1 |
| KNEG-53 | Backlog | take · T3 · Q1+Q6d | Append the dated contradiction: KNEG-53 (2026-08-28) says a Deepgram key is stored on the bench machine, and KNEG-56 says none exists. Whether one is stored now is unknown. | Resolved by Q6d. | pending Q1 |
| KNEG-56 | Backlog | take · T3 · Q1+Q6d | The same note. | Resolved by Q6d. | pending Q1 |
| KNEG-87 | Backlog | take · T3 · Q1+Q6c | Move to No priority, reframed as declined, only on the owner's ruling. Until then, the question alone. | — | pending Q6c |
| KNEG-299 | Backlog | take · T3 · Q1+Q6a | Add `Feature`. Close or re-scope on the owner's ruling. Until then, the question alone. | — | pending Q1, Q6a |
| KNEG-276 | Backlog | take · T3, T5 · Q1, Q4 | Re-scope: the minutes statement (2026-09-20) predates the move to the Knegis org (2026-09-26), where October's usage was 84 Linux and 96 Windows minutes at $0 net. The org's allowance was not readable. Done once run 39964300965 is read. | — | waiting on run 39964300965 |
| KNEG-220 | In Review | take · T5 · Q4 | Done on a green Linux Rust job. | — | waiting on run 39964300965 |
| KNEG-222 | In Review | take · T5 · Q4 | Done on a green Linux `Test`. | — | waiting on run 39964300965 |
| KNEG-255 | In Review | take · T5 · Q4 | Done on a green Linux `Test (Tauri IPC boundary)`. | — | waiting on run 39964300965 |
| KNEG-227 | In Review | take · T5 · Q4 | Done on a green Linux Rust job including "Shortcut portal private-bus regression". This closes the compile-and-test half only. | The D-Bus wire on a real desktop → KNEG-61. | waiting on run 39964300965 |
| KNEG-180 | In Review | take · T5 · Q4 | Done on the whole matrix green. | A real release → KNEG-43. | waiting on run 39964300965 |
| KNEG-42 | In Progress | take · T5 · Q4 | Re-scope to publication: release assets need `contents: write` and the owner's decision. Stays Low, open. | Publication. | waiting on run 39964300965 |
| KNEG-378 | Backlog | take · T6 · Q3a | Append item 49's result: `summary-smoke` passed; `notes-smoke` failed at line 72 (X-E). Keep it open. | Item 49 itself closes when X-E is fixed and the repository smoke passes. | pending Q1 |
| KNEG-298 | In Review | take-optional · T6 ext · Q3b | Untouched: Q3b was not taken. | — | not taken |
| KNEG-315 | In Review | take-optional · T6 ext · Q3b | Untouched: Q3b was not taken. | — | not taken |
| KNEG-96 | Backlog | take-optional · T7 · Q7 | Untouched: reading the owner's log was not approved. | — | not taken |
| KNEG-419 | Backlog | take-optional · T7 · Q7 | Untouched: the downloads and the Swedish recording were not approved. | — | not taken |
| KNEG-52 | In Progress | take-optional · T7 · Q7 | Untouched. A disposable debug profile keeps credentials in memory only (`state.rs`, `MemoryCredentialStore`), so this needs a test key provisioned into that session, never the owner's store (X-R4). | — | not taken |
| KNEG-47 | Backlog | leave | Physical keys and the mouse. | — | — |
| KNEG-121 | Backlog | leave | A real Zoom/Webex call. | — | — |
| KNEG-125 | Backlog | leave | Two people talking. | — | — |
| KNEG-127 | Backlog | leave | A real side button, a held chord. | — | — |
| KNEG-166 | Backlog | leave | A person looking at the offer window. | — | — |
| KNEG-58 | Backlog | leave | Real Zoom, Webex and FaceTime processes. | — | — |
| KNEG-65 | Backlog | leave | A real Windows account with a non-ASCII name. | — | — |
| KNEG-82 | Backlog | leave | An OAuth client and a real calendar account. | — | — |
| KNEG-328 | Backlog | leave | App-owned OAuth client registrations. | — | — |
| KNEG-44 | In Progress | leave | A natural update notification, as the owner asked; depends on 43. | — | — |
| KNEG-43 | Backlog | leave | A keypair, repository secrets and a release decision. | — | — |
| KNEG-257 | In Review | leave | Swedish and English human speech, and a live supported provider. | — | — |
| KNEG-278 | In Review | leave | A summaries model choice, real meetings and a BYOK key. | — | — |
| KNEG-54 | Backlog | leave | Enterprise credentials and spend. | — | — |
| KNEG-55 | Backlog | leave | An AssemblyAI key and spend. | — | — |
| KNEG-57 | Backlog | leave | A Corti account and spend. | — | — |
| KNEG-392 | Backlog | leave | A loopback-enforcing streaming server (X-B) and a real 30–60 minute meeting. | — | — |
| KNEG-59 | Backlog | leave | A Mac. | — | — |
| KNEG-81 | Backlog | leave | A Mac. | — | — |
| KNEG-61 | Backlog | leave | A Linux desktop session; hosted CI is not one. | — | — |
| KNEG-329 | Backlog | leave | A product design ruling. | — | — |
| KNEG-396 | Backlog | leave | A product design ruling. | — | — |
| KNEG-397 | Backlog | leave | A product design ruling. | — | — |
| KNEG-402 | Backlog | leave | A product design ruling. | — | — |
| KNEG-369 | Backlog | leave | A product design ruling. | — | — |
| KNEG-449 | Backlog | leave | The schema, migration and edited-summary decisions in its own text. | — | — |
| KNEG-366 | Backlog | leave | A connected calendar (82) and a decision to build it. | — | — |
| KNEG-319 | Backlog | leave | Labelled recordings from the owner. | — | — |
| KNEG-380 | Backlog | leave | A labelled evaluation set and a runtime decision. | — | — |
| KNEG-381 | Backlog | leave | Waits on 380. | — | — |
| KNEG-73 | Backlog | leave | Owner prioritisation. | — | — |
| KNEG-74 | Backlog | leave | Owner prioritisation. | — | — |
| KNEG-275 | Backlog | leave | Its children; the parent closes with them. | — | — |

34 take, 5 take-optional and 33 leave: 72. X-A to X-F are additions, not part of the 72.

## Docs corrected in place on this branch

- `NETWORK_BOUNDARY.md` § Loopback is not egress: which sidecars take a host argument;
  `kw_sidecar::port` only chooses the port; the measured `0.0.0.0` bind and the guard.
- `PLATFORM_SUPPORT.md` § The streaming server: the pinned server cannot bind loopback, so the app
  refuses it, and readiness does not know.
- `KNEG_316_MEETING_RECOVERY.md` § Validation: what the native IPC tests established, and that the
  Recover button and a real crash are KNEG-378 item 34.
- `FOLDER_SHARING_DESIGN.md` status line: merged into `dev` and `main` (PR #57), validated locally.
- `NOTES_WORKSPACE.md`: both native smokes ran on 2026-10-23, with the results above.

`PLATFORM_SUPPORT.md:1078-1095` needed no edit: it already names the diarization models as
size-only, which is KNEG-358's required update.

## Ordering (X-R1)

The doc commit on this branch comes first. `dev` is then fast-forwarded to it, and only after that
are the closes applied whose notes cite the corrected docs (KNEG-316, 411, 378). The fast-forward
alone carries `5c8f6aa` into `dev`. Nothing is pushed.

Source: the plan above, its run artifacts under
`~/.claude/plan-runs/KnegisWhisper-9fbbc2460e/20261022-144821-4e0ab670`, and the measurements named in
each row.
