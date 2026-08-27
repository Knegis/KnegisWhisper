# Bench check — what has been exercised on real hardware

Tests say the code does what it was written to do. A bench check says whether the thing works. The
two disagree often enough that this file exists: the check on 2026-08-25 found **five real defects
that 787 passing tests had all missed**, and the one on 2026-08-26 found three more.

Maintained in place. When something here becomes false, edit it.

## The rule this file follows

**A probe verifies the link it exercised and nothing further.** The keyboard-hook probe on
2026-08-25 reported *installed, fired exactly twice for two injected presses, uninstalled cleanly* —
all true, and it was offered as evidence that modifier-only hotkeys worked. They did not. The hook
fired and the event reached nobody, because the callback emitted and returned. "The hook fires" and
"the hotkey works" are different claims and only the first had been checked.

So each row below says which link it exercised, and the claims nobody has checked are listed as
unchecked rather than left implied.

---

## 2026-08-26 — chunks A–D, as far as one machine can go unattended

Windows 11 Pro 26200, AMD, `Speakers (Realtek(R) Audio)`, `Microphone Array (AMD Audio Device)`.
Debug build via `npm run tauri:dev`, `KNEGISWHISPER_LOG=info,knegiswhisper_lib=debug`.

### Verified

| What | Evidence |
| --- | --- |
| **A modifier-only chord starts and stops a real dictation** | `Control+Alt` injected through `SendInput`; `starting microphone capture device="Microphone Array (AMD Audio Device)" sample_rate=48000 channels=4` **66 ms** after the chord. Four presses, four alternations, two transcription attempts recorded. This is the link the hook probe had not reached |
| **Only the handled action is offered to the platform** | `keyboard hook installed accelerators=["Control+Alt"]` and `hotkeys registered live=1 total=5`. Before the fix this said `live=5`: four of them registered and dispatched nowhere |
| **The four unhandled actions are reported once, quietly** | Four `DEBUG nothing handles this action, so its hotkey was not registered` lines, one per action, and no warnings. A designed state should not shout on every launch |
| **The local model server is resumed after a restart** | `sidecar ready sidecar=whisper port=8178 pid=7788` → `resumed the local model server port=8178 model=tiny`. Commit `9f0f29d` written 2026-08-25, run for the first time here |
| **The resumed server actually transcribes** | A SAPI-synthesised WAV posted to `127.0.0.1:8178/inference` came back `{"text":" The quick brown fox jumps over the lazy dog.\n"}` |
| **Capture exclusion and non-activation, read back from the real window** | `capture_exclusion=Active { verified_by: ReadBack { api: "GetWindowDisplayAffinity", value: "WDA_EXCLUDEFROMCAPTURE" } } non_activating=Active { verified_by: "GetWindowLongPtrW(GWL_EXSTYLE) & WS_EX_NOACTIVATE" }` |
| **Both listeners are loopback-only** | `Get-NetTCPConnection`: `127.0.0.1:8201` (local API) and `127.0.0.1:8178` (whisper). `curl http://172.23.16.1:8201/v1/health` from the host's own LAN address is refused |
| **The local API refuses an unauthenticated caller, including on `/v1/health`** | 401 with no token, 401 with a well-formed wrong token. Nothing leaks a version string to an unauthenticated caller |
| **The CLI talks to the running application** | `knegiswhisper health` → `KnegisWhisper 0.1.0 is running (local API v1)`; `history list` and `notes list` both answer |
| **System audio captures real playback** | 192 000 samples, 2 ch at 48 kHz, 2.00 s, peak 1.0, mixed with a microphone track as 2 sources |
| **The keyboard hook fires once per press** | `keyhook_probe`: two injected `RightControl+RightShift` presses, two fires |
| **The real OS credential store works here** | New `credential_probe`: absent → written → `contains` agrees → read back identical → deleted → absent → a missing credential reports `NotFound` and not `NoBackend`. Every unit test uses `MemoryCredentialStore`, so `OsCredentialStore` — the only one the application uses — had never run under any check |

### Findings

**1. Sided modifiers are built and unconfigurable.** `crates/platform/src/keys.rs` handles
`RightControl+Super`, `PLATFORM_SUPPORT.md` said "Built with the same hook", and no accelerator a
user can type reaches it. `hotkey::MODIFIERS` has no sided spellings, so `classify` sees
`RightControl` as *the key*:

```
     RightControl+Super -> Standard { accelerator: "RightControl+Super" }
RightControl+RightShift -> Invalid { reason: "it names 2 non-modifier keys; a shortcut may have one" }
```

