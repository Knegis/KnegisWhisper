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
   largest. **Note, 2026-08-31:** the smallest models were never the broken ones. Eighteen of the
   thirty-five had a declared size 6.7–7.9% off the real file and were deleted after downloading in
   full; every model that matched was one of the small `lfm2*` or Gemma QAT builds. Doing this check
   with `lfm2.5-230m-q8_0` would have proved nothing about the other eighteen — see item 7 under the
   2026-08-28 round-two list.
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

## 2026-08-28 (later) — round two: cleanup proven, Parakeet proven, two defects

Same machine and build. Four items were outstanding; all four were exercised.

### Verified

| What | Evidence |
| --- | --- |
| **A dead local server repairs itself *when the button is pressed*** | The four failing dictations were a port held for a process that had gone. After the fix, *Use for dictation* started a real server and dictation worked. The link exercised is the button, and only the button: `start_model` learnt to health-check the recorded port and nothing else did, so dictating and importing still failed against a dead port until 2026-08-31 — see KNEG-96 |
| **The chat connection test passes** | `openai/chat pass — replied "OK"`, with the same key that produced `400 Unsupported parameter: 'max_tokens'` before the probe was taught to ask `params::shape_for` |
| **Dictation cleanup genuinely changes the text** | Proven with Parakeet, below. The first attempt could not tell, and the reason is worth keeping |
| **The Parakeet selector works end to end** | Installed the CLI and `parakeet-unified-en-0.6b` through the screen, selected it, and dictated through it. The runtime had been proven by a test; the screen, the commands and the engine switch had never been used by anyone until now |
| **Parakeet transcribes better than `tiny` on this hardware** | Same sentence, four takes. `tiny`: *"the lace dome"*, *"the quick brow fox"*, *"the quick plan forks"*. Parakeet: the sentence, correctly, every time |

**How cleanup was finally settled, and why the first attempt failed.** The A/B this file
prescribed — dictate a sentence with filler words, cleanup off then on — could not work on
`whisper-tiny`, because **whisper strips fillers itself**. All four `tiny` takes came back with no
"um" and no "you know" whether cleanup was on or off, so the comparison measured nothing. The test
was designed without accounting for the transcriber, which is the same mistake as trusting a probe
that verifies less than it looks like.

Parakeet does not strip them, which turned the same experiment into a clean one:

```
spoken:   "um, so the quick brown fox, you know, jumps over the lazy dog"

Parakeet, cleanup off:  "So the quick brown fox, you know, jumps over the lazy dog."
                        "So, you know, the quick brown fox jumps over the lazy dog."
Parakeet, cleanup on:   "So the quick brown fox jumps over the lazy dog."
                        "So the quick brown fox jumps over the lazy dog."
```

The filler survives with cleanup off and is gone with it on, twice each. **Cleanup works.** The
lesson is the more useful part: an A/B is only a test of the thing you changed if nothing else in
the pipeline is doing the same job.

### Findings

**1. Install reported no progress, so a 631 MB download looked like a dead button.** Pressing
*Install* on a Parakeet model produced no visible change. The progress line existed but rendered
once, near the top of the whisper block — far above the control that started it, and off screen.
`InstallProgress::Model` already carries the model id, so the line now renders on the row whose
button was pressed, for both engines. Fixed.

**2. `keep_raw_text` was opt-in with nothing to opt in with.** Fixed 2026-08-31.
`kw_app_core::dictation` stores the pre-cleanup transcript as `Transcription.raw_text`, but only
when `keep_raw_text` is true and only when it differs from the cleaned text — which would answer
"did cleanup change anything" exactly, and permanently. It is hardcoded `false` at
`apps/desktop/src-tauri/src/dictation.rs`, under a comment calling it opt-in. There is no setting,
no command and no UI, and nothing renders `rawText`, which is already in the IPC type and in
history. Had it been reachable, this run would not have needed a second transcriber to answer the
question. Same class as the four hotkeys that registered and dispatched nowhere: a capability that
exists, is described, and has no way in.

