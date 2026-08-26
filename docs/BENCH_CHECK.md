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
deletions.

**And the application says this is fine.** `Permissions::is_as_expected` returns `true` for
`InheritedFromProfile`, and its own doc comment defines that as "as restrictive as the platform
allows" — which Windows plainly is not, since an explicit owner-only DACL is available and unused.
`Integrations > Local API` warns only when `discoveryPermissionsAsExpected === false`, so it stays
silent, under a sentence reading *"readable by your user account and by administrators, which is
what your profile directory allows"*. Two trustees on this machine are neither. That sentence names
an ACL nothing ever read.

So this is not a gap that was honestly reported. It is the `contentProtected` pattern again: a
control that is not applied, described by a string that assumes what it would have done. Three
places state it and none of them checked — the code comment, the user-facing sentence, and
`SECURITY.md`.

The fix is an explicit owner-only DACL through `SetNamedSecurityInfoW` with
`PROTECTED_DACL_SECURITY_INFORMATION` so the profile's ACL stops being inherited, then reading the
DACL back and reporting `OwnerOnly` only if what came back is what was asked for — the same
read-back discipline the Unix branch of this very function already applies. Until that lands,
`is_as_expected` should return `false` on Windows and the screen should say so.

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

### What this run changed on the machine

- `transcription.localModel` was set to `tiny`, which is exactly what pressing **Use for dictation**
  writes. Left in place: it is what makes dictation work at the next launch, and it is what let the
  resume path be checked at all.
- `hotkey.dictation` was `Control+l`, changed to `Control+Alt` to exercise the hook, and **restored
  to `Control+l`**. It must not be left as `Control+Alt` on this keyboard — see AltGr above.
- Two failed transcriptions (ids 18 and 19) are real records of these attempts and were left alone.
- The database was copied before any of it.

---

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