The first is handed to the system shortcut register, which is the one mechanism that cannot
distinguish sides. The second is refused with a message blaming the user's spelling for a missing
alias — the same shape as the `Ctrl` defect already recorded in `ENGINEERING_LOG.md`. Same class as
the four dead hotkeys: a capability that exists, is documented, and has no way in.

**2. The local API token file gets no permissions of its own on Windows, and the comment saying
what it inherits is wrong on a real machine.** `discovery::restrict_file` is a no-op under
`cfg(not(unix))` and returns `Permissions::InheritedFromProfile`, with a comment reading "the file
inherits the profile directory's ACL, which normally grants this user and the administrators
group". On this machine `icacls` says otherwise — every entry is inherited (`(I)`) and two of them
are neither the user nor administrators:

```
Jobbdator\CodexSandboxUsers:(I)(M,DC)
S-1-5-21-…-3839493994:(I)(M,DC)
NT AUTHORITY\SYSTEM:(I)(F)
BUILTIN\Administrators:(I)(F)
AzureAD\AxelKarlsson:(I)(F)
```

A member of that group can read the bearer token and drive the local API — notes, transcriptions,
deletions. That fact stands: Windows applies no DACL of its own, so the file is only ever as
protected as the user profile directory happens to be, and this measurement is real evidence that
"happens to be" can include trustees nobody chose.

**Corrected 2026-08-27 — the rest of this finding described the application hiding that fact from
the user, and that has since been fixed.** It used to read, present tense, that
`Permissions::is_as_expected` returned `true` for `InheritedFromProfile` and that
`Integrations > Local API` therefore warned only when `discoveryPermissionsAsExpected === false` and
otherwise stayed silent under a sentence naming trustees nothing had read — accurate when this
finding was written. Task 14 fixed it: the settings screen no longer carries
`discoveryPermissionsAsExpected` at all, and the desktop layer's own
`DiscoveryPermissions::is_confirmed()` — deliberately narrower than
`kw_local_api::Permissions::is_as_expected()`, which still returns `true` for `InheritedFromProfile`
and is no longer read by anything user-facing — treats only `OwnerOnly` as confirmed. Both the
startup warning (`LocalApiRuntime::start`) and the two screens that render this state
(`Integrations > Local API`, `Integrations > CLI`) go through that one decision, and
`InheritedFromProfile` now renders "Not independently restricted" with a caveat naming
administrators by name, on both.

What is still open, and untouched by that fix: the DACL itself. The fix for *that* is an explicit
owner-only DACL through `SetNamedSecurityInfoW` with `PROTECTED_DACL_SECURITY_INFORMATION` so the
profile's ACL stops being inherited, then reading the DACL back and reporting `OwnerOnly` only if
what came back is what was asked for — the same read-back discipline the Unix branch of this very
function already applies. `discovery::restrict_file` is still a no-op under `cfg(not(unix))` today;
only the *reporting* of the gap was fixed, not the gap.

**3. `knegiswhisper history list` hides failures.** The table renders `id, when, provider, text`.
A failed dictation has neither a provider nor text, so it prints as two dashes and says nothing —
while `/v1/transcriptions/list` returns `"status": "failed"` and a precise `errorMessage`, which
the dashboard does show. The information exists and the presentation layer discards it:

```
id  when                 provider  text
19  2026-08-26 09:51:11  -         -
```

Phase 6 is marked verified, and this is in it.

### Observed, worth knowing, not defects

- **`Control+Alt` is AltGr.** On a Nordic, German or other AltGr layout, Windows generates
  left-Control + right-Alt for `@ $ € { [`, and the chord matches `EitherSide` on both. Injecting
  exactly that started a dictation. The chord fires on the *second modifier down*, so the
  documented rule that "an ordinary key held disqualifies every modifier-only chord" cannot save
  it — that rule catches a key already held, not one pressed afterwards. The shipped default
  (`Control+Super`) is unaffected. Anyone binding `Control+Alt` on such a layout should be warned,
  and nothing warns them.
- **WASAPI loopback delivers nothing at all from an idle output device** — not silence, no
  callback. With nothing playing the probe reports `captured 0 samples … the callback never fired`,
  which is honest, but for meetings it makes "capture is broken" and "the far end was quiet"
  indistinguishable at the API. Worth handling explicitly in chunk E rather than discovering it in
  a recording.
- The speech gate refused synthesised speech played through the speakers
  (`no speech detected; nothing was sent reason=InsufficientSpeech duration_ms=4850`). The AMD
  microphone array almost certainly cancels its own speakers, which is what echo cancellation is
  for. Not evidence about the gate either way — it is why the microphone leg below still needs a
  person.