Now reachable. Keeping the raw text moved from a `bool` fixed when `Dictation` was built to
`History::keep_raw_text`, asked of the sink per dictation — the same shape `History::enabled`
already had, and the reason a change now applies to the next thing said rather than after a
relaunch. `Settings` key `history.keepRawText`, off by default, with the toggle and the second line
on `Dictation`'s History section. The trait's default is `false`, so an implementation that does not
answer keeps one copy rather than two: a second copy of a person's speech is opted into, never out
of.

**Still unchecked:** nobody has switched it on and dictated. Turn it on, dictate a sentence with
filler words through Parakeet with cleanup on, and confirm the row shows both versions — and that a
dictation cleanup did *not* change shows only one.

**3. `history.enabled` was the same defect one row down.** Fixed 2026-08-31.
Not found by this run — found by fixing finding 2 and then reading the three controls the History
section offered. `DbHistory::enabled` is read before every row is written and defaults to on, and
there was no command, no IPC wrapper and no control anywhere: the only way to stop the application
writing down everything dictated was to edit SQLite by hand. Meanwhile the screen offered *Clear
history* and, as of finding 2, a toggle that stores **more** of it. Third instance of one pattern —
the four hotkeys that registered and dispatched nowhere, `keep_raw_text` above, this — and each time
the tests passed, because a test can set the value directly.

Surface only, and that was checked before any of it was written: `Dictation::write_history` in
`kw_app_core` gates every write on `enabled()`, `DbHistory` is the only production implementation of
`History`, and every other `transcriptions::record` call site is a test. Nothing bypasses the
switch. `history_enabled` / `history_set_enabled`, and a toggle above the keep-raw one — which is
now disabled while history is off, because with no rows being written there is nothing for a second
version to go on. Switching history off stops new rows and removes nothing; the row's detail text
says so in both states rather than leaving it to be discovered next to a button called *Clear
history*.

**Still unchecked:** nobody has switched it off and dictated. Switch it off, dictate, and confirm no
new entry appears and the entries already listed are untouched. Switch it back on and confirm the
next dictation is written without a relaunch — the sink reads the setting per row, so it should be.

### Added by the live Teams meeting, same day

**Verified:** `meeting-stream:local` produces a **live** transcript during recording, not only a
final one. Detection reported Teams as open-but-not-in-a-call *during an actual call*, which is the
honest answer and also the whole capability for that client -- Teams runs the call inside
`ms-teams.exe`, so no child process appears for the process-table detector to see. Filed as
**KNEG-94** (mic listener over `IAudioSessionManager2`), with a calendar cross-check noted as the
possibly cheaper half.

**`openai-realtime` cannot connect at all: the beta API shape was retired.** Its first live use
was closed on both sockets within milliseconds:

```
The Realtime Beta API is no longer supported. Please use /v1/realtime for the GA API.
invalid_request_error.beta_api_shape_disabled (code 4000)
```

The endpoint was already `/v1/realtime`, so the `OpenAI-Beta: realtime=v1` header was what selected
the retired shape. It is no longer sent. **Believed, not verified**: no live call has confirmed the
client works without it, only that it fails with it, and whether the GA shape also renames
`transcription_session.update` is unknown to this build. A further mismatch will now arrive as the
provider's own close reason.

Worth noting what this cost: the client was written against documentation, marked *written against
fixtures, no live call* in the manifest, and shipped correct-for-its-time. The failure was not a
coding mistake. It is what "unverified" means, and it took ten minutes of a real meeting to find.

**Removing the header was right, confirmed nine minutes later**: the socket opened and stayed
open. The same attempt named the next mismatch, which is the method working --

```
Invalid value: 'tra...ate'. Supported values are: 'session.update', 'session.close',
'input_audio_buffer.append', ...
```

`transcription_session.update` became `session.update`; the session events gained `session.created`
/ `session.updated`, and the client now accepts both spellings. `input_audio_buffer.append` is
still valid, so the audio frames were never the problem. **The session payload is still
unconfirmed** -- the server has objected only to type names so far, and guessing at a nested shape
is harder to read back from an error than a wrong name, so it was left alone.

Then `Missing required parameter: 'session.type'.` -- GA wants the session object to name itself
`transcription` even though the endpoint already carries `intent=transcription`. Added.