### Added by chunk E, and unchecked

Stage 1 (a meeting recorded, transcribed and saved) and stage 2 (detection and
the notification window) have both landed and **neither has been run by a
person**. The detection half already produced one measurement without a bench
check — `meeting_detect_probe` found `ms-teams.exe` on this machine, which is why
the Teams row in [PLATFORM_SUPPORT.md](PLATFORM_SUPPORT.md) is the only one that
says "seen to match". Everything else about chunk E is green tests.

1. **Record a meeting.** Play a video, start a meeting from `Settings > Meetings`,
   speak, stop. Confirm two transcript streams with the right source on each, a
   note with the transcript and duration, and — with the speakers muted — that
   zero system-audio samples reads as *nobody else spoke* rather than a failure.
2. **A meeting longer than thirty minutes**, to prove the old in-memory cap is
   gone and the recording on disk is complete.
3. **Run `meeting_detect_probe` three times**: nothing open, the client open, and
   during a real call. Zoom's `CptHost.exe` and Webex's `webexmta.exe` are
   written and unverified, and the difference between the second and third run is
   the whole feature.
4. **Join a real Zoom or Webex call** and see the offer appear. Dismiss it, and
   confirm it does not come back while the call runs. Confirm nothing recorded.
5. **Screen-record the desktop while the notification window is up.** It must be
   absent from the recording and still visible on screen. That is the entire
   claim of the window role, and no test can make it.
6. **Look at where the window lands.** Bottom-right is computed from the monitor
   size and a margin, because Tauri does not expose the work area — so the
   taskbar is cleared by guess. The dictation overlay still sets no position at
   all, which is item 7 below.
7. **Dictate while a meeting is recording** and check the tray tooltip, the
   overlay and the notification window all say something true at once.
8. **Record a meeting through OpenAI Realtime**, with the key that exists on this
   machine. Nothing about it has touched a live service: the request shaping and
   the event grammar are fixtures, and a 401 from a malformed header would look
   exactly like a network failure until somebody tries.
9. **A meeting longer than an hour through OpenAI Realtime.** Sixty minutes is a
   hard server cap and the client rebuilds both sockets two minutes early. The
   transcript must carry `reconnects: 1` and must not have lost a sentence at the
   seam. This is the only provider where that path exists, and no fixture reaches
   it.
10. **Record a meeting through AssemblyAI Realtime**, with a real API key. The
    endpoint, the query parameters, the `Authorization` header carrying the raw
    key with no `Bearer` prefix, the `Begin`/`Turn`/`Error` message envelope, and
    above all whether a real `Turn` frame carries a `speaker` field at all, are
    all read from public documentation and unconfirmed against a live socket --
    see docs/PLATFORM_SUPPORT.md. The capability table's `speaker_labels: true`
    for this provider rests on the last one.
11. **Record a meeting through Deepgram Realtime**, with a real API key. Same
    shape of gap as AssemblyAI: the endpoint, the query parameters (including
    whether sending `detect_language=true` actually makes the server
    auto-detect, versus falling back to some undocumented default), the
    `Authorization: Token` scheme, and whether the *first* word's speaker in a
    `Results` message is a safe stand-in for the whole message, are all
    unconfirmed against a live socket.
12. **Record a meeting through Corti Realtime**, with a real tenant, environment
    and credential. This is the one call that would tell us the most: the
    OAuth2 token exchange reuses code already tested for batch Corti
    transcription, so a live call would not be its first evidence -- but the
    socket wire underneath it (the endpoint path, the message envelope, and
    most of the session-config block) was invented outright, with no vendor
    documentation anywhere to check it against. A live connection here is the
    *first* evidence of any kind for that wire, not a second opinion on a
    documented one, and the most likely single outcome is a rejected handshake
    or a response nothing in this client recognises.
13. **Start a meeting on each cloud provider and confirm that a provider-reported
    failure is visible to the person in the room, not only in a log line.** This
    predates the three clients this phase added and is shared by every cloud
    client, OpenAI Realtime included: `assemblyai_socket.rs`, `deepgram_socket.rs`,
    `corti_socket.rs` and `openai_socket.rs` all read a provider error frame,
    log it with `tracing::warn!("the provider reported an error")`, and continue
    -- none of `connect`, `send_audio` or `finish` ever returns an `Err` because
    of it. A server that accepts a malformed session-config frame, keeps the
    socket open, and simply never emits a transcript would let a meeting
    "complete successfully" with an empty transcript, with nothing to show for
    it but that one background log line nobody in the room can see. Confirm the
    UI itself surfaces this, not just the log.

    **Sharpened 2026-08-27 — the actionable half of this item.** The user-visible
    shape a swallowed provider error produces is `MeetingOutcome::NothingTranscribed`,
    the same outcome a genuinely quiet room produces, and `labels.ts` now says so
    plainly ("no speech was recognised, or the provider returned nothing") rather
    than naming one cause it cannot verify. That wording change is not the fix:
    the actual gap is that nothing upstream of the label distinguishes the two
    cases at all, for any of the four cloud clients. Closing it means one of
    `connect`, `send_audio` or `finish` returning an `Err` for a provider-reported
    error frame instead of only logging it, so a rejected handshake stops being
    indistinguishable from silence by the time it reaches this screen.

### Still unchecked, and needs a person at the machine

Nothing here needs a developer. All of it is buttons, one sentence spoken aloud, and a restart.

1. **Speak into the microphone.** Press `Control+L` (the bound dictation hotkey), say *"the quick
   brown fox jumps over the lazy dog"*, press it again. The local model is installed and resumed at
   launch, and the server transcribes correctly when handed a file — the untested leg is
   microphone → speech gate → transcription. Do it in Notepad and again in VS Code, and watch that
   the caret never leaves the target window.
2. **Check the clipboard survives.** Copy `KEEP ME` first, dictate, then paste. You should get
   `KEEP ME` back.
3. **Store a provider key** under `Settings > Providers`, press Save, and run the connection test.
   The credential store itself is verified; what is not is the screen, the write-only input, and
   `probe_providers` against a live service. This is chunk A's whole claim and no person has
   exercised it.
4. **Switch dictation cleanup on** under `Settings > Intelligence` and dictate. Chunk B's claim is
   that the tidied text comes back; nobody has read it.
5. **Install a local LLM** under `Settings > Local models`, hold a chat, and leave it idle for five
   minutes to see it stop. Sizes reach 5.9 GB — the smallest is the point of the check, not the
   largest.
6. **Press `Control+Super`** once with the default restored, and watch what the Start menu does to
   focus while a dictation is running. The hook deliberately does not consume the keystroke, which
   is the right choice and has a consequence nobody has looked at.
7. **Look at the overlay.** Position, size, and whether it says anything useful. Nobody has looked
   at it once.

### Added by Task 16 (the updater and its notification window), and unchecked

Deliberately not run by hand in this session: launching the actual dev build would leave a real
window and a running process on a machine other agents' Rust builds share right now, and this
checkout's own history already has one story about a stray `knegiswhisper-desktop.exe` from a
launched GUI locking the binary and breaking the next build (see the `[[bin]]` comment in
`apps/desktop/src-tauri/Cargo.toml`). Everything below is green tests and a read of the code, not a
person at the machine.

1. **Press "Check for updates" on `Settings > Diagnostics`.** This is the one part of the whole
   feature that is honestly reachable right now, with nothing missing: no updater public key is
   configured, so the button should report "Update checking is not securely configured" rather than
   either freezing, doing nothing visible, or claiming an update check that did not happen. Confirm
   the sentence appears and the button re-enables afterward.
2. **Generate a real keypair** (`docs/SECURITY.md`), paste the public half into
   `plugins.updater.pubkey`, **and rebuild the application before pressing the button again** —
   corrected by Task 19, which found this item unfollowable as written. `plugins.updater.pubkey` is
   baked into the binary at compile time by Tauri's `generate_context!`; editing the file and
   pressing "Check for updates" on a build that is already running, or re-launching a debug build
   without a fresh `cargo build`/`npm run tauri:dev` restart that regenerates the context, changes
   nothing the button can see. Once rebuilt with the key present, press the button again with a real
   repository that has no releases yet. `GET /repos/{owner}/{repo}/releases/latest` should now
   attempt a real request rather than short-circuiting, and — for a repository with zero releases —
   come back 404, surfaced as `CheckFailed` rather than a silent "up to date". This is the first real
   exercise of `NetworkPolicy::get_text` outside its own unit tests, which use a hand-rolled local
   server rather than GitHub's actual API.
3. **Publish one real release** (dispatching `.github/workflows/release.yml` for the first time,
   which nobody has done) and press the button again. This is the only way to learn whether
   GitHub's actual response shape matches the `tag_name`/`html_url` fields `updater.rs` assumes, and
   whether the update-available sentence and the notification window both appear together and say
   the same version.
4. **Screen-record the desktop while the update notification is up**, the same claim as item 5 for
   the meeting notification above: it must be absent from the recording and visible on the real
   screen. It reuses the identical `overlay::apply_treatments` call, so this is expected to pass, but
   nobody has looked.