Then `Unknown parameter: 'session.input_audio_format'.` -- a different kind of answer from the
three before it. Those were names; this one says a field that had always sat at the top of
`session` is not known there at all, which means the audio family has moved and
`input_audio_transcription` and `turn_detection` are its siblings. All three were relocated under
`audio.input` in one step rather than one per round trip: relocating one and leaving two behind
would buy two more meetings' worth of attempts to learn what that message already implies. The
sample rate now comes from the session config, already checked against the capability table, rather
than a constant that could disagree with the audio being sent. **Inferred as a group and not
verified.**

**It works.** The sixth attempt transcribed a real meeting through `openai-realtime`, microphone
and system audio, live. The `audio.input` shape inferred as a group was right. This is the first
cloud meeting client in this project to meet a real server, and `meeting-stream:openai-realtime`
moves from *written against fixtures* to verified in the manifest and in
`docs/PLATFORM_SUPPORT.md`.

Five round trips, each about two minutes, each ending in the server naming the next thing. None of
it would have been possible before the close reason was kept.

**What this says about the other three.** AssemblyAI, Deepgram and Corti are in exactly the state
this client was in yesterday: written from documentation, covered by fixtures, marked *no live
call*. Every single one of the five faults here was a shape that had been correct when written and
had since changed or been misread — not one was a coding mistake, and not one was catchable by any
test in this repository. The fixtures still pass. They passed the whole way through.

**The honest-failure message works at Stop.** The fourth attempt ended with *"Nothing was
transcribed — no speech was recognised, or the provider returned nothing. The recording is at
…\meetings60828-135901.wav."* That is the right sentence: it does not claim success, it does
not blame the user, and it says where the audio is. The gap remains that it arrives at Stop rather
than at second zero -- **KNEG-95**.

**And the more serious defect it exposed: a meeting whose provider dies at second zero looks
exactly like one that works.** Both sockets closed immediately; the application reported
**Recording**, kept the indicator up, captured both streams for as long as it was left running, and
said nothing. One `DEBUG nothing is consuming the meeting audio` per second was the only trace.
`Incoming::Failed` is logged and never surfaced -- the cross-cutting gap `BUILD_PLAN.md` already
records for all four cloud clients -- and `pump`'s error sits in a `JoinHandle` nobody awaits until
Stop. Filed as **KNEG-95**, High, and deliberately not patched: it needs a decision about whether a
dead provider stops the recording, falls back immediately, or carries on with a fourth
`MeetingState` saying so.

### Still unchecked, and needs a person at the machine

1. **`Settings > Meetings`, re-read** — still carried over from an earlier paste. AssemblyAI and
   Corti should now name the missing key rather than reading "ready to use".