5. **Dispatch the release workflow once with `TAURI_SIGNING_PRIVATE_KEY` set and once without.**
   Confirms two things nobody has checked: that `createUpdaterArtifacts: true` does not make
   `tauri build` fail outright when the key is absent and the workflow's own `--config` override is
   in effect, and that a `.sig` file actually appears next to a bundle when the key is present.

### Added by chunk F (calendars), and unchecked

Added by Task 19: this file previously had no entries at all for calendars, media import, the MCP
and CLI screens, or the Task 20 boundary fix — an omission this run is what found and closes. All
of chunk F is green tests and code review; none of it has touched real hardware or a real account.

1. **Connect a real Google account** through `Integrations > Calendar`, using a PKCE client ID
   registered by hand. Confirm the loopback redirect actually completes in a real browser, that the
   calendar list that comes back matches the account, and that the refresh token lands in the OS
   credential store rather than anywhere in SQLite — `assert_no_credential_columns` proves the
   schema cannot hold one, not that the runtime code obeys it end to end with a real token.
2. **The same, for a Microsoft 365 account.** The wire shape (`/me/calendars`, `events/delta`) is
   modelled on public documentation and has never answered a real request.
3. **Disconnect an account and reconnect a different one.** Confirms the fix for the settings-read
   failure that could leave a live, working refresh token in the credential store the app could no
   longer see or revoke — the automated test proves the code path; only a real reconnect proves the
   credential store agrees.
4. **Let a sync fail partway through** (revoke access mid-sync, if the provider's console allows
   it, or disconnect network partway) and confirm the screen reports `RefreshFailed` or a partial
   failure rather than `SyncOutcome::Completed`. The `let _ =` that used to discard this is fixed and
   tested; nobody has watched it happen on a real account.
5. **Look at `Integrations > Calendar`'s Apple row.** Confirms it reads "Unsupported" with the
   EventKit reason rather than an empty calendar list or a hidden row — the one thing about Apple
   Calendar that *is* checkable without a Mac.

### Added by chunk G (media import), and unchecked

1. **Import a local audio file and a local video file** through `Notes > Upload`. Confirms ffmpeg
   actually strips both to 16 kHz mono WAV on this machine, that the resulting note is transcribed
   by whatever provider is configured, and that a file with spaces or non-ASCII characters in its
   path does not crash the pipeline the way whisper's own binary is known to on such paths.
2. ~~**Paste a URL and import it.**~~ **Withdrawn on 2026-08-27, not performed.** URL import was
   disabled before anyone ran this check: `media_import_url` is no longer a registered command and
   `Notes > Upload` has no URL field, so there is nothing to paste into. Struck rather than deleted,
   because "nobody checked it" and "it cannot be checked" are different states and this file exists
   to tell them apart. If the entry point is ever re-opened, this item comes back — and it should
   then also confirm what the withdrawal was about, which the original wording did not reach: that
   a hostname resolving to a private address only at connect time is refused, and that a redirect
   into a private address is refused.  See `docs/NETWORK_BOUNDARY.md`'s "SSRF guards for URL
   import".
3. **Import an OpenWhispr `transcriptions.db`** through the same screen and confirm the source file
   is unmodified afterward, the report names what was copied and what was dropped per table, and no
   credential value appears anywhere in the report.
4. **Kill the application mid-import** (a local file — the URL half of this item went with item 2)
   and restart it. Confirms whether an orphaned per-import scratch directory is left behind — a
   known, disclosed gap with no startup sweep.

### Added by Task 12 (`Notes > Upload`'s file pickers), and unchecked

A code-review finding, not yet exercised on real hardware or decided on: `pick_import_file` opens
a real dialog and hands the chosen path *back to the renderer*, which passes it as a plain string
to `media_import_audio`, `media_import_video`, or `import_openwhispr` on a later click. Those three
commands read whatever path they are called with — nothing confirms it is the one the dialog
returned. See `docs/SECURITY.md`'s Webview section for the full description.

1. **Decide whether to close the gap, and how.** Two directions, neither chosen: fuse pick and
   import into one atomic command the way `note_export_to_file` fuses pick and write (in tension
   with `Notes > Upload`'s pick-then-review-then-confirm flow, which a single-click save does not
   need); or add a capability that restricts which paths these three commands will read, independent
   of whatever the renderer supplies.
2. **If closed by restricting paths, confirm the restriction with a real compromised-renderer
   simulation** — call the command directly (bypassing the picker) with a path never returned by
   `pick_import_file`, e.g. a credential file or another user's document, and confirm it is refused
   rather than read. A unit test can assert the policy; only a real call against the running
   application proves the policy is actually consulted on this path.

### Added by chunk H (MCP and CLI screens), and unchecked

1. **Drive `tools/knegiswhisper-mcp` from a real MCP client** (an editor or agent that speaks the
   protocol over stdio), not the protocol-test harness this task wrote for itself. Confirm
   `initialize` negotiates a version correctly against a client that asks for one this server does
   not support, and that at least one tool call against a real running application (list notes,
   read history) returns real data with no secret in it.
2. **Look at `Integrations > CLI` and `Integrations > MCP` side by side** and confirm the
   permissions caveat reads identically on both — the fix for the two screens drifting apart is
   tested at the unit level; nobody has looked at both screens open at once.
3. **Run `knegiswhisper` against a machine where the local API is off**, and confirm the CLI's
   error is the one the screen would predict rather than a generic connection failure.

### Added by Task 20 (the SSRF boundary fix), and unchecked

The most significant change of this whole run, verified so far only by unit tests and by re-running
the parity and gate suite — never by a person driving real traffic through it.

1. **Point a self-hosted transcription or inference endpoint at a plain-HTTP server on the local
   network** (not loopback) and confirm it still works — the exemption this fix was careful not to
   narrow. This is the one thing that would be a regression nobody wants: a legitimate self-hosted
   setup silently refused.
2. **Point the same self-hosted setting at `https://<a private LAN address>/`** and confirm it also
   works — the new capability this fix intentionally added (HTTPS to a private host, for the
   self-hosted purpose only).
3. **Configure a cloud provider's custom-endpoint field to a private or loopback address**
   (`http://127.0.0.1/`, `https://169.254.169.254/`, `https://localhost./` with the trailing dot)
   and confirm each is refused with a message naming the host, for every purpose that is not
   self-hosted. The unit tests exercise `is_private_host` directly; nobody has driven a real
   `reqwest` call at a real listening address on this specific machine to confirm the refusal
   happens before any byte leaves the process.
4. **Confirm CGNAT and `0.0.0.0/8` addresses behave as documented**: refused for a cloud provider,
   reachable in the clear for the self-hosted purpose. Low-probability in ordinary use, but this is
   the one place the fix deliberately *widened* what a purpose may reach, and a widening deserves a
   real check as much as a narrowing does.

### What this run changed on the machine

- `transcription.localModel` was set to `tiny`, which is exactly what pressing **Use for dictation**
  writes. Left in place: it is what makes dictation work at the next launch, and it is what let the
  resume path be checked at all.
- `hotkey.dictation` was `Control+l`, changed to `Control+Alt` to exercise the hook, and **restored
  to `Control+l`**. It must not be left as `Control+Alt` on this keyboard — see AltGr above.
- Two failed transcriptions (ids 18 and 19) are real records of these attempts and were left alone.
- The database was copied before any of it.

---

## 2026-08-27 — first bench run of chunks E–J, partial

Windows 11 Pro 26200, `Headphones (Bose QC45)` as the default output. Debug build via
`npm run tauri:dev`, `KNEGISWHISPER_LOG=info,knegiswhisper_lib=debug`.

**Partial by design and partial by accident.** Two of the five planned steps were performed
against a browser tab at `127.0.0.1:5183` rather than the application window, so they measured
dev fixtures and are recorded below as not run rather than as passes. See finding 2.

### Verified

| What | Evidence |
| --- | --- |
| **The application starts and reaches a usable window** | Only after finding 1 was fixed. Before that it panicked in `setup` every time |
| **`meeting-stream:local` records, transcribes and saves, end to end** | Started from `Settings > Meetings` with the local provider and system audio on; stopped with *Stop and transcribe*; **saved as note 1 (1 min)** with the recording at `…\Roaming\KnegisWhisper\meetings\20260827-140423.wav`. The first genuine end-to-end pass anywhere in chunk E, and the highest-value item on the run's list |
| **System audio resolves the real default output** | `WASAPI loopback on the default output device — Headphones (Bose QC45)`, a device that appears in no fixture |
| **Meeting detection distinguishes running from in-a-call** | Microsoft Teams reported as *"open, but not detectably in a call, so recording is not offered for it — detected as ms-teams.exe"*. The distinction the five process-based detectors exist to make, holding on real hardware |
| **The recording window is capture-excluded** | `verified via GetWindowDisplayAffinity = WDA_EXCLUDEFROMCAPTURE`, read back from the real window |

### Findings