2. **Whether dictation and import now recover from a dead local server — the mechanism is
   verified, the wiring is not.** `local_dictation.rs` now stops the real server, confirms the
   health check reports it gone, starts it again on the same port, and transcribes through the
   revived one. It also probed a *busy* server 5,235 times mid-transcription with **zero** false
   negatives, which was the risk worth checking: a working server judged gone would have had a
   second one started on top of it. Both measurements are in
   [PLATFORM_SUPPORT.md](PLATFORM_SUPPORT.md#local-asr-the-health-check-and-a-busy-server), along
   with the surprise — a dead port costs the **full two-second timeout**, not a fast refusal.
   **Still needs a person:** the Tauri leg. Kill `whisper-server` from Task Manager while the app
   holds its port, then dictate, and confirm the two log lines appear and a transcript arrives. The
   remaining half of KNEG-96 — *why* the child exits unattended — is untouched by that fix.
3. **Whether a long import is transcribed in full — answered, verified end to end, and the
   diagnosis here was wrong.** The Swedish import *was* transcribed in full: 39,142 characters for
   3,401 seconds. It produced one segment at `00:00:00` because the server was started with
   `--no-timestamps` and so reported no timings at all — not because `media.rs` failed to divide
   it. Fixed by capping the segment length; the measurement is in
   [PLATFORM_SUPPORT.md](PLATFORM_SUPPORT.md#local-asr-whisper-server-timings).

   **Verified 2026-08-31** by `crates/transcription-core/tests/subtitle_export.rs`, which builds a
   78-second recording, runs it through the real server with the *shipped* `build_args`, and
   exports a real subtitle file. **20 cues from `00:00:00` to `00:01:17`**, each 3–5 seconds, text
   correct. The same test through the installed Parakeet model gives 20 cues with finer boundaries
   (`00:00:23,920` against whisper's `00:00:23,000`), non-overlapping, from per-token onsets.

   An import routed to a **cloud** provider still gets one cue, which is correct — no cloud batch
   provider on this path reports segments. **Still needs a person** only for the screen: press
   *Import* on a real file and export from the note, since nothing here has been through
   `NotesUpload`.
4. **`parakeet-tdt-0.6b-v3`** — offered on the same measured layout, still never downloaded.
5. **Whether a Parakeet import exports usable cues — verified for a long recording.** See item 3:
   `subtitle_export.rs` runs the real `sherpa-onnx-offline` against the installed
   `parakeet-unified-en-0.6b` over 78 seconds and gets 20 non-overlapping cues spanning the
   recording. Point it at a model with `KW_SHERPA_MODEL_DIR` and `KW_SHERPA_EXECUTABLE`.
   **Still uncovered:** real speech with hesitations, a recording long enough for the
   200-character backstop to fire, and any language other than English.
6. **Meeting detection firing on a real call.**
7. **Whether a previously-uninstallable local LLM now installs through the screen — nobody has
   pressed the button.** Until 2026-08-31, eighteen of the thirty-five models in
   `Settings > Local models` downloaded in full and were then deleted, because their declared size
   was a base-10 "GB" figure multiplied by 2^30 and `download_gguf` rejects anything more than 5%
   from the declared length. All 39 objects are now measured from Hugging Face and held to **exact**
   equality; the arithmetic and the evidence are in
   [ENGINEERING_LOG.md](ENGINEERING_LOG.md) under 2026-08-31.

   All 39 also carry a **SHA-256** as of 2026-08-31, so the install now hashes what it downloaded
   before renaming it into place.

   **What is verified:** the committed sizes and digests match what Hugging Face states — `cargo
   test -p knegiswhisper local_llm -- --ignored` checks all 39 of each and passed on all 39, as
   does `node tools/model-pin/pin.mjs --verify-stated` — and seven objects, one per publishing
   organisation, were downloaded in full and hashed, agreeing exactly. Unit tests prove every model
   resolves to a real size check and a real digest check rather than to "nothing declared".

   **What is not:** no GGUF has been downloaded *through the application* since either change.
   Press *Install (1.3 GB)* on `qwen2.5-1.5b-instruct-q5_k_m` — the smallest of the eighteen and
   the worst offender of them, at 7.9% — and confirm the download renames into place instead of
   being deleted, that the row then shows *Use this model*, and that pressing it starts a server
   that answers. **Two things on screen are part of the check.** The size on the button reads the
   measured figure, so *Install (1.3 GB)* where it used to say 1.4 GB is the size fix visible
   without instrumentation. And the row should read *checksum verified against the publisher's own*
   before the install and *… is installed — checksum verified against the publisher's own* after
   it, with a *Checking … against its checksum…* line in between; that phrase, rather than the
   bare *checksum verified* the whisper models get, is the digest fix visible without
   instrumentation. Nothing has rendered `VerifiedBy::StatedDigest` outside a dev fixture.

   A drafter is a second progress bar after the first, and nobody has seen that either.
   `gemma-4-e2b-it-qat-q4_0` is the cheapest way to reach it (3.3 GB plus a 59 MB drafter) — its
   size was already correct, so it exercises the drafter path and not the fix.

   **The thing to watch for is the opposite failure.** Exact equality means an upstream re-upload
   now fails at the *end* of the download rather than passing quietly. Two Gemma models were already
   2 240 bytes adrift from their old declared size, so this is not hypothetical. If an install fails
   with "the download is N bytes and this model measures exactly M", the pin is stale, not the
   connection — re-run the ignored tests and re-measure into `LOCAL_LLM_PINS`. The same is true of a
   failure naming a digest: since 2026-08-31 an upstream re-upload trips the hash as well as the
   length, and `node tools/model-pin/pin.mjs --verify-stated` tells the two apart in seconds —
   agreement there means the bytes were corrupted in transfer, disagreement means the pin is
   stale.
8. **Whether a newly digest-pinned whisper model installs through the screen — nobody has pressed
   the button on any of the five.** Until 2026-08-31 only `tiny` carried a digest; the other five
   were size-checked with a 5% tolerance. All six now carry a SHA-256 and an exact length measured
   by downloading them, so `Settings > Transcription > Local` holds every one of them to
   `kw_sidecar::integrity::verify` and the row's *Verified by* reads `digest` rather than `size`.
   See [ENGINEERING_LOG.md](ENGINEERING_LOG.md) under 2026-08-31 (whisper pins).

   **What is verified:** all six digests were reproduced from a real download by
   `tools/model-pin/pin.mjs`, and `tiny`'s round trip — download, digest match, server start, real
   transcription — is exercised end to end by `cargo test -p kw-transcription-core --test
   local_dictation -- --ignored`. **What is not:** the other five have never been installed by any
   route, through the screen or otherwise. `base` is the *recommended* model and the one a new user
   will press first.

   Press *Install* on `base` (142 MB) and confirm three things: the progress bar reaches
   `Verifying`, the row afterwards says the check was a **digest** and not a size, and the model
   then loads and transcribes. `small` (466 MB) is the cheapest second data point. `large` is 2.9 GB
   and proves nothing the others do not.

   **The thing to watch for is the same opposite failure as item 7, and it is sharper here.** These
   six are fetched from HuggingFace `resolve/main`, a branch pointer, and the check is now a digest
   rather than a 5% window — so *any* upstream re-upload, of any size, stops the install dead with
   an integrity error. `node tools/model-pin/pin.mjs --verify` re-measures all eight model pins
   (about 8 GB) and is what should be run before believing the download is at fault; the weekly
   `pin-verify.yml` job runs it unattended. See KNEG-103, and KNEG-105 for this item.

---

## 2026-08-28 — the bench run repeated in the application window

Windows 11 Pro 26200, WebView2 151.0.4129.107. Debug build via `npm run tauri:dev`,
`KNEGISWHISPER_LOG=info,knegiswhisper_lib=debug`.

**This one was real.** Every fixture tell from the day before came back correct: version `0.1.0`
rather than `0.1.0-dev`, the seven egress purposes under their real names rather than four
including `provider-catalog`, and the updater's honest refusal rather than "Up to date". The
banner added after that run did its job by being absent.

### Verified

| What | Evidence |
| --- | --- |
| **The application boots and all thirteen screens render** | Diagnostics reports the real machine: Windows Credential Manager, models under the per-user Roaming path with no fallback needed, `WASAPI loopback — Speakers (Realtek(R) Audio)`, `WH_KEYBOARD_LL hook on a dedicated thread` |
| **Capture exclusion and non-activation, read back from the real window** | `GetWindowDisplayAffinity = WDA_EXCLUDEFROMCAPTURE`; `GetWindowLongPtrW(GWL_EXSTYLE) & WS_EX_NOACTIVATE`. The screen says plainly that this confirms the flag, not that a given recorder honours it |
| **The updater refuses honestly rather than claiming to be current** | *"Update checking is not securely configured — no updater public key is set, so a downloaded update's authenticity could never be checked."* The path Task 16 wrote and nobody had pressed |
| **The egress list is the enum, not a copy of it** | All seven purposes, under the names `RequestPurpose` uses |
| **A stored OpenAI key reaches OpenAI** | `openai/catalog pass — 130 models live; all 3 pinned ids present`. Chunk A's keystone claim, unchecked since it was written, is now checked |
| **Audio import transcribes end to end** | A Swedish radio recording imported through `Notes > Upload` and came back with a transcript and a note; a second import ("Hello") also landed. Chunk G's first genuine pass |

### Findings

**1. The connection test reported a working provider as broken.**
`openai/chat` failed with
`400 Unsupported parameter: 'max_tokens' is not supported with this model. Use
'max_completion_tokens' instead.`

The provider was fine. `provider-probe::chat_probe_body` hardcoded `"max_tokens": 16`, under a
comment claiming that avoided parameters a provider might reject — it did the opposite.
`reasoning-core::params::shape_for` already exists precisely for this disagreement, reads
`token_param` per model from the registry, and its own module doc says it is *the* place that turns
registry facts into parameters. The probe was the one call site that went around it, and the
registry says OpenAI's first model, `gpt-5.6-sol`, takes `max_completion_tokens`.

Worth naming the shape of this: it is the project's usual failure mode inverted. Every other claim
defect found so far was a screen saying something worked when it did not. This one said a healthy
integration was broken. Both are false, and a false negative here would have sent someone hunting a
credential problem that did not exist. Fixed, with a test that walks every reasoning provider in the
registry and asserts the probe body carries that provider's parameter and not the other one.

**2. The test suite only passed on a machine where nobody had used the application.** Installing
ffmpeg through `Notes > Upload` — step 5 of this very run — broke two tests in `media.rs` that
asserted "nothing was downloaded in this test" while reading the real sidecar directory. They were
tests about the machine, not about the code. `MediaImportRuntime::rooted` now takes the sidecar
directory as a parameter, for the same reason `binary_locator::locate` takes its search inputs as
parameters, and the tests point it at an empty temporary directory.

**3. Dictation cleanup is still not verified.** Reported as "Works", but the A/B was not run and the
screen cannot show the difference: it displays the final transcript only, so a cleanup that did
nothing is indistinguishable from one that worked. Finding 1 makes this worth repeating rather than
assuming — until the probe was fixed there was no way to tell whether the reasoning path was
reachable at all.

### Still unchecked, and needs a person at the machine

1. **The cleanup A/B.** Dictate one sentence with a filler word, cleanup off; dictate it again with
   cleanup on; compare. Nothing else distinguishes the two states from the UI as it stands.
2. **`Test connections` re-run after finding 1.** The chat probe should now pass with the same key.
3. **Whether a long import is transcribed in full.** The Swedish import produced a single segment
   at `00:00:00`. If the source was a full radio programme rather than a clip, that is a truncation
   defect and not a transcription-quality one — `media.rs` hands the whole WAV to the executor in
   one call and chunks nothing.
4. **The Parakeet selector.** Built on 2026-08-27, never opened. Install a model, confirm the button
   turns blue, dictate through it, and confirm the whisper row above then says dictation is going
   elsewhere.
5. **`Settings > Meetings`, re-read.** The screen text quoted in this run's notes was copied from
   the day before: it still shows AssemblyAI and Corti as "ready to use" and Deepgram as needing a
   mid-session refresh, both of which were corrected in `be22bd0` after that text was captured.
6. **Meeting detection firing on a real call.** Teams was open but not in a call again.

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

**3. `Settings > Meetings` said "ready to use" for a provider with no stored credential.** Fixed.
`kw_meeting_core::capabilities::availability()` asks only whether a *client* exists for the wire
shape; it never consults the credential store. So AssemblyAI Realtime and Corti Realtime both read
`ready to use` with no key stored for either, while `start` would refuse them by design — Task 1
made it refuse with a message naming `Settings > Providers`. The screen and the command disagree,
and the screen is the optimistic one. The data needed to fix it already exists
(`credential_slot()`, and the `credentialSlots` command the Providers screen uses); what is missing
was for anything to ask the question.

`ClientAvailability` gained a fourth state, `NeedsCredential { slot }`, and
`listing_with_credentials` narrows `Available` to it for any provider whose slot the caller says is
empty. The caller is the `meeting_providers` command, which asks `CredentialStore::contains` -- the
same question `start` asks, now asked before the provider is offered rather than after the click.
`contains`, never `get`: knowing a key is there does not require reading it. The listing stays pure
and the rule is testable without a credential store, which is how three new tests cover it,
including one asserting no slot is looked up for a provider that could not be offered anyway.

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
5. **The Parakeet selector, added 2026-08-27 after this run.** `Settings > Transcription > Local`
   can now install the sherpa-onnx CLI and a Parakeet model and select it for dictation. Nothing
   about the *screen* has been exercised: install one model, confirm the button turns blue, dictate
   through it, and confirm the whisper row then says dictation is going elsewhere. Only
   `parakeet-unified-en-0.6b` has ever been run at all, by `tests/sherpa_dictation.rs`;
   `parakeet-tdt-0.6b-v3` is offered on the same measured layout and has never been downloaded.
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