**1. The application panicked at startup, every time, and no test could see it.**
`apps/desktop/src-tauri/src/meeting.rs:799` started the meeting-detection timer with bare
`tokio::spawn`. That runs from Tauri's `setup` hook, which is on the main thread with no Tokio
runtime entered, so it panicked and took the process down just after the two overlay windows were
created — `there is no reactor running, must be called from the context of a Tokio 1.x runtime`,
exit code 101.

It was the odd one out: `dictation.rs:885`, `local_llm.rs:324`, `local_model.rs:440` and
`reasoning.rs:593` all use `tauri::async_runtime::spawn`, which needs no entered context. Only this
one did not. Introduced in `3089442` (chunk E2), which predates the whole chunk E–J run.

**1296 tests passed and the application did not start.** No unit test could have caught it: the
panic needs a real Tauri app to happen in, and a test in this workspace may not even hold a
reference to Tauri-dependent code without dying at load (`STATUS_ENTRYPOINT_NOT_FOUND`). This is
the clearest example yet of what this file is for.

**2. Dev fixtures were invisible on screen, and got recorded as passes.**
`main.tsx` installs `devFixtures.ts` when `import.meta.env.DEV && !("__TAURI_INTERNALS__" in
window)` — that is, whenever the Vite URL is opened in an ordinary browser. The fixtures then
answer every command with invented data, including outcomes that read as passes:
`openai/voice pass — transcribed the tone clip as "one two three"` and
`deepgram/voice fail — 401 Unauthorized` are both string literals in that file, as are
`0.1.0-dev`, `webview 121.0.2277.128`, and a four-item egress-purpose list containing
`provider-catalog`, which is not one of the seven purposes at all.

Two steps of this run were written down as **verified** on that basis. Nothing on the page
contradicted them; the only notice was a `console.info` nobody reads while testing. For a project
whose brief says *do not fake integration success*, a mode that is visually indistinguishable from
reality is the defect, not the tester's mistake.

Fixed by installing a fixed red banner and a `FIXTURES — ` title prefix from inside
`installDevFixtures()` itself, so there is no arrangement of that module which mocks the IPC
without also marking the screen.

**3. `Settings > Meetings` says "ready to use" for a provider with no stored credential.** Open.
`kw_meeting_core::capabilities::availability()` asks only whether a *client* exists for the wire
shape; it never consults the credential store. So AssemblyAI Realtime and Corti Realtime both read
`ready to use` with no key stored for either, while `start` would refuse them by design — Task 1
made it refuse with a message naming `Settings > Providers`. The screen and the command disagree,
and the screen is the optimistic one. The data needed to fix it already exists
(`credential_slot()`, and the `credentialSlots` command the Providers screen uses); what is missing
is `credential_slot` on `MeetingProviderSummary` so the screen can cross-reference.

**4. The Deepgram row described a refresh this application does not perform.** It read *"its token
has to be refreshed mid-session"*, which describes an application that mints short-lived tokens.
Task 4 decided deliberately not to: the header carries the key the user stored, nothing is
scheduled, and implementing a refresh loop over an unchanging key would be ceremony that looks like
a safeguard. `token_rotation` is kept because it is true of the provider; the sentence now says
what it means here. Fixed.

### Still unchecked, and needs a person at the machine

1. **Steps 1, 2 and 3 of the run, repeated in the application window.** Everything recorded for
   them came from the browser tab. Specifically still unproven: that thirteen screens render, that
   a stored key passes a real connection test, and that dictation types at the caret.
2. **That the updater reports its honest refusal.** The real path returns
   `NoPublicKeyConfigured` → *"Update checking is not securely configured"*; the fixture said
   *"Up to date"*. Which of those appears is now the check.
3. **Whether dictation cleanup actually changed the text.** Reported as working, but the screen
   shows only the final transcript, so a cleanup that silently did nothing is indistinguishable
   from one that worked. Dictating the same sentence with cleanup off and on is the only way to
   tell from the UI as it stands.
4. **`Notes > Upload`** — step 5, not attempted.
5. **Meeting detection firing on a real call.** Teams was open but not in a call, so the
   offer-to-record window has still never appeared.

---

## 2026-08-27 — sherpa/Parakeet local ASR (chunk B4)

**Added by the whole-branch review's fix wave, retroactively.** This file describes itself as the
record of what has been exercised on real hardware, maintained in place — and until this section
was added, it had none at all for sherpa, while the sherpa probe and the ignored
`sherpa_dictation` integration test are the only hardware-verified events in the entire twenty-task
run. Nothing below is new measurement; `docs/PLATFORM_SUPPORT.md`'s "Local ASR: sherpa-onnx" section
and `docs/BUILD_PLAN.md`'s chunk B4 already recorded it, so nothing was falsely claimed — but a
reader of this file alone had no way to know any of it happened.

### Verified

| What | Evidence |
| --- | --- |
| **The pinned `sherpa-onnx-offline` CLI installs and runs on Windows** | `cargo run -p kw-transcription-core --example sherpa_probe` (Task 18) downloaded the real pinned CLI archive (24,378,225 bytes) and both pinned Parakeet model archives, extracted them, and drove a real transcription with each |
| **The extracted model layout is flat and identical between the two Parakeet models** | `encoder.int8.onnx`, `decoder.int8.onnx`, `joiner.int8.onnx`, `tokens.txt` directly in the model directory, plus a `test_wavs/` sample folder |
| **Both Parakeet models transcribe correctly** | `tests/fixtures/speech-16k-mono.wav` came back "The quick brown fox jumps over the lazy dog. This is a test of local transcription." from both — identical words to whisper's pinned `tiny` model on the same fixture, differing only in mid-sentence capitalisation |
| **The production `SherpaOffline` type reproduces this, not just the probe** | `crates/transcription-core/tests/sherpa_dictation.rs`, a real `--ignored` integration test against `kw_transcription_core::sherpa::SherpaOffline` — the type dictation actually calls, not a standalone example. Task 18 ran it once after fixing the `sizeMb` bug below. The whole-branch reviewer independently re-ran it cold (`cargo test -p kw-transcription-core --test sherpa_dictation -- --ignored`): 501,350,460 bytes downloaded, the same correct transcript, exit code 0, 58 s |
| **A real bug this measurement caught** | `config/provider-registry.json`'s `sizeMb` for a sherpa model is the extracted footprint (~663 MB), not the compressed download (501,350,460 real bytes, ~478 MB). An early `sherpa_models::install` checked the *download* against `sizeMb` with the 5% tolerance `models.rs` uses for whisper and failed every install. Fixed by dropping that check and relying on `discover_transducer_files` finding the real files after extraction instead — a stronger check than a byte count, since a truncated archive fails to produce a complete file set either way |
| **Installing the CLI extracts the whole `bin/` directory, not just the one binary** | Confirmed on Windows: 23 files — `onnxruntime.dll`, `onnxruntime_providers_shared.dll`, `sherpa-onnx-offline.exe`, and 20 other `sherpa-onnx-*.exe` tools this application never calls (keyword spotting, audio tagging, denoising, TTS, the online and offline WebSocket servers, and more) |

### Still unchecked, and needs a person at the machine

1. **The two Nemotron models** (`nemotron-3.5-asr-streaming-0.6b`, `nemotron-speech-streaming-en-0.6b`).
   Both are streaming, over `sherpa-onnx-online-websocket-server`'s own bespoke protocol — confirmed
   present in the CLI sidecar's `bin/` directory, but nobody has written or tested a client for it.
   The offline-CLI measurement above says nothing about whether that protocol works; both stay
   `reachable: false` in `config/integration-manifest.json` for exactly this reason.
2. **The install footprint on Linux and macOS.** Windows was measured file-by-file (23 files,
   above). Linux and macOS ship the larger "static" `sherpa` build instead (230-385 MB against
   Windows' 24 MB, per `config/sidecars.json`'s `sherpa` entry, because `onnxruntime` is statically
   linked into each executable rather than shared from one `.dll`) — nobody has extracted either and
   counted what actually lands on disk the way Windows was.
3. **A non-ASCII model path.** `docs/PLATFORM_SUPPORT.md`'s "Known environment constraints" section
   records that a non-ASCII or space character in the user data path crashes the native whisper *and*
   parakeet binaries, worked around by relocating the model cache to `%ProgramData%`. That workaround
   predates this run and was carried forward, not exercised here against a real non-ASCII Windows
   account with a Parakeet model actually installed.

## 2026-08-25 — phases 2 and 6

Seven local transcriptions in three minutes with the network off, text landing at the caret in
Notepad and VS Code, the clipboard given back, focus never moving, and the compiled CLI reading
history out of the running application. The procedure is in
[PHASES_0_6_COMPLETION_PLAN.md](PHASES_0_6_COMPLETION_PLAN.md) and the five defects it found are in
[ENGINEERING_LOG.md](ENGINEERING_LOG.md).

One thing from that run is still unexplained: `id=8`, a connection refused to `127.0.0.1:8178`
while the application believed a server was live. The resume path makes it less likely and has not
been proven to have removed it. On 2026-08-26 that port answered `HTTP 200` after a resume, which is
evidence against the symptom recurring and not an explanation of it.
